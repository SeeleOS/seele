use crate::process::Process;

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

#[inline]
pub fn profile_current_process<R>(_bucket: PerfBucket, func: impl FnOnce() -> R) -> R {
    func()
}

#[inline]
pub fn log_current_block(_kind: &str) {}

#[inline]
pub fn log_and_clear_process_summary(_process: &Process, _exit_code: u64) {}
