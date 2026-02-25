// Snapshot of the operating system. Including registers.
// Also known as Frame, Context, etc.


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

    pub error_code: u64,
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
}
