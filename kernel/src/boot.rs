use bootloader_api::{
    BootInfo, BootloaderConfig,
    config::Mapping,
    info::{FrameBuffer, MemoryRegion},
};
use conquer_once::spin::OnceCell;

const KERNEL_STACK_SIZE: u64 = 2 * 1024 * 1024;
const HIGHER_HALF_START: u64 = 0xffff_8000_0000_0000;

pub static BOOTLOADER_CONFIG: BootloaderConfig = {
    let mut config = BootloaderConfig::new_default();
    config.kernel_stack_size = KERNEL_STACK_SIZE;
    config.mappings.physical_memory = Some(Mapping::Dynamic);
    config.mappings.dynamic_range_start = Some(HIGHER_HALF_START);
    config
};

static BOOT_INFO: OnceCell<usize> = OnceCell::uninit();

pub fn init(boot_info: &'static mut BootInfo) {
    BOOT_INFO
        .try_init_once(|| boot_info as *mut BootInfo as usize)
        .expect("boot info already initialized");
}

fn boot_info() -> &'static BootInfo {
    let ptr = *BOOT_INFO.get().expect("boot info missing") as *const BootInfo;
    unsafe { &*ptr }
}

fn boot_info_mut() -> &'static mut BootInfo {
    let ptr = *BOOT_INFO.get().expect("boot info missing") as *mut BootInfo;
    unsafe { &mut *ptr }
}

pub fn physical_memory_offset() -> u64 {
    boot_info()
        .physical_memory_offset
        .into_option()
        .expect("bootloader physical memory offset missing")
}

pub fn memory_map() -> &'static [MemoryRegion] {
    &boot_info().memory_regions
}

pub fn framebuffer() -> &'static mut FrameBuffer {
    boot_info_mut()
        .framebuffer
        .as_mut()
        .expect("bootloader framebuffer missing")
}

pub fn rsdp_address() -> u64 {
    boot_info()
        .rsdp_addr
        .into_option()
        .expect("bootloader rsdp missing")
}
