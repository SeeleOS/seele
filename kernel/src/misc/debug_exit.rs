use qemu_exit::{QEMUExit, X86};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QemuExitCode {
    Success = 0x10,
    Failed = 0x11,
}

pub fn debug_exit(code: QemuExitCode) {
    X86::new(0xf4, 1).exit(code as u32);
}
