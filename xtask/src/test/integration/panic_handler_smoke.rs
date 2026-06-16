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
    fn test_count(&self) -> usize {
        PANIC_HANDLER_IMAGE.len()
    }

    fn run(&self) -> Result<Vec<IntegrationTestResult>> {
        let mut results = Vec::new();
        for kernel_test in build_kernel_with_mode(BuildMode::IntegrationTests(PANIC_HANDLER_IMAGE))?
        {
            let uefi_path = create_uefi_image(&kernel_test)?;
            let result =
                run_qemu_expect_serial_failure_capture(&uefi_path, PANIC_HANDLER_PATTERN, 1)?;
            fs::remove_file(&uefi_path)
                .with_context(|| format!("failed to remove UEFI image {}", uefi_path.display()))?;
            results.push(IntegrationTestResult {
                name: "integration::panic_handler_smoke".to_string(),
                exit_code: result.exit_code,
                failure: result.failure,
                output: result.serial_output,
            });
        }
        Ok(results)
    }
}
