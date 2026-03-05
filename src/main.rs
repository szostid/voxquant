use anyhow::Result;
use clap::Parser as _;

fn main() -> Result<()> {
    rayon::ThreadPoolBuilder::new().build_global()?;

    let args = voxquant::Args::parse();
    voxquant::voxelize(&args)
}
