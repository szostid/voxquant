use crate::io::*;
use crate::*;
use image::ImageBuffer;
use image::Rgb;
use image::Rgba;
use image::buffer::ConvertBuffer;
use rayon::prelude::*;
use std::sync::Mutex;

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

    let mut materials = document
        .materials()
        .collect::<Vec<_>>()
        .par_iter()
        .map(|material| parse_material(&material, &images))
        .collect::<Result<Vec<_>, _>>()
        .context("failed to parse materials")?;

    // i.e. default material
    materials.push(ImageOrColor::Color(image::Rgba([255, 255, 255, 255])));

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

    let all_triangles = Mutex::new(Vec::with_capacity(total_triangles));
    let all_extras = Mutex::new(Vec::with_capacity(total_triangles));
    let all_bounds = Mutex::new(BoundingBox::zero());

    document
        .meshes()
        .collect::<Vec<_>>()
        .into_par_iter()
        // at least 32 meshes per job to reduce overhead
        .with_min_len(32)
        .fold(
            || (Vec::new(), Vec::new(), BoundingBox::zero()),
            |mut acc, mesh| {
                let (tris, extras, bounds) = &mut acc;

                if let Err(e) = parse_mesh(&mesh, bounds, &materials, &buffers, tris, extras) {
                    eprintln!("Failed to parse mesh: {:?}", e);
                }

                acc
            },
        )
        .for_each(|(triangles, extras, bounds)| {
            all_bounds.lock().unwrap().combine(bounds);
            all_extras.lock().unwrap().extend(extras);
            all_triangles.lock().unwrap().extend(triangles);
        });

    Ok(Mesh {
        materials,
        triangles: all_triangles.into_inner().unwrap(),
        triangle_extras: all_extras.into_inner().unwrap(),
        bounds: all_bounds.into_inner().unwrap(),
    })
}
