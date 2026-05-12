use alloc::vec::Vec;
use core::{
    array,
    cmp::Reverse,
    sync::atomic::{AtomicU64, Ordering},
};

use conquer_once::spin::OnceCell;

use crate::{
    misc::{
        get_cycles, serial_print::SERIAL_PORT, time::NANOSECONDS_PER_SECOND, time::Time,
        time::tsc_frequency_hz,
    },
    s_println,
    smp::{current_cpu_index, topology},
};

const REPORT_INTERVAL_SECONDS: u64 = 5;
const TOP_CATEGORY_COUNT: usize = 8;
const TOP_SYSCALL_COUNT: usize = 8;
const MAX_SYSCALLS: usize = 1500;
const PROFILE_CATEGORY_COUNT: usize = 12;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum ProfileCategory {
    Scheduler = 0,
    SchedulerSelect = 1,
    SchedulerSwitch = 2,
    TimerWork = 3,
    NetPoll = 4,
    Syscall = 5,
    PageFault = 6,
    IrqTimer = 7,
    IrqKeyboard = 8,
    IrqMouse = 9,
    Idle = 10,
    OtherKernel = 11,
}

impl ProfileCategory {
    const ALL: [Self; PROFILE_CATEGORY_COUNT] = [
        Self::Scheduler,
        Self::SchedulerSelect,
        Self::SchedulerSwitch,
        Self::TimerWork,
        Self::NetPoll,
        Self::Syscall,
        Self::PageFault,
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
            Self::Scheduler => "scheduler",
            Self::SchedulerSelect => "sched_select",
            Self::SchedulerSwitch => "sched_switch",
            Self::TimerWork => "timer_work",
            Self::NetPoll => "net_poll",
            Self::Syscall => "syscall",
            Self::PageFault => "page_fault",
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

struct CpuProfileData {
    categories: [ProfileCounters; PROFILE_CATEGORY_COUNT],
    syscalls: [ProfileCounters; MAX_SYSCALLS],
}

impl Default for CpuProfileData {
    fn default() -> Self {
        Self {
            categories: array::from_fn(|_| ProfileCounters::default()),
            syscalls: array::from_fn(|_| ProfileCounters::default()),
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

pub fn record_syscall(syscall_no: usize, cycles: u64) {
    if cycles == 0 {
        return;
    }

    with_cpu_profile_data(|cpu| {
        if let Some(counter) = cpu.syscalls.get(syscall_no) {
            counter.record(cycles);
        }
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
    let mut syscall_totals = [ProfileSnapshot::default(); MAX_SYSCALLS];

    for cpu in &state.cpu_data {
        for category in ProfileCategory::ALL {
            category_totals[category.as_index()] +=
                cpu.categories[category.as_index()].snapshot_and_reset();
        }

        for (index, entry) in cpu.syscalls.iter().enumerate() {
            syscall_totals[index] += entry.snapshot_and_reset();
        }
    }

    let total_cycles: u64 = category_totals
        .iter()
        .map(|entry| entry.total_cycles)
        .fold(0, u64::saturating_add);

    if total_cycles == 0 {
        return;
    }

    let tsc_hz = tsc_frequency_hz().max(1);
    let window_ms = window_ns / 1_000_000;
    let cpu_count = topology::processors().len().max(1);

    s_println!(
        "PROFILE {}s total_ms={} cpus={} total_cycles={}",
        REPORT_INTERVAL_SECONDS,
        window_ms,
        cpu_count,
        total_cycles
    );
    s_println!("CATEGORIES");

    let mut top_categories: Vec<(ProfileCategory, ProfileSnapshot)> = ProfileCategory::ALL
        .into_iter()
        .map(|category| (category, category_totals[category.as_index()]))
        .filter(|(_, snapshot)| snapshot.calls != 0)
        .collect();
    top_categories.sort_by_key(|entry| Reverse(entry.1.total_cycles));

    for (category, snapshot) in top_categories.into_iter().take(TOP_CATEGORY_COUNT) {
        let percent = snapshot.total_cycles.saturating_mul(10_000) / total_cycles;
        s_println!(
            "  {:<14} {:>3}.{:02}% calls={} avg_us={} max_us={} total_ms={}",
            category.name(),
            percent / 100,
            percent % 100,
            snapshot.calls,
            cycles_to_microseconds(snapshot.total_cycles / snapshot.calls.max(1), tsc_hz),
            cycles_to_microseconds(snapshot.max_cycles, tsc_hz),
            cycles_to_milliseconds(snapshot.total_cycles, tsc_hz)
        );
    }

    s_println!("TOP SYSCALLS");
    let mut top_syscalls: Vec<(usize, ProfileSnapshot)> = syscall_totals
        .iter()
        .copied()
        .enumerate()
        .filter(|(_, snapshot)| snapshot.calls != 0)
        .collect();
    top_syscalls.sort_by_key(|entry| Reverse(entry.1.total_cycles));

    for (syscall_no, snapshot) in top_syscalls.into_iter().take(TOP_SYSCALL_COUNT) {
        s_println!(
            "  {:<4} calls={} avg_us={} max_us={} total_ms={}",
            syscall_no,
            snapshot.calls,
            cycles_to_microseconds(snapshot.total_cycles / snapshot.calls.max(1), tsc_hz),
            cycles_to_microseconds(snapshot.max_cycles, tsc_hz),
            cycles_to_milliseconds(snapshot.total_cycles, tsc_hz)
        );
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
