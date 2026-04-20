use core::sync::atomic::{AtomicI32, AtomicI64, AtomicU64, Ordering};

use crate::misc::get_cycles;
use x86_rtc::Rtc;

static BOOT_TSC: AtomicU64 = AtomicU64::new(0);
static TSC_FREQ_HZ: AtomicU64 = AtomicU64::new(0);
static REALTIME_BASE_NS: AtomicI64 = AtomicI64::new(0);
static TIMEZONE_MINUTESWEST: AtomicI32 = AtomicI32::new(0);
static TIMEZONE_DSTTIME: AtomicI32 = AtomicI32::new(0);

pub const NANOSECONDS_PER_MICROSECOND: u64 = 1_000;
pub const NANOSECONDS_PER_MILLISECOND: u64 = 1_000_000;
pub const NANOSECONDS_PER_SECOND: u64 = 1_000_000_000;
const DEFAULT_TSC_FREQ_HZ: u64 = 1_000_000_000;
const PROFILING: bool = false;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Default)]
pub struct Time(pub u64);

pub fn init() {
    BOOT_TSC.store(get_cycles(), Ordering::SeqCst);
    TSC_FREQ_HZ.store(
        detect_tsc_frequency_hz().unwrap_or(DEFAULT_TSC_FREQ_HZ),
        Ordering::SeqCst,
    );
    REALTIME_BASE_NS.store(
        (Rtc::new().get_unix_timestamp() as i64).saturating_mul(NANOSECONDS_PER_SECOND as i64),
        Ordering::SeqCst,
    );
}

fn nanoseconds_since_boot() -> u64 {
    let boot_tsc = BOOT_TSC.load(Ordering::SeqCst);
    let tsc_freq_hz = TSC_FREQ_HZ.load(Ordering::SeqCst);

    if boot_tsc == 0 || tsc_freq_hz == 0 {
        return 0;
    }

    let delta_cycles = get_cycles().saturating_sub(boot_tsc);
    ((delta_cycles as u128) * (NANOSECONDS_PER_SECOND as u128) / (tsc_freq_hz as u128)) as u64
}

pub fn unix_timestamp_seconds() -> u64 {
    unix_timestamp_nanoseconds() / NANOSECONDS_PER_SECOND
}

pub fn unix_timestamp_nanoseconds() -> u64 {
    let current = REALTIME_BASE_NS
        .load(Ordering::SeqCst)
        .saturating_add(nanoseconds_since_boot() as i64);
    current.max(0) as u64
}

pub fn set_unix_timestamp_nanoseconds(unix_time_ns: i64) {
    REALTIME_BASE_NS.store(
        unix_time_ns.saturating_sub(nanoseconds_since_boot() as i64),
        Ordering::SeqCst,
    );
}

pub fn timezone() -> (i32, i32) {
    (
        TIMEZONE_MINUTESWEST.load(Ordering::SeqCst),
        TIMEZONE_DSTTIME.load(Ordering::SeqCst),
    )
}

pub fn set_timezone(minuteswest: i32, dsttime: i32) {
    TIMEZONE_MINUTESWEST.store(minuteswest, Ordering::SeqCst);
    TIMEZONE_DSTTIME.store(dsttime, Ordering::SeqCst);
}

impl Time {
    pub const fn from_nanoseconds(nanoseconds: u64) -> Self {
        Self(nanoseconds)
    }

    pub fn current() -> Self {
        Self::from_nanoseconds(unix_timestamp_nanoseconds())
    }

    pub fn since_boot() -> Self {
        Self::from_nanoseconds(nanoseconds_since_boot())
    }

    pub const fn as_nanoseconds(self) -> u64 {
        self.0
    }

    pub const fn add_ns(self, nanoseconds: u64) -> Self {
        Self::from_nanoseconds(self.0.saturating_add(nanoseconds))
    }

    pub const fn add_ms(self, milliseconds: u64) -> Self {
        self.add_ns(milliseconds.saturating_mul(NANOSECONDS_PER_MILLISECOND))
    }

    pub const fn add_sec(self, seconds: u64) -> Self {
        self.add_ns(seconds.saturating_mul(NANOSECONDS_PER_SECOND))
    }

    pub const fn sub(self, other: Self) -> Self {
        Self::from_nanoseconds(self.0.saturating_sub(other.0))
    }

    pub const fn as_microseconds(self) -> u64 {
        self.0 / NANOSECONDS_PER_MICROSECOND
    }

    pub const fn as_milliseconds(self) -> u64 {
        self.0 / NANOSECONDS_PER_MILLISECOND
    }

    pub const fn as_seconds(self) -> u64 {
        self.0 / NANOSECONDS_PER_SECOND
    }

    pub const fn subsec_nanoseconds(self) -> u64 {
        self.0 % NANOSECONDS_PER_SECOND
    }

    pub const fn subsec_microseconds(self) -> u64 {
        self.subsec_nanoseconds() / NANOSECONDS_PER_MICROSECOND
    }

    pub const fn subsec_milliseconds(self) -> u64 {
        self.subsec_nanoseconds() / NANOSECONDS_PER_MILLISECOND
    }

    pub const fn unix_timestamp(self) -> u64 {
        self.as_seconds()
    }
}

pub fn with_profiling<T, F>(f: F, label: &str) -> T
where
    F: FnOnce() -> T,
{
    if !PROFILING {
        return f();
    }

    let start = Time::since_boot();
    crate::s_println!(
        "[profile] start {} at {}.{:03}s",
        label,
        start.as_seconds(),
        start.subsec_milliseconds()
    );

    let result = f();

    let end = Time::since_boot();
    let elapsed = end.sub(start);
    crate::s_println!(
        "[profile] end {} at {}.{:03}s (+{} ms)",
        label,
        end.as_seconds(),
        end.subsec_milliseconds(),
        elapsed.as_milliseconds()
    );

    result
}

fn detect_tsc_frequency_hz() -> Option<u64> {
    detect_tsc_frequency_from_leaf_0x15().or_else(detect_tsc_frequency_from_leaf_0x16)
}

fn detect_tsc_frequency_from_leaf_0x15() -> Option<u64> {
    let max_leaf = cpuid(0).eax;
    if max_leaf < 0x15 {
        return None;
    }

    let leaf = cpuid(0x15);
    if leaf.eax == 0 || leaf.ebx == 0 || leaf.ecx == 0 {
        return None;
    }

    Some((leaf.ecx as u64).saturating_mul(leaf.ebx as u64) / (leaf.eax as u64))
}

fn detect_tsc_frequency_from_leaf_0x16() -> Option<u64> {
    let max_leaf = cpuid(0).eax;
    if max_leaf < 0x16 {
        return None;
    }

    let leaf = cpuid(0x16);
    if leaf.eax == 0 {
        return None;
    }

    Some((leaf.eax as u64) * 1_000_000)
}

#[cfg(target_arch = "x86_64")]
fn cpuid(leaf: u32) -> CpuidResult {
    core::arch::x86_64::__cpuid(leaf)
}
use core::arch::x86_64::CpuidResult;
