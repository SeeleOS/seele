use super::terminal::{cleanup_socket, drain_serial_log, stream_serial_log};
use anyhow::{Context, Result};
use ovmf_prebuilt::{Arch, FileType, Prebuilt, Source};
use serde_json::json;
use std::{
    env, fs,
    io::Write,
    path::{Path, PathBuf},
    process::Command,
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

pub struct QemuTestResult {
    pub exit_code: i32,
    pub serial_output: String,
    pub failure: Option<String>,
}

struct QemuRunContext {
    serial_log: PathBuf,
    qmp_socket: PathBuf,
    debug_log: Option<PathBuf>,
    keep_debug_log: bool,
}

impl RunOptions {
    pub fn from_env() -> Self {
        Self {
            agent_mode: false,
            agent_timeout: env::var("SEELE_QEMU_TIMEOUT").ok(),
            machine: env::var("SEELE_QEMU_MACHINE").unwrap_or_else(|_| "q35".to_string()),
            cpu_model: env::var("SEELE_QEMU_CPU")
                .unwrap_or_else(|_| "host,+hypervisor,+kvmclock,+kvmclock-stable-bit".to_string()),
            smp: default_smp(),
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
            smp: default_smp(),
            qemu_gdb: env::var("SEELE_QEMU_GDB").ok(),
            wait_for_gdb: env::var_os("SEELE_QEMU_WAIT_GDB").is_some(),
            qemu_debug_log: env::var_os("SEELE_QEMU_DEBUG_LOG").map(PathBuf::from),
            qemu_debugcon: env::var_os("SEELE_QEMU_DEBUGCON").map(PathBuf::from),
        }
    }

    pub fn for_agent_run_without_timeout() -> Self {
        let mut options = Self::from_env();
        options.agent_mode = true;
        options.agent_timeout = None;
        options
    }
}

impl QemuRunContext {
    fn new(options: &RunOptions) -> Self {
        let serial_log = env::temp_dir().join(if options.agent_mode {
            "seele-agent-serial.log"
        } else {
            "seele-serial.log"
        });
        let qmp_socket = env::var_os("SEELE_QMP_SOCKET")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("/tmp/seele-agent-qmp.sock"));
        let keep_debug_log = options.qemu_debug_log.is_some();
        let debug_log = options.qemu_debug_log.clone().or_else(|| {
            options
                .agent_mode
                .then(|| env::temp_dir().join("seele-agent-qemu.log"))
        });

        Self {
            serial_log,
            qmp_socket,
            debug_log,
            keep_debug_log,
        }
    }
}

pub fn create_uefi_image(kernel_path: &Path) -> Result<PathBuf> {
    let image_path = kernel_path.with_extension("img");
    let _ = fs::remove_file(&image_path);

    let mut config = bootloader::BootConfig::default();
    config.frame_buffer_logging = false;

    bootloader::UefiBoot::new(kernel_path)
        .set_boot_config(&config)
        .create_disk_image(&image_path)
        .with_context(|| format!("failed to create UEFI image for {}", kernel_path.display()))?;
    Ok(image_path)
}

pub fn run_qemu(uefi_path: &Path, options: &RunOptions) -> Result<i32> {
    Ok(run_qemu_inner_capture(uefi_path, options, true)?.exit_code)
}

pub fn run_qemu_test_capture(uefi_path: &Path) -> Result<QemuTestResult> {
    run_qemu_inner_capture(uefi_path, &RunOptions::for_tests(), false)
}

pub fn run_qemu_expect_serial_failure_capture(
    uefi_path: &Path,
    serial_pattern: &str,
    expected_exit_code: i32,
) -> Result<QemuTestResult> {
    let options = RunOptions::for_tests();
    let context = QemuRunContext::new(&options);
    let mut cmd = build_qemu_command(uefi_path, &options, &context)?;
    let mut child = cmd.spawn().context("failed to start qemu-system-x86_64")?;

    let status = child.wait().context("failed to wait on qemu")?;

    let serial_output = fs::read_to_string(&context.serial_log).unwrap_or_default();
    let actual_exit_code = decode_qemu_exit_code(status.code(), &context)?;
    if !serial_output.contains(serial_pattern) {
        let failure = format!("expected serial pattern not observed: {serial_pattern}");
        cleanup_qemu_context(&context);
        cleanup_qemu_debug_log(&context);
        return Ok(QemuTestResult {
            exit_code: 1,
            serial_output,
            failure: Some(failure),
        });
    }
    if actual_exit_code != expected_exit_code {
        let failure = format!(
            "unexpected qemu exit code: expected {expected_exit_code}, got {actual_exit_code}"
        );
        cleanup_qemu_context(&context);
        cleanup_qemu_debug_log(&context);
        return Ok(QemuTestResult {
            exit_code: 1,
            serial_output,
            failure: Some(failure),
        });
    }

    cleanup_qemu_context(&context);
    cleanup_qemu_debug_log(&context);
    Ok(QemuTestResult {
        exit_code: 0,
        serial_output,
        failure: None,
    })
}

pub fn run_qemu_until_serial_condition_capture(
    uefi_path: &Path,
    options: &RunOptions,
    timeout: Duration,
    mut condition: impl FnMut(&str) -> bool,
) -> Result<QemuTestResult> {
    let context = QemuRunContext::new(options);
    let mut cmd = build_qemu_command(uefi_path, options, &context)?;
    let mut child = cmd.spawn().context("failed to start qemu-system-x86_64")?;
    let deadline = Instant::now() + timeout;
    let mut offset = 0;
    let mut serial_log = None;
    let mut captured = String::new();

    let (exit_code, failure) = loop {
        if serial_log.is_none()
            && let Ok(opened) = fs::File::open(&context.serial_log)
        {
            serial_log = Some(opened);
        }
        if let Some(file) = serial_log.as_mut() {
            captured.push_str(&drain_serial_log(file, &mut offset));
            if condition(&captured) {
                let _ = child.kill();
                let _ = child.wait();
                break (0, None);
            }
        }

        match child.try_wait() {
            Ok(Some(status)) => {
                if let Some(path) = &context.debug_log {
                    report_qemu_fault(path)?;
                }
                break (
                    status.code().unwrap_or(1).max(1),
                    Some("qemu exited before serial condition was observed".to_string()),
                );
            }
            Ok(None) => {}
            Err(err) => {
                let _ = child.kill();
                let _ = child.wait();
                break (1, Some(format!("failed to poll qemu: {err}")));
            }
        }

        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            break (
                1,
                Some("timed out waiting for serial condition".to_string()),
            );
        }

        thread::sleep(Duration::from_millis(10));
    };

    cleanup_qemu_context(&context);
    cleanup_qemu_debug_log(&context);
    Ok(QemuTestResult {
        exit_code,
        serial_output: captured,
        failure,
    })
}

fn run_qemu_inner_capture(
    uefi_path: &Path,
    options: &RunOptions,
    stream_output: bool,
) -> Result<QemuTestResult> {
    let context = QemuRunContext::new(options);
    let mut cmd = build_qemu_command(uefi_path, options, &context)?;
    let mut child = cmd.spawn().context("failed to start qemu-system-x86_64")?;
    let background_done = Arc::new(AtomicBool::new(false));
    let serial_log_thread = stream_output.then(|| {
        let serial_log = context.serial_log.clone();
        let done = background_done.clone();
        thread::spawn(move || stream_serial_log(&serial_log, &done))
    });
    let status = child.wait().context("failed to wait on qemu")?;
    background_done.store(true, Ordering::Release);
    if let Some(serial_log_thread) = serial_log_thread {
        let _ = serial_log_thread.join();
    }
    let serial_output = fs::read_to_string(&context.serial_log).unwrap_or_default();
    cleanup_qemu_context(&context);
    let exit_code = decode_qemu_exit_code(status.code(), &context)?;
    cleanup_qemu_debug_log(&context);
    Ok(QemuTestResult {
        exit_code,
        serial_output,
        failure: (exit_code != 0).then(|| format!("qemu exited with code {exit_code}")),
    })
}

pub fn run_qemu_mcp(uefi_path: &Path, options: &RunOptions) -> Result<i32> {
    let context = QemuRunContext::new(options);
    let mut cmd = build_qemu_command(uefi_path, options, &context)?;
    let mut child = cmd.spawn().context("failed to start qemu-system-x86_64")?;
    let metadata = json!({
        "runner_pid": std::process::id(),
        "qemu_pid": child.id(),
        "serial_log": context.serial_log,
        "qmp_socket": context.qmp_socket,
        "uefi_image": uefi_path,
    });
    println!("{metadata}");
    let _ = std::io::stdout().flush();
    let status = child.wait().context("failed to wait on qemu")?;
    cleanup_qemu_context(&context);
    let exit_code = decode_qemu_exit_code(status.code(), &context)?;
    cleanup_qemu_debug_log(&context);
    Ok(exit_code)
}

fn decode_qemu_exit_code(code: Option<i32>, context: &QemuRunContext) -> Result<i32> {
    Ok(match code.unwrap_or(1) {
        33 => 0,
        35 => 1,
        _ => {
            if let Some(path) = &context.debug_log {
                report_qemu_fault(path)?;
            }
            2
        }
    })
}

fn build_qemu_command(
    uefi_path: &Path,
    options: &RunOptions,
    context: &QemuRunContext,
) -> Result<Command> {
    let root_disk = env::var_os("SEELE_ROOT_DISK")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("..")
                .join("disk.img")
        });
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
    if let Some(parent) = context.qmp_socket.parent() {
        let _ = fs::create_dir_all(parent);
    }
    cleanup_socket(&context.qmp_socket);
    cmd.arg("-qmp").arg(format!(
        "unix:{},server=on,wait=off",
        context.qmp_socket.display()
    ));
    cmd.arg("-monitor").arg("none");
    cmd.arg("-device")
        .arg("isa-debug-exit,iobase=0xf4,iosize=0x04");
    cmd.arg("-device").arg("qemu-xhci");
    cmd.arg("-device").arg("usb-tablet");

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
        Prebuilt::fetch(Source::LATEST, "target/ovmf").context("failed to update prebuilt OVMF")?;
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

    Ok(cmd)
}

fn cleanup_qemu_context(context: &QemuRunContext) {
    let _ = fs::remove_file(&context.serial_log);
    cleanup_socket(&context.qmp_socket);
}

fn cleanup_qemu_debug_log(context: &QemuRunContext) {
    if !context.keep_debug_log
        && let Some(path) = &context.debug_log
    {
        let _ = fs::remove_file(path);
    }
}

fn default_smp() -> String {
    thread::available_parallelism()
        .map(|count| count.get().to_string())
        .unwrap_or_else(|_| "1".to_string())
}

fn report_qemu_fault(debug_log: &Path) -> Result<()> {
    let contents = fs::read_to_string(debug_log)
        .with_context(|| format!("failed to read qemu debug log {}", debug_log.display()))?;
    if contents.contains("Triple fault") {
        eprintln!("qemu: detected triple fault");
    }
    Ok(())
}
