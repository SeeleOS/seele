use alloc::{format, string::String, vec::Vec};
use core::{
    array,
    cmp::Reverse,
    sync::atomic::{AtomicU64, Ordering},
};

use crate::{
    misc::{
        get_cycles, serial_print::SERIAL_PORT, time::NANOSECONDS_PER_SECOND, time::Time,
        time::tsc_frequency_hz,
    },
    s_println,
    smp::{current_cpu_index, topology},
    thread::thread::Thread,
};
use conquer_once::spin::OnceCell;

const REPORT_INTERVAL_SECONDS: u64 = 5;
const TOP_CATEGORY_COUNT: usize = 10;
const TOP_SYSCALL_COUNT: usize = 8;
const MAX_SYSCALLS: usize = 1500;
const PROFILE_CATEGORY_COUNT: usize = 15;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum ProfileCategory {
    SchedulerWork = 0,
    SchedulerSelect = 1,
    SchedulerSwitch = 2,
    TimerWork = 3,
    NetPoll = 4,
    SyscallCpu = 5,
    PageFault = 6,
    PageFaultFileLazy = 7,
    PageFaultAnonLazy = 8,
    PageFaultCow = 9,
    IrqTimer = 10,
    IrqKeyboard = 11,
    IrqMouse = 12,
    Idle = 13,
    OtherKernel = 14,
}

impl ProfileCategory {
    const ALL: [Self; PROFILE_CATEGORY_COUNT] = [
        Self::SchedulerWork,
        Self::SchedulerSelect,
        Self::SchedulerSwitch,
        Self::TimerWork,
        Self::NetPoll,
        Self::SyscallCpu,
        Self::PageFault,
        Self::PageFaultFileLazy,
        Self::PageFaultAnonLazy,
        Self::PageFaultCow,
        Self::IrqTimer,
        Self::IrqKeyboard,
        Self::IrqMouse,
        Self::Idle,
        Self::OtherKernel,
    ];

    fn as_index(self) -> usize {
        self as usize
    }

    fn name(self) -> &'static str {
        match self {
            Self::SchedulerWork => "scheduler_work",
            Self::SchedulerSelect => "sched_select",
            Self::SchedulerSwitch => "sched_switch",
            Self::TimerWork => "timer_work",
            Self::NetPoll => "net_poll",
            Self::SyscallCpu => "syscall_cpu",
            Self::PageFault => "page_fault",
            Self::PageFaultFileLazy => "pf_file_lazy",
            Self::PageFaultAnonLazy => "pf_anon_lazy",
            Self::PageFaultCow => "pf_cow",
            Self::IrqTimer => "irq_timer",
            Self::IrqKeyboard => "irq_keyboard",
            Self::IrqMouse => "irq_mouse",
            Self::Idle => "idle",
            Self::OtherKernel => "other_kernel",
        }
    }
}

#[derive(Default)]
struct ProfileCounters {
    calls: AtomicU64,
    total_cycles: AtomicU64,
    max_cycles: AtomicU64,
}

impl ProfileCounters {
    fn record(&self, cycles: u64) {
        self.calls.fetch_add(1, Ordering::Relaxed);
        self.total_cycles.fetch_add(cycles, Ordering::Relaxed);

        let mut current = self.max_cycles.load(Ordering::Relaxed);
        while cycles > current {
            match self.max_cycles.compare_exchange_weak(
                current,
                cycles,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => break,
                Err(previous) => current = previous,
            }
        }
    }

    fn snapshot_and_reset(&self) -> ProfileSnapshot {
        ProfileSnapshot {
            calls: self.calls.swap(0, Ordering::AcqRel),
            total_cycles: self.total_cycles.swap(0, Ordering::AcqRel),
            max_cycles: self.max_cycles.swap(0, Ordering::AcqRel),
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct ProfileSnapshot {
    calls: u64,
    total_cycles: u64,
    max_cycles: u64,
}

impl core::ops::AddAssign for ProfileSnapshot {
    fn add_assign(&mut self, rhs: Self) {
        self.calls = self.calls.saturating_add(rhs.calls);
        self.total_cycles = self.total_cycles.saturating_add(rhs.total_cycles);
        self.max_cycles = self.max_cycles.max(rhs.max_cycles);
    }
}

#[derive(Default)]
struct FileLazyFaultCounters {
    faults: AtomicU64,
    cluster_pages_loaded: AtomicU64,
    cache_hits: AtomicU64,
    cache_misses: AtomicU64,
}

impl FileLazyFaultCounters {
    fn record(&self, cluster_pages_loaded: u64, cache_hits: u64, cache_misses: u64) {
        self.faults.fetch_add(1, Ordering::Relaxed);
        self.cluster_pages_loaded
            .fetch_add(cluster_pages_loaded, Ordering::Relaxed);
        self.cache_hits.fetch_add(cache_hits, Ordering::Relaxed);
        self.cache_misses.fetch_add(cache_misses, Ordering::Relaxed);
    }

    fn snapshot_and_reset(&self) -> FileLazyFaultSnapshot {
        FileLazyFaultSnapshot {
            faults: self.faults.swap(0, Ordering::AcqRel),
            cluster_pages_loaded: self.cluster_pages_loaded.swap(0, Ordering::AcqRel),
            cache_hits: self.cache_hits.swap(0, Ordering::AcqRel),
            cache_misses: self.cache_misses.swap(0, Ordering::AcqRel),
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct FileLazyFaultSnapshot {
    faults: u64,
    cluster_pages_loaded: u64,
    cache_hits: u64,
    cache_misses: u64,
}

impl core::ops::AddAssign for FileLazyFaultSnapshot {
    fn add_assign(&mut self, rhs: Self) {
        self.faults = self.faults.saturating_add(rhs.faults);
        self.cluster_pages_loaded = self
            .cluster_pages_loaded
            .saturating_add(rhs.cluster_pages_loaded);
        self.cache_hits = self.cache_hits.saturating_add(rhs.cache_hits);
        self.cache_misses = self.cache_misses.saturating_add(rhs.cache_misses);
    }
}

struct CpuProfileData {
    categories: [ProfileCounters; PROFILE_CATEGORY_COUNT],
    syscall_cpu: [ProfileCounters; MAX_SYSCALLS],
    syscall_blocked: [ProfileCounters; MAX_SYSCALLS],
    file_lazy_faults: FileLazyFaultCounters,
}

impl Default for CpuProfileData {
    fn default() -> Self {
        Self {
            categories: array::from_fn(|_| ProfileCounters::default()),
            syscall_cpu: array::from_fn(|_| ProfileCounters::default()),
            syscall_blocked: array::from_fn(|_| ProfileCounters::default()),
            file_lazy_faults: FileLazyFaultCounters::default(),
        }
    }
}

struct ProfileState {
    cpu_data: Vec<CpuProfileData>,
    last_report_ns: AtomicU64,
}

static PROFILE_STATE: OnceCell<ProfileState> = OnceCell::uninit();

pub fn init() {
    let cpu_count = topology::processors().len().max(1);
    PROFILE_STATE
        .try_init_once(|| ProfileState {
            cpu_data: (0..cpu_count).map(|_| CpuProfileData::default()).collect(),
            last_report_ns: AtomicU64::new(Time::since_boot().as_nanoseconds()),
        })
        .expect("profiling initialized twice");
}

pub fn scope_start() -> u64 {
    get_cycles()
}

pub fn record(category: ProfileCategory, start_cycles: u64) -> u64 {
    let elapsed = get_cycles().saturating_sub(start_cycles);
    record_cycles(category, elapsed);
    elapsed
}

pub fn record_cycles(category: ProfileCategory, cycles: u64) {
    if cycles == 0 {
        return;
    }

    with_cpu_profile_data(|cpu| cpu.categories[category.as_index()].record(cycles));
}

pub fn record_syscall_cpu(syscall_no: usize, cycles: u64) {
    record_syscall_counter(syscall_no, cycles, |cpu| &cpu.syscall_cpu);
}

pub fn record_syscall_blocked(syscall_no: usize, cycles: u64) {
    record_syscall_counter(syscall_no, cycles, |cpu| &cpu.syscall_blocked);
}

fn record_syscall_counter(
    syscall_no: usize,
    cycles: u64,
    counters: impl Fn(&CpuProfileData) -> &[ProfileCounters; MAX_SYSCALLS],
) {
    if cycles == 0 {
        return;
    }

    with_cpu_profile_data(|cpu| {
        if let Some(counter) = counters(cpu).get(syscall_no) {
            counter.record(cycles);
        }
    });
}

pub fn start_blocked_syscall(thread: &mut Thread) {
    if thread.active_syscall_profile.is_none() || thread.blocked_syscall_started_at.is_some() {
        return;
    }

    thread.blocked_syscall_started_at = Some(get_cycles());
}

pub fn finish_blocked_syscall(thread: &mut Thread) {
    let Some(start_cycles) = thread.blocked_syscall_started_at.take() else {
        return;
    };

    let elapsed = get_cycles().saturating_sub(start_cycles);
    thread.blocked_syscall_cycles = thread.blocked_syscall_cycles.saturating_add(elapsed);

    if let Some(blocked_syscall) = thread.active_syscall_profile {
        record_syscall_blocked(blocked_syscall.syscall_number(), elapsed);
    }
}

pub fn record_file_lazy_fault(cluster_pages_loaded: u64, cache_hits: u64, cache_misses: u64) {
    with_cpu_profile_data(|cpu| {
        cpu.file_lazy_faults
            .record(cluster_pages_loaded, cache_hits, cache_misses);
    });
}

pub fn maybe_report() {
    let now_ns = Time::since_boot().as_nanoseconds();
    let Some(state) = PROFILE_STATE.try_get().ok() else {
        return;
    };

    let last_report = state.last_report_ns.load(Ordering::Acquire);
    if now_ns.saturating_sub(last_report) < REPORT_INTERVAL_SECONDS * NANOSECONDS_PER_SECOND {
        return;
    }

    if state
        .last_report_ns
        .compare_exchange(last_report, now_ns, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return;
    }

    if SERIAL_PORT.try_get().is_err() {
        return;
    }

    report_window(now_ns.saturating_sub(last_report));
}

fn report_window(window_ns: u64) {
    let Some(state) = PROFILE_STATE.try_get().ok() else {
        return;
    };

    let mut category_totals = [ProfileSnapshot::default(); PROFILE_CATEGORY_COUNT];
    let mut syscall_cpu_totals = [ProfileSnapshot::default(); MAX_SYSCALLS];
    let mut syscall_blocked_totals = [ProfileSnapshot::default(); MAX_SYSCALLS];
    let mut file_lazy_faults = FileLazyFaultSnapshot::default();

    for cpu in &state.cpu_data {
        for category in ProfileCategory::ALL {
            category_totals[category.as_index()] +=
                cpu.categories[category.as_index()].snapshot_and_reset();
        }

        for (index, entry) in cpu.syscall_cpu.iter().enumerate() {
            syscall_cpu_totals[index] += entry.snapshot_and_reset();
        }

        for (index, entry) in cpu.syscall_blocked.iter().enumerate() {
            syscall_blocked_totals[index] += entry.snapshot_and_reset();
        }

        file_lazy_faults += cpu.file_lazy_faults.snapshot_and_reset();
    }

    let total_cycles: u64 = category_totals
        .iter()
        .map(|entry| entry.total_cycles)
        .fold(0, u64::saturating_add);

    let total_blocked_cycles: u64 = syscall_blocked_totals
        .iter()
        .map(|entry| entry.total_cycles)
        .fold(0, u64::saturating_add);

    if total_cycles == 0 && total_blocked_cycles == 0 {
        return;
    }

    let tsc_hz = tsc_frequency_hz().max(1);
    let window_ms = window_ns / 1_000_000;
    let cpu_count = topology::processors().len().max(1);

    s_println!(
        "PROFILE {}s window_ms={} cpus={} cpu_time_ms={} blocked_time_ms={}",
        REPORT_INTERVAL_SECONDS,
        window_ms,
        cpu_count,
        cycles_to_milliseconds(total_cycles, tsc_hz),
        cycles_to_milliseconds(total_blocked_cycles, tsc_hz)
    );
    s_println!("CATEGORIES");

    let mut top_categories: Vec<(ProfileCategory, ProfileSnapshot)> = ProfileCategory::ALL
        .into_iter()
        .map(|category| (category, category_totals[category.as_index()]))
        .filter(|(_, snapshot)| snapshot.calls != 0)
        .collect();
    top_categories.sort_by_key(|entry| Reverse(entry.1.total_cycles));

    for (category, snapshot) in top_categories.into_iter().take(TOP_CATEGORY_COUNT) {
        let percent = snapshot
            .total_cycles
            .saturating_mul(10_000)
            .checked_div(total_cycles)
            .unwrap_or(0);
        s_println!(
            "  {:<16} {:>3}.{:02}% calls={} cpu_time_ms={} max_cpu_us={}",
            category.name(),
            percent / 100,
            percent % 100,
            snapshot.calls,
            cycles_to_milliseconds(snapshot.total_cycles, tsc_hz),
            cycles_to_microseconds(snapshot.max_cycles, tsc_hz),
        );
    }

    report_syscalls("TOP CPU SYSCALLS", &syscall_cpu_totals, tsc_hz, true);
    report_syscalls(
        "TOP BLOCKED SYSCALLS",
        &syscall_blocked_totals,
        tsc_hz,
        false,
    );

    if file_lazy_faults.faults != 0 {
        s_println!(
            "FILE_LAZY_FAULTS faults={} cluster_pages_loaded={} cache_hit={} cache_miss={}",
            file_lazy_faults.faults,
            file_lazy_faults.cluster_pages_loaded,
            file_lazy_faults.cache_hits,
            file_lazy_faults.cache_misses
        );
    }
}

fn report_syscalls(title: &str, totals: &[ProfileSnapshot; MAX_SYSCALLS], tsc_hz: u64, cpu: bool) {
    s_println!("{title}");
    let mut top_syscalls: Vec<(usize, ProfileSnapshot)> = totals
        .iter()
        .copied()
        .enumerate()
        .filter(|(_, snapshot)| snapshot.calls != 0)
        .collect();
    top_syscalls.sort_by_key(|entry| Reverse(entry.1.total_cycles));

    for (syscall_no, snapshot) in top_syscalls.into_iter().take(TOP_SYSCALL_COUNT) {
        let label = syscall_name(syscall_no);
        if cpu {
            s_println!(
                "  {:<18} calls={} cpu_time_ms={} max_cpu_us={}",
                label,
                snapshot.calls,
                cycles_to_milliseconds(snapshot.total_cycles, tsc_hz),
                cycles_to_microseconds(snapshot.max_cycles, tsc_hz),
            );
        } else {
            s_println!(
                "  {:<18} calls={} blocked_time_ms={} max_blocked_us={}",
                label,
                snapshot.calls,
                cycles_to_milliseconds(snapshot.total_cycles, tsc_hz),
                cycles_to_microseconds(snapshot.max_cycles, tsc_hz),
            );
        }
    }
}

fn with_cpu_profile_data<R>(f: impl FnOnce(&CpuProfileData) -> R) -> R {
    let state = PROFILE_STATE.get().expect("profiling not initialized");
    let index = current_cpu_index();
    let cpu = state
        .cpu_data
        .get(index)
        .unwrap_or_else(|| panic!("missing profiling slot for cpu {index}"));
    f(cpu)
}

fn cycles_to_microseconds(cycles: u64, tsc_hz: u64) -> u64 {
    ((cycles as u128) * 1_000_000u128 / (tsc_hz as u128)) as u64
}

fn cycles_to_milliseconds(cycles: u64, tsc_hz: u64) -> u64 {
    ((cycles as u128) * 1_000u128 / (tsc_hz as u128)) as u64
}

fn syscall_name(syscall_no: usize) -> String {
    use crate::systemcall::numbers::SyscallNumber;

    if let Some(number) = SyscallNumber::from_number(syscall_no) {
        format!("{number:?}({syscall_no})")
    } else {
        format!("syscall({syscall_no})")
    }
}
