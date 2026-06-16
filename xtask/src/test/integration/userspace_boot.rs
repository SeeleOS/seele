use super::IntegrationTest;
use crate::run::{
    build::build_kernel,
    qemu::{RunOptions, create_uefi_image, run_qemu_until_serial_condition},
};
use anyhow::{Context, Result};
use std::{env, fs, path::Path, time::Duration};

pub const USERSPACE_BOOT: UserspaceBoot = UserspaceBoot;

const USERSPACE_STARTUP_PATTERNS: &[&str] = &[
    "Welcome to Arch Linux",
    "Reached target",
    "login",
    "systemd",
];

pub struct UserspaceBoot;

impl IntegrationTest for UserspaceBoot {
    fn name(&self) -> &'static str {
        "userspace_boot"
    }

    fn run(&self) -> Result<i32> {
        let kernel_paths = build_kernel()?;
        let kernel_path = kernel_paths
            .first()
            .map(Path::new)
            .context("kernel executable missing")?;
        let uefi_path = create_uefi_image(kernel_path)?;
        let options = RunOptions::for_agent_run_without_timeout();
        let exit_code = run_qemu_until_serial_condition(
            &uefi_path,
            &options,
            qemu_test_timeout(),
            userspace_startup_observed,
        )?;
        if exit_code == 0 {
            eprintln!("integration test userspace_boot: startup signal observed");
        } else {
            eprintln!("integration test userspace_boot: startup signal not observed");
        }
        fs::remove_file(&uefi_path)
            .with_context(|| format!("failed to remove UEFI image {}", uefi_path.display()))?;
        Ok(exit_code)
    }
}

fn userspace_startup_observed(output: &str) -> bool {
    USERSPACE_STARTUP_PATTERNS
        .iter()
        .any(|pattern| output.contains(pattern))
}

fn qemu_test_timeout() -> Duration {
    env::var("SEELE_QEMU_TIMEOUT")
        .ok()
        .and_then(|value| parse_duration(&value))
        .unwrap_or_else(|| Duration::from_secs(60))
}

fn parse_duration(value: &str) -> Option<Duration> {
    let value = value.trim();
    if let Some(milliseconds) = value.strip_suffix("ms") {
        return milliseconds.parse::<u64>().ok().map(Duration::from_millis);
    }
    if let Some(seconds) = value.strip_suffix('s') {
        return seconds.parse::<u64>().ok().map(Duration::from_secs);
    }
    if let Some(minutes) = value.strip_suffix('m') {
        return minutes
            .parse::<u64>()
            .ok()
            .map(|minutes| Duration::from_secs(minutes.saturating_mul(60)));
    }
    value.parse::<u64>().ok().map(Duration::from_secs)
}
