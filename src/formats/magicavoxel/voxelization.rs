use crate::*;
use scene::Scene;
use voxelizer::{SceneSlice, VoxelStore};

use glam::{IVec3, U8Vec3};
use std::collections::HashMap;
use std::ops::Range;

pub trait VoxelType: Clone + Copy + PartialEq + Eq + Send + Sync + 'static {
    fn from_pos_color(pos: U8Vec3, color: Rgba<u8>) -> Self;
    fn pos(&self) -> U8Vec3;
}

/// 256x256x256 Chunk of a magicavoxel model
#[derive(Clone)]
pub struct Chunk<T: VoxelType> {
    pub voxels: Vec<T>,
    pub origin: IVec3,
}

impl<T: VoxelType> Chunk<T> {
    pub const fn new(origin: IVec3) -> Self {
        Self {
            voxels: Vec::new(),
            origin,
        }
    }

    pub fn range(&self) -> Range<IVec3> {
        self.origin..(self.origin + 256)
    }

    #[profiling::function]
    pub fn optimize(&mut self) {
        // i've found that sorting by the material index to ensure that during deduplication
        // we prefer brighter colors over dark colors makes everything look much nicer
        self.voxels.sort_unstable_by_key(|v| {
            let pos = v.pos();
            u32::from_be_bytes([0, pos.z, pos.y, pos.x])
        });

        self.voxels.dedup_by_key(|v| v.pos());
    }
}

impl<T: VoxelType> VoxelStore for Chunk<T> {
    fn add_voxel(&mut self, position: IVec3, color: Rgba<u8>) {
        let pos_in_chunk = position - self.origin;

        if let Ok(local) = U8Vec3::try_from(pos_in_chunk) {
            self.voxels.push(T::from_pos_color(local, color));
        }
    }
}

/// Groups the triangles of the scene into bins for every magicavoxel chunk
///
/// This is used to paralellize the voxelization (each chunk can be voxelized
/// independenty; we can use [`SceneSlice`])
#[profiling::function]
fn group_triangles(scene: &Scene, size: u32) -> HashMap<IVec3, Vec<usize>> {
    let mut chunks = HashMap::<IVec3, Vec<usize>>::new();

    let largest_dim = scene.bounds.size().max_element();
    let scale = size as f32 / largest_dim;

    for (idx, tri) in scene.triangles.iter().enumerate() {
        let [a, b, c] = tri
            .vertices
            .map(|vertex| vertex.pos - scene.bounds.min)
            .map(|vertex| vertex * scale);

        let min = a.min(b).min(c);
        let max = a.max(b).max(c);

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
pub fn voxelize<T: VoxelType>(
    mesh: &Scene,
    size: u32,
    mode: VoxelizationMode,
    optimize: bool,
) -> Vec<Chunk<T>> {
    use rayon::prelude::*;

    group_triangles(mesh, size)
        .into_par_iter()
        .map(|(chunk_idx, tris)| {
            let mut chunk = Chunk::new(chunk_idx * 256);

            let input = SceneSlice {
                mesh,
                range: chunk.range(),
                indices: Some(&tris),
            };

            voxelizer::voxelize_scene(&mut chunk, input, mode, size);

            if optimize {
                chunk.optimize();
            }

            chunk
        })
        .collect()
}
