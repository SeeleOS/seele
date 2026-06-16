use super::{IntegrationTest, IntegrationTestResult};
use crate::run::{
    build::{BuildMode, build_kernel_with_mode},
    qemu::{create_uefi_image, run_qemu_expect_serial_failure_capture},
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

    fn run(&self) -> Result<IntegrationTestResult> {
        if let Some(kernel_test) =
            build_kernel_with_mode(BuildMode::IntegrationTests(PANIC_HANDLER_IMAGE))?
                .into_iter()
                .next()
        {
            let uefi_path = create_uefi_image(&kernel_test)?;
            let result =
                run_qemu_expect_serial_failure_capture(&uefi_path, PANIC_HANDLER_PATTERN, 1)?;
            fs::remove_file(&uefi_path)
                .with_context(|| format!("failed to remove UEFI image {}", uefi_path.display()))?;
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
