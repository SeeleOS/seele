#![no_std]
#![cfg_attr(test, no_main)]
#![feature(custom_test_frameworks, abi_x86_interrupt)]
#![reexport_test_harness_main = "test_main"]
#![test_runner(crate::misc::testing::run_tests)]

extern crate alloc;

#[cfg(test)]
use bootloader_api::entry_point;
pub const NAME: &str = "Seele";

pub mod acpi;
pub mod boot;
pub mod drivers;
pub mod drm;
pub mod elfloader;
pub mod evdev;
pub mod filesystem;
pub mod interrupts;
pub mod ipc;
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
#[cfg(test)]
pub use misc::testing;

#[cfg(test)]
use crate::boot::BOOTLOADER_CONFIG;
use crate::filesystem::vfs::VirtualFS;
use crate::misc::others::enable_sse;
use crate::misc::{agent_tty_input, framebuffer, logging, mouse, time};
use crate::process::manager::MANAGER;
use crate::smp::{init_bsp, release_application_processors, start_application_processors};
use bootloader_api::BootInfo;
#[cfg(test)]
use core::panic::PanicInfo;

#[cfg(test)]
entry_point!(test_kernel_main, config = &BOOTLOADER_CONFIG);

#[cfg(test)]
fn test_kernel_main(boot_info: &'static mut BootInfo) -> ! {
    init_kernel(boot_info);
    test_main();
    unreachable!("test_main returned");
}

pub fn init_kernel(boot_info: &'static mut BootInfo) {
    boot::init(boot_info);
    memory::init(boot::physical_memory_offset(), boot::memory_map());
    init_bsp();
    framebuffer::init(boot::framebuffer());
    terminal::init();
    logging::init();
    time::init();
    enable_sse();
    log::info!("init: sse enabled");
    drivers::init_early();
    log::info!("init: early drivers ready");

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
    let agent_tty_ready = agent_tty_input::init();
    if agent_tty_ready {
        log::info!("init: agent background terminal input ready");
    } else {
        log::info!("init: agent background terminal input unavailable");
    }
    interrupts::init();
    log::info!("init: interrupts ready");
    net::init();
    drivers::init_late();
    log::info!("init: late drivers ready");

    log::info!("init: mouse init start");
    match mouse::init() {
        Ok(()) => log::info!("init: mouse ready"),
        Err(err) => log::warn!("init: mouse unavailable ({err})"),
    }
    start_application_processors();
    release_application_processors();
}

pub fn init(boot_info: &'static mut BootInfo) -> ! {
    init_kernel(boot_info);
    thread::scheduling::run();
}

#[cfg(test)]
#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    use crate::misc::panic::test_handle_panic;

    test_handle_panic(_info);
}

#[cfg(test)]
mod tests {
    crate::test!("kernel test harness", || {
        assert_eq!(2 + 2, 4);
    });
}
