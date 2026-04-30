use core::{
    hint::spin_loop,
    sync::atomic::{AtomicI32, AtomicI64, AtomicU64, Ordering},
};

use crate::misc::get_cycles;
use x86_rtc::Rtc;
use x86_64::instructions::port::Port;

static BOOT_TSC: AtomicU64 = AtomicU64::new(0);
static TSC_FREQ_HZ: AtomicU64 = AtomicU64::new(0);
static REALTIME_BASE_NS: AtomicI64 = AtomicI64::new(0);
static TIMEZONE_MINUTESWEST: AtomicI32 = AtomicI32::new(0);
static TIMEZONE_DSTTIME: AtomicI32 = AtomicI32::new(0);

pub const NANOSECONDS_PER_MICROSECOND: u64 = 1_000;
pub const NANOSECONDS_PER_MILLISECOND: u64 = 1_000_000;
pub const NANOSECONDS_PER_SECOND: u64 = 1_000_000_000;
const DEFAULT_TSC_FREQ_HZ: u64 = 1_000_000_000;
const MIN_TSC_FREQ_HZ: u64 = 1_000_000;
const MAX_TSC_FREQ_HZ: u64 = 10_000_000_000;
const PIT_FREQUENCY_HZ: u64 = 1_193_182;
const PIT_CALIBRATION_MS: u64 = 10;
const PROFILING: bool = true;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Default)]
pub struct Time(pub u64);

struct TimeCalibration {
    boot_tsc: u64,
    tsc_freq_hz: u64,
    realtime_base_ns: i64,
}

pub fn init() {
    let rtc = Rtc::new();
    let calibration = detect_tsc_frequency_hz()
        .map(|tsc_freq_hz| TimeCalibration {
            boot_tsc: get_cycles(),
            tsc_freq_hz,
            realtime_base_ns: (rtc.get_unix_timestamp() as i64)
                .saturating_mul(NANOSECONDS_PER_SECOND as i64),
        })
        .or_else(|| calibrate_timebase(&rtc))
        .unwrap_or_else(|| TimeCalibration {
            boot_tsc: get_cycles(),
            tsc_freq_hz: DEFAULT_TSC_FREQ_HZ,
            realtime_base_ns: (rtc.get_unix_timestamp() as i64)
                .saturating_mul(NANOSECONDS_PER_SECOND as i64),
        });

    BOOT_TSC.store(calibration.boot_tsc, Ordering::SeqCst);
    TSC_FREQ_HZ.store(calibration.tsc_freq_hz, Ordering::SeqCst);
    REALTIME_BASE_NS.store(calibration.realtime_base_ns, Ordering::SeqCst);
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

pub fn profile_boot_stage<T, F>(label: &str, f: F) -> T
where
    F: FnOnce() -> T,
{
    if !PROFILING {
        return f();
    }

    let start_cycles = get_cycles();
    let start_time = Time::since_boot();
    let has_timebase = TSC_FREQ_HZ.load(Ordering::SeqCst) != 0;

    if has_timebase {
        crate::s_println!(
            "[profile] start {} at {}.{:03}s",
            label,
            start_time.as_seconds(),
            start_time.subsec_milliseconds()
        );
    } else {
        crate::s_println!("[profile] start {} at cycle {}", label, start_cycles);
    }

    let result = f();

    let end_cycles = get_cycles();
    let elapsed_cycles = end_cycles.saturating_sub(start_cycles);

    if has_timebase {
        let end_time = Time::since_boot();
        let elapsed = end_time.sub(start_time);
        crate::s_println!(
            "[profile] end {} at {}.{:03}s (+{} ms, {} cycles)",
            label,
            end_time.as_seconds(),
            end_time.subsec_milliseconds(),
            elapsed.as_milliseconds(),
            elapsed_cycles
        );
    } else {
        crate::s_println!(
            "[profile] end {} at cycle {} (+{} cycles)",
            label,
            end_cycles,
            elapsed_cycles
        );
    }

    result
}

fn detect_tsc_frequency_hz() -> Option<u64> {
    detect_tsc_frequency_from_kvm_leaf_0x40000010()
        .or_else(detect_tsc_frequency_from_leaf_0x15)
        .or_else(detect_tsc_frequency_from_leaf_0x16)
}

fn calibrate_timebase(rtc: &Rtc) -> Option<TimeCalibration> {
    calibrate_timebase_with_pit(rtc).or_else(|| calibrate_timebase_with_rtc(rtc))
}

fn calibrate_timebase_with_pit(rtc: &Rtc) -> Option<TimeCalibration> {
    let divisor = (PIT_FREQUENCY_HZ * PIT_CALIBRATION_MS / 1_000) as u16;
    let realtime_base_ns =
        (rtc.get_unix_timestamp() as i64).saturating_mul(NANOSECONDS_PER_SECOND as i64);
    let boot_tsc = get_cycles();
    let elapsed_cycles = measure_with_pit_channel_2(divisor)?;
    let tsc_freq_hz = (elapsed_cycles as u128)
        .checked_mul(1_000)?
        .checked_div(PIT_CALIBRATION_MS as u128)? as u64;

    if !(MIN_TSC_FREQ_HZ..=MAX_TSC_FREQ_HZ).contains(&tsc_freq_hz) {
        return None;
    }

    Some(TimeCalibration {
        boot_tsc,
        tsc_freq_hz,
        realtime_base_ns,
    })
}

fn calibrate_timebase_with_rtc(rtc: &Rtc) -> Option<TimeCalibration> {
    let (boot_second, boot_tsc) = wait_for_next_rtc_second(rtc, rtc.get_unix_timestamp())?;
    let (next_second, next_tsc) = wait_for_next_rtc_second(rtc, boot_second)?;
    let elapsed_seconds = next_second.checked_sub(boot_second)?;
    let elapsed_cycles = next_tsc.checked_sub(boot_tsc)?;
    let tsc_freq_hz = ((elapsed_cycles as u128) / (elapsed_seconds as u128)) as u64;

    if !(MIN_TSC_FREQ_HZ..=MAX_TSC_FREQ_HZ).contains(&tsc_freq_hz) {
        return None;
    }

    Some(TimeCalibration {
        boot_tsc,
        tsc_freq_hz,
        realtime_base_ns: (boot_second as i64).saturating_mul(NANOSECONDS_PER_SECOND as i64),
    })
}

fn measure_with_pit_channel_2(divisor: u16) -> Option<u64> {
    if divisor == 0 {
        return None;
    }

    let mut command_port = Port::<u8>::new(0x43);
    let mut channel_2_port = Port::<u8>::new(0x42);
    let mut speaker_port = Port::<u8>::new(0x61);

    unsafe {
        let original = speaker_port.read();
        speaker_port.write((original & !0x02) | 0x01);
        command_port.write(0xb0);
        channel_2_port.write((divisor & 0x00ff) as u8);
        channel_2_port.write((divisor >> 8) as u8);

        while speaker_port.read() & 0x20 != 0 {
            spin_loop();
        }

        let start = get_cycles();
        while speaker_port.read() & 0x20 == 0 {
            spin_loop();
        }
        let end = get_cycles();
        speaker_port.write(original);

        Some(end.saturating_sub(start))
    }
}

fn wait_for_next_rtc_second(rtc: &Rtc, second: u64) -> Option<(u64, u64)> {
    loop {
        let current_second = rtc.get_unix_timestamp();
        if current_second > second {
            return Some((current_second, get_cycles()));
        }
        spin_loop();
    }
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

fn detect_tsc_frequency_from_kvm_leaf_0x40000010() -> Option<u64> {
    let hypervisor_leaf = cpuid(0x4000_0000);
    if hypervisor_leaf.eax < 0x4000_0010 {
        return None;
    }

    if hypervisor_leaf.ebx != u32::from_le_bytes(*b"KVMK")
        || hypervisor_leaf.ecx != u32::from_le_bytes(*b"VMKV")
        || hypervisor_leaf.edx != u32::from_le_bytes(*b"M\0\0\0")
    {
        return None;
    }

    let timing = cpuid(0x4000_0010);
    if timing.eax == 0 {
        return None;
    }

    Some((timing.eax as u64) * 1_000)
}

#[cfg(target_arch = "x86_64")]
fn cpuid(leaf: u32) -> CpuidResult {
    core::arch::x86_64::__cpuid(leaf)
}
use core::arch::x86_64::CpuidResult;
