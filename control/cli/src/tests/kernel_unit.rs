use super::config::RunTestsConfig;
use crate::{
    build::{KernelBuildMode, KernelBuildOptions, build_kernel, shell_for_repo},
    vm::{BootConfig, VmConfig, create_boot_iso, run_iso_capture},
};
use anyhow::Result;
use std::{path::Path, time::Duration};

pub fn run(repo: &Path, config: &RunTestsConfig) -> Result<bool> {
    eprintln!("==> running kernel unit tests");
    let sh = shell_for_repo(repo)?;
    let kernels = build_kernel(
        &sh,
        KernelBuildMode::UnitTest,
        KernelBuildOptions {
            enable_profiling: config.enable_profiling,
        },
    )?;
    let iso = create_boot_iso(&sh, repo, &kernels[0], &BootConfig::default())?;
    let result = run_iso_capture(
        &sh,
        repo,
        &iso,
        test_vm_config(repo),
        Some(Duration::from_secs(10 * 60)),
        None,
    )?;
    if let Some(failure) = &result.failure {
        eprintln!("kernel unit test VM: {failure}");
    }
    Ok(result.exit_code == 0)
}

fn test_vm_config(repo: &Path) -> VmConfig {
    let mut config = VmConfig::for_repo(repo);
    config.display = false;
    config
}
