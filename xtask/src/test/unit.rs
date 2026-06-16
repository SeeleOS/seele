use crate::run::{
    build::build_kernel_tests,
    qemu::{create_uefi_image, run_qemu_test},
};
use anyhow::{Context, Result};
use std::fs;

pub fn run() -> Result<i32> {
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
