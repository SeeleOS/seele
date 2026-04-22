#![no_std]
#![cfg_attr(test, no_main)]
#![feature(custom_test_frameworks, abi_x86_interrupt)]
#![reexport_test_harness_main = "test_main"]
#![test_runner(crate::misc::testing::run_tests)]

extern crate alloc;

pub const NAME: &str = "Seele";

pub mod acpi;
pub mod drivers;
pub mod elfloader;
pub mod evdev;
pub mod filesystem;
pub mod interrupts;
pub mod keyboard;
pub mod memory;
pub mod misc;
pub mod object;
pub mod polling;
pub mod process;
pub mod smp;
pub mod socket;
pub mod systemcall;
pub mod terminal;
pub mod thread;
pub use misc::signal;

pub static BOOTLOADER_CONFIG: BootloaderConfig = {
    let mut config = BootloaderConfig::new_default();

    config.mappings.physical_memory = Some(Mapping::Dynamic);
    config.mappings.dynamic_range_start = Some(0xffff_8000_0000_0000);

    config
};

use crate::filesystem::vfs::VirtualFS;
use crate::misc::others::enable_sse;
use crate::misc::{framebuffer, logging, mouse, time};
use crate::process::manager::MANAGER;
use crate::smp::{init_bsp, release_application_processors, start_application_processors};
use crate::terminal::misc::clear;
use bootloader_api::BootInfo;
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

pub fn init(bootinfo: &'static mut BootInfo) -> ! {
    memory::init(
        bootinfo.physical_memory_offset.into_option().unwrap(),
        &bootinfo.memory_regions,
    );
    init_bsp();
    framebuffer::init(bootinfo.framebuffer.as_mut().unwrap());
    terminal::init();
    logging::init();
    time::init();
    enable_sse();
    log::info!("init: sse enabled");
    drivers::init();
    log::info!("init: drivers ready");

    VirtualFS.lock().init().unwrap();

    log::info!("init: vfs ready");
    log::info!("init: smp bsp ready");
    systemcall::init();
    log::info!("init: syscall ready");
    acpi::init(bootinfo.rsdp_addr.into_option().unwrap());
    log::info!("init: acpi ready");
    thread::init();
    MANAGER.lock().init();
    log::info!("init: multitasking ready");
    keyboard::init();
    log::info!("init: keyboard ready");
    interrupts::init();
    log::info!("init: interrupts ready");

    log::info!("init: mouse init start");
    mouse::init();
    log::info!("init: mouse init done");
    start_application_processors();
    clear();
    release_application_processors();
    thread::scheduling::run();
}

#[cfg(test)]
#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    use crate::misc::panic::test_handle_panic;

    test_handle_panic(_info);
}
