use anyhow::{Context, Result, bail};
use serde::Serialize;
use serde_json::Value;
use std::{
    io::{self, Write},
    path::Path,
    process::{Command, Stdio},
};
use xshell::Shell;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputMode {
    Human,
    Json,
}

impl OutputMode {
    pub fn is_json(self) -> bool {
        matches!(self, Self::Json)
    }
}

#[derive(Debug, Serialize)]
pub struct JsonEvent<'a> {
    pub event: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub step: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Value>,
}

impl<'a> JsonEvent<'a> {
    pub fn started(command: &'a str) -> Self {
        Self {
            event: "started",
            command: Some(command),
            name: None,
            status: None,
            step: None,
            stream: None,
            message: None,
            output: None,
            exit_code: None,
            metadata: None,
        }
    }

    pub fn progress(command: &'a str, step: &'a str, message: &'a str) -> Self {
        Self {
            event: "progress",
            command: Some(command),
            name: None,
            status: None,
            step: Some(step),
            stream: None,
            message: Some(message),
            output: None,
            exit_code: None,
            metadata: None,
        }
    }

    pub fn test(command: &'a str, name: &'a str, status: &'a str, result: &'a str) -> Self {
        Self {
            event: "test",
            command: Some(command),
            name: Some(name),
            status: Some(status),
            step: None,
            stream: None,
            message: Some(result),
            output: None,
            exit_code: None,
            metadata: None,
        }
    }

    pub fn log(command: &'a str, stream: &'a str, output: &'a str) -> Self {
        Self {
            event: "log",
            command: Some(command),
            name: None,
            status: None,
            step: None,
            stream: Some(stream),
            message: None,
            output: Some(output),
            exit_code: None,
            metadata: None,
        }
    }

    pub fn metadata(command: &'a str, metadata: Value) -> Self {
        Self {
            event: "metadata",
            command: Some(command),
            name: None,
            status: None,
            step: None,
            stream: None,
            message: None,
            output: None,
            exit_code: None,
            metadata: Some(metadata),
        }
    }

    pub fn finished(command: &'a str, exit_code: i32, status: &'a str) -> Self {
        Self {
            event: "finished",
            command: Some(command),
            name: None,
            status: Some(status),
            step: None,
            stream: None,
            message: None,
            output: None,
            exit_code: Some(exit_code),
            metadata: None,
        }
    }
}

pub fn emit(event: &JsonEvent<'_>) -> Result<()> {
    let mut stdout = io::stdout().lock();
    serde_json::to_writer(&mut stdout, event).context("failed to encode JSON output event")?;
    stdout
        .write_all(b"\n")
        .context("failed to write JSON output newline")
}

pub fn remove_file(path: &Path, mode: OutputMode) -> Result<()> {
    if let Err(err) = std::fs::remove_file(path)
        && mode.is_json()
    {
        emit(&JsonEvent::log(
            "cleanup",
            "stderr",
            &format!("failed to remove {}: {err}", path.display()),
        ))?;
    }
    Ok(())
}

pub fn run_xshell_command(
    command_name: &str,
    sh: &Shell,
    cmd: xshell::Cmd,
    mode: OutputMode,
) -> Result<()> {
    if !mode.is_json() {
        let rendered = cmd.to_string();
        let mut command: Command = cmd.into();
        command.current_dir(sh.current_dir());
        let status = command
            .status()
            .with_context(|| format!("failed to run command: {rendered}"))?;
        if !status.success() {
            bail!("command failed with status {}: {rendered}", status);
        }
        return Ok(());
    }

    let rendered = cmd.to_string();
    let mut command: Command = cmd.into();
    command
        .current_dir(sh.current_dir())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let output = command
        .output()
        .with_context(|| format!("failed to run command: {rendered}"))?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    if !stdout.is_empty() {
        emit(&JsonEvent::log(command_name, "stdout", &stdout))?;
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    if !stderr.is_empty() {
        emit(&JsonEvent::log(command_name, "stderr", &stderr))?;
    }

    if !output.status.success() {
        bail!("command failed with status {}: {rendered}", output.status);
    }

    Ok(())
}
