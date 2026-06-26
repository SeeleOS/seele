use crate::{JobContext, process::ProcessRunner};
use anyhow::{Context, Result};
use std::{fs, path::Path, process::Command};

const KIRK_URL: &str = "https://github.com/linux-test-project/kirk";

pub fn install_kirk(
    repo: &Path,
    runner: &ProcessRunner,
    context: &JobContext,
    rootfs_mount: &Path,
) -> Result<()> {
    let kirk_checkout = repo.join("target").join("kirk");
    let host_kirk_dir = rootfs_mount.join("opt").join("kirk");
    let host_kirk_bin = rootfs_mount.join("usr/local/bin/kirk");
    let kirk_bin = repo.join("target").join("kirk-runner");
    let ltp_runner = repo.join("target").join("seele-run-ltp");
    let host_ltp_runner = rootfs_mount.join("usr/local/bin/seele-run-ltp");

    if !kirk_checkout.exists() {
        runner.run_success(
            context,
            "kirk_clone",
            Command::new("git")
                .arg("clone")
                .arg("--depth")
                .arg("1")
                .arg(KIRK_URL)
                .arg(&kirk_checkout),
        )?;
    } else {
        runner.run_success(
            context,
            "kirk_update",
            Command::new("git")
                .arg("-C")
                .arg(&kirk_checkout)
                .args(["fetch", "--depth", "1", "origin", "master"]),
        )?;
        runner.run_success(
            context,
            "kirk_reset",
            Command::new("git").arg("-C").arg(&kirk_checkout).args([
                "reset",
                "--hard",
                "FETCH_HEAD",
            ]),
        )?;
        runner.run_success(
            context,
            "kirk_clean",
            Command::new("git")
                .arg("-C")
                .arg(&kirk_checkout)
                .args(["clean", "-fdx"]),
        )?;
    }

    runner.run_shell_success(
        context,
        "kirk_install_tree",
        &format!(
            "sudo rm -rf {} && sudo mkdir -p {} && sudo cp -a {}/. {}",
            sh(&host_kirk_dir),
            sh(&host_kirk_dir),
            sh(&kirk_checkout),
            sh(&host_kirk_dir)
        ),
    )?;
    fs::write(&kirk_bin, KIRK_RUNNER).context("failed to write kirk runner script")?;
    runner.run_success(
        context,
        "kirk_install_runner",
        Command::new("sudo")
            .arg("install")
            .arg("-Dm755")
            .arg(&kirk_bin)
            .arg(&host_kirk_bin),
    )?;
    fs::write(&ltp_runner, LTP_RUNNER).context("failed to write LTP runner script")?;
    runner.run_success(
        context,
        "ltp_install_runner",
        Command::new("sudo")
            .arg("install")
            .arg("-Dm755")
            .arg(&ltp_runner)
            .arg(&host_ltp_runner),
    )?;
    Ok(())
}

fn sh(path: &Path) -> String {
    format!("'{}'", path.to_string_lossy().replace('\'', "'\\''"))
}

const KIRK_RUNNER: &str = r#"#!/bin/sh
export PYTHONPATH="/opt/kirk${PYTHONPATH:+:$PYTHONPATH}"
exec /usr/bin/python /opt/kirk/kirk "$@"
"#;

const LTP_RUNNER: &str = r#"#!/bin/sh
export PATH="/usr/local/sbin:/usr/local/bin:/usr/bin:/usr/sbin:/sbin:/bin"
export PYTHONHOME=/usr
export PYTHONPATH=/opt/kirk:/usr/lib/python3.14:/usr/lib/python3.14/lib-dynload:/usr/lib/python3.14/site-packages
export PYTHONSAFEPATH=1

mountpoint -q /proc || mount -t proc proc /proc
mountpoint -q /sys || mount -t sysfs sysfs /sys
mountpoint -q /dev || mount -t devtmpfs devtmpfs /dev
mkdir -p /run /tmp /dev/pts /dev/shm
mountpoint -q /run || mount -t tmpfs tmpfs /run
mountpoint -q /tmp || mount -t tmpfs tmpfs /tmp
mountpoint -q /dev/pts || mount -t devpts devpts /dev/pts
mountpoint -q /dev/shm || mount -t tmpfs tmpfs /dev/shm

cmdline_value() {
    key="$1"
    tr ' ' '\n' < /proc/cmdline | sed -n "s/^$key=//p" | tail -n 1
}

cmdline_suite="$(cmdline_value seele.ltp_suite)"
cmdline_pattern="$(cmdline_value seele.ltp_pattern)"
suite="syscalls"
[ -n "$cmdline_suite" ] && suite="$cmdline_suite"
export LTP_SINGLE_FS_TYPE="${LTP_SINGLE_FS_TYPE:-ext4}"
default_pattern="^(abort01|arch_prctl01|accept0[1-3]|accept4_01|access01|access02|access03|access04|acct0[1-2]|add_key0[1-5]|alarm0[23567]|bind0[1-6]|brk01|brk02|sbrk0[1-3]|capget0[1-2]|capset0[1-4]|chdir01|chdir02|chdir04|chmod01|chown01|clock_adjtime0[1-2]|clock_getres01|clock_gettime01|clock_gettime0[2-4]|clock_settime0[1-2]|clock_nanosleep01|clock_nanosleep0[2-4]|clone01|close01|close02|close_range01|close_range02|connect0[1-2]|creat0[1-9]|dup01|dup201|dup202|dup301|epoll_create101|epoll_ctl01|epoll_pwait01|epoll_pwait201|epoll_wait01|eventfd.*|exit0[12]|execve01|exit_group01|faccessat01|faccessat02|faccessat201|fchdir01|fchdir02|fchdir03|fchmod0[1-6]|fchmodat01|fchown0[1-5]|fchown0[1-5]_16|fchownat01|fcntl01|fcntl02|fcntl03|fcntl04|fcntl05|fcntl06|fcntl07|fcntl08|fcntl09|fcntl10|fcntl11|fcntl12|fcntl13|fcntl14|fcntl(1[5-9]|2[0-9]|3[0-9])(_64)?|fdatasync01|fdatasync02|fdatasync03|flistxattr01|fork01|fork02|fstat01|fstat02|fstatat01|fstatfs01|fstatfs02|fsync01|fsync02|fsync03|fsync04|ftruncate01|ftruncate01_64|ftruncate03|ftruncate03_64|ftruncate04|ftruncate04_64|getcwd01|getcwd02|getcwd03|getcwd04|getdents01|getdents64.*|getegid01|geteuid01|getgid01|getgroups01|getgroups01_16|getitimer01|getpeername01|getpid01|getpid02|getrlimit01|getrusage01|getsid01|getsockname01|getsockopt0[1-2]|gettimeofday01|getuid01|getxattr01|ioctl01|kill01|lchown01|lgetxattr01|link01|linkat01|listen01|listxattr01|lseek01|lseek02|lseek03|lstat01|lstat02|lsetxattr01|madvise01|mincore01|mkdir01|mkdirat01|mlock01|mmap01|mmap02|mmap03|mmap04|mmap05|mmap06|mmap07|mmap0[8-9]|mmap1[2-5]|mmap1[7-9]|mmap2[02]|mmap21_0[12]|mmap_fixed01|mmapstress01|mount01|mprotect01|mprotect02|mprotect03|mprotect0[4-5]|mremap01|mremap0[2-7]|msync01|munlock01|munmap01|munmap02|nanosleep01|newfstatat01|newfstatat02|open01|open02|open03|open04|openat01|openat02|openat03|pipe01|pipe02|pipe201|pidfd_open0[1-3]|pidfd_send_signal0[1-2]|pivot_root01|poll01|ppoll01|pread64.*|preadv.*|prlimit64_01|pselect01|pwrite64.*|pwritev.*|read01|read02|read03|readlink01|readlinkat01|readv01|recvfrom01|removexattr01|rename01|renameat.*|rmdir01|rt_sigaction01|rt_sigpending01|rt_sigprocmask01|select01|select02|sendto0[1-3]|setsockopt(0[1-9]|10)|sched_getaffinity01|sched_getparam0[13]|sched_get_priority_max0[12]|sched_get_priority_min0[12]|sched_getscheduler0[12]|sched_rr_get_interval0[1-3]|sched_yield01|setgroups01|setitimer01|setpgid01|setrlimit01|set_robust_list01|set_tid_address01|setxattr01|sigaltstack01|sigsuspend01|socket0[1-2]|socketcall0[1-3]|socketpair01|stat01|stat02|statfs01|statfs02|statx01|statx02|symlink01|symlinkat01|sysinfo01|tgkill01|timer_create01|timer_gettime01|timer_settime01|timerfd_.*|tkill01|truncate01|umask01|umount01|uname01|unlink01|unlinkat01|utime01|utimensat01|utimes01|vfork01|wait401|waitid01|waitpid01|waitpid02|write01|write02|write03|writev01|unshare.*|setns.*|clone.*|clone3.*|mount.*|mount_setattr.*|open_tree.*|move_mount.*|fsopen.*|fsmount.*|fspick.*|pivot_root.*|chroot.*|execve.*|execveat.*|mkdir.*|mkdirat.*|chown.*|fchownat.*|chmod.*|fchmodat.*|statx.*|openat.*|faccessat.*|dup0[2-7]|dup20[3-7]|dup3_0[12]|epoll_create0[12]|epoll_create1_0[12]|epoll01|epoll_ctl0[2-5]|epoll_wait0[2-7]|epoll_pwait0[2-6]|getegid02|getegid02_16|geteuid02|geteuid02_16|getgid03|getgid03_16|getppid01|getppid02|getpgrp01|getpgid01|getpgid02|gettid01|gettid02|getuid03|getuid03_16|getsid02|get_robust_list01|getcpu01|getcpu02|getpagesize01|gethostid01|gethostname01|gethostname02|getdomainname01|confstr01|fpathconf01|getgroups03|getgroups03_16|getresuid0[1-3]|getresuid0[1-3]_16|getresgid0[1-3]|getresgid0[1-3]_16|getrandom0[1-5]|getpriority0[12]|getrusage0[24]|getrlimit0[23]|gettimeofday02|getitimer02)$"
pattern="$default_pattern"
[ -n "$cmdline_pattern" ] && pattern="$cmdline_pattern"
skip_tests=""
[ -z "$cmdline_pattern" ] && skip_tests="--skip-tests ^timerfd_settime02$"
report_dir=/tmp/seele-ltp
report="$report_dir/report.json"

mkdir -p "$report_dir"
rm -f "$report"
cat > /tmp/seele-kernel.config <<'EOF'
CONFIG_EVENTFD=y
CONFIG_EPOLL=y
CONFIG_TMPFS=y
CONFIG_PROC_FS=y
CONFIG_SYSVIPC=y
CONFIG_EXT4_FS=y
CONFIG_NET=y
CONFIG_UNIX=y
CONFIG_PID_NS=y
# CONFIG_AIO is not set
EOF
export KCONFIG_PATH=/tmp/seele-kernel.config

if [ -z "${LTP_DEV:-}" ]; then
    for dev in /dev/vd?; do
        [ "$dev" = "/dev/vda" ] && continue
        if [ -b "$dev" ]; then
            export LTP_DEV="$dev"
            break
        fi
    done
fi

LTPROOT=/usr/share LTP_COLORIZE_OUTPUT=0 kirk --no-colors \
    --run-suite "$suite" \
    --run-pattern "$pattern" \
    $skip_tests \
    --workers 1 \
    --json-report "$report"
status=$?

echo __SEELE_LTP_JSON_BEGIN__
if [ ! -s "$report" ]; then
    echo '{"results":[]}'
else
    python - "$report" <<'PY'
import re
import json
import sys
with open(sys.argv[1], "r", encoding="utf-8", errors="replace") as report:
    data = report.read()
data = re.sub(r"\x1b\[[0-?]*[ -/]*[@-~]", "", data)
data = re.sub(r"\x1b\][^\x07]*(?:\x07|\x1b\\)", "", data)
data = re.sub(r"\x1b[%()*+\-./].", "", data)
report = json.loads(data)
for result in report.get("results", []):
    test = result.get("test", {})
    status = result.get("status") or test.get("result")
    if status not in ("fail", "brok"):
        test.pop("log", None)
sys.stdout.write(json.dumps(report, separators=(",", ":")))
PY
fi
echo __SEELE_LTP_JSON_END__
echo __SEELE_LTP_EXIT__:$status
sync
exit "$status"
"#;
