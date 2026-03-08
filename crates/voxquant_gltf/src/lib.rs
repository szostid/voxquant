//! `glTF 2.0` input support for [`voxquant_core`] through the [`gltf`](https://docs.rs/gltf/latest/gltf/) crate
use anyhow::{Context as _, Result};
use clap::Args;
use glam::{Mat4, Vec2, Vec3, Vec4};
use image::{Rgba, RgbaImage};
use std::path::Path;
use std::sync::Arc;
use voxquant_core::geometry::{BoundingBox, Triangle, Vertex};
use voxquant_core::scene::{Material, MaterialTexturing, Scene, WrapMode};
use voxquant_core::{Format, InputFormat};

struct GltfTexturingExtras {
    tex_coord: u32,
}

struct GltfMaterialExtras {
    /// If the material has some [`texturing`](Material::texturing),
    /// this will contain the texturing extras
    texturing: Option<GltfTexturingExtras>,
}

struct MeshInstance<'a> {
    mesh: gltf::Mesh<'a>,
    transform: Mat4,
}

fn collect_instances<'a>(
    node: &gltf::Node<'a>,
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
        collect_instances(&child, global_transform, instances);
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

fn get_material_texture_data(
    mat: &gltf::Material,
    image_data: &[Arc<RgbaImage>],
) -> Result<Option<(MaterialTexturing, GltfTexturingExtras)>> {
    fn with_material_texture<R>(
        mat: &gltf::Material,
        f: impl FnOnce(gltf::texture::Info<'_>) -> R,
    ) -> Option<R> {
        if let Some(info) = mat.emissive_texture() {
            return Some(f(info));
        }

        if let Some(info) = mat.pbr_metallic_roughness().base_color_texture() {
            return Some(f(info));
        }

        if let Some(info) = mat
            .pbr_specular_glossiness()
            .and_then(|spectral| spectral.diffuse_texture())
        {
            return Some(f(info));
        }

        None
    }

    const fn into_voxelization_mode(value: gltf::texture::WrappingMode) -> WrapMode {
        match value {
            gltf::texture::WrappingMode::ClampToEdge => WrapMode::ClampToEdge,
            gltf::texture::WrappingMode::MirroredRepeat => WrapMode::MirroredRepeat,
            gltf::texture::WrappingMode::Repeat => WrapMode::Repeat,
        }
    }

    with_material_texture(mat, |texture_info| {
        let texture_index = texture_info.texture().source().index();

        let texture = image_data
            .get(texture_index)
            .context("failed to fetch image data (index is out of bounds)")?;

        Ok((
            MaterialTexturing {
                texture: Arc::clone(texture),
                wrap_mode: [
                    into_voxelization_mode(texture_info.texture().sampler().wrap_s()),
                    into_voxelization_mode(texture_info.texture().sampler().wrap_t()),
                ],
            },
            GltfTexturingExtras {
                tex_coord: texture_info.tex_coord(),
            },
        ))
    })
    .map_or(Ok(None), |f| f.map(Some))
}

#[profiling::function]
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "intentionally quantized to 8-bit RGB"
)]
fn parse_material(
    mat: &gltf::Material,
    image_data: &[Arc<RgbaImage>],
) -> Result<(Material, GltfMaterialExtras)> {
    let alpha_threshold = match mat.alpha_mode() {
        gltf::material::AlphaMode::Opaque => None,
        gltf::material::AlphaMode::Mask => {
            let cutoff = mat.alpha_cutoff().unwrap_or(0.5);
            Some((cutoff * 255.0) as u8)
        }
        // we cannot handle transparency yet, so we do a very high alpha threshold.
        // basically everything that's not opaque is not voxelized at all
        //
        // NOTE: don't use 255 here, i've found that (i guess due to precision issues?)
        // some stuff can become a swiss cheese with too high of a threashold
        gltf::material::AlphaMode::Blend => Some(250),
    };

    let emissive = mat.emissive_factor().into_iter().any(|c| c > 0.0);

    let base_color = if emissive {
        let [r, g, b] = mat.emissive_factor().map(|r| (r * 255.0) as u8);

        Rgba([r, g, b, 255])
    } else {
        mat.pbr_metallic_roughness()
            .base_color_factor()
            .map(|r| (r * 255.0) as u8)
            .into()
    };

    let (texturing, texturing_extras) = match get_material_texture_data(mat, image_data)? {
        Some((texturing, extras)) => (Some(texturing), Some(extras)),
        None => (None, None),
    };

    Ok((
        Material {
            texturing,
            alpha_threshold,
            base_color,
            emissive,
        },
        GltfMaterialExtras {
            texturing: texturing_extras,
        },
    ))
}

#[derive(Default)]
struct MeshScratch {
    positions: Vec<Vec3>,
    uvs: Vec<Vec2>,
    colors: Vec<Rgba<u8>>,
    indices: Vec<u32>,
}

#[profiling::function]
#[expect(
    clippy::cast_possible_truncation,
    reason = "safe to assume that neither material indices or triangle indices will be larger than usize"
)]
fn parse_mesh_instance(
    instance: MeshInstance,
    bounds: &mut BoundingBox,
    materials: &[Material],
    material_extras: &[GltfMaterialExtras],
    buffers: &[gltf::buffer::Data],
    triangles: &mut Vec<Triangle>,
    scratch: &mut MeshScratch,
) -> Result<()> {
    fn push_triangle(
        [i1, i2, i3]: [u32; 3],
        triangles: &mut Vec<Triangle>,
        scratch: &MeshScratch,
        material_index: u32,
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

        triangles.push(Triangle {
            vertices: [
                Vertex::new(
                    scratch.positions[i1],
                    scratch.uvs.get(i1).copied(),
                    scratch.colors.get(i1).copied(),
                ),
                Vertex::new(
                    scratch.positions[i2],
                    scratch.uvs.get(i2).copied(),
                    scratch.colors.get(i2).copied(),
                ),
                Vertex::new(
                    scratch.positions[i3],
                    scratch.uvs.get(i3).copied(),
                    scratch.colors.get(i3).copied(),
                ),
            ],
            material_index,
        });
    }

    for primitive in instance.mesh.primitives() {
        let reader = primitive.reader(|buffer| Some(&buffers[buffer.index()]));
        let material_idx = primitive.material().index().unwrap_or(materials.len() - 1);

        let material = &materials[material_idx];

        let material_tex_coord = material_extras[material_idx]
            .texturing
            .as_ref()
            .map_or(0, |tex| tex.tex_coord);

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
        if let Some(uv_iter) = reader.read_tex_coords(material_tex_coord) {
            scratch.uvs.extend(uv_iter.into_f32().map(Vec2::from));
        } else if material.texturing.is_some() {
            eprintln!("material has an explicit `tex_coord` which doesn't exist");
        }

        scratch.colors.clear();
        if let Some(color_iter) = reader.read_colors(0) {
            let color_iter = color_iter.into_rgba_u8().map(Rgba);

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
                    push_triangle(triangle, triangles, scratch, material_idx as u32);
                }
            }
            gltf::mesh::Mode::TriangleStrip => {
                for (i, window) in scratch.indices.windows(3).enumerate() {
                    let Ok([idx0, idx1, idx2]) = <[u32; 3]>::try_from(window) else {
                        unreachable!()
                    };

                    // winding order flips every odd triangle
                    if i.is_multiple_of(2) {
                        push_triangle([idx0, idx1, idx2], triangles, scratch, material_idx as u32);
                    } else {
                        push_triangle([idx0, idx2, idx1], triangles, scratch, material_idx as u32);
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

                        push_triangle([idx0, idx1, idx2], triangles, scratch, material_idx as u32);
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
    use rayon::prelude::*;

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
fn load_gltf(path: &Path, root_transform: Mat4) -> Result<Scene> {
    let (document, buffers, images) = import_gltf(path).context("failed to load the gltf file")?;

    let (mut materials, mut material_extras) = document
        .materials()
        .map(|material| parse_material(&material, &images))
        .collect::<Result<(Vec<_>, Vec<_>), _>>()
        .context("failed to parse materials")?;

    // default fallback material
    materials.push(Material {
        texturing: None,
        alpha_threshold: None,
        base_color: Rgba([255, 255, 255, 255]),
        emissive: false,
    });

    material_extras.push(GltfMaterialExtras { texturing: None });

    let mut instances = Vec::new();
    for scene in document.scenes() {
        for node in scene.nodes() {
            collect_instances(&node, root_transform, &mut instances);
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
    let mut bounds = BoundingBox::empty();

    let mut scratch = MeshScratch::default();

    for instance in instances {
        if let Err(e) = parse_mesh_instance(
            instance,
            &mut bounds,
            &materials,
            &material_extras,
            &buffers,
            &mut triangles,
            &mut scratch,
        ) {
            eprintln!("failed to parse mesh: {e}");
        }
    }

    Ok(Scene {
        triangles,
        materials,
        bounds,
    })
}

/// Config for the [`Gltf`] voxelizer.
#[derive(Debug, Args)]
#[command(next_help_heading = "`.gltf` format options")]
pub struct GltfConfig {
    /// The provided scale will be applied onto the model during importing
    #[arg(long, default_value_t = 1.0)]
    pub base_scale: f32,
}

/// The definition of the input format.
pub struct Gltf;

impl Format for Gltf {
    // Y: up, -Z: forward, X: right
    const BASIS: Mat4 = Mat4::from_cols(
        Vec4::new(1.0, 0.0, 0.0, 0.0),  // X
        Vec4::new(0.0, 1.0, 0.0, 0.0),  // Y
        Vec4::new(0.0, 0.0, -1.0, 0.0), // -Z
        Vec4::new(0.0, 0.0, 0.0, 1.0),  // W
    );
}

impl InputFormat for Gltf {
    type Config = GltfConfig;

    fn load(transform_matrix: Mat4, path: &Path, config: GltfConfig) -> Result<Scene> {
        let root_transform = transform_matrix * Mat4::from_scale(Vec3::splat(config.base_scale));

        load_gltf(path, root_transform)
    }
}
