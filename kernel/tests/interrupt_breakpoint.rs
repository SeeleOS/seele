#![no_std]
#![no_main]
#![feature(abi_x86_interrupt)]

mod common;

common::integration_test_entry!(test_main);

fn test_main() {
    x86_64::instructions::interrupts::int3();
}
