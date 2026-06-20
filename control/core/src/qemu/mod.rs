use crate::{
    Artifact, ArtifactKind, Event, JobContext, VmEvent, VmSmokeReport,
    build::{KernelBuildMode, KernelBuildOptions, build_kernel},
    iso::{BootConfig, create_boot_iso},
    process::ProcessRunner,
    target_dir,
};
use anyhow::{Context, Result, bail};
use ovmf_prebuilt::{Arch, FileType, Prebuilt, Source};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{
    fs,
    io::{BufRead, BufReader, Write},
    os::unix::net::UnixStream,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant},
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VmConfig {
    pub qmp_socket: PathBuf,
    pub serial_log: PathBuf,
    pub rootfs_image: PathBuf,
    pub ltp_device_image: PathBuf,
    pub iso_image: Option<PathBuf>,
    pub enable_profiling: bool,
    pub display: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VmStatus {
    pub running: bool,
    pub qemu_pid: Option<u32>,
    pub qmp_socket: PathBuf,
    pub qmp_connectable: bool,
    pub serial_log: PathBuf,
    pub serial_log_exists: bool,
}

#[derive(Debug, Clone)]
pub struct QemuRunResult {
    pub exit_code: i32,
    pub serial_log: PathBuf,
    pub serial_output: String,
    pub failure: Option<String>,
}

impl VmConfig {
    pub fn for_repo(repo: &Path) -> Self {
        Self {
            qmp_socket: PathBuf::from("/tmp/seele-agent-qmp.sock"),
            serial_log: PathBuf::from("/tmp/seele-agent-serial.log"),
            rootfs_image: target_dir(repo).join("rootfs.img"),
            ltp_device_image: target_dir(repo).join("ltp-dev.img"),
            iso_image: None,
            enable_profiling: false,
            display: false,
        }
    }
}

pub fn start_vm(repo: &Path, mut config: VmConfig, context: &JobContext) -> Result<i32> {
    absolutize_paths(repo, &mut config);
    let iso = match config.iso_image.clone() {
        Some(iso) => iso,
        None => {
            let kernels = build_kernel(
                repo,
                KernelBuildMode::Run,
                KernelBuildOptions {
                    enable_profiling: config.enable_profiling,
                },
                context,
            )?;
            create_boot_iso(repo, &kernels[0], &BootConfig::default(), context)?
        }
    };
    let mut child = spawn_qemu(repo, &iso, &config, context)?;
    wait_for_qmp_or_exit(&mut child, &config.qmp_socket, Duration::from_secs(30))
}

pub fn run_iso_capture(
    repo: &Path,
    iso: &Path,
    mut config: VmConfig,
    timeout: Option<Duration>,
    condition: Option<fn(&str) -> bool>,
    context: &JobContext,
) -> Result<QemuRunResult> {
    absolutize_paths(repo, &mut config);
    let mut child = spawn_qemu(repo, iso, &config, context)?;
    let deadline = timeout.map(|timeout| Instant::now() + timeout);
    let mut captured = String::new();
    let mut offset = 0;

    loop {
        append_serial(&config.serial_log, &mut offset, &mut captured);
        if condition.is_some_and(|condition| condition(&captured)) {
            let _ = child.kill();
            let _ = child.wait();
            return Ok(QemuRunResult {
                exit_code: 0,
                serial_log: config.serial_log,
                serial_output: captured,
                failure: None,
            });
        }

        if let Some(status) = child.try_wait().context("failed to poll qemu")? {
            append_serial(&config.serial_log, &mut offset, &mut captured);
            let exit_code = decode_qemu_exit_code(status.code());
            return Ok(QemuRunResult {
                exit_code,
                serial_log: config.serial_log,
                serial_output: captured,
                failure: (exit_code != 0).then(|| format!("qemu exited with code {exit_code}")),
            });
        }

        if deadline.is_some_and(|deadline| Instant::now() >= deadline) {
            let _ = child.kill();
            let _ = child.wait();
            append_serial(&config.serial_log, &mut offset, &mut captured);
            return Ok(QemuRunResult {
                exit_code: 1,
                serial_log: config.serial_log,
                serial_output: captured,
                failure: Some("timed out waiting for qemu".to_string()),
            });
        }

        thread::sleep(Duration::from_millis(20));
    }
}

pub fn stop_vm(repo: &Path, context: &JobContext) -> Result<i32> {
    let artifact_dir = target_dir(repo).join("control-artifacts").join("vm");
    let pid_path = artifact_dir.join("qemu.pid");
    if let Ok(pid) = fs::read_to_string(&pid_path).map(|pid| pid.trim().to_string())
        && !pid.is_empty()
    {
        let runner = ProcessRunner::new(&artifact_dir)?;
        let _ = runner.run(
            context,
            "qemu_kill",
            Command::new("kill").arg("-TERM").arg(&pid),
        )?;
        let _ = fs::remove_file(pid_path);
    }
    context.event(Event::Vm(VmEvent::Stopped));
    Ok(0)
}

pub fn vm_status(repo: &Path) -> VmStatus {
    let config = VmConfig::for_repo(repo);
    let artifact_dir = target_dir(repo).join("control-artifacts").join("vm");
    let qemu_pid = fs::read_to_string(artifact_dir.join("qemu.pid"))
        .ok()
        .and_then(|pid| pid.trim().parse::<u32>().ok());
    let running = qemu_pid.is_some_and(process_exists);
    VmStatus {
        running,
        qemu_pid,
        qmp_socket: config.qmp_socket.clone(),
        qmp_connectable: config.qmp_socket.exists(),
        serial_log_exists: config.serial_log.exists(),
        serial_log: config.serial_log,
    }
}

pub fn serial_tail(repo: &Path, lines: Option<usize>, bytes: Option<usize>) -> Result<String> {
    let log = VmConfig::for_repo(repo).serial_log;
    let content = fs::read_to_string(&log).unwrap_or_default();
    if let Some(bytes) = bytes {
        let start = content.len().saturating_sub(bytes);
        return Ok(content[start..].to_string());
    }
    if let Some(lines) = lines {
        let selected = content
            .lines()
            .rev()
            .take(lines)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect::<Vec<_>>()
            .join("\n");
        return Ok(selected);
    }
    Ok(content)
}

pub fn wait_serial(
    repo: &Path,
    pattern: &str,
    timeout_ms: Option<u64>,
    context: &JobContext,
) -> Result<String> {
    let deadline = Instant::now() + Duration::from_millis(timeout_ms.unwrap_or(30_000));
    loop {
        let tail = serial_tail(repo, None, None)?;
        if tail.contains(pattern) {
            context.event(Event::Vm(VmEvent::SerialMatched {
                pattern: pattern.to_string(),
            }));
            return Ok(tail);
        }
        if Instant::now() >= deadline {
            bail!("timed out waiting for serial pattern {pattern:?}");
        }
        thread::sleep(Duration::from_millis(100));
    }
}

pub fn vm_smoke_report(repo: &Path) -> VmSmokeReport {
    let status = vm_status(repo);
    VmSmokeReport {
        booted: status.serial_log_exists,
        qmp_connectable: status.qmp_connectable,
        serial_log: status.serial_log,
        screenshot: None,
    }
}

pub fn screenshot(repo: &Path) -> Result<PathBuf> {
    let config = VmConfig::for_repo(repo);
    let artifact_dir = target_dir(repo).join("control-artifacts").join("vm");
    fs::create_dir_all(&artifact_dir)
        .with_context(|| format!("failed to create {}", artifact_dir.display()))?;
    let path = artifact_dir.join("screenshot.ppm");
    qmp_execute(
        &config.qmp_socket,
        json!({
            "execute": "screendump",
            "arguments": { "filename": path }
        }),
    )?;
    Ok(path)
}

pub fn send_key(repo: &Path, keys: &[String]) -> Result<()> {
    qmp_execute(
        &VmConfig::for_repo(repo).qmp_socket,
        json!({
            "execute": "send-key",
            "arguments": {
                "keys": keys.iter().map(|key| json!({ "type": "qcode", "data": key })).collect::<Vec<_>>()
            }
        }),
    )?;
    Ok(())
}

pub fn type_text(repo: &Path, text: &str) -> Result<()> {
    for byte in text.bytes() {
        qmp_execute(
            &VmConfig::for_repo(repo).qmp_socket,
            json!({
                "execute": "input-send-event",
                "arguments": {
                    "events": [
                        { "type": "key", "data": { "down": true, "key": { "type": "number", "data": byte } } },
                        { "type": "key", "data": { "down": false, "key": { "type": "number", "data": byte } } }
                    ]
                }
            }),
        )?;
    }
    Ok(())
}

pub fn mouse_move(repo: &Path, x: i64, y: i64) -> Result<()> {
    qmp_execute(
        &VmConfig::for_repo(repo).qmp_socket,
        json!({
            "execute": "input-send-event",
            "arguments": {
                "events": [
                    { "type": "abs", "data": { "axis": "x", "value": x } },
                    { "type": "abs", "data": { "axis": "y", "value": y } }
                ]
            }
        }),
    )?;
    Ok(())
}

pub fn mouse_click(repo: &Path, button: &str) -> Result<()> {
    qmp_execute(
        &VmConfig::for_repo(repo).qmp_socket,
        json!({
            "execute": "input-send-event",
            "arguments": {
                "events": [
                    { "type": "btn", "data": { "down": true, "button": button } },
                    { "type": "btn", "data": { "down": false, "button": button } }
                ]
            }
        }),
    )?;
    Ok(())
}

fn spawn_qemu(repo: &Path, iso: &Path, config: &VmConfig, context: &JobContext) -> Result<Child> {
    let artifact_dir = target_dir(repo).join("control-artifacts").join("vm");
    fs::create_dir_all(&artifact_dir)
        .with_context(|| format!("failed to create {}", artifact_dir.display()))?;
    let _ = fs::remove_file(&config.qmp_socket);
    let _ = fs::remove_file(&config.serial_log);
    ensure_ltp_device_image(&config.ltp_device_image)?;
    let stderr = fs::File::create(artifact_dir.join("qemu.stderr.log"))?;
    let stdout = fs::File::create(artifact_dir.join("qemu.stdout.log"))?;
    context.artifact(Artifact {
        kind: ArtifactKind::SerialLog,
        path: config.serial_log.clone(),
        description: "QEMU serial log".to_string(),
    });

    let mut command = qemu_command(repo, iso, config)?;
    let child = command
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr))
        .spawn()
        .context("failed to start qemu-system-x86_64")?;
    let pid = child.id();
    fs::write(artifact_dir.join("qemu.pid"), pid.to_string())?;
    context.event(Event::Vm(VmEvent::Started {
        runner_pid: std::process::id(),
        qemu_pid: Some(pid),
        qmp_socket: config.qmp_socket.clone(),
        serial_log: config.serial_log.clone(),
    }));
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
        .arg(format!("file:{}", config.serial_log.display()))
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

fn wait_for_qmp_or_exit(child: &mut Child, qmp_socket: &Path, timeout: Duration) -> Result<i32> {
    let deadline = Instant::now() + timeout;
    loop {
        if qmp_socket.exists() {
            return Ok(0);
        }
        if let Some(status) = child.try_wait().context("failed to poll qemu")? {
            return Ok(status.code().unwrap_or(1).max(1));
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            bail!("timed out waiting for QMP socket");
        }
        thread::sleep(Duration::from_millis(50));
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
    let min_size = 512 * 1024 * 1024;
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

fn append_serial(path: &Path, offset: &mut usize, captured: &mut String) {
    let Ok(content) = fs::read_to_string(path) else {
        return;
    };
    if *offset > content.len() {
        *offset = 0;
    }
    if *offset < content.len() {
        captured.push_str(&content[*offset..]);
        *offset = content.len();
    }
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

fn process_exists(pid: u32) -> bool {
    Command::new("kill")
        .arg("-0")
        .arg(pid.to_string())
        .status()
        .is_ok_and(|status| status.success())
}

fn qmp_execute(socket: &Path, command: serde_json::Value) -> Result<serde_json::Value> {
    let mut stream = UnixStream::connect(socket)
        .with_context(|| format!("failed to connect QMP socket {}", socket.display()))?;
    let mut reader = BufReader::new(stream.try_clone().context("failed to clone QMP stream")?);
    let mut greeting = String::new();
    reader
        .read_line(&mut greeting)
        .context("failed to read QMP greeting")?;
    write_json_line(&mut stream, &json!({ "execute": "qmp_capabilities" }))?;
    let _ = read_qmp_response(&mut reader)?;
    write_json_line(&mut stream, &command)?;
    read_qmp_response(&mut reader)
}

fn write_json_line(stream: &mut UnixStream, value: &serde_json::Value) -> Result<()> {
    serde_json::to_writer(&mut *stream, value).context("failed to write QMP command")?;
    stream
        .write_all(b"\n")
        .context("failed to flush QMP command")
}

fn read_qmp_response(reader: &mut BufReader<UnixStream>) -> Result<serde_json::Value> {
    let mut line = String::new();
    loop {
        line.clear();
        let read = reader
            .read_line(&mut line)
            .context("failed to read QMP response")?;
        if read == 0 {
            bail!("QMP socket closed before response");
        }
        let value: serde_json::Value =
            serde_json::from_str(&line).context("failed to parse QMP response")?;
        if value.get("event").is_none() {
            if let Some(error) = value.get("error") {
                bail!("QMP command failed: {error}");
            }
            return Ok(value);
        }
    }
}
