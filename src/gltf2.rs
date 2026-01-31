use crate::io::*;
use crate::*;
use rayon::prelude::*;

struct MeshInstance<'a> {
    mesh: gltf::Mesh<'a>,
    transform: Mat4,
}

fn collect_instances<'a>(
    node: gltf::Node<'a>,
    parent_transform: Mat4,
    instances: &mut Vec<MeshInstance<'a>>,
) {
    let local_matrix = Mat4::from_cols_array_2d(&node.transform().matrix());

    let global_transform = parent_transform * local_matrix;

    if let Some(mesh) = node.mesh() {
        instances.push(MeshInstance {
            mesh,
            transform: global_transform,
        });
    }

    for child in node.children() {
        collect_instances(child, global_transform, instances);
    }
}

#[profiling::function]
fn convert_image(data: &gltf::image::Data) -> Result<image::RgbaImage> {
    use bytemuck::{Pod, PodCastError};
    use gltf::image::Format;
    use image::Pixel;
    use image::buffer::ConvertBuffer;
    use image::{ImageBuffer, Luma, LumaA, Rgb, Rgba, RgbaImage};
    use std::borrow::Cow;

    /// Given `data`, converts the image (assumed to have
    /// the pixel format `P`) into an Rgba8 image
    fn convert_image<P: Pixel>(data: &gltf::image::Data) -> Result<RgbaImage>
    where
        P::Subpixel: Pod,
        for<'a> ImageBuffer<P, Cow<'a, [P::Subpixel]>>: ConvertBuffer<RgbaImage>,
    {
        fn convert_bytes<T: Pod>(bytes: &[u8]) -> Result<Cow<'_, [T]>> {
            match bytemuck::try_cast_slice::<u8, T>(bytes) {
                Ok(slice) => Ok(Cow::Borrowed(slice)),
                Err(PodCastError::AlignmentMismatch) => {
                    Ok(Cow::Owned(bytemuck::pod_collect_to_vec(bytes)))
                }
                Err(e) => Err(anyhow::anyhow!(e)),
            }
        }

        let pixels = convert_bytes(&data.pixels).context("cannot convert texture contents")?;

        ImageBuffer::from_raw(data.width, data.height, pixels)
            .context("image has invalid dimensions")
            .map(|img| img.convert())
    }

    match data.format {
        Format::R32G32B32FLOAT => convert_image::<Rgb<f32>>(data),
        Format::R32G32B32A32FLOAT => convert_image::<Rgba<f32>>(data),

        Format::R16 => convert_image::<Luma<u16>>(data),
        Format::R16G16 => convert_image::<LumaA<u16>>(data),
        Format::R16G16B16 => convert_image::<Rgb<u16>>(data),
        Format::R16G16B16A16 => convert_image::<Rgba<u16>>(data),

        Format::R8 => convert_image::<Luma<u8>>(data),
        Format::R8G8 => convert_image::<LumaA<u8>>(data),
        Format::R8G8B8 => convert_image::<Rgb<u8>>(data),
        Format::R8G8B8A8 => RgbaImage::from_raw(data.width, data.height, data.pixels.clone())
            .context("image has invalid dimensions"),
    }
}

#[profiling::function]
fn parse_image(
    image_data: &[gltf::image::Data],
    texture: gltf::Texture,
) -> Result<image::RgbaImage> {
    let image_index = texture.source().index();

    let image = image_data
        .get(image_index)
        .context("failed to fetch image data (index is out of bounds)")?;

    convert_image(image).context("failed to convert image")
}

#[profiling::function]
fn parse_material(mat: &gltf::Material, image_data: &[gltf::image::Data]) -> Result<ImageOrColor> {
    if let Some(image) = mat
        .pbr_metallic_roughness()
        .base_color_texture()
        .map(|texture_info| texture_info.texture())
    {
        return parse_image(&image_data, image)
            .context("failed to parse the color image used by the material")
            .map(ImageOrColor::Image);
    }

    if let Some(image) = mat
        .emissive_texture()
        .map(|texture_info| texture_info.texture())
    {
        return parse_image(&image_data, image)
            .context("failed to parse the emissive image used by the material")
            .map(ImageOrColor::Image);
    }

    if let Some(image) = mat
        .pbr_specular_glossiness()
        .and_then(|spectral| spectral.diffuse_texture())
        .map(|texture_info| texture_info.texture())
    {
        return parse_image(&image_data, image)
            .context("failed to parse the color image of the spectral material")
            .map(ImageOrColor::Image);
    }

    let base_color = mat.pbr_metallic_roughness().base_color_factor();

    let base_color = image::Rgba([
        (base_color[0] * 255.0) as u8,
        (base_color[1] * 255.0) as u8,
        (base_color[2] * 255.0) as u8,
        (base_color[3] * 255.0) as u8,
    ]);

    Ok(ImageOrColor::Color(base_color))
}

#[profiling::function]
fn parse_mesh(
    mesh: &gltf::Mesh,
    transform: Mat4,
    bounds: &mut BoundingBox,
    materials: &[ImageOrColor],
    buffers: &[gltf::buffer::Data],
    triangles: &mut Vec<[Vec3; 3]>,
    extras: &mut Vec<[VertexExtras; 3]>,
) -> Result<()> {
    #[inline]
    fn get_extras(idx: usize, uvs: Option<&[Vec2]>, material_idx: u32) -> VertexExtras {
        let uv = uvs.as_ref().and_then(|uvs| uvs.get(idx)).copied();

        VertexExtras::new(uv, material_idx)
    }

    for primitive in mesh.primitives() {
        let mode = primitive.mode();

        if mode != gltf::mesh::Mode::Triangles {
            bail!("a mesh in the file uses non-triangle geometry");
        }

        let material_idx = primitive.material().index().unwrap_or(materials.len());

        let data = primitive.reader(|buffer| Some(&buffers[buffer.index()]));

        let mut indices = data
            .read_indices()
            .context("a mesh in the file has no vertex indices")?
            .into_u32();

        let vert_coords = data
            .read_positions()
            .context("a mesh in the file has no vertex positions")?
            .map(|v| transform.transform_point3(Vec3::from(v)))
            .collect::<Vec<_>>();

        for &v in &vert_coords {
            bounds.extend(v);
        }

        let uvs = data
            .read_tex_coords(0)
            .map(|uvs| uvs.into_f32().map(Vec2::from).collect::<Vec<_>>());

        loop {
            let i1 = indices.next();
            let i2 = indices.next();
            let i3 = indices.next();

            if i1.is_none() {
                break;
            }

            let (Some(i1), Some(i2), Some(i3)) = (i1, i2, i3) else {
                eprintln!("found a non-full triangle ({i1:?}, {i2:?}, {i3:?})");
                break;
            };

            triangles.push([
                vert_coords[i1 as usize],
                vert_coords[i2 as usize],
                vert_coords[i3 as usize],
            ]);

            extras.push([
                get_extras(i1 as usize, uvs.as_deref(), material_idx as u32),
                get_extras(i2 as usize, uvs.as_deref(), material_idx as u32),
                get_extras(i3 as usize, uvs.as_deref(), material_idx as u32),
            ]);
        }
    }

    Ok(())
}

#[profiling::function]
pub fn load_gltf(path: &str, scale: f32) -> Result<Mesh> {
    let (document, buffers, images) = {
        profiling::scope!("gltf::import");
        gltf::import(path).context("failed to load the gltf file")
    }?;

    let mut materials = document
        .materials()
        .collect::<Vec<_>>()
        .par_iter()
        .map(|material| parse_material(&material, &images))
        .collect::<Result<Vec<_>, _>>()
        .context("failed to parse materials")?;

    // i.e. default material
    materials.push(ImageOrColor::Color(image::Rgba([255, 255, 255, 255])));

    let mut instances = Vec::new();
    for scene in document.scenes() {
        for node in scene.nodes() {
            collect_instances(node, Mat4::from_scale(Vec3::splat(scale)), &mut instances);
        }
    }

    let total_triangles: usize = document
        .meshes()
        .flat_map(|m| m.primitives())
        .filter(|p| p.mode() == gltf::mesh::Mode::Triangles)
        .map(|p| {
            p.indices()
                .map(|accessor| accessor.count() / 3)
                .unwrap_or(0)
        })
        .sum();

    let mut triangles = Vec::with_capacity(total_triangles);
    let mut triangle_extras = Vec::with_capacity(total_triangles);
    let mut bounds = BoundingBox::zero();

    for instance in instances {
        if let Err(e) = parse_mesh(
            &instance.mesh,
            instance.transform,
            &mut bounds,
            &materials,
            &buffers,
            &mut triangles,
            &mut triangle_extras,
        ) {
            eprintln!("failed to parse mesh: {e}")
        };
    }

    Ok(Mesh {
        materials,
        triangles,
        triangle_extras,
        bounds,
    })
}
