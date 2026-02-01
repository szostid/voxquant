use crate::*;
use bytemuck::Pod;
use bytemuck::Zeroable;
use image::RgbaImage;
use std::sync::Arc;

#[repr(C)]
#[derive(Debug, Clone, Copy, Zeroable, Pod)]
pub struct Vertex {
    pub position: Vec3,
    pub color: [u8; 4],
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Zeroable, Pod)]
pub struct FloatVertex {
    pub position: Vec3,
    pub color: [f32; 4],
}

impl From<Vertex> for FloatVertex {
    fn from(value: Vertex) -> Self {
        let color = value.color.map(|c| c as f32 / 255.0);
        Self {
            position: value.position,
            color,
        }
    }
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Zeroable, Pod)]
pub struct VertexExtras {
    uv: Vec2,
    pub color: [u8; 4],
    pub material_idx: u32,
}

impl VertexExtras {
    pub fn new(uv: Option<Vec2>, color: Option<[u8; 4]>, material_idx: u32) -> Self {
        Self {
            uv: uv.unwrap_or(Vec2::NAN),
            color: color.unwrap_or([255, 255, 255, 255]),
            material_idx,
        }
    }

    #[inline]
    #[must_use]
    pub fn uv(&self) -> Option<Vec2> {
        (self.uv != Vec2::NAN).then_some(self.uv)
    }
}

#[derive(Debug, Clone)]
pub enum ImageOrColor {
    Image {
        image: Arc<RgbaImage>,
        alpha_threshold: Option<u8>,
    },
    Color {
        color: Color,
        alpha_threshold: Option<u8>,
    },
}

#[derive(Debug, Clone)]
pub struct Mesh {
    pub triangles: Vec<Triangle>,
    pub triangle_extras: Vec<TriangleExtras>,
    pub materials: Vec<ImageOrColor>,

    pub bounds: BoundingBox,
}

pub mod magica {
    // 6 shades of Red (0..5)
    // 7 shades of Green (0..6)
    // 6 shades of Blue (0..5)
    const R_STEPS: u16 = 6;
    const G_STEPS: u16 = 7;
    const B_STEPS: u16 = 6;

    /// Maps an RGBA color to a palette index (1-253).
    /// Index 0 is reserved for 'Air' in MagicaVoxel, so we shift everything by +1.
    pub const fn encode_color(color: [u8; 4]) -> u8 {
        let r = color[0] as u16;
        let g = color[1] as u16;
        let b = color[2] as u16;

        let r_idx = (r * (R_STEPS - 1) + 127) / 255;
        let g_idx = (g * (G_STEPS - 1) + 127) / 255;
        let b_idx = (b * (B_STEPS - 1) + 127) / 255;

        let packed = r_idx + (g_idx * R_STEPS) + (b_idx * R_STEPS * G_STEPS);

        (packed + 1) as u8
    }

    /// Maps a palette index (1-253) back to an RGBA color.
    pub const fn decode_color(byte: u8) -> [u8; 4] {
        if byte == 0 {
            return [0, 0, 0, 0];
        }

        let val = (byte - 1) as u16;

        let r_idx = val % R_STEPS;
        let g_idx = (val / R_STEPS) % G_STEPS;
        let b_idx = (val / (R_STEPS * G_STEPS)) % B_STEPS;

        // scale back to 0..255
        let r = (r_idx * 255) / (R_STEPS - 1);
        let g = (g_idx * 255) / (G_STEPS - 1);
        let b = (b_idx * 255) / (B_STEPS - 1);

        [r as u8, g as u8, b as u8, 255]
    }
}

#[profiling::function]
pub fn save_as_magica_voxel(chunks: Vec<voxelizer::Chunk>, file_path: &str) -> Result<()> {
    use dot_vox::*;

    // the palette starts at index 1 and ends later because magicavoxel only allows for 255
    // indices and reserves the first index for a black color. we can therefore skip the black
    // color
    let mut palette = Vec::with_capacity(256);

    for index in 0..=255 {
        let color = magica::decode_color(index);
        palette.push(dot_vox::Color {
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

        nodes.push(SceneNode::Transform {
            attributes: Default::default(),
            frames: vec![Frame {
                attributes: [(
                    "_t".to_string(),
                    format!("{} {} {}", chunk.origin.x, chunk.origin.z, chunk.origin.y),
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
    let data = dot_vox::DotVoxData {
        version: 150,
        models,
        palette,
        materials: Vec::new(),
        layers: Vec::new(),
        scenes: nodes,
    };

    let mut file = std::fs::File::create(file_path)?;

    data.write_vox(&mut file)?;

    Ok(())
}
