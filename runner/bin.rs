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
    let options = utils::RunOptions::from_env();
    let kernel = utils::build_kernel()?
        .into_iter()
        .next()
        .context("kernel binary missing")?;
    let uefi_path = utils::create_uefi_image(&kernel)?;
    let exit_code = utils::run_qemu(&uefi_path, &options)?;
    fs::remove_file(&uefi_path)
        .with_context(|| format!("failed to remove UEFI image {}", uefi_path.display()))?;
    Ok(exit_code)
}
