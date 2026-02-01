use crate::io::{ImageOrColor, Mesh};
use crate::*;
use dot_vox::Voxel;
use rayon::prelude::*;
use std::collections::HashMap;
use std::fmt::Display;

pub struct Chunk {
    pub voxels: Vec<dot_vox::Voxel>,
    pub origin: IVec3,
}

impl Chunk {
    pub fn new(origin: IVec3) -> Self {
        Self {
            voxels: Vec::new(),
            origin,
        }
    }

    pub fn add_voxel(&mut self, position: IVec3, value: Color) {
        let pos_in_chunk = position - self.origin;

        let Ok(pos_in_chunk) = U8Vec3::try_from(pos_in_chunk) else {
            return;
        };

        // GLTF is Y-up magicavoxel is Z-up
        self.voxels.push(Voxel {
            x: pos_in_chunk.x,
            y: pos_in_chunk.z,
            z: pos_in_chunk.y,
            i: crate::io::magica::encode_color(value.0),
        });
    }
}

#[inline]
fn voxelize_wireframe(store: &mut Chunk, shading: &ColorData, tri_pos: [Vec3; 3]) {
    voxelize_line(store, shading, tri_pos[0], tri_pos[1]);
    voxelize_line(store, shading, tri_pos[1], tri_pos[2]);
    voxelize_line(store, shading, tri_pos[0], tri_pos[2]);
}

#[inline]
fn voxelize_triangle(store: &mut Chunk, shading: &ColorData, tri: [Vec3; 3]) {
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
    let plane_d = normal.dot(tri[0]);

    // project A, B, C onto the axis
    let a = Vec2::new(tri[0][u_axis], tri[0][v_axis]);
    let b = Vec2::new(tri[1][u_axis], tri[1][v_axis]);
    let c = Vec2::new(tri[2][u_axis], tri[2][v_axis]);

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

    for u in min.x..=max.x {
        for v in min.y..=max.y {
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
                store.add_voxel(voxel_pos, color.into());
            }
        }
    }
}

/// Voxelizes a line going from `p1` to `p2` with the provided shading using a DDA algorythm
#[inline]
fn voxelize_line(store: &mut Chunk, shading: &ColorData, p1: Vec3, p2: Vec3) {
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
        let color = shading.get_color(map_pos);

        // alpha cutoff
        if color.0[3] > 128 {
            store.add_voxel(map_pos, color.into());
        }

        if map_pos == end {
            break;
        }

        let smallest = t_max.min_position();

        t_max[smallest] += t_delta[smallest];
        map_pos[smallest] += step[smallest];
    }
}

#[inline]
fn voxelize_points(store: &mut Chunk, shading: &ColorData, points: [Vec3; 3]) {
    let [a, b, c] = points.map(|p| p.as_ivec3());

    store.add_voxel(a, shading.sample_from_bary(Vec3::X));
    store.add_voxel(b, shading.sample_from_bary(Vec3::Y));
    store.add_voxel(c, shading.sample_from_bary(Vec3::Z));
}

#[derive(Clone, Copy)]
enum AlbedoData<'a> {
    Texture {
        image: &'a image::RgbaImage,
        uvs: [Vec2; 3],
    },
    Flat(Color),
}

struct ColorData<'a> {
    precalc: TriangleData,
    vert_colors: [Color; 3],
    albedo: AlbedoData<'a>,
}

impl ColorData<'_> {
    pub fn sample_from_bary(&self, bary: Vec3) -> Color {
        let v_color = interpolate_color(self.vert_colors, bary);

        let base_color = match self.albedo {
            AlbedoData::Texture { image, uvs, .. } => {
                let mut uv = (uvs[0] * bary.x) + (uvs[1] * bary.y) + (uvs[2] * bary.z);

                uv.x = uv.x.rem_euclid(1.0);
                uv.y = uv.y.rem_euclid(1.0);

                let (w, h) = image.dimensions();
                let x = (((w - 1) as f32) * uv.x) as u32;
                let y = (((h - 1) as f32) * uv.y) as u32;

                *image.get_pixel(x, y)
            }
            AlbedoData::Flat(base_color) => base_color,
        };

        multiply_colors(base_color, v_color)
    }

    pub fn get_color(&self, map_pos: IVec3) -> image::Rgba<u8> {
        let bary = self.precalc.get_closest_barycentric(map_pos.as_vec3());

        self.sample_from_bary(bary)
    }
}

#[derive(Debug, Clone, Copy, clap::ValueEnum)]
pub enum VoxelizationMode {
    #[value(name = "triangles")]
    Triangles,
    #[value(name = "wireframe")]
    Wireframe,
    #[value(name = "points")]
    Points,
}

impl Display for VoxelizationMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Triangles => f.write_str("triangles"),
            Self::Wireframe => f.write_str("wireframe"),
            Self::Points => f.write_str("points"),
        }
    }
}

#[profiling::function]
fn voxelize_chunk(
    mesh: &Mesh,
    size: u32,
    chunk_tris: &[usize],
    chunk_base: IVec3,
    mode: VoxelizationMode,
) -> Chunk {
    let largest_dim = mesh.bounds.size().max_element();

    let scale = size as f32 / largest_dim;

    let mut chunk = Chunk::new(chunk_base);

    // we estimate early rallocations by just guessing
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

        let vert_colors = extras.map(|extra| image::Rgba(extra.color));

        let albedo = match material {
            ImageOrColor::Image(image) => {
                let uvs = extras.map(|extras| extras.uv().unwrap());

                AlbedoData::Texture { image, uvs }
            }
            ImageOrColor::Color(color) => AlbedoData::Flat(*color),
        };

        let shading = ColorData {
            precalc: TriangleData::new(vertices),
            vert_colors,
            albedo,
        };

        match mode {
            VoxelizationMode::Triangles => voxelize_triangle(&mut chunk, &shading, vertices),
            VoxelizationMode::Wireframe => voxelize_wireframe(&mut chunk, &shading, vertices),
            VoxelizationMode::Points => voxelize_points(&mut chunk, &shading, vertices),
        }
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

#[profiling::function]
pub fn voxelize(mesh: &Mesh, size: u32, mode: VoxelizationMode) -> Vec<Chunk> {
    group_triangles(mesh, size)
        .into_par_iter()
        .map(|(chunk_idx, tris)| voxelize_chunk(mesh, size, &tris, chunk_idx * 256, mode))
        .collect()
}
