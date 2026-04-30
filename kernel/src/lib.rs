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
pub mod drm;
pub mod elfloader;
pub mod evdev;
pub mod filesystem;
pub mod ipc;
pub mod interrupts;
pub mod keyboard;
pub mod memory;
pub mod misc;
pub mod net;
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
use crate::misc::{
    agent_tty_input, framebuffer, logging, mouse,
    time::{self, profile_boot_stage},
};
use crate::process::manager::MANAGER;
use crate::smp::{init_bsp, release_application_processors, start_application_processors};
#[cfg(test)]
use core::panic::PanicInfo;

pub fn init() -> ! {
    profile_boot_stage("boot.assert_supported", boot::assert_supported);
    profile_boot_stage("memory.init", || {
        memory::init(boot::physical_memory_offset(), boot::memory_map());
    });
    profile_boot_stage("smp.init_bsp", init_bsp);
    profile_boot_stage("framebuffer.init", || framebuffer::init(boot::framebuffer()));
    profile_boot_stage("terminal.init", terminal::init);
    profile_boot_stage("logging.init", logging::init);
    profile_boot_stage("time.init", time::init);
    profile_boot_stage("enable_sse", enable_sse);
    log::info!("init: sse enabled");
    profile_boot_stage("drivers.init_early", drivers::init_early);
    log::info!("init: early drivers ready");

    profile_boot_stage("vfs.init", || VirtualFS.lock().init().unwrap());

    log::info!("init: vfs ready");
    log::info!("init: smp bsp ready");
    profile_boot_stage("systemcall.init", systemcall::init);
    log::info!("init: syscall ready");
    profile_boot_stage("acpi.init", || acpi::init(boot::rsdp_address()));
    log::info!("init: acpi ready");
    profile_boot_stage("thread.init", thread::init);
    profile_boot_stage("process_manager.init", || MANAGER.lock().init());
    log::info!("init: multitasking ready");
    profile_boot_stage("keyboard.init", keyboard::init);
    log::info!("init: keyboard ready");
    let agent_tty_ready = profile_boot_stage("agent_tty_input.init", agent_tty_input::init);
    if agent_tty_ready {
        log::info!("init: agent background terminal input ready");
    } else {
        log::info!("init: agent background terminal input unavailable");
    }
    profile_boot_stage("interrupts.init", interrupts::init);
    log::info!("init: interrupts ready");
    profile_boot_stage("net.init", net::init);
    profile_boot_stage("drivers.init_late", drivers::init_late);
    log::info!("init: late drivers ready");

    log::info!("init: mouse init start");
    match profile_boot_stage("mouse.init", mouse::init) {
        Ok(()) => log::info!("init: mouse ready"),
        Err(err) => log::warn!("init: mouse unavailable ({err})"),
    }
    profile_boot_stage("smp.start_application_processors", start_application_processors);
    profile_boot_stage(
        "smp.release_application_processors",
        release_application_processors,
    );
    thread::scheduling::run();
}

#[cfg(test)]
#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    use crate::misc::panic::test_handle_panic;

    test_handle_panic(_info);
}
