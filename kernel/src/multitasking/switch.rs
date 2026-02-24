use core::arch::{self, naked_asm};

use x86_64::{
    VirtAddr,
    registers::model_specific::{FsBase, Msr},
};

use crate::{
    misc::{CPU_CORE_CONTEXT, others::CpuCoreContext},
    multitasking::{self, context::Context, manager::Manager},
    new_syscall,
};

impl Context {
    /// Switches from [`source`] to [`self`]
    pub fn switch_from(&mut self, source: Option<&mut Context>) {
        if let Some(source) = source {
            source.save();
            source.save_msr();
        }
        self.load_msr();
        self.update_gs();
        self.load();
        self.load_page_table();
        self.switch_user();
    }

    fn update_gs(&mut self) {
        unsafe {
            CPU_CORE_CONTEXT.gs_kernel_stack_top = self.kernel_rsp;
            Msr::new(0xC0000102).write(((CPU_CORE_CONTEXT) as *const CpuCoreContext) as u64);
        }
    }

    fn save_msr(&mut self) {
        self.fs_base = FsBase::read().as_u64();
    }

    fn load_msr(&mut self) {
        FsBase::write(VirtAddr::new(self.fs_base));
    }

    /// Save all the cpu registers into [`self`]
    #[unsafe(naked)]
    extern "C" fn save(&mut self) {
        naked_asm!(
            "mov [rdi + 8], rsp",
            "mov [rdi + 56], rbp",
            "mov [rdi + 48], rbx",
            "mov [rdi + 40], r12",
            "mov [rdi + 32], r13",
            "mov [rdi + 24], r14",
            "mov [rdi + 16], r15",
            "ret"
        );
    }

    /// Laods all the cpu registers from [`self`]
    #[unsafe(naked)]
    extern "C" fn load(&mut self) {
        naked_asm!(
            "mov r15, [rdi + 16]",
            "mov r14, [rdi + 24]",
            "mov r13, [rdi + 32]",
            "mov r12, [rdi + 40]",
            "mov rbx, [rdi + 48]",
            "mov rbp, [rdi + 56]",
            "ret"
        )
    }

    #[unsafe(naked)]
    extern "C" fn load_page_table(&mut self) {
        naked_asm!("mov rax, [rdi]", "mov cr3, rax", "ret")
    }

    #[unsafe(naked)]
    extern "C" fn switch_user(&mut self) {
        naked_asm!(
            // Loads the kernel stack so it wont messup the user stack
            "mov rsp, [rdi + 8]",
            // Pushes the things required for iretq
            // TODO: use ret to return to whatever it came from
            // instead of just straightup jumping to userspace with iretq
            "push [rdi + 64]", // SS
            "push [rdi + 72]", // RSP
            "push [rdi + 80]", // RFlags
            "push [rdi + 88]", // CS
            "push [rdi + 96]", // RIP
            "iretq"
        )
    }
}

/// # Safety
/// Must provide valid pointer to context
#[unsafe(naked)]
pub unsafe extern "C" fn context_switch_zombie(next: *mut Context) {
    arch::naked_asm!(
        "mov rax, [rdi]",
        "mov cr3, rax",
        "mov rsp, [rdi + 8]",
        "popfq",
        "mov r15, [rdi + 16]",
        "mov r14, [rdi + 24]",
        "mov r13, [rdi + 32]",
        "mov r12, [rdi + 40]",
        "mov rbx, [rdi + 48]",
        "mov rbp, [rsi + 56]",
        "ret",
    );
}
