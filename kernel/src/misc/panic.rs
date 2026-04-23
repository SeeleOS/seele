// Implement panic handler beacuse the original implementaion is from the std lib, which doesnt
// exist anymore.

use core::panic::PanicInfo;

use x86_64::instructions::interrupts;

use crate::{misc::hlt_loop, s_println, terminal::state::DEFAULT_TERMINAL};

pub fn handle_panic(_info: &PanicInfo) -> ! {
    s_println!("KERNEL_PANIC!!! \n{}", _info);
    if let Some(terminal) = DEFAULT_TERMINAL.get() {
        use crate::object::traits::Writable;
        use alloc::format;

        let _ = terminal
            .lock()
            .write(format!("KERNEL PANIC!!! \n {_info}").as_bytes());
    }

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
