use crate::*;
use geometry::{BoundingBox, Triangle};

use std::sync::Arc;

pub struct Material {
    pub texturing: Option<MaterialTexturing>,
    pub alpha_threshold: Option<u8>,
    /// The base color of the material. If the material is emissive,
    /// this will be the color of its emissive texture.
    pub base_color: Rgba<u8>,
    /// Whether the material is emissive
    pub emissive: bool,
}

#[derive(Clone, Copy)]
pub enum WrapMode {
    ClampToEdge = 1,
    MirroredRepeat,
    Repeat,
}

impl WrapMode {
    #[inline]
    #[must_use]
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

pub struct Scene {
    pub triangles: Vec<Triangle>,
    pub materials: Vec<Material>,
    pub bounds: BoundingBox,
}
