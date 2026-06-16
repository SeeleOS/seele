use super::IntegrationTest;
use crate::run::{
    build::{BuildMode, build_kernel_with_mode},
    qemu::{create_uefi_image, run_qemu_test},
};
use anyhow::{Context, Result};
use std::fs;

pub const KERNEL_IMAGES: KernelImages = KernelImages;

const KERNEL_TEST_IMAGES: &[&str] = &["boot", "interrupt_breakpoint", "memory", "syscall", "vfs"];

pub struct KernelImages;

impl IntegrationTest for KernelImages {
    fn name(&self) -> &'static str {
        "kernel test images"
    }

    fn run(&self) -> Result<i32> {
        for kernel_test in build_kernel_with_mode(BuildMode::IntegrationTests(KERNEL_TEST_IMAGES))?
        {
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
}
