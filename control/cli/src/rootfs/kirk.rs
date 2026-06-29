use anyhow::{Context, Result};
use std::{fs, path::Path};
use xshell::{Shell, cmd};

const KIRK_URL: &str = "https://github.com/linux-test-project/kirk";

pub fn install_kirk(sh: &Shell, repo: &Path, rootfs_mount: &Path) -> Result<()> {
    let kirk_checkout = repo.join("target").join("kirk");
    let host_kirk_dir = rootfs_mount.join("opt").join("kirk");
    let host_kirk_bin = rootfs_mount.join("usr/local/bin/kirk");
    let kirk_bin = repo.join("target").join("kirk-runner");
    let ltp_runner = repo.join("target").join("seele-run-ltp");
    let host_ltp_runner = rootfs_mount.join("usr/local/bin/seele-run-ltp");

    if !kirk_checkout.exists() {
        let depth = "1";
        cmd!(sh, "git clone --depth {depth} {KIRK_URL} {kirk_checkout}").run()?;
    } else {
        let depth = "1";
        cmd!(
            sh,
            "git -C {kirk_checkout} fetch --depth {depth} origin master"
        )
        .run()?;
        cmd!(sh, "git -C {kirk_checkout} reset --hard FETCH_HEAD").run()?;
        cmd!(sh, "git -C {kirk_checkout} clean -fdx").run()?;
    }

    let script = format!(
        "sudo rm -rf {} && sudo mkdir -p {} && sudo cp -a {}/. {}",
        quote(&host_kirk_dir),
        quote(&host_kirk_dir),
        quote(&kirk_checkout),
        quote(&host_kirk_dir)
    );
    cmd!(sh, "bash -lc {script}").run()?;
    fs::write(&kirk_bin, KIRK_RUNNER).context("failed to write kirk runner script")?;
    let mode = "-Dm755";
    cmd!(sh, "sudo install {mode} {kirk_bin} {host_kirk_bin}").run()?;
    fs::write(&ltp_runner, LTP_RUNNER).context("failed to write LTP runner script")?;
    cmd!(sh, "sudo install {mode} {ltp_runner} {host_ltp_runner}").run()?;
    Ok(())
}

fn quote(path: &Path) -> String {
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
suites="syscalls containers commands"
[ -n "$cmdline_suite" ] && suites="$cmdline_suite"
export LTP_SINGLE_FS_TYPE="${LTP_SINGLE_FS_TYPE:-ext4}"
pattern=""
syscall_pattern="abort.*|arch_prctl.*|accept.*|accept4.*|access.*|acct.*|add_key.*|keyctl.*|request_key.*|alarm.*|bind.*|brk.*|sbrk.*|capget.*|capset.*|chdir.*|chmod.*|fchmod.*|fchmodat.*|chown.*|fchown.*|fchownat.*|lchown.*|clock_.*|clone.*|clone3.*|close.*|close_range.*|connect.*|creat.*|dup.*|epoll.*|eventfd.*|exit.*|exit_group.*|execve.*|execveat.*|faccessat.*|fallocate.*|fchdir.*|fcntl.*|flock.*|fdatasync.*|fgetxattr.*|flistxattr.*|fremovexattr.*|fsetxattr.*|fork.*|fstat.*|fstatat.*|fstatfs.*|fsync.*|ftruncate.*|getcwd.*|getdents.*|getegid.*|geteuid.*|getgid.*|getgroups.*|getitimer.*|getpeername.*|getpid.*|getppid.*|getpgrp.*|getpgid.*|gettid.*|getuid.*|getrlimit.*|getrusage.*|getsid.*|getsockname.*|getsockopt.*|gettimeofday.*|getxattr.*|get_robust_list.*|getcpu.*|getpagesize.*|gethostid.*|gethostname.*|getdomainname.*|confstr.*|fpathconf.*|getresuid.*|getresgid.*|getrandom.*|getpriority.*|ioctl.*|kill.*|lgetxattr.*|link.*|linkat.*|listen.*|listxattr.*|llistxattr.*|lseek.*|lstat.*|lremovexattr.*|lsetxattr.*|madvise.*|memfd_create.*|mincore.*|mkdir.*|mkdirat.*|mlock.*|mmap.*|mmap_fixed.*|mmapstress.*|mknod.*|mknodat.*|mount.*|mount_setattr.*|open_tree.*|move_mount.*|fsopen.*|fsmount.*|fspick.*|pivot_root.*|chroot.*|mprotect.*|mremap.*|msync.*|munlock.*|munmap.*|nanosleep.*|newfstatat.*|open.*|openat.*|pipe.*|pidfd_open.*|pidfd_send_signal.*|poll.*|ppoll.*|posix_fadvise.*|pread.*|preadv.*|prlimit64.*|pselect.*|pwrite.*|pwritev.*|read.*|readlink.*|readlinkat.*|readv.*|recvfrom.*|removexattr.*|rename.*|renameat.*|rmdir.*|rt_sigaction.*|rt_sigpending.*|rt_sigprocmask.*|select.*|sendto.*|setsockopt.*|sched_.*|setgroups.*|setitimer.*|setpgid.*|setrlimit.*|set_robust_list.*|set_tid_address.*|setxattr.*|sigaltstack.*|sigsuspend.*|socket.*|socketcall.*|socketpair.*|stat.*|statfs.*|statx.*|symlink.*|symlinkat.*|sysinfo.*|tgkill.*|timer_.*|timerfd_.*|tkill.*|truncate.*|umask.*|umount.*|uname.*|unlink.*|unlinkat.*|utime.*|utimensat.*|utimes.*|vfork.*|wait4.*|waitid.*|waitpid.*|write.*|writev.*|llseek.*|futimesat.*|readahead.*|copy_file_range.*|unshare.*|setns.*"
namespace_pattern="pidns.*|mqns_.*|netns_.*|shm.*|shmem_.*|mesgq_.*|msg_.*|sem.*|utsname.*|mountns.*|userns.*|timens.*|clock_.*|sysinfo.*|timerfd_.*|ioctl_ns.*|ioctl_pidfd.*|unshare.*|setns.*|clone.*|clone3.*"

append_pattern() {
    [ -z "$1" ] && return
    if [ -z "$pattern" ]; then
        pattern="$1"
    else
        pattern="$pattern|$1"
    fi
}

append_pattern "$syscall_pattern"
append_pattern "$namespace_pattern"
pattern="^($pattern)$"
[ -n "$cmdline_pattern" ] && pattern="$cmdline_pattern"
skip_tests=""
[ -z "$cmdline_pattern" ] && skip_tests="--skip-tests ^(timerfd_settime02|pidns05)$"
report_dir=/tmp/seele-ltp

mkdir -p "$report_dir"
rm -f "$report_dir"/report-*.json
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
CONFIG_USER_NS=y
CONFIG_NET_NS=y
CONFIG_TIME_NS=y
CONFIG_BSD_PROCESS_ACCT=y
CONFIG_BSD_PROCESS_ACCT_V3=y
CONFIG_BPF=y
CONFIG_BPF_SYSCALL=y
CONFIG_PACKET=y
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

status=0
for suite in $suites; do
    report="$report_dir/report-$suite.json"
    LTPROOT=/usr/share LTP_COLORIZE_OUTPUT=0 kirk --no-colors \
        --run-suite "$suite" \
        --run-pattern "$pattern" \
        $skip_tests \
        --workers 1 \
        --json-report "$report"
    suite_status=$?
    [ "$suite_status" -ne 0 ] && status="$suite_status"
done

echo __SEELE_LTP_JSON_BEGIN__
python - "$report_dir" <<'PY'
import glob
import os
import re
import json
import sys
combined = {"results": []}
for path in sorted(glob.glob(sys.argv[1] + "/report-*.json")):
    if not path or not os.path.getsize(path):
        continue
    with open(path, "r", encoding="utf-8", errors="replace") as report_file:
        data = report_file.read()
    data = re.sub(r"\x1b\[[0-?]*[ -/]*[@-~]", "", data)
    data = re.sub(r"\x1b\][^\x07]*(?:\x07|\x1b\\)", "", data)
    data = re.sub(r"\x1b[%()*+\-./].", "", data)
    report = json.loads(data)
    combined["results"].extend(report.get("results", []))
sys.stdout.write(json.dumps(combined, separators=(",", ":")))
PY
echo __SEELE_LTP_JSON_END__
echo __SEELE_LTP_EXIT__:$status
sync
exit "$status"
"#;
