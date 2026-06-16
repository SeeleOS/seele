use super::{IntegrationTest, IntegrationTestResult};
use crate::run::{
    build::{BuildMode, build_kernel_with_mode},
    qemu::{create_uefi_image, run_qemu_test_capture},
};
use anyhow::{Context, Result};
use std::{fs, path::Path};

pub const KERNEL_IMAGES: KernelImages = KernelImages;

const KERNEL_TEST_IMAGES: &[&str] = &["boot", "interrupt_breakpoint", "memory", "syscall", "vfs"];

pub struct KernelImages;

impl IntegrationTest for KernelImages {
    fn test_count(&self) -> usize {
        KERNEL_TEST_IMAGES.len()
    }

    fn run(&self) -> Result<Vec<IntegrationTestResult>> {
        let mut results = Vec::new();
        for kernel_test in build_kernel_with_mode(BuildMode::IntegrationTests(KERNEL_TEST_IMAGES))?
        {
            let uefi_path = create_uefi_image(&kernel_test)?;
            let result = run_qemu_test_capture(&uefi_path)?;
            fs::remove_file(&uefi_path)
                .with_context(|| format!("failed to remove UEFI image {}", uefi_path.display()))?;
            results.push(IntegrationTestResult {
                name: test_name(&kernel_test),
                exit_code: result.exit_code,
                failure: result.failure,
                output: result.serial_output,
            });
        }
        Ok(results)
    }
}

fn test_name(kernel_test: &Path) -> String {
    let name = kernel_test
        .file_stem()
        .and_then(|name| name.to_str())
        .unwrap_or("kernel_test");
    format!("integration::{name}")
}
