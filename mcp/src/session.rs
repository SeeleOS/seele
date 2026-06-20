use anyhow::{Context, Result, bail};
use seele_workflows::{
    build_rootfs::{BuildRootfsConfig, build_rootfs},
    reporter::{EventCollector, WorkflowEvent},
    test::test as run_workflow_tests,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    env,
    path::{Path, PathBuf},
    process::Stdio,
    sync::{
        Arc, Mutex as StdMutex, OnceLock,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
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
const COMMAND_STATUS_REFRESH: Duration = Duration::from_secs(1);
const STARTUP_METADATA_TIMEOUT: Duration = Duration::from_secs(10 * 60);
const STARTUP_METADATA_REFRESH: Duration = Duration::from_millis(100);
const SERIAL_WAIT_REFRESH: Duration = Duration::from_millis(200);

#[derive(Debug, Clone, Serialize)]
pub struct SessionStatus {
    pub running: bool,
    pub runner_pid: Option<u32>,
    pub qemu_pid: Option<u32>,
    pub qmp_socket: PathBuf,
    pub qmp_connectable: bool,
    pub serial_log: PathBuf,
    pub serial_log_exists: bool,
    pub mcp_run_log: PathBuf,
    pub mcp_run_log_exists: bool,
    pub last_exit: Option<i32>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SessionMetadata {
    pub runner_pid: u32,
    pub qemu_pid: Option<u32>,
    pub qmp_socket: PathBuf,
    pub serial_log: PathBuf,
    pub mcp_run_log: PathBuf,
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
    commands: Mutex<HashMap<u64, CommandTask>>,
}

#[derive(Debug)]
struct CommandTask {
    status_path: PathBuf,
    command: String,
    timeout_ms: Option<u64>,
    cancelled: Arc<AtomicBool>,
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
            commands: Mutex::new(HashMap::new()),
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
                let _ = stop_state(
                    &mut state,
                    &self.qmp_socket,
                    &self.serial_log,
                    &self.command_log_dir.join("mcp-run.log"),
                )
                .await;
                return Err(err);
            }
        };

        let mut state = self.state.lock().await;
        state.gdb = Some(gdb);

        Ok(DebugStartStatus {
            metadata,
            gdb_port: port,
            startup_output,
            connect_output,
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
        fs::create_dir_all(&self.command_log_dir)
            .await
            .with_context(|| format!("failed to create {}", self.command_log_dir.display()))?;
        let mcp_run_log = self.command_log_dir.join("mcp-run.log");
        let _ = fs::remove_file(&mcp_run_log).await;
        let log = std::fs::File::create(&mcp_run_log)
            .with_context(|| format!("failed to create {}", mcp_run_log.display()))?;
        let log_for_stderr = log
            .try_clone()
            .with_context(|| format!("failed to clone {}", mcp_run_log.display()))?;

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
            .stdout(Stdio::from(log))
            .stderr(Stdio::from(log_for_stderr));
        for (key, value) in envs {
            command.env(key, value);
        }
        let child = command.spawn().context("failed to start xtask mcp-run")?;
        let metadata = SessionMetadata {
            runner_pid: child.id().context("xtask mcp-run pid missing")?,
            qemu_pid: None,
            qmp_socket: self.qmp_socket.clone(),
            serial_log: self.serial_log.clone(),
            mcp_run_log,
            iso_image: None,
        };
        state.child = Some(child);
        state.metadata = Some(metadata.clone());
        state.last_exit = None;
        drop(state);

        self.wait_for_startup_metadata(STARTUP_METADATA_TIMEOUT)
            .await
    }

    async fn wait_for_startup_metadata(
        &self,
        timeout_duration: Duration,
    ) -> Result<SessionMetadata> {
        let deadline = Instant::now() + timeout_duration;
        let mcp_run_log = self.command_log_dir.join("mcp-run.log");
        loop {
            let mut state = self.state.lock().await;
            refresh_child_state(&mut state).await;
            refresh_metadata_from_log(&mut state, &mcp_run_log, &self.qmp_socket, &self.serial_log)
                .await;
            if let Some(metadata) = state
                .metadata
                .as_mut()
                .filter(|metadata| metadata.qemu_pid.is_some())
            {
                return Ok(metadata.clone());
            }
            if state.child.is_none() {
                bail!("xtask mcp-run exited before QEMU metadata became available");
            }
            drop(state);

            if Instant::now() >= deadline {
                let mut state = self.state.lock().await;
                let _ = stop_state(
                    &mut state,
                    &self.qmp_socket,
                    &self.serial_log,
                    &self.command_log_dir.join("mcp-run.log"),
                )
                .await;
                bail!("timed out waiting for QEMU metadata from xtask mcp-run");
            }
            tokio::time::sleep(STARTUP_METADATA_REFRESH).await;
        }
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
        stop_state(
            &mut state,
            &self.qmp_socket,
            &self.serial_log,
            &self.command_log_dir.join("mcp-run.log"),
        )
        .await?;
        drop(state);
        let _ = fs::remove_file(&self.qmp_socket).await;
        self.status().await
    }

    pub async fn cleanup(&self) -> Result<SessionStatus> {
        let cleanup_qmp_socket = {
            let mut state = self.state.lock().await;
            refresh_metadata_from_log(
                &mut state,
                &self.command_log_dir.join("mcp-run.log"),
                &self.qmp_socket,
                &self.serial_log,
            )
            .await;
            state
                .metadata
                .as_ref()
                .map(|metadata| metadata.qmp_socket.clone())
                .unwrap_or_else(|| self.qmp_socket.clone())
        };
        let mut state = self.state.lock().await;
        stop_state(
            &mut state,
            &self.qmp_socket,
            &self.serial_log,
            &self.command_log_dir.join("mcp-run.log"),
        )
        .await?;
        drop(state);
        let _ = fs::remove_file(&self.qmp_socket).await;
        let residual = qemu_pids_for_qmp_socket(&cleanup_qmp_socket).await;
        if !residual.is_empty() {
            bail!(
                "cleanup left QEMU processes running for {}: {residual:?}",
                cleanup_qmp_socket.display()
            );
        }
        self.status().await
    }

    pub async fn status(&self) -> Result<SessionStatus> {
        let mut state = self.state.lock().await;
        refresh_child_state(&mut state).await;
        refresh_metadata_from_log(
            &mut state,
            &self.command_log_dir.join("mcp-run.log"),
            &self.qmp_socket,
            &self.serial_log,
        )
        .await;
        let mut metadata = state.metadata.clone();
        let status_qmp_socket = metadata
            .as_ref()
            .map(|metadata| metadata.qmp_socket.clone())
            .unwrap_or_else(|| self.qmp_socket.clone());
        let status_serial_log = metadata
            .as_ref()
            .map(|metadata| metadata.serial_log.clone())
            .unwrap_or_else(|| self.serial_log.clone());
        let qmp_connectable = tokio::net::UnixStream::connect(&status_qmp_socket)
            .await
            .is_ok();
        let qemu_pids = qemu_pids_for_qmp_socket(&status_qmp_socket).await;
        if let Some(metadata) = metadata.as_mut()
            && metadata.qemu_pid.is_none()
        {
            metadata.qemu_pid = qemu_pids.first().copied();
            state.metadata = Some(metadata.clone());
        }
        let metadata_qemu_pid = metadata
            .as_ref()
            .and_then(|metadata| metadata.qemu_pid)
            .filter(|pid| qemu_pids.contains(pid));
        let qemu_pid = metadata_qemu_pid.or_else(|| qemu_pids.first().copied());
        let running = state.child.is_some() || qmp_connectable || qemu_pid.is_some();
        let serial_log_exists = fs::metadata(&status_serial_log).await.is_ok();
        let mcp_run_log = metadata
            .as_ref()
            .map(|metadata| metadata.mcp_run_log.clone())
            .unwrap_or_else(|| self.command_log_dir.join("mcp-run.log"));
        let mcp_run_log_exists = fs::metadata(&mcp_run_log).await.is_ok();

        Ok(SessionStatus {
            running,
            runner_pid: metadata.as_ref().map(|metadata| metadata.runner_pid),
            qemu_pid,
            qmp_socket: status_qmp_socket,
            qmp_connectable,
            serial_log: status_serial_log,
            serial_log_exists,
            mcp_run_log,
            mcp_run_log_exists,
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
                output,
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
        stop_state(
            &mut state,
            &self.qmp_socket,
            &self.serial_log,
            &self.command_log_dir.join("mcp-run.log"),
        )
        .await?;
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

    pub async fn wait_serial(
        &self,
        pattern: &str,
        timeout_ms: Option<u64>,
        lines: Option<usize>,
        bytes: Option<usize>,
    ) -> Result<String> {
        let deadline = Instant::now() + Duration::from_millis(timeout_ms.unwrap_or(30_000));
        loop {
            let tail = self.serial_tail(lines, bytes).await.unwrap_or_default();
            if tail.contains(pattern) {
                return Ok(tail);
            }
            if Instant::now() >= deadline {
                bail!("timed out waiting for serial pattern {pattern:?}");
            }
            tokio::time::sleep(SERIAL_WAIT_REFRESH).await;
        }
    }

    pub async fn start_xtest(
        &self,
        test: Option<&str>,
        ltp_pattern: Option<&str>,
        ltp_suite: Option<&str>,
    ) -> Result<CommandHandle> {
        self.start_workflow_command(
            WorkflowJob::Test {
                test: test.map(str::to_string),
                ltp_pattern: ltp_pattern.map(str::to_string),
                ltp_suite: ltp_suite.map(str::to_string),
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
        let status: CommandStatus = serde_json::from_slice(&data)
            .with_context(|| format!("failed to parse command status {}", path.display()))?;
        if status.finished {
            self.commands.lock().await.remove(&id);
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

    pub async fn command_cancel(&self, id: u64) -> Result<CommandStatus> {
        let task = self
            .commands
            .lock()
            .await
            .remove(&id)
            .with_context(|| format!("command {id} is not running or is unknown"))?;
        task.cancelled.store(true, Ordering::Release);
        for qemu_pid in qemu_pids_for_qmp_socket(&self.qmp_socket).await {
            kill_pid(qemu_pid).await;
        }
        let status = command_status_from_parts(
            id,
            &task.command,
            None,
            true,
            false,
            task.timeout_ms,
            130,
            Vec::new(),
            String::new(),
            format!("cancelled command {id}"),
        );
        write_command_status(&task.status_path, &status).await?;
        Ok(status)
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
            stdout_limit: None,
            stderr_limit: None,
            stdout_path: None,
            stderr_path: None,
            events: Vec::new(),
        };
        fs::write(&log_path, serde_json::to_vec_pretty(&initial_status)?)
            .await
            .with_context(|| format!("failed to write command status {}", log_path.display()))?;

        let log_path_for_task = log_path.clone();
        let command_for_task = command.clone();
        let cancelled = Arc::new(AtomicBool::new(false));
        let cancelled_for_task = cancelled.clone();
        tokio::spawn(async move {
            run_workflow_job(
                id,
                command_for_task,
                timeout_ms,
                log_path_for_task,
                job,
                cancelled_for_task,
            )
            .await;
        });
        self.commands.lock().await.insert(
            id,
            CommandTask {
                status_path: log_path.clone(),
                command: command.clone(),
                timeout_ms,
                cancelled,
            },
        );

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
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
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
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
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
    Test {
        test: Option<String>,
        ltp_pattern: Option<String>,
        ltp_suite: Option<String>,
    },
    BuildRootfs {
        config: BuildRootfsConfig,
    },
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
    cancelled: Arc<AtomicBool>,
) {
    let collector = EventCollector::default();
    let started = std::time::Instant::now();
    let handle = tokio::task::spawn_blocking({
        let collector = collector.clone();
        move || match job {
            WorkflowJob::Test {
                test,
                ltp_pattern,
                ltp_suite,
            } => with_optional_env_vars(
                [
                    ("SEELE_LTP_PATTERN", ltp_pattern.as_deref()),
                    ("SEELE_LTP_SUITE", ltp_suite.as_deref()),
                ],
                || run_workflow_tests(&collector, test.as_deref()),
            ),
            WorkflowJob::BuildRootfs { config } => build_rootfs(config, &collector),
        }
    });

    let timed_out = false;
    let exit_code = loop {
        if cancelled.load(Ordering::Acquire) {
            let events = collector.events();
            let status = command_status_from_parts(
                id,
                &command,
                None,
                true,
                false,
                timeout_ms,
                130,
                events,
                String::new(),
                format!("cancelled command {id}"),
            );
            let _ = write_command_status(&status_path, &status).await;
            return;
        }

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
                        err.to_string(),
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
    if cancelled.load(Ordering::Acquire) {
        let status = command_status_from_parts(
            id,
            &command,
            None,
            true,
            false,
            timeout_ms,
            130,
            events,
            stdout,
            format!("cancelled command {id}"),
        );
        let _ = write_command_status(&status_path, &status).await;
        return;
    }
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

fn with_optional_env_vars<T, const N: usize>(
    vars: [(&str, Option<&str>); N],
    f: impl FnOnce() -> T,
) -> T {
    static ENV_LOCK: OnceLock<StdMutex<()>> = OnceLock::new();
    let _guard = ENV_LOCK.get_or_init(|| StdMutex::new(())).lock().unwrap();
    let previous = vars
        .iter()
        .map(|(key, _)| (*key, env::var_os(key)))
        .collect::<Vec<_>>();

    unsafe {
        for (key, value) in vars {
            if let Some(value) = value {
                env::set_var(key, value);
            } else {
                env::remove_var(key);
            }
        }
    }

    let result = f();

    unsafe {
        for (key, value) in previous {
            if let Some(value) = value {
                env::set_var(key, value);
            } else {
                env::remove_var(key);
            }
        }
    }

    result
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
        err.to_string(),
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
        stdout,
        stderr,
        stdout_limit: None,
        stderr_limit: None,
        stdout_path: None,
        stderr_path: None,
        events,
    }
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
            WorkflowEvent::Log { output, .. } => lines.push(output.clone()),
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
    lines.join("\n")
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

async fn refresh_metadata_from_log(
    state: &mut SessionState,
    mcp_run_log: &Path,
    default_qmp_socket: &Path,
    default_serial_log: &Path,
) {
    let Ok(data) = fs::read_to_string(mcp_run_log).await else {
        return;
    };
    let Some(metadata) = parse_metadata_from_log(
        &data,
        Some(mcp_run_log),
        default_qmp_socket,
        default_serial_log,
    ) else {
        return;
    };
    state.metadata = Some(metadata);
}

fn parse_metadata_from_log(
    log: &str,
    mcp_run_log: Option<&Path>,
    default_qmp_socket: &Path,
    default_serial_log: &Path,
) -> Option<SessionMetadata> {
    log.lines()
        .rev()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .find_map(|value| {
            parse_metadata_value(&value, mcp_run_log, default_qmp_socket, default_serial_log).ok()
        })
}

fn parse_metadata_value(
    value: &serde_json::Value,
    mcp_run_log: Option<&Path>,
    default_qmp_socket: &Path,
    default_serial_log: &Path,
) -> Result<SessionMetadata> {
    let runner_pid = value
        .get("runner_pid")
        .and_then(serde_json::Value::as_u64)
        .context("metadata missing runner_pid")?;
    let qemu_pid = value
        .get("qemu_pid")
        .and_then(serde_json::Value::as_u64)
        .map(u32::try_from)
        .transpose()
        .context("metadata qemu_pid out of range")?;
    let qmp_socket = value
        .get("qmp_socket")
        .and_then(serde_json::Value::as_str)
        .map(PathBuf::from)
        .unwrap_or_else(|| default_qmp_socket.to_path_buf());
    let serial_log = value
        .get("serial_log")
        .and_then(serde_json::Value::as_str)
        .map(PathBuf::from)
        .unwrap_or_else(|| default_serial_log.to_path_buf());
    let iso_image = value
        .get("iso_image")
        .and_then(serde_json::Value::as_str)
        .map(PathBuf::from);

    Ok(SessionMetadata {
        runner_pid: u32::try_from(runner_pid).context("metadata runner_pid out of range")?,
        qemu_pid,
        qmp_socket,
        serial_log,
        mcp_run_log: mcp_run_log
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from(DEFAULT_COMMAND_LOG_DIR).join("mcp-run.log")),
        iso_image,
    })
}

async fn stop_state(
    state: &mut SessionState,
    default_qmp_socket: &Path,
    default_serial_log: &Path,
    default_mcp_run_log: &Path,
) -> Result<()> {
    stop_gdb_state(state).await?;
    refresh_metadata_from_log(
        state,
        default_mcp_run_log,
        default_qmp_socket,
        default_serial_log,
    )
    .await;
    if let Some(qemu_pid) = state
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.qemu_pid)
    {
        kill_pid(qemu_pid).await;
    }
    let qmp_socket = state
        .metadata
        .as_ref()
        .map(|metadata| metadata.qmp_socket.as_path())
        .unwrap_or(default_qmp_socket);
    for qemu_pid in qemu_pids_for_qmp_socket(qmp_socket).await {
        kill_pid(qemu_pid).await;
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

async fn kill_pid(pid: u32) {
    let _ = Command::new("kill").arg(pid.to_string()).status().await;
    for _ in 0..10 {
        if !process_exists(pid).await {
            return;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let _ = Command::new("kill")
        .arg("-9")
        .arg(pid.to_string())
        .status()
        .await;
}

async fn process_exists(pid: u32) -> bool {
    fs::metadata(format!("/proc/{pid}")).await.is_ok()
}

async fn qemu_pids_for_qmp_socket(qmp_socket: &Path) -> Vec<u32> {
    let Ok(mut entries) = fs::read_dir("/proc").await else {
        return Vec::new();
    };
    let mut result = Vec::new();
    while let Ok(Some(entry)) = entries.next_entry().await {
        let Some(pid) = entry
            .file_name()
            .to_str()
            .and_then(|name| name.parse::<u32>().ok())
        else {
            continue;
        };
        let cmdline_path = entry.path().join("cmdline");
        let Ok(cmdline) = fs::read(&cmdline_path).await else {
            continue;
        };
        if qemu_cmdline_uses_qmp_socket(&cmdline, qmp_socket) {
            result.push(pid);
        }
    }
    result
}

fn qemu_cmdline_uses_qmp_socket(cmdline: &[u8], qmp_socket: &Path) -> bool {
    let args = cmdline
        .split(|byte| *byte == 0)
        .filter(|arg| !arg.is_empty())
        .filter_map(|arg| std::str::from_utf8(arg).ok())
        .collect::<Vec<_>>();
    if !args
        .first()
        .is_some_and(|program| program.contains("qemu-system"))
    {
        return false;
    }
    let socket = qmp_socket.to_string_lossy();
    args.windows(2).any(|window| {
        window[0] == "-qmp"
            && window[1]
                .strip_prefix("unix:")
                .and_then(|value| value.split(',').next())
                .is_some_and(|path| path == socket)
    })
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

    #[test]
    fn parse_metadata_from_log_uses_latest_metadata_line() {
        let log = r#"
ignored
{"runner_pid":1,"qemu_pid":2,"qmp_socket":"/tmp/old.sock"}
finished build
{"runner_pid":3,"qemu_pid":4,"qmp_socket":"/tmp/new.sock","serial_log":"/tmp/new.log","iso_image":"/tmp/kernel.iso"}
"#;
        let metadata = parse_metadata_from_log(
            log,
            Some(Path::new("/tmp/mcp-run.log")),
            Path::new("/tmp/default-qmp.sock"),
            Path::new("/tmp/default-serial.log"),
        )
        .unwrap();
        assert_eq!(metadata.runner_pid, 3);
        assert_eq!(metadata.qemu_pid, Some(4));
        assert_eq!(metadata.qmp_socket, PathBuf::from("/tmp/new.sock"));
        assert_eq!(metadata.serial_log, PathBuf::from("/tmp/new.log"));
        assert_eq!(metadata.mcp_run_log, PathBuf::from("/tmp/mcp-run.log"));
        assert_eq!(metadata.iso_image, Some(PathBuf::from("/tmp/kernel.iso")));
    }

    #[test]
    fn qemu_cmdline_matches_exact_qmp_socket() {
        let cmdline = b"qemu-system-x86_64\0-m\04G\0-qmp\0unix:/tmp/seele-agent-qmp.sock,server=on,wait=off\0";
        assert!(qemu_cmdline_uses_qmp_socket(
            cmdline,
            Path::new("/tmp/seele-agent-qmp.sock")
        ));
        assert!(!qemu_cmdline_uses_qmp_socket(
            cmdline,
            Path::new("/tmp/other.sock")
        ));
    }
}
