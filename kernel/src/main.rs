#![no_std]
#![no_main]
#![feature(abi_x86_interrupt, custom_test_frameworks)]
#![reexport_test_harness_main = "test_main"]
#![test_runner(kernel::testing::run_tests)]

extern crate alloc;

use bootloader_api::{BootInfo, entry_point};
use core::panic::PanicInfo;

#[cfg(test)]
use kernel::misc::debug_exit::debug_exit;
use kernel::{boot::BOOTLOADER_CONFIG, init};

entry_point!(kernel_main, config = &BOOTLOADER_CONFIG);

fn kernel_main(boot_info: &'static mut BootInfo) -> ! {
    #[cfg(test)]
    debug_exit(kernel::misc::debug_exit::QemuExitCode::Success);

    init(boot_info);
}

#[cfg(test)]
#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    use kernel::misc::panic::test_handle_panic;

    test_handle_panic(_info);
}

#[cfg(not(test))]
#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    use kernel::misc::panic::handle_panic;

    handle_panic(_info);
}
