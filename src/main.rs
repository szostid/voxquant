#![warn(clippy::nursery)]
#![warn(clippy::pedantic)]

mod gltf2;
mod io;
mod math;
mod voxelizer;

use clap::Parser;
use voxelizer::{VoxelizationMode, voxelize};

use anyhow::{Context as _, Result, bail};
use math::*;

enum InputType {
    GlbGltf,
}

impl InputType {
    pub fn from_file(file: &str) -> Result<Self> {
        let extension = get_extension(file)?;

        match extension {
            "gltf" | "glb" => Ok(Self::GlbGltf),
            _ => bail!("unknown file extension (only `.gltf` and `.glb` are supported)"),
        }
    }
}

enum OutputType {
    MagicaVoxel,
}

impl OutputType {
    pub fn from_file(file: &str) -> Result<Self> {
        let extension = get_extension(file)?;

        match extension {
            "vox" => Ok(Self::MagicaVoxel),
            _ => bail!("unknown file extension (only `.vox` is supported)"),
        }
    }
}

fn voxelize_mesh(args: &Args) -> Result<()> {
    let input_type =
        InputType::from_file(&args.input).context("failed to infer input file type")?;
    let output_type =
        OutputType::from_file(&args.output).context("failed to infer output file type")?;

    let mesh = match input_type {
        InputType::GlbGltf => gltf2::load_gltf(&args.input, args.base_scale)
            .context("failed to load the input file")?,
    };

    println!("Mesh is loaded");

    let data = voxelize(&mesh, args.res, VoxelizationMode::Triangles);

    println!("Mesh is voxelized");

    match output_type {
        OutputType::MagicaVoxel => {
            io::save_as_magica_voxel(data, &args.output)?;
        }
    }

    println!("Mesh is saved");

    Ok(())
}

pub fn get_extension(path: &str) -> Result<&str> {
    std::path::Path::new(path)
        .extension()
        .context("failed to verify the file extension")?
        .to_str()
        .context("failed to convert file extension to str")
}

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    /// The input file that will be voxelized
    #[arg(short, long)]
    input: String,

    /// The output file after voxelization
    #[arg(short, long)]
    output: String,

    /// The resolution of the output model
    #[arg(long, default_value_t = 1024)]
    res: u32,

    /// The resolution of the output model
    #[arg(long, default_value_t = 1.0)]
    base_scale: f32,
}

fn main() -> Result<()> {
    tracy_client::Client::start();

    rayon::ThreadPoolBuilder::new()
        .num_threads(
            std::thread::available_parallelism()
                .map(|t| t.get())
                .unwrap_or(2),
        )
        .build_global()?;

    let args = Args::parse();
    voxelize_mesh(&args)
}
