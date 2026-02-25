use x86_64::{
    PhysAddr, VirtAddr,
    registers::control::{Cr0, Cr0Flags, Cr3, Cr3Flags, Cr4, Cr4Flags},
};

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
