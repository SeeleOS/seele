use alloc::collections::{btree_map::BTreeMap, vec_deque::VecDeque};
use seele_sys::permission::Permissions;
use spin::Mutex;
use x86_64::{VirtAddr, registers::model_specific::FsBase};

use crate::{
    define_syscall,
    process::{
        ProcessRef,
        manager::{MANAGER, get_current_process},
    },
    s_print,
    systemcall::utils::{SyscallError, SyscallImpl},
};

static FUTEX_QUEUE: Mutex<BTreeMap<u64, VecDeque<ProcessRef>>> = Mutex::new(BTreeMap::new());

define_syscall!(FutexWait, |arg1: u64, arg2: u64| {
    let mut queue = FUTEX_QUEUE.lock();
    let cur_value = unsafe { *(arg1 as *mut u64) };
    if cur_value != arg2 {
        return Err(SyscallError::TryAgain);
    }

    if !queue.contains_key(&arg1) {
        queue.insert(arg1, VecDeque::new());
    }

    queue
        .get_mut(&arg1)
        .unwrap()
        .push_back(MANAGER.lock().current.clone().unwrap());
    Ok(0)
});

define_syscall!(FutexWake, |arg1: u64, arg2: u64| {
    let mut queue = FUTEX_QUEUE.lock();
    let mut woken = 0;

    if let Some(queue) = queue.get_mut(&arg1) {
        for _ in 0..arg2 {
            if let Some(_process) = queue.pop_front() {
                log::warn!("[TODO] Futex wake not implemented");
                woken += 1;
            } else {
                break;
            }
        }
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
            .allocate_user_lazy(pages, permissions)
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
