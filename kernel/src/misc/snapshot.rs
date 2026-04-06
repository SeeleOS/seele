// Snapshot of the operating system. Including registers.
// Also known as Frame, Context, etc.

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
                // 1. 保存通用寄存器到结构体偏移
                "mov [rdi + 0x00], r15",
                "mov [rdi + 0x08], r14",
                "mov [rdi + 0x10], r13",
                "mov [rdi + 0x18], r12",
                "mov [rdi + 0x20], r11",
                "mov [rdi + 0x28], r10",
                "mov [rdi + 0x30], r9",
                "mov [rdi + 0x38], r8",
                "mov [rdi + 0x40], rdi", // 这里存的是原始的 rdi
                "mov [rdi + 0x48], rsi",
                "mov [rdi + 0x50], rbp",
                "mov [rdi + 0x58], rbx",
                "mov [rdi + 0x60], rdx",
                "mov [rdi + 0x68], rcx",
                "mov [rdi + 0x70], rax",

                // 2. 特殊寄存器处理
                "lea rax, [rip + 2f]",    // 捕获“跳出汇编”后的 RIP
                "mov [rdi + 0x78], rax",   // rip 偏移是 0x80 (16*8)

                "mov rax, cs",
                "mov [rdi + 0x80], rax",   // cs

                "pushfq",                 // 捕获 RFLAGS
                "pop qword ptr [rdi + 0x88]",

                "mov [rdi + 0x90], rsp",   // rsp

                "mov rax, ss",
                "mov [rdi + 0x98], rax",   // ss
                "2:",
                in("rdi") &mut snp,        // 将结构体地址传入 rdi
                options(nostack)
            );
        }
        snp
    }
}
