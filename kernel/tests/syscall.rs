#![no_std]
#![no_main]
#![feature(abi_x86_interrupt)]

mod common;

use kernel::{
    memory::protection::Protection,
    systemcall::{arg_types::SyscallArg, numbers::SyscallNumber, table::SYSCALL_TABLE},
};

common::integration_test_entry!(test_main);

fn test_main() {
    for number in [
        SyscallNumber::Read,
        SyscallNumber::Write,
        SyscallNumber::OpenAt,
        SyscallNumber::Mmap,
        SyscallNumber::Munmap,
        SyscallNumber::ClockGettime,
        SyscallNumber::Socket,
        SyscallNumber::Exit,
    ] {
        assert!(SYSCALL_TABLE[number as usize].is_some());
    }

    assert!(Protection::from_u64((Protection::READ | Protection::WRITE).bits()).is_ok());
    assert!(bool::from_u64(1).unwrap());
    assert_eq!(u32::from_u64(42).unwrap(), 42);
}
