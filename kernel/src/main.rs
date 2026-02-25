#![no_std]
// Disables main function to customize entry point
#![no_main]
#![feature(abi_x86_interrupt, custom_test_frameworks, naked_functions)]
#![reexport_test_harness_main = "test_main"]
#![test_runner(kernel::testing::run_tests)]
// renames main function for testing because we disabled main with #[no_main]
// Disable dynamic linking with the std library because there is no std library in our own os
//
extern crate alloc;

use core::panic::PanicInfo;

use bootloader_api::{BootInfo, entry_point};
#[cfg(test)]
use kernel::debug_exit::debug_exit;
use kernel::driver::keyboard::scancode_processing::process_keypresses;
use kernel::multitasking::MANAGER;
use kernel::multitasking::kernel_task::executor::Executor;
use kernel::multitasking::kernel_task::task::Task;
use kernel::{BOOTLOADER_CONFIG, println};
use kernel::{init, s_println};

entry_point!(k_main, config = &BOOTLOADER_CONFIG);

fn k_main(bootinfo: &'static mut BootInfo) -> ! {
    #[cfg(test)]
    debug_exit(kernel::debug_exit::QemuExitCode::Success);
    s_println!("Welcome  Elysia-OS v0.1.0");

    init(bootinfo);

    let mut executor = Executor::new();

    //executor.spawn(Task::new(init_processes()));
    //executor.spawn(Task::new(taskz()));
    executor.spawn(Task::new(process_keypresses()));
    executor.run();
}

async fn init_processes() {
    MANAGER.lock().init();
}

async fn taskz() {
    println!("println from async task!");
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

fn trigger_syscall() {
    let syscall_number = 1; // write
    let fd = 1;
    let buf = b"Hello from syscall!\n".as_ptr();
    let count = 20;

    unsafe {
        core::arch::asm!(
            "syscall",
            in("rax") syscall_number,
            in("rdi") fd,
            in("rsi") buf,
            in("rdx") count,
            out("rcx") _, // 系统调用会破坏rcx和r11
            out("r11") _,
        );
    }
}
