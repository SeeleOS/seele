use x86_64::{
    PrivilegeLevel, VirtAddr,
    instructions::interrupts::without_interrupts,
    registers::segmentation::SegmentSelector,
    registers::{
        control::{Efer, EferFlags},
        model_specific::{KernelGsBase, LStar, SFMask, Star},
        rflags::RFlags,
    },
};

use crate::{
    misc::{CPU_CORE_CONTEXT, gdt::GDT},
    systemcall::entry::syscall_entry,
};

pub mod arg_types;
pub mod entry;
pub mod error;
pub mod handling;
pub mod implementations;
pub mod numbers;
pub mod table;
pub mod utils;

pub fn init() {
    without_interrupts(|| {
        // enable systemcalls
        unsafe { Efer::update(|efer| efer.insert(EferFlags::SYSTEM_CALL_EXTENSIONS)) };

        // disable interrupts on systemcalls
        SFMask::write(RFlags::INTERRUPT_FLAG);

        // set segment selectors for SYSCALL/SYSRET
        let kernel_cs = SegmentSelector::new(GDT.1.kernel_code.index(), PrivilegeLevel::Ring0);
        let kernel_ss = SegmentSelector::new(GDT.1.kernel_data.index(), PrivilegeLevel::Ring0);
        let user_cs = SegmentSelector::new(GDT.1.user_code.index(), PrivilegeLevel::Ring3);
        let user_ss = SegmentSelector::new(GDT.1.user_data.index(), PrivilegeLevel::Ring3);
        Star::write(user_cs, user_ss, kernel_cs, kernel_ss)
            .expect("invalid STAR segment selectors");

        // sets the entry point for systemcalls
        let syscall_entry_addr = VirtAddr::new(syscall_entry as *const () as usize as u64);
        LStar::write(syscall_entry_addr);

        unsafe {
            KernelGsBase::write(VirtAddr::new(CPU_CORE_CONTEXT as u64));
        }
    })
}
