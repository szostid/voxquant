#![warn(clippy::pedantic)]
#![warn(clippy::nursery)]
#![warn(clippy::cargo)]
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_lossless,
    clippy::cast_precision_loss
)]

use anyhow::Result;
use clap::Args;
use glam::Mat4;
use scene::Scene;
use std::path::Path;
use voxelizer::VoxelizationMode;

pub mod geometry;
pub mod scene;
pub mod voxelizer;

#[derive(Args, Debug)]
pub struct VoxelizationConfig {
    /// The resolution of the output model
    #[arg(short, long, default_value_t = 1024)]
    pub res: u32,

    /// The scale of the output model
    #[arg(long, default_value_t = 1.0)]
    pub base_scale: f32,

    /// The mode of voxelization. Defaults to triangles,
    /// but you can voxelize the wireframe or vertices
    /// instead.
    #[arg(long, default_value_t = VoxelizationMode::Triangles)]
    pub mode: VoxelizationMode,
}

pub trait Format {
    /// The orthogonal basis matrix defining the format's coordinate system.
    const BASIS: Mat4;
}

pub trait InputFormat: Format {
    type Config;

    fn load(
        transform_matrix: Mat4,
        path: &Path,
        format_config: Self::Config,
        voxelization_config: &VoxelizationConfig,
    ) -> Result<Scene>;
}

pub trait OutputFormat: Format {
    type Config;

    fn voxelize_and_save(
        scene: Scene,
        path: &Path,
        format_config: Self::Config,
        voxelization_config: &VoxelizationConfig,
    ) -> Result<()>;
}
