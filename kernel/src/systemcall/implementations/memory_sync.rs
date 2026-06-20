use crate::memory::utils::Mut;
use alloc::{
    collections::{btree_map::BTreeMap, vec_deque::VecDeque},
    sync::Arc,
    vec::Vec,
};
use bitflags::bitflags;
use num_enum::TryFromPrimitive;
use x86_64::structures::paging::{FrameAllocator, PageSize, Size4KiB};
use x86_64::{VirtAddr, registers::model_specific::FsBase};

use crate::{
    define_syscall,
    memory::{
        addrspace::AddrSpace,
        addrspace::mem_area::{Data, MemoryArea, MmapPermissions, SharedFrames},
        paging::FRAME_ALLOCATOR,
        protection::Protection,
        user_safe,
        utils::apply_offset,
    },
    misc::others::protection_to_page_flags,
    misc::time::Time,
    process::manager::get_current_process,
    systemcall::utils::{SyscallError, SyscallImpl},
    thread::{
        ThreadRef, get_current_thread,
        manager::ThreadManager,
        misc::State,
        with_thread_manager,
        yielding::{BlockType, finish_block_current},
    },
};

#[derive(Clone, Copy, Debug, TryFromPrimitive)]
#[repr(u64)]
enum FutexOp {
    Wait = 0,
    Wake = 1,
    Requeue = 3,
    CmpRequeue = 4,
    WakeOp = 5,
    LockPi = 6,
    UnlockPi = 7,
    TrylockPi = 8,
    WaitBitset = 9,
    WakeBitset = 10,
}

#[derive(Clone, Copy, Debug, TryFromPrimitive)]
#[repr(u32)]
enum FutexWakeOp {
    Set = 0,
    Add = 1,
    Or = 2,
    AndN = 3,
    Xor = 4,
}

#[derive(Clone, Copy, Debug, TryFromPrimitive)]
#[repr(u32)]
enum FutexWakeCmp {
    Eq = 0,
    Ne = 1,
    Lt = 2,
    Le = 3,
    Gt = 4,
    Ge = 5,
}

#[derive(Clone, Copy, Debug, TryFromPrimitive)]
#[repr(u64)]
enum ArchPrctlCode {
    SetFs = 0x1002,
    GetFs = 0x1003,
}

bitflags! {
    #[derive(Clone, Copy, Debug)]
    pub(crate) struct MmapFlags: i32 {
        const SHARED = 0x01;
        const PRIVATE = 0x02;
        const SHARED_VALIDATE = 0x03;
        const DROPPABLE = 0x08;
        const FIXED = 0x10;
        const ANONYMOUS = 0x20;
        const MAP_32BIT = 0x40;
        const GROWSDOWN = 0x0100;
        const DENYWRITE = 0x0800;
        const EXECUTABLE = 0x1000;
        const LOCKED = 0x2000;
        const NORESERVE = 0x4000;
        const POPULATE = 0x008000;
        const NONBLOCK = 0x010000;
        const STACK = 0x020000;
        const HUGETLB = 0x040000;
        const SYNC = 0x080000;
        const FIXED_NOREPLACE = 0x100000;
    }
}

bitflags! {
    #[derive(Clone, Copy, Debug)]
    pub(crate) struct MremapFlags: u64 {
        const MAYMOVE = 0x1;
        const FIXED = 0x2;
        const DONTUNMAP = 0x4;
    }
}

bitflags! {
    #[derive(Clone, Copy, Debug)]
    pub(crate) struct MsyncFlags: i32 {
        const ASYNC = 0x1;
        const INVALIDATE = 0x2;
        const SYNC = 0x4;
    }
}

bitflags! {
    #[derive(Clone, Copy, Debug)]
    pub(crate) struct GetMempolicyFlags: u64 {
        const NODE = 0x1;
        const ADDR = 0x2;
        const MEMS_ALLOWED = 0x4;
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
struct FutexKey {
    pid: u64,
    addr: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FutexWaitId {
    key: FutexKey,
    waiter_id: u64,
}

#[derive(Clone)]
struct FutexWaiter {
    id: u64,
    thread: ThreadRef,
    bitset: u32,
}

#[derive(Default)]
struct FutexBucket {
    next_waiter_id: u64,
    waiters: VecDeque<FutexWaiter>,
}

impl FutexBucket {
    fn push_waiter(&mut self, thread: ThreadRef, bitset: u32) -> u64 {
        let waiter_id = self.next_waiter_id;
        self.next_waiter_id = self.next_waiter_id.saturating_add(1);
        self.waiters.push_back(FutexWaiter {
            id: waiter_id,
            thread,
            bitset,
        });
        waiter_id
    }
}

static FUTEX_QUEUE: Mut<BTreeMap<FutexKey, FutexBucket>> = Mut::new(BTreeMap::new());
const FUTEX_CLOCK_REALTIME: u64 = 0x100;
const FUTEX_BITSET_MATCH_ANY: u64 = 0xffff_ffff;
const FUTEX_OP_OPARG_SHIFT: u32 = 8;
const FUTEX_WAITERS: u32 = 0x8000_0000;
const FUTEX_OWNER_DIED: u32 = 0x4000_0000;
const FUTEX_TID_MASK: u32 = !(FUTEX_WAITERS | FUTEX_OWNER_DIED);

fn align_down(value: u64, align: u64) -> u64 {
    value & !(align - 1)
}

fn align_up(value: u64, align: u64) -> u64 {
    let addend = align - 1;
    value
        .checked_add(addend)
        .map(|value| align_down(value, align))
        .unwrap_or(!(align - 1))
}

#[repr(C)]
#[derive(Clone, Copy)]
struct LinuxTimespec {
    tv_sec: i64,
    tv_nsec: i64,
}

fn read_user_u32(addr: u64) -> Result<u32, SyscallError> {
    user_safe::read(addr as *const u32)
}

fn write_user_u32(addr: u64, value: u32) -> Result<(), SyscallError> {
    user_safe::write(addr as *mut u32, &value)
}

fn current_futex_key(addr: u64) -> FutexKey {
    let pid = get_current_process().lock().pid.0;
    FutexKey { pid, addr }
}

fn queue_waiter_in_locked_queue(
    queue: &mut BTreeMap<FutexKey, FutexBucket>,
    key: FutexKey,
    thread: ThreadRef,
    bitset: u32,
) -> FutexWaitId {
    let bucket = queue.entry(key).or_default();
    let waiter_id = bucket.push_waiter(thread, bitset);
    FutexWaitId { key, waiter_id }
}

fn take_futex_waiters_from_bucket(
    bucket: &mut FutexBucket,
    count: usize,
    wake_mask: Option<u32>,
) -> Vec<ThreadRef> {
    let mut woken = Vec::new();
    let mut scanned = 0;
    let initial_len = bucket.waiters.len();

    while woken.len() < count && scanned < initial_len {
        let waiter = bucket
            .waiters
            .pop_front()
            .expect("futex waiter queue length changed during wake");
        let should_wake = wake_mask.is_none_or(|mask| waiter.bitset & mask != 0);
        if should_wake {
            woken.push(waiter.thread);
        } else {
            bucket.waiters.push_back(waiter);
        }
        scanned += 1;
    }

    woken
}

fn bucket_is_empty(queue: &BTreeMap<FutexKey, FutexBucket>, key: FutexKey) -> bool {
    queue
        .get(&key)
        .is_some_and(|bucket| bucket.waiters.is_empty())
}

pub fn wake_futex_for_process(pid: u64, addr: u64, count: usize) -> usize {
    let threads = take_futex_waiters(pid, addr, count, None);
    let woken = threads.len();

    with_thread_manager(|manager| {
        for thread in threads {
            manager.wake(thread);
        }
    });

    woken
}

pub fn wake_futex_for_process_with_manager(
    pid: u64,
    addr: u64,
    count: usize,
    manager: &mut ThreadManager,
) -> usize {
    let threads = take_futex_waiters(pid, addr, count, None);
    let woken = threads.len();

    for thread in threads {
        manager.wake(thread);
    }

    woken
}

fn take_futex_waiters(pid: u64, addr: u64, count: usize, wake_mask: Option<u32>) -> Vec<ThreadRef> {
    let key = FutexKey { pid, addr };
    let mut queue = FUTEX_QUEUE.lock();
    let woken = queue
        .get_mut(&key)
        .map(|bucket| take_futex_waiters_from_bucket(bucket, count, wake_mask))
        .unwrap_or_default();

    if bucket_is_empty(&queue, key) {
        queue.remove(&key);
    }

    woken
}

pub fn remove_futex_waiter(wait_id: FutexWaitId) {
    let mut queue = FUTEX_QUEUE.lock();
    if let Some(bucket) = queue.get_mut(&wait_id.key) {
        bucket
            .waiters
            .retain(|waiter| waiter.id != wait_id.waiter_id);
        if bucket.waiters.is_empty() {
            queue.remove(&wait_id.key);
        }
    }
}

fn futex_timeout_timespec(timeout: u64) -> Result<Option<LinuxTimespec>, SyscallError> {
    if timeout == 0 {
        return Ok(None);
    }

    let timeout = user_safe::read(timeout as *const LinuxTimespec)?;
    if timeout.tv_sec < 0 || !(0..1_000_000_000).contains(&timeout.tv_nsec) {
        return Err(SyscallError::InvalidArguments);
    }

    Ok(Some(timeout))
}

fn validate_futex_user_addr(addr: u64) -> Result<(), SyscallError> {
    let _ = read_user_u32(addr)?;
    Ok(())
}

fn timespec_to_ns(timeout: LinuxTimespec) -> u64 {
    (timeout.tv_sec as u128)
        .saturating_mul(1_000_000_000)
        .saturating_add(timeout.tv_nsec as u128)
        .min(u64::MAX as u128) as u64
}

fn futex_relative_timeout_deadline(timeout: u64) -> Result<Option<Time>, SyscallError> {
    Ok(futex_timeout_timespec(timeout)?
        .map(|timeout| Time::since_boot().add_ns(timespec_to_ns(timeout))))
}

fn futex_absolute_timeout_deadline(
    timeout: u64,
    clock_realtime: bool,
) -> Result<Option<Time>, SyscallError> {
    let Some(timeout) = futex_timeout_timespec(timeout)? else {
        return Ok(None);
    };

    let absolute_ns = timespec_to_ns(timeout);
    if clock_realtime {
        let now_realtime = Time::current().as_nanoseconds();
        let delta_ns = absolute_ns.saturating_sub(now_realtime);
        Ok(Some(Time::since_boot().add_ns(delta_ns)))
    } else {
        Ok(Some(Time::from_nanoseconds(absolute_ns)))
    }
}

fn futex_wait_impl(
    arg1: u64,
    arg2: u64,
    deadline: Option<Time>,
    bitset: u32,
) -> Result<usize, SyscallError> {
    let key = current_futex_key(arg1);
    let current = get_current_thread();
    // Keep the value check and queue publication ordered with wakeups on the
    // same futex bucket. Without this, a wake can race between the user-space
    // store and our waiter publication, leaving the thread asleep forever.
    with_thread_manager(|manager| -> Result<(), SyscallError> {
        let mut queue = FUTEX_QUEUE.lock();
        let cur_value = u64::from(read_user_u32(arg1)?);
        if cur_value != arg2 {
            return Err(SyscallError::TryAgain);
        }
        if let Some(deadline) = deadline
            && Time::since_boot() >= deadline
        {
            return Err(SyscallError::TimedOut);
        }

        // Block the thread before publishing it in the futex bucket so any
        // racing wake observes a consistent Blocked state after we drop both
        // locks.
        let wait_id = queue_waiter_in_locked_queue(&mut queue, key, current.clone(), bitset);
        manager.block(current.clone(), BlockType::Futex { deadline, wait_id });
        Ok(())
    })?;

    finish_block_current();

    if let Some(deadline) = deadline
        && Time::since_boot() >= deadline
    {
        return Err(SyscallError::TimedOut);
    }

    Ok(0)
}

fn futex_wake_impl(arg1: u64, arg2: u64) -> Result<usize, SyscallError> {
    validate_futex_user_addr(arg1)?;
    let key = current_futex_key(arg1);
    let woken = wake_futex_for_process(key.pid, key.addr, arg2 as usize);
    Ok(woken)
}

fn futex_wake_bitset_impl(arg1: u64, arg2: u64, bitset: u32) -> Result<usize, SyscallError> {
    validate_futex_user_addr(arg1)?;
    let key = current_futex_key(arg1);
    let threads = take_futex_waiters(key.pid, key.addr, arg2 as usize, Some(bitset));
    let woken = threads.len();

    with_thread_manager(|manager| {
        for thread in threads {
            manager.wake(thread);
        }
    });

    Ok(woken)
}

fn futex_requeue_impl(
    arg1: u64,
    wake_count: u64,
    requeue_count: u64,
    uaddr2: u64,
    compare: Option<u64>,
) -> Result<usize, SyscallError> {
    if let Some(expected) = compare {
        let cur_value = u64::from(read_user_u32(arg1)?);
        if cur_value != expected {
            return Err(SyscallError::TryAgain);
        }
    }

    let pid = get_current_process().lock().pid.0;
    let source = FutexKey { pid, addr: arg1 };
    let target = FutexKey { pid, addr: uaddr2 };
    let mut queue = FUTEX_QUEUE.lock();
    let mut woken = Vec::new();
    let mut moved = Vec::new();

    if let Some(bucket) = queue.get_mut(&source) {
        for _ in 0..wake_count {
            if let Some(waiter) = bucket.waiters.pop_front() {
                woken.push(waiter.thread);
            } else {
                break;
            }
        }
        for _ in 0..requeue_count {
            if let Some(waiter) = bucket.waiters.pop_front() {
                moved.push(waiter);
            } else {
                break;
            }
        }
    }

    if bucket_is_empty(&queue, source) {
        queue.remove(&source);
    }
    if !moved.is_empty() {
        let moved_ids = moved
            .iter()
            .map(|waiter| {
                let wait_id = queue_waiter_in_locked_queue(
                    &mut queue,
                    target,
                    waiter.thread.clone(),
                    waiter.bitset,
                );
                (waiter.thread.clone(), wait_id)
            })
            .collect::<Vec<_>>();
        drop(queue);
        for (thread, wait_id) in moved_ids {
            let mut thread = thread.lock();
            match &mut thread.state {
                State::Blocking(BlockType::Futex {
                    wait_id: current_wait_id,
                    ..
                })
                | State::Blocked(BlockType::Futex {
                    wait_id: current_wait_id,
                    ..
                }) => {
                    *current_wait_id = wait_id;
                }
                _ => {}
            }
        }
    } else {
        drop(queue);
    }

    let woke = woken.len();
    with_thread_manager(|manager| {
        for thread in woken {
            manager.wake(thread);
        }
    });

    Ok(woke)
}

fn futex_wake_op_apply(old_value: u32, encoded: u32) -> Result<(u32, bool), SyscallError> {
    let op =
        FutexWakeOp::try_from((encoded >> 28) & 0xf).map_err(|_| SyscallError::InvalidArguments)?;
    let cmp = FutexWakeCmp::try_from((encoded >> 24) & 0xf)
        .map_err(|_| SyscallError::InvalidArguments)?;
    let mut op_arg = (encoded >> 12) & 0xfff;
    let cmp_arg = encoded & 0xfff;
    if matches!(op, FutexWakeOp::Set) && (op_arg & FUTEX_OP_OPARG_SHIFT) != 0 {
        return Err(SyscallError::InvalidArguments);
    }
    if (op_arg & FUTEX_OP_OPARG_SHIFT) != 0 {
        let shift = op_arg & !FUTEX_OP_OPARG_SHIFT;
        op_arg = 1u32
            .checked_shl(shift)
            .ok_or(SyscallError::InvalidArguments)?;
    }

    let new_value = match op {
        FutexWakeOp::Set => op_arg,
        FutexWakeOp::Add => old_value.wrapping_add(op_arg),
        FutexWakeOp::Or => old_value | op_arg,
        FutexWakeOp::AndN => old_value & !op_arg,
        FutexWakeOp::Xor => old_value ^ op_arg,
    };
    let should_wake_second = match cmp {
        FutexWakeCmp::Eq => old_value == cmp_arg,
        FutexWakeCmp::Ne => old_value != cmp_arg,
        FutexWakeCmp::Lt => old_value < cmp_arg,
        FutexWakeCmp::Le => old_value <= cmp_arg,
        FutexWakeCmp::Gt => old_value > cmp_arg,
        FutexWakeCmp::Ge => old_value >= cmp_arg,
    };
    Ok((new_value, should_wake_second))
}

fn futex_wake_op_impl(
    arg1: u64,
    wake_count_1: u64,
    wake_count_2: u64,
    uaddr2: u64,
    encoded_op: u64,
) -> Result<usize, SyscallError> {
    let pid = get_current_process().lock().pid.0;
    let old_value = read_user_u32(uaddr2)?;
    let encoded_op = u32::try_from(encoded_op).map_err(|_| SyscallError::InvalidArguments)?;
    let (new_value, should_wake_second) = futex_wake_op_apply(old_value, encoded_op)?;
    write_user_u32(uaddr2, new_value)?;

    let mut total_woken = wake_futex_for_process(pid, arg1, wake_count_1 as usize);
    if should_wake_second {
        total_woken += wake_futex_for_process(pid, uaddr2, wake_count_2 as usize);
    }
    Ok(total_woken)
}

fn current_tid_u32() -> Result<u32, SyscallError> {
    u32::try_from(get_current_thread().lock().id.0).map_err(|_| SyscallError::InvalidArguments)
}

fn futex_lock_pi_impl(
    arg1: u64,
    timeout: u64,
    clock_realtime: bool,
) -> Result<usize, SyscallError> {
    let key = current_futex_key(arg1);
    let current = get_current_thread();
    let tid = current_tid_u32()?;
    let deadline = if timeout == 0 {
        None
    } else {
        futex_absolute_timeout_deadline(timeout, clock_realtime)?
    };

    loop {
        {
            let cur_value = read_user_u32(arg1)?;
            let owner = cur_value & FUTEX_TID_MASK;
            if owner == tid {
                return Err(SyscallError::ResourceDeadlock);
            }

            if owner == 0 {
                let new_value = tid | (cur_value & FUTEX_OWNER_DIED);
                write_user_u32(arg1, new_value)?;
                return Ok(0);
            }

            if let Some(deadline) = deadline
                && Time::since_boot() >= deadline
            {
                return Err(SyscallError::TimedOut);
            }
        }

        {
            let cur_value = read_user_u32(arg1)?;
            let owner = cur_value & FUTEX_TID_MASK;
            if owner == 0 {
                continue;
            }
            if owner == tid {
                return Err(SyscallError::ResourceDeadlock);
            }
            if let Some(deadline) = deadline
                && Time::since_boot() >= deadline
            {
                return Err(SyscallError::TimedOut);
            }

            if (cur_value & FUTEX_WAITERS) == 0 {
                write_user_u32(arg1, cur_value | FUTEX_WAITERS)?;
            }

            with_thread_manager(|manager| {
                let mut queue = FUTEX_QUEUE.lock();
                let wait_id = queue_waiter_in_locked_queue(
                    &mut queue,
                    key,
                    current.clone(),
                    FUTEX_BITSET_MATCH_ANY as u32,
                );
                manager.block(current.clone(), BlockType::Futex { deadline, wait_id });
            });
        }

        finish_block_current();

        if let Some(deadline) = deadline
            && Time::since_boot() >= deadline
        {
            return Err(SyscallError::TimedOut);
        }
    }
}

fn futex_trylock_pi_impl(arg1: u64) -> Result<usize, SyscallError> {
    let tid = current_tid_u32()?;
    let cur_value = read_user_u32(arg1)?;
    let owner = cur_value & FUTEX_TID_MASK;
    if owner == tid {
        return Err(SyscallError::ResourceDeadlock);
    }
    if owner != 0 {
        return Err(SyscallError::TryAgain);
    }

    write_user_u32(arg1, tid | (cur_value & FUTEX_OWNER_DIED))?;
    Ok(0)
}

fn futex_unlock_pi_impl(arg1: u64) -> Result<usize, SyscallError> {
    let tid = current_tid_u32()?;
    let cur_value = read_user_u32(arg1)?;
    let owner = cur_value & FUTEX_TID_MASK;
    if owner != tid {
        return Err(SyscallError::PermissionDenied);
    }

    let next_waiter = {
        let key = current_futex_key(arg1);
        let mut queue = FUTEX_QUEUE.lock();
        let next = queue
            .get_mut(&key)
            .and_then(|bucket| bucket.waiters.pop_front())
            .map(|waiter| waiter.thread);
        if bucket_is_empty(&queue, key) {
            queue.remove(&key);
        }
        next
    };

    if let Some(next) = next_waiter {
        let next_tid = {
            let next = next.lock();
            u32::try_from(next.id.0).map_err(|_| SyscallError::InvalidArguments)?
        };
        let still_has_waiters = {
            let key = current_futex_key(arg1);
            FUTEX_QUEUE
                .lock()
                .get(&key)
                .is_some_and(|bucket| !bucket.waiters.is_empty())
        };
        let new_value = next_tid | if still_has_waiters { FUTEX_WAITERS } else { 0 };
        write_user_u32(arg1, new_value)?;
        with_thread_manager(|manager| manager.wake(next));
    } else {
        write_user_u32(arg1, 0)?;
    }

    Ok(0)
}

define_syscall!(Futex, |arg1: u64,
                        op: u64,
                        arg2: u64,
                        timeout: u64,
                        uaddr2: u64,
                        val3: u64| {
    let base_op = op & 0x7f;
    let futex_op = FutexOp::try_from(base_op).map_err(|_| SyscallError::InvalidArguments)?;

    match futex_op {
        FutexOp::Wait => futex_wait_impl(
            arg1,
            arg2,
            futex_relative_timeout_deadline(timeout)?,
            FUTEX_BITSET_MATCH_ANY as u32,
        ),
        FutexOp::Wake => futex_wake_impl(arg1, arg2),
        FutexOp::Requeue => futex_requeue_impl(arg1, arg2, timeout, uaddr2, None),
        FutexOp::CmpRequeue => futex_requeue_impl(arg1, arg2, timeout, uaddr2, Some(val3)),
        FutexOp::WakeOp => futex_wake_op_impl(arg1, arg2, timeout, uaddr2, val3),
        FutexOp::LockPi => futex_lock_pi_impl(arg1, timeout, op & FUTEX_CLOCK_REALTIME != 0),
        FutexOp::UnlockPi => futex_unlock_pi_impl(arg1),
        FutexOp::TrylockPi => futex_trylock_pi_impl(arg1),
        FutexOp::WaitBitset => {
            let bitset = u32::try_from(val3).map_err(|_| SyscallError::InvalidArguments)?;
            if bitset == 0 {
                return Err(SyscallError::InvalidArguments);
            }
            futex_wait_impl(
                arg1,
                arg2,
                futex_absolute_timeout_deadline(timeout, op & FUTEX_CLOCK_REALTIME != 0)?,
                bitset,
            )
        }
        FutexOp::WakeBitset => {
            let bitset = u32::try_from(val3).map_err(|_| SyscallError::InvalidArguments)?;
            if bitset == 0 {
                return Err(SyscallError::InvalidArguments);
            }
            futex_wake_bitset_impl(arg1, arg2, bitset)
        }
    }
});

define_syscall!(ArchPrctl, |code: u64, addr: u64| {
    match ArchPrctlCode::try_from(code).map_err(|_| SyscallError::InvalidArguments)? {
        ArchPrctlCode::SetFs => {
            FsBase::write(VirtAddr::new(addr));
            Ok(0)
        }
        ArchPrctlCode::GetFs => {
            user_safe::write(addr as *mut u8, &FsBase::read().as_u64())?;
            Ok(0)
        }
    }
});

fn prot_to_protection(prot: i32) -> Result<Protection, SyscallError> {
    let mut protection = Protection::empty();
    if (prot & Protection::READ.bits() as i32) != 0 {
        protection |= Protection::READ;
    }
    if (prot & Protection::WRITE.bits() as i32) != 0 {
        protection |= Protection::WRITE;
    }
    if (prot & Protection::EXEC.bits() as i32) != 0 {
        protection |= Protection::EXEC;
    }
    Ok(protection)
}

fn mapping_overlaps(areas: &[MemoryArea], start: VirtAddr, end: VirtAddr) -> bool {
    areas
        .iter()
        .any(|area| area.start < end && area.end > start)
}

fn mmap_shared(flags: MmapFlags) -> Result<bool, SyscallError> {
    match flags.bits() & MmapFlags::SHARED_VALIDATE.bits() {
        bits if bits == MmapFlags::SHARED.bits() => Ok(true),
        bits if bits == MmapFlags::PRIVATE.bits() => Ok(false),
        bits if bits == MmapFlags::SHARED_VALIDATE.bits() => Ok(true),
        _ => Err(SyscallError::InvalidArguments),
    }
}

fn checked_user_mapping(addr: u64, pages: u64) -> Result<(VirtAddr, VirtAddr), SyscallError> {
    if addr == 0 || !addr.is_multiple_of(Size4KiB::SIZE) {
        return Err(SyscallError::InvalidArguments);
    }
    let len = pages
        .checked_mul(Size4KiB::SIZE)
        .ok_or(SyscallError::InvalidArguments)?;
    AddrSpace::checked_user_range(addr, len).ok_or(SyscallError::InvalidArguments)
}

fn checked_user_range_for_memory_syscall(addr: u64, len: u64) -> Result<(u64, u64), SyscallError> {
    if len == 0 {
        return Ok((addr, addr));
    }
    let end = addr
        .checked_add(len)
        .ok_or(SyscallError::InvalidArguments)?;
    let _ = AddrSpace::checked_user_range(addr, len).ok_or(SyscallError::NoMemory)?;
    Ok((addr, end))
}

fn mapped_area_covers_range(areas: &[MemoryArea], start: u64, end: u64) -> bool {
    let mut cursor = start;
    while cursor < end {
        let Some(area) = areas
            .iter()
            .find(|area| area.start.as_u64() <= cursor && area.end.as_u64() > cursor)
        else {
            return false;
        };
        cursor = area.end.as_u64().min(end);
    }
    true
}

fn ensure_mapped_user_range(addr: VirtAddr, len: u64) -> Result<(), SyscallError> {
    let (start, end) = checked_user_range_for_memory_syscall(addr.as_u64(), len)?;
    if start == end {
        return Ok(());
    }

    let process = get_current_process();
    let process = process.lock();
    if mapped_area_covers_range(&process.addrspace.memory_areas, start, end) {
        Ok(())
    } else {
        Err(SyscallError::NoMemory)
    }
}

fn resized_file_mapping(
    file: Arc<crate::filesystem::object::FileLikeObject>,
    offset: u64,
    pages: u64,
    shared: bool,
) -> Data {
    file.mmap_data(offset, pages, shared)
}

fn anonymous_mapping_data(
    pages: u64,
    shared: bool,
    protection: Protection,
) -> Result<Data, SyscallError> {
    if !shared {
        return Ok(Data::Normal(MmapPermissions {
            shared_mapping: false,
            ..Default::default()
        }));
    }

    let mut frames = Vec::with_capacity(pages as usize);
    let mut allocator = FRAME_ALLOCATOR.get().unwrap().lock();
    for _ in 0..pages {
        let frame = allocator.allocate_frame().ok_or(SyscallError::NoMemory)?;
        unsafe {
            core::ptr::write_bytes(
                apply_offset(frame.start_address().as_u64()) as *mut u8,
                0,
                Size4KiB::SIZE as usize,
            );
        }
        frames.push(frame);
    }

    Ok(Data::Shared {
        frames: Arc::new(SharedFrames::new(frames)),
        flags: protection_to_page_flags(protection),
    })
}

fn fd_allows_shared_write(object: &Arc<dyn crate::object::Object>) -> Result<bool, SyscallError> {
    let flags = object.clone().get_flags().map_err(SyscallError::from)?;
    Ok(flags.intersects(crate::object::FileFlags::WRONLY | crate::object::FileFlags::RDWR))
}

fn check_shared_writable_mapping(
    object: &Arc<dyn crate::object::Object>,
    shared: bool,
    protection: Protection,
) -> Result<(), SyscallError> {
    if !shared || !protection.contains(Protection::WRITE) {
        return Ok(());
    }

    if fd_allows_shared_write(object)? {
        Ok(())
    } else {
        Err(SyscallError::AccessDenied)
    }
}

fn shared_write_permission(
    object: &Arc<dyn crate::object::Object>,
    shared: bool,
) -> Result<bool, SyscallError> {
    if !shared {
        return Ok(false);
    }
    fd_allows_shared_write(object)
}

define_syscall!(Mmap, |addr: u64,
                       len: u64,
                       prot: i32,
                       flags: MmapFlags,
                       fd: i32,
                       offset: u64| {
    if len == 0 {
        return Err(SyscallError::InvalidArguments);
    }
    let protection = prot_to_protection(prot)?;
    let pages = len.div_ceil(4096);
    let fixed = flags.intersects(MmapFlags::FIXED | MmapFlags::FIXED_NOREPLACE);
    let shared = mmap_shared(flags)?;

    if fixed {
        if !offset.is_multiple_of(4096) {
            return Err(SyscallError::InvalidArguments);
        }
        let (start, end) = checked_user_mapping(addr, pages)?;

        let file_mapping = if flags.contains(MmapFlags::ANONYMOUS) {
            None
        } else {
            if fd < 0 {
                return Err(SyscallError::InvalidArguments);
            }
            let object = crate::object::misc::get_object_current_process(fd as u64)
                .map_err(SyscallError::from)?;
            check_shared_writable_mapping(&object, shared, protection)?;
            let file = object.as_file_like()?;
            if file.is_device_backed() {
                return Err(SyscallError::InvalidArguments);
            }
            Some(file.mmap_data(offset, pages, shared))
        };

        let current = get_current_process();
        let mut current = current.lock();

        if flags.contains(MmapFlags::FIXED_NOREPLACE)
            && mapping_overlaps(&current.addrspace.memory_areas, start, end)
        {
            return Err(SyscallError::FileAlreadyExists);
        }

        if flags.contains(MmapFlags::FIXED) {
            current.addrspace.unmap(start, pages * 4096);
        }

        let data = if let Some(data) = file_mapping {
            data
        } else {
            anonymous_mapping_data(pages, shared, protection)?
        };

        current.addrspace.register_area(MemoryArea::new(
            start,
            pages,
            protection_to_page_flags(protection),
            protection,
            data,
            true,
        ));
        return Ok(addr as usize);
    }

    if addr != 0 {
        return Err(SyscallError::InvalidArguments);
    }

    if flags.contains(MmapFlags::ANONYMOUS) {
        let current = get_current_process();
        let data = anonymous_mapping_data(pages, shared, protection)?;
        return Ok(current
            .lock()
            .addrspace
            .allocate_user_lazy(pages, protection, data)
            .as_u64() as usize);
    }

    if !offset.is_multiple_of(4096) || fd < 0 {
        return Err(SyscallError::InvalidArguments);
    }
    let object =
        crate::object::misc::get_object_current_process(fd as u64).map_err(SyscallError::from)?;
    check_shared_writable_mapping(&object, shared, protection)?;
    let shared_write_allowed = shared_write_permission(&object, shared)?;
    if let Ok(file) = object.clone().as_file_like()
        && !file.is_device_backed()
    {
        let data = file.mmap_data(offset, pages, shared);
        let address = get_current_process()
            .lock()
            .addrspace
            .allocate_user_lazy(pages, protection, data);
        return Ok(address.as_u64() as usize);
    }
    let object = object.as_mappable()?;
    let address = object.map(offset, pages, protection)?;
    if shared {
        let current = get_current_process();
        if let Some(area) = current.lock().addrspace.get_area_mut(address)
            && let Data::Normal(permissions) = &mut area.data
        {
            permissions.shared_write_allowed = Some(shared_write_allowed);
        }
    }
    Ok(address.as_u64() as usize)
});

define_syscall!(Munmap, |addr: VirtAddr, len: u64| {
    get_current_process().lock().addrspace.unmap(addr, len);
    Ok(0)
});

define_syscall!(Msync, |addr: VirtAddr, len: u64, flags: MsyncFlags| {
    if !addr.is_aligned(Size4KiB::SIZE) {
        return Err(SyscallError::InvalidArguments);
    }
    if len == 0 {
        return Ok(0);
    }
    if flags.contains(MsyncFlags::INVALIDATE) {
        return Err(SyscallError::OperationNotSupported);
    }
    if flags.contains(MsyncFlags::ASYNC) && flags.contains(MsyncFlags::SYNC) {
        return Err(SyscallError::InvalidArguments);
    }

    let process = get_current_process();
    let mut process = process.lock();
    process
        .addrspace
        .flush_file_mappings(addr, len)
        .map_err(SyscallError::from)?;
    Ok(0)
});

define_syscall!(Mremap, |old_addr: VirtAddr,
                         old_len: u64,
                         new_len: u64,
                         flags: MremapFlags,
                         _new_addr: u64| {
    if old_len == 0 || new_len == 0 {
        return Err(SyscallError::InvalidArguments);
    }

    let old_pages = old_len.div_ceil(4096);
    let new_pages = new_len.div_ceil(4096);

    let current = get_current_process();
    let mut current = current.lock();
    let area = current
        .addrspace
        .get_area(old_addr)
        .cloned()
        .ok_or(SyscallError::InvalidArguments)?;

    if area.start != old_addr {
        return Err(SyscallError::InvalidArguments);
    }

    if new_pages <= old_pages {
        if new_len < old_len {
            current
                .addrspace
                .unmap(old_addr + new_len, old_len - new_len);
        }
        return Ok(old_addr.as_u64() as usize);
    }

    if !flags.contains(MremapFlags::MAYMOVE) {
        return Err(SyscallError::NoMemory);
    }

    let new_start = current.addrspace.fetch_add_user_mem(new_pages);
    let new_data = match &area.data {
        Data::Normal(permissions) => Data::Normal(*permissions),
        Data::File {
            offset,
            file,
            shared,
            ..
        } => resized_file_mapping(file.clone(), *offset, new_pages, *shared),
        Data::Shared { .. } => return Err(SyscallError::InvalidArguments),
    };
    let new_area = MemoryArea::new(
        new_start,
        new_pages,
        area.flags,
        area.protection,
        new_data,
        area.lazy,
    );
    current.addrspace.register_area(new_area.clone());

    let copy_pages = match &area.data {
        Data::Normal(_) => old_pages,
        Data::File { .. } => old_pages,
        Data::Shared { .. } => 0,
    };
    for page_index in 0..copy_pages {
        let src_addr = old_addr + page_index * 4096;
        let Some(_) = current.addrspace.translate_addr(src_addr) else {
            continue;
        };

        let dst_addr = new_start + page_index * 4096;
        current.addrspace.apply_page(
            x86_64::structures::paging::Page::containing_address(dst_addr),
            new_area.clone(),
        );
        let src_phys = current
            .addrspace
            .translate_addr(src_addr)
            .ok_or(SyscallError::InvalidArguments)?;
        let dst_phys = current
            .addrspace
            .translate_addr(dst_addr)
            .ok_or(SyscallError::InvalidArguments)?;
        if src_phys == dst_phys {
            continue;
        }
        let copy_len = core::cmp::min(4096, (old_len - page_index * 4096) as usize);
        unsafe {
            core::ptr::copy_nonoverlapping(
                crate::memory::utils::apply_offset(src_phys.as_u64()) as *const u8,
                crate::memory::utils::apply_offset(dst_phys.as_u64()) as *mut u8,
                copy_len,
            );
        }
    }

    current.addrspace.unmap(old_addr, old_len);
    Ok(new_start.as_u64() as usize)
});

define_syscall!(Mprotect, |addr: VirtAddr, len: u64, prot: i32| {
    let protection = prot_to_protection(prot)?;
    if len == 0 {
        return Ok(0);
    }

    if !addr.is_aligned(Size4KiB::SIZE) {
        return Err(SyscallError::InvalidArguments);
    }

    let pages = len.div_ceil(4096);
    let end = addr
        .as_u64()
        .checked_add(
            pages
                .checked_mul(Size4KiB::SIZE)
                .ok_or(SyscallError::NoMemory)?,
        )
        .ok_or(SyscallError::NoMemory)?;
    let end = VirtAddr::new(end);

    let current_process = get_current_process();
    let mut current = current_process.lock();
    current
        .addrspace
        .validate_permission_update(addr, end, protection)?;
    current.addrspace.update_permissions(addr, end, protection);
    Ok(0)
});

define_syscall!(Mlock, |addr: VirtAddr, len: u64| {
    if len == 0 {
        return Ok(0);
    }

    ensure_mapped_user_range(addr, len)?;
    let start = align_down(addr.as_u64(), Size4KiB::SIZE);
    let end = align_up(
        addr.as_u64()
            .checked_add(len)
            .ok_or(SyscallError::InvalidArguments)?,
        Size4KiB::SIZE,
    );
    let rounded_len = end
        .checked_sub(start)
        .ok_or(SyscallError::InvalidArguments)?;
    let start = VirtAddr::new(start);
    let last_page = VirtAddr::new(end - 1);

    let process = get_current_process();
    if rounded_len > process.lock().rlimit_memlock_cur {
        return Err(SyscallError::NoMemory);
    }

    let mut process = process.lock();
    let addrspace = &mut process.addrspace;
    let mut page_addr = start;
    loop {
        let _ = addrspace.read(page_addr.as_u64() as *const u8)?;
        if page_addr >= last_page {
            break;
        }
        page_addr += Size4KiB::SIZE;
    }

    Ok(0)
});

define_syscall!(Munlock, |addr: VirtAddr, len: u64| {
    if len == 0 {
        return Ok(0);
    }

    ensure_mapped_user_range(addr, len)?;
    Ok(0)
});

define_syscall!(Mincore, |addr: VirtAddr, len: usize, vec: *mut u8| {
    if len == 0 {
        return Ok(0);
    }
    if vec.is_null() {
        return Err(SyscallError::BadAddress);
    }
    if !addr.is_aligned(Size4KiB::SIZE) {
        return Err(SyscallError::InvalidArguments);
    }

    let page_count = len.div_ceil(Size4KiB::SIZE as usize);
    user_safe::read_buffer(vec, page_count)?;
    let mut residency = Vec::with_capacity(page_count);

    {
        let current = get_current_process();
        let mut current = current.lock();

        for page_index in 0..page_count {
            let page_addr = addr + (page_index * Size4KiB::SIZE as usize) as u64;
            if current.addrspace.get_area(page_addr).is_none() {
                return Err(SyscallError::NoMemory);
            }

            residency.push(u8::from(
                current.addrspace.translate_addr(page_addr).is_some(),
            ));
        }
    }

    user_safe::write_buffer(vec, &residency)?;
    Ok(0)
});

define_syscall!(
    GetMempolicy,
    |mode: *mut i32, nodemask: *mut u64, maxnode: u64, addr: u64, flags: GetMempolicyFlags| {
        if flags.contains(GetMempolicyFlags::MEMS_ALLOWED)
            && flags.intersects(GetMempolicyFlags::ADDR | GetMempolicyFlags::NODE)
        {
            return Err(SyscallError::InvalidArguments);
        }
        if flags.contains(GetMempolicyFlags::ADDR) {
            if addr == 0 {
                return Err(SyscallError::InvalidArguments);
            }
        } else if addr != 0 {
            return Err(SyscallError::InvalidArguments);
        }
        if flags.contains(GetMempolicyFlags::NODE) && !flags.contains(GetMempolicyFlags::ADDR) {
            return Err(SyscallError::InvalidArguments);
        }
        if !nodemask.is_null() && maxnode == 0 {
            return Err(SyscallError::InvalidArguments);
        }

        if flags.contains(GetMempolicyFlags::ADDR) {
            let process = get_current_process();
            let mut process = process.lock();
            let addr = VirtAddr::new(addr);
            if process.addrspace.get_area(addr).is_none() {
                return Err(SyscallError::BadAddress);
            }
            if flags.contains(GetMempolicyFlags::NODE)
                && process.addrspace.translate_addr(addr).is_none()
            {
                let _ = process.addrspace.read(addr.as_u64() as *const u8)?;
            }
        }

        if !mode.is_null() && !flags.contains(GetMempolicyFlags::MEMS_ALLOWED) {
            user_safe::write(mode, &0i32)?;
        }

        if !nodemask.is_null() {
            let word_count = maxnode.div_ceil(u64::BITS as u64) as usize;
            let first_word = if flags.contains(GetMempolicyFlags::MEMS_ALLOWED) {
                1u64
            } else {
                0u64
            };
            user_safe::write(nodemask, &first_word)?;
            for word_index in 1..word_count {
                unsafe {
                    user_safe::write(nodemask.add(word_index), &0u64)?;
                }
            }
        }

        Ok(0)
    }
);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        filesystem::{path::Path, vfs::VirtualFS},
        memory::protection::Protection,
        misc::timer::ClockId,
        process::Process,
        signal::{Signal, Signals},
        systemcall::{
            arg_types::SyscallArg,
            implementations::{
                Brk, Ftruncate, Futex, GetMempolicy, Lseek, Mincore, Mlock, Mmap, Mprotect, Mremap,
                Msync, Munlock, Munmap, OpenAt, PollEvents, Read, Write, filesystem::OpenFlags,
            },
            test::{
                TestLinuxTimespec, assert_user_bytes, close_test_fd, expect_errno, expect_fd,
                expect_ok, write_user_cstr,
            },
            test_helpers::{
                SyscallArgs, allocate_user_test_page, read_user_value, write_user_value,
            },
            utils::SyscallError,
        },
    };

    crate::test!(
        futex_syscalls,
        "futex syscalls follow linux rules",
        futex_syscalls_follow_linux_rules
    );
    crate::test!(
        memory_mapping_syscalls,
        "brk mmap mprotect munmap mremap msync and mincore follow linux rules",
        memory_mapping_syscalls_follow_linux_rules
    );
    crate::test!(
        typed_syscall_args,
        "typed syscall args convert flags and enums at boundary",
        typed_syscall_args_convert_flags_and_enums_at_boundary
    );

    fn futex_syscalls_follow_linux_rules() {
        const FUTEX_WAIT: u64 = 0;
        const FUTEX_WAKE: u64 = 1;
        const FUTEX_WAIT_BITSET: u64 = 9;
        const FUTEX_WAKE_BITSET: u64 = 10;

        let page = allocate_user_test_page();
        write_user_value(page + 384, &7u32);
        expect_errno(
            SyscallArgs::new([page + 384, FUTEX_WAIT, 8, 0, 0, 0]).call::<Futex>(),
            SyscallError::TryAgain,
        );
        write_user_value(
            page + 392,
            &TestLinuxTimespec {
                tv_sec: 0,
                tv_nsec: 1_000_000_000,
            },
        );
        expect_errno(
            SyscallArgs::new([page + 384, FUTEX_WAIT, 7, page + 392, 0, 0]).call::<Futex>(),
            SyscallError::InvalidArguments,
        );
        expect_ok(
            SyscallArgs::new([page + 384, FUTEX_WAKE, 3, 0, 0, 0]).call::<Futex>(),
            0,
        );
        expect_errno(
            SyscallArgs::new([page + 384, FUTEX_WAIT_BITSET, 7, 0, 0, 0]).call::<Futex>(),
            SyscallError::InvalidArguments,
        );
        expect_errno(
            SyscallArgs::new([page + 384, FUTEX_WAKE_BITSET, 1, 0, 0, 0]).call::<Futex>(),
            SyscallError::InvalidArguments,
        );
        write_user_value(
            page + 392,
            &TestLinuxTimespec {
                tv_sec: 0,
                tv_nsec: 0,
            },
        );
        expect_errno(
            SyscallArgs::new([page + 384, FUTEX_WAIT_BITSET, 7, page + 392, 0, 1]).call::<Futex>(),
            SyscallError::TimedOut,
        );
        expect_ok(
            SyscallArgs::new([page + 384, FUTEX_WAKE_BITSET, 1, 0, 0, 1]).call::<Futex>(),
            0,
        );
        expect_errno(
            SyscallArgs::new([0, FUTEX_WAKE, 1, 0, 0, 0]).call::<Futex>(),
            SyscallError::BadAddress,
        );
    }

    fn memory_mapping_syscalls_follow_linux_rules() {
        const MAP_SHARED: u64 = 0x01;
        const MAP_PRIVATE: u64 = 0x02;
        const MAP_ANONYMOUS: u64 = 0x20;
        const MAP_FIXED_NOREPLACE: u64 = 0x100000;
        const MREMAP_MAYMOVE: u64 = 0x1;
        const MS_ASYNC: u64 = 0x1;
        const MS_INVALIDATE: u64 = 0x2;
        const MS_SYNC: u64 = 0x4;
        const AT_FDCWD: u64 = (-100i32) as u64;

        let process = get_current_process();
        let original_break = process.lock().program_break;
        let current_break = SyscallArgs::new([0, 0, 0, 0, 0, 0])
            .call::<Brk>()
            .expect("brk query should succeed") as u64;
        let grown_break = current_break + 5000;
        expect_ok(
            SyscallArgs::new([grown_break, 0, 0, 0, 0, 0]).call::<Brk>(),
            grown_break as usize,
        );
        assert_eq!(process.lock().program_break, grown_break);
        let brk_area = process
            .lock()
            .addrspace
            .get_area(x86_64::VirtAddr::new(current_break.div_ceil(4096) * 4096))
            .cloned()
            .expect("brk growth should create mapped area");
        assert!(matches!(brk_area.data, Data::Normal(_)));
        expect_ok(
            SyscallArgs::new([current_break, 0, 0, 0, 0, 0]).call::<Brk>(),
            current_break as usize,
        );
        process.lock().program_break = original_break;

        let anon_addr = SyscallArgs::new([
            0,
            8192,
            (Protection::READ | Protection::WRITE).bits() as u64,
            MAP_PRIVATE | MAP_ANONYMOUS,
            u64::MAX,
            0,
        ])
        .call::<Mmap>()
        .expect("anon mmap should succeed") as u64;
        let anon_area = process
            .lock()
            .addrspace
            .get_area(x86_64::VirtAddr::new(anon_addr))
            .cloned()
            .expect("anon mmap should register area");
        assert!(matches!(anon_area.data, Data::Normal(_)));
        assert_eq!(
            SyscallArgs::new([anon_addr, 4096, 0, 0, 0, 0]).call::<Mlock>(),
            Ok(0),
            "mlock should succeed on initial anonymous mapping"
        );
        expect_ok(
            SyscallArgs::new([anon_addr, 4096, 0, 0, 0, 0]).call::<Munlock>(),
            0,
        );
        process
            .lock()
            .addrspace
            .write_buffer(anon_addr as *mut u8, b"mmap")
            .unwrap();
        assert_user_bytes(anon_addr, b"mmap");
        expect_errno(
            SyscallArgs::new([
                0,
                0,
                Protection::READ.bits() as u64,
                MAP_PRIVATE | MAP_ANONYMOUS,
                u64::MAX,
                0,
            ])
            .call::<Mmap>(),
            SyscallError::InvalidArguments,
        );
        expect_errno(
            SyscallArgs::new([
                0x2000,
                4096,
                Protection::READ.bits() as u64,
                MAP_PRIVATE | MAP_ANONYMOUS,
                u64::MAX,
                0,
            ])
            .call::<Mmap>(),
            SyscallError::InvalidArguments,
        );
        expect_errno(
            SyscallArgs::new([
                anon_addr,
                4096,
                Protection::READ.bits() as u64,
                MAP_PRIVATE | MAP_ANONYMOUS | MAP_FIXED_NOREPLACE,
                u64::MAX,
                0,
            ])
            .call::<Mmap>(),
            SyscallError::FileAlreadyExists,
        );

        expect_ok(
            SyscallArgs::new([anon_addr, 4096, Protection::READ.bits() as u64, 0, 0, 0])
                .call::<Mprotect>(),
            0,
        );
        let readonly_area = process
            .lock()
            .addrspace
            .get_area(x86_64::VirtAddr::new(anon_addr))
            .cloned()
            .expect("mprotect should keep mapping");
        assert!(
            !readonly_area
                .flags
                .contains(x86_64::structures::paging::PageTableFlags::WRITABLE)
        );

        let remapped_addr = SyscallArgs::new([anon_addr, 4096, 8192, MREMAP_MAYMOVE, 0, 0])
            .call::<Mremap>()
            .expect("mremap should succeed") as u64;
        assert_user_bytes(remapped_addr, b"mmap");
        assert!(
            process
                .lock()
                .addrspace
                .get_area(x86_64::VirtAddr::new(anon_addr))
                .is_none()
        );
        expect_errno(
            SyscallArgs::new([remapped_addr, 4096, 12288, 0, 0, 0]).call::<Mremap>(),
            SyscallError::NoMemory,
        );
        expect_ok(
            SyscallArgs::new([anon_addr, 0, 0, 0, 0, 0]).call::<Mlock>(),
            0,
        );
        expect_ok(
            SyscallArgs::new([anon_addr, 0, 0, 0, 0, 0]).call::<Munlock>(),
            0,
        );
        assert_eq!(
            SyscallArgs::new([remapped_addr + 1, 4095, 0, 0, 0, 0]).call::<Mlock>(),
            Ok(0),
            "mlock should succeed on unaligned remapped address"
        );
        expect_ok(
            SyscallArgs::new([remapped_addr + 1, 4095, 0, 0, 0, 0]).call::<Munlock>(),
            0,
        );
        expect_errno(
            SyscallArgs::new([anon_addr, 4096, 0, 0, 0, 0]).call::<Mlock>(),
            SyscallError::NoMemory,
        );
        expect_errno(
            SyscallArgs::new([anon_addr, 4096, 0, 0, 0, 0]).call::<Munlock>(),
            SyscallError::NoMemory,
        );
        expect_errno(
            SyscallArgs::new([0x2000_0000, 4096, 0, 0, 0, 0]).call::<Mlock>(),
            SyscallError::NoMemory,
        );
        expect_errno(
            SyscallArgs::new([0x2000_0000, 4096, 0, 0, 0, 0]).call::<Munlock>(),
            SyscallError::NoMemory,
        );
        let old_memlock_limit = process.lock().rlimit_memlock_cur;
        process.lock().rlimit_memlock_cur = 0;
        expect_errno(
            SyscallArgs::new([remapped_addr, 4096, 0, 0, 0, 0]).call::<Mlock>(),
            SyscallError::NoMemory,
        );
        process.lock().rlimit_memlock_cur = old_memlock_limit;

        let mincore_vec = allocate_user_test_page();
        expect_ok(
            SyscallArgs::new([remapped_addr, 4096, mincore_vec, 0, 0, 0]).call::<Mincore>(),
            0,
        );
        assert_ne!(read_user_value::<u8>(mincore_vec), 0);
        expect_errno(
            SyscallArgs::new([remapped_addr + 1, 4096, mincore_vec, 0, 0, 0]).call::<Mincore>(),
            SyscallError::InvalidArguments,
        );
        expect_errno(
            SyscallArgs::new([remapped_addr, 4096, 0, 0, 0, 0]).call::<Mincore>(),
            SyscallError::BadAddress,
        );
        expect_errno(
            SyscallArgs::new([0x2000_0000, 4096, mincore_vec, 0, 0, 0]).call::<Mincore>(),
            SyscallError::NoMemory,
        );

        let mode_addr = mincore_vec + 64;
        let nodemask_addr = mincore_vec + 128;
        write_user_value(mode_addr, &-1i32);
        write_user_value(nodemask_addr, &u64::MAX);
        expect_ok(
            SyscallArgs::new([mode_addr, nodemask_addr, 64, 0, 0, 0]).call::<GetMempolicy>(),
            0,
        );
        assert_eq!(read_user_value::<i32>(mode_addr), 0);
        assert_eq!(read_user_value::<u64>(nodemask_addr), 0);
        write_user_value(mode_addr, &-1i32);
        write_user_value(nodemask_addr, &0u64);
        expect_ok(
            SyscallArgs::new([
                mode_addr,
                nodemask_addr,
                64,
                0,
                GetMempolicyFlags::MEMS_ALLOWED.bits(),
                0,
            ])
            .call::<GetMempolicy>(),
            0,
        );
        assert_eq!(read_user_value::<i32>(mode_addr), -1);
        assert_eq!(read_user_value::<u64>(nodemask_addr), 1);
        write_user_value(mode_addr, &-1i32);
        expect_ok(
            SyscallArgs::new([
                mode_addr,
                nodemask_addr,
                64,
                remapped_addr,
                (GetMempolicyFlags::ADDR | GetMempolicyFlags::NODE).bits(),
                0,
            ])
            .call::<GetMempolicy>(),
            0,
        );
        assert_eq!(read_user_value::<i32>(mode_addr), 0);
        expect_errno(
            SyscallArgs::new([mode_addr, nodemask_addr, 64, remapped_addr, 0, 0])
                .call::<GetMempolicy>(),
            SyscallError::InvalidArguments,
        );
        expect_errno(
            SyscallArgs::new([
                mode_addr,
                nodemask_addr,
                64,
                0,
                GetMempolicyFlags::NODE.bits(),
                0,
            ])
            .call::<GetMempolicy>(),
            SyscallError::InvalidArguments,
        );
        expect_errno(
            SyscallArgs::new([
                mode_addr,
                nodemask_addr,
                64,
                remapped_addr,
                (GetMempolicyFlags::ADDR | GetMempolicyFlags::MEMS_ALLOWED).bits(),
                0,
            ])
            .call::<GetMempolicy>(),
            SyscallError::InvalidArguments,
        );
        expect_errno(
            SyscallArgs::new([
                mode_addr,
                nodemask_addr,
                64,
                0x2000_0000,
                GetMempolicyFlags::ADDR.bits(),
                0,
            ])
            .call::<GetMempolicy>(),
            SyscallError::BadAddress,
        );
        expect_errno(
            SyscallArgs::new([mode_addr, nodemask_addr, 0, 0, 0, 0]).call::<GetMempolicy>(),
            SyscallError::InvalidArguments,
        );
        expect_errno(
            SyscallArgs::new([mode_addr, 1, 64, 0, 0, 0]).call::<GetMempolicy>(),
            SyscallError::BadAddress,
        );
        expect_errno(
            SyscallArgs::new([mode_addr, nodemask_addr, 64, 0, 8, 0]).call::<GetMempolicy>(),
            SyscallError::InvalidArguments,
        );

        let page = allocate_user_test_page();
        write_user_cstr(page, b"/tmp/syscall-mmap-file-test\0");
        let fd = expect_fd(
            SyscallArgs::new([
                AT_FDCWD,
                page,
                (OpenFlags::CREAT | OpenFlags::TRUNC).bits() as u64 | 0o2,
                0o600,
                0,
                0,
            ])
            .call::<OpenAt>(),
        );
        process
            .lock()
            .addrspace
            .write_buffer((page + 128) as *mut u8, b"abcdef")
            .unwrap();
        expect_ok(
            SyscallArgs::new([fd as u64, page + 128, 6, 0, 0, 0]).call::<Write>(),
            6,
        );
        let file_map_addr = SyscallArgs::new([
            0,
            8192,
            (Protection::READ | Protection::WRITE).bits() as u64,
            MAP_SHARED,
            fd as u64,
            0,
        ])
        .call::<Mmap>()
        .expect("file mmap should succeed") as u64;
        process
            .lock()
            .addrspace
            .write_buffer(file_map_addr as *mut u8, b"XYZ")
            .unwrap();
        expect_ok(
            SyscallArgs::new([file_map_addr, 4096, MS_SYNC, 0, 0, 0]).call::<Msync>(),
            0,
        );
        expect_ok(
            SyscallArgs::new([fd as u64, 0, 0, 0, 0, 0]).call::<Lseek>(),
            0,
        );
        process
            .lock()
            .addrspace
            .write_buffer((page + 256) as *mut u8, &[0; 6])
            .unwrap();
        expect_ok(
            SyscallArgs::new([fd as u64, page + 256, 6, 0, 0, 0]).call::<Read>(),
            6,
        );
        assert_user_bytes(page + 256, b"XYZdef");
        expect_ok(
            SyscallArgs::new([file_map_addr, 0, 0, 0, 0, 0]).call::<Msync>(),
            0,
        );
        expect_ok(
            SyscallArgs::new([fd as u64, 8192, 0, 0, 0, 0]).call::<Ftruncate>(),
            0,
        );
        process
            .lock()
            .addrspace
            .write_buffer((file_map_addr + 4096) as *mut u8, b"tail")
            .unwrap();
        expect_ok(
            SyscallArgs::new([file_map_addr, 8192, MS_SYNC, 0, 0, 0]).call::<Msync>(),
            0,
        );
        expect_ok(
            SyscallArgs::new([fd as u64, 4096, 0, 0, 0, 0]).call::<Lseek>(),
            4096,
        );
        process
            .lock()
            .addrspace
            .write_buffer((page + 384) as *mut u8, &[0; 4])
            .unwrap();
        expect_ok(
            SyscallArgs::new([fd as u64, page + 384, 4, 0, 0, 0]).call::<Read>(),
            4,
        );
        assert_user_bytes(page + 384, b"tail");

        let second_file_map_addr = SyscallArgs::new([
            0,
            8192,
            (Protection::READ | Protection::WRITE).bits() as u64,
            MAP_SHARED,
            fd as u64,
            0,
        ])
        .call::<Mmap>()
        .expect("second file mmap should succeed") as u64;
        process
            .lock()
            .addrspace
            .write_buffer((file_map_addr + 16) as *mut u8, b"live")
            .unwrap();
        assert_eq!(
            process
                .lock()
                .addrspace
                .read_buffer((second_file_map_addr + 16) as *const u8, 4)
                .expect("second MAP_SHARED mapping should read first mapping writes"),
            b"live",
            "independent MAP_SHARED mappings of the same file page must share writes before msync"
        );

        expect_errno(
            SyscallArgs::new([file_map_addr + 1, 4096, MS_SYNC, 0, 0, 0]).call::<Msync>(),
            SyscallError::InvalidArguments,
        );
        expect_errno(
            SyscallArgs::new([file_map_addr, 4096, MS_ASYNC | MS_SYNC, 0, 0, 0]).call::<Msync>(),
            SyscallError::InvalidArguments,
        );
        expect_errno(
            SyscallArgs::new([file_map_addr, 4096, MS_INVALIDATE, 0, 0, 0]).call::<Msync>(),
            SyscallError::OperationNotSupported,
        );

        let (forked_process, _) = Process::fork(process.clone());
        process
            .lock()
            .addrspace
            .write_buffer((file_map_addr + 8) as *mut u8, b"shared")
            .unwrap();
        assert_eq!(
            forked_process
                .lock()
                .addrspace
                .read_buffer((file_map_addr + 8) as *const u8, 6)
                .expect("forked child should read the shared mapping"),
            b"shared",
            "MAP_SHARED file mappings must remain shared after fork instead of becoming COW"
        );

        let anonymous_shared_addr = SyscallArgs::new([
            0,
            4096,
            (Protection::READ | Protection::WRITE).bits() as u64,
            MAP_SHARED | MAP_ANONYMOUS,
            u64::MAX,
            0,
        ])
        .call::<Mmap>()
        .expect("anonymous shared mmap should succeed") as u64;
        let (anonymous_shared_child, _) = Process::fork(process.clone());
        anonymous_shared_child
            .lock()
            .addrspace
            .write(anonymous_shared_addr as *mut i32, &252i32)
            .expect("forked child should write anonymous shared mapping");
        assert_eq!(
            process
                .lock()
                .addrspace
                .read::<i32>(anonymous_shared_addr as *const i32)
                .expect("parent should read forked child shared mapping write"),
            252,
            "MAP_SHARED anonymous mappings must remain shared after fork"
        );
        anonymous_shared_child.lock().addrspace.clean();
        assert_eq!(
            process
                .lock()
                .addrspace
                .read::<i32>(anonymous_shared_addr as *const i32)
                .expect("parent should keep anonymous shared mapping after child exit"),
            252,
            "child exit must not release the shared anonymous frame still mapped by the parent"
        );
        expect_ok(
            SyscallArgs::new([anonymous_shared_addr, 4096, 0, 0, 0, 0]).call::<Munmap>(),
            0,
        );

        write_user_cstr(page, b"/dev/zero\0");
        let zero_fd = expect_fd(
            SyscallArgs::new([AT_FDCWD, page, OpenFlags::empty().bits() as u64, 0, 0, 0])
                .call::<OpenAt>(),
        );
        expect_errno(
            SyscallArgs::new([
                0,
                4096,
                Protection::WRITE.bits() as u64,
                MAP_SHARED,
                zero_fd as u64,
                0,
            ])
            .call::<Mmap>(),
            SyscallError::AccessDenied,
        );

        expect_ok(
            SyscallArgs::new([second_file_map_addr, 8192, 0, 0, 0, 0]).call::<Munmap>(),
            0,
        );
        expect_ok(
            SyscallArgs::new([file_map_addr, 8192, 0, 0, 0, 0]).call::<Munmap>(),
            0,
        );
        expect_ok(
            SyscallArgs::new([remapped_addr, 8192, 0, 0, 0, 0]).call::<Munmap>(),
            0,
        );
        assert!(
            process
                .lock()
                .addrspace
                .get_area(x86_64::VirtAddr::new(remapped_addr))
                .is_none()
        );
        close_test_fd(zero_fd);
        close_test_fd(fd);
        let _ = VirtualFS
            .lock()
            .delete_file(Path::new("/tmp/syscall-mmap-file-test"));
    }

    fn typed_syscall_args_convert_flags_and_enums_at_boundary() {
        assert_eq!(<u32 as SyscallArg>::from_u64(u64::MAX).unwrap(), u32::MAX);
        assert!(<bool as SyscallArg>::from_u64(2).unwrap());
        assert_eq!(
            <Signal as SyscallArg>::from_u64(Signal::SIGTERM as u64).unwrap(),
            Signal::SIGTERM
        );
        assert!(matches!(
            <Signal as SyscallArg>::from_u64(0),
            Err(SyscallError::InvalidArguments)
        ));
        assert_eq!(
            <ClockId as SyscallArg>::from_u64(ClockId::Realtime as u64).unwrap(),
            ClockId::Realtime
        );
        assert_eq!(
            <Protection as SyscallArg>::from_u64((Protection::READ | Protection::WRITE).bits())
                .unwrap()
                .bits(),
            (Protection::READ | Protection::WRITE).bits()
        );
        assert_eq!(
            <Signals as SyscallArg>::from_u64(Signal::SIGINT.mask())
                .unwrap()
                .bits(),
            Signals::SIGINT.bits()
        );
        assert_eq!(
            <OpenFlags as SyscallArg>::from_u64(
                (OpenFlags::CLOEXEC | OpenFlags::NONBLOCK).bits() as u64
            )
            .unwrap()
            .bits(),
            (OpenFlags::CLOEXEC | OpenFlags::NONBLOCK).bits()
        );
        assert!(<PollEvents as SyscallArg>::from_u64(PollEvents::POLLIN.bits() as u64).is_ok());
    }
}
