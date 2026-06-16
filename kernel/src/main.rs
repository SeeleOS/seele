#![no_std]
#![no_main]
#![feature(abi_x86_interrupt, custom_test_frameworks)]
#![reexport_test_harness_main = "test_main"]
#![test_runner(kernel::testing::run_tests)]

use core::panic::PanicInfo;

extern crate alloc;

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
    kernel::init();
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
