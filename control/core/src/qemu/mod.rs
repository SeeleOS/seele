use crate::{
    Artifact, ArtifactKind, Event, JobContext, VmEvent, VmSmokeReport, process::ProcessRunner,
    target_dir,
};
use anyhow::{Context, Result, bail};
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
    pub enable_profiling: bool,
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

impl VmConfig {
    pub fn for_repo(repo: &Path) -> Self {
        Self {
            qmp_socket: PathBuf::from("/tmp/seele-agent-qmp.sock"),
            serial_log: PathBuf::from("/tmp/seele-agent-serial.log"),
            rootfs_image: target_dir(repo).join("rootfs.img"),
            enable_profiling: false,
        }
    }
}

pub fn start_vm(repo: &Path, mut config: VmConfig, context: &JobContext) -> Result<i32> {
    config.rootfs_image = if config.rootfs_image.is_relative() {
        repo.join(&config.rootfs_image)
    } else {
        config.rootfs_image
    };
    let artifact_dir = target_dir(repo).join("control-artifacts").join("vm");
    fs::create_dir_all(&artifact_dir)
        .with_context(|| format!("failed to create {}", artifact_dir.display()))?;
    let _ = fs::remove_file(&config.qmp_socket);
    let _ = fs::remove_file(&config.serial_log);
    let stderr = fs::File::create(artifact_dir.join("qemu.stderr.log"))?;
    let stdout = fs::File::create(artifact_dir.join("qemu.stdout.log"))?;
    context.artifact(Artifact {
        kind: ArtifactKind::SerialLog,
        path: config.serial_log.clone(),
        description: "QEMU serial log".to_string(),
    });

    let mut command = qemu_command(&config);
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
    wait_for_qmp_or_exit(child, &config.qmp_socket, Duration::from_secs(30))
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

fn qemu_command(config: &VmConfig) -> Command {
    let mut command = Command::new("qemu-system-x86_64");
    command
        .args(["-machine", "q35"])
        .args(["-m", "2G"])
        .args(["-serial"])
        .arg(format!("file:{}", config.serial_log.display()))
        .args(["-qmp"])
        .arg(format!(
            "unix:{},server=on,wait=off",
            config.qmp_socket.display()
        ))
        .args(["-drive"])
        .arg(format!(
            "file={},format=raw,if=virtio",
            config.rootfs_image.display()
        ))
        .args(["-display", "none"]);
    command
}

fn wait_for_qmp_or_exit(mut child: Child, qmp_socket: &Path, timeout: Duration) -> Result<i32> {
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
