#![no_std]
#![cfg_attr(test, no_main)]
#![feature(custom_test_frameworks, abi_x86_interrupt)]
#![reexport_test_harness_main = "test_main"]
#![test_runner(crate::misc::testing::run_tests)]

extern crate alloc;

pub mod acpi;
pub mod filesystem;
pub mod interrupts;
pub mod keyboard;
pub mod memory;
pub mod misc;
pub mod multitasking;
pub mod object;
pub mod systemcall;
pub mod terminal;
pub mod userspace;

pub static BOOTLOADER_CONFIG: BootloaderConfig = {
    let mut config = BootloaderConfig::new_default();

    config.mappings.physical_memory = Some(Mapping::Dynamic);
    config.mappings.dynamic_range_start = Some(0xffff_8000_0000_0000);

    config
};

use crate::filesystem::block_device::initrd::{self};
use crate::filesystem::path::Path;
use crate::filesystem::vfs::VirtualFS;
use crate::misc::others::enable_sse;
use crate::misc::{framebuffer, gdt, logging, tss};
use crate::multitasking::kernel_task;
use bootloader_api::BootInfo;
use bootloader_api::{BootloaderConfig, config::Mapping};
#[cfg(test)]
use core::panic::PanicInfo;
use log::logger;

#[cfg(test)]
entry_point!(test_k_main, config = &BOOTLOADER_CONFIG);

#[cfg(test)]
fn test_k_main(_boot_info: &'static mut BootInfo) -> ! {
    use crate::misc::hlt_loop;

    init(_boot_info);

    test_main();

    hlt_loop();
}

pub fn init(bootinfo: &'static mut BootInfo) -> ! {
    memory::init(
        bootinfo.physical_memory_offset.into_option().unwrap(),
        &bootinfo.memory_regions,
    );
    framebuffer::init(bootinfo.framebuffer.as_mut().unwrap());
    terminal::init();
    logging::init();
    enable_sse();
    log::info!("init: sse enabled");
    tss::init();
    log::info!("init: tss ready");
    initrd::init(
        bootinfo.ramdisk_addr.into_option().expect("No ramdisk."),
        bootinfo.ramdisk_len,
    );
    log::info!("init: initrd ready");

    VirtualFS.lock().init().unwrap();

    log::info!("init: vfs ready");
    gdt::init();
    log::info!("init: gdt ready");
    misc::init();
    log::info!("init: misc ready");
    systemcall::init();
    log::info!("init: syscall ready");
    acpi::init();
    log::info!("init: acpi ready");
    let mut executor = kernel_task::init();
    log::info!("init: kernel task executor ready");
    multitasking::init();
    log::info!("init: multitasking ready");
    interrupts::init();
    log::info!("init: interrupts ready");

    executor.run();
}

#[cfg(test)]
#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    use crate::misc::panic::test_handle_panic;

    test_handle_panic(_info);
}
