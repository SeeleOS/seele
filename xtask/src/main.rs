mod cli;

use anyhow::Result;
use cli::{BuildRootfsArgs, Command, McpRunArgs, RunArgs};
use seele_workflows::{
    build_rootfs::{BuildRootfsConfig, build_rootfs},
    reporter::HumanReporter,
    run::{McpRunConfig, RunArgs as WorkflowRunArgs, mcp_run, run},
    test::test,
};
use std::process::exit;

fn main() {
    match real_main() {
        Ok(code) => exit(code),
        Err(err) => {
            eprintln!("{err:?}");
            exit(1);
        }
    }
}

fn real_main() -> Result<i32> {
    let reporter = HumanReporter;
    match cli::parse().command {
        Command::Run(args) => run(run_args(args)),
        Command::McpRun(args) => mcp_run(mcp_run_config(args), &reporter),
        Command::Test(args) => test(&reporter, args.test.as_deref()),
        Command::BuildRootfs(args) => build_rootfs(build_rootfs_config(args), &reporter),
    }
}

fn run_args(args: RunArgs) -> WorkflowRunArgs {
    WorkflowRunArgs { agent: args.agent }
}

fn mcp_run_config(args: McpRunArgs) -> McpRunConfig {
    McpRunConfig {
        enable_profiling: args.enable_profiling,
    }
}

fn build_rootfs_config(args: BuildRootfsArgs) -> BuildRootfsConfig {
    BuildRootfsConfig {
        override_rootfs: args.override_rootfs,
        rebuild_aur: args.rebuild_aur,
        rebuild_aur_packages: args.rebuild_aur_packages,
        passthrough: args.passthrough,
    }
}
