use anyhow::Result;
use clap::Parser as _;

fn main() -> Result<()> {
    #[cfg(feature = "profiling")]
    tracy_client::Client::start();

    rayon::ThreadPoolBuilder::new().build_global()?;

    let args = voxquant::Args::parse();
    voxquant::voxelize(&args)
}
