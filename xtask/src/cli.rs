use clap::{Parser, Subcommand};

use crate::{rootfs::RootfsArgs, run::RunArgs};

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
    RootfsBuild(RootfsArgs),
}

pub fn parse() -> Cli {
    Cli::parse()
}
