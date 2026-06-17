use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    env,
    path::{Path, PathBuf},
    process::Stdio,
};
use tokio::{
    fs,
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    process::{Child, ChildStdin, ChildStdout, Command},
    sync::Mutex,
    time::{Duration, timeout},
};

const DEFAULT_REPO: &str = "/home/elysia/coding-project/seele-os-linux";
const DEFAULT_QMP_SOCKET: &str = "/tmp/seele-agent-qmp.sock";
const DEFAULT_SERIAL_LOG: &str = "/tmp/seele-agent-serial.log";

#[derive(Debug, Clone, Serialize)]
pub struct SessionStatus {
    pub running: bool,
    pub runner_pid: Option<u32>,
    pub qemu_pid: Option<u32>,
    pub qmp_socket: PathBuf,
    pub qmp_connectable: bool,
    pub serial_log: PathBuf,
    pub serial_log_exists: bool,
    pub last_exit: Option<i32>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SessionMetadata {
    pub runner_pid: u32,
    pub qemu_pid: Option<u32>,
    pub qmp_socket: PathBuf,
    pub serial_log: PathBuf,
    pub iso_image: Option<PathBuf>,
}

#[derive(Debug, Default)]
struct SessionState {
    child: Option<Child>,
    metadata: Option<SessionMetadata>,
    gdb: Option<GdbState>,
    last_exit: Option<i32>,
}

#[derive(Debug)]
struct GdbState {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    port: u16,
}

#[derive(Debug)]
pub struct AgentSession {
    repo: PathBuf,
    qmp_socket: PathBuf,
    serial_log: PathBuf,
    state: Mutex<SessionState>,
}

impl AgentSession {
    pub fn from_env() -> Result<Self> {
        let repo = env::var_os("SEELE_REPO")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(DEFAULT_REPO));
        let qmp_socket = env::var_os("SEELE_QMP_SOCKET")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(DEFAULT_QMP_SOCKET));
        let serial_log = env::var_os("SEELE_SERIAL_LOG")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(DEFAULT_SERIAL_LOG));

        Ok(Self {
            repo,
            qmp_socket,
            serial_log,
            state: Mutex::new(SessionState::default()),
        })
    }

    pub fn qmp_socket(&self) -> &Path {
        &self.qmp_socket
    }

    pub async fn start(&self, enable_profiling: bool) -> Result<SessionMetadata> {
        self.start_with_env([], enable_profiling).await
    }

    pub async fn debug_start(&self, port: Option<u16>) -> Result<DebugStartStatus> {
        let port = port.unwrap_or(1234);
        let metadata = self
            .start_with_env(
                [
                    ("SEELE_QEMU_GDB".to_string(), format!("tcp::{port}")),
                    ("SEELE_QEMU_WAIT_GDB".to_string(), "1".to_string()),
                ],
                false,
            )
            .await?;
        let result = async {
            let mut gdb = self.spawn_gdb(port).await?;
            let startup_output = read_gdb_until_prompt(&mut gdb.stdout, Duration::from_secs(10))
                .await
                .context("timed out waiting for initial gdb prompt")?;
            write_gdb_command(&mut gdb.stdin, &format!("target remote :{port}")).await?;
            let connect_output = read_gdb_until_prompt(&mut gdb.stdout, Duration::from_secs(20))
                .await
                .context("timed out waiting for gdb target remote")?;
            Ok((gdb, startup_output, connect_output))
        }
        .await;
        let (gdb, startup_output, connect_output) = match result {
            Ok(result) => result,
            Err(err) => {
                let mut state = self.state.lock().await;
                let _ = stop_state(&mut state).await;
                return Err(err);
            }
        };

        let mut state = self.state.lock().await;
        state.gdb = Some(gdb);

        Ok(DebugStartStatus {
            metadata,
            gdb_port: port,
            startup_output: truncate_log(&startup_output),
            connect_output: truncate_log(&connect_output),
        })
    }

    async fn start_with_env<I>(&self, envs: I, enable_profiling: bool) -> Result<SessionMetadata>
    where
        I: IntoIterator<Item = (String, String)>,
    {
        let mut state = self.state.lock().await;
        refresh_child_state(&mut state).await;
        if state.child.is_some() {
            bail!("VM session is already running");
        }

        let _ = fs::remove_file(&self.qmp_socket).await;
        let _ = fs::remove_file(&self.serial_log).await;

        let mut command = Command::new("cargo");
        command
            .arg("run")
            .arg("--quiet")
            .arg("-p")
            .arg("xtask")
            .arg("--")
            .arg("mcp-run")
            .arg("--json-output");
        if enable_profiling {
            command.arg("--enable-profiling");
        }
        command
            .current_dir(&self.repo)
            .env("SEELE_QMP_SOCKET", &self.qmp_socket)
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        for (key, value) in envs {
            command.env(key, value);
        }
        let mut child = command.spawn().context("failed to start xtask mcp-run")?;

        let stdout = child
            .stdout
            .take()
            .context("xtask mcp-run stdout was not piped")?;
        let metadata = timeout(
            Duration::from_secs(120),
            read_metadata_event(stdout, child.id(), &self.qmp_socket, &self.serial_log),
        )
        .await
        .context("timed out waiting for xtask mcp metadata")??;
        state.child = Some(child);
        state.metadata = Some(metadata.clone());
        state.last_exit = None;
        Ok(metadata)
    }

    async fn spawn_gdb(&self, port: u16) -> Result<GdbState> {
        let kernel = self
            .repo
            .join("target")
            .join("x86_64-unknown-none")
            .join("debug")
            .join("kernel");
        let gdb_bin = env::var_os("SEELE_GDB")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("gdb"));
        let mut child = Command::new(&gdb_bin)
            .arg("-q")
            .arg(&kernel)
            .arg("-ex")
            .arg("set pagination off")
            .arg("-ex")
            .arg("set confirm off")
            .current_dir(&self.repo)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .with_context(|| format!("failed to start gdb at {}", gdb_bin.display()))?;
        let stdin = child.stdin.take().context("gdb stdin was not piped")?;
        let stdout = child.stdout.take().context("gdb stdout was not piped")?;
        Ok(GdbState {
            child,
            stdin,
            stdout: BufReader::new(stdout),
            port,
        })
    }

    pub async fn stop(&self) -> Result<SessionStatus> {
        let mut state = self.state.lock().await;
        stop_state(&mut state).await?;
        drop(state);
        let _ = fs::remove_file(&self.qmp_socket).await;
        self.status().await
    }

    pub async fn cleanup(&self) -> Result<SessionStatus> {
        let mut state = self.state.lock().await;
        stop_state(&mut state).await?;
        drop(state);
        let _ = fs::remove_file(&self.qmp_socket).await;
        self.status().await
    }

    pub async fn status(&self) -> Result<SessionStatus> {
        let mut state = self.state.lock().await;
        refresh_child_state(&mut state).await;
        let metadata = state.metadata.clone();
        let running = state.child.is_some();
        let qmp_connectable = tokio::net::UnixStream::connect(&self.qmp_socket)
            .await
            .is_ok();
        let serial_log_exists = fs::metadata(&self.serial_log).await.is_ok();

        Ok(SessionStatus {
            running,
            runner_pid: metadata.as_ref().map(|metadata| metadata.runner_pid),
            qemu_pid: metadata.as_ref().and_then(|metadata| metadata.qemu_pid),
            qmp_socket: metadata
                .as_ref()
                .map(|metadata| metadata.qmp_socket.clone())
                .unwrap_or_else(|| self.qmp_socket.clone()),
            qmp_connectable,
            serial_log: metadata
                .as_ref()
                .map(|metadata| metadata.serial_log.clone())
                .unwrap_or_else(|| self.serial_log.clone()),
            serial_log_exists,
            last_exit: state.last_exit,
        })
    }

    pub async fn debug_status(&self) -> Result<DebugStatus> {
        let mut state = self.state.lock().await;
        refresh_child_state(&mut state).await;
        refresh_gdb_state(&mut state).await;
        Ok(DebugStatus {
            vm_running: state.child.is_some(),
            gdb_running: state.gdb.is_some(),
            gdb_port: state.gdb.as_ref().map(|gdb| gdb.port),
            metadata: state.metadata.clone(),
            last_exit: state.last_exit,
        })
    }

    pub async fn debug_command(
        &self,
        command: &str,
        timeout_ms: Option<u64>,
    ) -> Result<GdbCommandOutput> {
        let mut state = self.state.lock().await;
        refresh_gdb_state(&mut state).await;
        let gdb = state.gdb.as_mut().context("gdb session is not running")?;
        write_gdb_command(&mut gdb.stdin, command).await?;
        let timeout_duration = Duration::from_millis(timeout_ms.unwrap_or(5_000));
        match read_gdb_until_prompt(&mut gdb.stdout, timeout_duration).await {
            Ok(output) => Ok(GdbCommandOutput {
                timed_out: false,
                output: truncate_log(&output),
            }),
            Err(err) if err.is::<tokio::time::error::Elapsed>() => Ok(GdbCommandOutput {
                timed_out: true,
                output: String::new(),
            }),
            Err(err) => Err(err),
        }
    }

    pub async fn debug_stop(&self) -> Result<SessionStatus> {
        let mut state = self.state.lock().await;
        stop_gdb_state(&mut state).await?;
        stop_state(&mut state).await?;
        drop(state);
        let _ = fs::remove_file(&self.qmp_socket).await;
        self.status().await
    }

    pub async fn serial_tail(&self, lines: Option<usize>, bytes: Option<usize>) -> Result<String> {
        let status = self.status().await?;
        let data = fs::read(&status.serial_log).await.with_context(|| {
            format!("failed to read serial log {}", status.serial_log.display())
        })?;
        let max_bytes = bytes.unwrap_or(16 * 1024).min(1024 * 1024);
        let start = data.len().saturating_sub(max_bytes);
        let text = String::from_utf8_lossy(&data[start..]).into_owned();
        if let Some(lines) = lines {
            let mut tail = text.lines().rev().take(lines).collect::<Vec<_>>();
            tail.reverse();
            Ok(tail.join("\n"))
        } else {
            Ok(text)
        }
    }

    pub async fn run_cargo_alias(&self, alias: &str) -> Result<CommandOutput> {
        let command = match alias {
            "xtest" => "test",
            "xbuild-rootfs" => "build-rootfs",
            _ => bail!("unsupported MCP cargo alias: {alias}"),
        };
        let output = Command::new("cargo")
            .arg("run")
            .arg("--quiet")
            .arg("-p")
            .arg("xtask")
            .arg("--")
            .arg(command)
            .arg("--json-output")
            .current_dir(&self.repo)
            .output()
            .await
            .with_context(|| format!("failed to run xtask {command}"))?;
        let events = parse_xtask_events(String::from_utf8_lossy(&output.stdout).as_ref());
        Ok(CommandOutput {
            exit_code: output.status.code().unwrap_or(1),
            stdout: summarize_xtask_events(&events),
            stderr: truncate_log(String::from_utf8_lossy(&output.stderr).as_ref()),
            events,
        })
    }

    pub async fn ensure_rootfs_mounted(&self) -> Result<CommandOutput> {
        let target = self.repo.join("target");
        let rootfs_mount = target.join("rootfs_mnt");
        let rootfs_image = target.join("rootfs.img");

        let mountpoint = Command::new("mountpoint")
            .arg("-q")
            .arg(&rootfs_mount)
            .output()
            .await
            .with_context(|| format!("failed to inspect mountpoint {}", rootfs_mount.display()))?;
        if mountpoint.status.success() {
            return Ok(CommandOutput {
                exit_code: 0,
                stdout: String::new(),
                stderr: String::new(),
                events: Vec::new(),
            });
        }

        let output = Command::new("sudo")
            .arg("mount")
            .arg("-o")
            .arg("loop")
            .arg(&rootfs_image)
            .arg(&rootfs_mount)
            .current_dir(&self.repo)
            .output()
            .await
            .with_context(|| format!("failed to mount rootfs from {}", rootfs_image.display()))?;
        Ok(CommandOutput {
            exit_code: output.status.code().unwrap_or(1),
            stdout: truncate_log(String::from_utf8_lossy(&output.stdout).as_ref()),
            stderr: truncate_log(String::from_utf8_lossy(&output.stderr).as_ref()),
            events: Vec::new(),
        })
    }
}

async fn read_metadata_event(
    stdout: ChildStdout,
    child_id: Option<u32>,
    default_qmp_socket: &Path,
    default_serial_log: &Path,
) -> Result<SessionMetadata> {
    let mut reader = BufReader::new(stdout);
    let mut line = String::new();
    loop {
        line.clear();
        let read = reader
            .read_line(&mut line)
            .await
            .context("failed to read xtask mcp metadata")?;
        if read == 0 {
            bail!("xtask mcp-run exited before metadata");
        }
        let Ok(event) = serde_json::from_str::<XtaskEvent>(line.trim()) else {
            continue;
        };
        if event.event == "metadata" {
            let value = event
                .metadata
                .context("xtask metadata event missing metadata")?;
            let metadata =
                parse_metadata_value(&value, child_id, default_qmp_socket, default_serial_log)?;
            tokio::spawn(async move {
                let mut line = String::new();
                while matches!(reader.read_line(&mut line).await, Ok(read) if read != 0) {
                    line.clear();
                }
            });
            return Ok(metadata);
        }
    }
}

#[derive(Debug, Serialize)]
pub struct CommandOutput {
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
    pub events: Vec<XtaskEvent>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct XtaskEvent {
    pub event: String,
    pub command: Option<String>,
    pub name: Option<String>,
    pub status: Option<String>,
    pub step: Option<String>,
    pub stream: Option<String>,
    pub message: Option<String>,
    pub output: Option<String>,
    pub exit_code: Option<i32>,
    pub metadata: Option<Value>,
}

#[derive(Debug, Serialize)]
pub struct DebugStartStatus {
    pub metadata: SessionMetadata,
    pub gdb_port: u16,
    pub startup_output: String,
    pub connect_output: String,
}

#[derive(Debug, Serialize)]
pub struct DebugStatus {
    pub vm_running: bool,
    pub gdb_running: bool,
    pub gdb_port: Option<u16>,
    pub metadata: Option<SessionMetadata>,
    pub last_exit: Option<i32>,
}

#[derive(Debug, Serialize)]
pub struct GdbCommandOutput {
    pub timed_out: bool,
    pub output: String,
}

fn parse_metadata_value(
    value: &Value,
    child_id: Option<u32>,
    default_qmp_socket: &Path,
    default_serial_log: &Path,
) -> Result<SessionMetadata> {
    Ok(SessionMetadata {
        runner_pid: value
            .get("runner_pid")
            .and_then(Value::as_u64)
            .or(child_id.map(u64::from))
            .context("xtask metadata missing runner_pid")? as u32,
        qemu_pid: value
            .get("qemu_pid")
            .and_then(Value::as_u64)
            .map(|pid| pid as u32),
        qmp_socket: value
            .get("qmp_socket")
            .and_then(Value::as_str)
            .map(PathBuf::from)
            .unwrap_or_else(|| default_qmp_socket.to_path_buf()),
        serial_log: value
            .get("serial_log")
            .and_then(Value::as_str)
            .map(PathBuf::from)
            .unwrap_or_else(|| default_serial_log.to_path_buf()),
        iso_image: value
            .get("iso_image")
            .and_then(Value::as_str)
            .map(PathBuf::from),
    })
}

fn parse_xtask_events(output: &str) -> Vec<XtaskEvent> {
    output
        .lines()
        .filter_map(|line| serde_json::from_str::<XtaskEvent>(line).ok())
        .map(|mut event| {
            if let Some(output) = &event.output {
                event.output = Some(truncate_log(output));
            }
            event
        })
        .collect()
}

fn summarize_xtask_events(events: &[XtaskEvent]) -> String {
    let mut lines = Vec::new();
    for event in events {
        match event.event.as_str() {
            "started" => {
                if let Some(command) = &event.command {
                    lines.push(format!("started {command}"));
                }
            }
            "progress" => {
                let step = event.step.as_deref().unwrap_or("progress");
                let message = event.message.as_deref().unwrap_or_default();
                lines.push(format!("{step}: {message}"));
            }
            "test" => {
                let name = event.name.as_deref().unwrap_or("test");
                let status = event.status.as_deref().unwrap_or("unknown");
                let message = event.message.as_deref().unwrap_or_default();
                if message.is_empty() {
                    lines.push(format!("test {name}: {status}"));
                } else {
                    lines.push(format!("test {name}: {status} - {message}"));
                }
            }
            "log" => {
                if let Some(output) = &event.output {
                    lines.push(truncate_log(output));
                }
            }
            "finished" => {
                let command = event.command.as_deref().unwrap_or("command");
                let status = event.status.as_deref().unwrap_or("unknown");
                let exit_code = event.exit_code.unwrap_or_default();
                lines.push(format!("finished {command}: {status} (exit {exit_code})"));
            }
            _ => {}
        }
    }
    truncate_log(&lines.join("\n"))
}

async fn refresh_child_state(state: &mut SessionState) {
    let Some(child) = state.child.as_mut() else {
        return;
    };
    if let Ok(Some(status)) = child.try_wait() {
        state.last_exit = status.code();
        state.child = None;
        state.metadata = None;
    }
}

async fn refresh_gdb_state(state: &mut SessionState) {
    let Some(gdb) = state.gdb.as_mut() else {
        return;
    };
    if matches!(gdb.child.try_wait(), Ok(Some(_))) {
        state.gdb = None;
    }
}

async fn stop_state(state: &mut SessionState) -> Result<()> {
    stop_gdb_state(state).await?;
    if let Some(qemu_pid) = state
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.qemu_pid)
    {
        let _ = Command::new("kill")
            .arg(qemu_pid.to_string())
            .status()
            .await;
    }
    if let Some(child) = state.child.as_mut() {
        let _ = child.kill().await;
        let status = child.wait().await.ok();
        state.last_exit = status.and_then(|status| status.code());
    }
    state.child = None;
    state.metadata = None;
    Ok(())
}

async fn stop_gdb_state(state: &mut SessionState) -> Result<()> {
    if let Some(mut gdb) = state.gdb.take() {
        let _ = write_gdb_command(&mut gdb.stdin, "quit").await;
        let _ = gdb.child.kill().await;
        let _ = gdb.child.wait().await;
    }
    Ok(())
}

async fn write_gdb_command(stdin: &mut ChildStdin, command: &str) -> Result<()> {
    stdin
        .write_all(command.as_bytes())
        .await
        .context("failed to write gdb command")?;
    stdin
        .write_all(b"\n")
        .await
        .context("failed to write gdb newline")?;
    stdin.flush().await.context("failed to flush gdb stdin")
}

async fn read_gdb_until_prompt(
    stdout: &mut BufReader<ChildStdout>,
    timeout_duration: Duration,
) -> Result<String> {
    timeout(timeout_duration, async {
        let mut output = Vec::new();
        let mut byte = [0_u8; 1];
        loop {
            let read = stdout
                .read(&mut byte)
                .await
                .context("failed to read gdb output")?;
            if read == 0 {
                bail!("gdb exited before prompt");
            }
            output.push(byte[0]);
            if output.ends_with(b"(gdb) ") || output.ends_with(b"(gdb)") {
                return Ok(String::from_utf8_lossy(&output).into_owned());
            }
        }
    })
    .await?
}

fn truncate_log(text: &str) -> String {
    const LIMIT: usize = 64 * 1024;
    if text.len() <= LIMIT {
        return text.to_string();
    }
    let start = text.len().saturating_sub(LIMIT);
    format!("[truncated]\n{}", &text[start..])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_metadata_uses_defaults() {
        let value = serde_json::json!({"runner_pid":42,"qemu_pid":99});
        let metadata = parse_metadata_value(
            &value,
            None,
            Path::new("/tmp/qmp.sock"),
            Path::new("/tmp/serial.log"),
        )
        .unwrap();
        assert_eq!(metadata.runner_pid, 42);
        assert_eq!(metadata.qemu_pid, Some(99));
        assert_eq!(metadata.qmp_socket, PathBuf::from("/tmp/qmp.sock"));
        assert_eq!(metadata.serial_log, PathBuf::from("/tmp/serial.log"));
    }
}
