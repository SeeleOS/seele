use super::config::RebuildAur;
use crate::{JobContext, process::ProcessRunner};
use anyhow::{Context, Result, bail};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

pub const AUR_PACKAGES: &[&str] = &["linux-test-project-git"];
const BUILD_USER: &str = "seelebuild";
const GUEST_BUILD_ROOT: &str = "/var/tmp/seele-aur-build";
const ROOTFS_CACHE_DIR: &str = "rootfs-cache/aur";

pub fn validate_rebuild_packages(packages: &[String]) -> Result<()> {
    for package in packages {
        if !AUR_PACKAGES.contains(&package.as_str()) {
            bail!("unknown AUR package for rebuild: {package}");
        }
    }
    Ok(())
}

pub fn install_aur_packages(
    repo: &Path,
    runner: &ProcessRunner,
    context: &JobContext,
    pacman_conf: &Path,
    rootfs_mount: &Path,
    rebuild_aur: &RebuildAur,
) -> Result<()> {
    let build_root = repo.join("target").join("aur-build");
    let cache_root = repo.join("target").join(ROOTFS_CACHE_DIR);
    fs::create_dir_all(&build_root)
        .with_context(|| format!("failed to create {}", build_root.display()))?;
    fs::create_dir_all(&cache_root)
        .with_context(|| format!("failed to create {}", cache_root.display()))?;
    install_pacman_config(runner, context, rootfs_mount, pacman_conf)?;
    let uid = ensure_build_user(runner, context, rootfs_mount)?;
    let install_context = AurInstallContext {
        runner,
        context,
        rootfs_mount,
        build_root: &build_root,
        cache_root: &cache_root,
        build_user_uid: uid,
    };
    for package in AUR_PACKAGES {
        ensure_cached_package(&install_context, package, rebuild_aur)?;
    }
    Ok(())
}

struct AurInstallContext<'a> {
    runner: &'a ProcessRunner,
    context: &'a JobContext,
    rootfs_mount: &'a Path,
    build_root: &'a Path,
    cache_root: &'a Path,
    build_user_uid: u32,
}

fn ensure_cached_package(
    install_context: &AurInstallContext<'_>,
    package: &str,
    rebuild_aur: &RebuildAur,
) -> Result<()> {
    let package_cache = install_context.cache_root.join(package);
    let package_rebuild =
        rebuild_aur.all || rebuild_aur.packages.iter().any(|name| name == package);
    if !package_rebuild && cached_package_exists(&package_cache)? {
        return install_cached_package(
            install_context.runner,
            install_context.context,
            install_context.rootfs_mount,
            &package_cache,
            package,
        );
    }

    let package_dir = prepare_package_checkout(
        install_context.runner,
        install_context.context,
        install_context.build_root,
        package,
    )?;
    let guest_package_dir = sync_package_to_rootfs(
        install_context.runner,
        install_context.context,
        install_context.rootfs_mount,
        &package_dir,
        package,
        install_context.build_user_uid,
    )?;
    build_package_in_rootfs(
        install_context.runner,
        install_context.context,
        install_context.rootfs_mount,
        &guest_package_dir,
    )?;
    refresh_package_cache(
        install_context.runner,
        install_context.context,
        install_context.rootfs_mount,
        &package_cache,
        &guest_package_dir,
        package,
    )?;
    install_cached_package(
        install_context.runner,
        install_context.context,
        install_context.rootfs_mount,
        &package_cache,
        package,
    )?;
    Ok(())
}

fn install_pacman_config(
    runner: &ProcessRunner,
    context: &JobContext,
    rootfs_mount: &Path,
    pacman_conf: &Path,
) -> Result<()> {
    runner.run_success(
        context,
        "rootfs_install_pacman_conf",
        Command::new("sudo")
            .arg("install")
            .arg("-Dm644")
            .arg(pacman_conf)
            .arg(rootfs_mount.join("etc/pacman.conf")),
    )?;
    Ok(())
}

fn ensure_build_user(
    runner: &ProcessRunner,
    context: &JobContext,
    rootfs_mount: &Path,
) -> Result<u32> {
    if run_chroot_output(
        runner,
        context,
        rootfs_mount,
        &format!("id -u {BUILD_USER}"),
    )
    .is_err()
    {
        run_chroot_shell(
            runner,
            context,
            rootfs_mount,
            &format!("useradd -m -s /bin/bash {BUILD_USER}"),
        )?;
    }
    Ok(run_chroot_output(
        runner,
        context,
        rootfs_mount,
        &format!("id -u {BUILD_USER}"),
    )?
    .trim()
    .parse()?)
}

fn prepare_package_checkout(
    runner: &ProcessRunner,
    context: &JobContext,
    build_root: &Path,
    package: &str,
) -> Result<PathBuf> {
    let package_dir = build_root.join(package);
    let aur_url = format!("https://aur.archlinux.org/{package}.git");
    runner.run_shell_success(
        context,
        &format!("aur_clone_{package}"),
        &format!(
            "rm -rf {} && git clone {} {}",
            sh(&package_dir),
            sh(&aur_url),
            sh(&package_dir)
        ),
    )?;
    Ok(package_dir)
}

fn sync_package_to_rootfs(
    runner: &ProcessRunner,
    context: &JobContext,
    rootfs_mount: &Path,
    package_dir: &Path,
    package: &str,
    build_user_uid: u32,
) -> Result<String> {
    let host_build_root = rootfs_mount.join(GUEST_BUILD_ROOT.trim_start_matches('/'));
    let host_package_dir = host_build_root.join(package);
    runner.run_shell_success(
        context,
        &format!("aur_sync_{package}"),
        &format!(
            "sudo rm -rf {} && sudo mkdir -p {} && sudo cp -a {} {} && sudo chown -R {}:{} {}",
            sh(&host_package_dir),
            sh(&host_build_root),
            sh(package_dir),
            sh(&host_build_root),
            build_user_uid,
            build_user_uid,
            sh(&host_package_dir)
        ),
    )?;
    Ok(format!("{GUEST_BUILD_ROOT}/{package}"))
}

fn build_package_in_rootfs(
    runner: &ProcessRunner,
    context: &JobContext,
    rootfs_mount: &Path,
    guest_package_dir: &str,
) -> Result<()> {
    run_chroot_shell(
        runner,
        context,
        rootfs_mount,
        &format!(
            "cd {} && runuser -u {BUILD_USER} -- makepkg --syncdeps --noconfirm --needed --cleanbuild --force --nocheck",
            shell_quote(guest_package_dir)
        ),
    )
}

fn refresh_package_cache(
    runner: &ProcessRunner,
    context: &JobContext,
    rootfs_mount: &Path,
    package_cache: &Path,
    guest_package_dir: &str,
    package: &str,
) -> Result<()> {
    let host_package_dir = rootfs_mount.join(guest_package_dir.trim_start_matches('/'));
    let built_packages = package_artifacts(&host_package_dir)?;
    if built_packages.is_empty() {
        bail!("AUR package {package} did not produce a non-debug package artifact");
    }
    runner.run_shell_success(
        context,
        &format!("aur_cache_{package}"),
        &format!(
            "sudo rm -rf {} && sudo mkdir -p {}",
            sh(package_cache),
            sh(package_cache)
        ),
    )?;
    for built_package in built_packages {
        runner.run_success(
            context,
            &format!("aur_cache_copy_{package}"),
            Command::new("sudo")
                .arg("cp")
                .arg("-a")
                .arg(built_package)
                .arg(package_cache),
        )?;
    }
    Ok(())
}

fn install_cached_package(
    runner: &ProcessRunner,
    context: &JobContext,
    rootfs_mount: &Path,
    package_cache: &Path,
    package: &str,
) -> Result<()> {
    let guest_cache_path = format!("/var/tmp/seele-aur-cache/{package}");
    let host_cache_path = rootfs_mount.join(guest_cache_path.trim_start_matches('/'));
    let cached_packages = package_artifacts(package_cache)?;
    if cached_packages.is_empty() {
        bail!("AUR package cache for {package} is empty");
    }
    runner.run_shell_success(
        context,
        &format!("aur_stage_cache_{package}"),
        &format!(
            "sudo rm -rf {} && sudo mkdir -p {}",
            sh(&host_cache_path),
            sh(&host_cache_path)
        ),
    )?;
    let mut guest_packages = Vec::new();
    for cached_package in cached_packages {
        let file_name = cached_package
            .file_name()
            .context("cached package missing file name")?;
        runner.run_success(
            context,
            &format!("aur_stage_pkg_{package}"),
            Command::new("sudo")
                .arg("cp")
                .arg("-a")
                .arg(&cached_package)
                .arg(&host_cache_path),
        )?;
        guest_packages.push(format!(
            "{guest_cache_path}/{}",
            file_name.to_string_lossy()
        ));
    }
    run_chroot_shell(
        runner,
        context,
        rootfs_mount,
        &format!(
            "pacman --noconfirm --needed -U {}",
            guest_packages
                .iter()
                .map(|package| shell_quote(package))
                .collect::<Vec<_>>()
                .join(" ")
        ),
    )
}

fn run_chroot_shell(
    runner: &ProcessRunner,
    context: &JobContext,
    rootfs_mount: &Path,
    script: &str,
) -> Result<()> {
    runner.run_success(
        context,
        "rootfs_chroot_shell",
        Command::new("sudo")
            .arg("arch-chroot")
            .arg(rootfs_mount)
            .arg("/usr/bin/bash")
            .arg("-lc")
            .arg(script),
    )?;
    Ok(())
}

fn run_chroot_output(
    runner: &ProcessRunner,
    context: &JobContext,
    rootfs_mount: &Path,
    script: &str,
) -> Result<String> {
    let result = runner.run_success(
        context,
        "rootfs_chroot_output",
        Command::new("sudo")
            .arg("arch-chroot")
            .arg(rootfs_mount)
            .arg("/usr/bin/bash")
            .arg("-lc")
            .arg(script),
    )?;
    Ok(fs::read_to_string(result.stdout_artifact)?)
}

fn cached_package_exists(package_cache: &Path) -> Result<bool> {
    Ok(!package_artifacts(package_cache)?.is_empty())
}

fn package_artifacts(dir: &Path) -> Result<Vec<PathBuf>> {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(err) => return Err(err).with_context(|| format!("failed to read {}", dir.display())),
    };
    let mut packages = Vec::new();
    for entry in entries {
        let path = entry?.path();
        if path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.contains(".pkg.tar.") && !name.contains("-debug-"))
        {
            packages.push(path);
        }
    }
    packages.sort();
    Ok(packages)
}

fn sh(path: impl AsRef<std::ffi::OsStr>) -> String {
    shell_quote(&path.as_ref().to_string_lossy())
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}
