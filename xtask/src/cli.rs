use clap::{Args, Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(version, about = "Seele OS development tasks")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    Run(RunArgs),
    McpRun(McpRunArgs),
    Test(TestArgs),
    BuildRootfs(BuildRootfsArgs),
}

pub fn parse() -> Cli {
    Cli::parse()
}

#[derive(Debug, Args)]
pub struct RunArgs {
    #[arg(long)]
    pub agent: bool,
}

#[derive(Debug, Args)]
pub struct McpRunArgs {
    #[arg(long)]
    pub enable_profiling: bool,
}

#[derive(Debug, Args)]
pub struct TestArgs {
    /// Optional test filter. Omit for kernel unit tests plus LTP; use "full" for every integration test.
    pub test: Option<String>,
}

#[derive(Debug, Args)]
pub struct BuildRootfsArgs {
    #[arg(long)]
    pub override_rootfs: bool,

    #[arg(long)]
    pub rebuild_aur: bool,

    #[arg(long = "rebuild-aur-package")]
    pub rebuild_aur_packages: Vec<String>,

    #[arg(trailing_var_arg = true, allow_hyphen_values = true, hide = true)]
    pub passthrough: Vec<String>,
}
