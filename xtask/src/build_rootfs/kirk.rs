use anyhow::{Context, Result};
use std::path::Path;
use xshell::{Shell, cmd};

use crate::json_output::{OutputMode, run_xshell_command};

const KIRK_URL: &str = "https://github.com/linux-test-project/kirk";

pub fn install_kirk(
    sh: &Shell,
    repo_root: &Path,
    rootfs_mount: &Path,
    output_mode: OutputMode,
) -> Result<()> {
    let kirk_checkout = repo_root.join("target").join("kirk");
    let host_kirk_dir = rootfs_mount.join("opt").join("kirk");
    let host_kirk_bin = rootfs_mount.join("usr/local/bin/kirk");

    if !kirk_checkout.exists() {
        run_xshell_command(
            "build-rootfs",
            sh,
            cmd!(sh, "git clone --depth 1 {KIRK_URL} {kirk_checkout}"),
            output_mode,
        )
        .context("failed to clone kirk")?;
    } else {
        run_xshell_command(
            "build-rootfs",
            sh,
            cmd!(sh, "git -C {kirk_checkout} fetch --depth 1 origin master"),
            output_mode,
        )
        .context("failed to update kirk checkout")?;
        run_xshell_command(
            "build-rootfs",
            sh,
            cmd!(sh, "git -C {kirk_checkout} reset --hard origin/master"),
            output_mode,
        )
        .context("failed to reset kirk checkout")?;
        run_xshell_command(
            "build-rootfs",
            sh,
            cmd!(sh, "git -C {kirk_checkout} clean -fdx"),
            output_mode,
        )
        .context("failed to clean kirk checkout")?;
    }

    run_xshell_command(
        "build-rootfs",
        sh,
        cmd!(sh, "sudo rm -rf {host_kirk_dir}"),
        output_mode,
    )?;
    run_xshell_command(
        "build-rootfs",
        sh,
        cmd!(sh, "sudo mkdir -p {host_kirk_dir}"),
        output_mode,
    )?;
    run_xshell_command(
        "build-rootfs",
        sh,
        cmd!(sh, "sudo cp -a {kirk_checkout}/. {host_kirk_dir}"),
        output_mode,
    )?;
    run_xshell_command(
        "build-rootfs",
        sh,
        cmd!(
            sh,
            "sudo install -Dm755 {host_kirk_dir}/kirk {host_kirk_bin}"
        ),
        output_mode,
    )?;
    Ok(())
}
