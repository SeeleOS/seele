mod cli;
mod rootfs;
mod run;
mod test;

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
        Command::McpRun => run::mcp_run(),
        Command::Test => test::test(),
        Command::RootfsBuild(args) => rootfs::rootfs(args),
    }
}
