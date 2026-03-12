//! Core voxelization algorithms and storage traits.
use crate::geometry::Triangle;
use crate::scene::{Scene, WrapMode};
use glam::{IVec3, Vec2, Vec3, Vec4};
use image::RgbaImage;
use std::fmt;
use std::ops::Range;

/// Determines the topological style of the generated voxels.
#[derive(Debug, Clone, Copy)]
#[cfg_attr(feature = "clap", derive(clap::ValueEnum))]
pub enum VoxelizationMode {
    /// Voxelizes the whole triangle
    Triangles,
    /// Voxelizes the whole triangle, with fat voxelization, meaning
    /// that voxels are guaranteed to share faces (this can prevent
    /// unwanted leakage in some use cases)
    FatTriangles,
    /// Voxelizes only the wireframe of a triangle
    Wireframe,
    /// Voxelizes only the three vertices of a triangle
    Points,
}

impl fmt::Display for VoxelizationMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Triangles => f.write_str("triangles"),
            Self::FatTriangles => f.write_str("fat-triangles"),
            Self::Wireframe => f.write_str("wireframe"),
            Self::Points => f.write_str("points"),
        }
    }
}

/// A sink for generated voxel data.
///
/// Implement this trait to define how and where voxel output is stored.
pub trait VoxelStore {
    /// Appends a voxel at `pos` with a `color` to the storage.
    ///
    /// The function won't be called if a voxel is discarded because of the alpha
    /// threshold. The provided position is the global position (i.e. the `0,0,0`
    /// is the `min` position of the AABB of the scene) and not a position within
    /// the slice of the voxelized scene.
    fn add_voxel(&mut self, pos: [i32; 3], color: [u8; 4], is_emissive: bool);
}

/// Voxelizes the edges of the provided `triangle`.
#[inline]
fn voxelize_wireframe<T: VoxelStore>(
    store: &mut T,
    shading: &TriangleData,
    triangle: Triangle,
    range: Range<[i32; 3]>,
) {
    let [a, b, c] = triangle.unpack_vertices_to_glam();

    voxelize_line(store, shading, a, b, range.clone());
    voxelize_line(store, shading, b, c, range.clone());
    voxelize_line(store, shading, a, c, range);
}

/// Voxelizes the provided `triangle`.
#[inline]
#[expect(clippy::suboptimal_flops, reason = "FMA makes the function unreadable")]
fn voxelize_triangle<T: VoxelStore, const FAT: bool>(
    store: &mut T,
    shading: &TriangleData,
    triangle: Triangle,
    range: Range<[i32; 3]>,
) {
    // TLDR: we voxelize the triangle by flattening it onto some plane
    // and then by iterating over points on that plane, unflattening
    // them back onto the triangle
    //
    // Detailed description:
    //
    // We pick the plane as the axis-aligned plane on which the
    // triangle will take up the most area. the axis of the normal
    // of that plane is called `d` here. the two other axes are `u, v`
    //
    // We project the points onto that plane, creating a new 2D triangle
    // made up of the points `a, b, c`.
    //
    // If that triangle is very small, or very string-like, then the loop
    // will likely produce incomplete results (because of aliasing - similar
    // to fences in games without antialiasing) because it will miss too
    // many voxels, so we do a conservative rasterization by voxelizing
    // the wireframe of the triangle too.
    //
    // The rest of the algorythm consists of finding the bounds of this
    // 2D triangle and iterating over their bounding box - for every
    // picked point we find its barycentric coordinates and determine
    // if it is within the triangle.
    //
    // If a picked point lies within the triangle, we need to solve the
    // equation for a point that would lie on the plane defined by the
    // original triangle to determine the third (so-called depth) coordinate
    // of the point, and then we can derive the original coordinates of
    // the point and append it to the store.
    const EPSILON: f32 = -0.001;

    // conservative rasterization
    voxelize_wireframe(store, shading, triangle, range.clone());

    let normal = shading.precalc.normal();
    let [raw_a, raw_b, raw_c] = triangle.unpack_vertices_to_glam();

    let d_axis = normal.abs().max_position();
    let u_axis = (d_axis + 1) % 3;
    let v_axis = (d_axis + 2) % 3;

    let normal_u = normal[u_axis];
    let normal_v = normal[v_axis];
    let normal_d_inv = 1.0 / normal[d_axis];
    // plane constant (plane is defined by `P dot N = D`)
    let plane_d = normal.dot(raw_a);

    // project A, B, C onto the axis
    let a = Vec2::new(raw_a[u_axis], raw_a[v_axis]);
    let b = Vec2::new(raw_b[u_axis], raw_b[v_axis]);
    let c = Vec2::new(raw_c[u_axis], raw_c[v_axis]);

    let ab = b - a;
    let ac = c - a;

    // note: area of a triangle would technically be 1/2 * (AB x AC)
    // but since we're using the ratios anyways, the 1/2 would cancel
    // out (perp_dot is the cross product)
    let area = ab.perp_dot(ac);
    let area_inv = 1.0 / area;

    if area.abs() < f32::EPSILON {
        return;
    }

    let min = a.min(b).min(c).floor().as_ivec2();
    let max = a.max(b).max(c).ceil().as_ivec2();

    let u_start = min.x.max(range.start[u_axis]);
    let u_end = max.x.min(range.end[u_axis]);
    let v_start = min.y.max(range.start[v_axis]);
    let v_end = max.y.min(range.end[v_axis]);

    // how much the `d` could potentially change across one unit of `v` and `u`
    let delta_d = if FAT {
        0.5 * ((normal_u * normal_d_inv).abs() + (normal_v * normal_d_inv).abs())
    } else {
        0.0
    };

    for u in u_start..=u_end {
        for v in v_start..=v_end {
            let p = Vec2::new(u as f32 + 0.5, v as f32 + 0.5);
            let ap = p - a;

            let c_bary = ab.perp_dot(ap) * area_inv; // area of APB / area of ABC
            let b_bary = ap.perp_dot(ac) * area_inv; // area of APC / area of ABC
            let a_bary = 1.0 - c_bary - b_bary;

            if a_bary >= EPSILON && b_bary >= EPSILON && c_bary >= EPSILON {
                // we need to find the depth. we solve the equation of the plane defined by the
                // triangle to find the `d` (third/z) coordinate of a point `P` that lies on it:
                // N dot P = D
                // N.u * u + N.v * v + N.d * d = D
                // N.d * d = D - N.u * u - N.v * v
                // d = (D - N.u * u - N.v * v) / N.d
                // note that `plane_d` is the plane constant `D` from the equation above
                let depth = (plane_d - normal_u * p.x - normal_v * p.y) * normal_d_inv;

                let color = shading.sample_from_bary(Vec3::new(a_bary, b_bary, c_bary));

                if let Some(color) = color {
                    if FAT {
                        let d_min = (depth - delta_d).floor() as i32;
                        let d_max = (depth + delta_d).floor() as i32;

                        for d in d_min..=d_max {
                            let mut voxel_pos = [0; 3];
                            voxel_pos[u_axis] = u;
                            voxel_pos[v_axis] = v;
                            voxel_pos[d_axis] = d;

                            store.add_voxel(voxel_pos, color, shading.is_emissive());
                        }
                    } else {
                        let mut voxel_pos = [0; 3];
                        voxel_pos[u_axis] = u;
                        voxel_pos[v_axis] = v;
                        voxel_pos[d_axis] = depth.floor() as i32;

                        store.add_voxel(voxel_pos, color, shading.is_emissive());
                    }
                }
            }
        }
    }
}

/// Voxelizes a line going from `p1` to `p2` with the provided shading using a DDA algorythm
#[inline]
fn voxelize_line<T: VoxelStore>(
    store: &mut T,
    shading: &TriangleData,
    p1: Vec3,
    p2: Vec3,
    range: Range<[i32; 3]>,
) {
    let end = p2.floor().as_ivec3();
    let ray_pos = p1;

    let box_min = IVec3::from_array(range.start).as_vec3();
    let box_max = IVec3::from_array(range.end).as_vec3();

    let ray_dir = (p2 - p1).normalize();

    if !ray_dir.is_finite() {
        return;
    }

    let inv_dir = Vec3::ONE / ray_dir;

    let mut t_entry = 0.0_f32;
    let mut t_exit = (p2 - p1).length();

    for i in 0..3 {
        // line is parallel and its outside of the bounding box
        if ray_dir[i].abs() < f32::EPSILON && p1[i] < box_min[i] || p1[i] > box_max[i] {
            return;
        }

        let t0 = (box_min[i] - p1[i]) * inv_dir[i];
        let t1 = (box_max[i] - p1[i]) * inv_dir[i];

        let (t_near, t_far) = if inv_dir[i] < 0.0 { (t1, t0) } else { (t0, t1) };

        t_entry = t_entry.max(t_near);
        t_exit = t_exit.min(t_far);
    }

    if t_entry > t_exit {
        return;
    }

    if p1 == p2 {
        return;
    }

    let mut current_ray_pos = p1;
    if t_entry > 0.0 {
        current_ray_pos += ray_dir * t_entry;
    }
    let limit = t_exit + 0.01;

    let mut voxel_pos = current_ray_pos.floor().as_ivec3();

    let t_delta = inv_dir.abs();
    let step = ray_dir.signum().as_ivec3();

    let step_clamped = step.max(IVec3::ZERO);
    let next_pos = (voxel_pos + step_clamped).as_vec3();

    let mut t_max = (next_pos - ray_pos) * inv_dir;

    // safety bound
    let max_steps = (t_exit - t_entry) as u32 * 10;

    for _ in 0..max_steps {
        let color = shading.snap_and_get_color(voxel_pos);

        if let Some(color) = color {
            store.add_voxel(voxel_pos.to_array(), color, shading.is_emissive());
        }

        if voxel_pos == end {
            break;
        }

        let smallest = t_max.min_position();

        if t_max[smallest] > limit {
            break;
        }

        t_max[smallest] += t_delta[smallest];
        voxel_pos[smallest] += step[smallest];
    }
}

/// Voxelizes the points of the provided `triangle`
#[inline]
fn voxelize_points<T: VoxelStore>(store: &mut T, shading: &TriangleData, triangle: Triangle) {
    let [a, b, c] = triangle.vertices.map(|vertex| vertex.pos.map(|p| p as i32));

    if let Some(color) = shading.sample_from_bary(Vec3::X) {
        store.add_voxel(a, color, shading.is_emissive());
    }
    if let Some(color) = shading.sample_from_bary(Vec3::Y) {
        store.add_voxel(b, color, shading.is_emissive());
    }
    if let Some(color) = shading.sample_from_bary(Vec3::Z) {
        store.add_voxel(c, color, shading.is_emissive());
    }
}

#[inline]
#[must_use]
fn interpolate_color(colors: [[u8; 4]; 3], bary: Vec3) -> [u8; 4] {
    let c0 = Vec4::from_array(colors[0].map(|c| c as f32));
    let c1 = Vec4::from_array(colors[1].map(|c| c as f32));
    let c2 = Vec4::from_array(colors[2].map(|c| c as f32));

    let final_color = c0 * bary.x + c1 * bary.y + c2 * bary.z;

    final_color.as_u8vec4().to_array()
}

#[inline]
#[must_use]
fn multiply_colors(c1: [u8; 4], c2: [u8; 4]) -> [u8; 4] {
    std::array::from_fn(|i| ((c1[i] as u16 * c2[i] as u16) / 255) as u8)
}

struct TriangleTextureData<'a> {
    pub texture: &'a RgbaImage,
    pub uvs: [Vec2; 3],
    pub wrap: [WrapMode; 2],
}

struct TriangleData<'a> {
    precalc: TriangleInterpolator,
    vert_colors: [[u8; 4]; 3],
    base_color: [u8; 4],
    is_emissive: bool,
    texture: Option<TriangleTextureData<'a>>,
    alpha_threshold: Option<u8>,
}

impl TriangleData<'_> {
    #[inline]
    #[must_use]
    pub const fn is_emissive(&self) -> bool {
        self.is_emissive
    }

    #[inline]
    #[must_use]
    pub fn sample_from_bary(&self, mut bary: Vec3) -> Option<[u8; 4]> {
        bary = bary.max(Vec3::ZERO);

        let sum = bary.x + bary.y + bary.z;
        if sum > f32::EPSILON {
            bary /= sum;
        }

        let vertex_color = interpolate_color(self.vert_colors, bary);

        let base_color = match self.texture {
            Some(TriangleTextureData {
                texture,
                uvs,
                wrap: [wrap_u, wrap_v],
            }) => {
                let mut uv = (uvs[0] * bary.x) + (uvs[1] * bary.y) + (uvs[2] * bary.z);

                uv.x = wrap_u.apply(uv.x);
                uv.y = wrap_v.apply(uv.y);

                let (w, h) = texture.dimensions();
                let x = (((w - 1) as f32) * uv.x) as u32;
                let y = (((h - 1) as f32) * uv.y) as u32;

                let tex_color = texture.get_pixel(x, y).0;

                multiply_colors(tex_color, self.base_color)
            }
            None => self.base_color,
        };

        let color = multiply_colors(base_color, vertex_color);

        if let Some(threshold) = self.alpha_threshold
            && color[3] < threshold
        {
            return None;
        }

        Some(color)
    }

    #[inline]
    #[must_use]
    pub fn snap_and_get_color(&self, pos: IVec3) -> Option<[u8; 4]> {
        let bary = self.precalc.get_closest_barycentric(pos.as_vec3());

        self.sample_from_bary(bary)
    }
}

#[derive(Debug, Clone, Copy)]
struct TriangleInterpolator {
    /// `a`
    a: Vec3,

    /// `b - a`
    v0: Vec3,
    /// `c - a`
    v1: Vec3,

    /// `(b - a)^2`
    d00: f32,
    /// `(b - a)(c - a)`
    d01: f32,
    /// `(c - a)^2`
    d11: f32,

    /// Inverse determinant for Cramer's rule
    inv_det: f32,
}

impl TriangleInterpolator {
    #[inline]
    #[must_use]
    #[expect(clippy::suspicious_operation_groupings, reason = "???")]
    #[expect(
        clippy::suboptimal_flops,
        reason = "fma makes this unreadable, and it only influences precision, not performance"
    )]
    pub fn new(tri: Triangle) -> Self {
        let [a, b, c] = tri.unpack_vertices_to_glam();

        let v0 = b - a;
        let v1 = c - a;

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
            a,
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
    #[expect(
        clippy::suboptimal_flops,
        reason = "fma makes this unreadable, and it only influences precision, not performance"
    )]
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

/// A part of the scene.
pub struct SceneSlice<'a> {
    /// The original, whole scene
    pub scene: &'a Scene,
    /// The voxel range (in the scene's bounds!) that the scene
    /// spans over. Note that if you don't provide actual
    /// [`indices`](Self::indices) the voxelizer will still visit
    /// every triangle and discard most of it.
    pub range: Range<[i32; 3]>,
    /// The indices which the voxelizer should voxelize. Even if
    /// a triangle falls within the [`range`](Self::range), the
    /// voxelizer won't touch it. If no indices are provided,
    /// the voxelizer will visit every triangle in the scene, and
    /// discard most (if not all) of it.
    pub indices: Option<&'a [usize]>,
}

impl SceneSlice<'_> {
    fn for_each_triangle(&self, mut op: impl FnMut(Triangle)) {
        match self.indices {
            Some(indices) => {
                for &idx in indices {
                    op(self.scene.triangles[idx]);
                }
            }
            None => {
                for &tri in &self.scene.triangles {
                    op(tri);
                }
            }
        }
    }
}

/// Voxelizes a slice of a scene using the provided settings.
#[profiling::function]
pub fn voxelize_scene<T: VoxelStore>(
    store: &mut T,
    input: SceneSlice,
    mode: VoxelizationMode,
    size: u32,
) {
    let largest_dim = Vec3::from_array(input.scene.bounds.size()).max_element();
    let scale = size as f32 / largest_dim;

    input.for_each_triangle(|mut triangle| {
        for vertex in &mut triangle.vertices {
            vertex.pos =
                ((Vec3::from_array(vertex.pos) - Vec3::from_array(input.scene.bounds.min)) * scale)
                    .to_array();
        }

        let mat_id = triangle.material_index;

        let material = input
            .scene
            .materials
            .get(mat_id as usize)
            .unwrap_or(&input.scene.materials[0]);

        let texture = material.texturing.as_ref().map(|data| TriangleTextureData {
            texture: &data.texture,
            uvs: triangle.uvs().unwrap().map(Vec2::from_array),
            wrap: data.wrap_mode,
        });

        let shading = TriangleData {
            texture,
            precalc: TriangleInterpolator::new(triangle),
            vert_colors: triangle.colors(),
            is_emissive: material.emissive,
            base_color: material.base_color,
            alpha_threshold: material.alpha_threshold,
        };

        match mode {
            VoxelizationMode::Triangles => {
                voxelize_triangle::<T, false>(store, &shading, triangle, input.range.clone());
            }
            VoxelizationMode::FatTriangles => {
                voxelize_triangle::<T, true>(store, &shading, triangle, input.range.clone());
            }
            VoxelizationMode::Wireframe => {
                voxelize_wireframe(store, &shading, triangle, input.range.clone());
            }
            VoxelizationMode::Points => {
                voxelize_points(store, &shading, triangle);
            }
        }
    });
}
