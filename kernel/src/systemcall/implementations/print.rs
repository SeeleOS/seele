use core::str::from_utf8;

use crate::{
    new_syscall,
    println,
    systemcall::{error::SyscallError, implementations::utils::SyscallImpl, syscall_no::SyscallNo},
};

new_syscall!(PrintImpl, SyscallNo::Print, buf: *const u8, count: usize, empty: u64, |buf: *const u8, count: usize, _empty: u64| -> Result<usize, SyscallError> {
    println!("{}", from_utf8(unsafe { core::slice::from_raw_parts(buf, count) }).unwrap());
    Ok(0)
});
