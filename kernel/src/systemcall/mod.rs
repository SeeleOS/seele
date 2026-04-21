use x86_64::{
    PrivilegeLevel, VirtAddr,
    instructions::interrupts::without_interrupts,
    registers::segmentation::SegmentSelector,
    registers::{
        control::{Efer, EferFlags},
        model_specific::{LStar, SFMask, Star},
        rflags::RFlags,
    },
};

use crate::{
    smp::{
        kernel_code_selector, kernel_data_selector, load_current_kernel_gs_base,
        user_code_selector, user_data_selector,
    },
    systemcall::entry::syscall_entry,
};

pub mod arg_types;
pub mod entry;
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
        let kernel_cs = SegmentSelector::new(kernel_code_selector().index(), PrivilegeLevel::Ring0);
        let kernel_ss = SegmentSelector::new(kernel_data_selector().index(), PrivilegeLevel::Ring0);
        let user_cs = SegmentSelector::new(user_code_selector().index(), PrivilegeLevel::Ring3);
        let user_ss = SegmentSelector::new(user_data_selector().index(), PrivilegeLevel::Ring3);
        Star::write(user_cs, user_ss, kernel_cs, kernel_ss)
            .expect("invalid STAR segment selectors");

        // sets the entry point for systemcalls
        let syscall_entry_addr = VirtAddr::new(syscall_entry as *const () as usize as u64);
        LStar::write(syscall_entry_addr);

        load_current_kernel_gs_base();
    })
}
