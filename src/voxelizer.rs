use crate::io::{ImageOrColor, Mesh};
use crate::math::{closest_point_triangle, get_barycentric_coordinates};
use crate::octree::*;
use glam::*;

fn voxelize_wireframe(store: &mut Octree, shading: &Shading, tri_pos: [Vec3; 3]) {
    voxelize_line(store, shading, tri_pos[0], tri_pos[1]);
    voxelize_line(store, shading, tri_pos[1], tri_pos[2]);
    voxelize_line(store, shading, tri_pos[0], tri_pos[2]);
}

fn voxelize_triangle(store: &mut Octree, shading: &Shading, tri_pos: [Vec3; 3]) {
    const LINES: [(usize, usize); 3] = [(1, 2), (0, 2), (0, 1)];

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
fn voxelize_line(store: &mut Octree, shading: &Shading, p1: Vec3, p2: Vec3) {
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
            store.store(map_pos, color);
        }

        if map_pos == end {
            break;
        }

        let smallest = t_max.min_position();

        t_max[smallest] += t_delta[smallest];
        map_pos[smallest] += step[smallest];
    }
}

fn voxelize_point(store: &mut Octree, point: Vec3) {
    let point = point.round().as_ivec3();
    store.store(point, image::Rgba([32, 32, 32, 255]));
}

#[derive(Debug)]
struct TexturedShading<'a> {
    pub image: &'a image::RgbaImage,
    pub vertices: [Vec3; 3],
    pub uvs: [Vec2; 3],
}

#[derive(Debug)]
enum Shading<'a> {
    Texture(TexturedShading<'a>),
    Color(image::Rgba<u8>),
}

impl Shading<'_> {
    pub fn get_color(&self, map_pos: IVec3) -> image::Rgba<u8> {
        match self {
            Shading::Texture(texture) => {
                let point = closest_point_triangle(map_pos.as_vec3(), texture.vertices);

                let barycentric = get_barycentric_coordinates(point, texture.vertices);

                let mut texture_cords = (texture.uvs[0] * barycentric.x)
                    + (texture.uvs[1] * barycentric.y)
                    + (texture.uvs[2] * barycentric.z);

                texture_cords.x = texture_cords.x.rem_euclid(1.0);
                texture_cords.y = texture_cords.y.rem_euclid(1.0);

                let (x, y) = texture.image.dimensions();
                let x = (((x - 1) as f32) * texture_cords.x) as u32;
                let y = (((y - 1) as f32) * texture_cords.y) as u32;

                *texture.image.get_pixel(x, y)
            }

            Shading::Color(color) => *color,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum VoxelizationMode {
    Triangles,
    Lines,
    Points,
}

#[profiling::function]
pub fn voxelize(mesh: &Mesh, size: u32, mode: VoxelizationMode) -> Octree {
    let num_tris = mesh.triangles.len();

    // leave one voxel gap around model to allow for inside/outside checking
    let max_size = size - 1;
    let depth = 31 - (size + 1).leading_zeros();

    let largest_dim = mesh.bounds.size().max_element();

    let scale = max_size as f32 / largest_dim;

    let mut tree = Octree::new(depth);

    for tri in 0..num_tris {
        // we have to translate every vertex into a position relative to
        // the bounds of the storage, and then scaled to fit as well as
        // possible
        let vertices = mesh.triangles[tri]
            .map(|vertex| vertex - mesh.bounds.min)
            .map(|vertex| vertex * scale)
            .map(|vertex| vertex + Vec3::ONE);

        let mat_id = mesh.triangle_extras[tri][0].material_idx;
        let material = mesh
            .materials
            .get(mat_id as usize)
            .unwrap_or(&mesh.materials[0]);

        let shading = match material {
            ImageOrColor::Image(image) => {
                let uvs = mesh.triangle_extras[tri].map(|extras| extras.uv().unwrap());

                let texture = TexturedShading {
                    image,
                    vertices,
                    uvs,
                };

                Shading::Texture(texture)
            }
            ImageOrColor::Color(color) => Shading::Color(*color),
        };

        match mode {
            VoxelizationMode::Triangles => {
                voxelize_triangle(&mut tree, &shading, vertices);
            }
            VoxelizationMode::Lines => {
                voxelize_wireframe(&mut tree, &shading, vertices);
            }
            VoxelizationMode::Points => {
                for point in vertices {
                    voxelize_point(&mut tree, point);
                }
            }
        }
    }

    tree
}
