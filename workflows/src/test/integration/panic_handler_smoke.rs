use super::{IntegrationTest, IntegrationTestResult};
use crate::reporter::WorkflowReporter;
use crate::run::{
    build::{BuildMode, BuildOptions, build_kernel_with_mode},
    build_iso::create_boot_iso,
    qemu::run_qemu_expect_serial_failure_capture,
};
use anyhow::{Context, Result};
use std::fs;

pub const PANIC_HANDLER_SMOKE: PanicHandlerSmoke = PanicHandlerSmoke;

const PANIC_HANDLER_IMAGE: &[&str] = &["panic_handler"];
const PANIC_HANDLER_PATTERN: &str = "panic handler smoke";

pub struct PanicHandlerSmoke;

impl IntegrationTest for PanicHandlerSmoke {
    fn name(&self) -> &'static str {
        "integration::panic_handler_smoke"
    }

    fn run(&self, reporter: &dyn WorkflowReporter) -> Result<IntegrationTestResult> {
        if let Some(kernel_test) = build_kernel_with_mode(
            BuildMode::IntegrationTests(PANIC_HANDLER_IMAGE),
            reporter,
            BuildOptions::default(),
        )?
        .into_iter()
        .next()
        {
            let iso_path = create_boot_iso(&kernel_test)?;
            let result =
                run_qemu_expect_serial_failure_capture(&iso_path, PANIC_HANDLER_PATTERN, 1)?;
            fs::remove_file(&iso_path)
                .with_context(|| format!("failed to remove ISO image {}", iso_path.display()))?;
            Ok(IntegrationTestResult {
                exit_code: result.exit_code,
                failure: result.failure,
                output: result.serial_output,
            })
        } else {
            Ok(IntegrationTestResult {
                exit_code: 0,
                failure: None,
                output: String::new(),
            })
        }
    }
}
