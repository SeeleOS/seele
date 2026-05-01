use core::sync::atomic::{AtomicU64, Ordering};

use crate::{
    misc::time::Time,
    process::{Process, ProcessExitStatus},
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

    fn index(self) -> usize {
        self as usize
    }

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
}

struct BucketPerfStat {
    total_ns: AtomicU64,
    count: AtomicU64,
    slow_count: AtomicU64,
}

impl BucketPerfStat {
    const fn new() -> Self {
        Self {
            total_ns: AtomicU64::new(0),
            count: AtomicU64::new(0),
            slow_count: AtomicU64::new(0),
        }
    }
}

static BUCKET_STATS: [BucketPerfStat; PerfBucket::ALL.len()] = [
    BucketPerfStat::new(),
    BucketPerfStat::new(),
    BucketPerfStat::new(),
    BucketPerfStat::new(),
    BucketPerfStat::new(),
    BucketPerfStat::new(),
    BucketPerfStat::new(),
    BucketPerfStat::new(),
    BucketPerfStat::new(),
    BucketPerfStat::new(),
    BucketPerfStat::new(),
    BucketPerfStat::new(),
    BucketPerfStat::new(),
    BucketPerfStat::new(),
    BucketPerfStat::new(),
];

const SLOW_BUCKET_THRESHOLD_NS: u64 = 20_000_000;
const SUMMARY_BUCKET_THRESHOLD_NS: u64 = 5_000_000;

fn now_ns() -> u64 {
    Time::since_boot().as_nanoseconds()
}

#[inline]
pub fn profile_current_process<R>(bucket: PerfBucket, func: impl FnOnce() -> R) -> R {
    let start_ns = now_ns();
    let result = func();
    let elapsed_ns = now_ns().saturating_sub(start_ns);

    let stat = &BUCKET_STATS[bucket.index()];
    stat.total_ns.fetch_add(elapsed_ns, Ordering::Relaxed);
    stat.count.fetch_add(1, Ordering::Relaxed);

    if elapsed_ns >= SLOW_BUCKET_THRESHOLD_NS {
        stat.slow_count.fetch_add(1, Ordering::Relaxed);
        crate::s_println!(
            "perf slow bucket={} elapsed={}ms",
            bucket.label(),
            elapsed_ns / 1_000_000
        );
    }

    result
}

#[inline]
pub fn log_current_block(_kind: &str) {}

#[inline]
pub fn log_and_clear_process_summary(process: &Process, exit_status: ProcessExitStatus) {
    let mut parts = alloc::vec::Vec::new();
    for bucket in PerfBucket::ALL {
        let stat = &BUCKET_STATS[bucket.index()];
        let total_ns = stat.total_ns.load(Ordering::Relaxed);
        if total_ns < SUMMARY_BUCKET_THRESHOLD_NS {
            continue;
        }

        parts.push(alloc::format!(
            "{}={}ms/{} slow={}",
            bucket.label(),
            total_ns / 1_000_000,
            stat.count.load(Ordering::Relaxed),
            stat.slow_count.load(Ordering::Relaxed)
        ));
    }

    if parts.is_empty() {
        return;
    }

    let comm = process
        .command_line
        .first()
        .and_then(|command| command.rsplit('/').next())
        .unwrap_or("?");
    crate::s_println!(
        "perf summary comm={} pid={} exit={:?} buckets=[{}]",
        comm,
        process.pid.0,
        exit_status,
        parts.join(", ")
    );
}
