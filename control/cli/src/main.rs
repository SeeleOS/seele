use anyhow::Result;
use clap::{Args, Parser, Subcommand};
use control_cli::{
    rootfs, tests, vm,
    vm::{SerialConfig, VmConfig},
};
use std::{path::PathBuf, process::ExitCode};

#[derive(Debug, Parser)]
#[command(version, about = "Seele OS human control CLI")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    Run(VmArgs),
    Rootfs(RootfsArgs),
    Test(TestArgs),
}

#[derive(Debug, Args)]
struct VmArgs {
    #[arg(long)]
    rootfs_image: Option<PathBuf>,
    #[arg(long, hide = true)]
    ltp_device_image: Option<PathBuf>,
    #[arg(long, hide = true)]
    iso_image: Option<PathBuf>,
    #[arg(long)]
    enable_profiling: bool,
    #[arg(long)]
    no_display: bool,
}

#[derive(Debug, Args)]
struct RootfsArgs {
    #[arg(long, alias = "override")]
    override_rootfs: bool,
    #[arg(long)]
    rebuild_aur: bool,
    #[arg(long = "rebuild-aur-package")]
    rebuild_aur_packages: Vec<String>,
}

#[derive(Debug, Args)]
struct TestArgs {
    selector: Option<String>,
    #[arg(long)]
    ltp_suite: Option<String>,
    #[arg(long)]
    ltp_pattern: Option<String>,
    #[arg(long)]
    enable_profiling: bool,
}

fn main() -> ExitCode {
    match real_main() {
        Ok(code) => ExitCode::from(code as u8),
        Err(err) => {
            eprintln!("{err:#}");
            ExitCode::from(1)
        }
    }
}

fn real_main() -> Result<i32> {
    let cli = Cli::parse_from(normalized_args());
    let repo = std::env::current_dir()?;
    match cli.command {
        Command::Run(args) => vm::run(&repo, vm_config(&repo, args)),
        Command::Rootfs(args) => rootfs::build_rootfs(
            &repo,
            &rootfs::BuildRootfsConfig {
                override_rootfs: args.override_rootfs,
                rebuild_aur: args.rebuild_aur,
                rebuild_aur_packages: args.rebuild_aur_packages,
            },
        ),
        Command::Test(args) => tests::run_tests(
            &repo,
            &tests::RunTestsConfig {
                selector: args.selector,
                ltp_suite: args.ltp_suite,
                ltp_pattern: args.ltp_pattern,
                enable_profiling: args.enable_profiling,
            },
        ),
    }
}

fn normalized_args() -> Vec<String> {
    let mut args = std::env::args().collect::<Vec<_>>();
    if args.len() == 4 && args[2] == "--" && matches!(args[3].as_str(), "--help" | "-h") {
        args.remove(2);
    }
    args
}

fn vm_config(repo: &std::path::Path, args: VmArgs) -> VmConfig {
    let mut config = VmConfig::for_repo(repo);
    if let Some(path) = args.rootfs_image {
        config.rootfs_image = path;
    }
    if let Some(path) = args.ltp_device_image {
        config.ltp_device_image = path;
    }
    if let Some(path) = args.iso_image {
        config.iso_image = Some(path);
    }
    config.enable_profiling = args.enable_profiling;
    config.display = !args.no_display;
    config.serial = SerialConfig::Stdio;
    config
}
