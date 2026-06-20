use crate::{
    JobContext, KernelUnitReport,
    build::{KernelBuildMode, KernelBuildOptions, build_kernel},
    iso::{BootConfig, create_boot_iso},
    qemu::{VmConfig, run_iso_capture},
};
use anyhow::Result;
use std::{path::Path, time::Duration};

pub fn run(repo: &Path, context: &JobContext) -> Result<KernelUnitReport> {
    let kernels = build_kernel(
        repo,
        KernelBuildMode::UnitTest,
        KernelBuildOptions::default(),
        context,
    )?;
    let iso = create_boot_iso(repo, &kernels[0], &BootConfig::default(), context)?;
    let result = run_iso_capture(
        repo,
        &iso,
        VmConfig::for_repo(repo),
        Some(Duration::from_secs(10 * 60)),
        None,
        context,
    )?;
    Ok(KernelUnitReport {
        executable: kernels[0].clone(),
        iso: Some(iso),
        passed: result.exit_code == 0,
        serial_log: Some(result.serial_log),
        stdout: String::new(),
        stderr: String::new(),
    })
}
