#![no_std]
#![no_main]
#![feature(abi_x86_interrupt)]

use bootloader_api::{BootInfo, entry_point};
use core::panic::PanicInfo;
use kernel::{
    boot::BOOTLOADER_CONFIG,
    init_kernel,
    misc::{
        debug_exit::{QemuExitCode, debug_exit},
        hlt_loop,
    },
    s_println,
};

entry_point!(stack_overflow_kernel_main, config = &BOOTLOADER_CONFIG);

fn stack_overflow_kernel_main(boot_info: &'static mut BootInfo) -> ! {
    init_kernel(boot_info);
    stack_overflow();
    debug_exit(QemuExitCode::Failed);
    hlt_loop();
}

#[allow(unconditional_recursion)]
fn stack_overflow() {
    stack_overflow();
    core::hint::black_box(());
}

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    s_println!("stack overflow reached panic path");
    debug_exit(QemuExitCode::Success);
    hlt_loop();
}
