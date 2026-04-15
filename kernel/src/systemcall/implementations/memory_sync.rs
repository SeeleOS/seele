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

define_syscall!(FutexWait, |arg1: u64, arg2: u64| {
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
});

define_syscall!(FutexWake, |arg1: u64, arg2: u64| {
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
});

define_syscall!(SetGs, { Err(SyscallError::other("setgs unimplemented")) });

define_syscall!(SetFs, |fs: u64| {
    FsBase::write(VirtAddr::new(fs));
    Ok(0)
});

define_syscall!(GetFs, { Ok(FsBase::read().as_u64() as usize) });

define_syscall!(
    AllocateMem,
    |pages: u64, _unused: u64, permissions: Permissions| {
        let current = get_current_process();
        Ok(current
            .lock()
            .addrspace
            .allocate_user_lazy(pages, permissions, Data::Normal)
            .as_u64() as usize)
    }
);

define_syscall!(DeallocateMem, |addr: VirtAddr, len: u64| {
    get_current_process().lock().addrspace.unmap(addr, len);
    Ok(0)
});

define_syscall!(
    UpdateMemPerms,
    |addr: VirtAddr, pages: u64, permissions: Permissions| {
        get_current_process().lock().addrspace.update_permissions(
            addr,
            addr + pages * 4096,
            permissions,
        );
        Ok(0)
    }
);
