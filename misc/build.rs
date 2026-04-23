use std::{
    env,
    fs::{self, File},
    path::{Path, PathBuf},
    process::Command,
};

fn main() {
    println!("cargo:rerun-if-changed=misc/build.rs");
    println!("cargo:rerun-if-changed=limine.conf");
    println!("cargo:rerun-if-changed=third_party/limine");
    println!("cargo:rerun-if-changed=third_party/limine-binary");
    println!("cargo:rerun-if-changed=disk.img");

    umount_sysroot();

    let out_dir = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    let kernel = PathBuf::from(env::var_os("CARGO_BIN_FILE_KERNEL_kernel").unwrap());
    let project_root = discover_project_root();
    let limine_dir = find_limine_dir(&project_root);
    let uefi_path = out_dir.join("uefi.img");

    create_limine_uefi_image(&uefi_path, &kernel, &project_root, &limine_dir);

    println!("cargo:rustc-env=UEFI_PATH={}", uefi_path.display());
}

fn find_limine_dir(project_root: &Path) -> PathBuf {
    let binary_dir = project_root.join("third_party/limine-binary");
    if binary_dir.join("BOOTX64.EFI").is_file() {
        return binary_dir;
    }

    let source_dir = project_root.join("third_party/limine");
    if source_dir.join("BOOTX64.EFI").is_file() {
        return source_dir;
    }

    panic!(
        "no usable limine binary tree found; expected BOOTX64.EFI under {} or {}",
        binary_dir.display(),
        source_dir.display()
    );
}

fn create_limine_uefi_image(
    image_path: &Path,
    kernel: &Path,
    project_root: &Path,
    limine_dir: &Path,
) {
    let _ = fs::remove_file(image_path);
    let file = File::create(image_path).expect("failed to create limine uefi image");
    file.set_len(64 * 1024 * 1024)
        .expect("failed to size limine uefi image");

    run(Command::new("mformat").arg("-i").arg(image_path).arg("-F").arg("::"));
    run(Command::new("mmd")
        .arg("-i")
        .arg(image_path)
        .arg("::/EFI")
        .arg("::/EFI/BOOT")
        .arg("::/boot")
        .arg("::/boot/limine"));
    run(Command::new("mcopy")
        .arg("-i")
        .arg(image_path)
        .arg(kernel)
        .arg("::/boot/kernel"));
    run(Command::new("mcopy")
        .arg("-i")
        .arg(image_path)
        .arg(project_root.join("limine.conf"))
        .arg("::/boot/limine/limine.conf"));
    run(Command::new("mcopy")
        .arg("-i")
        .arg(image_path)
        .arg(project_root.join("limine.conf"))
        .arg("::/EFI/BOOT/limine.conf"));
    run(Command::new("mcopy")
        .arg("-i")
        .arg(image_path)
        .arg(project_root.join("limine.conf"))
        .arg("::/limine.conf"));
    run(Command::new("mcopy")
        .arg("-i")
        .arg(image_path)
        .arg(limine_dir.join("BOOTX64.EFI"))
        .arg("::/EFI/BOOT/BOOTX64.EFI"));
}

fn run(command: &mut Command) {
    let status = command.status().expect("failed to execute build helper");
    assert!(status.success(), "build helper failed: {command:?}");
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
