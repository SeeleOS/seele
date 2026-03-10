use crate::{
    misc::snapshot::Snapshot,
    multitasking::thread::THREAD_MANAGER,
    println, s_println,
    systemcall::{error::SyscallError, table::SYSCALL_TABLE},
};

#[unsafe(no_mangle)]
extern "C" fn syscall_handler(snapshot_ptr: *mut Snapshot) {
    let snapshot = unsafe { &mut *snapshot_ptr };

    THREAD_MANAGER
        .get()
        .unwrap()
        .lock()
        .current
        .clone()
        .unwrap()
        .lock()
        .snapshot
        .inner = *snapshot;

    let result = syscall_handler_unwrapped(
        snapshot.rax,
        snapshot.rdi,
        snapshot.rsi,
        snapshot.rdx,
        snapshot.r10,
        snapshot.r8,
        snapshot.r9,
    );

    snapshot.rax = result;
}

fn syscall_handler_unwrapped(
    syscall_no: isize,
    arg1: u64,
    arg2: u64,
    arg3: u64,
    arg4: u64,
    arg5: u64,
    arg6: u64,
) -> isize {
    if let Some(Some(handler)) = SYSCALL_TABLE.get(syscall_no as usize) {
        match handler(arg1, arg2, arg3, arg4, arg5, arg6) {
            Ok(value) => value as isize,
            Err(err) => err as isize,
        }
    } else {
        println!("Attempted to call invalid syscall {}", syscall_no);
        SyscallError::NoSyscall as isize
    }
}
