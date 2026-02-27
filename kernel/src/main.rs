#![no_std]
// Disables main function to customize entry point
#![no_main]
#![feature(abi_x86_interrupt, custom_test_frameworks)]
#![reexport_test_harness_main = "test_main"]
#![test_runner(kernel::testing::run_tests)]
// renames main function for testing because we disabled main with #[no_main]
// Disable dynamic linking with the std library because there is no std library in our own os
//
extern crate alloc;

use core::panic::PanicInfo;

use bootloader_api::{BootInfo, entry_point};
use kernel::BOOTLOADER_CONFIG;
#[cfg(test)]
use kernel::debug_exit::debug_exit;
use kernel::multitasking::kernel_task::executor::Executor;
use kernel::multitasking::kernel_task::task::Task;
use kernel::{init, s_println};

entry_point!(k_main, config = &BOOTLOADER_CONFIG);

fn k_main(bootinfo: &'static mut BootInfo) -> ! {
    #[cfg(test)]
    debug_exit(kernel::debug_exit::QemuExitCode::Success);
    s_println!("Welcome  Elysia-OS v0.1.0");

    init(bootinfo);
}

#[cfg(test)]
#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    test_handle_panic(_info);
    use kernel::panic_handler::test_handle_panic;
}

#[cfg(not(test))]
#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    use kernel::panic_handler::handle_panic;

    handle_panic(_info);
}
