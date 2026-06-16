use crate::qemu::RunOptions;
use anyhow::Result;
use clap::{Args, Parser, Subcommand};
use std::{env, path::PathBuf, time::Duration};

#[derive(Debug, Parser)]
#[command(version, about = "Seele OS development tasks")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    Run(RunArgs),
    McpRun,
    Test,
    IntegrationTest,
    RootfsBuild(RootfsBuildArgs),
    SysrootMount,
    VmPs,
}

#[derive(Debug, Args)]
pub struct RunArgs {
    #[arg(long)]
    pub agent: bool,
}

#[derive(Debug, Args)]
pub struct RootfsBuildArgs {
    #[arg(long)]
    pub r#override: bool,
}

pub fn parse() -> Cli {
    Cli::parse()
}

impl RunArgs {
    pub fn into_options(self) -> RunOptions {
        let mut options = RunOptions::from_env();
        options.agent_mode = self.agent;
        options
    }
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

pub fn repo_root() -> Result<PathBuf> {
    Ok(env::current_dir()?)
}
