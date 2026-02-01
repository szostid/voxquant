#![warn(clippy::pedantic)]
#![warn(clippy::nursery)]
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_lossless,
    clippy::cast_precision_loss
)]

use anyhow::{Context as _, Result, bail};
use clap::Parser;
use image::{Rgba, RgbaImage};
use std::fmt::Display;
use std::path::{Path, PathBuf};
use std::time::Instant;

mod formats;
mod geometry;
mod math;
mod scene;
mod voxelizer;

use geometry::{BoundingBox, Triangle};

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

/// Returns the extension of the file at `path`
fn get_extension(path: &Path) -> Result<&str> {
    path.extension()
        .context("failed to verify the file extension")?
        .to_str()
        .context("failed to convert file extension to str")
}

pub enum InputType {
    GlbGltf,
}

impl InputType {
    /// Derives the input type from the extension of the provided file
    ///
    /// # Errors
    /// Returns an error if the format is unsupported or if the file
    /// extension cannot be determined
    pub fn from_file(file: &Path) -> Result<Self> {
        let extension = get_extension(file)?;

        match extension {
            "gltf" | "glb" => Ok(Self::GlbGltf),
            _ => bail!("unknown file extension (only `.gltf` and `.glb` are supported)"),
        }
    }
}

pub enum OutputType {
    MagicaVoxel,
}

impl OutputType {
    /// Derives the output type from the extension of the provided file
    ///
    /// # Errors
    /// Returns an error if the format is unsupported or if the file
    /// extension cannot be determined
    pub fn from_file(file: &Path) -> Result<Self> {
        let extension = get_extension(file)?;

        match extension {
            "vox" => Ok(Self::MagicaVoxel),
            _ => bail!("unknown file extension (only `.vox` is supported)"),
        }
    }
}

pub fn voxelize_mesh(args: &Args) -> Result<()> {
    let input_type =
        InputType::from_file(&args.input).context("failed to infer input file type")?;
    let output_type =
        OutputType::from_file(&args.output).context("failed to infer output file type")?;

    let t0 = Instant::now();

    let mesh = match input_type {
        InputType::GlbGltf => formats::gltf2::load_gltf(&args.input, args.base_scale)
            .context("failed to load the input file")?,
    };

    let t1 = Instant::now();

    println!("Mesh loaded in {}s", (t1 - t0).as_secs_f32());

    let data = voxelizer::voxelize(&mesh, args.res, args.mode, !args.no_optimization);

    let t2 = Instant::now();

    println!("Mesh voxelized in {}s", (t2 - t1).as_secs_f32());

    match output_type {
        OutputType::MagicaVoxel => {
            // we offset the chunks bu the half the model size + 128 to ensure
            // that the model is centered on the magicavoxel stage
            let largest_dim = mesh.bounds.size().max_element();
            let scale = args.res as f32 / largest_dim;

            let voxel_bounds_size = mesh.bounds.size() * scale;

            let center_offset = -(voxel_bounds_size / 2.0).round().as_ivec3() + 128;

            formats::vox::save_vox(data, &args.output, center_offset)?;
        }
    }

    let t3 = Instant::now();

    println!("Mesh saved in {}s", (t3 - t2).as_secs_f32());

    Ok(())
}

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
pub struct Args {
    /// The input file that will be voxelized
    #[arg(short, long)]
    input: PathBuf,

    /// The output file after voxelization
    #[arg(short, long)]
    output: PathBuf,

    /// The resolution of the output model
    #[arg(short, long, default_value_t = 1024)]
    res: u32,

    /// The scale of the output model
    #[arg(long, default_value_t = 1.0)]
    base_scale: f32,

    /// The mode of voxelization. Defaults to triangles,
    /// but you can voxelize the wireframe or vertices
    /// instead.
    #[arg(long, default_value_t = VoxelizationMode::Triangles)]
    mode: VoxelizationMode,

    /// With this option, if two triangles share a voxel,
    /// both voxels will be present in the output file
    /// (magicavoxel will likely present the last one)
    #[arg(long, default_value_t = false)]
    no_optimization: bool,
}
