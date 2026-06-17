use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    env,
    path::{Path, PathBuf},
    process::Stdio,
    sync::atomic::{AtomicU64, Ordering},
};
use tokio::{
    fs,
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    process::{Child, ChildStdin, ChildStdout, Command},
    sync::Mutex,
    time::{Duration, Instant, timeout},
};

const DEFAULT_REPO: &str = "/home/elysia/coding-project/seele-os-linux";
const DEFAULT_QMP_SOCKET: &str = "/tmp/seele-agent-qmp.sock";
const DEFAULT_SERIAL_LOG: &str = "/tmp/seele-agent-serial.log";
const DEFAULT_COMMAND_LOG_DIR: &str = "/tmp/seele-agent-xtask";
const LOG_LIMIT: usize = 64 * 1024;
const COMMAND_STATUS_REFRESH: Duration = Duration::from_secs(1);

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
    command_log_dir: PathBuf,
    next_command_id: AtomicU64,
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
        let command_log_dir = env::var_os("SEELE_COMMAND_LOG_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(DEFAULT_COMMAND_LOG_DIR));

        Ok(Self {
            repo,
            qmp_socket,
            serial_log,
            command_log_dir,
            next_command_id: AtomicU64::new(1),
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

    pub async fn start_xtest(&self, test: Option<&str>) -> Result<CommandHandle> {
        self.start_xtask_command(
            "test",
            test.into_iter().map(|value| value.to_string()).collect(),
            None,
        )
        .await
    }

    pub async fn start_build_rootfs(
        &self,
        timeout_ms: Option<u64>,
        override_rootfs: bool,
        rebuild_aur: bool,
        rebuild_aur_packages: &[String],
    ) -> Result<CommandHandle> {
        let mut args = Vec::new();
        if override_rootfs {
            args.push("--override-rootfs".to_string());
        }
        if rebuild_aur {
            args.push("--rebuild-aur".to_string());
        }
        for package in rebuild_aur_packages {
            args.push("--rebuild-aur-package".to_string());
            args.push(package.clone());
        }
        self.start_xtask_command("build-rootfs", args, timeout_ms)
            .await
    }

    pub async fn command_status(&self, id: u64) -> Result<CommandStatus> {
        let path = self.command_log_dir.join(format!("{id}.json"));
        let data = fs::read(&path)
            .await
            .with_context(|| format!("failed to read command status {}", path.display()))?;
        let mut status: CommandStatus = serde_json::from_slice(&data)
            .with_context(|| format!("failed to parse command status {}", path.display()))?;
        if !status.finished {
            if let Some(stdout_path) = &status.stdout_path {
                status.stdout = read_trailing_text(stdout_path, status.stdout_limit).await?;
            }
            if let Some(stderr_path) = &status.stderr_path {
                status.stderr = read_trailing_text(stderr_path, status.stderr_limit).await?;
            }
        }
        Ok(status)
    }

    pub async fn command_wait(&self, id: u64, timeout_ms: Option<u64>) -> Result<CommandStatus> {
        let deadline =
            timeout_ms.map(|timeout_ms| Instant::now() + Duration::from_millis(timeout_ms));
        loop {
            let status = self.command_status(id).await?;
            if status.finished {
                return Ok(status);
            }
            if deadline.is_some_and(|deadline| Instant::now() >= deadline) {
                bail!("timed out waiting for command {id}");
            }
            tokio::time::sleep(COMMAND_STATUS_REFRESH).await;
        }
    }

    async fn start_xtask_command(
        &self,
        command: &str,
        args: Vec<String>,
        timeout_ms: Option<u64>,
    ) -> Result<CommandHandle> {
        fs::create_dir_all(&self.command_log_dir)
            .await
            .with_context(|| format!("failed to create {}", self.command_log_dir.display()))?;
        let id = self.next_command_id.fetch_add(1, Ordering::Relaxed);
        let log_path = self.command_log_dir.join(format!("{id}.json"));
        let stdout = command_log_path(&self.command_log_dir, id, "stdout");
        let stderr = command_log_path(&self.command_log_dir, id, "stderr");
        let initial_status = CommandStatus {
            id,
            command: command.to_string(),
            pid: None,
            finished: false,
            timed_out: false,
            timeout_ms,
            exit_code: 0,
            stdout: String::new(),
            stderr: String::new(),
            stdout_limit: Some(LOG_LIMIT),
            stderr_limit: Some(LOG_LIMIT),
            stdout_path: Some(stdout.clone()),
            stderr_path: Some(stderr.clone()),
            events: Vec::new(),
        };
        fs::write(&log_path, serde_json::to_vec_pretty(&initial_status)?)
            .await
            .with_context(|| format!("failed to write command status {}", log_path.display()))?;

        let repo = self.repo.clone();
        let log_path_for_task = log_path.clone();
        let command_name = command.to_string();
        tokio::spawn(async move {
            let output = run_xtask_process(RunCommandRequest {
                id,
                repo: &repo,
                command: &command_name,
                args: &args,
                timeout_ms,
                status_path: &log_path_for_task,
                stdout_path: &stdout,
                stderr_path: &stderr,
            })
            .await;
            let status =
                command_status_from_output(id, &command_name, timeout_ms, &stdout, &stderr, output);
            let Ok(encoded) = serde_json::to_vec_pretty(&status) else {
                return;
            };
            let _ = fs::write(log_path_for_task, encoded).await;
        });

        Ok(CommandHandle {
            id,
            command: command.to_string(),
            timeout_ms,
            status_path: log_path,
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

    pub async fn unmount_rootfs(&self) -> Result<CommandOutput> {
        let rootfs_mount = self.repo.join("target").join("rootfs_mnt");

        let mountpoint = Command::new("mountpoint")
            .arg("-q")
            .arg(&rootfs_mount)
            .output()
            .await
            .with_context(|| format!("failed to inspect mountpoint {}", rootfs_mount.display()))?;
        if !mountpoint.status.success() {
            return Ok(CommandOutput {
                exit_code: 0,
                stdout: String::new(),
                stderr: String::new(),
                events: Vec::new(),
            });
        }

        let output = Command::new("sudo")
            .arg("umount")
            .arg("-l")
            .arg(&rootfs_mount)
            .current_dir(&self.repo)
            .output()
            .await
            .with_context(|| format!("failed to unmount {}", rootfs_mount.display()))?;
        Ok(CommandOutput {
            exit_code: output.status.code().unwrap_or(1),
            stdout: truncate_log(String::from_utf8_lossy(&output.stdout).as_ref()),
            stderr: truncate_log(String::from_utf8_lossy(&output.stderr).as_ref()),
            events: Vec::new(),
        })
    }
}

struct RunCommandRequest<'a> {
    id: u64,
    repo: &'a Path,
    command: &'a str,
    args: &'a [String],
    timeout_ms: Option<u64>,
    status_path: &'a Path,
    stdout_path: &'a Path,
    stderr_path: &'a Path,
}

async fn run_xtask_process(request: RunCommandRequest<'_>) -> Result<CommandProcessOutput> {
    let command_name = request.command.to_string();
    let mut command_process = Command::new("cargo");
    command_process
        .arg("run")
        .arg("--quiet")
        .arg("-p")
        .arg("xtask")
        .arg("--")
        .arg(request.command)
        .arg("--json-output");
    for arg in request.args {
        command_process.arg(arg);
    }
    command_process
        .current_dir(request.repo)
        .stdout(Stdio::from(
            std::fs::File::create(request.stdout_path)
                .with_context(|| format!("failed to open {}", request.stdout_path.display()))?,
        ))
        .stderr(Stdio::from(
            std::fs::File::create(request.stderr_path)
                .with_context(|| format!("failed to open {}", request.stderr_path.display()))?,
        ))
        .env("SEELE_MCP_COMMAND_STDOUT", request.stdout_path)
        .env("SEELE_MCP_COMMAND_STDERR", request.stderr_path);
    let child = command_process
        .spawn()
        .with_context(|| format!("failed to start xtask {}", request.command))?;
    wait_for_command_output(request, child)
        .await
        .with_context(|| format!("failed to wait for xtask {command_name}"))
}

async fn wait_for_command_output(
    request: RunCommandRequest<'_>,
    mut child: Child,
) -> Result<CommandProcessOutput> {
    let pid = child.id();
    let deadline = request
        .timeout_ms
        .map(|timeout_ms| Instant::now() + Duration::from_millis(timeout_ms));
    let (status, timed_out) = loop {
        if let Some(status) = child.try_wait().context("failed to poll xtask child")? {
            break (status, false);
        }
        if deadline.is_some_and(|deadline| Instant::now() >= deadline) {
            let _ = child.kill().await;
            let status = child
                .wait()
                .await
                .context("failed to wait for timed-out xtask child")?;
            break (status, true);
        }
        write_running_command_status(&request, pid).await?;
        tokio::time::sleep(COMMAND_STATUS_REFRESH).await;
    };
    write_running_command_status(&request, pid).await?;
    let stdout = fs::read(request.stdout_path)
        .await
        .with_context(|| format!("failed to read {}", request.stdout_path.display()))?;
    let stderr = fs::read(request.stderr_path)
        .await
        .with_context(|| format!("failed to read {}", request.stderr_path.display()))?;
    Ok(CommandProcessOutput {
        status,
        stdout,
        stderr,
        timed_out,
        pid,
    })
}

async fn write_running_command_status(
    request: &RunCommandRequest<'_>,
    pid: Option<u32>,
) -> Result<()> {
    let stdout_text = read_trailing_text(request.stdout_path, Some(LOG_LIMIT)).await?;
    let events = parse_xtask_events(&stdout_text);
    let status = CommandStatus {
        id: request.id,
        command: request.command.to_string(),
        pid,
        finished: false,
        timed_out: false,
        timeout_ms: request.timeout_ms,
        exit_code: 0,
        stdout: live_stdout_summary(&stdout_text, &events),
        stderr: read_trailing_text(request.stderr_path, Some(LOG_LIMIT)).await?,
        stdout_limit: Some(LOG_LIMIT),
        stderr_limit: Some(LOG_LIMIT),
        stdout_path: Some(request.stdout_path.to_path_buf()),
        stderr_path: Some(request.stderr_path.to_path_buf()),
        events,
    };
    fs::write(request.status_path, serde_json::to_vec_pretty(&status)?)
        .await
        .with_context(|| {
            format!(
                "failed to write command status {}",
                request.status_path.display()
            )
        })
}

fn command_status_from_output(
    id: u64,
    command: &str,
    timeout_ms: Option<u64>,
    stdout_path: &Path,
    stderr_path: &Path,
    output: Result<CommandProcessOutput>,
) -> CommandStatus {
    match output {
        Ok(output) => {
            let events = parse_xtask_events(String::from_utf8_lossy(&output.stdout).as_ref());
            CommandStatus {
                id,
                command: command.to_string(),
                pid: output.pid,
                finished: true,
                timed_out: output.timed_out,
                timeout_ms,
                exit_code: output.status.code().unwrap_or(1),
                stdout: final_stdout_summary(&events, &output.stdout),
                stderr: truncate_log(String::from_utf8_lossy(&output.stderr).as_ref()),
                stdout_limit: Some(LOG_LIMIT),
                stderr_limit: Some(LOG_LIMIT),
                stdout_path: Some(stdout_path.to_path_buf()),
                stderr_path: Some(stderr_path.to_path_buf()),
                events,
            }
        }
        Err(err) => CommandStatus {
            id,
            command: command.to_string(),
            pid: None,
            finished: true,
            timed_out: err.to_string().contains("timed out running xtask"),
            timeout_ms,
            exit_code: 1,
            stdout: String::new(),
            stderr: truncate_log(&err.to_string()),
            stdout_limit: Some(LOG_LIMIT),
            stderr_limit: Some(LOG_LIMIT),
            stdout_path: Some(stdout_path.to_path_buf()),
            stderr_path: Some(stderr_path.to_path_buf()),
            events: Vec::new(),
        },
    }
}

fn command_log_path(command_log_dir: &Path, id: u64, stream: &str) -> PathBuf {
    command_log_dir.join(format!("{id}.{stream}.log"))
}

async fn read_trailing_text(path: &Path, limit: Option<usize>) -> Result<String> {
    match fs::read(path).await {
        Ok(data) => {
            let limit = limit.unwrap_or(LOG_LIMIT);
            let start = data.len().saturating_sub(limit);
            Ok(String::from_utf8_lossy(&data[start..]).into_owned())
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
        Err(err) => Err(err).with_context(|| format!("failed to read {}", path.display())),
    }
}

fn final_stdout_summary(events: &[XtaskEvent], stdout: &[u8]) -> String {
    if events.is_empty() {
        truncate_log(String::from_utf8_lossy(stdout).as_ref())
    } else {
        summarize_xtask_events(events)
    }
}

fn live_stdout_summary(stdout: &str, events: &[XtaskEvent]) -> String {
    if events.is_empty() {
        stdout.to_string()
    } else {
        summarize_xtask_events(events)
    }
}

struct CommandProcessOutput {
    status: std::process::ExitStatus,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
    timed_out: bool,
    pid: Option<u32>,
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
pub struct CommandHandle {
    pub id: u64,
    pub command: String,
    pub timeout_ms: Option<u64>,
    pub status_path: PathBuf,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CommandStatus {
    pub id: u64,
    pub command: String,
    pub pid: Option<u32>,
    pub finished: bool,
    pub timed_out: bool,
    pub timeout_ms: Option<u64>,
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
    pub stdout_limit: Option<usize>,
    pub stderr_limit: Option<usize>,
    pub stdout_path: Option<PathBuf>,
    pub stderr_path: Option<PathBuf>,
    pub events: Vec<XtaskEvent>,
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
