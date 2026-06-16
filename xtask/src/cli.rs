use clap::{Parser, Subcommand};

use crate::{check::CheckArgs, rootfs::RootfsArgs, run::RunArgs, sysroot::SysrootArgs, vm::VmArgs};

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
    VmPs(VmArgs),
}

pub fn parse() -> Cli {
    Cli::parse()
}
