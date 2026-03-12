//! Format-agnostic voxelization library
//!
//! This provides only the core algorithms for describing and voxelizing scenes. The voxelizer needs an actual
//! implementation for a voxel storage ([`VoxelStore`](voxelizer::VoxelStore)) and it needs to be provided with
//! a slice of scene to voxelize ([`SceneSlice`](voxelizer::SceneSlice)) - the primary way of mutltithreading
//! voxelization is voxelizing in slices of the scene in chunks, and then composing the data into a whole scene.
//! For instance, `MagicaVoxel` requires chunks of 256^3 at most anyways, so that's the perfect place to
//! multithread the voxelization.
//!
//! Built for the [`voxquant`](https://docs.rs/voxquant/latest/voxquant/) CLI, but they can be used anywhere.
//! You can implement and use custom [`InputFormat`]s and [`OutputFormat`]s.
#![expect(
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

pub use image;

pub mod geometry;
pub mod io;
pub mod scene;
pub mod voxelizer;

/// Configuration required by the voxelizer.
#[derive(Args, Debug)]
#[command(next_help_heading = "Voxelization options")]
pub struct VoxelizationConfig {
    /// The resolution of the output model
    #[arg(short, long, default_value_t = 1024)]
    pub res: u32,

    /// The mode of voxelization. Defaults to triangles,
    /// but you can voxelize the wireframe or vertices
    /// instead.
    #[arg(long, default_value_t = VoxelizationMode::Triangles)]
    pub mode: VoxelizationMode,
}

/// Base trait for supported 3D file formats.
pub trait Format {
    /// The orthogonal basis matrix defining the format's coordinate system.
    ///
    /// You can translate from an input format's coordinate system into an output
    /// format's coordinate system with the `output_basis.inverse() * input_basis`
    /// matrix.
    const BASIS: Mat4;
}

/// Base trait for supported input file formats.
pub trait InputFormat: Format {
    /// The specific format config required by this format
    type Config;

    /// Loads the scene from the provided reader using this format.
    ///
    /// The scene will be transformed using the `transform_matrix`.
    ///
    /// # Errors
    /// This depends on the exact implementation of the format. Usually
    /// missing or malformed files or unsupported features will cause
    /// erros.
    fn read<R: io::SceneReader>(
        transform_matrix: Mat4,
        reader: R,
        format_config: Self::Config,
    ) -> Result<Scene>;

    /// Loads the scene from the file at `path` using this format.
    ///
    /// The scene will be transformed using the `transform_matrix`.
    ///
    /// # Errors
    /// This depends on the exact implementation of the format. Usually
    /// missing or malformed files or unsupported features will cause
    /// erros.
    fn load(transform_matrix: Mat4, path: &Path, format_config: Self::Config) -> Result<Scene> {
        let reader = io::LocalFile::open(path)?;

        Self::read(transform_matrix, reader, format_config)
    }
}

/// Base trait for supported output file formats.
pub trait OutputFormat: Format {
    /// The specific format config required by this format
    type Config;

    /// Voxelizes and writes the scene to `writer` using this format.
    ///
    /// # Errors
    /// This depends on the exact implementation of the format. Usually
    /// missing or malformed files or unsupported features will cause
    /// erros.
    fn voxelize_and_write<W: io::SceneWriter>(
        scene: Scene,
        writer: W,
        format_config: Self::Config,
        voxelization_config: &VoxelizationConfig,
    ) -> Result<()>;

    /// Voxelizes and saves the scene at `path` using this format.
    ///
    /// # Errors
    /// This depends on the exact implementation of the format. Usually
    /// missing or malformed files or unsupported features will cause
    /// erros.
    fn voxelize_and_save(
        scene: Scene,
        path: &Path,
        format_config: Self::Config,
        voxelization_config: &VoxelizationConfig,
    ) -> Result<()> {
        let writer = io::LocalFile::create(path)?;

        Self::voxelize_and_write(scene, writer, format_config, voxelization_config)
    }
}
