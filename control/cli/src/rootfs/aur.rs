use super::config::RebuildAur;
use anyhow::{Context, Result, bail};
use std::{
    fs,
    path::{Path, PathBuf},
};
use xshell::{Shell, cmd};

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
    sh: &Shell,
    repo: &Path,
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

    install_pacman_config(sh, rootfs_mount, pacman_conf)?;
    let uid = ensure_build_user(sh, rootfs_mount)?;
    let install_context = AurInstallContext {
        sh,
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
    sh: &'a Shell,
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
            install_context.sh,
            install_context.rootfs_mount,
            &package_cache,
            package,
        );
    }

    let package_dir =
        prepare_package_checkout(install_context.sh, install_context.build_root, package)?;
    let guest_package_dir = sync_package_to_rootfs(
        install_context.sh,
        install_context.rootfs_mount,
        &package_dir,
        package,
        install_context.build_user_uid,
    )?;
    build_package_in_rootfs(
        install_context.sh,
        install_context.rootfs_mount,
        &guest_package_dir,
    )?;
    refresh_package_cache(
        install_context.sh,
        install_context.rootfs_mount,
        &package_cache,
        &guest_package_dir,
        package,
    )?;
    install_cached_package(
        install_context.sh,
        install_context.rootfs_mount,
        &package_cache,
        package,
    )?;
    Ok(())
}

fn install_pacman_config(sh: &Shell, rootfs_mount: &Path, pacman_conf: &Path) -> Result<()> {
    let dest = rootfs_mount.join("etc/pacman.conf");
    cmd!(sh, "sudo install -Dm644 {pacman_conf} {dest}").run()?;
    Ok(())
}

fn ensure_build_user(sh: &Shell, rootfs_mount: &Path) -> Result<u32> {
    if run_chroot_output(sh, rootfs_mount, &format!("id -u {BUILD_USER}")).is_err() {
        run_chroot_shell(
            sh,
            rootfs_mount,
            &format!("useradd -m -s /bin/bash {BUILD_USER}"),
        )?;
    }
    Ok(
        run_chroot_output(sh, rootfs_mount, &format!("id -u {BUILD_USER}"))?
            .trim()
            .parse()?,
    )
}

fn prepare_package_checkout(sh: &Shell, build_root: &Path, package: &str) -> Result<PathBuf> {
    let package_dir = build_root.join(package);
    let aur_url = format!("https://aur.archlinux.org/{package}.git");
    sh.remove_path(&package_dir)?;
    cmd!(sh, "git clone {aur_url} {package_dir}").run()?;
    Ok(package_dir)
}

fn sync_package_to_rootfs(
    sh: &Shell,
    rootfs_mount: &Path,
    package_dir: &Path,
    package: &str,
    build_user_uid: u32,
) -> Result<String> {
    let host_build_root = rootfs_mount.join(GUEST_BUILD_ROOT.trim_start_matches('/'));
    let host_package_dir = host_build_root.join(package);
    let script = format!(
        "sudo rm -rf {} && sudo mkdir -p {} && sudo cp -a {} {} && sudo chown -R {}:{} {}",
        quote(&host_package_dir),
        quote(&host_build_root),
        quote(package_dir),
        quote(&host_build_root),
        build_user_uid,
        build_user_uid,
        quote(&host_package_dir)
    );
    cmd!(sh, "bash -lc {script}").run()?;
    Ok(format!("{GUEST_BUILD_ROOT}/{package}"))
}

fn build_package_in_rootfs(sh: &Shell, rootfs_mount: &Path, guest_package_dir: &str) -> Result<()> {
    run_chroot_shell(
        sh,
        rootfs_mount,
        &format!(
            "cd {} && runuser -u {BUILD_USER} -- makepkg --syncdeps --noconfirm --needed --cleanbuild --force --nocheck",
            shell_quote(guest_package_dir)
        ),
    )
}

fn refresh_package_cache(
    sh: &Shell,
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

    let script = format!(
        "sudo rm -rf {} && sudo mkdir -p {}",
        quote(package_cache),
        quote(package_cache)
    );
    cmd!(sh, "bash -lc {script}").run()?;
    for built_package in built_packages {
        cmd!(sh, "sudo cp -a {built_package} {package_cache}").run()?;
    }
    Ok(())
}

fn install_cached_package(
    sh: &Shell,
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

    let script = format!(
        "sudo rm -rf {} && sudo mkdir -p {}",
        quote(&host_cache_path),
        quote(&host_cache_path)
    );
    cmd!(sh, "bash -lc {script}").run()?;
    let mut guest_packages = Vec::new();
    for cached_package in cached_packages {
        let file_name = cached_package
            .file_name()
            .context("cached package missing file name")?;
        cmd!(sh, "sudo cp -a {cached_package} {host_cache_path}").run()?;
        guest_packages.push(format!(
            "{guest_cache_path}/{}",
            file_name.to_string_lossy()
        ));
    }
    run_chroot_shell(
        sh,
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

fn run_chroot_shell(sh: &Shell, rootfs_mount: &Path, script: &str) -> Result<()> {
    cmd!(
        sh,
        "sudo arch-chroot {rootfs_mount} /usr/bin/bash -lc {script}"
    )
    .run()?;
    Ok(())
}

fn run_chroot_output(sh: &Shell, rootfs_mount: &Path, script: &str) -> Result<String> {
    Ok(cmd!(
        sh,
        "sudo arch-chroot {rootfs_mount} /usr/bin/bash -lc {script}"
    )
    .read()?)
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

fn quote(path: &Path) -> String {
    shell_quote(&path.to_string_lossy())
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}
