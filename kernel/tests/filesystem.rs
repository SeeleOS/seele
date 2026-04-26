#![no_std]
// Disables main function to customize entry point
#![no_main]
#![feature(abi_x86_interrupt, custom_test_frameworks)]
#![reexport_test_harness_main = "test_main"]
#![test_runner(kernel::testing::run_tests)]

use core::panic::PanicInfo;

use bootloader::{BootInfo, entry_point};
use kernel::{
    debug_exit::debug_exit, init, misc::hlt_loop, panic_handler::test_handle_panic, s_println,
};

entry_point!(k_main);

fn k_main(bootinfo: &'static BootInfo) -> ! {
    init(bootinfo);

    test_main();

    s_println!("\nSomething went wrong. Process contiuned after test");
    debug_exit(kernel::debug_exit::QemuExitCode::Failed);

    hlt_loop();
}

extern crate alloc;

use alloc::string::ToString;

use kernel::{
    filesystem::{
        errors::FSError,
        path::Path,
        vfs::{FileData, VirtualFS},
    },
    test,
};

test!("VFS Basic", || {
    let a_txt = Path::new("/test/vfs_create.txt");
    VirtualFS.lock().create_file(a_txt.clone()).unwrap();
    VirtualFS
        .lock()
        .write_file(
            a_txt.clone(),
            FileData {
                content: "abc".to_string(),
            },
        )
        .unwrap();
    let content = VirtualFS.lock().read_file(a_txt.clone()).unwrap().content;

    assert_eq!(content, "abc");
});

test!("VFS Create Dir", || {
    let a_txt = Path::new("/test/vfs_dir");
    VirtualFS.lock().create_dir(a_txt.clone()).unwrap();
});

test!("VFS Reject File Create With Trailing Slash", || {
    let path = Path::new("/test/vfs_file_slash/");
    let err = VirtualFS.lock().create_file(path).unwrap_err();
    assert_eq!(err, FSError::NotADirectory);
});

test!("VFS List Contents", || {
    let a = Path::new("/tests/dir/a.txt");
    let b = Path::new("/tests/dir/b.txt");
    let c = Path::new("/tests/dir/c.txt");
    VirtualFS.lock().create_file(a.clone()).unwrap();
    VirtualFS.lock().create_file(b.clone()).unwrap();
    VirtualFS.lock().create_file(c.clone()).unwrap();
    VirtualFS.lock().write_file(
        a,
        FileData {
            content: "gwergwegf".to_string(),
        },
    );

    let content = VirtualFS
        .lock()
        .list_contents(Path::new("/tests/dir"))
        .unwrap();

    assert!({
        content.contains(&"a.txt".to_string())
            && content.contains(&"b.txt".to_string())
            && content.contains(&"c.txt".to_string())
    });
});
#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    test_handle_panic(info)
}
