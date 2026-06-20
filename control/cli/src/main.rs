use anyhow::Result;
use clap::{Args, Parser, Subcommand};
use control_core::{
    plane::ControlPlane, qemu::VmConfig, rootfs::BuildRootfsConfig, tests::RunTestsConfig,
};
use std::{path::PathBuf, process::ExitCode};

#[derive(Debug, Parser)]
#[command(version, about = "Seele OS control-plane CLI")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    Run(VmArgs),
    Test(TestArgs),
    Rootfs(RootfsArgs),
    Vm {
        #[command(subcommand)]
        command: VmCommand,
    },
    Job {
        #[command(subcommand)]
        command: JobCommand,
    },
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
struct TestArgs {
    selector: Option<String>,
    #[arg(long)]
    ltp_suite: Option<String>,
    #[arg(long)]
    ltp_pattern: Option<String>,
}

#[derive(Debug, Args)]
struct RootfsArgs {
    #[arg(long)]
    override_rootfs: bool,
    #[arg(long)]
    rebuild_aur: bool,
    #[arg(long = "rebuild-aur-package")]
    rebuild_aur_packages: Vec<String>,
}

#[derive(Debug, Subcommand)]
enum VmCommand {
    Start(VmArgs),
    Stop,
    Status,
    SerialTail {
        #[arg(long)]
        lines: Option<usize>,
        #[arg(long)]
        bytes: Option<usize>,
    },
}

#[derive(Debug, Subcommand)]
enum JobCommand {
    Status {
        id: u64,
    },
    Wait {
        id: u64,
        #[arg(long)]
        timeout_ms: Option<u64>,
    },
    Cancel {
        id: u64,
    },
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
    let cli = Cli::parse();
    let repo = std::env::current_dir()?;
    let plane = ControlPlane::new(repo);
    match cli.command {
        Command::Run(args) => print_json(&plane.start_vm(vm_config(args)))?,
        Command::Test(args) => print_json(&plane.start_tests(RunTestsConfig {
            selector: args.selector,
            ltp_suite: args.ltp_suite,
            ltp_pattern: args.ltp_pattern,
        }))?,
        Command::Rootfs(args) => print_json(&plane.start_build_rootfs(BuildRootfsConfig {
            override_rootfs: args.override_rootfs,
            rebuild_aur: args.rebuild_aur,
            rebuild_aur_packages: args.rebuild_aur_packages,
        }))?,
        Command::Vm { command } => match command {
            VmCommand::Start(args) => print_json(&plane.start_vm(vm_config(args)))?,
            VmCommand::Stop => print_json(&plane.stop_vm())?,
            VmCommand::Status => print_json(&plane.vm_status())?,
            VmCommand::SerialTail { lines, bytes } => {
                println!("{}", plane.serial_tail(lines, bytes)?)
            }
        },
        Command::Job { command } => match command {
            JobCommand::Status { id } => print_json(&plane.jobs().status(id)?)?,
            JobCommand::Wait { id, timeout_ms } => print_json(&plane.jobs().wait(id, timeout_ms)?)?,
            JobCommand::Cancel { id } => print_json(&plane.jobs().cancel(id)?)?,
        },
    }
    Ok(0)
}

fn vm_config(args: VmArgs) -> VmConfig {
    let repo = std::env::current_dir().expect("failed to resolve current directory");
    let mut config = VmConfig::for_repo(&repo);
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

fn print_json(value: &impl serde::Serialize) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}
