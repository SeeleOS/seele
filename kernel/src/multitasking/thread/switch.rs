use core::arch::naked_asm;

use core::mem::offset_of;
use x86_64::{
    VirtAddr,
    instructions::interrupts::without_interrupts,
    registers::model_specific::{FsBase, KernelGsBase},
};

use crate::{
    misc::{CPU_CORE_CONTEXT, others::CpuCoreContext, snapshot::Snapshot},
    multitasking::thread::snapshot::{ThreadSnapshot, ThreadSnapshotType},
    s_println,
};

impl ThreadSnapshot {
    /// Switches from [`source`] to [`self`]
    pub fn switch_from(
        &mut self,
        source: Option<&mut ThreadSnapshot>,
        snapshot: Option<&mut Snapshot>,
    ) {
        without_interrupts(|| {
            if let Some(source) = source {
                // Saves the current RSP, which have the RIP saved
                // on the stacktop when we called switch_from()
                // So when we use jump_to_executor(), it will load
                // the RSP, get the RIP, and when RET back
                if matches!(source.snapshot_type, ThreadSnapshotType::Executor) {
                    source.save_executor_rsp();
                }

                // Saves the current state of the system (snapshot)
                if let Some(snapshot) = snapshot {
                    source.inner = *snapshot;
                }
                source.save_msr();
            }

            self.update_gs();
            self.load_msr();
        });
        match self.snapshot_type {
            ThreadSnapshotType::Thread => self.jump_user(),
            ThreadSnapshotType::Executor => self.jump_to_executor(),
        }
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
    extern "C" fn save_executor_rsp(&mut self) {
        naked_asm!("mov [rdi + 160], rsp", "ret")
    }

    #[unsafe(naked)]
    extern "C" fn jump_to_executor(&mut self) {
        naked_asm!(
            // Loads the kernel stack so it wont messup the user stack
            "mov rsp, [rdi + 160]",
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
            "ret"
        )
    }

    #[unsafe(naked)]
    extern "C" fn jump_user(&mut self) {
        naked_asm!(
            // 先把 self 的指针存入 rax，防止修改 rsp 后 rdi 访问出现奇怪的问题
                "mov rax, rdi",

                // 1. 设置内核栈 (从 ThreadSnapshot 的字段读)
                "mov rsp, [rax + {K_RSP_OFF}]",

                // 2. 构造 iretq 栈帧 (从 Snapshot 的字段读)
                // 编译器会自动计算 Snapshot 在 ThreadSnapshot 里的位置 (INNER_OFF)
                // 以及各寄存器在 Snapshot 里的位置
                "push [rax + {INNER_OFF} + {SS_OFF}]",   // SS
                "push [rax + {INNER_OFF} + {RSP_OFF}]",  // RSP
                "push [rax + {INNER_OFF} + {FLAGS_OFF}]",// RFlags
                "push [rax + {INNER_OFF} + {CS_OFF}]",   // CS
                "push [rax + {INNER_OFF} + {RIP_OFF}]",  // RIP

                // 3. 恢复通用寄存器
                "mov r15, [rax + {INNER_OFF} + {R15_OFF}]",
                "mov r14, [rax + {INNER_OFF} + {R14_OFF}]",
                "mov r13, [rax + {INNER_OFF} + {R13_OFF}]",
                "mov r12, [rax + {INNER_OFF} + {R12_OFF}]",
                "mov r11, [rax + {INNER_OFF} + {R11_OFF}]",
                "mov r10, [rax + {INNER_OFF} + {R10_OFF}]",
                "mov r9,  [rax + {INNER_OFF} + {R9_OFF}]",
                "mov r8,  [rax + {INNER_OFF} + {R8_OFF}]",
                "mov rsi, [rax + {INNER_OFF} + {RSI_OFF}]",
                "mov rbp, [rax + {INNER_OFF} + {RBP_OFF}]",
                "mov rbx, [rax + {INNER_OFF} + {RBX_OFF}]",
                "mov rdx, [rax + {INNER_OFF} + {RDX_OFF}]",
                "mov rcx, [rax + {INNER_OFF} + {RCX_OFF}]",
                "mov rax, [rax + {INNER_OFF} + {RAX_OFF}]", // 此时 rax 被覆盖了

                // 最后恢复 rdi (此时我们不能再用 rax 了，只能从内存读原来的 rdi)
                // 注意：这里需要再通过 rdi 的原始备份读一次，或者直接从栈/内存读
                // 为简单起见，我们重新用 rdi 读一次，因为我们还没改它
                "mov rdi, [rdi + {INNER_OFF} + {RDI_OFF}]",

                "iretq",

                // 常量偏移定义
                K_RSP_OFF = const offset_of!(ThreadSnapshot, kernel_rsp),
                INNER_OFF = const offset_of!(ThreadSnapshot, inner),
                RIP_OFF   = const offset_of!(Snapshot, rip),
                CS_OFF    = const offset_of!(Snapshot, cs),
                FLAGS_OFF = const offset_of!(Snapshot, rflags),
                RSP_OFF   = const offset_of!(Snapshot, rsp),
                SS_OFF    = const offset_of!(Snapshot, ss),
                R15_OFF   = const offset_of!(Snapshot, r15),
                R14_OFF   = const offset_of!(Snapshot, r14),
                R13_OFF   = const offset_of!(Snapshot, r13),
                R12_OFF   = const offset_of!(Snapshot, r12),
                R11_OFF   = const offset_of!(Snapshot, r11),
                R10_OFF   = const offset_of!(Snapshot, r10),
                R9_OFF    = const offset_of!(Snapshot, r9),
                R8_OFF    = const offset_of!(Snapshot, r8),
                RDI_OFF   = const offset_of!(Snapshot, rdi),
                RSI_OFF   = const offset_of!(Snapshot, rsi),
                RBP_OFF   = const offset_of!(Snapshot, rbp),
                RBX_OFF   = const offset_of!(Snapshot, rbx),
                RDX_OFF   = const offset_of!(Snapshot, rdx),
                RCX_OFF   = const offset_of!(Snapshot, rcx),
                RAX_OFF   = const offset_of!(Snapshot, rax),
        )
    }
}
