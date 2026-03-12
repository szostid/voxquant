//! Geometric primitives and basic math for voxelization.
use glam::Vec3;

/// A vertex with some associated color and UV (if present) data.
#[derive(Debug, Clone, Copy)]
pub struct Vertex {
    /// Position of the vertex.
    ///
    /// The exact origin of the vertex space is unspecified.
    /// It will be translated by the voxelizer based on
    /// the [`BoundingBox`] of the scene.
    pub pos: [f32; 3],
    /// The base color of the vertex. If a texture is
    /// present, its color will be tinted by this field.
    pub color: [u8; 4],
    /// [`Vec2::NAN`] if UV's not present
    uv: [f32; 2],
}

impl Vertex {
    /// Creates a new vertex
    #[inline]
    #[must_use]
    pub fn new(pos: [f32; 3], uv: Option<[f32; 2]>, color: Option<[u8; 4]>) -> Self {
        Self {
            pos,
            uv: uv.unwrap_or([f32::NAN; 2]),
            color: color.unwrap_or([255, 255, 255, 255]),
        }
    }

    /// Returns the UV coordinates of this vertex, if they were provided
    /// when the vertex was created.
    #[inline]
    #[must_use]
    pub fn uv(&self) -> Option<[f32; 2]> {
        (!self.uv[0].is_nan()).then_some(self.uv)
    }
}

/// Triangle, defined by three vertices and a material that it uses.
#[derive(Clone, Copy)]
pub struct Triangle {
    /// The vertices of the triangle. Named `a, b, c` respectively
    /// in many parts of the code
    pub vertices: [Vertex; 3],
    /// The material used by this triangle. This is
    /// an index into the scene's materials
    pub material_index: u32,
}

impl Triangle {
    /// Returns the UV coordinates of the three vertices `a, b, c`,
    /// if ALL are present (and they should be either all present
    /// or all absent)
    #[inline]
    #[must_use]
    pub fn uvs(&self) -> Option<[[f32; 2]; 3]> {
        let [va, vb, vc] = &self.vertices;

        let uv_a = va.uv()?;
        let uv_b = vb.uv()?;
        let uv_c = vc.uv()?;

        Some([uv_a, uv_b, uv_c])
    }

    /// Returns the base colors of the three vertices
    #[inline]
    #[must_use]
    pub fn colors(&self) -> [[u8; 4]; 3] {
        self.vertices.map(|v| v.color)
    }

    #[inline]
    #[must_use]
    pub(crate) fn unpack_vertices_to_glam(&self) -> [Vec3; 3] {
        self.vertices.map(|vertex| Vec3::from_array(vertex.pos))
    }
}

/// The bounding box of a scene.
///
/// During voxelization, the voxels (which should all be positioned within
/// the bounding box of the scene) will be translated so that instead of
/// starting at [`min`](Self::min) and ending at [`max`](Self::max), they
/// will start at `0, 0, 0` and end at [`size`](Self::size) instead.
#[derive(Debug, Clone, Copy)]
pub struct BoundingBox {
    /// The smallest (minimum) point of the bounding box
    pub min: [f32; 3],
    /// The largest (maximum) point of the bounding box
    pub max: [f32; 3],
}

impl BoundingBox {
    /// Creates an empty bounding box with no volume.
    #[inline]
    #[must_use]
    pub const fn empty() -> Self {
        Self {
            min: [f32::MAX; 3],
            max: [f32::MIN; 3],
        }
    }

    /// Returns true if no points have been added to this box.
    #[inline]
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.min[0] > self.max[0] || self.min[1] > self.max[1] || self.min[2] > self.max[2]
    }

    /// Extends the bounding box so that it contains the point `p`
    #[inline]
    pub const fn extend(&mut self, p: [f32; 3]) {
        self.min[0] = self.min[0].min(p[0]);
        self.min[1] = self.min[1].min(p[1]);
        self.min[2] = self.min[2].min(p[2]);
        self.max[0] = self.max[0].max(p[0]);
        self.max[1] = self.max[1].max(p[1]);
        self.max[2] = self.max[2].max(p[2]);
    }

    /// Returns the size of the bounding box (i.e. `max - min`).
    ///
    /// If the bounding box is [`empty`](Self::empty), returns [`Vec3::ZERO`]
    #[inline]
    #[must_use]
    pub fn size(&self) -> [f32; 3] {
        [
            self.max[0] - self.min[0],
            self.max[1] - self.min[1],
            self.max[2] - self.min[2],
        ]
    }
}
