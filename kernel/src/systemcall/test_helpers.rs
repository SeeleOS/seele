use crate::systemcall::utils::{SyscallError, SyscallImpl, SyscallResult};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SyscallArgs(pub [u64; 6]);

impl SyscallArgs {
    pub const fn new(args: [u64; 6]) -> Self {
        Self(args)
    }

    pub const fn none() -> Self {
        Self([0; 6])
    }

    pub fn call<T: SyscallImpl>(self) -> SyscallResult {
        let [arg1, arg2, arg3, arg4, arg5, arg6] = self.0;
        T::handle_call(arg1, arg2, arg3, arg4, arg5, arg6)
    }
}

pub fn expect_ok(result: SyscallResult, expected: usize) {
    assert_eq!(result, Ok(expected));
}

pub fn expect_errno(result: SyscallResult, expected: SyscallError) {
    assert_eq!(result, Err(expected));
}

pub fn errno_code(error: SyscallError) -> isize {
    error.as_isize()
}

pub fn assert_linux_layout<T>(expected_size: usize, expected_align: usize) {
    assert_eq!(core::mem::size_of::<T>(), expected_size);
    assert_eq!(core::mem::align_of::<T>(), expected_align);
}

pub fn user_ptr<T>(value: &T) -> u64 {
    value as *const T as u64
}

pub fn user_mut_ptr<T>(value: &mut T) -> u64 {
    value as *mut T as u64
}
