#[path = "utils.rs"]
mod utils;

use std::{fs, path::Path, process::exit};

struct IntegrationCase {
    name: &'static str,
    run: fn() -> i32,
}

fn main() {
    let cases = [
        IntegrationCase {
            name: "kernel test images",
            run: run_kernel_test_images,
        },
        IntegrationCase {
            name: "userspace_boot",
            run: run_userspace_boot,
        },
    ];

    for case in cases {
        eprintln!("running integration test: {}", case.name);
        let exit_code = (case.run)();

        if exit_code != 0 {
            exit(exit_code);
        }
    }

    exit(0);
}

fn run_kernel_test_images() -> i32 {
    for kernel_test in utils::build_kernel_integration_tests() {
        eprintln!("running integration test: {}", kernel_test.display());
        let uefi_path = utils::create_uefi_image(&kernel_test);
        let exit_code = utils::run_qemu_test(&uefi_path);
        let _ = fs::remove_file(&uefi_path);

        if exit_code != 0 {
            return exit_code;
        }
    }

    0
}

fn run_userspace_boot() -> i32 {
    let kernel_paths = utils::build_kernel();
    let kernel_path = kernel_paths
        .first()
        .map(Path::new)
        .expect("kernel executable missing");
    let uefi_path = utils::create_uefi_image(kernel_path);
    let exit_code = utils::run_qemu_userspace_boot_test(&uefi_path);
    let _ = fs::remove_file(&uefi_path);
    exit_code
}
