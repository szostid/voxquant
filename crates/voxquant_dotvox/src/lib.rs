use voxquant_core::scene::Scene;
use voxquant_core::{Args, ColorMode};

mod serialization;
mod voxelization;

#[profiling::function]
pub fn voxelize_and_save(scene: Scene, args: &Args) -> anyhow::Result<()> {
    let largest_dim = scene.bounds.size().max_element();
    let scale = args.res as f32 / largest_dim;

    let voxel_bounds_size = scene.bounds.size() * scale;

    let center_offset = -(voxel_bounds_size / 2.0).round().as_ivec3() + 128;

    match args.color {
        ColorMode::Static => {
            let data = voxelization::voxelize(&scene, args.res, args.mode, !args.no_optimization);
            serialization::save_vox_static(data, &args.output, center_offset)?;
        }
        #[cfg(feature = "dynamic_palette")]
        ColorMode::Dynamic => {
            let data = voxelization::voxelize(&scene, args.res, args.mode, !args.no_optimization);
            serialization::save_vox_dynamic(data, &args.output, center_offset)?;
        }
    }

    Ok(())
}
