use crate::memory::utils::Mut;
use alloc::{
    collections::{btree_map::BTreeMap, vec_deque::VecDeque},
    sync::Arc,
    vec::Vec,
};
use bitflags::bitflags;
use num_enum::TryFromPrimitive;
use x86_64::structures::paging::{PageSize, Size4KiB};
use x86_64::{VirtAddr, registers::model_specific::FsBase};

use crate::{
    define_syscall,
    memory::{
        addrspace::AddrSpace,
        addrspace::mem_area::{Data, MemoryArea},
        protection::Protection,
        user_safe,
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

fn resized_file_mapping(
    file: Arc<crate::filesystem::object::FileLikeObject>,
    offset: u64,
    pages: u64,
    shared: bool,
) -> Data {
    file.mmap_data(offset, pages, shared)
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

        let data = file_mapping.unwrap_or(Data::Normal);

        current.addrspace.register_area(MemoryArea::new(
            start,
            pages,
            protection_to_page_flags(protection),
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
        return Ok(current
            .lock()
            .addrspace
            .allocate_user_lazy(pages, protection, Data::Normal)
            .as_u64() as usize);
    }

    if !offset.is_multiple_of(4096) || fd < 0 {
        return Err(SyscallError::InvalidArguments);
    }
    let object =
        crate::object::misc::get_object_current_process(fd as u64).map_err(SyscallError::from)?;
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
        Data::Normal => Data::Normal,
        Data::File {
            offset,
            file,
            shared,
            ..
        } => resized_file_mapping(file.clone(), *offset, new_pages, *shared),
        Data::Shared { .. } => return Err(SyscallError::InvalidArguments),
    };
    let new_area = MemoryArea::new(new_start, new_pages, area.flags, new_data, area.lazy);
    current.addrspace.register_area(new_area.clone());

    let copy_pages = match &area.data {
        Data::Normal => old_pages,
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
    let pages = len.div_ceil(4096);
    get_current_process().lock().addrspace.update_permissions(
        addr,
        addr + pages * 4096,
        protection,
    );
    Ok(0)
});

define_syscall!(Mlock, |addr: VirtAddr, len: u64| {
    if len == 0 {
        return Ok(0);
    }

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
        if addrspace.get_area(page_addr).is_none() {
            return Err(SyscallError::NoMemory);
        }
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

    let start = align_down(addr.as_u64(), Size4KiB::SIZE);
    let end = align_up(
        addr.as_u64()
            .checked_add(len)
            .ok_or(SyscallError::InvalidArguments)?,
        Size4KiB::SIZE,
    );
    let start = VirtAddr::new(start);
    let last_page = VirtAddr::new(end - 1);

    let process = get_current_process();
    let mut process = process.lock();
    let addrspace = &mut process.addrspace;
    let mut page_addr = start;
    loop {
        if addrspace.get_area(page_addr).is_none() {
            return Err(SyscallError::NoMemory);
        }
        if page_addr >= last_page {
            break;
        }
        page_addr += Size4KiB::SIZE;
    }

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

#[cfg(test)]
mod tests {
    use crate::systemcall::{
        implementations::Futex,
        test::{TestLinuxTimespec, memory_mapping_syscalls_follow_linux_rules},
        test_helpers::{
            SyscallArgs, allocate_user_test_page, expect_errno, expect_ok, write_user_value,
        },
        utils::SyscallError,
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
