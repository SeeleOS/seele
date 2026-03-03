use alloc::boxed::Box;
use x86_64::instructions::hlt;

use crate::{
    memory::paging::MAPPER, misc::others::CpuCoreContext,
    multitasking::memory::allocate_kernel_stack,
};

pub mod aux;
pub mod debug_exit;
pub mod error;
pub mod gdt;
pub mod logging;
pub mod others;
pub mod panic;
pub mod serial_print;
pub mod snapshot;
pub mod stack_builder;
pub mod testing;
pub mod tss;

pub static mut CPU_CORE_CONTEXT: &mut CpuCoreContext = &mut CpuCoreContext {
    gs_kernel_stack_top: 0,
    gs_user_stack_top: 0,
};

pub fn init() {
    unsafe {
        CPU_CORE_CONTEXT = Box::leak(Box::new(CpuCoreContext {
            gs_kernel_stack_top: allocate_kernel_stack(16, &mut MAPPER.get().unwrap().lock())
                .finish()
                .as_u64(),
            gs_user_stack_top: 0,
        }))
    };

    logging::init();
}

pub fn hlt_loop() -> ! {
    loop {
        hlt();
    }
}

#[macro_export]
macro_rules! read_addr {
    ($addr: expr, $type: ty) => {
        *($addr as *mut $type)
    };
}

#[macro_export]
macro_rules! write_addr {
    ($addr: expr, $type: ty, $value: expr) => {
        read_addr!($addr, $type) = $value
    };
}

#[macro_export]
macro_rules! read_port {
    ($port: expr) => {
        Port::new($port).read()
    };
}

#[macro_export]
macro_rules! write_port {
    ($port: expr,$value: expr) => {
        Port::new($port).write($value)
    };
}
