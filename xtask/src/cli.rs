use clap::{Args, Parser, Subcommand};

use crate::{build_rootfs::BuildRootfsArgs, run::RunArgs};

#[derive(Debug, Parser)]
#[command(version, about = "Seele OS development tasks")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    Run(RunArgs),
    McpRun(JsonOutputArgs),
    Test(JsonOutputArgs),
    BuildRootfs(BuildRootfsArgs),
}

pub fn parse() -> Cli {
    Cli::parse()
}

#[derive(Debug, Args)]
pub struct JsonOutputArgs {
    #[arg(long)]
    pub json_output: bool,
}
