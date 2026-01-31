use crate::io::{ImageOrColor, Mesh};
use crate::*;
use dot_vox::Voxel;
use rayon::prelude::*;
use std::collections::HashMap;

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

fn voxelize_wireframe(store: &mut Chunk, shading: &Coloring, tri_pos: [Vec3; 3]) {
    voxelize_line(store, shading, tri_pos[0], tri_pos[1]);
    voxelize_line(store, shading, tri_pos[1], tri_pos[2]);
    voxelize_line(store, shading, tri_pos[0], tri_pos[2]);
}

fn voxelize_triangle(store: &mut Chunk, shading: &Coloring, tri_pos: [Vec3; 3]) {
    const LINES: [(usize, usize); 3] = [(1, 2), (0, 2), (0, 1)];

    // find the longest side and the indices of the two vectors it connects
    let (a, b, ab) = LINES
        .map(|(a, b)| (a, b, tri_pos[a].distance_squared(tri_pos[b])))
        .into_iter()
        .max_by(|(_, _, l1), (_, _, l2)| l1.total_cmp(l2))
        .map(|(a, b, ab)| (a, b, ab.sqrt()))
        .unwrap();

    let c = 3 - a - b;

    // ab is the longest line, c is the point that doesn't lay on it
    // we want to cast a bunch of lines from the point c onto the longest line ab

    let num_steps = (ab.ceil() as i32).max(1);
    let dir = (tri_pos[b] - tri_pos[a]) / num_steps as f32;

    for i in 0..=num_steps {
        let start = tri_pos[a] + dir * i as f32;
        voxelize_line(store, shading, start, tri_pos[c]);
    }
}

/// Voxelizes a line going from `p1` to `p2` with the provided shading using a DDA algorythm
fn voxelize_line(store: &mut Chunk, shading: &Coloring, p1: Vec3, p2: Vec3) {
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

fn voxelize_point(store: &mut Chunk, point: Vec3) {
    let point = point.round().as_ivec3();
    store.add_voxel(point, image::Rgba([32, 32, 32, 255]));
}

enum Coloring<'a> {
    Texture {
        image: &'a image::RgbaImage,
        vertices: [Vec3; 3],
        uvs: [Vec2; 3],
        vert_colors: [Color; 3],
    },
    Flat {
        base_color: Color,
        vertices: [Vec3; 3],
        vert_colors: [Color; 3],
    },
}

impl Coloring<'_> {
    pub fn get_color(&self, map_pos: IVec3) -> image::Rgba<u8> {
        let (vertices, vert_colors) = match self {
            Coloring::Texture {
                vertices,
                vert_colors,
                ..
            }
            | Coloring::Flat {
                vertices,
                vert_colors,
                ..
            } => (*vertices, *vert_colors),
        };

        let point = closest_point_triangle(map_pos.as_vec3(), vertices);
        let bary = get_barycentric_coordinates(point, vertices);

        let v_color = interpolate_color(vert_colors, bary);

        let base_color = match self {
            Coloring::Texture { image, uvs, .. } => {
                let mut uv = (uvs[0] * bary.x) + (uvs[1] * bary.y) + (uvs[2] * bary.z);

                uv.x = uv.x.rem_euclid(1.0);
                uv.y = uv.y.rem_euclid(1.0);

                let (w, h) = image.dimensions();
                let x = (((w - 1) as f32) * uv.x) as u32;
                let y = (((h - 1) as f32) * uv.y) as u32;

                *image.get_pixel(x, y)
            }
            Coloring::Flat { base_color, .. } => *base_color,
        };

        multiply_colors(base_color, v_color)
    }
}

#[derive(Debug, Clone, Copy)]
pub enum VoxelizationMode {
    Triangles,
    Lines,
    Points,
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

    for &tri in chunk_tris {
        // we have to translate every vertex into a position relative to
        // the bounds of the storage, and then scaled to fit as well as
        // possible
        let vertices = mesh.triangles[tri]
            .map(|vertex| vertex - mesh.bounds.min)
            .map(|vertex| vertex * scale);

        let extras = mesh.triangle_extras[tri];

        // material_idx should be uniform across extras so this shouldnt matter
        let mat_id = extras[0].material_idx;

        let material = mesh
            .materials
            .get(mat_id as usize)
            .unwrap_or(&mesh.materials[0]);

        let vert_colors = extras.map(|extra| image::Rgba(extra.color));

        let shading = match material {
            ImageOrColor::Image(image) => {
                let uvs = extras.map(|extras| extras.uv().unwrap());

                Coloring::Texture {
                    image,
                    vertices,
                    uvs,
                    vert_colors,
                }
            }
            ImageOrColor::Color(color) => Coloring::Flat {
                base_color: *color,
                vertices,
                vert_colors,
            },
        };

        match mode {
            VoxelizationMode::Triangles => {
                voxelize_triangle(&mut chunk, &shading, vertices);
            }
            VoxelizationMode::Lines => {
                voxelize_wireframe(&mut chunk, &shading, vertices);
            }
            VoxelizationMode::Points => {
                for point in vertices {
                    voxelize_point(&mut chunk, point);
                }
            }
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
