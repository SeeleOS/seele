mod build_rootfs;
mod cli;
mod json_output;
mod run;
mod test;

use anyhow::Result;
use cli::Command;
use std::process::exit;

use crate::{
    build_rootfs::build_rootfs,
    json_output::OutputMode,
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
        Command::McpRun(args) => mcp_run(args),
        Command::Test(args) => test(output_mode(args.json_output), args.test.as_deref()),
        Command::BuildRootfs(args) => build_rootfs(args),
    }
}

fn output_mode(json_output: bool) -> OutputMode {
    if json_output {
        OutputMode::Json
    } else {
        OutputMode::Human
    }
}
