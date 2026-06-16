pub mod build;
pub mod build_iso;
pub mod qemu;
mod terminal;

use anyhow::{Context, Result};
use clap::Args;
use std::fs;

use self::{
    build::build_kernel,
    build_iso::create_boot_iso,
    qemu::{RunOptions, run_qemu, run_qemu_mcp},
};

#[derive(Debug, Args)]
pub struct RunArgs {
    #[arg(long)]
    pub agent: bool,
}

impl RunArgs {
    fn into_options(self) -> RunOptions {
        let mut options = RunOptions::from_env();
        options.agent_mode = self.agent;
        options
    }
}

pub fn run(args: RunArgs) -> Result<i32> {
    run_kernel(args.into_options())
}

pub fn mcp_run() -> Result<i32> {
    let kernel = build_kernel()?
        .into_iter()
        .next()
        .context("kernel binary missing")?;
    let iso_path = create_boot_iso(&kernel)?;
    let options = RunOptions::for_agent_run_without_timeout();
    let exit_code = run_qemu_mcp(&iso_path, &options)?;
    let _ = fs::remove_file(&iso_path);
    Ok(exit_code)
}

fn run_kernel(options: RunOptions) -> Result<i32> {
    let kernel = build_kernel()?
        .into_iter()
        .next()
        .context("kernel binary missing")?;
    let iso_path = create_boot_iso(&kernel)?;
    let exit_code = run_qemu(&iso_path, &options)?;
    fs::remove_file(&iso_path)
        .with_context(|| format!("failed to remove ISO image {}", iso_path.display()))?;
    Ok(exit_code)
}
