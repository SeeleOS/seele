use crate::reporter::{WorkflowReporter, log_event};
use crate::run::{
    build::build_kernel,
    build_iso::create_boot_iso,
    qemu::{RunOptions, run_qemu_until_serial_condition_capture},
};
use anyhow::{Context, Result};
use std::{env, fs, path::Path, time::Duration};

pub const NAME: &str = "integration::userspace_boot";

pub fn run(reporter: &dyn WorkflowReporter) -> Result<i32> {
    let kernel_paths = build_kernel(reporter)?;
    let kernel_path = kernel_paths
        .first()
        .map(Path::new)
        .context("kernel executable missing")?;
    let iso_path = create_boot_iso(kernel_path)?;
    let options = RunOptions::for_agent_run_without_timeout();
    let result = run_qemu_until_serial_condition_capture(
        &iso_path,
        &options,
        qemu_test_timeout(),
        userspace_startup_observed,
    )?;
    fs::remove_file(&iso_path)
        .with_context(|| format!("failed to remove ISO image {}", iso_path.display()))?;
    if result.exit_code != 0 {
        log_failure(reporter, result.failure.as_deref(), &result.serial_output)?;
    }
    Ok(result.exit_code)
}

fn log_failure(reporter: &dyn WorkflowReporter, failure: Option<&str>, output: &str) -> Result<()> {
    if let Some(failure) = failure {
        log_event(reporter, "test", "stderr", failure)?;
    }
    if !output.is_empty() {
        log_event(reporter, "test", "serial", output)?;
    }
    if !reporter.capture_subprocess_output() {
        if let Some(failure) = failure {
            eprintln!("{failure}");
        }
        if !output.is_empty() {
            eprint!("{output}");
            if !output.ends_with('\n') {
                eprintln!();
            }
        }
    }
    Ok(())
}

fn userspace_startup_observed(output: &str) -> bool {
    output.lines().any(is_userspace_ready_line)
}

fn is_userspace_ready_line(line: &str) -> bool {
    let line = line.trim_end_matches('\r');
    is_shell_prompt_line(line) || is_login_prompt_line(line)
}

fn is_shell_prompt_line(line: &str) -> bool {
    (line.contains("bash-") || line.contains("root@")) && line.contains("# ")
}

fn is_login_prompt_line(line: &str) -> bool {
    line.trim_end().ends_with("Seele login:")
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
