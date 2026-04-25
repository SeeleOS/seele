#![no_std]
#![cfg_attr(test, no_main)]
#![feature(custom_test_frameworks, abi_x86_interrupt)]
#![reexport_test_harness_main = "test_main"]
#![test_runner(crate::misc::testing::run_tests)]

extern crate alloc;

pub const NAME: &str = "Seele";

pub mod acpi;
pub mod boot;
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

use crate::filesystem::vfs::VirtualFS;
use crate::misc::others::enable_sse;
use crate::misc::{agent_tty_input, framebuffer, logging, mouse, time};
use crate::process::manager::MANAGER;
use crate::smp::{init_bsp, release_application_processors, start_application_processors};
use crate::terminal::misc::clear;
#[cfg(test)]
use core::panic::PanicInfo;

pub fn init() -> ! {
    boot::assert_supported();
    memory::init(boot::physical_memory_offset(), boot::memory_map());
    init_bsp();
    framebuffer::init(boot::framebuffer());
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
    acpi::init(boot::rsdp_address());
    log::info!("init: acpi ready");
    thread::init();
    MANAGER.lock().init();
    log::info!("init: multitasking ready");
    keyboard::init();
    log::info!("init: keyboard ready");
    agent_tty_input::init();
    log::info!("init: agent tty input ready");
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
