//! In-memory representation of the [`Scene`].
use crate::geometry::{BoundingBox, Triangle};
use image::{Rgba, RgbaImage};
use std::sync::Arc;

/// A complete 3D scene with all the data required for voxelization.
pub struct Scene {
    /// All triangles contained within all instances of models of the scene.
    ///
    /// The scene does not distinguish models. If you have a model
    /// with multiple instances, you should just expand them all
    /// into different triangles.
    pub triangles: Vec<Triangle>,
    /// All materials contained within the scene
    pub materials: Vec<Material>,
    /// The bounding box of all triangles within the scene.
    ///
    /// During voxelization, the voxels (which should all be positioned
    /// within the bounding box of the scene) will be translated so
    /// that instead of starting at [`min`](BoundingBox::min) and ending at
    /// [`max`](BoundingBox::max), they will start at `0, 0, 0` and end at
    /// [`size`](BoundingBox::size) instead.
    pub bounds: BoundingBox,
}

/// Determines how a mesh's color and emission are rendered.
pub struct Material {
    /// Data about the albedo texture of the material
    pub texturing: Option<MaterialTexturing>,
    /// The color alpha threshold below which any voxels should be
    /// discarded. If not present, no discarding will happen.
    pub alpha_threshold: Option<u8>,
    /// The base color of the material. If the material is emissive,
    /// this will be the color of its emissive texture.
    pub base_color: Rgba<u8>,
    /// Whether the material is emissive
    pub emissive: bool,
}

/// Data about the albedo texture of the material
pub struct MaterialTexturing {
    /// The actual texture
    pub texture: Arc<RgbaImage>,
    /// Wrap modes for `u, v` respectively
    pub wrap_mode: [WrapMode; 2],
}

/// Wrap mode of a texture
#[derive(Clone, Copy)]
pub enum WrapMode {
    /// Clamps every texture coordinates into the `[0, 1]` range.
    ClampToEdge = 1,
    /// Repeats the texture if UVs go out of the `[0, 1]` range,
    /// but mirrors it.
    MirroredRepeat,
    /// Repeats the texture if UVs go out of the `[0, 1]` range
    Repeat,
}

impl WrapMode {
    /// Applies the wrap mode onto a single coordinate `c`
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
