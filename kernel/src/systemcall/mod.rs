use x86_64::{
    VirtAddr,
    instructions::interrupts::without_interrupts,
    registers::{
        control::{Efer, EferFlags},
        model_specific::{KernelGsBase, LStar, SFMask},
        rflags::RFlags,
    },
};

use crate::{
    misc::{CPU_CORE_CONTEXT, others::CpuCoreContext},
    systemcall::entry::syscall_entry,
};

// [TODO] Fix panics, add swapgs shit
pub mod entry;
pub mod error;
pub mod handling;
pub mod implementations;
pub mod syscall_no;
pub mod syscalls_table;
pub mod utils;

pub fn init() {
    without_interrupts(|| {
        // enable systemcalls
        unsafe { Efer::update(|efer| efer.insert(EferFlags::SYSTEM_CALL_EXTENSIONS)) };

        // disable interrupts on systemcalls
        SFMask::write(RFlags::INTERRUPT_FLAG);

        // sets the entry point for systemcalls
        let syscall_entry_addr = VirtAddr::new(syscall_entry as *const () as usize as u64);
        LStar::write(syscall_entry_addr);

        unsafe {
            KernelGsBase::write(VirtAddr::new(
                ((CPU_CORE_CONTEXT) as *const CpuCoreContext) as u64,
            ));
        }
    })
}
