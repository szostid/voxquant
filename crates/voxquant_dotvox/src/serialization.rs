use crate::voxelization::{Chunk, VoxelType};
use anyhow::Result;
use dot_vox::{Color, Model, Voxel};
use glam::{IVec3, U8Vec3};
use image::Rgba;
use std::num::NonZeroU8;
use voxquant_core::io::SceneWriter;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct VoxelWithColor {
    pub pos: U8Vec3,
    pub color: Rgba<u8>,
}

impl VoxelType for VoxelWithColor {
    #[inline]
    fn from_pos_color(pos: U8Vec3, color: Rgba<u8>) -> Self {
        Self { pos, color }
    }

    #[inline]
    fn pos(&self) -> U8Vec3 {
        self.pos
    }
}

impl VoxelType for dot_vox::Voxel {
    #[inline]
    fn from_pos_color(pos: U8Vec3, color: Rgba<u8>) -> Self {
        Self {
            x: pos.x,
            y: pos.y,
            z: pos.z,
            i: encode_color(color.0),
        }
    }

    #[inline]
    fn pos(&self) -> U8Vec3 {
        U8Vec3 {
            x: self.x,
            y: self.y,
            z: self.z,
        }
    }
}

// 6 shades of Red (0..5)
// 7 shades of Green (0..6)
// 6 shades of Blue (0..5)
const R_STEPS: u16 = 6;
const G_STEPS: u16 = 7;
const B_STEPS: u16 = 6;

/// Maps an RGBA color to a static palette index (1-253).
/// Index 0 is reserved for 'Air' in `MagicaVoxel`, so we shift everything by +1.
const fn encode_color(color: [u8; 4]) -> u8 {
    let r = color[0] as u16;
    let g = color[1] as u16;
    let b = color[2] as u16;

    let r_idx = (r * (R_STEPS - 1) + 127) / 255;
    let g_idx = (g * (G_STEPS - 1) + 127) / 255;
    let b_idx = (b * (B_STEPS - 1) + 127) / 255;

    let packed = r_idx + (g_idx * R_STEPS) + (b_idx * R_STEPS * G_STEPS);

    packed as u8
}

/// Maps a palette index (1-253) back to an RGBA color.
const fn decode_color(byte: u8) -> [u8; 4] {
    if byte == 0 {
        return [0, 0, 0, 0];
    }

    let val = byte as u16;

    let r_idx = val % R_STEPS;
    let g_idx = (val / R_STEPS) % G_STEPS;
    let b_idx = (val / (R_STEPS * G_STEPS)) % B_STEPS;

    // scale back to 0..255
    let r = (r_idx * 255) / (R_STEPS - 1);
    let g = (g_idx * 255) / (G_STEPS - 1);
    let b = (b_idx * 255) / (B_STEPS - 1);

    [r as u8, g as u8, b as u8, 255]
}

#[profiling::function]
#[cfg(feature = "dynamic_palette")]
fn quantize_colors(chunks: Vec<Chunk<VoxelWithColor>>) -> (Vec<Model>, Vec<IVec3>, Vec<Color>) {
    use quantette::{ImageRef, PaletteSize, Pipeline, QuantizeMethod, deps::palette::rgb::Rgb};
    use rayon::prelude::*;

    let voxel_colors = {
        profiling::scope!("extract_colors");

        chunks
            .par_iter()
            .with_min_len(128)
            .flat_map_iter(|chunk| {
                chunk
                    .voxels
                    .iter()
                    .map(|v| Rgb::new(v.color[0], v.color[1], v.color[2]))
            })
            .collect::<Vec<_>>()
    };

    if voxel_colors.is_empty() {
        return (Vec::new(), Vec::new(), Vec::new());
    }

    let output = Pipeline::new()
        .quantize_method(QuantizeMethod::Wu)
        .palette_size(PaletteSize::from_nz_u8(NonZeroU8::new(255).unwrap()))
        .ditherer(None)
        .parallel(true)
        .input_image(ImageRef::new(voxel_colors.len() as u32, 1, &voxel_colors).unwrap())
        .output_srgb8_indexed_image();

    let palette = output
        .palette()
        .iter()
        .map(|c| dot_vox::Color {
            r: c.red,
            g: c.green,
            b: c.blue,
            a: 255,
        })
        .collect::<Vec<_>>();

    let indices = output.indices();

    let (models, origins) = chunks
        .into_iter()
        .scan(0, |offset, chunk| {
            let start = *offset;
            let size = chunk.voxels.len();
            *offset += size;
            Some((chunk, &indices[start..start + size]))
        })
        .collect::<Vec<_>>()
        .into_par_iter()
        .with_min_len(128)
        .map(|(chunk, voxel_indices)| {
            profiling::scope!("Chunk::remap_indices");

            let new_voxels = chunk
                .voxels
                .iter()
                .zip(voxel_indices)
                .map(|(raw, &idx)| Voxel {
                    x: raw.pos.x,
                    y: raw.pos.y,
                    z: raw.pos.z,
                    i: idx,
                })
                .collect();

            (
                Model {
                    size: dot_vox::Size {
                        x: 256,
                        y: 256,
                        z: 256,
                    },
                    voxels: new_voxels,
                },
                chunk.origin,
            )
        })
        .collect::<(Vec<_>, Vec<_>)>();

    (models, origins, palette)
}

#[cfg(feature = "dynamic_palette")]
#[profiling::function]
#[expect(
    clippy::default_trait_access,
    reason = "we don't have access to the AHashMap type"
)]
pub fn write_vox_dynamic(
    chunks: Vec<Chunk<VoxelWithColor>>,
    mut output: impl SceneWriter,
    shift: IVec3,
) -> Result<()> {
    use dot_vox::{DotVoxData, Frame, SceneNode, ShapeModel};

    let (models, origins, palette) = quantize_colors(chunks);

    let mut nodes = Vec::new();

    nodes.push(SceneNode::Transform {
        attributes: Default::default(),
        frames: vec![Frame {
            attributes: Default::default(),
        }],
        child: 1,
        layer_id: 0,
    });

    nodes.push(SceneNode::Group {
        attributes: Default::default(),
        children: Vec::new(),
    });

    for (model_id, origin) in origins.into_iter().enumerate() {
        let transform_index = nodes.len() as u32;
        let shape_index = transform_index + 1;

        let origin = origin + shift;

        nodes.push(SceneNode::Transform {
            attributes: Default::default(),
            frames: vec![Frame {
                attributes: [(
                    "_t".to_string(),
                    format!("{} {} {}", origin.x, origin.y, origin.z),
                )]
                .into(),
            }],
            child: shape_index,
            layer_id: 0,
        });

        nodes.push(SceneNode::Shape {
            attributes: Default::default(),
            models: vec![ShapeModel {
                model_id: model_id as u32,
                attributes: Default::default(),
            }],
        });

        let SceneNode::Group { children, .. } = &mut nodes[1] else {
            unreachable!()
        };

        children.push(transform_index);
    }

    // Construct the scene
    let data = DotVoxData {
        version: 150,
        models,
        palette,
        index_map: (0..=255).collect(),
        materials: Vec::new(),
        layers: Vec::new(),
        scenes: nodes,
    };

    data.write_vox(&mut output)?;

    Ok(())
}

#[profiling::function]
#[expect(
    clippy::default_trait_access,
    reason = "we don't have access to the AHashMap type"
)]
pub fn write_vox_static(
    chunks: Vec<Chunk<dot_vox::Voxel>>,
    mut output: impl SceneWriter,
    shift: IVec3,
) -> Result<()> {
    use dot_vox::{DotVoxData, Frame, SceneNode, ShapeModel, Size};

    // the palette starts at index 1 and ends later because magicavoxel only allows for 255
    // indices and reserves the first index for a black color. we can therefore skip the black
    // color
    let mut palette = Vec::with_capacity(256);

    for index in 0..=255 {
        let color = decode_color(index);

        palette.push(Color {
            r: color[0],
            g: color[1],
            b: color[2],
            a: 255,
        });
    }

    let mut models = Vec::new();
    let mut nodes = Vec::new();

    nodes.push(SceneNode::Transform {
        attributes: Default::default(),
        frames: vec![Frame {
            attributes: Default::default(),
        }],
        child: 1,
        layer_id: 0,
    });

    nodes.push(SceneNode::Group {
        attributes: Default::default(),
        children: Vec::new(),
    });

    for chunk in chunks {
        let model_id = models.len() as u32;

        models.push(Model {
            size: Size {
                x: 256,
                y: 256,
                z: 256,
            },
            voxels: chunk.voxels,
        });

        let transform_index = nodes.len() as u32;
        let shape_index = transform_index + 1;

        let origin = chunk.origin + shift;

        nodes.push(SceneNode::Transform {
            attributes: Default::default(),
            frames: vec![Frame {
                attributes: [(
                    "_t".to_string(),
                    format!("{} {} {}", origin.x, origin.y, origin.z),
                )]
                .into(),
            }],
            child: shape_index,
            layer_id: 0,
        });

        nodes.push(SceneNode::Shape {
            attributes: Default::default(),
            models: vec![ShapeModel {
                model_id,
                attributes: Default::default(),
            }],
        });

        let SceneNode::Group { children, .. } = &mut nodes[1] else {
            unreachable!()
        };

        children.push(transform_index);
    }

    // Construct the scene
    let data = DotVoxData {
        version: 150,
        models,
        palette,
        materials: Vec::new(),
        index_map: (0..=255).collect(),
        layers: Vec::new(),
        scenes: nodes,
    };

    data.write_vox(&mut output)?;

    Ok(())
}
