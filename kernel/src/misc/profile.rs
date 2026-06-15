use alloc::{format, string::String, vec::Vec};
use core::{
    array,
    cmp::Reverse,
    sync::atomic::{AtomicU64, Ordering},
};

use crate::{
    memory::utils::Mut,
    misc::{
        get_cycles, serial_print::SERIAL_PORT, time::NANOSECONDS_PER_SECOND, time::Time,
        time::tsc_frequency_hz,
    },
    object::linux_ioctl::{LinuxIoctlOp, LinuxIoctlTarget},
    s_println,
    smp::{current_cpu_index, topology},
    thread::thread::Thread,
};
use conquer_once::spin::OnceCell;

const REPORT_INTERVAL_SECONDS: u64 = 5;
const TOP_CATEGORY_COUNT: usize = 10;
const TOP_SYSCALL_COUNT: usize = 8;
const TOP_IOCTL_COUNT: usize = 8;
const MAX_SYSCALLS: usize = 1500;
const PROFILE_CATEGORY_COUNT: usize = ProfileCategory::COUNT;
const HOT_SYSCALL_PHASE_COUNT: usize = HotSyscallPhase::COUNT;
const IOCTL_OP_COUNT: usize = LinuxIoctlOp::COUNT;
const IOCTL_TARGET_COUNT: usize = LinuxIoctlTarget::COUNT;

macro_rules! define_profile_enum {
    (
        $vis:vis enum $name:ident {
            $($variant:ident => $label:literal),+ $(,)?
        }
    ) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        #[repr(u8)]
        $vis enum $name {
            $($variant,)+
        }

        impl $name {
            const COUNT: usize = <[()]>::len(&[$(define_profile_enum!(@unit $variant)),+]);
            const ALL: [Self; Self::COUNT] = [$(Self::$variant,)+];

            fn as_index(self) -> usize {
                self as usize
            }

            fn name(self) -> &'static str {
                match self {
                    $(Self::$variant => $label,)+
                }
            }
        }
    };
    (@unit $variant:ident) => {
        ()
    };
}

define_profile_enum! {
    pub enum ProfileCategory {
        SchedulerWork => "scheduler_work",
        TimerWork => "timer_work",
        NetPoll => "net_poll",
        SyscallCpu => "syscall_cpu",
        PageFault => "page_fault",
        IrqTimer => "irq_timer",
        IrqKeyboard => "irq_keyboard",
        IrqMouse => "irq_mouse",
        Idle => "idle",
        OtherKernel => "other_kernel",
        SchedulerSelect => "sched_select",
        SchedulerSwitch => "sched_switch",
        SchedulerDispatch => "sched_dispatch",
        SchedulerAfterYield => "sched_after_yield",
        ThreadRunWindow => "thread_run_window",
        SyscallEntry => "syscall_entry",
        SyscallBody => "syscall_body",
        SyscallExit => "syscall_exit",
        PageFaultLookup => "pf_lookup",
        PageFaultResolve => "pf_resolve",
        PageFaultFileLazy => "pf_file_lazy",
        PageFaultAnonLazy => "pf_anon_lazy",
        PageFaultCow => "pf_cow",
        PageFaultFileLazyCacheLookup => "pf_file_cache_lookup",
        PageFaultFileLazyCacheLoad => "pf_file_cache_load",
        PageFaultFileLazyMap => "pf_file_map",
        PageFaultFileLazyCopy => "pf_file_copy",
    }
}

impl ProfileCategory {
    const PRIMARY: [Self; 10] = [
        Self::SchedulerWork,
        Self::TimerWork,
        Self::NetPoll,
        Self::SyscallCpu,
        Self::PageFault,
        Self::IrqTimer,
        Self::IrqKeyboard,
        Self::IrqMouse,
        Self::Idle,
        Self::OtherKernel,
    ];
}

define_profile_enum! {
    pub enum HotSyscallPhase {
        OpenAtPathResolve => "openat_resolve",
        OpenAtInitialOpen => "openat_initial_open",
        OpenAtInitialOpenVfs => "openat_open_vfs",
        OpenAtInitialOpenObject => "openat_open_object",
        OpenAtInitialOpenStat => "openat_open_stat",
        OpenAtProcSelfFd => "openat_proc_self_fd",
        OpenAtCreateFile => "openat_create_file",
        OpenAtCreateReopen => "openat_create_reopen",
        OpenAtInfo => "openat_info",
        OpenAtNofollowCheck => "openat_nofollow_check",
        OpenAtDirectoryCheck => "openat_directory_check",
        OpenAtTruncate => "openat_truncate",
        OpenAtSetFlags => "openat_set_flags",
        OpenAtInstallFd => "openat_install_fd",
        MkdirPathResolve => "mkdir_resolve",
        MkdirCreateDir => "mkdir_create_dir",
        MkdirApplyMode => "mkdir_apply_mode",
        NewfstatatPathResolve => "newfstatat_resolve",
        NewfstatatEmptyPath => "newfstatat_empty_path",
        NewfstatatResolveFinal => "newfstatat_resolve_final",
        NewfstatatBuildStat => "newfstatat_build_stat",
        NewfstatatMountInfo => "newfstatat_mount_info",
        NewfstatatWriteUser => "newfstatat_write_user",
        StatxPathResolve => "statx_resolve",
        StatxEmptyPath => "statx_empty_path",
        StatxResolveFinal => "statx_resolve_final",
        StatxBuildStat => "statx_build_stat",
        StatxMountInfo => "statx_mount_info",
        StatxPackOutput => "statx_pack_output",
        StatxWriteUser => "statx_write_user",
        FsyncProcessLock => "fsync_process_lock",
        FsyncFlushMappings => "fsync_flush_mappings",
        FsyncCollectAreas => "fsync_collect_areas",
        FsyncWriteArea => "fsync_write_area",
        FsyncWritePage => "fsync_write_page",
        FsyncWriteFile => "fsync_write_file",
        ReadFileLike => "read_file_like",
        ReadReadable => "read_readable",
        ReadCopyToUser => "read_copy_to_user",
        ReadTryRead => "read_try_read",
        ReadBlockPrepare => "read_block_prepare",
        ReadBlockRetry => "read_block_retry",
        TtyReadCopy => "tty_read_copy",
        ReadReadableTty => "read_tty",
        ReadReadablePtySlave => "read_pty_slave",
        ReadReadableUnixSocket => "read_unix_socket",
        ReadReadableInetSocket => "read_inet_socket",
        ReadReadableNetlinkSocket => "read_netlink_socket",
        ReadReadableFuseDevice => "read_fuse_device",
        ReadReadableOther => "read_other_readable",
        ReadUnixDatagram => "read_unix_datagram",
        ReadUnixSeqpacketPeek => "read_unix_seqpacket_peek",
        ReadUnixSeqpacketDrain => "read_unix_seqpacket_drain",
        ReadUnixStreamPeek => "read_unix_stream_peek",
        ReadUnixStreamDrain => "read_unix_stream_drain",
        ReadUnixWaitReadable => "read_unix_wait_readable",
        ReadUnixWaitRegister => "read_unix_wait_register",
        ReadUnixWaitFastpath => "read_unix_wait_fastpath",
        ReadUnixWaitPrepareBlock => "read_unix_wait_prepare_block",
        ReadUnixWaitRecheck => "read_unix_wait_recheck",
        PfFileCopyCacheRead => "pf_file_copy_cache_read",
        PfFileCopyMemcpy => "pf_file_copy_memcpy",
        PfFileCopyZeroFill => "pf_file_copy_zero_fill",
        PfFileCopyFrameMap => "pf_file_copy_frame_map",
        PfFileCopyClusterMap => "pf_file_copy_cluster_map",
        PfFileCopySharedRef => "pf_file_copy_shared_ref",
        PfFileCopyCacheInsert => "pf_file_copy_cache_insert",
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
    cache_lookup_cycles: AtomicU64,
    cache_load_cycles: AtomicU64,
    map_cycles: AtomicU64,
    copy_cycles: AtomicU64,
}

impl FileLazyFaultCounters {
    fn record(&self, stats: FileLazyFaultRecord) {
        self.faults.fetch_add(1, Ordering::Relaxed);
        self.cluster_pages_loaded
            .fetch_add(stats.cluster_pages_loaded, Ordering::Relaxed);
        self.cache_hits
            .fetch_add(stats.cache_hits, Ordering::Relaxed);
        self.cache_misses
            .fetch_add(stats.cache_misses, Ordering::Relaxed);
        self.cache_lookup_cycles
            .fetch_add(stats.cache_lookup_cycles, Ordering::Relaxed);
        self.cache_load_cycles
            .fetch_add(stats.cache_load_cycles, Ordering::Relaxed);
        self.map_cycles
            .fetch_add(stats.map_cycles, Ordering::Relaxed);
        self.copy_cycles
            .fetch_add(stats.copy_cycles, Ordering::Relaxed);
    }

    fn snapshot_and_reset(&self) -> FileLazyFaultSnapshot {
        FileLazyFaultSnapshot {
            faults: self.faults.swap(0, Ordering::AcqRel),
            cluster_pages_loaded: self.cluster_pages_loaded.swap(0, Ordering::AcqRel),
            cache_hits: self.cache_hits.swap(0, Ordering::AcqRel),
            cache_misses: self.cache_misses.swap(0, Ordering::AcqRel),
            cache_lookup_cycles: self.cache_lookup_cycles.swap(0, Ordering::AcqRel),
            cache_load_cycles: self.cache_load_cycles.swap(0, Ordering::AcqRel),
            map_cycles: self.map_cycles.swap(0, Ordering::AcqRel),
            copy_cycles: self.copy_cycles.swap(0, Ordering::AcqRel),
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct FileLazyFaultRecord {
    pub cluster_pages_loaded: u64,
    pub cache_hits: u64,
    pub cache_misses: u64,
    pub cache_lookup_cycles: u64,
    pub cache_load_cycles: u64,
    pub map_cycles: u64,
    pub copy_cycles: u64,
}

#[derive(Clone, Copy, Debug, Default)]
struct FileLazyFaultSnapshot {
    faults: u64,
    cluster_pages_loaded: u64,
    cache_hits: u64,
    cache_misses: u64,
    cache_lookup_cycles: u64,
    cache_load_cycles: u64,
    map_cycles: u64,
    copy_cycles: u64,
}

impl core::ops::AddAssign for FileLazyFaultSnapshot {
    fn add_assign(&mut self, rhs: Self) {
        self.faults = self.faults.saturating_add(rhs.faults);
        self.cluster_pages_loaded = self
            .cluster_pages_loaded
            .saturating_add(rhs.cluster_pages_loaded);
        self.cache_hits = self.cache_hits.saturating_add(rhs.cache_hits);
        self.cache_misses = self.cache_misses.saturating_add(rhs.cache_misses);
        self.cache_lookup_cycles = self
            .cache_lookup_cycles
            .saturating_add(rhs.cache_lookup_cycles);
        self.cache_load_cycles = self.cache_load_cycles.saturating_add(rhs.cache_load_cycles);
        self.map_cycles = self.map_cycles.saturating_add(rhs.map_cycles);
        self.copy_cycles = self.copy_cycles.saturating_add(rhs.copy_cycles);
    }
}

struct CpuProfileData {
    categories: [ProfileCounters; PROFILE_CATEGORY_COUNT],
    syscall_cpu: [ProfileCounters; MAX_SYSCALLS],
    syscall_blocked: [ProfileCounters; MAX_SYSCALLS],
    ioctl_ops: [ProfileCounters; IOCTL_OP_COUNT],
    ioctl_targets: [ProfileCounters; IOCTL_TARGET_COUNT],
    hot_syscall_phases: [ProfileCounters; HOT_SYSCALL_PHASE_COUNT],
    file_lazy_faults: FileLazyFaultCounters,
}

impl Default for CpuProfileData {
    fn default() -> Self {
        Self {
            categories: array::from_fn(|_| ProfileCounters::default()),
            syscall_cpu: array::from_fn(|_| ProfileCounters::default()),
            syscall_blocked: array::from_fn(|_| ProfileCounters::default()),
            ioctl_ops: array::from_fn(|_| ProfileCounters::default()),
            ioctl_targets: array::from_fn(|_| ProfileCounters::default()),
            hot_syscall_phases: array::from_fn(|_| ProfileCounters::default()),
            file_lazy_faults: FileLazyFaultCounters::default(),
        }
    }
}

struct ProfileState {
    cpu_data: Mut<Vec<CpuProfileData>>,
    last_report_ns: AtomicU64,
}

static PROFILE_STATE: OnceCell<ProfileState> = OnceCell::uninit();
static TIMER_INTERRUPTS: AtomicU64 = AtomicU64::new(0);
static TIMER_PREEMPTIONS: AtomicU64 = AtomicU64::new(0);
static RESCHED_IPI_WAKEUPS: AtomicU64 = AtomicU64::new(0);

pub fn init() {
    let cpu_count = topology::processors().len().max(1);
    PROFILE_STATE
        .try_init_once(|| ProfileState {
            cpu_data: Mut::new((0..cpu_count).map(|_| CpuProfileData::default()).collect()),
            last_report_ns: AtomicU64::new(Time::since_boot().as_nanoseconds()),
        })
        .expect("profiling initialized twice");
}

pub fn ensure_cpu_slots(cpu_count: usize) {
    let state = PROFILE_STATE.get().expect("profiling not initialized");
    state
        .cpu_data
        .lock()
        .resize_with(cpu_count.max(1), CpuProfileData::default);
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

pub fn record_hot_syscall_phase(phase: HotSyscallPhase, cycles: u64) {
    if cycles == 0 {
        return;
    }

    with_cpu_profile_data(|cpu| cpu.hot_syscall_phases[phase.as_index()].record(cycles));
}

pub fn record_ioctl_op(op: LinuxIoctlOp, cycles: u64) {
    if cycles == 0 {
        return;
    }

    with_cpu_profile_data(|cpu| cpu.ioctl_ops[op.as_index()].record(cycles));
}

pub fn record_ioctl_target(target: LinuxIoctlTarget, cycles: u64) {
    if cycles == 0 {
        return;
    }

    with_cpu_profile_data(|cpu| cpu.ioctl_targets[target.as_index()].record(cycles));
}

pub fn increment_timer_interrupts() {
    TIMER_INTERRUPTS.fetch_add(1, Ordering::Relaxed);
}

pub fn increment_timer_preemptions() {
    TIMER_PREEMPTIONS.fetch_add(1, Ordering::Relaxed);
}

pub fn increment_resched_ipi_wakeups() {
    RESCHED_IPI_WAKEUPS.fetch_add(1, Ordering::Relaxed);
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

pub fn record_file_lazy_fault(stats: FileLazyFaultRecord) {
    with_cpu_profile_data(|cpu| {
        cpu.file_lazy_faults.record(stats);
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
    let mut ioctl_op_totals = [ProfileSnapshot::default(); IOCTL_OP_COUNT];
    let mut ioctl_target_totals = [ProfileSnapshot::default(); IOCTL_TARGET_COUNT];
    let mut hot_syscall_phase_totals = [ProfileSnapshot::default(); HOT_SYSCALL_PHASE_COUNT];
    let mut file_lazy_faults = FileLazyFaultSnapshot::default();

    let cpu_data = state.cpu_data.lock();
    for cpu in cpu_data.iter() {
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

        for op in LinuxIoctlOp::ALL.iter().copied() {
            ioctl_op_totals[op.as_index()] += cpu.ioctl_ops[op.as_index()].snapshot_and_reset();
        }

        for target in LinuxIoctlTarget::ALL.iter().copied() {
            ioctl_target_totals[target.as_index()] +=
                cpu.ioctl_targets[target.as_index()].snapshot_and_reset();
        }

        for phase in HotSyscallPhase::ALL {
            hot_syscall_phase_totals[phase.as_index()] +=
                cpu.hot_syscall_phases[phase.as_index()].snapshot_and_reset();
        }

        file_lazy_faults += cpu.file_lazy_faults.snapshot_and_reset();
    }

    let total_cycles: u64 = ProfileCategory::PRIMARY
        .iter()
        .map(|category| category_totals[category.as_index()].total_cycles)
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
    let timer_interrupts = TIMER_INTERRUPTS.swap(0, Ordering::AcqRel);
    let timer_preemptions = TIMER_PREEMPTIONS.swap(0, Ordering::AcqRel);
    let resched_ipi_wakeups = RESCHED_IPI_WAKEUPS.swap(0, Ordering::AcqRel);

    s_println!(
        "PROFILE {}s window_ms={} cpus={} cpu_time_ms={} blocked_time_ms={}",
        REPORT_INTERVAL_SECONDS,
        window_ms,
        cpu_count,
        cycles_to_milliseconds(total_cycles, tsc_hz),
        cycles_to_milliseconds(total_blocked_cycles, tsc_hz)
    );
    s_println!(
        "SCHED_EVENTS timer_interrupts={} timer_preemptions={} resched_ipi_wakeups={}",
        timer_interrupts,
        timer_preemptions,
        resched_ipi_wakeups
    );
    s_println!("CATEGORIES");

    let mut top_categories: Vec<(ProfileCategory, ProfileSnapshot)> = ProfileCategory::PRIMARY
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
    report_ioctl_ops(&ioctl_op_totals, tsc_hz);
    report_ioctl_targets(&ioctl_target_totals, tsc_hz);
    report_hot_syscall_phases(&hot_syscall_phase_totals, tsc_hz);

    report_category_breakdown(
        "SCHEDULER_BREAKDOWN",
        &category_totals,
        tsc_hz,
        &[
            ProfileCategory::SchedulerWork,
            ProfileCategory::SchedulerSelect,
            ProfileCategory::SchedulerSwitch,
            ProfileCategory::SchedulerDispatch,
            ProfileCategory::SchedulerAfterYield,
            ProfileCategory::ThreadRunWindow,
        ],
    );
    report_category_breakdown(
        "SYSCALL_BREAKDOWN",
        &category_totals,
        tsc_hz,
        &[
            ProfileCategory::SyscallCpu,
            ProfileCategory::SyscallEntry,
            ProfileCategory::SyscallBody,
            ProfileCategory::SyscallExit,
        ],
    );
    report_category_breakdown(
        "PAGE_FAULT_BREAKDOWN",
        &category_totals,
        tsc_hz,
        &[
            ProfileCategory::PageFault,
            ProfileCategory::PageFaultLookup,
            ProfileCategory::PageFaultResolve,
            ProfileCategory::PageFaultFileLazy,
            ProfileCategory::PageFaultAnonLazy,
            ProfileCategory::PageFaultCow,
        ],
    );
    report_category_breakdown(
        "FILE_LAZY_BREAKDOWN",
        &category_totals,
        tsc_hz,
        &[
            ProfileCategory::PageFaultFileLazy,
            ProfileCategory::PageFaultFileLazyCacheLookup,
            ProfileCategory::PageFaultFileLazyCacheLoad,
            ProfileCategory::PageFaultFileLazyMap,
            ProfileCategory::PageFaultFileLazyCopy,
        ],
    );

    if file_lazy_faults.faults != 0 {
        s_println!(
            "FILE_LAZY_FAULTS faults={} cluster_pages_loaded={} page_cache_hits={} page_cache_misses={} cache_lookup_ms={} cache_load_ms={} copy_ms={} map_ms={}",
            file_lazy_faults.faults,
            file_lazy_faults.cluster_pages_loaded,
            file_lazy_faults.cache_hits,
            file_lazy_faults.cache_misses,
            cycles_to_milliseconds(file_lazy_faults.cache_lookup_cycles, tsc_hz),
            cycles_to_milliseconds(file_lazy_faults.cache_load_cycles, tsc_hz),
            cycles_to_milliseconds(file_lazy_faults.copy_cycles, tsc_hz),
            cycles_to_milliseconds(file_lazy_faults.map_cycles, tsc_hz),
        );
    }
}

fn report_category_breakdown(
    title: &str,
    totals: &[ProfileSnapshot; PROFILE_CATEGORY_COUNT],
    tsc_hz: u64,
    categories: &[ProfileCategory],
) {
    let entries: Vec<(ProfileCategory, ProfileSnapshot)> = categories
        .iter()
        .copied()
        .map(|category| (category, totals[category.as_index()]))
        .filter(|(_, snapshot)| snapshot.calls != 0 || snapshot.total_cycles != 0)
        .collect();
    if entries.is_empty() {
        return;
    }

    s_println!("{title}");
    for (category, snapshot) in entries {
        s_println!(
            "  {:<20} calls={} cpu_time_ms={} max_cpu_us={}",
            category.name(),
            snapshot.calls,
            cycles_to_milliseconds(snapshot.total_cycles, tsc_hz),
            cycles_to_microseconds(snapshot.max_cycles, tsc_hz),
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

fn report_hot_syscall_phases(totals: &[ProfileSnapshot; HOT_SYSCALL_PHASE_COUNT], tsc_hz: u64) {
    let entries: Vec<(HotSyscallPhase, ProfileSnapshot)> = HotSyscallPhase::ALL
        .into_iter()
        .map(|phase| (phase, totals[phase.as_index()]))
        .filter(|(_, snapshot)| snapshot.calls != 0 || snapshot.total_cycles != 0)
        .collect();
    if entries.is_empty() {
        return;
    }

    s_println!("HOT SYSCALL PHASES");
    for (phase, snapshot) in entries {
        s_println!(
            "  {:<22} calls={} cpu_time_ms={} max_cpu_us={}",
            phase.name(),
            snapshot.calls,
            cycles_to_milliseconds(snapshot.total_cycles, tsc_hz),
            cycles_to_microseconds(snapshot.max_cycles, tsc_hz),
        );
    }
}

fn report_ioctl_ops(totals: &[ProfileSnapshot; IOCTL_OP_COUNT], tsc_hz: u64) {
    let mut entries: Vec<(LinuxIoctlOp, ProfileSnapshot)> = LinuxIoctlOp::ALL
        .iter()
        .copied()
        .map(|op| (op, totals[op.as_index()]))
        .filter(|(_, snapshot)| snapshot.calls != 0)
        .collect();
    if entries.is_empty() {
        return;
    }

    entries.sort_by_key(|entry| Reverse(entry.1.total_cycles));
    s_println!("TOP IOCTL REQUESTS");
    for (op, snapshot) in entries.into_iter().take(TOP_IOCTL_COUNT) {
        s_println!(
            "  {:<22} calls={} cpu_time_ms={} max_cpu_us={}",
            format!("{op:?}"),
            snapshot.calls,
            cycles_to_milliseconds(snapshot.total_cycles, tsc_hz),
            cycles_to_microseconds(snapshot.max_cycles, tsc_hz),
        );
    }
}

fn report_ioctl_targets(totals: &[ProfileSnapshot; IOCTL_TARGET_COUNT], tsc_hz: u64) {
    let mut entries: Vec<(LinuxIoctlTarget, ProfileSnapshot)> = LinuxIoctlTarget::ALL
        .iter()
        .copied()
        .map(|target| (target, totals[target.as_index()]))
        .filter(|(_, snapshot)| snapshot.calls != 0)
        .collect();
    if entries.is_empty() {
        return;
    }

    entries.sort_by_key(|entry| Reverse(entry.1.total_cycles));
    s_println!("TOP IOCTL TARGETS");
    for (target, snapshot) in entries.into_iter().take(TOP_IOCTL_COUNT) {
        s_println!(
            "  {:<22} calls={} cpu_time_ms={} max_cpu_us={}",
            format!("{target:?}"),
            snapshot.calls,
            cycles_to_milliseconds(snapshot.total_cycles, tsc_hz),
            cycles_to_microseconds(snapshot.max_cycles, tsc_hz),
        );
    }
}

fn with_cpu_profile_data<R>(f: impl FnOnce(&CpuProfileData) -> R) -> R {
    let state = PROFILE_STATE.get().expect("profiling not initialized");
    let index = current_cpu_index();
    let cpu_data = state.cpu_data.lock();
    let cpu = cpu_data
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
