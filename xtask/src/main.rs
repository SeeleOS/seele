mod check;
mod cli;
mod rootfs;
mod run;
mod sysroot;

use anyhow::Result;
use cli::Command;
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
    match cli::parse().command {
        Command::Run(args) => run::run(args),
        Command::Check(args) => check::check(args),
        Command::McpRun => run::mcp_run(),
        Command::Test => check::unit(),
        Command::IntegrationTest => check::integration(),
        Command::RootfsBuild(args) => rootfs::rootfs(args),
        Command::SysrootMount(args) => sysroot::sysroot(args),
    }
}
