use alloc::collections::{btree_map::BTreeMap, vec_deque::VecDeque};
use seele_sys::permission::Permissions;
use spin::Mutex;
use x86_64::{VirtAddr, registers::model_specific::FsBase};

use crate::{
    define_syscall,
    memory::addrspace::mem_area::Data,
    process::{manager::get_current_process},
    s_println,
    systemcall::utils::{SyscallError, SyscallImpl},
    thread::{
        THREAD_MANAGER, ThreadRef, get_current_thread,
        yielding::{BlockType, finish_block_current, prepare_block_current},
    },
};

const FUTEX_WAIT: u64 = 0;
const FUTEX_WAKE: u64 = 1;
const ARCH_SET_FS: u64 = 0x1002;
const ARCH_GET_FS: u64 = 0x1003;
const PROT_READ: i32 = 0x1;
const PROT_WRITE: i32 = 0x2;
const PROT_EXEC: i32 = 0x4;
const MAP_FIXED: i32 = 0x10;
const MAP_ANONYMOUS: i32 = 0x20;
const MAP_FIXED_NOREPLACE: i32 = 0x100000;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct FutexKey {
    pid: u64,
    addr: u64,
}

static FUTEX_QUEUE: Mutex<BTreeMap<FutexKey, VecDeque<ThreadRef>>> = Mutex::new(BTreeMap::new());
const DEADLOCK_LOG: bool = false;

fn current_futex_key(addr: u64) -> FutexKey {
    let pid = get_current_process().lock().pid.0;
    FutexKey { pid, addr }
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

        queue
            .get_mut(&key)
            .unwrap()
            .push_back(current);

        if DEADLOCK_LOG {
            let len = queue.get(&key).map(|bucket| bucket.len()).unwrap_or(0);
            s_println!(
                "futex_wait block: pid={} addr={:#x} value={} queued={}",
                key.pid,
                key.addr,
                arg2,
                len
            );
        }

        // Mark the thread blocked before releasing the futex bucket so a
        // concurrent wake cannot slip between queue insertion and scheduling.
        prepare_block_current(BlockType::Futex);
    }

    // Do not keep FUTEX_QUEUE locked across scheduling, or FutexWake will
    // deadlock trying to take the same lock from another thread.
    finish_block_current();

    if DEADLOCK_LOG {
        s_println!("futex_wait resume: pid={} addr={:#x}", key.pid, key.addr);
    }

    Ok(0)
}

fn futex_wake_impl(arg1: u64, arg2: u64) -> Result<usize, SyscallError> {
    let key = current_futex_key(arg1);
    let mut queue = FUTEX_QUEUE.lock();
    let mut woken = 0;
    let mut remove_key = false;

    if let Some(queue) = queue.get_mut(&key) {
        for _ in 0..arg2 {
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

    if DEADLOCK_LOG && woken > 0 {
        s_println!(
            "futex_wake: pid={} addr={:#x} requested={} woke={}",
            key.pid,
            key.addr,
            arg2,
            woken
        );
    }

    Ok(woken)
}

define_syscall!(Futex, |arg1: u64, op: u64, arg2: u64| {
    match op & 0x7f {
        FUTEX_WAIT => futex_wait_impl(arg1, arg2),
        FUTEX_WAKE => futex_wake_impl(arg1, arg2),
        _ => Err(SyscallError::InvalidArguments),
    }
});

define_syscall!(ArchPrctl, |code: u64, addr: u64| {
    match code {
        ARCH_SET_FS => {
            FsBase::write(VirtAddr::new(addr));
            Ok(0)
        }
        ARCH_GET_FS => unsafe {
            *(addr as *mut u64) = FsBase::read().as_u64();
            Ok(0)
        },
        _ => Err(SyscallError::InvalidArguments),
    }
});

fn prot_to_permissions(prot: i32) -> Result<Permissions, SyscallError> {
    let mut permissions = Permissions::empty();
    if (prot & PROT_READ) != 0 {
        permissions |= Permissions::READABLE;
    }
    if (prot & PROT_WRITE) != 0 {
        permissions |= Permissions::WRITABLE;
    }
    if (prot & PROT_EXEC) != 0 {
        permissions |= Permissions::EXECUTABLE;
    }
    Ok(permissions)
}

define_syscall!(Mmap, |addr: u64, len: u64, prot: i32, flags: i32, fd: i32, offset: u64| {
    if len == 0 {
        return Err(SyscallError::InvalidArguments);
    }
    if addr != 0 || (flags & (MAP_FIXED | MAP_FIXED_NOREPLACE)) != 0 {
        return Err(SyscallError::NoSyscall);
    }
    let permissions = prot_to_permissions(prot)?;
    let pages = len.div_ceil(4096);

    if (flags & MAP_ANONYMOUS) != 0 {
        let current = get_current_process();
        return Ok(current
            .lock()
            .addrspace
            .allocate_user_lazy(pages, permissions, Data::Normal)
            .as_u64() as usize);
    }

    if offset % 4096 != 0 || fd < 0 {
        return Err(SyscallError::InvalidArguments);
    }
    let object =
        crate::object::misc::get_object_current_process(fd as u64).map_err(SyscallError::from)?;
    let object = object.as_mappable()?;
    let address = object.map(offset, pages, permissions)?;
    Ok(address.as_u64() as usize)
});

define_syscall!(Munmap, |addr: VirtAddr, len: u64| {
    get_current_process().lock().addrspace.unmap(addr, len);
    Ok(0)
});

define_syscall!(Mprotect, |addr: VirtAddr, len: u64, prot: i32| {
    let permissions = prot_to_permissions(prot)?;
    let pages = len.div_ceil(4096);
    get_current_process()
        .lock()
        .addrspace
        .update_permissions(addr, addr + pages * 4096, permissions);
    Ok(0)
});
