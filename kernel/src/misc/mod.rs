use alloc::boxed::Box;
use x2apic::lapic::LocalApicBuilder;
use x86_64::instructions::hlt;

use crate::{
    memory::paging::MAPPER, misc::others::CpuCoreContext,
    multitasking::memory::allocate_kernel_stack,
};

pub mod aux;
pub mod c_types;
pub mod debug_exit;

pub mod error;
pub mod framebuffer;
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
    local_apic: None,
    gs_kernel_stack_top: 0,
    gs_user_stack_top: 0,
};

pub unsafe fn with_cpu_core_context<R>(f: impl FnOnce(&mut CpuCoreContext) -> R) -> R {
    let ctx = core::ptr::addr_of_mut!(CPU_CORE_CONTEXT);
    f(&mut *ctx)
}

pub fn init() {
    unsafe {
        CPU_CORE_CONTEXT = Box::leak(Box::new(CpuCoreContext {
            local_apic: Some(
                LocalApicBuilder::new()
                    .timer_vector(32)
                    .error_vector(0xFE)
                    .spurious_vector(0xFF)
                    .build()
                    .unwrap(),
            ),
            gs_kernel_stack_top: allocate_kernel_stack(16).finish().as_u64(),
            gs_user_stack_top: 0,
        }))
    };
}

pub fn hlt_loop() -> ! {
    loop {
        hlt();
    }
}

#[cfg(target_arch = "x86_64")]
pub fn get_cycles() -> u64 {
    let low: u32;
    let high: u32;
    unsafe {
        core::arch::asm!(
            "rdtsc",
            out("eax") low,
            out("edx") high,
            options(nomem, nostack, preserves_flags)
        );
    }
    ((high as u64) << 32) | low as u64
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
