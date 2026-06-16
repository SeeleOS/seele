use anyhow::{Context, Result, bail};
use clap::Args;
use std::{
    env, fs,
    path::{Path, PathBuf},
    process::Command,
};

const DISK_SIZE: &str = "10G";
const ALPINE_BRANCH: &str = "v3.24";
const ALPINE_MIRROR: &str = "https://dl-cdn.alpinelinux.org/alpine";
const ALPINE_PACKAGES: &[&str] = &[
    "alpine-keys",
    "alpine-base",
    "openrc",
    "busybox",
    "bash",
    "coreutils",
    "util-linux",
    "procps",
    "iproute2",
    "curl",
    "gcc",
    "musl-dev",
    "make",
    "pkgconf",
    "git",
    "rust",
    "cargo",
];

#[derive(Debug, Args)]
pub struct BuildRootfsArgs {
    #[arg(long)]
    pub override_disk: bool,

    #[arg(trailing_var_arg = true, allow_hyphen_values = true, hide = true)]
    pub passthrough: Vec<String>,
}

pub fn build_rootfs(args: BuildRootfsArgs) -> Result<i32> {
    let repo_root = repo_root()?;
    env::set_current_dir(&repo_root)
        .with_context(|| format!("failed to enter {}", repo_root.display()))?;

    let disk = repo_root.join("disk.img");
    let sysroot = repo_root.join("sysroot");
    fs::create_dir_all(&sysroot)
        .with_context(|| format!("failed to create {}", sysroot.display()))?;

    unmount_if_mounted(&sysroot)?;
    prepare_disk(&disk, args.override_disk()?)?;
    mount_disk(&disk, &sysroot)?;

    let _mount = MountedSysroot { path: &sysroot };
    write_repositories(&repo_root, &sysroot)?;
    install_packages(&sysroot)?;
    prepare_openrc(&sysroot)?;

    Ok(0)
}

impl BuildRootfsArgs {
    fn override_disk(&self) -> Result<bool> {
        let mut override_disk = self.override_disk;

        for arg in &self.passthrough {
            match arg.as_str() {
                "--override" => override_disk = true,
                _ => bail!("unknown argument: {arg}"),
            }
        }

        Ok(override_disk)
    }
}

fn repo_root() -> Result<PathBuf> {
    Ok(env::current_dir()?)
}

fn prepare_disk(disk: &Path, override_disk: bool) -> Result<()> {
    if disk.exists() && !override_disk {
        println!("reusing existing disk image: {}", disk.display());
        return Ok(());
    }

    if disk.exists() {
        fs::remove_file(disk).with_context(|| format!("failed to remove {}", disk.display()))?;
    }

    run(Command::new("truncate").arg("-s").arg(DISK_SIZE).arg(disk))?;
    run(Command::new("mkfs.ext4").arg("-F").arg(disk))?;
    Ok(())
}

fn mount_disk(disk: &Path, sysroot: &Path) -> Result<()> {
    run(Command::new("sudo")
        .arg("mount")
        .arg("-o")
        .arg("loop")
        .arg(disk)
        .arg(sysroot))
}

fn unmount_if_mounted(sysroot: &Path) -> Result<()> {
    if is_mountpoint(sysroot)? {
        run(Command::new("sudo").arg("umount").arg("-l").arg(sysroot))?;
    }
    Ok(())
}

fn is_mountpoint(path: &Path) -> Result<bool> {
    let status = Command::new("mountpoint")
        .arg("-q")
        .arg(path)
        .status()
        .with_context(|| format!("failed to inspect mountpoint {}", path.display()))?;
    Ok(status.success())
}

fn write_repositories(repo_root: &Path, sysroot: &Path) -> Result<()> {
    let repositories = format!(
        "{mirror}/{branch}/main\n{mirror}/{branch}/community\n",
        mirror = ALPINE_MIRROR,
        branch = ALPINE_BRANCH,
    );
    let temp_file = repo_root.join(".seele-apk-repositories");
    fs::write(&temp_file, repositories)
        .with_context(|| format!("failed to write {}", temp_file.display()))?;

    let target = sysroot.join("etc/apk/repositories");
    let install_result = run(Command::new("sudo")
        .arg("install")
        .arg("-D")
        .arg("-m")
        .arg("0644")
        .arg(&temp_file)
        .arg(&target));
    fs::remove_file(&temp_file).ok();
    install_result
}

fn install_packages(sysroot: &Path) -> Result<()> {
    let mut command = Command::new("sudo");
    command
        .arg("apk")
        .arg("--root")
        .arg(sysroot)
        .arg("--initdb")
        .arg("--update-cache")
        .arg("--allow-untrusted")
        .arg("add")
        .args(ALPINE_PACKAGES);
    run(&mut command)
}

fn prepare_openrc(sysroot: &Path) -> Result<()> {
    let run_openrc = sysroot.join("run/openrc");
    run(Command::new("sudo")
        .arg("install")
        .arg("-d")
        .arg("-m")
        .arg("0755")
        .arg(&run_openrc))?;
    run(Command::new("sudo")
        .arg("touch")
        .arg(run_openrc.join("softlevel")))?;
    Ok(())
}

fn run(command: &mut Command) -> Result<()> {
    println!("running: {command:?}");
    let status = command
        .status()
        .with_context(|| format!("failed to spawn {command:?}"))?;
    if !status.success() {
        bail!("{command:?} exited with {status}");
    }
    Ok(())
}

struct MountedSysroot<'a> {
    path: &'a Path,
}

impl Drop for MountedSysroot<'_> {
    fn drop(&mut self) {
        if is_mountpoint(self.path).unwrap_or(false) {
            let _ = Command::new("sudo")
                .arg("umount")
                .arg("-l")
                .arg(self.path)
                .status();
        }
    }
}
