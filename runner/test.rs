#[path = "utils.rs"]
mod utils;

use anyhow::{Context, Result};
use std::{fs, process::exit};

fn main() {
    match real_main() {
        Ok(code) => exit(code),
        Err(err) => {
            eprintln!("{err:?}");
            exit(1);
        }
    }
}

fn real_main() -> Result<i32> {
    let mut exit_code = 0;

    for kernel_test in utils::build_kernel_tests()? {
        let uefi_path = utils::create_uefi_image(&kernel_test)?;
        exit_code = utils::run_qemu_test(&uefi_path)?;
        fs::remove_file(&uefi_path)
            .with_context(|| format!("failed to remove UEFI image {}", uefi_path.display()))?;

        if exit_code != 0 {
            break;
        }
    }

    Ok(exit_code)
}
