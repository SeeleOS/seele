pub mod build;
pub mod build_iso;
pub mod interaction;
pub mod qemu;
mod terminal;

use anyhow::{Context, Result};
use std::fs;

use self::{
    build::{BuildMode, BuildOptions, build_kernel, build_kernel_with_options},
    build_iso::create_boot_iso,
    qemu::{RunOptions, run_qemu, run_qemu_mcp},
};
use crate::reporter::{
    FinishStatus, HumanReporter, WorkflowReporter, finished, remove_file, started,
};

#[derive(Debug, Default, Clone, Copy)]
pub struct RunArgs {
    pub agent: bool,
}

#[derive(Debug, Default, Clone, Copy)]
pub struct McpRunConfig {
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

pub fn mcp_run(config: McpRunConfig, reporter: &dyn WorkflowReporter) -> Result<i32> {
    started(reporter, "mcp-run")?;
    let kernel = build_kernel_with_options(
        BuildMode::Run,
        reporter,
        BuildOptions {
            enable_profiling: config.enable_profiling,
        },
    )?
    .into_iter()
    .next()
    .context("kernel binary missing")?;
    let iso_path = create_boot_iso(&kernel)?;
    let options = RunOptions::for_agent_run_without_timeout();
    let exit_code = run_qemu_mcp(&iso_path, &options, reporter)?;
    remove_file(&iso_path, reporter)?;
    finished(
        reporter,
        "mcp-run",
        exit_code,
        FinishStatus::from_exit_code(exit_code),
    )?;
    Ok(exit_code)
}

fn run_kernel(options: RunOptions) -> Result<i32> {
    let reporter = HumanReporter;
    let kernel = build_kernel(&reporter)?
        .into_iter()
        .next()
        .context("kernel binary missing")?;
    let iso_path = create_boot_iso(&kernel)?;
    let exit_code = run_qemu(&iso_path, &options)?;
    fs::remove_file(&iso_path)
        .with_context(|| format!("failed to remove ISO image {}", iso_path.display()))?;
    Ok(exit_code)
}
