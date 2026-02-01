use crate::*;
use bytemuck::{Pod, Zeroable};
use glam::Vec2;
use std::sync::Arc;

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

pub struct Material {
    pub texturing: Option<MaterialTexturing>,
    pub alpha_threshold: Option<u8>,
    pub base_color: Rgba<u8>,
}

#[derive(Clone, Copy)]
pub enum WrapMode {
    ClampToEdge = 1,
    MirroredRepeat,
    Repeat,
}

impl WrapMode {
    pub fn apply(self, c: f32) -> f32 {
        match self {
            Self::ClampToEdge => c.clamp(0.0, 1.0),
            Self::Repeat => c.rem_euclid(1.0),
            Self::MirroredRepeat => {
                // we calculate as though UVs range from 0..2 and we just
                // flip the UVs in the 1..2 range to be mirrored
                let m = c.rem_euclid(2.0);
                if m > 1.0 { 2.0 - m } else { m }
            }
        }
    }
}

pub struct MaterialTexturing {
    pub texture: Arc<RgbaImage>,
    pub tex_coords: u32,
    pub wrap_mode: [WrapMode; 2],
}

pub struct Mesh {
    pub triangles: Vec<Triangle>,
    pub triangle_extras: Vec<TriangleExtras>,
    pub materials: Vec<Material>,

    pub bounds: BoundingBox,
}
