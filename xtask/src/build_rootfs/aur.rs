use anyhow::{Context, Result, bail};
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};
use xshell::{Shell, cmd};

use super::RebuildAur;
use crate::json_output::{JsonEvent, OutputMode, emit, run_xshell_command};

pub const AUR_PACKAGES: &[&str] = &["linux-test-project-git"];
const BUILD_USER: &str = "seelebuild";
const GUEST_BUILD_ROOT: &str = "/var/tmp/seele-aur-build";
const ROOTFS_CACHE_DIR: &str = "rootfs-cache/aur";

pub fn install_aur_packages(
    sh: &Shell,
    repo_root: &Path,
    pacman_conf: &Path,
    rootfs_mount: &Path,
    rebuild_aur: RebuildAur,
    output_mode: OutputMode,
) -> Result<()> {
    let build_root = repo_root.join("target").join("aur-build");
    let cache_root = repo_root.join("target").join(ROOTFS_CACHE_DIR);
    fs::create_dir_all(&build_root)
        .with_context(|| format!("failed to create {}", build_root.display()))?;
    fs::create_dir_all(&cache_root)
        .with_context(|| format!("failed to create {}", cache_root.display()))?;

    install_pacman_config(rootfs_mount, pacman_conf, output_mode)?;
    let build_user_uid = ensure_build_user(rootfs_mount, output_mode)?;
    let context = AurInstallContext {
        sh,
        rootfs_mount,
        build_root: &build_root,
        cache_root: &cache_root,
        build_user_uid,
        output_mode,
    };

    for package in AUR_PACKAGES {
        ensure_cached_package(&context, package, &rebuild_aur)?;
    }

    Ok(())
}

pub fn validate_rebuild_packages(packages: &[String]) -> Result<()> {
    for package in packages {
        if !AUR_PACKAGES.contains(&package.as_str()) {
            bail!("unknown AUR package for rebuild: {package}");
        }
    }
    Ok(())
}

fn ensure_cached_package(
    context: &AurInstallContext<'_>,
    package: &str,
    rebuild_aur: &RebuildAur,
) -> Result<()> {
    let package_cache = context.cache_root.join(package);
    let package_rebuild =
        rebuild_aur.all || rebuild_aur.packages.iter().any(|name| name == package);
    let cache_hit = !package_rebuild && cached_package_exists(&package_cache)?;

    if cache_hit {
        emit_cache_event(
            context.output_mode,
            package,
            "aur-cache-hit",
            "using cached AUR package",
        )?;
        install_cached_package(
            context.sh,
            context.rootfs_mount,
            &package_cache,
            package,
            context.output_mode,
        )?;
        return Ok(());
    }

    emit_cache_event(
        context.output_mode,
        package,
        "aur-cache-miss",
        "building AUR package cache",
    )?;
    let package_dir =
        prepare_package_checkout(context.sh, context.build_root, package, context.output_mode)?;
    let guest_package_dir = sync_package_to_rootfs(
        context.sh,
        context.rootfs_mount,
        &package_dir,
        package,
        context.build_user_uid,
        context.output_mode,
    )?;
    emit_cache_event(
        context.output_mode,
        package,
        "aur-build",
        "running makepkg in rootfs",
    )?;
    build_package_in_rootfs(
        context.rootfs_mount,
        &guest_package_dir,
        context.output_mode,
    )?;
    refresh_package_cache(
        context.sh,
        context.rootfs_mount,
        &package_cache,
        &guest_package_dir,
        package,
        context.output_mode,
    )?;
    emit_cache_event(
        context.output_mode,
        package,
        "aur-install",
        "installing cached AUR package",
    )?;
    install_cached_package(
        context.sh,
        context.rootfs_mount,
        &package_cache,
        package,
        context.output_mode,
    )?;

    if context.output_mode.is_json() {
        emit(&JsonEvent::progress(
            "build-rootfs",
            "aur-cache",
            &format!("cached AUR package {package}"),
        ))?;
    }
    Ok(())
}

struct AurInstallContext<'a> {
    sh: &'a Shell,
    rootfs_mount: &'a Path,
    build_root: &'a Path,
    cache_root: &'a Path,
    build_user_uid: u32,
    output_mode: OutputMode,
}

fn emit_cache_event(
    output_mode: OutputMode,
    package: &str,
    step: &'static str,
    message: &'static str,
) -> Result<()> {
    if output_mode.is_json() {
        emit(&JsonEvent::progress(
            "build-rootfs",
            step,
            &format!("{package}: {message}"),
        ))?;
    } else {
        eprintln!("{package}: {message}");
    }
    Ok(())
}

fn cached_package_exists(package_cache: &Path) -> Result<bool> {
    let entries = match fs::read_dir(package_cache) {
        Ok(entries) => entries,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(err) => {
            return Err(err).with_context(|| {
                format!(
                    "failed to read cached package dir {}",
                    package_cache.display()
                )
            });
        }
    };

    for entry in entries {
        let entry = entry
            .with_context(|| format!("failed to read entry in {}", package_cache.display()))?;
        let path = entry.path();
        if is_package_artifact(&path) {
            return Ok(true);
        }
    }
    Ok(false)
}

fn refresh_package_cache(
    sh: &Shell,
    rootfs_mount: &Path,
    package_cache: &Path,
    guest_package_dir: &str,
    package: &str,
    output_mode: OutputMode,
) -> Result<()> {
    let host_cache_dir = package_cache;
    run_xshell_command(
        "build-rootfs",
        sh,
        cmd!(sh, "sudo rm -rf {host_cache_dir}"),
        output_mode,
    )?;
    run_xshell_command(
        "build-rootfs",
        sh,
        cmd!(sh, "sudo mkdir -p {host_cache_dir}"),
        output_mode,
    )?;

    let host_package_dir = rootfs_mount.join(guest_package_dir.trim_start_matches('/'));
    let built_packages = package_artifacts(&host_package_dir)?;
    if built_packages.is_empty() {
        bail!("AUR package {package} did not produce a non-debug package artifact");
    }
    for built_package in built_packages {
        run_xshell_command(
            "build-rootfs",
            sh,
            cmd!(sh, "sudo cp -a {built_package} {host_cache_dir}"),
            output_mode,
        )?;
    }
    Ok(())
}

fn install_cached_package(
    sh: &Shell,
    rootfs_mount: &Path,
    package_cache: &Path,
    package: &str,
    output_mode: OutputMode,
) -> Result<()> {
    let guest_cache_path = format!("/tmp/seele-aur-cache/{package}");
    let host_cache_path = rootfs_mount.join(guest_cache_path.trim_start_matches('/'));
    let cached_packages = package_artifacts(package_cache)?;
    if cached_packages.is_empty() {
        bail!("AUR package cache for {package} is empty");
    }
    run_xshell_command(
        "build-rootfs",
        sh,
        cmd!(sh, "sudo rm -rf {host_cache_path}"),
        output_mode,
    )?;
    run_xshell_command(
        "build-rootfs",
        sh,
        cmd!(sh, "sudo mkdir -p {host_cache_path}"),
        output_mode,
    )?;
    for cached_package in cached_packages {
        run_xshell_command(
            "build-rootfs",
            sh,
            cmd!(sh, "sudo cp -a {cached_package} {host_cache_path}"),
            output_mode,
        )?;
    }
    run_chroot_shell(
        rootfs_mount,
        &format!("pacman --noconfirm --needed -U {guest_cache_path}/*.pkg.tar.*"),
        output_mode,
    )
    .with_context(|| format!("failed to install cached AUR package {package}"))
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

    run_xshell_command(
        "build-rootfs",
        sh,
        cmd!(sh, "rm -rf {package_dir}"),
        output_mode,
    )?;
    run_xshell_command(
        "build-rootfs",
        sh,
        cmd!(sh, "git clone {aur_url} {package_dir}"),
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

fn is_package_artifact(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.contains(".pkg.tar.") && !name.contains("-debug-"))
}

fn package_artifacts(dir: &Path) -> Result<Vec<PathBuf>> {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(err) => return Err(err).with_context(|| format!("failed to read {}", dir.display())),
    };
    let mut packages = Vec::new();
    for entry in entries {
        let path = entry
            .with_context(|| format!("failed to read entry in {}", dir.display()))?
            .path();
        if is_package_artifact(&path) {
            packages.push(path);
        }
    }
    packages.sort();
    Ok(packages)
}
