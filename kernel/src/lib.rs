#![no_std]
#![cfg_attr(test, no_main)]
#![feature(custom_test_frameworks, abi_x86_interrupt)]
#![reexport_test_harness_main = "test_main"]
#![test_runner(crate::testing::run_tests)]

extern crate alloc;

pub mod acpi;
pub mod debug_exit;
pub mod driver;
pub mod exception_interrupt;
pub mod filesystem;
pub mod gdt;
pub mod graphics;
pub mod hardware_interrupt;
pub mod interrupts;
pub mod memory;
pub mod misc;
pub mod multitasking;
pub mod os;
pub mod panic_handler;
pub mod serial_print;
pub mod systemcall;
pub mod testing;
pub mod tss;
pub mod userspace;

pub static BOOTLOADER_CONFIG: BootloaderConfig = {
    let mut config = BootloaderConfig::new_default();

    config.mappings.physical_memory = Some(Mapping::Dynamic);
    config.mappings.dynamic_range_start = Some(0xffff_8000_0000_0000);

    config
};

use crate::misc::others::enable_sse;
use bootloader_api::{BootInfo, entry_point};
use bootloader_api::{BootloaderConfig, config::Mapping};
#[cfg(test)]
use core::panic::PanicInfo;

#[cfg(test)]
entry_point!(test_k_main, config = &BOOTLOADER_CONFIG);

#[cfg(test)]
fn test_k_main(_boot_info: &'static mut BootInfo) -> ! {
    use crate::misc::hlt_loop;

    init(_boot_info);

    test_main();

    hlt_loop();
}

pub fn init(bootinfo: &'static mut BootInfo) {
    enable_sse();
    tss::init();
    memory::init(
        bootinfo.physical_memory_offset.into_option().unwrap(),
        &bootinfo.memory_regions,
    );
    graphics::init(bootinfo.framebuffer.as_mut().unwrap());
    gdt::init();
    misc::init();
    systemcall::init();
    acpi::init();
    multitasking::init();
    interrupts::init();
}

#[cfg(test)]
#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    use crate::panic_handler::test_handle_panic;

    test_handle_panic(_info);
}
