use crate::{
    memory::{addrspace::mem_area::Data, protection::Protection},
    process::manager::get_current_process,
    systemcall::utils::{SyscallError, SyscallImpl, SyscallResult},
};

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

pub fn allocate_user_test_page() -> u64 {
    let process = get_current_process();
    let mut process = process.lock();
    process
        .addrspace
        .allocate_user_lazy(1, Protection::READ | Protection::WRITE, Data::Normal)
        .as_u64()
}

pub fn read_user_value<T: Copy>(addr: u64) -> T {
    get_current_process()
        .lock()
        .addrspace
        .read(addr as *const T)
        .expect("test user address should be readable")
}

pub fn write_user_value<T>(addr: u64, value: &T) {
    get_current_process()
        .lock()
        .addrspace
        .write(addr as *mut T, value)
        .expect("test user address should be writable");
}

pub fn assert_user_bytes(addr: u64, expected: &[u8]) {
    let actual = get_current_process()
        .lock()
        .addrspace
        .read_buffer(addr as *const u8, expected.len())
        .expect("test user address should be readable");
    assert_eq!(actual.as_slice(), expected);
}

pub fn user_ptr<T>(value: &T) -> u64 {
    value as *const T as u64
}

pub fn user_mut_ptr<T>(value: &mut T) -> u64 {
    value as *mut T as u64
}
