#![no_std]
#![no_main]
#![feature(abi_x86_interrupt)]

mod common;

use kernel::{
    drivers::pci::enumerate_devices,
    drivers::virtio::block::root_device,
    filesystem::{path::Path, vfs::VirtualFS},
    misc::time::Time,
    net,
    smp::topology,
    systemcall::{numbers::SyscallNumber, table::SYSCALL_TABLE},
};

common::integration_test_entry!(test_main);

fn test_main() {
    assert!(VirtualFS.lock().resolve_dir(Path::new("/")).is_ok());
    assert!(VirtualFS.lock().resolve_dir(Path::new("/proc")).is_ok());
    assert!(VirtualFS.lock().resolve_dir(Path::new("/sys")).is_ok());
    assert!(VirtualFS.lock().resolve_dir(Path::new("/dev")).is_ok());

    assert!(SYSCALL_TABLE[SyscallNumber::Read as usize].is_some());
    assert!(SYSCALL_TABLE[SyscallNumber::Write as usize].is_some());
    assert!(SYSCALL_TABLE[SyscallNumber::Mmap as usize].is_some());

    assert!(Time::current().as_nanoseconds() > 0);
    assert!(!topology::processors().is_empty());
    assert!(!enumerate_devices().is_empty());
    assert!(root_device().is_some());
    let _ = net::interfaces();
}
