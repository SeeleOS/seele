#![allow(dead_code)]

use ovmf_prebuilt::{Arch, FileType, Prebuilt, Source};
use serde_json::Value;
use std::{
    env, fs,
    io::{self, BufRead, BufReader, Read, Seek, SeekFrom, Write},
    os::{fd::AsRawFd, unix::net::UnixStream},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

pub struct RunOptions {
    pub agent_mode: bool,
    agent_timeout: Option<String>,
    machine: String,
    cpu_model: String,
    smp: String,
    qemu_gdb: Option<String>,
    wait_for_gdb: bool,
    qemu_debug_log: Option<PathBuf>,
    qemu_debugcon: Option<PathBuf>,
}

pub enum BuildMode {
    Run,
    UnitTest,
    IntegrationTests(&'static [&'static str]),
}

impl RunOptions {
    pub fn from_env() -> Self {
        Self {
            agent_mode: env::args().any(|arg| arg == "--agent"),
            agent_timeout: env::var("SEELE_QEMU_TIMEOUT").ok(),
            machine: env::var("SEELE_QEMU_MACHINE").unwrap_or_else(|_| "q35".to_string()),
            cpu_model: env::var("SEELE_QEMU_CPU")
                .unwrap_or_else(|_| "host,+hypervisor,+kvmclock,+kvmclock-stable-bit".to_string()),
            smp: env::var("SEELE_QEMU_SMP").unwrap_or_else(|_| default_smp()),
            qemu_gdb: env::var("SEELE_QEMU_GDB").ok(),
            wait_for_gdb: env::var_os("SEELE_QEMU_WAIT_GDB").is_some(),
            qemu_debug_log: env::var_os("SEELE_QEMU_DEBUG_LOG").map(PathBuf::from),
            qemu_debugcon: env::var_os("SEELE_QEMU_DEBUGCON").map(PathBuf::from),
        }
    }

    fn for_tests() -> Self {
        Self {
            agent_mode: true,
            agent_timeout: env::var("SEELE_QEMU_TIMEOUT").ok(),
            machine: env::var("SEELE_QEMU_MACHINE").unwrap_or_else(|_| "q35".to_string()),
            cpu_model: env::var("SEELE_QEMU_CPU")
                .unwrap_or_else(|_| "host,+hypervisor,+kvmclock,+kvmclock-stable-bit".to_string()),
            smp: env::var("SEELE_QEMU_SMP").unwrap_or_else(|_| "1".to_string()),
            qemu_gdb: env::var("SEELE_QEMU_GDB").ok(),
            wait_for_gdb: env::var_os("SEELE_QEMU_WAIT_GDB").is_some(),
            qemu_debug_log: env::var_os("SEELE_QEMU_DEBUG_LOG").map(PathBuf::from),
            qemu_debugcon: env::var_os("SEELE_QEMU_DEBUGCON").map(PathBuf::from),
        }
    }

    pub fn for_agent_run_without_timeout() -> Self {
        Self {
            agent_mode: true,
            agent_timeout: None,
            machine: env::var("SEELE_QEMU_MACHINE").unwrap_or_else(|_| "q35".to_string()),
            cpu_model: env::var("SEELE_QEMU_CPU")
                .unwrap_or_else(|_| "host,+hypervisor,+kvmclock,+kvmclock-stable-bit".to_string()),
            smp: env::var("SEELE_QEMU_SMP").unwrap_or_else(|_| default_smp()),
            qemu_gdb: env::var("SEELE_QEMU_GDB").ok(),
            wait_for_gdb: env::var_os("SEELE_QEMU_WAIT_GDB").is_some(),
            qemu_debug_log: env::var_os("SEELE_QEMU_DEBUG_LOG").map(PathBuf::from),
            qemu_debugcon: env::var_os("SEELE_QEMU_DEBUGCON").map(PathBuf::from),
        }
    }
}

struct QemuRunContext {
    serial_log: PathBuf,
    tty_input_socket: PathBuf,
    debug_log: Option<PathBuf>,
    keep_debug_log: bool,
}

impl QemuRunContext {
    fn new(options: &RunOptions) -> Self {
        let serial_log = env::temp_dir().join(if options.agent_mode {
            "seele-agent-serial.log"
        } else {
            "seele-serial.log"
        });
        let tty_input_socket = env::var_os("SEELE_AGENT_TTY_SOCKET")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("/tmp/seele-agent-tty.sock"));
        let keep_debug_log = options.qemu_debug_log.is_some();
        let debug_log = options.qemu_debug_log.clone().or_else(|| {
            options
                .agent_mode
                .then(|| env::temp_dir().join("seele-agent-qemu.log"))
        });

        Self {
            serial_log,
            tty_input_socket,
            debug_log,
            keep_debug_log,
        }
    }
}

fn default_smp() -> String {
    thread::available_parallelism()
        .map(|count| count.get().min(8).to_string())
        .unwrap_or_else(|_| "1".to_string())
}

pub fn build_kernel() -> Vec<PathBuf> {
    build_kernel_with_mode(BuildMode::Run)
}

pub fn build_kernel_tests() -> Vec<PathBuf> {
    build_kernel_with_mode(BuildMode::UnitTest)
}

pub fn build_kernel_with_mode(mode: BuildMode) -> Vec<PathBuf> {
    let cargo = env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());
    let mut command = Command::new(cargo);
    command.arg(match mode {
        BuildMode::Run => "build",
        BuildMode::UnitTest | BuildMode::IntegrationTests(_) => "test",
    });
    command.args(["-p", "kernel", "--target", "x86_64-unknown-none"]);

    if !cfg!(debug_assertions) {
        command.arg("--release");
    }

    match mode {
        BuildMode::Run => {
            command.args(["--bin", "kernel"]);
        }
        BuildMode::UnitTest => {
            command.args([
                "--lib",
                "-Z",
                "build-std=core,alloc",
                "-Z",
                "panic-abort-tests",
                "--no-run",
            ]);
            command.env("RUSTFLAGS", append_rustflags());
        }
        BuildMode::IntegrationTests(tests) => {
            for test in tests {
                command.args(["--test", test]);
            }
            command.args([
                "-Z",
                "build-std=core,alloc",
                "-Z",
                "panic-abort-tests",
                "--no-run",
            ]);
            command.env("RUSTFLAGS", append_rustflags());
        }
    }

    command.arg("--message-format=json-render-diagnostics");
    command.stdout(Stdio::piped());
    command.stderr(Stdio::inherit());

    let mut child = command.spawn().expect("failed to start cargo");
    let stdout = child.stdout.take().expect("missing cargo stdout");
    let reader = BufReader::new(stdout);
    let mut executables = Vec::new();

    for line in reader.lines() {
        let line = line.expect("failed to read cargo output");
        if let Some(path) = handle_cargo_message(&line, &mode) {
            executables.push(path);
        }
    }

    let status = child.wait().expect("failed to wait on cargo");
    if !status.success() {
        std::process::exit(status.code().unwrap_or(1));
    }

    assert!(
        !executables.is_empty(),
        "kernel executable missing from cargo output"
    );
    executables
}

pub fn create_uefi_image(kernel_path: &Path) -> PathBuf {
    let image_path = kernel_path.with_extension("img");
    let _ = fs::remove_file(&image_path);

    let mut config = bootloader::BootConfig::default();
    config.frame_buffer_logging = false;

    bootloader::UefiBoot::new(kernel_path)
        .set_boot_config(&config)
        .create_disk_image(&image_path)
        .expect("failed to create UEFI disk image");
    image_path
}

pub fn run_qemu(uefi_path: &Path, options: &RunOptions) -> i32 {
    run_qemu_inner(uefi_path, options)
}

pub fn run_qemu_test(uefi_path: &Path) -> i32 {
    run_qemu_inner(uefi_path, &RunOptions::for_tests())
}

fn run_qemu_inner(uefi_path: &Path, options: &RunOptions) -> i32 {
    let context = QemuRunContext::new(options);
    let mut cmd = build_qemu_command(uefi_path, options, &context);
    let mut child = cmd.spawn().expect("failed to start qemu-system-x86_64");
    let background_done = Arc::new(AtomicBool::new(false));
    let serial_log_thread = {
        let serial_log = context.serial_log.clone();
        let done = background_done.clone();
        thread::spawn(move || stream_serial_log(&serial_log, &done))
    };
    let tty_input_thread = {
        let tty_input_socket = context.tty_input_socket.clone();
        let done = background_done.clone();
        thread::spawn(move || forward_terminal_input(&tty_input_socket, &done))
    };
    let status = child.wait().expect("failed to wait on qemu");
    background_done.store(true, Ordering::Release);
    let _ = serial_log_thread.join();
    let _ = tty_input_thread.join();
    cleanup_qemu_context(&context);
    let exit_code = match status.code().unwrap_or(1) {
        33 => 0,
        35 => 1,
        _ => {
            if let Some(path) = &context.debug_log {
                report_qemu_fault(path);
            }
            2
        }
    };
    cleanup_qemu_debug_log(&context);
    exit_code
}

pub fn run_qemu_until_serial_condition(
    uefi_path: &Path,
    options: &RunOptions,
    timeout: Duration,
    mut condition: impl FnMut(&str) -> bool,
) -> i32 {
    let context = QemuRunContext::new(options);
    let mut cmd = build_qemu_command(uefi_path, options, &context);
    let mut child = cmd.spawn().expect("failed to start qemu-system-x86_64");
    let background_done = Arc::new(AtomicBool::new(false));
    let tty_input_thread = {
        let tty_input_socket = context.tty_input_socket.clone();
        let done = background_done.clone();
        thread::spawn(move || forward_terminal_input(&tty_input_socket, &done))
    };
    let deadline = Instant::now() + timeout;
    let mut offset = 0;
    let mut serial_log = None;
    let mut captured = String::new();

    let exit_code = loop {
        if serial_log.is_none() {
            match fs::File::open(&context.serial_log) {
                Ok(opened) => serial_log = Some(opened),
                Err(_) => {}
            }
        }

        if let Some(file) = serial_log.as_mut() {
            captured.push_str(&drain_serial_log(file, &mut offset));
            if condition(&captured) {
                let _ = child.kill();
                let _ = child.wait();
                break 0;
            }
        }

        match child.try_wait() {
            Ok(Some(status)) => {
                eprintln!("qemu exited before serial condition was observed");
                if let Some(path) = &context.debug_log {
                    report_qemu_fault(path);
                }
                break status.code().unwrap_or(1).max(1);
            }
            Ok(None) => {}
            Err(err) => {
                eprintln!("failed to poll qemu: {err}");
                let _ = child.kill();
                let _ = child.wait();
                break 1;
            }
        }

        if Instant::now() >= deadline {
            eprintln!("timed out waiting for serial condition");
            let _ = child.kill();
            let _ = child.wait();
            break 1;
        }

        thread::sleep(Duration::from_millis(10));
    };

    background_done.store(true, Ordering::Release);
    let _ = tty_input_thread.join();
    cleanup_qemu_context(&context);
    cleanup_qemu_debug_log(&context);
    exit_code
}

fn build_qemu_command(uefi_path: &Path, options: &RunOptions, context: &QemuRunContext) -> Command {
    let root_disk = env::var_os("SEELE_ROOT_DISK")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("disk.img"));
    let mut cmd = if options.agent_mode {
        if let Some(timeout) = &options.agent_timeout {
            let mut timeout_cmd = Command::new("timeout");
            timeout_cmd.arg(timeout).arg("qemu-system-x86_64");
            timeout_cmd
        } else {
            Command::new("qemu-system-x86_64")
        }
    } else {
        Command::new("qemu-system-x86_64")
    };

    cmd.arg("-m").arg("4G");
    cmd.arg("-machine").arg(&options.machine);
    cmd.arg("-smp").arg(&options.smp);
    let _ = fs::remove_file(&context.serial_log);
    cmd.arg("-serial")
        .arg(format!("file:{}", context.serial_log.display()));
    if let Some(parent) = context.tty_input_socket.parent() {
        let _ = fs::create_dir_all(parent);
    }
    cleanup_socket(&context.tty_input_socket);
    eprintln!(
        "background terminal input path: {}",
        context.tty_input_socket.display()
    );
    cmd.arg("-serial").arg(format!(
        "unix:{},server=on,wait=off",
        context.tty_input_socket.display()
    ));
    cmd.arg("-monitor").arg("none");
    cmd.arg("-device")
        .arg("isa-debug-exit,iobase=0xf4,iosize=0x04");

    if let Some(endpoint) = &options.qemu_gdb {
        eprintln!("qemu gdb stub: {endpoint}");
        cmd.arg("-gdb").arg(endpoint);
        if options.wait_for_gdb {
            cmd.arg("-S");
        }
    }
    if let Some(path) = &options.qemu_debugcon {
        cmd.arg("-debugcon").arg(format!("file:{}", path.display()));
        cmd.arg("-global").arg("isa-debugcon.iobase=0xe9");
    }
    cmd.arg("-display")
        .arg(if options.agent_mode { "none" } else { "sdl" });

    if Path::new("/dev/kvm").exists() {
        cmd.arg("-enable-kvm");
        cmd.arg("-cpu").arg(&options.cpu_model);
    } else {
        eprintln!("warning: /dev/kvm not found, falling back to software emulation");
    }

    let prebuilt =
        Prebuilt::fetch(Source::LATEST, "target/ovmf").expect("failed to update prebuilt");
    let code = prebuilt.get_file(Arch::X64, FileType::Code);
    let vars = prebuilt.get_file(Arch::X64, FileType::Vars);

    cmd.arg("-drive").arg(format!(
        "if=none,format=raw,file={},id=bootdisk",
        uefi_path.display()
    ));
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
    if let Some(path) = &context.debug_log {
        cmd.arg("-d").arg("int,cpu_reset,guest_errors");
        cmd.arg("-D").arg(path);
    }
    cmd.arg("-drive").arg(format!(
        "if=pflash,format=raw,unit=1,file={},snapshot=on",
        vars.display()
    ));

    cmd
}

fn cleanup_qemu_context(context: &QemuRunContext) {
    let _ = fs::remove_file(&context.serial_log);
    cleanup_socket(&context.tty_input_socket);
}

fn cleanup_qemu_debug_log(context: &QemuRunContext) {
    if !context.keep_debug_log
        && let Some(path) = &context.debug_log
    {
        let _ = fs::remove_file(path);
    }
}

fn handle_cargo_message(line: &str, mode: &BuildMode) -> Option<PathBuf> {
    let value: Value = match serde_json::from_str(line) {
        Ok(value) => value,
        Err(_) => {
            println!("{line}");
            return None;
        }
    };

    match value.get("reason").and_then(Value::as_str) {
        Some("compiler-message") => {
            if let Some(rendered) = value["message"]["rendered"].as_str() {
                print!("{rendered}");
            }
            None
        }
        Some("compiler-artifact") => {
            let kind = value["target"]["kind"].as_array()?;
            let keep = match mode {
                BuildMode::Run => kind.iter().any(|item| item.as_str() == Some("bin")),
                BuildMode::UnitTest => {
                    kind.iter().any(|item| item.as_str() == Some("lib"))
                        && value["profile"]["test"].as_bool() == Some(true)
                }
                BuildMode::IntegrationTests(_) => {
                    kind.iter().any(|item| item.as_str() == Some("test"))
                        && value["profile"]["test"].as_bool() == Some(true)
                }
            };

            if !keep {
                return None;
            }

            value
                .get("executable")
                .and_then(Value::as_str)
                .map(PathBuf::from)
        }
        _ => None,
    }
}

fn append_rustflags() -> String {
    let extra = "-Zunstable-options -Cpanic=immediate-abort";
    match env::var("RUSTFLAGS") {
        Ok(existing) if !existing.trim().is_empty() => format!("{existing} {extra}"),
        _ => extra.to_string(),
    }
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
                    "failed to connect background terminal input path {}: {err}",
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
            Some(file) => drain_serial_log(file, &mut offset).len(),
            None => 0,
        };

        if done.load(Ordering::Acquire) && drained == 0 {
            break;
        }

        thread::sleep(Duration::from_millis(10));
    }
}

fn drain_serial_log(file: &mut fs::File, offset: &mut u64) -> String {
    if file.seek(SeekFrom::Start(*offset)).is_err() {
        return String::new();
    }

    let mut buffer = [0; 4096];
    let mut output = String::new();
    loop {
        match file.read(&mut buffer) {
            Ok(0) => break,
            Ok(read) => {
                *offset += read as u64;
                let chunk = String::from_utf8_lossy(&buffer[..read]);
                print!("{chunk}");
                let _ = io::stdout().flush();
                output.push_str(&chunk);
            }
            Err(_) => break,
        }
    }
    output
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
        raw.c_lflag &= !(libc::ICANON | libc::ECHO | libc::IEXTEN | libc::ISIG);
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
