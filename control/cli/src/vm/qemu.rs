use crate::{
    build::{KernelBuildMode, KernelBuildOptions, build_kernel, shell_for_repo},
    target_dir,
    vm::iso::{BootConfig, create_boot_iso},
};
use anyhow::{Context, Result};
use ovmf_prebuilt::{Arch, FileType, Prebuilt, Source};
use std::{
    fs,
    io::{self, Read, Write},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::mpsc,
    thread,
    thread::JoinHandle,
    time::{Duration, Instant},
};
use xshell::Shell;

#[derive(Debug, Clone)]
pub struct VmConfig {
    pub qmp_socket: PathBuf,
    pub serial_log: PathBuf,
    pub serial: SerialConfig,
    pub rootfs_image: PathBuf,
    pub ltp_device_image: PathBuf,
    pub iso_image: Option<PathBuf>,
    pub enable_profiling: bool,
    pub display: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SerialConfig {
    File,
    Stdio,
}

#[derive(Debug, Clone)]
pub struct QemuRunResult {
    pub exit_code: i32,
    pub serial_output: String,
    pub failure: Option<String>,
}

impl VmConfig {
    pub fn for_repo(repo: &Path) -> Self {
        let artifact_dir = target_dir(repo).join("control-cli").join("vm");
        Self {
            qmp_socket: artifact_dir.join("qmp.sock"),
            serial_log: artifact_dir.join("serial.log"),
            serial: SerialConfig::File,
            rootfs_image: target_dir(repo).join("rootfs.img"),
            ltp_device_image: target_dir(repo).join("ltp-dev.img"),
            iso_image: None,
            enable_profiling: false,
            display: false,
        }
    }
}

pub fn run(repo: &Path, mut config: VmConfig) -> Result<i32> {
    let sh = shell_for_repo(repo)?;
    absolutize_paths(repo, &mut config);
    let iso = match config.iso_image.clone() {
        Some(iso) => iso,
        None => {
            let kernels = build_kernel(
                &sh,
                KernelBuildMode::Run,
                KernelBuildOptions {
                    enable_profiling: config.enable_profiling,
                },
            )?;
            create_boot_iso(&sh, repo, &kernels[0], &BootConfig::default())?
        }
    };
    eprintln!("==> launching QEMU");
    config.serial.describe(&config.serial_log);
    let mut child = spawn_qemu(repo, &iso, &config)?;
    let status = child.wait().context("failed to wait for qemu")?;
    let _ = fs::remove_file(qemu_pid_path(repo));
    Ok(decode_qemu_exit_code(status.code()))
}

pub fn run_iso_capture(
    sh: &Shell,
    repo: &Path,
    iso: &Path,
    mut config: VmConfig,
    timeout: Option<Duration>,
    condition: Option<fn(&str) -> bool>,
) -> Result<QemuRunResult> {
    absolutize_paths(repo, &mut config);
    config.serial = SerialConfig::Stdio;
    let mut child = spawn_qemu_with_stdout(repo, iso, &config, Stdio::piped())?;
    let stdout = child
        .stdout
        .take()
        .context("captured QEMU process did not expose stdout")?;
    let serial_stdout = spawn_stdout_reader(stdout);
    let qemu_pid_path = qemu_pid_path(repo);
    let deadline = timeout.map(|timeout| Instant::now() + timeout);
    let mut captured = String::new();

    loop {
        drain_serial_stdout(&serial_stdout.rx, &mut captured);
        if condition.is_some_and(|condition| condition(&captured)) {
            let _ = child.kill();
            let _ = child.wait();
            let _ = sh.remove_path(&qemu_pid_path);
            finish_serial_stdout(serial_stdout, &mut captured);
            return Ok(QemuRunResult {
                exit_code: 0,
                serial_output: captured,
                failure: None,
            });
        }

        if let Some(status) = child.try_wait().context("failed to poll qemu")? {
            let _ = sh.remove_path(&qemu_pid_path);
            finish_serial_stdout(serial_stdout, &mut captured);
            let exit_code = decode_qemu_exit_code(status.code());
            return Ok(QemuRunResult {
                exit_code,
                serial_output: captured,
                failure: (exit_code != 0).then(|| format!("qemu exited with code {exit_code}")),
            });
        }

        if deadline.is_some_and(|deadline| Instant::now() >= deadline) {
            let _ = child.kill();
            let _ = child.wait();
            let _ = sh.remove_path(&qemu_pid_path);
            finish_serial_stdout(serial_stdout, &mut captured);
            return Ok(QemuRunResult {
                exit_code: 1,
                serial_output: captured,
                failure: Some("timed out waiting for qemu".to_string()),
            });
        }

        thread::sleep(Duration::from_millis(20));
    }
}

fn spawn_qemu(repo: &Path, iso: &Path, config: &VmConfig) -> Result<Child> {
    spawn_qemu_with_stdout(repo, iso, config, Stdio::inherit())
}

fn spawn_qemu_with_stdout(
    repo: &Path,
    iso: &Path,
    config: &VmConfig,
    stdout: Stdio,
) -> Result<Child> {
    let artifact_dir = target_dir(repo).join("control-cli").join("vm");
    fs::create_dir_all(&artifact_dir)
        .with_context(|| format!("failed to create {}", artifact_dir.display()))?;
    let _ = fs::remove_file(&config.qmp_socket);
    let _ = fs::remove_file(&config.serial_log);
    ensure_ltp_device_image(&config.ltp_device_image)?;

    let mut command = qemu_command(repo, iso, config)?;
    let child = command
        .stdin(Stdio::null())
        .stdout(stdout)
        .stderr(Stdio::inherit())
        .spawn()
        .context("failed to start qemu-system-x86_64")?;
    let pid = child.id();
    fs::write(qemu_pid_path(repo), pid.to_string())?;
    Ok(child)
}

fn qemu_command(repo: &Path, iso: &Path, config: &VmConfig) -> Result<Command> {
    let mut command = Command::new("qemu-system-x86_64");
    command
        .args(["-machine", "q35"])
        .args(["-m", "4G"])
        .arg("-smp")
        .arg(default_smp())
        .args(["-serial"])
        .arg(config.serial.qemu_arg(&config.serial_log))
        .args(["-qmp"])
        .arg(format!(
            "unix:{},server=on,wait=off",
            config.qmp_socket.display()
        ))
        .args(["-monitor", "none"])
        .args(["-device", "isa-debug-exit,iobase=0xf4,iosize=0x04"])
        .args(["-device", "qemu-xhci"])
        .args(["-device", "usb-tablet"])
        .args(["-display", if config.display { "sdl" } else { "none" }])
        .args(["-cdrom"])
        .arg(iso);

    if Path::new("/dev/kvm").exists() {
        command
            .arg("-enable-kvm")
            .args(["-cpu", "host,+hypervisor,+kvmclock,+kvmclock-stable-bit"]);
    }
    if config.rootfs_image.exists() {
        command.arg("-drive").arg(format!(
            "if=none,format=raw,file={},id=rootdisk",
            config.rootfs_image.display()
        ));
        command
            .arg("-device")
            .arg("virtio-blk-pci,drive=rootdisk,disable-legacy=on,disable-modern=off");
    }
    command.arg("-drive").arg(format!(
        "if=none,format=raw,file={},id=ltpdisk",
        config.ltp_device_image.display()
    ));
    command
        .arg("-device")
        .arg("virtio-blk-pci,drive=ltpdisk,disable-legacy=on,disable-modern=off");
    command
        .args(["-netdev", "user,id=net0"])
        .args(["-device", "e1000,netdev=net0,mac=52:54:00:12:34:56"]);

    let prebuilt = Prebuilt::fetch(Source::LATEST, repo.join("target/ovmf"))
        .context("failed to fetch OVMF")?;
    let code = prebuilt.get_file(Arch::X64, FileType::Code);
    let vars = prebuilt.get_file(Arch::X64, FileType::Vars);
    command.arg("-drive").arg(format!(
        "if=pflash,format=raw,unit=0,file={},readonly=on",
        code.display()
    ));
    command.arg("-drive").arg(format!(
        "if=pflash,format=raw,unit=1,file={},snapshot=on",
        vars.display()
    ));
    command.args(["-no-reboot", "-action", "reboot=shutdown"]);
    Ok(command)
}

impl SerialConfig {
    fn qemu_arg(self, serial_log: &Path) -> String {
        match self {
            Self::File => format!("file:{}", serial_log.display()),
            Self::Stdio => "stdio".to_string(),
        }
    }

    fn describe(self, serial_log: &Path) {
        match self {
            Self::File => eprintln!("    serial log: {}", serial_log.display()),
            Self::Stdio => eprintln!("    serial: stdout"),
        }
    }
}

fn decode_qemu_exit_code(code: Option<i32>) -> i32 {
    match code.unwrap_or(1) {
        33 => 0,
        35 => 1,
        other => other.max(1),
    }
}

fn ensure_ltp_device_image(path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    let file = fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .open(path)
        .with_context(|| format!("failed to open {}", path.display()))?;
    let min_size = 1024 * 1024 * 1024;
    if file.metadata()?.len() < min_size {
        file.set_len(min_size)
            .with_context(|| format!("failed to size {}", path.display()))?;
    }
    Ok(())
}

fn default_smp() -> String {
    thread::available_parallelism()
        .map(|count| count.get().to_string())
        .unwrap_or_else(|_| "1".to_string())
}

struct SerialStdout {
    rx: mpsc::Receiver<Vec<u8>>,
    reader: JoinHandle<()>,
}

fn spawn_stdout_reader(mut stdout: impl Read + Send + 'static) -> SerialStdout {
    let (tx, rx) = mpsc::channel();
    let reader = thread::spawn(move || {
        let mut buffer = [0; 8192];
        loop {
            match stdout.read(&mut buffer) {
                Ok(0) => break,
                Ok(count) => {
                    if tx.send(buffer[..count].to_vec()).is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    });
    SerialStdout { rx, reader }
}

fn drain_serial_stdout(rx: &mpsc::Receiver<Vec<u8>>, captured: &mut String) {
    while let Ok(chunk) = rx.try_recv() {
        append_stdout_chunk(&chunk, captured);
    }
}

fn finish_serial_stdout(serial_stdout: SerialStdout, captured: &mut String) {
    while let Ok(chunk) = serial_stdout.rx.recv() {
        append_stdout_chunk(&chunk, captured);
    }
    let _ = serial_stdout.reader.join();
}

fn append_stdout_chunk(chunk: &[u8], captured: &mut String) {
    let text = String::from_utf8_lossy(chunk);
    print!("{text}");
    let _ = io::stdout().flush();
    captured.push_str(&text);
}

fn qemu_pid_path(repo: &Path) -> PathBuf {
    target_dir(repo)
        .join("control-cli")
        .join("vm")
        .join("qemu.pid")
}

fn absolutize_paths(repo: &Path, config: &mut VmConfig) {
    if config.rootfs_image.is_relative() {
        config.rootfs_image = repo.join(&config.rootfs_image);
    }
    if config.ltp_device_image.is_relative() {
        config.ltp_device_image = repo.join(&config.ltp_device_image);
    }
    if let Some(iso) = &config.iso_image
        && iso.is_relative()
    {
        config.iso_image = Some(repo.join(iso));
    }
}
