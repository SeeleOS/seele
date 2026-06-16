use super::IntegrationTest;
use crate::run::{
    build::{BuildMode, build_kernel_with_mode},
    qemu::{create_uefi_image, run_qemu_expect_serial_failure},
};
use anyhow::{Context, Result};
use std::fs;

pub const PANIC_HANDLER_SMOKE: PanicHandlerSmoke = PanicHandlerSmoke;

const PANIC_HANDLER_IMAGE: &[&str] = &["panic_handler"];
const PANIC_HANDLER_PATTERN: &str = "panic handler smoke";

pub struct PanicHandlerSmoke;

impl IntegrationTest for PanicHandlerSmoke {
    fn name(&self) -> &'static str {
        "panic_handler_smoke"
    }

    fn run(&self) -> Result<i32> {
        for kernel_test in build_kernel_with_mode(BuildMode::IntegrationTests(PANIC_HANDLER_IMAGE))?
        {
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
}
