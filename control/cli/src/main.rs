use anyhow::{Result, bail};
use clap::{Args, Parser, Subcommand};
use control_cli::{
    JobState, plane::ControlPlane, rootfs::BuildRootfsConfig, tests::RunTestsConfig, vm::VmConfig,
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
    qmp_socket: Option<PathBuf>,
    #[arg(long)]
    serial_log: Option<PathBuf>,
    #[arg(long)]
    rootfs_image: Option<PathBuf>,
    #[arg(long)]
    ltp_device_image: Option<PathBuf>,
    #[arg(long)]
    iso_image: Option<PathBuf>,
    #[arg(long)]
    enable_profiling: bool,
    #[arg(long)]
    display: bool,
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
    let plane = ControlPlane::new(&repo);
    let status = match cli.command {
        Command::Run(args) => plane.start_vm(vm_config(&repo, args)),
        Command::Rootfs(args) => plane.start_build_rootfs(BuildRootfsConfig {
            override_rootfs: args.override_rootfs,
            rebuild_aur: args.rebuild_aur,
            rebuild_aur_packages: args.rebuild_aur_packages,
        }),
        Command::Test(args) => plane.start_tests(RunTestsConfig {
            selector: args.selector,
            ltp_suite: args.ltp_suite,
            ltp_pattern: args.ltp_pattern,
        }),
    };
    let status = plane.jobs().wait(status.id, None)?;
    println!("{}", serde_json::to_string_pretty(&status)?);
    match status.state {
        JobState::Finished => Ok(status.exit_code.unwrap_or(0)),
        JobState::Failed | JobState::Cancelled | JobState::TimedOut => {
            Ok(status.exit_code.unwrap_or(1))
        }
        JobState::Queued | JobState::Running => bail!("job did not reach a terminal state"),
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
    if let Some(path) = args.qmp_socket {
        config.qmp_socket = path;
    }
    if let Some(path) = args.serial_log {
        config.serial_log = path;
    }
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
    config.display = args.display;
    config
}
