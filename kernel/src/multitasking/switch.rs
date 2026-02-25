use core::arch::{self, naked_asm};

use x86_64::{
    VirtAddr,
    registers::model_specific::{FsBase, GsBase, KernelGsBase, Msr},
};

use crate::{
    misc::{CPU_CORE_CONTEXT, others::CpuCoreContext, snapshot::Snapshot},
    multitasking::{self, context::ProcessSnapshot, manager::Manager},
    new_syscall, s_println,
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

        self.update_gs();
        self.inner.load();
        self.load_page_table();
        self.load_msr();
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
        naked_asm!("mov rax, [rdi + 160]", "mov cr3, rax", "ret")
    }

    #[unsafe(naked)]
    extern "C" fn switch_user(&mut self) {
        naked_asm!(
            // Loads the kernel stack so it wont messup the user stack
            "mov rsp, [rdi + 168]",
            // Pushes the things required for iretq
            // TODO: use ret to return to whatever it came from
            // instead of just straightup jumping to userspace with iretq
            "push [rdi + 152]", // SS
            "push [rdi + 144]", // RSP
            "push [rdi + 136]", // RFlags
            "push [rdi + 128]", // CS
            "push [rdi + 120]", // RIP
            "iretq"
        )
    }
}
