use super::{
    arch::{
        PacmanConfig, configure_login_services, install_modprobe_wrapper, install_packages,
        set_empty_root_password,
    },
    aur::{install_aur_packages, validate_rebuild_packages},
    config::BuildRootfsConfig,
    kirk::install_kirk,
    mount::{ensure_mounted, unmount},
    paths::paths,
};
use anyhow::{Context, Result};
use std::{fs, path::Path};
use xshell::{Shell, cmd};

pub fn build_rootfs(repo: &Path, config: &BuildRootfsConfig) -> Result<i32> {
    let sh = Shell::new()?;
    sh.change_dir(repo);
    let paths = paths(repo);
    fs::create_dir_all(&paths.mount)
        .with_context(|| format!("failed to create {}", paths.mount.display()))?;
    let existing_rootfs = paths.image.exists() && !config.override_rootfs;

    step("prepare rootfs image");
    if config.override_rootfs && paths.image.exists() {
        fs::remove_file(&paths.image)
            .with_context(|| format!("failed to remove {}", paths.image.display()))?;
    }
    if !paths.image.exists() {
        let size = "16G";
        let image = &paths.image;
        cmd!(sh, "truncate -s {size} {image}").run()?;
        cmd!(sh, "mkfs.ext4 -F {image}").run()?;
    }

    step("mount rootfs");
    ensure_mounted(&sh, &paths)?;
    let pacman_conf = PacmanConfig::create(repo)?;
    let rebuild_aur = config.rebuild_aur();
    validate_rebuild_packages(&rebuild_aur.packages)?;
    if !existing_rootfs {
        step("install base packages");
        install_packages(&sh, pacman_conf.path(), &paths.mount)?;
        step("set empty root password");
        set_empty_root_password(&sh, &paths.mount)?;
        step("configure login services");
        configure_login_services(&sh, &paths.mount)?;
    }
    if !existing_rootfs || rebuild_aur.all || !rebuild_aur.packages.is_empty() {
        step("install AUR packages");
        install_aur_packages(&sh, repo, pacman_conf.path(), &paths.mount, &rebuild_aur)?;
    }
    step("install kirk LTP runner");
    install_kirk(&sh, repo, &paths.mount)?;
    step("configure rootfs directories");
    fs::create_dir_all(paths.mount.join("var/log")).context("failed to create var/log")?;
    fs::create_dir_all(paths.mount.join("tmp")).context("failed to create tmp")?;
    install_modprobe_wrapper(&sh, &paths.mount)?;
    step("unmount rootfs");
    unmount(&sh, &paths.mount)?;
    eprintln!("rootfs image: {}", paths.image.display());
    Ok(0)
}

fn step(name: &str) {
    eprintln!("==> {name}");
}
