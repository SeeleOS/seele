use core::arch;

use crate::{errors::SyscallError, numbers::SyscallNumber, syscalls::print};

pub type SyscallResult = Result<usize, SyscallError>;

#[inline(always)]
pub fn raw_syscall(
    num: SyscallNumber,
    arg1: u64,
    arg2: u64,
    arg3: u64,
    arg4: u64,
    arg5: u64,
    arg6: u64,
) -> SyscallResult {
    unsafe {
        let mut return_value: isize;
        arch::asm!(
               "syscall",
               in("rax") num as isize,
               in("rdi") arg1,
               in("rsi") arg2,
               in("rdx") arg3,
               in("r10") arg4,
               in("r8") arg5,
               in("r9") arg6,
               out("rcx") _, // syscall 会覆盖 rcx
               out("r11") _, // syscall 会覆盖 r11
               lateout("rax") return_value,
        );
        if return_value >= 0 {
            Ok(return_value as usize)
        } else {
            Err(SyscallError::from(return_value))
        }
    }
}

#[macro_export]
macro_rules! syscall {
    // 匹配 0 到 6 个参数，不足的补 0
    ($n:ident) => {
        $crate::utils::raw_syscall($crate::numbers::SyscallNumber::$n, 0, 0, 0, 0, 0, 0)
    };
    ($n:ident, $a1:expr) => {
        $crate::utils::raw_syscall($crate::numbers::SyscallNumber::$n, $a1, 0, 0, 0, 0, 0)
    };
    ($n:ident, $a1:expr, $a2:expr) => {
        $crate::utils::raw_syscall($crate::numbers::SyscallNumber::$n, $a1, $a2, 0, 0, 0, 0)
    };
    ($n:ident, $a1:expr, $a2:expr, $a3:expr) => {
        $crate::utils::raw_syscall($crate::numbers::SyscallNumber::$n, $a1, $a2, $a3, 0, 0, 0)
    };
    ($n:ident, $a1:expr, $a2:expr, $a3:expr, $a4:expr) => {
        $crate::utils::raw_syscall($crate::numbers::SyscallNumber::$n, $a1, $a2, $a3, $a4, 0, 0)
    };
    ($n:ident, $a1:expr, $a2:expr, $a3:expr, $a4:expr, $a5:expr) => {
        $crate::utils::raw_syscall(
            $crate::numbers::SyscallNumber::$n,
            $a1,
            $a2,
            $a3,
            $a4,
            $a5,
            0,
        )
    };
    ($n:ident, $a1:expr, $a2:expr, $a3:expr, $a4:expr, $a5:expr, $a6:expr) => {
        $crate::utils::raw_syscall(
            $crate::numbers::SyscallNumber::$n,
            $a1,
            $a2,
            $a3,
            $a4,
            $a5,
            $a6,
        )
    };
}
