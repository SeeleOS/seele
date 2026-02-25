use core::arch::naked_asm;

use x86_64::{
    VirtAddr,
    registers::model_specific::{FsBase, KernelGsBase},
};

use crate::{
    misc::{CPU_CORE_CONTEXT, others::CpuCoreContext, snapshot::Snapshot},
    multitasking::process::context::ProcessSnapshot, s_println,
};

impl ProcessSnapshot {
    /// Switches from [`source`] to [`self`]
    pub fn switch_from(
        &mut self,
        source: Option<&mut ProcessSnapshot>,
        snapshot: Option<&mut Snapshot>,
    ) {
        if let Some(source) = source {
            // Saves the current state of the system (snapshot)
            source.inner = *snapshot.unwrap();
            source.save_msr();
        }

        s_println!("update gs");
        self.update_gs();
        self.load_page_table();
        self.load_msr();
        s_println!("final jump");
        self.switch_user();
    }

    fn update_gs(&mut self) {
        unsafe {
            CPU_CORE_CONTEXT.gs_kernel_stack_top = self.kernel_rsp;
            KernelGsBase::write(VirtAddr::new(
                ((CPU_CORE_CONTEXT) as *const CpuCoreContext) as u64,
            ));
        }
    }

    fn save_msr(&mut self) {
        self.fs_base = FsBase::read().as_u64();
    }

    fn load_msr(&mut self) {
        FsBase::write(VirtAddr::new(self.fs_base));
    }

    #[unsafe(naked)]
    extern "C" fn load_page_table(&mut self) {
        naked_asm!("mov rax, [rdi + 168]", "mov cr3, rax", "ret")
    }

    #[unsafe(naked)]
    extern "C" fn switch_user(&mut self) {
        naked_asm!(
            // Loads the kernel stack so it wont messup the user stack
            "mov rsp, [rdi + 176]",
            // Pushes the things required for iretq
            "push [rdi + 160]", // SS
            "push [rdi + 152]", // RSP
            "push [rdi + 144]", // RFlags
            "push [rdi + 136]", // CS
            "push [rdi + 128]", // RIP
            // load registers
            "mov r15, [rdi + 0]",
            "mov r14, [rdi + 8]",
            "mov r13, [rdi + 16]",
            "mov r12, [rdi + 24]",
            "mov r11, [rdi + 32]",
            "mov r10, [rdi + 40]",
            "mov r9,  [rdi + 48]",
            "mov r8,  [rdi + 56]",
            "mov rsi, [rdi + 72]",
            "mov rbp, [rdi + 80]",
            "mov rbx, [rdi + 88]",
            "mov rdx, [rdi + 96]",
            "mov rcx, [rdi + 104]",
            "mov rax, [rdi + 112]",
            "mov rdi, [rdi + 64]",
            "iretq"
        )
    }
}
