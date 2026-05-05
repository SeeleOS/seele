#![no_std]
#![no_main]
#![feature(abi_x86_interrupt)]

mod common;

extern crate alloc;

use alloc::{string::String, vec::Vec};

use kernel::filesystem::{errors::FSError, path::Path, vfs::VirtualFS};

common::integration_test_entry!(test_main);

fn test_main() {
    let root = Path::new("/tmp/integration-vfs");
    let nested = Path::new("/tmp/integration-vfs/nested");

    VirtualFS.lock().create_dir(root.clone()).unwrap();
    VirtualFS.lock().create_dir(nested).unwrap();

    let names = VirtualFS
        .lock()
        .list_contents(root)
        .unwrap()
        .into_iter()
        .map(|entry| entry.name)
        .collect::<Vec<_>>();
    assert!(names.contains(&String::from("nested")));

    let trailing_file = Path::new("/tmp/integration-vfs/trailing/");
    let err = VirtualFS.lock().create_file(trailing_file).unwrap_err();
    assert!(matches!(err, FSError::NotADirectory));

    for path in ["/proc", "/sys", "/sys/devices", "/sys/class", "/dev"] {
        assert!(
            VirtualFS.lock().resolve_dir(Path::new(path)).is_ok(),
            "{path}"
        );
    }

    for path in ["/proc", "/sys", "/dev"] {
        assert!(
            VirtualFS.lock().mount_path(Path::new(path)).is_ok(),
            "{path}"
        );
    }
}
