#[path = "utils.rs"]
mod utils;

use std::{fs, process::exit};

fn main() {
    let mut exit_code = 0;

    for kernel_test in utils::build_kernel_integration_tests() {
        eprintln!("running integration test: {}", kernel_test.display());
        let uefi_path = utils::create_uefi_image(&kernel_test);
        exit_code = utils::run_qemu_test(&uefi_path);
        let _ = fs::remove_file(&uefi_path);

        if exit_code != 0 {
            break;
        }
    }

    exit(exit_code);
}
