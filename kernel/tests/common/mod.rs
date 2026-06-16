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
        #[used]
        #[unsafe(link_section = ".requests_start_marker")]
        static REQUESTS_START: limine::request::RequestsStartMarker =
            limine::request::RequestsStartMarker::new();

        #[used]
        #[unsafe(link_section = ".requests")]
        static BASE_REVISION: limine::BaseRevision = limine::BaseRevision::new();

        #[used]
        #[unsafe(link_section = ".requests")]
        static ENTRY_POINT_REQUEST: limine::request::EntryPointRequest =
            limine::request::EntryPointRequest::new().with_entry_point(kmain);

        #[used]
        #[unsafe(link_section = ".requests_end_marker")]
        static REQUESTS_END: limine::request::RequestsEndMarker =
            limine::request::RequestsEndMarker::new();

        #[unsafe(no_mangle)]
        extern "C" fn kmain() -> ! {
            assert!(BASE_REVISION.is_supported());
            kernel::init_kernel();
            {
                let mut vfs = kernel::filesystem::vfs::VirtualFS.lock();
                vfs.mount(
                    kernel::filesystem::path::Path::new("/tmp"),
                    kernel::filesystem::tmpfs::TmpFs::new(),
                )
                .expect("failed to mount test tmpfs");
                vfs.mount(
                    kernel::filesystem::path::Path::new("/run"),
                    kernel::filesystem::tmpfs::TmpFs::new(),
                )
                .expect("failed to mount test runfs");
                vfs.mount(
                    kernel::filesystem::path::Path::new("/proc"),
                    kernel::filesystem::procfs::ProcFs::new(),
                )
                .expect("failed to mount test procfs");
                vfs.mount(
                    kernel::filesystem::path::Path::new("/sys"),
                    kernel::filesystem::sysfs::SysFs::new(),
                )
                .expect("failed to mount test sysfs");
                vfs.mount(
                    kernel::filesystem::path::Path::new("/sys/fs/cgroup"),
                    kernel::filesystem::cgroupfs::CgroupFs::new(),
                )
                .expect("failed to mount test cgroupfs");
                vfs.mount(
                    kernel::filesystem::path::Path::new("/dev"),
                    kernel::filesystem::devfs::DevFs::new(),
                )
                .expect("failed to mount test devfs");
                vfs.mount(
                    kernel::filesystem::path::Path::new("/dev/pts"),
                    kernel::filesystem::devfs::DevPtsFs::new(),
                )
                .expect("failed to mount test devpts");
                vfs.mount(
                    kernel::filesystem::path::Path::new("/dev/shm"),
                    kernel::filesystem::tmpfs::TmpFs::new(),
                )
                .expect("failed to mount test shmfs");
            }
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
