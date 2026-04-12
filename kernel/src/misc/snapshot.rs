// Snapshot of the operating system. Including registers.
// Also known as Frame, Context, etc.

use core::mem::offset_of;
use x86_64::structures::idt::InterruptStackFrame;

#[repr(C)]
#[derive(Clone, Copy, Default, Debug)]
pub struct Snapshot {
    pub r15: u64,
    pub r14: u64,
    pub r13: u64,
    pub r12: u64,
    pub r11: u64,
    pub r10: u64,
    pub r9: u64,
    pub r8: u64,
    pub rdi: u64,
    pub rsi: u64,
    pub rbp: u64,
    pub rbx: u64,
    pub rdx: u64,
    pub rcx: u64,
    pub rax: isize,

    pub rip: u64,
    pub cs: u64,
    pub rflags: u64,
    pub rsp: u64,
    pub ss: u64,
}

impl Snapshot {
    pub fn default_regs(rip: u64, cs: u16, rflags: u64, rsp: u64, ss: u16) -> Self {
        Self {
            rip,
            cs: cs as u64,
            rflags,
            rsp,
            ss: ss as u64,
            ..Default::default()
        }
    }

    pub fn update_with_stackframe(&mut self, stackframe: &InterruptStackFrame) {
        self.rip = stackframe.instruction_pointer.as_u64();
        self.cs = stackframe.code_segment.0 as u64;
        self.ss = stackframe.stack_segment.0 as u64;
        self.rsp = stackframe.stack_pointer.as_u64();
        self.rflags = stackframe.cpu_flags.bits();
    }

    pub fn from_current() -> Self {
        let mut snp = Self::default();
        unsafe {
            core::arch::asm!(
                // Executor snapshots only need precise callee-saved registers and
                // control state. Use rax as the output base so the live rdi value
                // is captured instead of being overwritten by the snapshot pointer.
                "mov [rax + {R15_OFF}], r15",
                "mov [rax + {R14_OFF}], r14",
                "mov [rax + {R13_OFF}], r13",
                "mov [rax + {R12_OFF}], r12",
                "mov [rax + {R11_OFF}], r11",
                "mov [rax + {R10_OFF}], r10",
                "mov [rax + {R9_OFF}], r9",
                "mov [rax + {R8_OFF}], r8",
                "mov [rax + {RDI_OFF}], rdi",
                "mov [rax + {RSI_OFF}], rsi",
                "mov [rax + {RBP_OFF}], rbp",
                "mov [rax + {RBX_OFF}], rbx",
                "mov [rax + {RDX_OFF}], rdx",
                "mov [rax + {RCX_OFF}], rcx",

                "lea rdx, [rip + 2f]",
                "mov [rax + {RIP_OFF}], rdx",

                "mov rdx, cs",
                "mov [rax + {CS_OFF}], rdx",

                "pushfq",
                "pop qword ptr [rax + {RFLAGS_OFF}]",

                "mov [rax + {RSP_OFF}], rsp",

                "mov rdx, ss",
                "mov [rax + {SS_OFF}], rdx",
                "2:",
                in("rax") &mut snp,
                lateout("rdx") _,
                R15_OFF = const offset_of!(Snapshot, r15),
                R14_OFF = const offset_of!(Snapshot, r14),
                R13_OFF = const offset_of!(Snapshot, r13),
                R12_OFF = const offset_of!(Snapshot, r12),
                R11_OFF = const offset_of!(Snapshot, r11),
                R10_OFF = const offset_of!(Snapshot, r10),
                R9_OFF = const offset_of!(Snapshot, r9),
                R8_OFF = const offset_of!(Snapshot, r8),
                RDI_OFF = const offset_of!(Snapshot, rdi),
                RSI_OFF = const offset_of!(Snapshot, rsi),
                RBP_OFF = const offset_of!(Snapshot, rbp),
                RBX_OFF = const offset_of!(Snapshot, rbx),
                RDX_OFF = const offset_of!(Snapshot, rdx),
                RCX_OFF = const offset_of!(Snapshot, rcx),
                RIP_OFF = const offset_of!(Snapshot, rip),
                CS_OFF = const offset_of!(Snapshot, cs),
                RFLAGS_OFF = const offset_of!(Snapshot, rflags),
                RSP_OFF = const offset_of!(Snapshot, rsp),
                SS_OFF = const offset_of!(Snapshot, ss),
            );
        }
        snp
    }
}
