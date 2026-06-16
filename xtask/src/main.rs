mod build_rootfs;
mod cli;
mod run;
mod test;

use anyhow::Result;
use cli::Command;
use std::process::exit;

use crate::{
    build_rootfs::build_rootfs,
    run::{mcp_run, run},
    test::test,
};

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
        Command::Run(args) => run(args),
        Command::McpRun => mcp_run(),
        Command::Test => test(),
        Command::BuildRootfs(args) => build_rootfs(args),
    }
}
