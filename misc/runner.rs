use ovmf_prebuilt::{Arch, FileType, Prebuilt, Source};
use std::{
    env, fs,
    io::{self, Read, Seek, SeekFrom, Write},
    os::{fd::AsRawFd, unix::net::UnixStream},
    path::{Path, PathBuf},
    process::{Command, exit},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::Duration,
};

fn main() {
    let agent_mode = env::args().any(|arg| arg == "--agent");
    let agent_timeout = env::var("SEELE_QEMU_TIMEOUT").ok();
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
    let serial_log = env::temp_dir().join(if agent_mode {
        "seele-agent-serial.log"
    } else {
        "seele-serial.log"
    });
    let tty_input_socket = env::var_os("SEELE_AGENT_TTY_SOCKET")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/tmp/seele-agent-tty.sock"));
    let keep_debug_log = qemu_debug_log.is_some();
    let debug_log = qemu_debug_log
        .as_ref()
        .map(PathBuf::from)
        .or_else(|| agent_mode.then(|| env::temp_dir().join("seele-agent-qemu.log")));

    let mut cmd = if agent_mode {
        if let Some(timeout) = &agent_timeout {
            let mut timeout_cmd = Command::new("timeout");
            timeout_cmd.arg(timeout).arg("qemu-system-x86_64");
            timeout_cmd
        } else {
            Command::new("qemu-system-x86_64")
        }
    } else {
        Command::new("qemu-system-x86_64")
    };
    // give the guest 8 GiB of RAM
    cmd.arg("-m").arg("4G");
    cmd.arg("-machine").arg(&machine);
    cmd.arg("-smp").arg(&smp);
    let _ = fs::remove_file(&serial_log);
    cmd.arg("-serial")
        .arg(format!("file:{}", serial_log.display()));
    if let Some(parent) = tty_input_socket.parent() {
        let _ = fs::create_dir_all(parent);
    }
    cleanup_socket(&tty_input_socket);
    eprintln!("tty input socket: {}", tty_input_socket.display());
    cmd.arg("-serial").arg(format!(
        "unix:{},server=on,wait=off",
        tty_input_socket.display()
    ));
    cmd.arg("-monitor").arg("none");
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
    cmd.arg("-netdev").arg("user,id=net0");
    cmd.arg("-device")
        .arg("e1000,netdev=net0,mac=52:54:00:12:34:56");
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
    let background_done = Arc::new(AtomicBool::new(false));
    let serial_log_thread = {
        let serial_log = serial_log.clone();
        let done = background_done.clone();
        thread::spawn(move || stream_serial_log(&serial_log, &done))
    };
    let tty_input_thread = {
        let tty_input_socket = tty_input_socket.clone();
        let done = background_done.clone();
        thread::spawn(move || forward_terminal_input(&tty_input_socket, &done))
    };
    let status = child.wait().expect("failed to wait on qemu");
    background_done.store(true, Ordering::Release);
    let _ = serial_log_thread.join();
    let _ = tty_input_thread.join();
    let _ = fs::remove_file(serial_log);
    cleanup_socket(&tty_input_socket);
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

fn cleanup_socket(path: &Path) {
    let _ = fs::remove_file(path);
}

fn forward_terminal_input(socket_path: &Path, done: &AtomicBool) {
    let mut socket = loop {
        match UnixStream::connect(socket_path) {
            Ok(socket) => break socket,
            Err(_) if !done.load(Ordering::Acquire) => {
                thread::sleep(Duration::from_millis(10));
            }
            Err(err) => {
                eprintln!(
                    "failed to connect tty input socket {}: {err}",
                    socket_path.display()
                );
                return;
            }
        }
    };

    let stdin = io::stdin();
    let stdin_fd = stdin.as_raw_fd();
    let _terminal_mode = match TerminalInputModeGuard::new(stdin_fd) {
        Ok(mode) => mode,
        Err(err) => {
            eprintln!("failed to prepare terminal input forwarding: {err}");
            None
        }
    };
    let mut stdin = stdin.lock();
    let mut buffer = [0; 1024];

    loop {
        if done.load(Ordering::Acquire) {
            break;
        }

        match poll_stdin(stdin_fd, 10) {
            Ok(false) => continue,
            Ok(true) => {}
            Err(err) => {
                eprintln!("failed to poll terminal input: {err}");
                break;
            }
        }

        match stdin.read(&mut buffer) {
            Ok(0) => break,
            Ok(read) => {
                if socket.write_all(&buffer[..read]).is_err() {
                    break;
                }
            }
            Err(err) if err.kind() == io::ErrorKind::Interrupted => continue,
            Err(err) => {
                eprintln!("failed to read terminal input: {err}");
                break;
            }
        }
    }
}

fn stream_serial_log(serial_log: &Path, done: &AtomicBool) {
    let mut offset = 0;
    let mut file = None;

    loop {
        if file.is_none() {
            match fs::File::open(serial_log) {
                Ok(opened) => file = Some(opened),
                Err(_) if !done.load(Ordering::Acquire) => {
                    thread::sleep(Duration::from_millis(10));
                    continue;
                }
                Err(err) => {
                    eprintln!("failed to open serial log {}: {err}", serial_log.display());
                    break;
                }
            }
        }

        let drained = match file.as_mut() {
            Some(file) => drain_serial_log(file, &mut offset),
            None => 0,
        };

        if done.load(Ordering::Acquire) && drained == 0 {
            break;
        }

        thread::sleep(Duration::from_millis(10));
    }
}

fn drain_serial_log(file: &mut fs::File, offset: &mut u64) -> usize {
    if file.seek(SeekFrom::Start(*offset)).is_err() {
        return 0;
    }

    let mut buffer = [0; 4096];
    let mut total = 0;
    loop {
        match file.read(&mut buffer) {
            Ok(0) => break,
            Ok(read) => {
                total += read;
                *offset += read as u64;
                print!("{}", String::from_utf8_lossy(&buffer[..read]));
                let _ = io::stdout().flush();
            }
            Err(_) => break,
        }
    }
    total
}

fn poll_stdin(fd: i32, timeout_ms: i32) -> io::Result<bool> {
    let mut pollfd = libc::pollfd {
        fd,
        events: libc::POLLIN,
        revents: 0,
    };
    let result = unsafe { libc::poll(&mut pollfd, 1, timeout_ms) };
    if result < 0 {
        return Err(io::Error::last_os_error());
    }
    if result == 0 {
        return Ok(false);
    }
    if pollfd.revents & (libc::POLLERR | libc::POLLHUP | libc::POLLNVAL) != 0 {
        return Ok(false);
    }
    Ok(pollfd.revents & libc::POLLIN != 0)
}

struct TerminalInputModeGuard {
    fd: i32,
    original: libc::termios,
}

impl TerminalInputModeGuard {
    fn new(fd: i32) -> io::Result<Option<Self>> {
        if unsafe { libc::isatty(fd) } != 1 {
            return Ok(None);
        }

        let mut original = unsafe { std::mem::zeroed::<libc::termios>() };
        if unsafe { libc::tcgetattr(fd, &mut original) } != 0 {
            return Err(io::Error::last_os_error());
        }

        let mut raw = original;
        raw.c_iflag &= !(libc::BRKINT | libc::ICRNL | libc::INPCK | libc::ISTRIP | libc::IXON);
        raw.c_lflag &= !(libc::ICANON | libc::ECHO | libc::IEXTEN);
        raw.c_cc[libc::VMIN] = 1;
        raw.c_cc[libc::VTIME] = 0;

        if unsafe { libc::tcsetattr(fd, libc::TCSANOW, &raw) } != 0 {
            return Err(io::Error::last_os_error());
        }

        Ok(Some(Self { fd, original }))
    }
}

impl Drop for TerminalInputModeGuard {
    fn drop(&mut self) {
        unsafe {
            libc::tcsetattr(self.fd, libc::TCSANOW, &self.original);
        }
    }
}

fn report_qemu_fault(debug_log: &Path) {
    let Ok(contents) = fs::read_to_string(debug_log) else {
        return;
    };

    if contents.contains("Triple fault") {
        eprintln!("qemu: detected triple fault");
    }
}
