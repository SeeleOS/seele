use alloc::{
    collections::{btree_map::BTreeMap, vec_deque::VecDeque},
    sync::Arc,
    vec::Vec,
};
use bitflags::bitflags;
use num_enum::TryFromPrimitive;
use spin::Mutex;
use x86_64::{VirtAddr, registers::model_specific::FsBase};

use crate::{
    define_syscall,
    memory::{
        addrspace::mem_area::{Data, MemoryArea},
        protection::Protection,
        user_safe,
    },
    misc::others::protection_to_page_flags,
    misc::systemd_perf::{self, PerfBucket},
    misc::time::Time,
    process::manager::get_current_process,
    systemcall::utils::{SyscallError, SyscallImpl},
    thread::{
        THREAD_MANAGER, ThreadRef, get_current_thread,
        manager::ThreadManager,
        yielding::{BlockType, finish_block_current, prepare_block_current},
    },
};

#[derive(Clone, Copy, Debug, TryFromPrimitive)]
#[repr(u64)]
enum FutexOp {
    Wait = 0,
    Wake = 1,
    WaitBitset = 9,
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

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct FutexKey {
    pid: u64,
    addr: u64,
}

static FUTEX_QUEUE: Mutex<BTreeMap<FutexKey, VecDeque<ThreadRef>>> = Mutex::new(BTreeMap::new());
#[repr(C)]
#[derive(Clone, Copy)]
struct LinuxTimespec {
    tv_sec: i64,
    tv_nsec: i64,
}

fn current_futex_key(addr: u64) -> FutexKey {
    let pid = get_current_process().lock().pid.0;
    FutexKey { pid, addr }
}

pub fn wake_futex_for_process(pid: u64, addr: u64, count: usize) -> usize {
    let threads = take_futex_waiters(pid, addr, count);
    let woken = threads.len();

    let mut manager = THREAD_MANAGER.get().unwrap().lock();
    for thread in threads {
        manager.wake(thread);
    }

    woken
}

pub fn wake_futex_for_process_with_manager(
    pid: u64,
    addr: u64,
    count: usize,
    manager: &mut ThreadManager,
) -> usize {
    let threads = take_futex_waiters(pid, addr, count);
    let woken = threads.len();

    for thread in threads {
        manager.wake(thread);
    }

    woken
}

fn take_futex_waiters(pid: u64, addr: u64, count: usize) -> Vec<ThreadRef> {
    let key = FutexKey { pid, addr };
    let mut queue = FUTEX_QUEUE.lock();
    let mut woken = Vec::new();
    let mut remove_key = false;

    if let Some(queue) = queue.get_mut(&key) {
        for _ in 0..count {
            if let Some(thread) = queue.pop_front() {
                woken.push(thread);
            } else {
                break;
            }
        }

        remove_key = queue.is_empty();
    }

    if remove_key {
        queue.remove(&key);
    }

    woken
}

pub fn remove_futex_waiter(thread_ref: &ThreadRef) {
    let mut queue = FUTEX_QUEUE.lock();
    let mut empty_keys = Vec::new();

    for (key, waiters) in queue.iter_mut() {
        waiters.retain(|thread| !Arc::ptr_eq(thread, thread_ref));
        if waiters.is_empty() {
            empty_keys.push(*key);
        }
    }

    for key in empty_keys {
        queue.remove(&key);
    }
}

fn futex_timeout_deadline(timeout: u64) -> Result<Option<Time>, SyscallError> {
    if timeout == 0 {
        return Ok(None);
    }

    let timeout = unsafe { *(timeout as *const LinuxTimespec) };
    if timeout.tv_sec < 0 || !(0..1_000_000_000).contains(&timeout.tv_nsec) {
        return Err(SyscallError::InvalidArguments);
    }

    let timeout_ns = (timeout.tv_sec as u128)
        .saturating_mul(1_000_000_000)
        .saturating_add(timeout.tv_nsec as u128);
    let timeout_ns = timeout_ns.min(u64::MAX as u128) as u64;
    Ok(Some(Time::since_boot().add_ns(timeout_ns)))
}

fn futex_wait_impl(arg1: u64, arg2: u64, timeout: u64) -> Result<usize, SyscallError> {
    let key = current_futex_key(arg1);
    let current = get_current_thread();
    let deadline = futex_timeout_deadline(timeout)?;
    {
        let mut queue = FUTEX_QUEUE.lock();
        let cur_value = unsafe { *(arg1 as *const u32) } as u64;
        if cur_value != arg2 {
            return Err(SyscallError::TryAgain);
        }

        queue.entry(key).or_default().push_back(current.clone());

        // Mark the thread blocked before releasing the futex bucket so a
        // concurrent wake cannot slip between queue insertion and scheduling.
        prepare_block_current(BlockType::Futex { deadline });
    }

    // Do not keep FUTEX_QUEUE locked across scheduling, or FutexWake will
    // deadlock trying to take the same lock from another thread.
    finish_block_current();

    remove_futex_waiter(&current);

    if let Some(deadline) = deadline
        && Time::since_boot() >= deadline
    {
        return Err(SyscallError::TryAgain);
    }

    Ok(0)
}

fn futex_wake_impl(arg1: u64, arg2: u64) -> Result<usize, SyscallError> {
    let key = current_futex_key(arg1);
    let woken = wake_futex_for_process(key.pid, key.addr, arg2 as usize);

    Ok(woken)
}

define_syscall!(Futex, |arg1: u64,
                        op: u64,
                        arg2: u64,
                        timeout: u64,
                        _uaddr2: u64,
                        val3: u64| {
    systemd_perf::profile_current_process(PerfBucket::Futex, || {
        match FutexOp::try_from(op & 0x7f).map_err(|_| SyscallError::InvalidArguments)? {
            FutexOp::Wait => futex_wait_impl(arg1, arg2, timeout),
            FutexOp::Wake => futex_wake_impl(arg1, arg2),
            FutexOp::WaitBitset => {
                if val3 == 0 {
                    return Err(SyscallError::InvalidArguments);
                }
                futex_wait_impl(arg1, arg2, timeout)
            }
        }
    })
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
    let start = VirtAddr::new(addr);
    let end = start + pages * 4096;

    if fixed {
        if addr == 0 || !offset.is_multiple_of(4096) {
            return Err(SyscallError::InvalidArguments);
        }

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
                src_addr.as_u64() as *const u8,
                dst_addr.as_u64() as *mut u8,
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
