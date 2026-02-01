use crate::*;
use dot_vox::Voxel;
use glam::{IVec3, U8Vec3, Vec2, Vec3};
use io::{Mesh, WrapMode};
use rayon::prelude::*;
use std::collections::HashMap;

/// 256x256x256 Chunk of a magicavoxel model
pub struct Chunk {
    pub voxels: Vec<dot_vox::Voxel>,
    pub origin: IVec3,
}

impl Chunk {
    pub const fn new(origin: IVec3) -> Self {
        Self {
            voxels: Vec::new(),
            origin,
        }
    }

    pub fn add_voxel(&mut self, position: IVec3, color: Option<Rgba<u8>>) {
        let Some(color) = color else { return };

        let pos_in_chunk = position - self.origin;

        let Ok(pos_in_chunk) = U8Vec3::try_from(pos_in_chunk) else {
            return;
        };

        // GLTF is Y-up magicavoxel is Z-up
        self.voxels.push(Voxel {
            x: pos_in_chunk.x,
            y: pos_in_chunk.z,
            z: pos_in_chunk.y,
            i: crate::io::magica::encode_color(color.0),
        });
    }

    #[profiling::function]
    pub fn optimize(&mut self) {
        // i've found that sorting by the material index to ensure that during deduplication
        // we prefer brighter colors over dark colors makes everything look much nicer
        self.voxels
            .sort_unstable_by_key(|v| u32::from_be_bytes([v.z, v.y, v.x, 255 - v.i]));

        self.voxels.dedup_by_key(|v| (v.x, v.y, v.z));
    }
}

/// Voxelizes the edges of the provided `triangle`.
#[inline]
fn voxelize_wireframe(store: &mut Chunk, shading: &TriangleData, triangle: Triangle) {
    voxelize_line(store, shading, triangle[0], triangle[1]);
    voxelize_line(store, shading, triangle[1], triangle[2]);
    voxelize_line(store, shading, triangle[0], triangle[2]);
}

/// Voxelizes the provided `triangle`.
///
/// If the triangle is small, or string-like then this will instead fallback
/// to wireframe voxelization.
#[inline]
#[expect(clippy::similar_names, reason = "`u` vs `v` is quite clear")]
#[expect(clippy::suboptimal_flops, reason = "FMA makes the function unreadable")]
fn voxelize_triangle(
    store: &mut Chunk,
    shading: &TriangleData,
    triangle: Triangle,
    chunk_origin: IVec3,
) {
    // TLDR: we voxelize the triangle by flattening it onto some plane
    // and then by iterating over points on that plane, unflattening
    // them back onto the triangle
    //
    // Detailed description:
    //
    // we pick the plane as the axis-aligned plane on which the
    // triangle will take up the most area. the axis of the normal
    // of that plane is called `d` here. the two other axes are `u, v`
    //
    // we project the points onto that plane, creating a new 2D triangle
    // made up of the points `a, b, c`.
    //
    // if that triangle is very small, or very string-like, then the DDA
    // loop will likely produce incomplete results (because of aliasing,
    // similar to fences in some antialiasing implementations) because it
    // will miss too many voxels. in that case, we just voxelize the edges
    // of the triangles as lines and skip the normal voxelization loop.
    //
    // the rest of the algorythm consists of finding the bounds of this
    // 2D triangle and iterating over their bounding box. for every
    // picked point we find its barycentric coordinates and determine
    // if it is within the triangle.
    //
    // if a picked point lies within the triangle, we need to solve the
    // equation for a point that would lie on the plane defined by the
    // original triangle to determine the third (so-called depth) coordinate
    // of the point. then we can derive the original coordinates of the point
    // and append it to the store.
    let normal = shading.precalc.normal();

    let d_axis = normal.abs().max_position();
    let u_axis = (d_axis + 1) % 3;
    let v_axis = (d_axis + 2) % 3;

    let normal_u = normal[u_axis];
    let normal_v = normal[v_axis];
    let normal_d_inv = 1.0 / normal[d_axis];
    // plane constant (plane is defined by `P dot N = D`)
    let plane_d = normal.dot(triangle[0]);

    // project A, B, C onto the axis
    let a = Vec2::new(triangle[0][u_axis], triangle[0][v_axis]);
    let b = Vec2::new(triangle[1][u_axis], triangle[1][v_axis]);
    let c = Vec2::new(triangle[2][u_axis], triangle[2][v_axis]);

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

    {
        let bbox_size = max - min;
        let bbox_area = bbox_size.x * bbox_size.y;

        let is_tiny = bbox_size.x <= 2 || bbox_size.y <= 2;

        // we consider the triangle to be a thin string whenever
        // the area of the triangle within the bounding box is
        // much smaller than the area of the triangle (that is,
        // the triangle takes up very little of the space within
        // the bounding box)
        //
        // NOTE: no need to handle near-zero bbox_area because then
        // `is_tiny` will be true and we will voxelize as wifeframe
        // anyways
        let density = area.abs() / bbox_area as f32;
        let is_string = density < 0.05;

        if is_tiny || is_string {
            voxelize_wireframe(store, shading, triangle);
            return;
        }
    }

    let chunk_u_min = chunk_origin[u_axis];
    let chunk_u_max = chunk_u_min + 255;
    let chunk_v_min = chunk_origin[v_axis];
    let chunk_v_max = chunk_v_min + 255;

    let u_start = min.x.max(chunk_u_min);
    let u_end = max.x.min(chunk_u_max);
    let v_start = min.y.max(chunk_v_min);
    let v_end = max.y.min(chunk_v_max);

    for u in u_start..=u_end {
        for v in v_start..=v_end {
            let p = Vec2::new(u as f32 + 0.5, v as f32 + 0.5);
            let ap = p - a;

            let c_bary = ab.perp_dot(ap) * area_inv; // area of APB / area of ABC
            let b_bary = ap.perp_dot(ac) * area_inv; // area of APC / area of ABC
            let a_bary = 1.0 - c_bary - b_bary;

            if a_bary >= 0.0 && b_bary >= 0.0 && c_bary >= 0.0 {
                // we need to find the depth. we solve the equation of the plane defined by the
                // triangle to find the `d` (third/z) coordinate of a point `P` that lies on it:
                // N dot P = D
                // N.u * u + N.v * v + N.d * d = D
                // N.d * d = D - N.u * u - N.v * v
                // d = (D - N.u * u - N.v * v) / N.d
                // note that `plane_d` is the plane constant `D` from the equation above
                let depth = (plane_d - normal_u * p.x - normal_v * p.y) * normal_d_inv;

                let mut voxel_pos = IVec3::ZERO;
                voxel_pos[u_axis] = u;
                voxel_pos[v_axis] = v;
                voxel_pos[d_axis] = depth.round() as i32;

                let color = shading.sample_from_bary(Vec3::new(a_bary, b_bary, c_bary));
                store.add_voxel(voxel_pos, color);
            }
        }
    }
}

/// Voxelizes a line going from `p1` to `p2` with the provided shading using a DDA algorythm
#[inline]
fn voxelize_line(store: &mut Chunk, shading: &TriangleData, p1: Vec3, p2: Vec3) {
    let end = p2.as_ivec3();
    let ray_pos = p1;

    if p1 == p2 {
        return;
    }

    let ray_dir = (p2 - p1).normalize();

    if !ray_dir.is_finite() {
        return;
    }

    let inv_dir = Vec3::ONE / ray_dir;

    let mut map_pos = ray_pos.floor().as_ivec3();

    let t_delta = inv_dir.abs();
    let step = ray_dir.signum().as_ivec3();

    let step_clamped = step.max(IVec3::ZERO);
    let next_pos = (map_pos + step_clamped).as_vec3();

    let mut t_max = (next_pos - ray_pos) * inv_dir;

    loop {
        let color = shading.snap_and_get_color(map_pos);

        store.add_voxel(map_pos, color);

        if map_pos == end {
            break;
        }

        let smallest = t_max.min_position();

        t_max[smallest] += t_delta[smallest];
        map_pos[smallest] += step[smallest];
    }
}

/// Voxelizes the points of the provided `triangle`
#[inline]
fn voxelize_points(store: &mut Chunk, shading: &TriangleData, triangle: Triangle) {
    let [a, b, c] = triangle.map(|p| p.as_ivec3());

    store.add_voxel(a, shading.sample_from_bary(Vec3::X));
    store.add_voxel(b, shading.sample_from_bary(Vec3::Y));
    store.add_voxel(c, shading.sample_from_bary(Vec3::Z));
}

struct TriangleTextureData<'a> {
    pub texture: &'a RgbaImage,
    pub uvs: [Vec2; 3],
    pub wrap: [WrapMode; 2],
}

struct TriangleData<'a> {
    precalc: math::TriangleInterpolator,
    vert_colors: [Rgba<u8>; 3],
    base_color: Rgba<u8>,
    texture: Option<TriangleTextureData<'a>>,
    alpha_threshold: Option<u8>,
}

impl TriangleData<'_> {
    pub fn sample_from_bary(&self, mut bary: Vec3) -> Option<Rgba<u8>> {
        bary = bary.max(Vec3::ZERO);

        let sum = bary.x + bary.y + bary.z;
        if sum > f32::EPSILON {
            bary /= sum;
        }

        let vertex_color = math::interpolate_color(self.vert_colors, bary);

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

                let tex_color = *texture.get_pixel(x, y);

                math::multiply_colors(tex_color, self.base_color)
            }
            None => self.base_color,
        };

        let color = math::multiply_colors(base_color, vertex_color);

        if let Some(threshold) = self.alpha_threshold
            && color.0[3] < threshold
        {
            return None;
        }

        Some(color)
    }

    pub fn snap_and_get_color(&self, pos: IVec3) -> Option<Rgba<u8>> {
        let bary = self.precalc.get_closest_barycentric(pos.as_vec3());

        self.sample_from_bary(bary)
    }
}

#[profiling::function]
fn voxelize_chunk(
    mesh: &Mesh,
    size: u32,
    chunk_tris: &[usize],
    chunk_base: IVec3,
    mode: VoxelizationMode,
    optimize: bool,
) -> Chunk {
    let largest_dim = mesh.bounds.size().max_element();

    let scale = size as f32 / largest_dim;

    let mut chunk = Chunk::new(chunk_base);

    // we optimize early reallocations by just guessing
    // that every triangle generates about 50 voxels
    chunk.voxels.reserve(chunk_tris.len() * 50);

    for &tri in chunk_tris {
        // we have to translate every vertex into a position relative to
        // the bounds of the storage, and then scaled to fit as well as
        // possible
        let vertices = mesh.triangles[tri].map(|vertex| (vertex - mesh.bounds.min) * scale);

        let extras = mesh.triangle_extras[tri];

        // material_idx should be uniform across extras so this shouldnt matter
        let mat_id = extras[0].material_idx;

        let material = mesh
            .materials
            .get(mat_id as usize)
            .unwrap_or(&mesh.materials[0]);

        let texture = material.texturing.as_ref().map(|data| TriangleTextureData {
            texture: &data.texture,
            uvs: extras.map(|e| e.uv().unwrap()),
            wrap: data.wrap_mode,
        });

        let shading = TriangleData {
            texture,
            precalc: math::TriangleInterpolator::new(vertices),
            vert_colors: extras.map(|extra| Rgba(extra.color)),
            base_color: material.base_color,
            alpha_threshold: material.alpha_threshold,
        };

        match mode {
            VoxelizationMode::Triangles => {
                voxelize_triangle(&mut chunk, &shading, vertices, chunk_base);
            }
            VoxelizationMode::Wireframe => voxelize_wireframe(&mut chunk, &shading, vertices),
            VoxelizationMode::Points => voxelize_points(&mut chunk, &shading, vertices),
        }
    }

    if optimize {
        chunk.optimize();
    }

    chunk
}

#[profiling::function]
fn group_triangles(mesh: &Mesh, size: u32) -> HashMap<IVec3, Vec<usize>> {
    let mut chunks = HashMap::<IVec3, Vec<usize>>::new();

    let largest_dim = mesh.bounds.size().max_element();
    let scale = size as f32 / largest_dim;

    for (idx, tri) in mesh.triangles.iter().enumerate() {
        let voxel_verts = tri
            .map(|vertex| vertex - mesh.bounds.min)
            .map(|vertex| vertex * scale);

        let min = voxel_verts[0].min(voxel_verts[1]).min(voxel_verts[2]);
        let max = voxel_verts[0].max(voxel_verts[1]).max(voxel_verts[2]);

        let min_chunk = (min / 256.0).floor().as_ivec3();
        let max_chunk = (max / 256.0).floor().as_ivec3();

        for z in min_chunk.z..=max_chunk.z {
            for y in min_chunk.y..=max_chunk.y {
                for x in min_chunk.x..=max_chunk.x {
                    chunks.entry(IVec3::new(x, y, z)).or_default().push(idx);
                }
            }
        }
    }

    chunks
}

/// The core algorythm that voxelizes the mesh.
#[profiling::function]
pub fn voxelize(mesh: &Mesh, size: u32, mode: VoxelizationMode, optimize: bool) -> Vec<Chunk> {
    group_triangles(mesh, size)
        .into_par_iter()
        .map(|(chunk_idx, tris)| voxelize_chunk(mesh, size, &tris, chunk_idx * 256, mode, optimize))
        .collect()
}
