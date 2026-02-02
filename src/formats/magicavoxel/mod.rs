use crate::*;
use scene::Scene;

mod serialization;
mod voxelization;

#[profiling::function]
pub fn voxelize_and_save(scene: Scene, args: &Args) -> Result<()> {
    let largest_dim = scene.bounds.size().max_element();
    let scale = args.res as f32 / largest_dim;

    let voxel_bounds_size = scene.bounds.size() * scale;

    let center_offset = -(voxel_bounds_size / 2.0).round().as_ivec3() + 128;

    match args.color {
        ColorMode::Static => {
            let t1 = Instant::now();
            let data = voxelization::voxelize(&scene, args.res, args.mode, !args.no_optimization);
            let t2 = Instant::now();
            println!("Scene voxelized in {}s", (t2 - t1).as_secs_f32());
            serialization::save_vox_static(data, &args.output, center_offset)?;
            let t3 = Instant::now();
            println!("Scene saved in {}s", (t3 - t2).as_secs_f32());
        }
        #[cfg(feature = "dynamic_palette")]
        ColorMode::Dynamic => {
            let t1 = Instant::now();
            let data = voxelization::voxelize(&scene, args.res, args.mode, !args.no_optimization);
            let t2 = Instant::now();
            println!("Scene voxelized in {}s", (t2 - t1).as_secs_f32());
            serialization::save_vox_dynamic(data, &args.output, center_offset)?;
            let t3 = Instant::now();
            println!("Scene saved in {}s", (t3 - t2).as_secs_f32());
        }
    }

    Ok(())
}
