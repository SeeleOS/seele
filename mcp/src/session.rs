use anyhow::{Context, Result, bail};
use serde::Serialize;
use serde_json::Value;
use std::{
    env,
    path::{Path, PathBuf},
    process::Stdio,
};
use tokio::{
    fs,
    io::{AsyncBufReadExt, BufReader},
    process::{Child, Command},
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
    pub uefi_image: Option<PathBuf>,
}

#[derive(Debug, Default)]
struct SessionState {
    child: Option<Child>,
    metadata: Option<SessionMetadata>,
    last_exit: Option<i32>,
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

    pub async fn start(&self) -> Result<SessionMetadata> {
        let mut state = self.state.lock().await;
        refresh_child_state(&mut state).await;
        if state.child.is_some() {
            bail!("agent session is already running");
        }

        let _ = fs::remove_file(&self.qmp_socket).await;
        let _ = fs::remove_file(&self.serial_log).await;

        let mut child = Command::new("cargo")
            .arg("run")
            .arg("-p")
            .arg("xtask")
            .arg("--")
            .arg("mcp-run")
            .current_dir(&self.repo)
            .env("SEELE_QMP_SOCKET", &self.qmp_socket)
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .context("failed to start xtask mcp-run")?;

        let stdout = child
            .stdout
            .take()
            .context("xtask mcp-run stdout was not piped")?;
        let mut line = String::new();
        let read = timeout(
            Duration::from_secs(120),
            BufReader::new(stdout).read_line(&mut line),
        )
        .await
        .context("timed out waiting for xtask mcp metadata")?
        .context("failed to read xtask mcp metadata")?;
        if read == 0 {
            let status = child.wait().await.ok();
            bail!("xtask mcp-run exited before metadata: {status:?}");
        }

        let metadata = parse_metadata(line.trim(), child.id(), &self.qmp_socket, &self.serial_log)?;
        state.child = Some(child);
        state.metadata = Some(metadata.clone());
        state.last_exit = None;
        Ok(metadata)
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
        let output = Command::new("cargo")
            .arg(alias)
            .current_dir(&self.repo)
            .output()
            .await
            .with_context(|| format!("failed to run cargo {alias}"))?;
        Ok(CommandOutput {
            exit_code: output.status.code().unwrap_or(1),
            stdout: truncate_log(String::from_utf8_lossy(&output.stdout).as_ref()),
            stderr: truncate_log(String::from_utf8_lossy(&output.stderr).as_ref()),
        })
    }
}

#[derive(Debug, Serialize)]
pub struct CommandOutput {
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
}

fn parse_metadata(
    line: &str,
    child_id: Option<u32>,
    default_qmp_socket: &Path,
    default_serial_log: &Path,
) -> Result<SessionMetadata> {
    let value: Value =
        serde_json::from_str(line).with_context(|| format!("invalid xtask metadata: {line}"))?;
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
        uefi_image: value
            .get("uefi_image")
            .and_then(Value::as_str)
            .map(PathBuf::from),
    })
}

async fn refresh_child_state(state: &mut SessionState) {
    let Some(child) = state.child.as_mut() else {
        return;
    };
    if let Ok(Some(status)) = child.try_wait() {
        state.last_exit = status.code();
        state.child = None;
    }
}

async fn stop_state(state: &mut SessionState) -> Result<()> {
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
    Ok(())
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
        let metadata = parse_metadata(
            r#"{"runner_pid":42,"qemu_pid":99}"#,
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
