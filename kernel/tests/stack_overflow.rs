#![no_std]
#![no_main]
#![feature(abi_x86_interrupt)]

use core::panic::PanicInfo;
use kernel::{
    init_kernel,
    misc::{
        debug_exit::{QemuExitCode, debug_exit},
        hlt_loop,
    },
    s_println,
};
use limine::{
    BaseRevision,
    request::{EntryPointRequest, RequestsEndMarker, RequestsStartMarker},
};

#[used]
#[unsafe(link_section = ".requests_start_marker")]
static REQUESTS_START: RequestsStartMarker = RequestsStartMarker::new();

#[used]
#[unsafe(link_section = ".requests")]
static BASE_REVISION: BaseRevision = BaseRevision::new();

#[used]
#[unsafe(link_section = ".requests")]
static ENTRY_POINT_REQUEST: EntryPointRequest = EntryPointRequest::new().with_entry_point(kmain);

#[used]
#[unsafe(link_section = ".requests_end_marker")]
static REQUESTS_END: RequestsEndMarker = RequestsEndMarker::new();

#[unsafe(no_mangle)]
extern "C" fn kmain() -> ! {
    assert!(BASE_REVISION.is_supported());
    init_kernel();
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
