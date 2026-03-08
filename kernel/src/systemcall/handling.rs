use crate::{
    misc::snapshot::Snapshot,
    println, s_println,
    systemcall::{error::SyscallError, syscalls_table::SYSCALL_TABLE},
};

#[unsafe(no_mangle)]
extern "C" fn syscall_handler(snapshot_ptr: *mut Snapshot) {
    unsafe {
        s_println!("actrual rip is {:x}", (*snapshot_ptr).rip);
    }
    let snapshot = unsafe { &mut *snapshot_ptr };

    let result = syscall_handler_unwrapped(
        snapshot.rax as isize,
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
        SyscallError::InvalidSyscall as isize
    }
}
