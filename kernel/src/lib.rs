#![no_std]
#![cfg_attr(test, no_main)]
#![feature(custom_test_frameworks, abi_x86_interrupt)]
#![reexport_test_harness_main = "test_main"]
#![test_runner(crate::misc::testing::run_tests)]

extern crate alloc;

pub const NAME: &str = "Seele";
pub const SMP_ENABLED: bool = false;

pub mod acpi;
pub mod boot;
pub mod drivers;
pub mod drm;
pub mod elfloader;
pub mod evdev;
pub mod filesystem;
pub mod interrupts;
pub mod ipc;
pub mod keyboard;
pub mod linux_kpi;
pub mod memory;
pub mod misc;
pub mod net;
pub mod object;
pub mod polling;
pub mod process;
pub mod smp;
pub mod socket;
pub mod systemcall;
pub mod terminal;
pub mod thread;
pub use misc::signal;
#[cfg(test)]
pub use misc::testing;

use crate::filesystem::vfs::VirtualFS;
#[cfg(test)]
use crate::filesystem::{
    cgroupfs::CgroupFs,
    devfs::{DevFs, DevPtsFs},
    path::Path,
    procfs::ProcFs,
    sysfs::SysFs,
    tmpfs::TmpFs,
};
use crate::misc::others::enable_sse;
use crate::misc::{framebuffer, logging, mouse, profile, time};
use crate::process::manager::MANAGER;
use crate::smp::{init_bsp, release_application_processors, start_application_processors};
#[cfg(test)]
use core::panic::PanicInfo;
#[cfg(test)]
use limine::{
    BaseRevision,
    request::{EntryPointRequest, RequestsEndMarker, RequestsStartMarker},
};

#[cfg(test)]
#[used]
#[unsafe(link_section = ".requests_start_marker")]
static REQUESTS_START: RequestsStartMarker = RequestsStartMarker::new();
#[cfg(test)]
#[used]
#[unsafe(link_section = ".requests")]
static BASE_REVISION: BaseRevision = BaseRevision::new();
#[cfg(test)]
#[used]
#[unsafe(link_section = ".requests")]
static ENTRY_POINT_REQUEST: EntryPointRequest = EntryPointRequest::new().with_entry_point(kmain);
#[cfg(test)]
#[used]
#[unsafe(link_section = ".requests_end_marker")]
static REQUESTS_END: RequestsEndMarker = RequestsEndMarker::new();
#[cfg(test)]
#[unsafe(no_mangle)]
extern "C" fn kmain() -> ! {
    assert!(BASE_REVISION.is_supported());
    init_kernel();
    init_test_filesystems();
    test_main();
    unreachable!("test_main returned");
}

#[cfg(test)]
fn init_test_filesystems() {
    let mut vfs = VirtualFS.lock();
    vfs.mount(Path::new("/tmp"), TmpFs::new())
        .expect("failed to mount test tmpfs");
    vfs.mount(Path::new("/run"), TmpFs::new())
        .expect("failed to mount test runfs");
    vfs.mount(Path::new("/proc"), ProcFs::new())
        .expect("failed to mount test procfs");
    vfs.mount(Path::new("/sys"), SysFs::new())
        .expect("failed to mount test sysfs");
    vfs.mount(Path::new("/sys/fs/cgroup"), CgroupFs::new())
        .expect("failed to mount test cgroupfs");
    vfs.mount(Path::new("/dev"), DevFs::new())
        .expect("failed to mount test devfs");
    vfs.mount(Path::new("/dev/pts"), DevPtsFs::new())
        .expect("failed to mount test devpts");
    vfs.mount(Path::new("/dev/shm"), TmpFs::new())
        .expect("failed to mount test shmfs");
}

pub fn init_kernel() {
    boot::init();
    memory::init(boot::physical_memory_offset(), boot::memory_map());
    init_bsp();
    framebuffer::init(boot::framebuffer());
    terminal::init();
    logging::init();
    time::init();
    profile::init();
    enable_sse();
    log::info!("init: extended state enabled");
    drivers::init_early();
    log::info!("init: early drivers ready");

    VirtualFS.lock().init().unwrap();

    log::info!("init: vfs ready");
    log::info!("init: smp bsp ready");
    systemcall::init();
    log::info!("init: syscall ready");
    acpi::init(boot::rsdp_address());
    log::info!("init: acpi ready");
    thread::init();
    MANAGER.lock().init();
    log::info!("init: multitasking ready");
    keyboard::init();
    log::info!("init: keyboard ready");
    interrupts::init();
    log::info!("init: interrupts ready");
    net::init();
    drivers::init_late();
    log::info!("init: late drivers ready");

    log::info!("init: mouse init start");
    match mouse::init() {
        Ok(()) => log::info!("init: mouse ready"),
        Err(err) => log::warn!("init: mouse unavailable ({err})"),
    }
    start_application_processors();
    release_application_processors();
}

pub fn init() -> ! {
    init_kernel();
    thread::scheduling::run();
}

#[cfg(test)]
#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    use crate::misc::panic::test_handle_panic;

    test_handle_panic(_info);
}

#[cfg(test)]
mod tests {
    crate::test!("kernel test harness", || {
        assert_eq!(2 + 2, 4);
    });
}
