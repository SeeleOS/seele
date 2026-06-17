use anyhow::{Context, Result, bail};
use seele_workflows::{
    build_rootfs::{BuildRootfsConfig, build_rootfs},
    reporter::{EventCollector, WorkflowEvent},
    test::test as run_workflow_tests,
};
use serde::{Deserialize, Serialize};
use std::{
    env,
    path::{Path, PathBuf},
    process::Stdio,
    sync::atomic::{AtomicU64, Ordering},
};
use tokio::{
    fs,
    io::{AsyncReadExt, AsyncWriteExt, BufReader},
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
            .arg("mcp-run");
        if enable_profiling {
            command.arg("--enable-profiling");
        }
        command
            .current_dir(&self.repo)
            .env("SEELE_QMP_SOCKET", &self.qmp_socket)
            .env("SEELE_SERIAL_LOG", &self.serial_log)
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        for (key, value) in envs {
            command.env(key, value);
        }
        let child = command.spawn().context("failed to start xtask mcp-run")?;
        let metadata = SessionMetadata {
            runner_pid: child.id().context("xtask mcp-run pid missing")?,
            qemu_pid: None,
            qmp_socket: self.qmp_socket.clone(),
            serial_log: self.serial_log.clone(),
            iso_image: None,
        };
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
        self.start_workflow_command(
            WorkflowJob::Test {
                test: test.map(str::to_string),
            },
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
        self.start_workflow_command(
            WorkflowJob::BuildRootfs {
                config: BuildRootfsConfig {
                    override_rootfs,
                    rebuild_aur,
                    rebuild_aur_packages: rebuild_aur_packages.to_vec(),
                    passthrough: Vec::new(),
                },
            },
            timeout_ms,
        )
        .await
    }

    pub async fn command_status(&self, id: u64) -> Result<CommandStatus> {
        let path = self.command_log_dir.join(format!("{id}.json"));
        let data = fs::read(&path)
            .await
            .with_context(|| format!("failed to read command status {}", path.display()))?;
        serde_json::from_slice(&data)
            .with_context(|| format!("failed to parse command status {}", path.display()))
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

    async fn start_workflow_command(
        &self,
        job: WorkflowJob,
        timeout_ms: Option<u64>,
    ) -> Result<CommandHandle> {
        fs::create_dir_all(&self.command_log_dir)
            .await
            .with_context(|| format!("failed to create {}", self.command_log_dir.display()))?;
        let id = self.next_command_id.fetch_add(1, Ordering::Relaxed);
        let log_path = self.command_log_dir.join(format!("{id}.json"));
        let command = job.command_name().to_string();
        let initial_status = CommandStatus {
            id,
            command: command.clone(),
            pid: None,
            finished: false,
            timed_out: false,
            timeout_ms,
            exit_code: 0,
            stdout: String::new(),
            stderr: String::new(),
            stdout_limit: Some(LOG_LIMIT),
            stderr_limit: Some(LOG_LIMIT),
            stdout_path: None,
            stderr_path: None,
            events: Vec::new(),
        };
        fs::write(&log_path, serde_json::to_vec_pretty(&initial_status)?)
            .await
            .with_context(|| format!("failed to write command status {}", log_path.display()))?;

        let log_path_for_task = log_path.clone();
        let command_for_task = command.clone();
        tokio::spawn(async move {
            run_workflow_job(id, command_for_task, timeout_ms, log_path_for_task, job).await;
        });

        Ok(CommandHandle {
            id,
            command,
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
    pub events: Vec<WorkflowEvent>,
}

#[derive(Debug, Serialize)]
pub struct CommandOutput {
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
    pub events: Vec<WorkflowEvent>,
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

#[derive(Debug)]
enum WorkflowJob {
    Test { test: Option<String> },
    BuildRootfs { config: BuildRootfsConfig },
}

impl WorkflowJob {
    fn command_name(&self) -> &'static str {
        match self {
            Self::Test { .. } => "test",
            Self::BuildRootfs { .. } => "build-rootfs",
        }
    }
}

async fn run_workflow_job(
    id: u64,
    command: String,
    timeout_ms: Option<u64>,
    status_path: PathBuf,
    job: WorkflowJob,
) {
    let collector = EventCollector::default();
    let started = std::time::Instant::now();
    let handle = tokio::task::spawn_blocking({
        let collector = collector.clone();
        move || match job {
            WorkflowJob::Test { test } => run_workflow_tests(&collector, test.as_deref()),
            WorkflowJob::BuildRootfs { config } => build_rootfs(config, &collector),
        }
    });

    let timed_out = false;
    let exit_code = loop {
        if let Some(limit_ms) = timeout_ms
            && started.elapsed() >= Duration::from_millis(limit_ms)
        {
            handle.abort();
            let events = collector.events();
            let stdout = summarize_workflow_events(&events);
            let status = command_status_from_parts(
                id,
                &command,
                None,
                true,
                true,
                timeout_ms,
                1,
                events,
                stdout,
                format!("timed out waiting for command {id}"),
            );
            let _ = write_command_status(&status_path, &status).await;
            return;
        }

        if handle.is_finished() {
            break match handle.await {
                Ok(Ok(code)) => code,
                Ok(Err(err)) => {
                    let status = status_from_error(
                        id,
                        &command,
                        timeout_ms,
                        timed_out,
                        collector.events(),
                        err,
                    );
                    let _ = write_command_status(&status_path, &status).await;
                    return;
                }
                Err(err) => {
                    let status = command_status_from_parts(
                        id,
                        &command,
                        None,
                        true,
                        timed_out,
                        timeout_ms,
                        1,
                        collector.events(),
                        String::new(),
                        truncate_log(&err.to_string()),
                    );
                    let _ = write_command_status(&status_path, &status).await;
                    return;
                }
            };
        }

        let events = collector.events();
        let stdout = summarize_workflow_events(&events);
        let status = command_status_from_parts(
            id,
            &command,
            None,
            false,
            false,
            timeout_ms,
            0,
            events,
            stdout,
            String::new(),
        );
        let _ = write_command_status(&status_path, &status).await;
        tokio::time::sleep(COMMAND_STATUS_REFRESH).await;
    };

    let events = collector.events();
    let stdout = summarize_workflow_events(&events);
    let status = command_status_from_parts(
        id,
        &command,
        None,
        true,
        timed_out,
        timeout_ms,
        exit_code,
        events,
        stdout,
        String::new(),
    );
    let _ = write_command_status(&status_path, &status).await;
}

async fn write_command_status(path: &Path, status: &CommandStatus) -> Result<()> {
    fs::write(path, serde_json::to_vec_pretty(status)?)
        .await
        .with_context(|| format!("failed to write command status {}", path.display()))
}

fn status_from_error(
    id: u64,
    command: &str,
    timeout_ms: Option<u64>,
    timed_out: bool,
    events: Vec<WorkflowEvent>,
    err: anyhow::Error,
) -> CommandStatus {
    command_status_from_parts(
        id,
        command,
        None,
        true,
        timed_out,
        timeout_ms,
        1,
        events,
        String::new(),
        truncate_log(&err.to_string()),
    )
}

#[allow(clippy::too_many_arguments)]
fn command_status_from_parts(
    id: u64,
    command: &str,
    pid: Option<u32>,
    finished: bool,
    timed_out: bool,
    timeout_ms: Option<u64>,
    exit_code: i32,
    events: Vec<WorkflowEvent>,
    stdout: String,
    stderr: String,
) -> CommandStatus {
    CommandStatus {
        id,
        command: command.to_string(),
        pid,
        finished,
        timed_out,
        timeout_ms,
        exit_code,
        stdout: truncate_log(&stdout),
        stderr: truncate_log(&stderr),
        stdout_limit: Some(LOG_LIMIT),
        stderr_limit: Some(LOG_LIMIT),
        stdout_path: None,
        stderr_path: None,
        events: truncate_workflow_events(events),
    }
}

fn truncate_workflow_events(events: Vec<WorkflowEvent>) -> Vec<WorkflowEvent> {
    events
        .into_iter()
        .map(|event| match event {
            WorkflowEvent::Log {
                command,
                stream,
                output,
            } => WorkflowEvent::Log {
                command,
                stream,
                output: truncate_log(&output),
            },
            other => other,
        })
        .collect()
}

fn summarize_workflow_events(events: &[WorkflowEvent]) -> String {
    let mut lines = Vec::new();
    for event in events {
        match event {
            WorkflowEvent::Started { command } => lines.push(format!("started {command}")),
            WorkflowEvent::Progress { step, message, .. } => {
                lines.push(format!("{step}: {message}"));
            }
            WorkflowEvent::Test {
                name,
                status,
                message,
                ..
            } => {
                if message.is_empty() {
                    lines.push(format!("test {name}: {}", status.as_str()));
                } else {
                    lines.push(format!("test {name}: {} - {message}", status.as_str()));
                }
            }
            WorkflowEvent::Log { output, .. } => lines.push(truncate_log(output)),
            WorkflowEvent::Metadata { metadata, .. } => lines.push(metadata.to_string()),
            WorkflowEvent::Finished {
                command,
                exit_code,
                status,
            } => lines.push(format!(
                "finished {command}: {} (exit {exit_code})",
                status.as_str()
            )),
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
