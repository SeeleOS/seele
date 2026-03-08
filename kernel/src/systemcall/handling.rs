use crate::{
    println, s_println,
    systemcall::{error::SyscallError, syscalls_table::SYSCALL_TABLE},
};

// Repr C to make the assembly code can successfully construct it
#[repr(C)]
struct SyscallSnapshot {
    arg6: u64,
    arg5: u64,
    arg4: u64,
    arg3: u64,         // rdx
    arg2: u64,         // rsi
    arg1: u64,         // rdi
    syscall_no: isize, // rax
    // required for sysret to correctly resume (go back to the previous instruction)
    rflags: u64, // r11
    rip: u64,    // rcx
}

#[unsafe(no_mangle)]
extern "C" fn syscall_handler(snapshot_ptr: *mut SyscallSnapshot) {
    unsafe {
        s_println!("actrual rip is {:x}", (*snapshot_ptr).rip);
    }
    let snapshot = unsafe { &mut *snapshot_ptr };

    let result = syscall_handler_unwrapped(
        snapshot.syscall_no,
        snapshot.arg1,
        snapshot.arg2,
        snapshot.arg3,
        snapshot.arg4,
        snapshot.arg5,
        snapshot.arg6,
    );

    snapshot.syscall_no = result;
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
