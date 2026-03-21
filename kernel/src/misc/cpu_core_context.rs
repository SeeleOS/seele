use alloc::boxed::Box;
use x2apic::lapic::{LocalApic, LocalApicBuilder, xapic_base};

use crate::{interrupts::default_local_apic, multitasking::memory::allocate_kernel_stack};

#[derive(Debug)]
#[repr(C)]
pub struct CpuCoreContext {
    // Used on syscall_entry with swapgs
    pub gs_kernel_stack_top: u64,
    pub gs_user_stack_top: u64,
    pub local_apic: Option<LocalApic>,
}

pub static mut CPU_CORE_CONTEXT: *mut CpuCoreContext = core::ptr::null_mut();

pub fn with_cpu_core_context<R>(f: impl FnOnce(&mut CpuCoreContext) -> R) -> R {
    unsafe {
        let ctx = CPU_CORE_CONTEXT;
        assert!(!ctx.is_null(), "CPU core context not initialized");
        f(&mut *ctx)
    }
}

pub fn init() {
    let ctx = Box::leak(Box::new(CpuCoreContext {
        local_apic: Some(default_local_apic()),
        gs_kernel_stack_top: allocate_kernel_stack(16).finish().as_u64(),
        gs_user_stack_top: 0,
    }));

    unsafe {
        CPU_CORE_CONTEXT = ctx as *mut CpuCoreContext;
    }
}
