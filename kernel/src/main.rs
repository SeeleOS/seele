#![no_std]
#![no_main]
#![feature(abi_x86_interrupt, custom_test_frameworks)]
#![reexport_test_harness_main = "test_main"]
#![test_runner(kernel::testing::run_tests)]

extern crate alloc;

use core::panic::PanicInfo;

#[cfg(test)]
use kernel::debug_exit::debug_exit;
use kernel::init;

#[unsafe(no_mangle)]
unsafe extern "C" fn kmain() -> ! {
    #[cfg(test)]
    debug_exit(kernel::debug_exit::QemuExitCode::Success);

    init();
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
