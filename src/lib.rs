#![warn(clippy::pedantic)]
#![warn(clippy::nursery)]
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_lossless,
    clippy::cast_precision_loss
)]

use anyhow::{Context as _, Result, bail};
use clap::{Parser, ValueEnum};
use image::{Rgba, RgbaImage};
use std::fmt::Display;
use std::path::{Path, PathBuf};
use std::time::Instant;

pub mod formats;
pub mod geometry;
pub mod scene;
pub mod voxelizer;

#[derive(Debug, Clone, Copy, ValueEnum)]
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

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum ColorMode {
    #[value(name = "static")]
    Static,
    #[cfg(feature = "dynamic_palette")]
    #[value(name = "dynamic")]
    Dynamic,
}

impl Display for ColorMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Static => f.write_str("static"),
            #[cfg(feature = "dynamic_palette")]
            Self::Dynamic => f.write_str("dynamic"),
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

#[derive(Clone, Copy, PartialEq, Eq)]
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

#[derive(Clone, Copy, PartialEq, Eq)]
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

pub fn voxelize(args: &Args) -> Result<()> {
    let input_type =
        InputType::from_file(&args.input).context("failed to infer input file type")?;
    let output_type =
        OutputType::from_file(&args.output).context("failed to infer output file type")?;

    let t0 = Instant::now();

    let scene = match input_type {
        InputType::GlbGltf => formats::gltf2::load_gltf(
            &args.input,
            args.base_scale,
            output_type == OutputType::MagicaVoxel,
        )
        .context("failed to load the input file")?,
    };

    let t1 = Instant::now();

    println!("Scene loaded in {}s", (t1 - t0).as_secs_f32());

    match output_type {
        OutputType::MagicaVoxel => formats::magicavoxel::voxelize_and_save(scene, args)?,
    }

    Ok(())
}

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
pub struct Args {
    /// The input file that will be voxelized
    #[arg(short, long)]
    pub input: PathBuf,

    /// The output file after voxelization
    #[arg(short, long)]
    pub output: PathBuf,

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

    /// The palette generation mode. Dynamic palette looks
    /// much better, but the static palette is much faster.
    ///
    /// Dynamic palette is only enabled if the feature `dynamic_palette`
    /// is enabled (enabled by default)
    #[cfg_attr(feature = "dynamic_palette", arg(long, default_value_t = ColorMode::Dynamic))]
    #[cfg_attr(not(feature = "dynamic_palette"), arg(long, default_value_t = ColorMode::Static))]
    pub color: ColorMode,

    /// With this option, if two triangles share a voxel,
    /// both voxels will be present in the output file
    /// (magicavoxel will likely present the last one)
    #[arg(long, default_value_t = false)]
    pub no_optimization: bool,
}
