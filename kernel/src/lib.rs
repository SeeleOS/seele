#![no_std]
#![cfg_attr(test, no_main)]
#![feature(custom_test_frameworks, abi_x86_interrupt)]
#![reexport_test_harness_main = "test_main"]
#![test_runner(crate::testing::run_tests)]

extern crate alloc;

pub mod acpi;
pub mod exception_interrupt;
pub mod filesystem;
pub mod gdt;
pub mod graphics;
pub mod hardware_interrupt;
pub mod interrupts;
pub mod keyboard;
pub mod memory;
pub mod misc;
pub mod multitasking;
pub mod object;
pub mod os;
pub mod panic_handler;
pub mod serial_print;
pub mod systemcall;
pub mod testing;
pub mod tss;
pub mod userspace;

pub static BOOTLOADER_CONFIG: BootloaderConfig = {
    let mut config = BootloaderConfig::new_default();

    config.mappings.physical_memory = Some(Mapping::Dynamic);
    config.mappings.dynamic_range_start = Some(0xffff_8000_0000_0000);

    config
};

use crate::filesystem::block_device::BlockDevice;
use crate::filesystem::block_device::initrd::{self, RAMDISK};
use crate::filesystem::path::Path;
use crate::filesystem::vfs::VirtualFS;
use crate::misc::others::enable_sse;
use crate::multitasking::kernel_task;
use bootloader_api::BootInfo;
use bootloader_api::{BootloaderConfig, config::Mapping};
#[cfg(test)]
use core::panic::PanicInfo;

#[cfg(test)]
entry_point!(test_k_main, config = &BOOTLOADER_CONFIG);

#[cfg(test)]
fn test_k_main(_boot_info: &'static mut BootInfo) -> ! {
    use crate::misc::hlt_loop;

    init(_boot_info);

    test_main();

    hlt_loop();
}

pub fn init(bootinfo: &'static mut BootInfo) -> ! {
    enable_sse();
    tss::init();
    memory::init(
        bootinfo.physical_memory_offset.into_option().unwrap(),
        &bootinfo.memory_regions,
    );
    graphics::init(bootinfo.framebuffer.as_mut().unwrap());
    gdt::init();
    misc::init();
    systemcall::init();
    acpi::init();
    let mut executor = kernel_task::init();
    multitasking::init();
    interrupts::init();

    initrd::init(
        bootinfo.ramdisk_addr.into_option().expect("No ramdisk."),
        bootinfo.ramdisk_len,
    );

    {
        let ramdisk = RAMDISK.get().unwrap();
        let mut buf = [0u8; 3];

        ramdisk.read_by_bytes(509, &mut buf).unwrap();

        println!("{:?}", buf);
    }

    let mut vfs = VirtualFS.lock();

    vfs.init().unwrap();
    let mut buf = [0u8; 16];
    vfs.read_file(Path::new("/test.txt"), &mut buf).unwrap();
    println!("{:?}", buf);

    executor.run();
}

#[cfg(test)]
#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    use crate::panic_handler::test_handle_panic;

    test_handle_panic(_info);
}
