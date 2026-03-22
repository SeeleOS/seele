// Implement panic handler beacuse the original implementaion is from the std lib, which doesnt
// exist anymore.

use core::panic::PanicInfo;

use x86_64::instructions::interrupts;

use crate::{misc::hlt_loop, println, s_println};

pub fn handle_panic(_info: &PanicInfo) -> ! {
    s_println!("KERNEL_PANIC!!! \n{}", _info);
    println!("KERNEL PANIC!!! \n {}", _info);

    interrupts::disable();

    hlt_loop();
}

pub fn test_handle_panic(_info: &PanicInfo) -> ! {
    use crate::{
        misc::debug_exit::{QemuExitCode, debug_exit},
        s_println,
    };

    s_println!("[FAILED]\n");
    s_println!("{}\n", _info);

    debug_exit(QemuExitCode::Failed);

    hlt_loop();
}
