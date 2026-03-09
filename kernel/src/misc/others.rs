use alloc::string::String;
use x86_64::{
    PhysAddr,
    registers::control::{Cr0, Cr0Flags, Cr3Flags, Cr4, Cr4Flags},
};
/// Context for a CPU Core
#[derive(Debug)]
#[repr(C)]
pub struct CpuCoreContext {
    // Used on syscall_entry with swapgs
    pub gs_kernel_stack_top: u64,
    pub gs_user_stack_top: u64,
}

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

/// # Safety
/// Caller mush provide valid pointer
pub unsafe fn from_cstr(ptr: *const u8) -> Result<String, ()> {
    const MAX_LENGTH: usize = 4096;

    let mut str = String::new();

    for i in 0..MAX_LENGTH {
        unsafe {
            let char = *ptr.add(i) as char;

            if char == '\0' {
                return Ok(str);
            }
            str.push(char);
        }
    }

    Err(())
}
