use std::{env, path::PathBuf, process::Command};

fn main() {
    println!("cargo:rerun-if-changed=misc/build.rs");
    println!("cargo:rerun-if-changed=disk.img");

    umount_sysroot();

    // set by cargo, build scripts should use this directory for output files
    let out_dir = PathBuf::from(std::env::var_os("OUT_DIR").unwrap());
    // set by cargo's artifact dependency feature, see
    // https://doc.rust-lang.org/nightly/cargo/reference/unstable.html#artifact-dependencies
    let kernel = PathBuf::from(std::env::var_os("CARGO_BIN_FILE_KERNEL_kernel").unwrap());

    // create an UEFI disk image (optional)
    let uefi_path = out_dir.join("uefi.img");
    bootloader::UefiBoot::new(&kernel)
        .create_disk_image(&uefi_path)
        .unwrap();

    // create a BIOS disk image
    let bios_path = out_dir.join("bios.img");
    bootloader::BiosBoot::new(&kernel)
        .create_disk_image(&bios_path)
        .unwrap();

    // pass the disk image paths as env variables to the
    println!("cargo:rustc-env=UEFI_PATH={}", uefi_path.display());
    println!("cargo:rustc-env=BIOS_PATH={}", bios_path.display());
}

fn umount_sysroot() {
    let project_root = discover_project_root();
    let sysroot = project_root.join("sysroot");

    let is_mounted = Command::new("mountpoint")
        .arg("-q")
        .arg(&sysroot)
        .status()
        .expect("failed to check whether sysroot is mounted")
        .success();

    if !is_mounted {
        return;
    }

    let status = Command::new("sudo")
        .arg("umount")
        .arg(&sysroot)
        .status()
        .expect("failed to unmount sysroot");

    assert!(
        status.success(),
        "failed to unmount mounted sysroot at {}",
        sysroot.display()
    );
}

fn discover_project_root() -> PathBuf {
    let cwd = env::current_dir().expect("failed to get current working directory");

    for dir in cwd.ancestors() {
        if dir.join("disk.img").is_file() && dir.join("sysroot").is_dir() {
            return dir.to_path_buf();
        }
    }

    panic!("could not locate project root from current working directory");
}
