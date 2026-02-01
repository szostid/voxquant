use crate::*;

use glam::{Vec2, Vec3};
use std::ops::Index;

#[derive(Debug, Clone, Copy)]
pub struct Vertex {
    pub pos: Vec3,
    pub color: Rgba<u8>,
    /// [`Vec2::NAN`] if UV's not present
    uv: Vec2,
}

impl Vertex {
    pub fn new(pos: Vec3, uv: Option<Vec2>, color: Option<Rgba<u8>>) -> Self {
        Self {
            pos,
            uv: uv.unwrap_or(Vec2::NAN),
            color: color.unwrap_or(Rgba([255, 255, 255, 255])),
        }
    }

    #[inline]
    #[must_use]
    pub fn uv(&self) -> Option<Vec2> {
        (self.uv != Vec2::NAN).then_some(self.uv)
    }
}

/// Triangle, defined by three vertices and a material that it uses.
#[derive(Clone, Copy)]
pub struct Triangle {
    pub vertices: [Vertex; 3],
    pub material_index: u32,
}

impl Triangle {
    /// Returns the UV coordinates of the three vertices, if present
    #[inline]
    #[must_use]
    pub fn uvs(&self) -> Option<[Vec2; 3]> {
        let [va, vb, vc] = &self.vertices;

        let uv_a = va.uv()?;
        let uv_b = vb.uv()?;
        let uv_c = vc.uv()?;

        Some([uv_a, uv_b, uv_c])
    }

    /// Returns the base colors of the three vertices
    #[inline]
    #[must_use]
    pub fn colors(&self) -> [Rgba<u8>; 3] {
        self.vertices.map(|v| v.color)
    }
}

impl Index<usize> for Triangle {
    type Output = Vec3;

    #[inline]
    fn index(&self, index: usize) -> &Vec3 {
        &self.vertices[index].pos
    }
}

#[must_use]
#[derive(Debug, Clone, Copy)]
pub struct BoundingBox {
    pub min: Vec3,
    pub max: Vec3,
}

impl BoundingBox {
    #[inline]
    pub const fn zero() -> Self {
        Self {
            min: Vec3::MAX,
            max: Vec3::MIN,
        }
    }

    #[inline]
    pub fn extend(&mut self, pos: Vec3) {
        self.min = self.min.min(pos);
        self.max = self.max.max(pos);
    }

    #[inline]
    pub fn size(&self) -> Vec3 {
        self.max - self.min
    }
}

#[derive(Debug, Clone, Copy)]
pub struct TriangleInterpolator {
    /// `a`
    a: Vec3,

    /// `b - a`
    v0: Vec3,
    /// `c - a`
    v1: Vec3,

    /// `v0 * v0`
    d00: f32,
    /// `v0 * v1`
    d01: f32,
    /// `v1 * v1`
    d11: f32,

    /// Inverse determinant for Cramer's rule
    inv_det: f32,
}

impl TriangleInterpolator {
    #[inline]
    #[must_use]
    #[expect(clippy::suspicious_operation_groupings, reason = "???")]
    pub fn new(tri: Triangle) -> Self {
        let v0 = tri[1] - tri[0];
        let v1 = tri[2] - tri[0];

        let d00 = v0.dot(v0);
        let d01 = v0.dot(v1);
        let d11 = v1.dot(v1);

        let det = d00 * d11 - d01 * d01;

        let inv_det = if det.abs() < f32::EPSILON {
            0.0
        } else {
            1.0 / det
        };

        Self {
            a: tri[0],
            v0,
            v1,
            d00,
            d01,
            d11,
            inv_det,
        }
    }

    #[inline]
    #[must_use]
    pub fn normal(&self) -> Vec3 {
        self.v0.cross(self.v1)
    }

    #[inline]
    #[must_use]
    pub fn get_closest_barycentric(&self, p: Vec3) -> Vec3 {
        let v2 = p - self.a;

        let d20 = self.v0.dot(v2);
        let d21 = self.v1.dot(v2);

        let v = (self.d11 * d20 - self.d01 * d21) * self.inv_det;
        let w = (self.d00 * d21 - self.d01 * d20) * self.inv_det;
        let u = 1.0 - v - w;

        Vec3::new(u, v, w)
    }
}
