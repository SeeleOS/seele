use alloc::{collections::BTreeMap, format, string::{String, ToString}, vec::Vec};
use lazy_static::lazy_static;
use spin::Mutex;

use crate::{
    misc::time::Time,
    process::{Process, ProcessExitStatus, manager::get_current_process},
};

#[derive(Clone, Copy)]
pub enum PerfBucket {
    OpenAt,
    Newfstatat,
    Statx,
    Getdents64,
    Fstatfs,
    Recvfrom,
    EpollPwait2,
    Poll,
    Pselect6,
    Futex,
    ClockGettime,
    ResolvePathAt,
    Ext4Lookup,
    Ext4DirGet,
    Ext4BlockRead,
}

impl PerfBucket {
    const ALL: [Self; 15] = [
        Self::OpenAt,
        Self::Newfstatat,
        Self::Statx,
        Self::Getdents64,
        Self::Fstatfs,
        Self::Recvfrom,
        Self::EpollPwait2,
        Self::Poll,
        Self::Pselect6,
        Self::Futex,
        Self::ClockGettime,
        Self::ResolvePathAt,
        Self::Ext4Lookup,
        Self::Ext4DirGet,
        Self::Ext4BlockRead,
    ];

    fn label(self) -> &'static str {
        match self {
            Self::OpenAt => "openat",
            Self::Newfstatat => "newfstatat",
            Self::Statx => "statx",
            Self::Getdents64 => "getdents64",
            Self::Fstatfs => "fstatfs",
            Self::Recvfrom => "recvfrom",
            Self::EpollPwait2 => "epoll_pwait2",
            Self::Poll => "poll",
            Self::Pselect6 => "pselect6",
            Self::Futex => "futex",
            Self::ClockGettime => "clock_gettime",
            Self::ResolvePathAt => "resolve_path_at",
            Self::Ext4Lookup => "ext4_lookup",
            Self::Ext4DirGet => "ext4_dir_get",
            Self::Ext4BlockRead => "ext4_block_read",
        }
    }

    fn index(self) -> usize {
        self as usize
    }
}

#[derive(Clone, Copy, Default)]
struct BucketStat {
    total_ns: u64,
    count: u64,
    slow_count: u64,
}

#[derive(Default)]
struct ProcessPerfState {
    first_seen_ns: u64,
    buckets: [BucketStat; PerfBucket::ALL.len()],
    block_counts: BTreeMap<String, u64>,
}

lazy_static! {
    static ref PROCESS_PERF: Mutex<BTreeMap<u64, ProcessPerfState>> = Mutex::new(BTreeMap::new());
}

const SLOW_BUCKET_THRESHOLD_NS: u64 = 5_000_000;
const SUMMARY_BUCKET_THRESHOLD_NS: u64 = 1_000_000;

fn now_ns() -> u64 {
    Time::since_boot().as_nanoseconds()
}

fn command_name(process: &Process) -> &str {
    process
        .command_line
        .first()
        .and_then(|command| command.rsplit('/').next())
        .unwrap_or("?")
}

#[inline]
pub fn profile_current_process<R>(bucket: PerfBucket, func: impl FnOnce() -> R) -> R {
    let start_ns = now_ns();
    let result = func();
    let elapsed_ns = now_ns().saturating_sub(start_ns);

    let process = get_current_process();
    let process = process.lock();
    let pid = process.pid.0;
    let comm = command_name(&process).to_string();

    {
        let mut perf = PROCESS_PERF.lock();
        let state = perf.entry(pid).or_insert_with(|| ProcessPerfState {
            first_seen_ns: start_ns,
            ..Default::default()
        });
        let stat = &mut state.buckets[bucket.index()];
        stat.total_ns = stat.total_ns.saturating_add(elapsed_ns);
        stat.count = stat.count.saturating_add(1);
        if elapsed_ns >= SLOW_BUCKET_THRESHOLD_NS {
            stat.slow_count = stat.slow_count.saturating_add(1);
        }
    }

    if elapsed_ns >= SLOW_BUCKET_THRESHOLD_NS {
        crate::s_println!(
            "perf slow comm={} pid={} bucket={} elapsed={}ms",
            comm,
            pid,
            bucket.label(),
            elapsed_ns / 1_000_000
        );
    }

    result
}

#[inline]
pub fn log_current_block(kind: &str) {
    let process = get_current_process();
    let process = process.lock();
    let pid = process.pid.0;
    let now_ns = now_ns();

    let mut perf = PROCESS_PERF.lock();
    let state = perf.entry(pid).or_insert_with(|| ProcessPerfState {
        first_seen_ns: now_ns,
        ..Default::default()
    });
    *state.block_counts.entry(kind.into()).or_default() += 1;
}

#[inline]
pub fn log_and_clear_process_summary(process: &Process, exit_status: ProcessExitStatus) {
    let pid = process.pid.0;
    let Some(state) = PROCESS_PERF.lock().remove(&pid) else {
        return;
    };

    let lifetime_ms = now_ns()
        .saturating_sub(state.first_seen_ns)
        .saturating_div(1_000_000);
    let mut parts = Vec::new();
    for bucket in PerfBucket::ALL {
        let stat = state.buckets[bucket.index()];
        if stat.total_ns < SUMMARY_BUCKET_THRESHOLD_NS {
            continue;
        }
        parts.push(format!(
            "{}={}ms/{}{}",
            bucket.label(),
            stat.total_ns / 1_000_000,
            stat.count,
            if stat.slow_count == 0 {
                String::new()
            } else {
                format!(" slow={}", stat.slow_count)
            }
        ));
    }

    let mut block_parts = Vec::new();
    for (kind, count) in state.block_counts {
        block_parts.push(format!("{kind}={count}"));
    }

    if parts.is_empty() && block_parts.is_empty() {
        return;
    }

    crate::s_println!(
        "perf summary comm={} pid={} exit={:?} lifetime={}ms buckets=[{}] blocks=[{}]",
        command_name(process),
        pid,
        exit_status,
        lifetime_ms,
        parts.join(", "),
        block_parts.join(", ")
    );
}
