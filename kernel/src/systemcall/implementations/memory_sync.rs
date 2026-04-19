use alloc::collections::{btree_map::BTreeMap, vec_deque::VecDeque};
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
    process::manager::get_current_process,
    systemcall::utils::{SyscallError, SyscallImpl},
    thread::{
        THREAD_MANAGER, ThreadRef, get_current_thread,
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
    struct MmapFlags: i32 {
        const FIXED = 0x10;
        const ANONYMOUS = 0x20;
        const FIXED_NOREPLACE = 0x100000;
    }
}

bitflags! {
    #[derive(Clone, Copy, Debug)]
    struct MremapFlags: u64 {
        const MAYMOVE = 0x1;
    }
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct FutexKey {
    pid: u64,
    addr: u64,
}

static FUTEX_QUEUE: Mutex<BTreeMap<FutexKey, VecDeque<ThreadRef>>> = Mutex::new(BTreeMap::new());

fn current_futex_key(addr: u64) -> FutexKey {
    let pid = get_current_process().lock().pid.0;
    FutexKey { pid, addr }
}

pub fn wake_futex_for_process(pid: u64, addr: u64, count: usize) -> usize {
    let key = FutexKey { pid, addr };
    let mut queue = FUTEX_QUEUE.lock();
    let mut woken = 0;
    let mut remove_key = false;

    if let Some(queue) = queue.get_mut(&key) {
        for _ in 0..count {
            if let Some(thread) = queue.pop_front() {
                THREAD_MANAGER.get().unwrap().lock().wake(thread);
                woken += 1;
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

fn futex_wait_impl(arg1: u64, arg2: u64) -> Result<usize, SyscallError> {
    let key = current_futex_key(arg1);
    let current = get_current_thread();
    {
        let mut queue = FUTEX_QUEUE.lock();
        let cur_value = unsafe { *(arg1 as *const u32) } as u64;
        if cur_value != arg2 {
            return Err(SyscallError::TryAgain);
        }

        if !queue.contains_key(&key) {
            queue.insert(key, VecDeque::new());
        }

        queue.get_mut(&key).unwrap().push_back(current);

        // Mark the thread blocked before releasing the futex bucket so a
        // concurrent wake cannot slip between queue insertion and scheduling.
        prepare_block_current(BlockType::Futex);
    }

    // Do not keep FUTEX_QUEUE locked across scheduling, or FutexWake will
    // deadlock trying to take the same lock from another thread.
    finish_block_current();

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
                        _timeout: u64,
                        _uaddr2: u64,
                        val3: u64| {
    match FutexOp::try_from(op & 0x7f).map_err(|_| SyscallError::InvalidArguments)? {
        FutexOp::Wait => futex_wait_impl(arg1, arg2),
        FutexOp::Wake => futex_wake_impl(arg1, arg2),
        FutexOp::WaitBitset => {
            if val3 == 0 {
                return Err(SyscallError::InvalidArguments);
            }
            futex_wait_impl(arg1, arg2)
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

fn mapping_overlaps(
    areas: &[crate::memory::addrspace::mem_area::MemoryArea],
    start: VirtAddr,
    end: VirtAddr,
) -> bool {
    areas
        .iter()
        .any(|area| area.start < end && area.end > start)
}

define_syscall!(Mmap, |addr: u64,
                       len: u64,
                       prot: i32,
                       flags: i32,
                       fd: i32,
                       offset: u64| {
    if len == 0 {
        return Err(SyscallError::InvalidArguments);
    }
    let protection = prot_to_protection(prot)?;
    let flags = MmapFlags::from_bits_truncate(flags);
    let pages = len.div_ceil(4096);
    let fixed = flags.intersects(MmapFlags::FIXED | MmapFlags::FIXED_NOREPLACE);
    let start = VirtAddr::new(addr);
    let end = start + pages * 4096;

    if fixed {
        if addr == 0 || offset % 4096 != 0 {
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
            let file_bytes = file
                .info()
                .map(|info| (info.size as u64).saturating_sub(offset).min(pages * 4096))
                .unwrap_or(0);
            Some(Data::File {
                offset,
                file_bytes,
                file,
            })
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

    if offset % 4096 != 0 || fd < 0 {
        return Err(SyscallError::InvalidArguments);
    }
    let object =
        crate::object::misc::get_object_current_process(fd as u64).map_err(SyscallError::from)?;
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
                         flags: u64,
                         _new_addr: u64| {
    if old_len == 0 || new_len == 0 {
        return Err(SyscallError::InvalidArguments);
    }

    let flags = MremapFlags::from_bits_truncate(flags);
    let old_pages = old_len.div_ceil(4096);
    let new_pages = new_len.div_ceil(4096);

    let current = get_current_process();
    let mut current = current.lock();
    let area = current
        .addrspace
        .get_area(old_addr)
        .cloned()
        .ok_or(SyscallError::InvalidArguments)?;

    if area.start != old_addr || !matches!(area.data, Data::Normal) {
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
    current.addrspace.map(MemoryArea::new(
        new_start,
        new_pages,
        area.flags,
        Data::Normal,
        false,
    ));

    unsafe {
        core::ptr::copy_nonoverlapping(
            old_addr.as_u64() as *const u8,
            new_start.as_u64() as *mut u8,
            old_len as usize,
        );
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
