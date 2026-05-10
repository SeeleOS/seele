use alloc::{collections::BTreeMap, format, string::String, vec::Vec};
use lazy_static::lazy_static;
use spin::Mutex;

use crate::{
    misc::time::Time,
    process::{Process, ProcessExitStatus, misc::ProcessID},
    smp::try_current_process,
};

const SLOW_BUCKET_THRESHOLD_MS: u64 = 250;
const SLOW_BUCKET_THRESHOLD_NS: u64 = SLOW_BUCKET_THRESHOLD_MS * 1_000_000;
const SUMMARY_BUCKET_LIMIT: usize = 6;
const SUMMARY_BLOCK_LIMIT: usize = 6;

lazy_static! {
    static ref PROCESS_SUMMARIES: Mutex<BTreeMap<u64, ProcessPerfSummary>> =
        Mutex::new(BTreeMap::new());
}

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
    const COUNT: usize = 15;
    const ALL: [Self; Self::COUNT] = [
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

    const fn index(self) -> usize {
        self as usize
    }

    const fn name(self) -> &'static str {
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
            Self::ResolvePathAt => "resolve_path",
            Self::Ext4Lookup => "ext4_lookup",
            Self::Ext4DirGet => "ext4_dir_get",
            Self::Ext4BlockRead => "ext4_block_read",
        }
    }
}

#[derive(Clone, Copy, Default)]
struct BucketStat {
    count: u64,
    total_ns: u64,
    max_ns: u64,
}

struct ProcessPerfSummary {
    comm: String,
    exec_path: String,
    started_at: Time,
    buckets: [BucketStat; PerfBucket::COUNT],
    block_counts: BTreeMap<String, u64>,
}

impl ProcessPerfSummary {
    fn new(comm: String, exec_path: String, started_at: Time) -> Self {
        Self {
            comm,
            exec_path,
            started_at,
            buckets: [BucketStat::default(); PerfBucket::COUNT],
            block_counts: BTreeMap::new(),
        }
    }
}

#[derive(Clone)]
struct ProcessPerfIdentity {
    comm: String,
    exec_path: String,
}

fn profiled_comm(comm: &str) -> bool {
    matches!(
        comm,
        "startplasma-wayland"
            | "startplasma-x11"
            | "ksplashqml"
            | "plasmashell"
            | "kwin_wayland"
            | "kwin_wayland_wrapper"
            | "kwin_x11"
            | "kded6"
            | "ksmserver"
            | "Xorg"
            | "icewm"
    )
}

fn process_comm(process: &Process) -> String {
    process
        .command_line
        .first()
        .and_then(|command| command.rsplit('/').next())
        .unwrap_or("?")
        .into()
}

fn current_pid() -> Option<ProcessID> {
    Some(try_current_process()?.try_lock()?.pid)
}

fn current_profiled_process() -> Option<(u64, ProcessPerfIdentity)> {
    let pid = current_pid()?.0;
    let summaries = PROCESS_SUMMARIES.lock();
    let summary = summaries.get(&pid)?;
    Some((
        pid,
        ProcessPerfIdentity {
            comm: summary.comm.clone(),
            exec_path: summary.exec_path.clone(),
        },
    ))
}

pub fn is_current_process_profiled() -> bool {
    current_pid()
        .map(|pid| PROCESS_SUMMARIES.lock().contains_key(&pid.0))
        .unwrap_or(false)
}

fn format_exit_status(exit_status: ProcessExitStatus) -> String {
    match exit_status {
        ProcessExitStatus::Exited(code) => format!("exit({code})"),
        ProcessExitStatus::Signaled(signal) => format!("signal({signal:?})"),
    }
}

fn update_summary(
    pid: u64,
    comm: &str,
    exec_path: &str,
    started_at: Time,
    f: impl FnOnce(&mut ProcessPerfSummary),
) {
    let mut summaries = PROCESS_SUMMARIES.lock();
    let summary = summaries
        .entry(pid)
        .or_insert_with(|| ProcessPerfSummary::new(comm.into(), exec_path.into(), started_at));
    if summary.comm != comm || summary.exec_path != exec_path {
        *summary = ProcessPerfSummary::new(comm.into(), exec_path.into(), started_at);
    }
    f(summary);
}

pub fn note_execve(process: &Process, exec_path: &str) {
    let pid = process.pid.0;
    let comm = process_comm(process);
    let now = Time::since_boot();
    let mut summaries = PROCESS_SUMMARIES.lock();
    if !profiled_comm(&comm) {
        summaries.remove(&pid);
        return;
    }

    summaries.insert(
        pid,
        ProcessPerfSummary::new(comm.clone(), exec_path.into(), now),
    );
    drop(summaries);

    crate::s_println!(
        "kde-perf exec pid={} comm={} t={}ms path={}",
        pid,
        comm,
        now.as_milliseconds(),
        exec_path
    );
}

#[inline]
pub fn profile_current_process<R>(bucket: PerfBucket, func: impl FnOnce() -> R) -> R {
    let maybe_pid = current_pid().map(|pid| pid.0);
    let start = maybe_pid.as_ref().map(|_| Time::since_boot());
    let result = func();

    let Some(pid) = maybe_pid else {
        return result;
    };
    let Some(start) = start else {
        return result;
    };

    let elapsed_ns = Time::since_boot().sub(start).as_nanoseconds();
    let mut summaries = PROCESS_SUMMARIES.lock();
    let Some(summary) = summaries.get_mut(&pid) else {
        return result;
    };
    let comm = if elapsed_ns >= SLOW_BUCKET_THRESHOLD_NS {
        Some(summary.comm.clone())
    } else {
        None
    };
    {
        let stat = &mut summary.buckets[bucket.index()];
        stat.count = stat.count.saturating_add(1);
        stat.total_ns = stat.total_ns.saturating_add(elapsed_ns);
        stat.max_ns = stat.max_ns.max(elapsed_ns);
    }
    drop(summaries);

    if let Some(comm) = comm {
        crate::s_println!(
            "kde-perf slow pid={} comm={} bucket={} dur={}ms at={}ms",
            pid,
            comm,
            bucket.name(),
            elapsed_ns / 1_000_000,
            Time::since_boot().as_milliseconds()
        );
    }

    result
}

#[inline]
pub fn log_current_block(kind: &str) {
    let Some((pid, identity)) = current_profiled_process() else {
        return;
    };

    update_summary(
        pid,
        &identity.comm,
        &identity.exec_path,
        Time::since_boot(),
        |summary| {
            *summary.block_counts.entry(kind.into()).or_default() += 1;
        },
    );
}

#[inline]
pub fn log_and_clear_process_summary(process: &Process, exit_status: ProcessExitStatus) {
    let pid = process.pid.0;
    let comm = process_comm(process);
    if !profiled_comm(&comm) {
        PROCESS_SUMMARIES.lock().remove(&pid);
        return;
    }

    let now = Time::since_boot();
    let summary = PROCESS_SUMMARIES.lock().remove(&pid);
    let Some(summary) = summary else {
        crate::s_println!(
            "kde-perf exit pid={} comm={} status={} t={}ms summary=none",
            pid,
            comm,
            format_exit_status(exit_status),
            now.as_milliseconds()
        );
        return;
    };

    let mut bucket_parts = Vec::new();
    for bucket in PerfBucket::ALL {
        let stat = summary.buckets[bucket.index()];
        if stat.count == 0 || stat.total_ns == 0 {
            continue;
        }
        bucket_parts.push((
            stat.total_ns,
            format!(
                "{}={}ms max={}ms n={}",
                bucket.name(),
                stat.total_ns / 1_000_000,
                stat.max_ns / 1_000_000,
                stat.count
            ),
        ));
    }
    bucket_parts.sort_unstable_by_key(|part| core::cmp::Reverse(part.0));
    let bucket_summary = bucket_parts
        .into_iter()
        .take(SUMMARY_BUCKET_LIMIT)
        .map(|(_, part)| part)
        .collect::<Vec<_>>()
        .join(", ");

    let mut block_parts = summary
        .block_counts
        .into_iter()
        .map(|(kind, count)| (count, format!("{kind}={count}")))
        .collect::<Vec<_>>();
    block_parts.sort_unstable_by_key(|part| core::cmp::Reverse(part.0));
    let block_summary = block_parts
        .into_iter()
        .take(SUMMARY_BLOCK_LIMIT)
        .map(|(_, part)| part)
        .collect::<Vec<_>>()
        .join(", ");

    crate::s_println!(
        "kde-perf exit pid={} comm={} status={} lifetime={}ms buckets=[{}] blocks=[{}]",
        pid,
        summary.comm,
        format_exit_status(exit_status),
        now.sub(summary.started_at).as_milliseconds(),
        bucket_summary,
        block_summary
    );
}
