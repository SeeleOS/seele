// Snapshot of the operating system. Including registers.
// Also known as Frame, Context, etc.

use core::arch::naked_asm;

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
    pub rax: u64,

    pub rip: u64,
    pub cs: u64,
    pub rflags: u64,
    pub rsp: u64,
    pub ss: u64,
}

impl Snapshot {
    #[unsafe(naked)]
    pub extern "C" fn load(&self) {
        naked_asm!(
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
            "ret"
        )
    }

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
}
