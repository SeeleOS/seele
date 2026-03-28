use ovmf_prebuilt::{Arch, FileType, Prebuilt, Source};
use std::{
    env,
    path::{Path, PathBuf},
    process::Command,
    task::Context,
};

fn main() {
    umount_sysroot();

    // read env variables that were set in build script
    let uefi_path = env!("UEFI_PATH");

    let mut cmd = Command::new("qemu-system-x86_64");
    // give the guest 8 GiB of RAM
    cmd.arg("-m").arg("290M");
    // print serial output to the shell
    cmd.arg("-serial").arg("mon:stdio");
    // enable the guest to exit qemu
    cmd.arg("-device")
        .arg("isa-debug-exit,iobase=0xf4,iosize=0x04");

    if Path::new("/dev/kvm").exists() {
        cmd.arg("-enable-kvm");
        cmd.arg("-cpu").arg("host");
    } else {
        eprintln!("warning: /dev/kvm not found, falling back to software emulation");
    }

    let prebuilt =
        Prebuilt::fetch(Source::LATEST, "target/ovmf").expect("failed to update prebuilt");

    let code = prebuilt.get_file(Arch::X64, FileType::Code);
    let vars = prebuilt.get_file(Arch::X64, FileType::Vars);

    cmd.arg("-drive")
        .arg(format!("if=none,format=raw,file={uefi_path},id=bootdisk"));
    cmd.arg("-device").arg("virtio-blk-pci,drive=bootdisk");
    cmd.arg("-drive").arg(format!(
        "if=pflash,format=raw,unit=0,file={},readonly=on",
        code.display()
    ));
    // copy vars and enable rw instead of snapshot if you want to store data (e.g. enroll secure boot keys)
    cmd.arg("-drive").arg(format!(
        "if=pflash,format=raw,unit=1,file={},snapshot=on",
        vars.display()
    ));

    let mut child = cmd.spawn().expect("failed to start qemu-system-x86_64");
    let status = child.wait().expect("failed to wait on qemu");
    match status.code().unwrap_or(1) {
        0x10 => 0, // success
        0x11 => 1, // failure
        _ => 2,    // unknown fault
    };
}

fn umount_sysroot() {
    let project_root = discover_project_root();
    let sysroot = project_root.join("sysroot");

    Command::new("sudo")
        .arg("umount")
        .arg(&sysroot)
        .spawn()
        .unwrap();
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
