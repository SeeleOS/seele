pub mod build;
pub mod build_iso;
pub mod interaction;
pub mod qemu;
mod terminal;

use anyhow::{Context, Result};
use clap::Args;
use std::fs;

use self::{
    build::{BuildMode, BuildOptions, build_kernel, build_kernel_with_options},
    build_iso::create_boot_iso,
    qemu::{RunOptions, run_qemu, run_qemu_mcp},
};
use crate::json_output::{JsonEvent, OutputMode, emit, remove_file};

#[derive(Debug, Args)]
pub struct RunArgs {
    #[arg(long)]
    pub agent: bool,
}

#[derive(Debug, Args)]
pub struct McpRunArgs {
    #[arg(long)]
    pub json_output: bool,

    #[arg(long)]
    pub enable_profiling: bool,
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

pub fn mcp_run(args: McpRunArgs) -> Result<i32> {
    let output_mode = if args.json_output {
        OutputMode::Json
    } else {
        OutputMode::Human
    };
    if output_mode.is_json() {
        emit(&JsonEvent::started("mcp-run"))?;
    }
    let kernel = build_kernel_with_options(
        BuildMode::Run,
        output_mode,
        BuildOptions {
            enable_profiling: args.enable_profiling,
        },
    )?
    .into_iter()
    .next()
    .context("kernel binary missing")?;
    let iso_path = create_boot_iso(&kernel)?;
    let options = RunOptions::for_agent_run_without_timeout();
    let exit_code = run_qemu_mcp(&iso_path, &options, output_mode)?;
    remove_file(&iso_path, output_mode)?;
    if output_mode.is_json() {
        emit(&JsonEvent::finished(
            "mcp-run",
            exit_code,
            if exit_code == 0 { "ok" } else { "failed" },
        ))?;
    }
    Ok(exit_code)
}

fn run_kernel(options: RunOptions) -> Result<i32> {
    let kernel = build_kernel(OutputMode::Human)?
        .into_iter()
        .next()
        .context("kernel binary missing")?;
    let iso_path = create_boot_iso(&kernel)?;
    let exit_code = run_qemu(&iso_path, &options)?;
    fs::remove_file(&iso_path)
        .with_context(|| format!("failed to remove ISO image {}", iso_path.display()))?;
    Ok(exit_code)
}
