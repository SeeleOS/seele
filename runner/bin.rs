mod utils;

use std::{fs, process::exit};

fn main() {
    let options = utils::RunOptions::from_env();
    let kernel = utils::build_kernel()
        .into_iter()
        .next()
        .expect("kernel binary missing");
    let uefi_path = utils::create_uefi_image(&kernel);
    let exit_code = utils::run_qemu(&uefi_path, &options);
    let _ = fs::remove_file(&uefi_path);
    exit(exit_code);
}
