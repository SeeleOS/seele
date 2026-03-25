#![no_std]
#![no_main]
#![feature(custom_test_frameworks)]
#![test_runner(kernel::testing::run_tests)]
#![reexport_test_harness_main = "test_main"]

extern crate alloc;

use alloc::{
    boxed::Box,
    string::{String, ToString},
    sync::Arc,
    vec::Vec,
};
use bootloader::{BootInfo, entry_point};
use core::panic::PanicInfo;
use crossbeam_queue::ArrayQueue;
use kernel::{
    init, memory::heap::HEAP_SIZE, misc::hlt_loop, panic_handler::test_handle_panic,
    task::task::TaskID, test,
};

entry_point!(main);

fn main(boot_info: &'static BootInfo) -> ! {
    init(boot_info);

    test_main();

    hlt_loop();
}

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    test_handle_panic(info)
}

test!("Box value 1", || assert_eq!(*(Box::new(420)), 420));
test!("Box value 2", || assert_eq!(*(Box::new(69)), 69));

test!("Arc", || {
    Arc::new(ArrayQueue::<TaskID>::new(16));
});

test!("A lot of Boxes", || {
    for i in 0..HEAP_SIZE {
        assert_eq!(*Box::new(i), i);
    }
});

test!("String", || assert_eq!(
    {
        let mut string = String::new();
        string.push_str("seele");
        string.push_str("best");
        string.push_str("girl");
        string
    },
    "seelebestgirl".to_string()
));

test!("Vec", || {
    let mut vec = Vec::new();
    vec.push(11);
    vec.push(45);
    vec.push(14);

    assert_eq!(vec.get(1).unwrap(), &45);
});
