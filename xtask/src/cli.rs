use clap::{Parser, Subcommand};

use crate::{check::CheckArgs, rootfs::RootfsArgs, run::RunArgs, sysroot::SysrootArgs};

#[derive(Debug, Parser)]
#[command(version, about = "Seele OS development tasks")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    Run(RunArgs),
    Check(CheckArgs),
    McpRun,
    Test,
    IntegrationTest,
    RootfsBuild(RootfsArgs),
    SysrootMount(SysrootArgs),
}

pub fn parse() -> Cli {
    Cli::parse()
}
