#![no_std]
#![no_main]
#![feature(abi_x86_interrupt)]

mod common;

common::integration_test_entry!(test_main);

fn test_main() {
    panic!("panic handler smoke");
}
