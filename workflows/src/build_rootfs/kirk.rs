use anyhow::{Context, Result};
use std::{fs, path::Path};
use xshell::{Shell, cmd};

use crate::reporter::{WorkflowReporter, run_xshell_command};

const KIRK_URL: &str = "https://github.com/linux-test-project/kirk";

pub fn install_kirk(
    sh: &Shell,
    repo_root: &Path,
    rootfs_mount: &Path,
    reporter: &dyn WorkflowReporter,
) -> Result<()> {
    let kirk_checkout = repo_root.join("target").join("kirk");
    let host_kirk_dir = rootfs_mount.join("opt").join("kirk");
    let host_kirk_bin = rootfs_mount.join("usr/local/bin/kirk");
    let ltp_runner = repo_root.join("target").join("seele-run-ltp");
    let host_ltp_runner = rootfs_mount.join("usr/local/bin/seele-run-ltp");

    if !kirk_checkout.exists() {
        run_xshell_command(
            "build-rootfs",
            sh,
            cmd!(sh, "git clone --depth 1 {KIRK_URL} {kirk_checkout}"),
            reporter,
        )
        .context("failed to clone kirk")?;
    } else {
        run_xshell_command(
            "build-rootfs",
            sh,
            cmd!(sh, "git -C {kirk_checkout} fetch --depth 1 origin master"),
            reporter,
        )
        .context("failed to update kirk checkout")?;
        run_xshell_command(
            "build-rootfs",
            sh,
            cmd!(sh, "git -C {kirk_checkout} reset --hard FETCH_HEAD"),
            reporter,
        )
        .context("failed to reset kirk checkout")?;
        run_xshell_command(
            "build-rootfs",
            sh,
            cmd!(sh, "git -C {kirk_checkout} clean -fdx"),
            reporter,
        )
        .context("failed to clean kirk checkout")?;
    }

    run_xshell_command(
        "build-rootfs",
        sh,
        cmd!(sh, "sudo rm -rf {host_kirk_dir}"),
        reporter,
    )?;
    run_xshell_command(
        "build-rootfs",
        sh,
        cmd!(sh, "sudo mkdir -p {host_kirk_dir}"),
        reporter,
    )?;
    run_xshell_command(
        "build-rootfs",
        sh,
        cmd!(sh, "sudo cp -a {kirk_checkout}/. {host_kirk_dir}"),
        reporter,
    )?;
    run_xshell_command(
        "build-rootfs",
        sh,
        cmd!(
            sh,
            "sudo install -Dm755 {host_kirk_dir}/kirk {host_kirk_bin}"
        ),
        reporter,
    )?;
    fs::write(&ltp_runner, LTP_RUNNER).context("failed to write LTP runner script")?;
    run_xshell_command(
        "build-rootfs",
        sh,
        cmd!(sh, "sudo install -Dm755 {ltp_runner} {host_ltp_runner}"),
        reporter,
    )?;
    Ok(())
}

const LTP_RUNNER: &str = r#"#!/bin/sh
suite="${SEELE_LTP_SUITE:-syscalls}"
pattern="${SEELE_LTP_PATTERN:-^(getpid01|getpid02|brk01|access01|open01|close01|read01|write01)$}"
report_dir=/tmp/seele-ltp
report="$report_dir/report.json"

mkdir -p "$report_dir"
rm -f "$report"

LTPROOT=/usr/share LTP_COLORIZE_OUTPUT=0 kirk --no-colors \
    --run-suite "$suite" \
    --run-pattern "$pattern" \
    --workers 1 \
    --json-report "$report"
status=$?

echo __SEELE_LTP_JSON_BEGIN__
if command -v python >/dev/null 2>&1; then
    python -m json.tool "$report" 2>/dev/null || cat "$report" 2>/dev/null
else
    cat "$report" 2>/dev/null
fi
echo __SEELE_LTP_JSON_END__
echo __SEELE_LTP_EXIT__:$status
exit "$status"
"#;
