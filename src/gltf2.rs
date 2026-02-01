use crate::*;
use glam::{Mat4, Vec2, Vec3};
use io::{ImageOrColor, Mesh, VertexExtras};
use rayon::prelude::*;
use std::sync::Arc;

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
fn convert_image(data: gltf::image::Data) -> Result<Arc<RgbaImage>> {
    use bytemuck::Pod;
    use gltf::image::Format;
    use image::Pixel;
    use image::buffer::ConvertBuffer;
    use image::{ImageBuffer, Luma, LumaA, Rgb, Rgba, RgbaImage};

    /// Given `data`, converts the image (assumed to have
    /// the pixel format `P`) into an Rgba8 image
    fn convert_image<P: Pixel>(data: gltf::image::Data) -> Result<Arc<RgbaImage>>
    where
        P::Subpixel: Pod,
        ImageBuffer<P, Vec<P::Subpixel>>: ConvertBuffer<RgbaImage>,
    {
        fn convert_data<T: Pod>(data: Vec<u8>) -> Vec<T> {
            match bytemuck::try_cast_vec::<u8, T>(data) {
                Ok(data) => data,
                Err((_, data)) => bytemuck::pod_collect_to_vec(&data),
            }
        }

        let pixels = convert_data(data.pixels);

        ImageBuffer::from_vec(data.width, data.height, pixels)
            .context("image has invalid dimensions")
            .map(|img| Arc::new(img.convert()))
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
        Format::R8G8B8A8 => RgbaImage::from_vec(data.width, data.height, data.pixels)
            .context("image has invalid dimensions")
            .map(Arc::new),
    }
}

#[profiling::function]
fn parse_image(image_data: &[Arc<RgbaImage>], texture: gltf::Texture) -> Result<Arc<RgbaImage>> {
    let image_index = texture.source().index();

    let image = image_data
        .get(image_index)
        .context("failed to fetch image data (index is out of bounds)")?;

    Ok(Arc::clone(image))
}

#[profiling::function]
fn parse_material(mat: &gltf::Material, image_data: &[Arc<RgbaImage>]) -> Result<ImageOrColor> {
    let alpha_threshold = match mat.alpha_mode() {
        gltf::material::AlphaMode::Opaque => None,
        gltf::material::AlphaMode::Mask => {
            let cutoff = mat.alpha_cutoff().unwrap_or(0.5);
            Some((cutoff * 255.0) as u8)
        }
        gltf::material::AlphaMode::Blend => Some(128),
    };

    if let Some(image) = mat
        .pbr_metallic_roughness()
        .base_color_texture()
        .map(|texture_info| texture_info.texture())
    {
        let image = parse_image(image_data, image)
            .context("failed to parse the color image used by the material")?;

        return Ok(ImageOrColor::Image {
            image,
            alpha_threshold,
        });
    }

    if let Some(image) = mat
        .emissive_texture()
        .map(|texture_info| texture_info.texture())
    {
        let image = parse_image(image_data, image)
            .context("failed to parse the emissive image used by the material")?;

        return Ok(ImageOrColor::Image {
            image,
            alpha_threshold,
        });
    }

    if let Some(image) = mat
        .pbr_specular_glossiness()
        .and_then(|spectral| spectral.diffuse_texture())
        .map(|texture_info| texture_info.texture())
    {
        let image = parse_image(image_data, image)
            .context("failed to parse the spectral image used by the material")?;

        return Ok(ImageOrColor::Image {
            image,
            alpha_threshold,
        });
    }

    let base_color = mat.pbr_metallic_roughness().base_color_factor();

    let base_color = Rgba([
        (base_color[0] * 255.0) as u8,
        (base_color[1] * 255.0) as u8,
        (base_color[2] * 255.0) as u8,
        (base_color[3] * 255.0) as u8,
    ]);

    Ok(ImageOrColor::Color {
        color: base_color,
        alpha_threshold,
    })
}

#[derive(Default)]
struct MeshScratch {
    positions: Vec<Vec3>,
    uvs: Vec<Vec2>,
    colors: Vec<[u8; 4]>,
    indices: Vec<u32>,
}

#[profiling::function]
fn parse_mesh_instance(
    instance: MeshInstance,
    bounds: &mut BoundingBox,
    materials: &[ImageOrColor],
    buffers: &[gltf::buffer::Data],
    triangles: &mut Vec<Triangle>,
    extras: &mut Vec<TriangleExtras>,
    scratch: &mut MeshScratch,
) -> Result<()> {
    fn push_triangle(
        [i1, i2, i3]: [u32; 3],
        triangles: &mut Vec<Triangle>,
        extras: &mut Vec<TriangleExtras>,
        scratch: &MeshScratch,
        material_idx: u32,
    ) {
        let i1 = i1 as usize;
        let i2 = i2 as usize;
        let i3 = i3 as usize;

        // check for malformed indices
        if i1 >= scratch.positions.len()
            || i2 >= scratch.positions.len()
            || i3 >= scratch.positions.len()
        {
            return;
        }

        triangles.push([
            scratch.positions[i1],
            scratch.positions[i2],
            scratch.positions[i3],
        ]);

        extras.push([
            VertexExtras::new(
                scratch.uvs.get(i1).copied(),
                scratch.colors.get(i1).copied(),
                material_idx,
            ),
            VertexExtras::new(
                scratch.uvs.get(i2).copied(),
                scratch.colors.get(i2).copied(),
                material_idx,
            ),
            VertexExtras::new(
                scratch.uvs.get(i3).copied(),
                scratch.colors.get(i3).copied(),
                material_idx,
            ),
        ]);
    }

    for primitive in instance.mesh.primitives() {
        let reader = primitive.reader(|buffer| Some(&buffers[buffer.index()]));
        let material_idx = primitive.material().index().unwrap_or(materials.len()) as u32;

        let positions = reader
            .read_positions()
            .context("mesh has no positions")?
            .map(|pos| instance.transform.transform_point3(Vec3::from(pos)));

        scratch.positions.clear();
        scratch.positions.reserve(positions.len());

        for pos in positions {
            bounds.extend(pos);
            scratch.positions.push(pos);
        }

        scratch.uvs.clear();
        if let Some(uv_iter) = reader.read_tex_coords(0) {
            scratch.uvs.extend(uv_iter.into_f32().map(Vec2::from));
        }

        scratch.colors.clear();
        if let Some(color_iter) = reader.read_colors(0) {
            let color_iter = color_iter.into_rgba_u8();

            scratch.colors.extend(color_iter);
        }

        scratch.indices.clear();
        if let Some(indices) = reader.read_indices() {
            scratch.indices.extend(indices.into_u32());
        } else {
            scratch.indices.extend(0..scratch.positions.len() as u32);
        }

        match primitive.mode() {
            gltf::mesh::Mode::Triangles => {
                let (triangle_indices, _) = scratch.indices.as_chunks::<3>();

                for &triangle in triangle_indices {
                    push_triangle(triangle, triangles, extras, scratch, material_idx);
                }
            }
            gltf::mesh::Mode::TriangleStrip => {
                for (i, window) in scratch.indices.windows(3).enumerate() {
                    let Ok([idx0, idx1, idx2]) = <[u32; 3]>::try_from(window) else {
                        unreachable!()
                    };

                    // winding order flips every odd triangle
                    if i.is_multiple_of(2) {
                        push_triangle([idx0, idx1, idx2], triangles, extras, scratch, material_idx);
                    } else {
                        push_triangle([idx0, idx2, idx1], triangles, extras, scratch, material_idx);
                    }
                }
            }
            gltf::mesh::Mode::TriangleFan => {
                if scratch.indices.len() >= 3 {
                    let idx0 = scratch.indices[0];

                    for window in scratch.indices[1..].windows(2) {
                        let Ok([idx1, idx2]) = <[u32; 2]>::try_from(window) else {
                            unreachable!()
                        };

                        push_triangle([idx0, idx1, idx2], triangles, extras, scratch, material_idx);
                    }
                }
            }
            gltf::mesh::Mode::LineLoop | gltf::mesh::Mode::Lines | gltf::mesh::Mode::LineStrip => {
                eprintln!("line primitives are not supported");
            }
            gltf::mesh::Mode::Points => {
                eprintln!("point primitives are not supported");
            }
        }
    }

    Ok(())
}

/// A multithreaded version of `gltf::import`. Outputs `RgbaImage` instead of `gltf::image::Data`.
#[profiling::function]
fn import_gltf(
    path: &Path,
) -> Result<(gltf::Document, Vec<gltf::buffer::Data>, Vec<Arc<RgbaImage>>)> {
    let base = path.parent().unwrap_or_else(|| std::path::Path::new("."));

    let mut gltf = {
        profiling::scope!("gltf::load_document");

        let file = std::fs::File::open(path).context("failed to open file")?;
        let reader = std::io::BufReader::new(file);
        gltf::Gltf::from_reader(reader).context("failed to parse gltf")?
    };

    let buffers = {
        profiling::scope!("gltf::import_buffers");

        gltf::import_buffers(&gltf.document, Some(base), gltf.blob.take())
            .context("failed to read buffers")?
    };

    let images = gltf
        .images()
        .collect::<Vec<_>>()
        .into_par_iter()
        .map(|image| {
            profiling::scope!("gltf::load_image");

            let texture_data = gltf::image::Data::from_source(image.source(), Some(base), &buffers)
                .context("failed to read texture")?;

            convert_image(texture_data)
        })
        .collect::<Result<Vec<_>>>()?;

    Ok((gltf.document, buffers, images))
}

#[profiling::function]
pub fn load_gltf(path: &Path, scale: f32) -> Result<Mesh> {
    let (document, buffers, images) = import_gltf(path).context("failed to load the gltf file")?;

    let mut materials = document
        .materials()
        .map(|material| parse_material(&material, &images))
        .collect::<Result<Vec<_>, _>>()
        .context("failed to parse materials")?;

    // i.e. default material
    materials.push(ImageOrColor::Color {
        color: Rgba([255, 255, 255, 255]),
        alpha_threshold: None,
    });

    let mut instances = Vec::new();
    for scene in document.scenes() {
        for node in scene.nodes() {
            collect_instances(node, Mat4::from_scale(Vec3::splat(scale)), &mut instances);
        }
    }

    let total_triangles: usize = instances
        .iter()
        .map(|instance| {
            instance
                .mesh
                .primitives()
                .filter(|p| p.mode() == gltf::mesh::Mode::Triangles)
                .map(|p| p.indices().map_or(0, |a| a.count() / 3))
                .sum::<usize>()
        })
        .sum();

    let mut triangles = Vec::with_capacity(total_triangles);
    let mut triangle_extras = Vec::with_capacity(total_triangles);
    let mut bounds = BoundingBox::zero();

    let mut scratch = MeshScratch::default();

    for instance in instances {
        if let Err(e) = parse_mesh_instance(
            instance,
            &mut bounds,
            &materials,
            &buffers,
            &mut triangles,
            &mut triangle_extras,
            &mut scratch,
        ) {
            eprintln!("failed to parse mesh: {e}");
        }
    }

    Ok(Mesh {
        triangles,
        triangle_extras,
        materials,
        bounds,
    })
}
