use crate::io::*;
use crate::*;
use image::ImageBuffer;
use image::Rgb;
use image::Rgba;
use image::buffer::ConvertBuffer;
use rayon::prelude::*;

#[profiling::function]
fn convert_image(data: &gltf::image::Data) -> Result<image::RgbaImage> {
    match data.format {
        gltf::image::Format::R32G32B32FLOAT => {
            let pixels: &[f32] = bytemuck::cast_slice(&data.pixels);

            ImageBuffer::<Rgb<f32>, _>::from_raw(data.width, data.height, pixels.to_vec())
                .context("image has invalid dimensions")
                .map(|img| img.convert())
        }

        gltf::image::Format::R16G16B16 => {
            let pixels: &[u16] = bytemuck::cast_slice(&data.pixels);

            ImageBuffer::<Rgb<u16>, _>::from_raw(data.width, data.height, pixels.to_vec())
                .context("image has invalid dimensions")
                .map(|img| img.convert())
        }

        gltf::image::Format::R8G8B8 => {
            let pixels = data.pixels.clone();

            ImageBuffer::<Rgb<u8>, _>::from_raw(data.width, data.height, pixels)
                .context("image has invalid dimensions")
                .map(|img| img.convert())
        }

        gltf::image::Format::R32G32B32A32FLOAT => {
            let pixels: &[f32] = bytemuck::cast_slice(&data.pixels);

            ImageBuffer::<Rgba<f32>, _>::from_raw(data.width, data.height, pixels.to_vec())
                .context("image has invalid dimensions")
                .map(|img| img.convert())
        }

        gltf::image::Format::R16G16B16A16 => {
            let pixels: &[u16] = bytemuck::cast_slice(&data.pixels);

            ImageBuffer::<Rgba<u16>, _>::from_raw(data.width, data.height, pixels.to_vec())
                .context("image has invalid dimensions")
                .map(|img| img.convert())
        }

        gltf::image::Format::R8G8B8A8 => {
            let pixels = data.pixels.clone();

            ImageBuffer::<Rgba<u8>, _>::from_raw(data.width, data.height, pixels)
                .context("image has invalid dimensions")
        }

        _ => bail!("format {:?} is unsupported", data.format),
    }
}

#[profiling::function]
fn parse_image(
    image_data: &[gltf::image::Data],
    texture: gltf::Texture,
    source_dir: &str,
) -> Result<image::RgbaImage> {
    let source = texture.source().source();

    match source {
        gltf::image::Source::Uri { uri, .. } => {
            let path = format!("{source_dir}/{uri}");

            image::open(path.as_str())
                .with_context(|| format!("failed to fetch file `{path}` used by the mesh"))
                .map(|img| img.into_rgba8())
        }

        gltf::image::Source::View { .. } => {
            let image = image_data
                .get(texture.source().index())
                .context("failed to fetch image data (index is out of bounds)")?;

            convert_image(image).context("failed to convert image")
        }
    }
}

#[profiling::function]
fn parse_material(
    mat: &gltf::Material,
    image_data: &[gltf::image::Data],
    source_dir: &str,
) -> Result<ImageOrColor> {
    if let Some(image) = mat
        .pbr_metallic_roughness()
        .base_color_texture()
        .map(|texture_info| texture_info.texture())
    {
        return parse_image(&image_data, image, source_dir)
            .context("failed to parse the color image used by the material")
            .map(ImageOrColor::Image);
    }

    if let Some(image) = mat
        .emissive_texture()
        .map(|texture_info| texture_info.texture())
    {
        return parse_image(&image_data, image, source_dir)
            .context("failed to parse the emissive image used by the material")
            .map(ImageOrColor::Image);
    }

    if let Some(image) = mat
        .pbr_specular_glossiness()
        .and_then(|spectral| spectral.diffuse_texture())
        .map(|texture_info| texture_info.texture())
    {
        return parse_image(&image_data, image, source_dir)
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

        let bound = primitive.bounding_box();
        bounds.extend(bound.min.into());
        bounds.extend(bound.max.into());

        let material_idx = primitive.material().index().unwrap_or(materials.len());

        let data = primitive.reader(|buffer| Some(&buffers[buffer.index()]));

        let mut indices = data
            .read_indices()
            .context("a mesh in the file has no vertex indices")?
            .into_u32();

        let vert_coords = data
            .read_positions()
            .context("a mesh in the file has no vertex positions")?
            .map(Vec3::from)
            .collect::<Vec<_>>();

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
pub fn load_gltf(path: &str) -> Result<Mesh> {
    let (document, buffers, images) = {
        profiling::scope!("gltf::import");
        gltf::import(path).context("failed to load the gltf file")
    }?;

    let folder = std::path::Path::new(path)
        .parent()
        .and_then(|file| file.as_os_str().to_str())
        .context("failed to read the parent folder of the file")?;

    let mut triangles = Vec::new();
    let mut triangle_extras = Vec::new();

    let mut materials = document
        .materials()
        .collect::<Vec<_>>()
        .par_iter()
        .map(|material| parse_material(&material, &images, folder))
        .collect::<Result<Vec<_>, _>>()
        .context("failed to parse materials")?;

    // i.e. default material
    materials.push(ImageOrColor::Color(image::Rgba([255, 255, 255, 255])));

    let mut bounds = BoundingBox::max();

    for mesh in document.meshes() {
        parse_mesh(
            &mesh,
            &mut bounds,
            &materials,
            &buffers,
            &mut triangles,
            &mut triangle_extras,
        )?;
    }

    Ok(Mesh {
        materials,
        triangles,
        triangle_extras,
        bounds,
    })
}
