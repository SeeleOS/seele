mod cargo_build;
mod cli;
mod qemu;
mod rootfs;
mod sysroot;
mod terminal;
mod vm;

use crate::{
    cargo_build::{BuildMode, build_kernel, build_kernel_tests, build_kernel_with_mode},
    cli::{Command, RootfsCommand},
    qemu::{
        RunOptions, create_uefi_image, run_qemu, run_qemu_expect_serial_failure, run_qemu_mcp,
        run_qemu_test, run_qemu_until_serial_condition,
    },
};
use anyhow::{Context, Result};
use std::{env, fs, path::Path, process::exit, time::Duration};

const KERNEL_TEST_IMAGES: &[&str] = &["boot", "interrupt_breakpoint", "memory", "syscall", "vfs"];
const PANIC_HANDLER_SMOKE: &[&str] = &["panic_handler"];
const PANIC_HANDLER_PATTERN: &str = "panic handler smoke";
const USERSPACE_STARTUP_PATTERNS: &[&str] = &[
    "Welcome to Arch Linux",
    "Reached target",
    "login",
    "systemd",
];

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
    match cli::parse(env::args().skip(1))? {
        Command::Run(options) => run(options),
        Command::McpRun => mcp_run(),
        Command::Test => test(),
        Command::IntegrationTest => integration_test(),
        Command::Rootfs(RootfsCommand::Build { override_disk }) => rootfs::build(override_disk),
        Command::SysrootMount => sysroot::mount(),
        Command::VmPs => vm::ps(),
    }
}

fn run(options: RunOptions) -> Result<i32> {
    let kernel = build_kernel()?
        .into_iter()
        .next()
        .context("kernel binary missing")?;
    let uefi_path = create_uefi_image(&kernel)?;
    let exit_code = run_qemu(&uefi_path, &options)?;
    fs::remove_file(&uefi_path)
        .with_context(|| format!("failed to remove UEFI image {}", uefi_path.display()))?;
    Ok(exit_code)
}

fn mcp_run() -> Result<i32> {
    let kernel = build_kernel()?
        .into_iter()
        .next()
        .context("kernel binary missing")?;
    let uefi_path = create_uefi_image(&kernel)?;
    let options = RunOptions::for_agent_run_without_timeout();
    let exit_code = run_qemu_mcp(&uefi_path, &options)?;
    let _ = fs::remove_file(&uefi_path);
    Ok(exit_code)
}

fn test() -> Result<i32> {
    let mut exit_code = 0;

    for kernel_test in build_kernel_tests()? {
        let uefi_path = create_uefi_image(&kernel_test)?;
        exit_code = run_qemu_test(&uefi_path)?;
        fs::remove_file(&uefi_path)
            .with_context(|| format!("failed to remove UEFI image {}", uefi_path.display()))?;

        if exit_code != 0 {
            break;
        }
    }

    Ok(exit_code)
}

fn integration_test() -> Result<i32> {
    for case in [
        (
            "kernel test images",
            run_kernel_test_images as fn() -> Result<i32>,
        ),
        ("userspace_boot", run_userspace_boot as fn() -> Result<i32>),
        (
            "panic_handler_smoke",
            run_panic_handler_smoke as fn() -> Result<i32>,
        ),
    ] {
        eprintln!("running integration test: {}", case.0);
        let exit_code = case.1()?;
        if exit_code != 0 {
            return Ok(exit_code);
        }
    }

    Ok(0)
}

fn run_kernel_test_images() -> Result<i32> {
    for kernel_test in build_kernel_with_mode(BuildMode::IntegrationTests(KERNEL_TEST_IMAGES))? {
        eprintln!("running integration test: {}", kernel_test.display());
        let uefi_path = create_uefi_image(&kernel_test)?;
        let exit_code = run_qemu_test(&uefi_path)?;
        fs::remove_file(&uefi_path)
            .with_context(|| format!("failed to remove UEFI image {}", uefi_path.display()))?;
        if exit_code != 0 {
            return Ok(exit_code);
        }
    }
    Ok(0)
}

fn run_userspace_boot() -> Result<i32> {
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

fn run_panic_handler_smoke() -> Result<i32> {
    for kernel_test in build_kernel_with_mode(BuildMode::IntegrationTests(PANIC_HANDLER_SMOKE))? {
        eprintln!("running integration test: {}", kernel_test.display());
        let uefi_path = create_uefi_image(&kernel_test)?;
        let exit_code = run_qemu_expect_serial_failure(&uefi_path, PANIC_HANDLER_PATTERN, 1)?;
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
        .and_then(|value| cli::parse_duration(&value))
        .unwrap_or_else(|| Duration::from_secs(60))
}
