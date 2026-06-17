use anyhow::{Context, Result, bail};
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};
use xshell::{Shell, cmd};

use crate::json_output::{JsonEvent, OutputMode, emit, run_xshell_command};

const AUR_PACKAGES: &[&str] = &["linux-test-project-git"];
const BUILD_USER: &str = "seelebuild";
const GUEST_BUILD_ROOT: &str = "/var/tmp/seele-aur-build";

pub fn install_aur_packages(
    sh: &Shell,
    repo_root: &Path,
    pacman_conf: &Path,
    rootfs_mount: &Path,
    output_mode: OutputMode,
) -> Result<()> {
    let build_root = repo_root.join("target").join("aur-build");
    fs::create_dir_all(&build_root)
        .with_context(|| format!("failed to create {}", build_root.display()))?;
    install_pacman_config(rootfs_mount, pacman_conf, output_mode)?;
    let build_user_uid = ensure_build_user(rootfs_mount, output_mode)?;

    for package in AUR_PACKAGES {
        if output_mode.is_json() {
            emit(&JsonEvent::progress(
                "build-rootfs",
                "aur",
                &format!("building AUR package {package}"),
            ))?;
        } else {
            eprintln!("building AUR package {package}");
        }

        let package_dir = prepare_package_checkout(sh, &build_root, package, output_mode)?;
        let guest_package_dir = sync_package_to_rootfs(
            sh,
            rootfs_mount,
            &package_dir,
            package,
            build_user_uid,
            output_mode,
        )?;
        build_package_in_rootfs(rootfs_mount, &guest_package_dir, output_mode)?;
        install_built_package_in_rootfs(rootfs_mount, &guest_package_dir, output_mode)?;
    }

    Ok(())
}

fn ensure_build_user(rootfs_mount: &Path, output_mode: OutputMode) -> Result<u32> {
    let user_exists =
        run_chroot_shell(rootfs_mount, &format!("id -u {BUILD_USER}"), output_mode).is_ok();
    if user_exists {
        return get_build_user_uid(rootfs_mount, output_mode);
    }

    run_chroot_shell(
        rootfs_mount,
        &format!("useradd -m -s /bin/bash {BUILD_USER}"),
        output_mode,
    )
    .context("failed to create AUR build user")?;
    get_build_user_uid(rootfs_mount, output_mode)
}

fn get_build_user_uid(rootfs_mount: &Path, output_mode: OutputMode) -> Result<u32> {
    let output = run_chroot_output(rootfs_mount, &format!("id -u {BUILD_USER}"), output_mode)?;
    let uid = output
        .trim()
        .parse()
        .with_context(|| format!("failed to parse {BUILD_USER} uid from {:?}", output.trim()))?;
    Ok(uid)
}

fn install_pacman_config(
    rootfs_mount: &Path,
    pacman_conf: &Path,
    output_mode: OutputMode,
) -> Result<()> {
    let rootfs_pacman_conf = rootfs_mount.join("etc").join("pacman.conf");
    run_command(
        "build-rootfs",
        Command::new("sudo")
            .arg("install")
            .arg("-Dm644")
            .arg(pacman_conf)
            .arg(&rootfs_pacman_conf),
        output_mode,
    )
    .context("failed to install pacman config into rootfs")
}

fn prepare_package_checkout(
    sh: &Shell,
    build_root: &Path,
    package: &str,
    output_mode: OutputMode,
) -> Result<PathBuf> {
    let package_dir = build_root.join(package);
    let aur_url = format!("https://aur.archlinux.org/{package}.git");

    if !package_dir.exists() {
        run_xshell_command(
            "build-rootfs",
            sh,
            cmd!(sh, "git clone {aur_url} {package_dir}"),
            output_mode,
        )?;
        return Ok(package_dir);
    }

    if !package_dir.join(".git").is_dir() {
        bail!(
            "AUR build directory exists but is not a git checkout: {}",
            package_dir.display()
        );
    }

    run_xshell_command(
        "build-rootfs",
        sh,
        cmd!(sh, "git -C {package_dir} fetch --all --prune"),
        output_mode,
    )?;
    run_xshell_command(
        "build-rootfs",
        sh,
        cmd!(sh, "git -C {package_dir} reset --hard origin/master"),
        output_mode,
    )?;
    run_xshell_command(
        "build-rootfs",
        sh,
        cmd!(sh, "git -C {package_dir} clean -fdx"),
        output_mode,
    )?;
    Ok(package_dir)
}

fn sync_package_to_rootfs(
    sh: &Shell,
    rootfs_mount: &Path,
    package_dir: &Path,
    package: &str,
    build_user_uid: u32,
    output_mode: OutputMode,
) -> Result<String> {
    let host_build_root = rootfs_mount.join(GUEST_BUILD_ROOT.trim_start_matches('/'));
    let host_package_dir = host_build_root.join(package);
    run_xshell_command(
        "build-rootfs",
        sh,
        cmd!(sh, "sudo rm -rf {host_package_dir}"),
        output_mode,
    )?;
    run_xshell_command(
        "build-rootfs",
        sh,
        cmd!(sh, "sudo mkdir -p {host_build_root}"),
        output_mode,
    )?;
    run_xshell_command(
        "build-rootfs",
        sh,
        cmd!(sh, "sudo cp -a {package_dir} {host_build_root}"),
        output_mode,
    )?;
    run_xshell_command(
        "build-rootfs",
        sh,
        {
            let owner = format!("{build_user_uid}:{build_user_uid}");
            cmd!(sh, "sudo chown -R {owner} {host_package_dir}")
        },
        output_mode,
    )?;
    Ok(format!("{GUEST_BUILD_ROOT}/{package}"))
}

fn build_package_in_rootfs(
    rootfs_mount: &Path,
    guest_package_dir: &str,
    output_mode: OutputMode,
) -> Result<()> {
    run_chroot_shell(
        rootfs_mount,
        &format!(
            "cd {guest_package_dir} && runuser -u {BUILD_USER} -- makepkg --syncdeps --noconfirm --needed --cleanbuild --force --nocheck"
        ),
        output_mode,
    )
    .with_context(|| format!("failed to build AUR package in guest path {guest_package_dir}"))
}

fn install_built_package_in_rootfs(
    rootfs_mount: &Path,
    guest_package_dir: &str,
    output_mode: OutputMode,
) -> Result<()> {
    run_chroot_shell(
        rootfs_mount,
        &format!("cd {guest_package_dir} && pacman --noconfirm --needed -U ./*.pkg.tar.*"),
        output_mode,
    )
    .with_context(|| format!("failed to install AUR package from guest path {guest_package_dir}"))
}

fn run_chroot_shell(rootfs_mount: &Path, script: &str, output_mode: OutputMode) -> Result<()> {
    let mut command = Command::new("sudo");
    command
        .arg("arch-chroot")
        .arg(rootfs_mount)
        .arg("/usr/bin/bash")
        .arg("-lc")
        .arg(script);
    run_command("build-rootfs", &mut command, output_mode)
}

fn run_command(command_name: &str, command: &mut Command, output_mode: OutputMode) -> Result<()> {
    if !output_mode.is_json() {
        let status = command.status().context("failed to run command")?;
        if !status.success() {
            bail!("command failed with status {status}");
        }
        return Ok(());
    }

    let output = command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .context("failed to run command")?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    if !stdout.is_empty() {
        emit(&JsonEvent::log(command_name, "stdout", &stdout))?;
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    if !stderr.is_empty() {
        emit(&JsonEvent::log(command_name, "stderr", &stderr))?;
    }
    if !output.status.success() {
        bail!("command failed with status {}", output.status);
    }
    Ok(())
}

fn run_chroot_output(rootfs_mount: &Path, script: &str, output_mode: OutputMode) -> Result<String> {
    let mut command = Command::new("sudo");
    command
        .arg("arch-chroot")
        .arg(rootfs_mount)
        .arg("/usr/bin/bash")
        .arg("-lc")
        .arg(script);

    if !output_mode.is_json() {
        let output = command.output().context("failed to run command")?;
        if !output.status.success() {
            bail!("command failed with status {}", output.status);
        }
        return Ok(String::from_utf8_lossy(&output.stdout).to_string());
    }

    let output = command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .context("failed to run command")?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    if !stdout.is_empty() {
        emit(&JsonEvent::log("build-rootfs", "stdout", &stdout))?;
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    if !stderr.is_empty() {
        emit(&JsonEvent::log("build-rootfs", "stderr", &stderr))?;
    }
    if !output.status.success() {
        bail!("command failed with status {}", output.status);
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}
