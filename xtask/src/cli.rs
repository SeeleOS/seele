use crate::qemu::RunOptions;
use anyhow::{Result, bail};
use std::{env, time::Duration};

pub enum Command {
    Run(RunOptions),
    McpRun,
    Test,
    IntegrationTest,
    Rootfs(RootfsCommand),
    SysrootMount,
    VmPs,
}

pub enum RootfsCommand {
    Build { override_disk: bool },
}

pub fn parse(args: impl IntoIterator<Item = String>) -> Result<Command> {
    let mut args = args.into_iter();
    let Some(command) = args.next() else {
        bail!("missing xtask command");
    };

    match command.as_str() {
        "run" => parse_run(args),
        "mcp-run" => Ok(Command::McpRun),
        "test" => Ok(Command::Test),
        "integration-test" => Ok(Command::IntegrationTest),
        "rootfs-build" => parse_rootfs_build(args),
        "sysroot-mount" => Ok(Command::SysrootMount),
        "vm-ps" => Ok(Command::VmPs),
        _ => bail!("unknown xtask command: {command}"),
    }
}

fn parse_run(args: impl IntoIterator<Item = String>) -> Result<Command> {
    let mut options = RunOptions::from_env();
    for arg in args {
        match arg.as_str() {
            "--" => {}
            "--agent" => options.agent_mode = true,
            other => bail!("unknown run argument: {other}"),
        }
    }
    Ok(Command::Run(options))
}

fn parse_rootfs_build(args: impl IntoIterator<Item = String>) -> Result<Command> {
    let mut override_disk = false;
    for arg in args {
        match arg.as_str() {
            "--override" => override_disk = true,
            other => bail!("unknown rootfs-build argument: {other}"),
        }
    }
    Ok(Command::Rootfs(RootfsCommand::Build { override_disk }))
}

pub fn parse_duration(value: &str) -> Option<Duration> {
    let value = value.trim();
    if let Some(milliseconds) = value.strip_suffix("ms") {
        return milliseconds.parse::<u64>().ok().map(Duration::from_millis);
    }
    if let Some(seconds) = value.strip_suffix('s') {
        return seconds.parse::<u64>().ok().map(Duration::from_secs);
    }
    if let Some(minutes) = value.strip_suffix('m') {
        return minutes
            .parse::<u64>()
            .ok()
            .map(|minutes| Duration::from_secs(minutes.saturating_mul(60)));
    }
    value.parse::<u64>().ok().map(Duration::from_secs)
}

pub fn repo_root() -> Result<std::path::PathBuf> {
    Ok(env::current_dir()?)
}
