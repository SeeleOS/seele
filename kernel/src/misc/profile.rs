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
const PROFILE_CATEGORY_COUNT: usize = 27;
const HOT_SYSCALL_PHASE_COUNT: usize = 54;
const IOCTL_OP_COUNT: usize = 93;
const IOCTL_TARGET_COUNT: usize = 11;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum ProfileCategory {
    SchedulerWork = 0,
    TimerWork = 1,
    NetPoll = 2,
    SyscallCpu = 3,
    PageFault = 4,
    IrqTimer = 5,
    IrqKeyboard = 6,
    IrqMouse = 7,
    Idle = 8,
    OtherKernel = 9,
    SchedulerSelect = 10,
    SchedulerSwitch = 11,
    SchedulerDispatch = 12,
    SchedulerAfterYield = 13,
    ThreadRunWindow = 14,
    SyscallEntry = 15,
    SyscallBody = 16,
    SyscallExit = 17,
    PageFaultLookup = 18,
    PageFaultResolve = 19,
    PageFaultFileLazy = 20,
    PageFaultAnonLazy = 21,
    PageFaultCow = 22,
    PageFaultFileLazyCacheLookup = 23,
    PageFaultFileLazyCacheLoad = 24,
    PageFaultFileLazyMap = 25,
    PageFaultFileLazyCopy = 26,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum HotSyscallPhase {
    OpenAtPathResolve = 0,
    OpenAtInitialOpen = 1,
    OpenAtProcSelfFd = 2,
    OpenAtCreateFile = 3,
    OpenAtCreateReopen = 4,
    OpenAtInfo = 5,
    OpenAtNofollowCheck = 6,
    OpenAtDirectoryCheck = 7,
    OpenAtTruncate = 8,
    OpenAtSetFlags = 9,
    OpenAtInstallFd = 10,
    NewfstatatPathResolve = 11,
    NewfstatatEmptyPath = 12,
    NewfstatatResolveFinal = 13,
    NewfstatatBuildStat = 14,
    NewfstatatMountInfo = 15,
    NewfstatatWriteUser = 16,
    StatxPathResolve = 17,
    StatxEmptyPath = 18,
    StatxResolveFinal = 19,
    StatxBuildStat = 20,
    StatxMountInfo = 21,
    StatxPackOutput = 22,
    StatxWriteUser = 23,
    FsyncProcessLock = 24,
    FsyncFlushMappings = 25,
    FsyncCollectAreas = 26,
    FsyncWriteArea = 27,
    FsyncWritePage = 28,
    FsyncWriteFile = 29,
    ReadFileLike = 30,
    ReadReadable = 31,
    ReadCopyToUser = 32,
    ReadTryRead = 33,
    ReadBlockPrepare = 34,
    ReadBlockRetry = 35,
    TtyReadCopy = 36,
    ReadReadableTty = 37,
    ReadReadablePtySlave = 38,
    ReadReadableUnixSocket = 39,
    ReadReadableInetSocket = 40,
    ReadReadableNetlinkSocket = 41,
    ReadReadableFuseDevice = 42,
    ReadReadableOther = 43,
    ReadUnixDatagram = 44,
    ReadUnixSeqpacketPeek = 45,
    ReadUnixSeqpacketDrain = 46,
    ReadUnixStreamPeek = 47,
    ReadUnixStreamDrain = 48,
    ReadUnixWaitReadable = 49,
    ReadUnixWaitRegister = 50,
    ReadUnixWaitFastpath = 51,
    ReadUnixWaitPrepareBlock = 52,
    ReadUnixWaitRecheck = 53,
}

impl HotSyscallPhase {
    const ALL: [Self; HOT_SYSCALL_PHASE_COUNT] = [
        Self::OpenAtPathResolve,
        Self::OpenAtInitialOpen,
        Self::OpenAtProcSelfFd,
        Self::OpenAtCreateFile,
        Self::OpenAtCreateReopen,
        Self::OpenAtInfo,
        Self::OpenAtNofollowCheck,
        Self::OpenAtDirectoryCheck,
        Self::OpenAtTruncate,
        Self::OpenAtSetFlags,
        Self::OpenAtInstallFd,
        Self::NewfstatatPathResolve,
        Self::NewfstatatEmptyPath,
        Self::NewfstatatResolveFinal,
        Self::NewfstatatBuildStat,
        Self::NewfstatatMountInfo,
        Self::NewfstatatWriteUser,
        Self::StatxPathResolve,
        Self::StatxEmptyPath,
        Self::StatxResolveFinal,
        Self::StatxBuildStat,
        Self::StatxMountInfo,
        Self::StatxPackOutput,
        Self::StatxWriteUser,
        Self::FsyncProcessLock,
        Self::FsyncFlushMappings,
        Self::FsyncCollectAreas,
        Self::FsyncWriteArea,
        Self::FsyncWritePage,
        Self::FsyncWriteFile,
        Self::ReadFileLike,
        Self::ReadReadable,
        Self::ReadCopyToUser,
        Self::ReadTryRead,
        Self::ReadBlockPrepare,
        Self::ReadBlockRetry,
        Self::TtyReadCopy,
        Self::ReadReadableTty,
        Self::ReadReadablePtySlave,
        Self::ReadReadableUnixSocket,
        Self::ReadReadableInetSocket,
        Self::ReadReadableNetlinkSocket,
        Self::ReadReadableFuseDevice,
        Self::ReadReadableOther,
        Self::ReadUnixDatagram,
        Self::ReadUnixSeqpacketPeek,
        Self::ReadUnixSeqpacketDrain,
        Self::ReadUnixStreamPeek,
        Self::ReadUnixStreamDrain,
        Self::ReadUnixWaitReadable,
        Self::ReadUnixWaitRegister,
        Self::ReadUnixWaitFastpath,
        Self::ReadUnixWaitPrepareBlock,
        Self::ReadUnixWaitRecheck,
    ];

    fn as_index(self) -> usize {
        self as usize
    }

    fn name(self) -> &'static str {
        match self {
            Self::OpenAtPathResolve => "openat_resolve",
            Self::OpenAtInitialOpen => "openat_initial_open",
            Self::OpenAtProcSelfFd => "openat_proc_self_fd",
            Self::OpenAtCreateFile => "openat_create_file",
            Self::OpenAtCreateReopen => "openat_create_reopen",
            Self::OpenAtInfo => "openat_info",
            Self::OpenAtNofollowCheck => "openat_nofollow_check",
            Self::OpenAtDirectoryCheck => "openat_directory_check",
            Self::OpenAtTruncate => "openat_truncate",
            Self::OpenAtSetFlags => "openat_set_flags",
            Self::OpenAtInstallFd => "openat_install_fd",
            Self::NewfstatatPathResolve => "newfstatat_resolve",
            Self::NewfstatatEmptyPath => "newfstatat_empty_path",
            Self::NewfstatatResolveFinal => "newfstatat_resolve_final",
            Self::NewfstatatBuildStat => "newfstatat_build_stat",
            Self::NewfstatatMountInfo => "newfstatat_mount_info",
            Self::NewfstatatWriteUser => "newfstatat_write_user",
            Self::StatxPathResolve => "statx_resolve",
            Self::StatxEmptyPath => "statx_empty_path",
            Self::StatxResolveFinal => "statx_resolve_final",
            Self::StatxBuildStat => "statx_build_stat",
            Self::StatxMountInfo => "statx_mount_info",
            Self::StatxPackOutput => "statx_pack_output",
            Self::StatxWriteUser => "statx_write_user",
            Self::FsyncProcessLock => "fsync_process_lock",
            Self::FsyncFlushMappings => "fsync_flush_mappings",
            Self::FsyncCollectAreas => "fsync_collect_areas",
            Self::FsyncWriteArea => "fsync_write_area",
            Self::FsyncWritePage => "fsync_write_page",
            Self::FsyncWriteFile => "fsync_write_file",
            Self::ReadFileLike => "read_file_like",
            Self::ReadReadable => "read_readable",
            Self::ReadCopyToUser => "read_copy_to_user",
            Self::ReadTryRead => "read_try_read",
            Self::ReadBlockPrepare => "read_block_prepare",
            Self::ReadBlockRetry => "read_block_retry",
            Self::TtyReadCopy => "tty_read_copy",
            Self::ReadReadableTty => "read_tty",
            Self::ReadReadablePtySlave => "read_pty_slave",
            Self::ReadReadableUnixSocket => "read_unix_socket",
            Self::ReadReadableInetSocket => "read_inet_socket",
            Self::ReadReadableNetlinkSocket => "read_netlink_socket",
            Self::ReadReadableFuseDevice => "read_fuse_device",
            Self::ReadReadableOther => "read_other_readable",
            Self::ReadUnixDatagram => "read_unix_datagram",
            Self::ReadUnixSeqpacketPeek => "read_unix_seqpacket_peek",
            Self::ReadUnixSeqpacketDrain => "read_unix_seqpacket_drain",
            Self::ReadUnixStreamPeek => "read_unix_stream_peek",
            Self::ReadUnixStreamDrain => "read_unix_stream_drain",
            Self::ReadUnixWaitReadable => "read_unix_wait_readable",
            Self::ReadUnixWaitRegister => "read_unix_wait_register",
            Self::ReadUnixWaitFastpath => "read_unix_wait_fastpath",
            Self::ReadUnixWaitPrepareBlock => "read_unix_wait_prepare_block",
            Self::ReadUnixWaitRecheck => "read_unix_wait_recheck",
        }
    }
}

impl LinuxIoctlOp {
    const ALL: [Self; IOCTL_OP_COUNT] = [
        Self::FbGetVariableScreenInfo,
        Self::FbPutVariableScreenInfo,
        Self::FbGetFixedScreenInfo,
        Self::FbGetColorMap,
        Self::FbPutColorMap,
        Self::FbPanDisplay,
        Self::FbBlank,
        Self::LinuxTcGets,
        Self::LinuxTcSets,
        Self::LinuxTcFlush,
        Self::LinuxTcGets2,
        Self::LinuxTcSets2,
        Self::LinuxTiocnxcl,
        Self::LinuxTiocsctty,
        Self::LinuxTiocgPgrp,
        Self::LinuxTiocnotty,
        Self::LinuxTiocspgrp,
        Self::LinuxTiocoutq,
        Self::LinuxTiocgwinsz,
        Self::LinuxTiocswinsz,
        Self::LinuxTiocgptn,
        Self::LinuxTiocsptlck,
        Self::LinuxTiocgptpeer,
        Self::LinuxTiocvhangup,
        Self::LinuxKdGetKeyboardMode,
        Self::LinuxKdSetKeyboardMode,
        Self::LinuxKdGetKeyboardType,
        Self::LinuxKdGetKeyboardEntry,
        Self::LinuxKdGetDisplayMode,
        Self::LinuxKdSetDisplayMode,
        Self::LinuxKdSignalAccept,
        Self::LinuxVtOpenQuery,
        Self::LinuxVtGetMode,
        Self::LinuxVtGetState,
        Self::LinuxVtSetMode,
        Self::LinuxVtActivate,
        Self::LinuxVtWaitActive,
        Self::LinuxVtRelDisp,
        Self::DrmVersion,
        Self::DrmGetUnique,
        Self::DrmGetMagic,
        Self::DrmGetCap,
        Self::DrmWaitVblank,
        Self::DrmSetUnique,
        Self::DrmAuthMagic,
        Self::DrmSetClientCap,
        Self::DrmSetMaster,
        Self::DrmDropMaster,
        Self::DrmModeGetResources,
        Self::DrmModeGetCrtc,
        Self::DrmModeSetCrtc,
        Self::DrmModeCursor,
        Self::DrmModeCursor2,
        Self::DrmModeGetGamma,
        Self::DrmModeSetGamma,
        Self::DrmModeGetEncoder,
        Self::DrmModeGetConnector,
        Self::DrmModeGetProperty,
        Self::DrmModeObjGetProperties,
        Self::DrmModeGetPlaneResources,
        Self::DrmModeGetPlane,
        Self::DrmModeListLessees,
        Self::DrmModeAddFb,
        Self::DrmModeAddFb2,
        Self::DrmModeRemoveFb,
        Self::DrmModePageFlip,
        Self::DrmModeDirtyFb,
        Self::DrmModeCreateDumb,
        Self::DrmModeMapDumb,
        Self::DrmModeDestroyDumb,
        Self::DrmGemClose,
        Self::DrmPrimeHandleToFd,
        Self::DrmPrimeFdToHandle,
        Self::RawFionbio,
        Self::RawFioclex,
        Self::DmaBufSync,
        Self::DmaBufExportSyncFile,
        Self::DmaBufImportSyncFile,
        Self::EvdevGetVersion,
        Self::EvdevGetId,
        Self::EvdevGetRepeat,
        Self::EvdevGetName,
        Self::EvdevGetPhys,
        Self::EvdevGetUniq,
        Self::EvdevGetProp,
        Self::EvdevGetKey,
        Self::EvdevGetLed,
        Self::EvdevGetSnd,
        Self::EvdevGetSw,
        Self::EvdevGetBit,
        Self::EvdevGrab,
        Self::EvdevRevoke,
        Self::EvdevSetClockId,
    ];

    fn as_index(self) -> usize {
        self as usize
    }
}

impl LinuxIoctlTarget {
    const ALL: [Self; IOCTL_TARGET_COUNT] = [
        Self::Framebuffer,
        Self::Terminal,
        Self::TtyDevice,
        Self::PtyMaster,
        Self::PtySlave,
        Self::DrmCard,
        Self::DrmPrime,
        Self::EvdevClient,
        Self::UnixSocket,
        Self::InetSocket,
        Self::NetlinkSocket,
    ];

    fn as_index(self) -> usize {
        self as usize
    }
}

impl ProfileCategory {
    const ALL: [Self; PROFILE_CATEGORY_COUNT] = [
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
        Self::SchedulerSelect,
        Self::SchedulerSwitch,
        Self::SchedulerDispatch,
        Self::SchedulerAfterYield,
        Self::ThreadRunWindow,
        Self::SyscallEntry,
        Self::SyscallBody,
        Self::SyscallExit,
        Self::PageFaultLookup,
        Self::PageFaultResolve,
        Self::PageFaultFileLazy,
        Self::PageFaultAnonLazy,
        Self::PageFaultCow,
        Self::PageFaultFileLazyCacheLookup,
        Self::PageFaultFileLazyCacheLoad,
        Self::PageFaultFileLazyMap,
        Self::PageFaultFileLazyCopy,
    ];

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

    fn as_index(self) -> usize {
        self as usize
    }

    fn name(self) -> &'static str {
        match self {
            Self::SchedulerWork => "scheduler_work",
            Self::TimerWork => "timer_work",
            Self::NetPoll => "net_poll",
            Self::SyscallCpu => "syscall_cpu",
            Self::PageFault => "page_fault",
            Self::IrqTimer => "irq_timer",
            Self::IrqKeyboard => "irq_keyboard",
            Self::IrqMouse => "irq_mouse",
            Self::Idle => "idle",
            Self::OtherKernel => "other_kernel",
            Self::SchedulerSelect => "sched_select",
            Self::SchedulerSwitch => "sched_switch",
            Self::SchedulerDispatch => "sched_dispatch",
            Self::SchedulerAfterYield => "sched_after_yield",
            Self::ThreadRunWindow => "thread_run_window",
            Self::SyscallEntry => "syscall_entry",
            Self::SyscallBody => "syscall_body",
            Self::SyscallExit => "syscall_exit",
            Self::PageFaultLookup => "pf_lookup",
            Self::PageFaultResolve => "pf_resolve",
            Self::PageFaultFileLazy => "pf_file_lazy",
            Self::PageFaultAnonLazy => "pf_anon_lazy",
            Self::PageFaultCow => "pf_cow",
            Self::PageFaultFileLazyCacheLookup => "pf_file_cache_lookup",
            Self::PageFaultFileLazyCacheLoad => "pf_file_cache_load",
            Self::PageFaultFileLazyMap => "pf_file_map",
            Self::PageFaultFileLazyCopy => "pf_file_copy",
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
    cpu_data: Vec<CpuProfileData>,
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

        for op in LinuxIoctlOp::ALL {
            ioctl_op_totals[op.as_index()] += cpu.ioctl_ops[op.as_index()].snapshot_and_reset();
        }

        for target in LinuxIoctlTarget::ALL {
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
        .into_iter()
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
        .into_iter()
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
