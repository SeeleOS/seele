use crate::{
    object::linux_ioctl::{LinuxIoctlOp, LinuxIoctlTarget},
    thread::thread::Thread,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum ProfileCategory {
    SchedulerWork,
    TimerWork,
    NetPoll,
    SyscallCpu,
    PageFault,
    IrqTimer,
    IrqKeyboard,
    IrqMouse,
    Idle,
    OtherKernel,
    SchedulerSelect,
    SchedulerSwitch,
    SchedulerDispatch,
    SchedulerAfterYield,
    ThreadRunWindow,
    SyscallEntry,
    SyscallBody,
    SyscallExit,
    PageFaultLookup,
    PageFaultResolve,
    PageFaultFileLazy,
    PageFaultAnonLazy,
    PageFaultCow,
    PageFaultFileLazyCacheLookup,
    PageFaultFileLazyCacheLoad,
    PageFaultFileLazyMap,
    PageFaultFileLazyCopy,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum HotSyscallPhase {
    OpenAtPathResolve,
    OpenAtInitialOpen,
    OpenAtInitialOpenVfs,
    OpenAtInitialOpenObject,
    OpenAtInitialOpenStat,
    OpenAtProcSelfFd,
    OpenAtCreateFile,
    OpenAtCreateReopen,
    OpenAtInfo,
    OpenAtNofollowCheck,
    OpenAtDirectoryCheck,
    OpenAtTruncate,
    OpenAtSetFlags,
    OpenAtInstallFd,
    MkdirPathResolve,
    MkdirCreateDir,
    MkdirApplyMode,
    NewfstatatPathResolve,
    NewfstatatEmptyPath,
    NewfstatatResolveFinal,
    NewfstatatBuildStat,
    NewfstatatMountInfo,
    NewfstatatWriteUser,
    StatxPathResolve,
    StatxEmptyPath,
    StatxResolveFinal,
    StatxBuildStat,
    StatxMountInfo,
    StatxPackOutput,
    StatxWriteUser,
    FsyncProcessLock,
    FsyncFlushMappings,
    FsyncCollectAreas,
    FsyncWriteArea,
    FsyncWritePage,
    FsyncWriteFile,
    ReadFileLike,
    ReadReadable,
    ReadCopyToUser,
    ReadTryRead,
    ReadBlockPrepare,
    ReadBlockRetry,
    TtyReadCopy,
    ReadReadableTty,
    ReadReadablePtySlave,
    ReadReadableUnixSocket,
    ReadReadableInetSocket,
    ReadReadableNetlinkSocket,
    ReadReadableFuseDevice,
    ReadReadableOther,
    ReadUnixDatagram,
    ReadUnixSeqpacketPeek,
    ReadUnixSeqpacketDrain,
    ReadUnixStreamPeek,
    ReadUnixStreamDrain,
    ReadUnixWaitReadable,
    ReadUnixWaitRegister,
    ReadUnixWaitFastpath,
    ReadUnixWaitPrepareBlock,
    ReadUnixWaitRecheck,
    PfFileCopyCacheRead,
    PfFileCopyMemcpy,
    PfFileCopyZeroFill,
    PfFileCopyFrameMap,
    PfFileCopyClusterMap,
    PfFileCopySharedRef,
    PfFileCopyCacheInsert,
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

pub fn init() {}

pub fn ensure_cpu_slots(_cpu_count: usize) {}

pub fn scope_start() -> u64 {
    0
}

pub fn record(_category: ProfileCategory, _start_cycles: u64) -> u64 {
    0
}

pub fn record_cycles(_category: ProfileCategory, _cycles: u64) {}

pub fn record_syscall_cpu(_syscall_no: usize, _cycles: u64) {}

pub fn record_syscall_blocked(_syscall_no: usize, _cycles: u64) {}

pub fn record_hot_syscall_phase(_phase: HotSyscallPhase, _cycles: u64) {}

pub fn record_ioctl_op(_op: LinuxIoctlOp, _cycles: u64) {}

pub fn record_ioctl_target(_target: LinuxIoctlTarget, _cycles: u64) {}

pub fn increment_timer_interrupts() {}

pub fn increment_timer_preemptions() {}

pub fn increment_resched_ipi_wakeups() {}

pub fn start_blocked_syscall(_thread: &mut Thread) {}

pub fn finish_blocked_syscall(_thread: &mut Thread) {}

pub fn record_file_lazy_fault(_stats: FileLazyFaultRecord) {}

pub fn maybe_report() {}
