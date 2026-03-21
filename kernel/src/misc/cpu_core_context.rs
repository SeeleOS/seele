use alloc::boxed::Box;
use x2apic::lapic::{LocalApic, LocalApicBuilder};

use crate::multitasking::memory::allocate_kernel_stack;

#[derive(Debug)]
#[repr(C)]
pub struct CpuCoreContext {
    pub local_apic: Option<LocalApic>,
    // Used on syscall_entry with swapgs
    pub gs_kernel_stack_top: u64,
    pub gs_user_stack_top: u64,
}

pub static mut CPU_CORE_CONTEXT: &mut CpuCoreContext = &mut CpuCoreContext {
    local_apic: None,
    gs_kernel_stack_top: 0,
    gs_user_stack_top: 0,
};

pub fn with_cpu_core_context<R>(f: impl FnOnce(&mut CpuCoreContext) -> R) -> R {
    let ctx = core::ptr::addr_of_mut!(CPU_CORE_CONTEXT);
    unsafe { f(&mut *ctx) }
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
    }
}
