use crate::*;
use glam::{Vec3, Vec4};

pub type Triangle = [Vec3; 3];
pub type TriangleExtras = [io::VertexExtras; 3];

#[must_use]
#[derive(Debug, Clone, Copy)]
pub struct BoundingBox {
    pub min: Vec3,
    pub max: Vec3,
}

impl BoundingBox {
    pub const fn zero() -> Self {
        Self {
            min: Vec3::MAX,
            max: Vec3::MIN,
        }
    }

    pub fn extend(&mut self, pos: Vec3) {
        self.min = self.min.min(pos);
        self.max = self.max.max(pos);
    }

    pub fn size(&self) -> Vec3 {
        self.max - self.min
    }
}

#[inline]
#[must_use]
pub fn interpolate_color(colors: [Rgba<u8>; 3], bary: Vec3) -> Rgba<u8> {
    let c0 = Vec4::from_array(colors[0].0.map(|c| c as f32));
    let c1 = Vec4::from_array(colors[1].0.map(|c| c as f32));
    let c2 = Vec4::from_array(colors[2].0.map(|c| c as f32));

    let final_color = c0 * bary.x + c1 * bary.y + c2 * bary.z;

    Rgba([
        final_color.x as u8,
        final_color.y as u8,
        final_color.z as u8,
        final_color.w as u8,
    ])
}

#[inline]
#[must_use]
pub fn multiply_colors(c1: Rgba<u8>, c2: Rgba<u8>) -> Rgba<u8> {
    Rgba([
        ((c1[0] as u16 * c2[0] as u16) / 255) as u8,
        ((c1[1] as u16 * c2[1] as u16) / 255) as u8,
        ((c1[2] as u16 * c2[2] as u16) / 255) as u8,
        ((c1[3] as u16 * c2[3] as u16) / 255) as u8,
    ])
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
