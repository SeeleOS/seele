use core::panic::PanicInfo;

use kernel::{
    misc::{
        debug_exit::{QemuExitCode, debug_exit},
        hlt_loop,
    },
    s_println,
};

pub fn pass() -> ! {
    debug_exit(QemuExitCode::Success);
    hlt_loop();
}

pub fn fail() -> ! {
    debug_exit(QemuExitCode::Failed);
    hlt_loop();
}

pub fn handle_panic(info: &PanicInfo) -> ! {
    s_println!("integration test failed: {}", info);
    fail();
}

macro_rules! integration_test_entry {
    ($main:path) => {
        bootloader_api::entry_point!(
            integration_kernel_main,
            config = &kernel::boot::BOOTLOADER_CONFIG
        );

        fn integration_kernel_main(boot_info: &'static mut bootloader_api::BootInfo) -> ! {
            kernel::init_kernel(boot_info);
            $main();
            $crate::common::pass();
        }

        #[panic_handler]
        fn panic(info: &core::panic::PanicInfo) -> ! {
            $crate::common::handle_panic(info)
        }
    };
}

pub(crate) use integration_test_entry;
