use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    io::{self, Write},
    path::Path,
    process::{Command, Output, Stdio},
    sync::{Arc, Mutex},
};
use xshell::Shell;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum WorkflowEvent {
    Started {
        command: String,
    },
    Progress {
        command: String,
        step: String,
        message: String,
    },
    Test {
        command: String,
        name: String,
        status: TestStatus,
        message: String,
    },
    Log {
        command: String,
        stream: String,
        output: String,
    },
    Metadata {
        command: String,
        metadata: Value,
    },
    Finished {
        command: String,
        exit_code: i32,
        status: FinishStatus,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TestStatus {
    Running,
    Ok,
    Failed,
    Broken,
    Skipped,
}

impl TestStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Ok => "ok",
            Self::Failed => "failed",
            Self::Broken => "broken",
            Self::Skipped => "skipped",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FinishStatus {
    Ok,
    Failed,
}

impl FinishStatus {
    pub fn from_exit_code(exit_code: i32) -> Self {
        if exit_code == 0 {
            Self::Ok
        } else {
            Self::Failed
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::Failed => "failed",
        }
    }
}

pub trait WorkflowReporter: Send + Sync {
    fn emit(&self, event: WorkflowEvent) -> Result<()>;

    fn capture_subprocess_output(&self) -> bool {
        true
    }
}

#[derive(Debug, Default)]
pub struct HumanReporter;

impl WorkflowReporter for HumanReporter {
    fn emit(&self, event: WorkflowEvent) -> Result<()> {
        match event {
            WorkflowEvent::Started { command } => {
                eprintln!("started {command}");
            }
            WorkflowEvent::Progress { step, message, .. } => {
                eprintln!("{step}: {message}");
            }
            WorkflowEvent::Test {
                name,
                status,
                message,
                ..
            } => match status {
                TestStatus::Running => eprintln!("test {name} ... running"),
                TestStatus::Ok => eprintln!("test {name} ... ok"),
                TestStatus::Failed | TestStatus::Broken => {
                    eprintln!("test {name} ... {}", status.as_str());
                    if !message.is_empty() {
                        eprintln!("{message}");
                    }
                }
                TestStatus::Skipped => eprintln!("test {name} ... skipped"),
            },
            WorkflowEvent::Log { stream, output, .. } => {
                if stream == "stderr" {
                    eprint!("{output}");
                    io::stderr().flush().ok();
                } else {
                    print!("{output}");
                    io::stdout().flush().ok();
                }
            }
            WorkflowEvent::Metadata { metadata, .. } => {
                println!("{metadata}");
            }
            WorkflowEvent::Finished {
                command,
                exit_code,
                status,
            } => {
                eprintln!("finished {command}: {} (exit {exit_code})", status.as_str());
            }
        }
        Ok(())
    }

    fn capture_subprocess_output(&self) -> bool {
        false
    }
}

#[derive(Debug, Default, Clone)]
pub struct EventCollector {
    events: Arc<Mutex<Vec<WorkflowEvent>>>,
}

impl EventCollector {
    pub fn events(&self) -> Vec<WorkflowEvent> {
        self.events
            .lock()
            .map(|events| events.clone())
            .unwrap_or_default()
    }
}

impl WorkflowReporter for EventCollector {
    fn emit(&self, event: WorkflowEvent) -> Result<()> {
        self.events
            .lock()
            .map_err(|_| anyhow::anyhow!("workflow event collector lock poisoned"))?
            .push(event);
        Ok(())
    }
}

pub fn started(reporter: &dyn WorkflowReporter, command: &str) -> Result<()> {
    reporter.emit(WorkflowEvent::Started {
        command: command.to_string(),
    })
}

pub fn progress(
    reporter: &dyn WorkflowReporter,
    command: &str,
    step: &str,
    message: &str,
) -> Result<()> {
    reporter.emit(WorkflowEvent::Progress {
        command: command.to_string(),
        step: step.to_string(),
        message: message.to_string(),
    })
}

pub fn test_event(
    reporter: &dyn WorkflowReporter,
    command: &str,
    name: &str,
    status: TestStatus,
    message: &str,
) -> Result<()> {
    reporter.emit(WorkflowEvent::Test {
        command: command.to_string(),
        name: name.to_string(),
        status,
        message: message.to_string(),
    })
}

pub fn log_event(
    reporter: &dyn WorkflowReporter,
    command: &str,
    stream: &str,
    output: &str,
) -> Result<()> {
    reporter.emit(WorkflowEvent::Log {
        command: command.to_string(),
        stream: stream.to_string(),
        output: output.to_string(),
    })
}

pub fn log_command_output_on_failure(
    reporter: &dyn WorkflowReporter,
    command_name: &str,
    output: &Output,
) -> Result<()> {
    if output.status.success() {
        return Ok(());
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    if !stdout.is_empty() {
        log_event(reporter, command_name, "stdout", &stdout)?;
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    if !stderr.is_empty() {
        log_event(reporter, command_name, "stderr", &stderr)?;
    }
    Ok(())
}

pub fn metadata_event(
    reporter: &dyn WorkflowReporter,
    command: &str,
    metadata: Value,
) -> Result<()> {
    reporter.emit(WorkflowEvent::Metadata {
        command: command.to_string(),
        metadata,
    })
}

pub fn finished(
    reporter: &dyn WorkflowReporter,
    command: &str,
    exit_code: i32,
    status: FinishStatus,
) -> Result<()> {
    reporter.emit(WorkflowEvent::Finished {
        command: command.to_string(),
        exit_code,
        status,
    })
}

pub fn remove_file(path: &Path, reporter: &dyn WorkflowReporter) -> Result<()> {
    if let Err(err) = std::fs::remove_file(path) {
        log_event(
            reporter,
            "cleanup",
            "stderr",
            &format!("failed to remove {}: {err}\n", path.display()),
        )?;
    }
    Ok(())
}

pub fn run_xshell_command(
    command_name: &str,
    sh: &Shell,
    cmd: xshell::Cmd,
    reporter: &dyn WorkflowReporter,
) -> Result<()> {
    let rendered = cmd.to_string();
    let mut command: Command = cmd.into();
    command.current_dir(sh.current_dir());

    if !reporter.capture_subprocess_output() {
        let status = command
            .status()
            .with_context(|| format!("failed to run command: {rendered}"))?;
        if !status.success() {
            bail!("command failed with status {}: {rendered}", status);
        }
        return Ok(());
    }

    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    let output = command
        .output()
        .with_context(|| format!("failed to run command: {rendered}"))?;
    log_command_output_on_failure(reporter, command_name, &output)?;
    if !output.status.success() {
        bail!("command failed with status {}: {rendered}", output.status);
    }

    Ok(())
}
