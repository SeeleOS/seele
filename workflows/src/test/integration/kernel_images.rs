use crate::reporter::{WorkflowReporter, log_event};
use crate::run::{
    build::{BuildMode, BuildOptions, build_kernel_with_mode},
    build_iso::create_boot_iso,
    qemu::run_qemu_test_capture,
};
use anyhow::{Context, Result};
use std::fs;

pub const NAME: &str = "integration::kernel_images";
const KERNEL_TEST_IMAGES: &[&str] = &["boot", "interrupt_breakpoint", "memory", "syscall", "vfs"];

pub fn run(reporter: &dyn WorkflowReporter) -> Result<i32> {
    for kernel_test in build_kernel_with_mode(
        BuildMode::IntegrationTests(KERNEL_TEST_IMAGES),
        reporter,
        BuildOptions::default(),
    )? {
        let iso_path = create_boot_iso(&kernel_test)?;
        let result = run_qemu_test_capture(&iso_path, reporter)?;
        fs::remove_file(&iso_path)
            .with_context(|| format!("failed to remove ISO image {}", iso_path.display()))?;
        if result.exit_code != 0 {
            log_failure(reporter, result.failure.as_deref(), &result.serial_output)?;
            return Ok(result.exit_code);
        }
    }
    Ok(0)
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
