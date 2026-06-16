use super::{IntegrationTest, IntegrationTestResult};
use crate::run::{
    build::build_kernel,
    qemu::{RunOptions, create_uefi_image, run_qemu_until_serial_condition_capture},
};
use anyhow::{Context, Result};
use std::{env, fs, path::Path, time::Duration};

pub const USERSPACE_BOOT: UserspaceBoot = UserspaceBoot;

pub struct UserspaceBoot;

impl IntegrationTest for UserspaceBoot {
    fn name(&self) -> &'static str {
        "integration::userspace_boot"
    }

    fn run(&self) -> Result<IntegrationTestResult> {
        let kernel_paths = build_kernel()?;
        let kernel_path = kernel_paths
            .first()
            .map(Path::new)
            .context("kernel executable missing")?;
        let uefi_path = create_uefi_image(kernel_path)?;
        let options = RunOptions::for_agent_run_without_timeout();
        let result = run_qemu_until_serial_condition_capture(
            &uefi_path,
            &options,
            qemu_test_timeout(),
            userspace_startup_observed,
        )?;
        fs::remove_file(&uefi_path)
            .with_context(|| format!("failed to remove UEFI image {}", uefi_path.display()))?;
        Ok(IntegrationTestResult {
            exit_code: result.exit_code,
            failure: result.failure,
            output: result.serial_output,
        })
    }
}

fn userspace_startup_observed(output: &str) -> bool {
    output.lines().any(is_shell_prompt_line)
}

fn is_shell_prompt_line(line: &str) -> bool {
    let line = line.trim_end_matches('\r');
    (line.contains("bash-") || line.contains("root@")) && line.contains("# ")
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
