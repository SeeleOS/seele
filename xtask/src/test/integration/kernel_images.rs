use super::{IntegrationTest, IntegrationTestResult};
use crate::run::{
    build::{BuildMode, build_kernel_with_mode},
    qemu::{create_uefi_image, run_qemu_test_capture},
};
use anyhow::{Context, Result};
use std::fs;

pub const KERNEL_IMAGES: KernelImages = KernelImages;

const KERNEL_TEST_IMAGES: &[&str] = &["boot", "interrupt_breakpoint", "memory", "syscall", "vfs"];

pub struct KernelImages;

impl IntegrationTest for KernelImages {
    fn name(&self) -> &'static str {
        "integration::kernel_images"
    }

    fn run(&self) -> Result<IntegrationTestResult> {
        for kernel_test in build_kernel_with_mode(BuildMode::IntegrationTests(KERNEL_TEST_IMAGES))?
        {
            let uefi_path = create_uefi_image(&kernel_test)?;
            let result = run_qemu_test_capture(&uefi_path)?;
            fs::remove_file(&uefi_path)
                .with_context(|| format!("failed to remove UEFI image {}", uefi_path.display()))?;
            if result.exit_code != 0 {
                return Ok(IntegrationTestResult {
                    exit_code: result.exit_code,
                    failure: result.failure,
                    output: result.serial_output,
                });
            }
        }
        Ok(IntegrationTestResult {
            exit_code: 0,
            failure: None,
            output: String::new(),
        })
    }
}
