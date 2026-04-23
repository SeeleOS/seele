use ovmf_prebuilt::{Arch, FileType, Prebuilt, Source};
use std::{
    env, fs,
    path::{Path, PathBuf},
    process::{Command, exit},
    thread,
};

fn main() {
    let agent_mode = env::args().any(|arg| arg == "--agent");
    let agent_timeout = env::var("SEELE_QEMU_TIMEOUT").unwrap_or_else(|_| "10s".to_string());
    let machine = env::var("SEELE_QEMU_MACHINE").unwrap_or_else(|_| "q35".to_string());
    let smp = env::var("SEELE_QEMU_SMP").unwrap_or_else(|_| {
        thread::available_parallelism()
            .map(|count| count.get().to_string())
            .unwrap_or_else(|_| "1".to_string())
    });
    let qemu_debug_log = env::var_os("SEELE_QEMU_DEBUG_LOG");
    let qemu_debugcon = env::var_os("SEELE_QEMU_DEBUGCON");

    // read env variables that were set in build script
    let uefi_path = env!("UEFI_PATH");
    let root_disk = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("disk.img");
    let serial_log = agent_mode.then(|| env::temp_dir().join("seele-agent-serial.log"));
    let keep_debug_log = qemu_debug_log.is_some();
    let debug_log = qemu_debug_log
        .as_ref()
        .map(PathBuf::from)
        .or_else(|| agent_mode.then(|| env::temp_dir().join("seele-agent-qemu.log")));

    let mut cmd = if agent_mode {
        let mut timeout = Command::new("timeout");
        timeout.arg(&agent_timeout).arg("qemu-system-x86_64");
        timeout
    } else {
        Command::new("qemu-system-x86_64")
    };
    // give the guest 8 GiB of RAM
    cmd.arg("-m").arg("4G");
    cmd.arg("-machine").arg(&machine);
    cmd.arg("-smp").arg(&smp);
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
    if let Some(path) = qemu_debugcon {
        cmd.arg("-debugcon")
            .arg(format!("file:{}", PathBuf::from(path).display()));
        cmd.arg("-global").arg("isa-debugcon.iobase=0xe9");
    }
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
    cmd.arg("-no-reboot").arg("-action").arg("reboot=shutdown");
    if let Some(path) = &debug_log {
        cmd.arg("-d").arg("int,cpu_reset,guest_errors");
        cmd.arg("-D").arg(path);
    }
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
        _ => {
            if let Some(path) = &debug_log {
                report_qemu_fault(path);
            }
            2
        } // unknown fault
    };
    if !keep_debug_log && let Some(path) = debug_log {
        let _ = fs::remove_file(path);
    }
    exit(exit_code);
}

fn report_qemu_fault(debug_log: &Path) {
    let Ok(contents) = fs::read_to_string(debug_log) else {
        return;
    };

    if contents.contains("Triple fault") {
        eprintln!("qemu: detected triple fault");
    }
}
