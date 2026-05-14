#[path = "utils.rs"]
mod utils;

use anyhow::{Context, Result};
use std::{env, time::Duration};
use std::{fs, path::Path, process::exit};

const KERNEL_TEST_IMAGES: &[&str] = &["boot", "interrupt_breakpoint", "memory", "syscall", "vfs"];
const PANIC_HANDLER_SMOKE: &[&str] = &["panic_handler"];
const PANIC_HANDLER_PATTERN: &str = "panic handler smoke";

const USERSPACE_STARTUP_PATTERNS: &[&str] = &[
    "Welcome to Arch Linux",
    "Reached target",
    "login",
    "systemd",
];

struct IntegrationCase {
    name: &'static str,
    run: fn() -> Result<i32>,
}

fn main() {
    match real_main() {
        Ok(code) => exit(code),
        Err(err) => {
            eprintln!("{err:?}");
            exit(1);
        }
    }
}

fn real_main() -> Result<i32> {
    let cases = [
        IntegrationCase {
            name: "kernel test images",
            run: run_kernel_test_images,
        },
        IntegrationCase {
            name: "userspace_boot",
            run: run_userspace_boot,
        },
        IntegrationCase {
            name: "panic_handler_smoke",
            run: run_panic_handler_smoke,
        },
    ];

    for case in cases {
        eprintln!("running integration test: {}", case.name);
        let exit_code = (case.run)()?;

        if exit_code != 0 {
            return Ok(exit_code);
        }
    }

    Ok(0)
}

fn run_kernel_test_images() -> Result<i32> {
    for kernel_test in
        utils::build_kernel_with_mode(utils::BuildMode::IntegrationTests(KERNEL_TEST_IMAGES))?
    {
        eprintln!("running integration test: {}", kernel_test.display());
        let uefi_path = utils::create_uefi_image(&kernel_test)?;
        let exit_code = utils::run_qemu_test(&uefi_path)?;
        fs::remove_file(&uefi_path)
            .with_context(|| format!("failed to remove UEFI image {}", uefi_path.display()))?;

        if exit_code != 0 {
            return Ok(exit_code);
        }
    }

    Ok(0)
}

fn run_userspace_boot() -> Result<i32> {
    let kernel_paths = utils::build_kernel()?;
    let kernel_path = kernel_paths
        .first()
        .map(Path::new)
        .context("kernel executable missing")?;
    let uefi_path = utils::create_uefi_image(kernel_path)?;
    let options = utils::RunOptions::for_agent_run_without_timeout();
    let exit_code = utils::run_qemu_until_serial_condition(
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

fn run_panic_handler_smoke() -> Result<i32> {
    for kernel_test in
        utils::build_kernel_with_mode(utils::BuildMode::IntegrationTests(PANIC_HANDLER_SMOKE))?
    {
        eprintln!("running integration test: {}", kernel_test.display());
        let uefi_path = utils::create_uefi_image(&kernel_test)?;
        let exit_code =
            utils::run_qemu_expect_serial_failure(&uefi_path, PANIC_HANDLER_PATTERN, 1)?;
        fs::remove_file(&uefi_path)
            .with_context(|| format!("failed to remove UEFI image {}", uefi_path.display()))?;

        if exit_code != 0 {
            return Ok(exit_code);
        }
    }

    Ok(0)
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
