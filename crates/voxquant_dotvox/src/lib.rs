//! `MagicaVoxel` support for [`voxquant_core`] through the [`dot_vox`](https://docs.rs/dot_vox/latest/dot_vox/) crate
use anyhow::Result;
use clap::{Args, ValueEnum};
use glam::{Mat4, Vec4};
use std::fmt;
use voxquant_core::io::SceneWriter;
use voxquant_core::scene::Scene;
use voxquant_core::{Format, OutputFormat, VoxelizationConfig};

mod serialization;
mod voxelization;

/// Determines the algorithm that assigns color indices to
/// generated voxel colors.
#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum ColorMode {
    /// The palette will be static, it will use the default palette defined
    /// by this crate (NOT the default magicavoxel palette!)
    #[value(name = "static")]
    Static,
    /// The palette will be determined using the colors present within the
    /// generated model. A quantization algorithm will assign colors to
    /// make the output file colors as accurate as possible
    #[cfg(feature = "dynamic_palette")]
    #[value(name = "dynamic")]
    Dynamic,
}

impl fmt::Display for ColorMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Static => f.write_str("static"),
            #[cfg(feature = "dynamic_palette")]
            Self::Dynamic => f.write_str("dynamic"),
        }
    }
}

#[profiling::function]
fn voxelize_and_write(
    scene: Scene,
    format_config: &DotVoxConfig,
    voxelization_config: &VoxelizationConfig,
    output: impl SceneWriter,
) -> Result<()> {
    let largest_dim = scene.bounds.size().max_element();
    let scale = voxelization_config.res as f32 / largest_dim;

    let voxel_bounds_size = scene.bounds.size() * scale;

    let center_offset = -(voxel_bounds_size / 2.0).round().as_ivec3() + 128;

    match format_config.color {
        ColorMode::Static => {
            let data = voxelization::voxelize(
                &scene,
                voxelization_config.res,
                voxelization_config.mode,
                !format_config.no_optimization,
            );

            serialization::write_vox_static(data, output, center_offset)?;
        }
        #[cfg(feature = "dynamic_palette")]
        ColorMode::Dynamic => {
            let data = voxelization::voxelize(
                &scene,
                voxelization_config.res,
                voxelization_config.mode,
                !format_config.no_optimization,
            );

            serialization::write_vox_dynamic(data, output, center_offset)?;
        }
    }

    Ok(())
}

/// Config for the [`DotVox`] voxelizer.
#[derive(Debug, Args)]
#[command(next_help_heading = "`.vox` format options")]
pub struct DotVoxConfig {
    /// The palette generation mode. Dynamic palette looks
    /// much better, but the static palette is much faster.
    ///
    /// Dynamic palette is only enabled if the feature `dynamic_palette`
    /// is enabled (the feature is enabled by default)
    #[cfg_attr(feature = "dynamic_palette", arg(long, default_value_t = ColorMode::Dynamic))]
    #[cfg_attr(not(feature = "dynamic_palette"), arg(long, default_value_t = ColorMode::Static))]
    pub color: ColorMode,

    /// With this option, if two triangles share a voxel,
    /// both voxels will be present in the output file
    /// (magicavoxel will likely present the last one)
    #[arg(long, default_value_t = false)]
    pub no_optimization: bool,
}

/// The definition of the output format.
///
/// NOTE: Does not use [`SceneWriter::base_path`]
/// at all. You may return [`None`].
pub struct DotVox;

impl Format for DotVox {
    // Z: up, Y: forward, X: right
    const BASIS: Mat4 = Mat4::from_cols(
        Vec4::new(1.0, 0.0, 0.0, 0.0),
        Vec4::new(0.0, 0.0, 1.0, 0.0),
        Vec4::new(0.0, 1.0, 0.0, 0.0),
        Vec4::new(0.0, 0.0, 0.0, 1.0),
    );
}

impl OutputFormat for DotVox {
    type Config = DotVoxConfig;

    fn voxelize_and_write<W: SceneWriter>(
        scene: Scene,
        output: W,
        format_config: Self::Config,
        voxelization_config: &VoxelizationConfig,
    ) -> Result<()> {
        voxelize_and_write(scene, &format_config, voxelization_config, output)
    }
}
