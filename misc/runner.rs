use ovmf_prebuilt::{Arch, FileType, Prebuilt, Source};
use std::{
    env, fs,
    path::{Path, PathBuf},
    process::{Command, exit},
};

fn main() {
    let agent_mode = env::args().any(|arg| arg == "--agent");

    // read env variables that were set in build script
    let uefi_path = env!("UEFI_PATH");
    let root_disk = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("disk.img");
    let serial_log = agent_mode.then(|| env::temp_dir().join("seele-agent-serial.log"));

    let mut cmd = if agent_mode {
        let mut timeout = Command::new("timeout");
        timeout.arg("10s").arg("qemu-system-x86_64");
        timeout
    } else {
        Command::new("qemu-system-x86_64")
    };
    // give the guest 8 GiB of RAM
    cmd.arg("-m").arg("4G");
    // print serial output to the shell
    if agent_mode {
        if let Some(serial_log) = &serial_log {
            let _ = fs::remove_file(serial_log);
            cmd.arg("-serial")
                .arg(format!("file:{}", serial_log.display()));
        }
        cmd.arg("-monitor").arg("none");
    } else {
        cmd.arg("-serial").arg("mon:stdio");
    }
    // enable the guest to exit qemu
    cmd.arg("-device")
        .arg("isa-debug-exit,iobase=0xf4,iosize=0x04");
    cmd.arg("-display")
        .arg(if agent_mode { "none" } else { "sdl" });

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
    cmd.arg("-device")
        .arg("virtio-blk-pci,drive=bootdisk,disable-legacy=on,disable-modern=off");
    if root_disk.exists() {
        cmd.arg("-drive").arg(format!(
            "if=none,format=raw,file={},id=rootdisk",
            root_disk.display()
        ));
        cmd.arg("-device")
            .arg("virtio-blk-pci,drive=rootdisk,disable-legacy=on,disable-modern=off");
    }
    cmd.arg("-drive").arg(format!(
        "if=pflash,format=raw,unit=0,file={},readonly=on",
        code.display()
    ));
    cmd.arg("-no-reboot").arg("-no-shutdown");
    // copy vars and enable rw instead of snapshot if you want to store data (e.g. enroll secure boot keys)
    cmd.arg("-drive").arg(format!(
        "if=pflash,format=raw,unit=1,file={},snapshot=on",
        vars.display()
    ));

    let mut child = cmd.spawn().expect("failed to start qemu-system-x86_64");
    let status = child.wait().expect("failed to wait on qemu");
    if let Some(serial_log) = serial_log {
        match fs::read_to_string(&serial_log) {
            Ok(contents) => {
                print!("{contents}");
            }
            Err(err) => {
                eprintln!("failed to read serial log {}: {err}", serial_log.display());
            }
        }
        let _ = fs::remove_file(serial_log);
    }
    let exit_code = match status.code().unwrap_or(1) {
        0x10 => 0, // success
        0x11 => 1, // failure
        _ => 2,    // unknown fault
    };
    exit(exit_code);
}
