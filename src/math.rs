pub use glam::*;

/// Given a triangle `a, b, c`, and a point `p`, returns the point on the triangle
/// that is the closest to the point `p`.
///
/// https://github.com/embree/embree/blob/master/tutorials/common/math/closest_point.h
#[must_use]
pub fn closest_point_triangle(p: Vec3, tri: [Vec3; 3]) -> Vec3 {
    let [a, b, c] = tri;

    let ab = b - a;
    let ac = c - a;
    let ap = p - a;

    let d1 = ab.dot(ap);
    let d2 = ac.dot(ap);
    if d1 <= 0.0 && d2 <= 0.0 {
        return a;
    };

    let bp = p - b;
    let d3 = ab.dot(bp);
    let d4 = ac.dot(bp);
    if d3 >= 0.0 && d4 <= d3 {
        return b;
    };

    let cp = p - c;
    let d5 = ab.dot(cp);
    let d6 = ac.dot(cp);
    if d6 >= 0.0 && d5 <= d6 {
        return c;
    };

    let vc = d1 * d4 - d3 * d2;
    if vc <= 0.0 && d1 >= 0.0 && d3 <= 0.0 {
        let v = d1 / (d1 - d3);
        return a + v * ab;
    }

    let vb = d5 * d2 - d1 * d6;
    if vb <= 0.0 && d2 >= 0.0 && d6 <= 0.0 {
        let v = d2 / (d2 - d6);
        return a + v * ac;
    }

    let va = d3 * d6 - d5 * d4;
    if va <= 0.0 && (d4 - d3) >= 0.0 && (d5 - d6) >= 0.0 {
        let v = (d4 - d3) / ((d4 - d3) + (d5 - d6));
        return b + v * (c - b);
    }

    let denom = 1.0 / (va + vb + vc);
    let v = vb * denom;
    let w = vc * denom;
    return a + v * ab + w * ac;
}

/// Returns the normal vector of the triangle `a, b, c`
#[must_use]
pub fn get_normal(tri: [Vec3; 3]) -> Vec3 {
    let [a, b, c] = tri;

    (b - a).cross(c - a).normalize()
}

/// Given a triangle `a, b, c`, and a point `p`, returns the barycentric coordinates of the point `p`.
///
/// https://gamedev.stackexchange.com/questions/23743/whats-the-most-efficient-way-to-find-barycentric-coordinates
#[must_use]
pub fn get_barycentric_coordinates(p: Vec3, tri: [Vec3; 3]) -> Vec3 {
    let [a, b, c] = tri;

    let normal = get_normal(tri);

    let area_abc = normal.dot((b - a).cross(c - a));
    let area_pbc = normal.dot((b - p).cross(c - p));
    let area_pca = normal.dot((c - p).cross(a - p));

    let x = area_pbc / area_abc;
    let y = area_pca / area_abc;

    let bary = Vec3::new(x, y, 1.0 - (x + y));

    bary
}

#[must_use]
#[derive(Debug, Clone, Copy)]
pub struct BoundingBox {
    pub min: Vec3,
    pub max: Vec3,
}

impl BoundingBox {
    pub const fn max() -> Self {
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
