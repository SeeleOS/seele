#![no_std]
// Disables main function to customize entry point
#![no_main]
#![feature(custom_test_frameworks, abi_x86_interrupt)]
// renames main function for testing because we disabled main with #[no_main]
#![reexport_test_harness_main = "test_main"]
#![test_runner(testing::run_tests)]
use bootloader::BootInfo;
use bootloader::entry_point;
use kernel::debug_exit::debug_exit;
use kernel::init;
use kernel::s_print;
use kernel::s_println;
// Disable dynamic linking with the std library because there is no std library in our own os

use core::panic::PanicInfo;

entry_point!(_start);
fn _start(bootinfo: &'static BootInfo) -> ! {
    s_print!("\nStack overflow double-fault handling ");

    init(bootinfo);

    stack_overflow();

    s_println!("[FAILED]\n");
    s_println!("Test continued to run after stack overflow\n");
    debug_exit(kernel::debug_exit::QemuExitCode::Failed);

    loop {}
}

#[allow(unconditional_recursion)]
fn stack_overflow() {
    stack_overflow();
}

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    s_println!("[OK]\n");
    s_println!("Test success!");
    debug_exit(kernel::debug_exit::QemuExitCode::Success);
    loop {}
}
