use super::{IntegrationTest, IntegrationTestResult};
use crate::reporter::WorkflowReporter;
use crate::run::{
    build::{BuildMode, BuildOptions, build_kernel_with_mode},
    build_iso::create_boot_iso,
    qemu::run_qemu_test_capture,
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

    fn run(&self, reporter: &dyn WorkflowReporter) -> Result<IntegrationTestResult> {
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
