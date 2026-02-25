// Implement panic handler beacuse the original implementaion is from the std lib, which doesnt
// exist anymore.

use core::panic::PanicInfo;

use crate::{misc::hlt_loop, s_println};

pub fn handle_panic(_info: &PanicInfo) -> ! {
    s_println!("{}", _info);

    hlt_loop();
}

pub fn test_handle_panic(_info: &PanicInfo) -> ! {
    use crate::{
        debug_exit::{QemuExitCode, debug_exit},
        s_println,
    };

    s_println!("[FAILED]\n");
    s_println!("{}\n", _info);

    debug_exit(QemuExitCode::Failed);

    hlt_loop();
}
