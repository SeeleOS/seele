use alloc::{collections::btree_map::BTreeMap, format, string::String, vec::Vec};
use lazy_static::lazy_static;

use crate::{
    misc::time::Time,
    process::{Process, misc::ProcessID},
    s_println,
    smp::try_current_process,
};

const TARGET_COMMANDS: [&str; 2] = ["systemd-tmpfiles", "ldconfig"];

#[derive(Clone, Copy)]
pub enum PerfBucket {
    OpenAt,
    Newfstatat,
    Statx,
    Getdents64,
    Fstatfs,
    ResolvePathAt,
    Ext4Lookup,
    Ext4DirGet,
    Ext4BlockRead,
}

impl PerfBucket {
    const ALL: [Self; 9] = [
        Self::OpenAt,
        Self::Newfstatat,
        Self::Statx,
        Self::Getdents64,
        Self::Fstatfs,
        Self::ResolvePathAt,
        Self::Ext4Lookup,
        Self::Ext4DirGet,
        Self::Ext4BlockRead,
    ];

    const fn index(self) -> usize {
        match self {
            Self::OpenAt => 0,
            Self::Newfstatat => 1,
            Self::Statx => 2,
            Self::Getdents64 => 3,
            Self::Fstatfs => 4,
            Self::ResolvePathAt => 5,
            Self::Ext4Lookup => 6,
            Self::Ext4DirGet => 7,
            Self::Ext4BlockRead => 8,
        }
    }

    const fn label(self) -> &'static str {
        match self {
            Self::OpenAt => "openat",
            Self::Newfstatat => "newfstatat",
            Self::Statx => "statx",
            Self::Getdents64 => "getdents64",
            Self::Fstatfs => "fstatfs",
            Self::ResolvePathAt => "resolve_path_at",
            Self::Ext4Lookup => "ext4_lookup",
            Self::Ext4DirGet => "ext4_dir_get",
            Self::Ext4BlockRead => "ext4_block_read",
        }
    }
}

#[derive(Clone, Copy, Default)]
struct PerfCounter {
    calls: u64,
    total_ns: u64,
}

#[derive(Default)]
struct ProcessPerfStats {
    command: String,
    first_ns: u64,
    last_ns: u64,
    counters: [PerfCounter; PerfBucket::ALL.len()],
}

lazy_static! {
    static ref PROCESS_PERF_STATS: spin::Mutex<BTreeMap<ProcessID, ProcessPerfStats>> =
        spin::Mutex::new(BTreeMap::new());
}

fn should_profile_command(command: &str) -> bool {
    TARGET_COMMANDS
        .iter()
        .any(|target| command.contains(target))
}

fn current_target_process() -> Option<(ProcessID, String)> {
    let process = try_current_process()?;
    let process = process.try_lock()?;
    let command = process.command_line.first()?.clone();
    should_profile_command(&command).then_some((process.pid, command))
}

fn record_sample(pid: ProcessID, command: String, bucket: PerfBucket, elapsed_ns: u64) {
    let now_ns = Time::since_boot().as_nanoseconds();
    let mut all_stats = PROCESS_PERF_STATS.lock();
    let stats = all_stats.entry(pid).or_insert_with(|| ProcessPerfStats {
        command: command.clone(),
        first_ns: now_ns,
        last_ns: now_ns,
        counters: [PerfCounter::default(); PerfBucket::ALL.len()],
    });
    if stats.command.is_empty() {
        stats.command = command;
    }
    stats.last_ns = now_ns;
    let counter = &mut stats.counters[bucket.index()];
    counter.calls += 1;
    counter.total_ns = counter.total_ns.saturating_add(elapsed_ns);
}

pub fn profile_current_process<R>(bucket: PerfBucket, func: impl FnOnce() -> R) -> R {
    let target = current_target_process();
    let start_ns = target
        .as_ref()
        .map(|_| Time::since_boot().as_nanoseconds())
        .unwrap_or(0);
    let result = func();
    if let Some((pid, command)) = target {
        let elapsed_ns = Time::since_boot().as_nanoseconds().saturating_sub(start_ns);
        record_sample(pid, command, bucket, elapsed_ns);
    }
    result
}

pub fn log_and_clear_process_summary(process: &Process, exit_code: u64) {
    let Some(command) = process.command_line.first() else {
        PROCESS_PERF_STATS.lock().remove(&process.pid);
        return;
    };
    if !should_profile_command(command) {
        PROCESS_PERF_STATS.lock().remove(&process.pid);
        return;
    }

    let Some(stats) = PROCESS_PERF_STATS.lock().remove(&process.pid) else {
        return;
    };

    let wall_ms = stats.last_ns.saturating_sub(stats.first_ns) / 1_000_000;
    let mut parts = Vec::new();
    for bucket in PerfBucket::ALL {
        let counter = stats.counters[bucket.index()];
        if counter.calls == 0 {
            continue;
        }
        parts.push(format!(
            "{}={}x/{}ms",
            bucket.label(),
            counter.calls,
            counter.total_ns / 1_000_000
        ));
    }

    s_println!(
        "perf summary: pid={} argv0={} exit={} wall={}ms {}",
        process.pid.0,
        stats.command,
        exit_code,
        wall_ms,
        parts.join(" ")
    );
}
