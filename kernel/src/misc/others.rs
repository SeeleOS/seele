use alloc::vec::Vec;
use x86_64::{
    PhysAddr, PrivilegeLevel,
    registers::control::{Cr0, Cr0Flags, Cr3Flags, Cr4, Cr4Flags},
    structures::{idt::InterruptStackFrame, paging::PageTableFlags},
};

use crate::{memory::protection::Protection, misc::error::KernelResult};
pub fn calc_cr3_value(addr: PhysAddr, flags: Cr3Flags) -> u64 {
    ((false as u64) << 63) | addr.as_u64() | flags.bits()
}

pub fn enable_sse() {
    unsafe {
        // 1. 清除 CR0.EM (Emulation), 设置 CR0.MP (Monitor Coprocessor)
        let mut cr0 = Cr0::read();
        cr0.remove(Cr0Flags::EMULATE_COPROCESSOR);
        cr0.insert(Cr0Flags::MONITOR_COPROCESSOR);
        Cr0::write(cr0);

        // 2. 设置 CR4.OSFXSR (Operating System Support for FXSAVE/FXRSTOR)
        //    和 CR4.OSXMMEXCPT (Operating System Support for Unmasked SIMD Exceptions)
        let mut cr4 = Cr4::read();
        cr4.insert(Cr4Flags::OSFXSR);
        cr4.insert(Cr4Flags::OSXMMEXCPT_ENABLE);
        Cr4::write(cr4);
    }
}

pub trait KernelFrom<T> {
    fn k_from(val: T) -> KernelResult<Self>
    where
        Self: Sized;
}

pub fn protection_to_page_flags(protection: Protection) -> PageTableFlags {
    let mut flags = PageTableFlags::PRESENT | PageTableFlags::USER_ACCESSIBLE;

    if protection.contains(Protection::WRITE) {
        flags |= PageTableFlags::WRITABLE;
    }

    if !protection.contains(Protection::EXEC) {
        flags |= PageTableFlags::NO_EXECUTE;
    }

    flags
}

#[macro_export]
macro_rules! define_with_accessor {
    ($name: literal, $type: ty, $getter: ident) => {
        paste::paste! {
            pub fn [<with_$name>]<R, F>(func: F) -> R
            where
                F: FnOnce(&mut $type) -> R,
            {
                let current_thread_ref = $getter();
                let mut current_thread = current_thread_ref.lock();
                func(&mut current_thread)
            }
        }
    };
}

pub fn push_and_return_index<T>(vec: &mut Vec<T>, item: T) -> usize {
    vec.push(item);
    vec.len() - 1
}

pub fn is_user_mode(stackframe: &InterruptStackFrame) -> bool {
    stackframe.code_segment.rpl() == PrivilegeLevel::Ring3
        && stackframe.stack_segment.rpl() == PrivilegeLevel::Ring3
}
