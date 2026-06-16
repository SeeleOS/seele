use crate::run::{
    build::build_kernel_tests,
    qemu::{create_uefi_image, run_qemu_test_capture},
};
use anyhow::{Context, Result};
use std::fs;

pub fn run() -> Result<i32> {
    let mut exit_code = 0;

    for kernel_test in build_kernel_tests()? {
        let uefi_path = create_uefi_image(&kernel_test)?;
        let result = run_qemu_test_capture(&uefi_path)?;
        exit_code = result.exit_code;
        fs::remove_file(&uefi_path)
            .with_context(|| format!("failed to remove UEFI image {}", uefi_path.display()))?;

        if exit_code != 0 {
            if let Some(failure) = result.failure {
                eprintln!("{failure}");
            }
            if !result.serial_output.is_empty() {
                eprint!("{}", result.serial_output);
                if !result.serial_output.ends_with('\n') {
                    eprintln!();
                }
            }
            break;
        }
    }

    Ok(exit_code)
}
