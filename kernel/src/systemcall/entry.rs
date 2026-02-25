
// entry point for all system calls
#[unsafe(no_mangle)]
#[unsafe(naked)]
pub extern "C" fn syscall_entry() {
    core::arch::naked_asm!(
        "swapgs",
        // Saves the userspace RSP into gs
        "mov gs:[0x8], rsp",
        // loads the stack saved in gs
        "mov rsp, gs:[0x0]",
        // Pushing arguments required for SyscallSnapshot
        "push rcx",
        "push r11",
        "push rax",
        "push rdi",
        "push rsi",
        "push rdx",
        "push r10",
        "push r8",
        "push r9",
        // 16 bits align the rsp
        "sub rsp, 8",
        "mov rdi, rsp",
        "add rdi, 8",
        "call syscall_handler",
        "add rsp, 8",
        // resume
        "pop r9",
        "pop r8",
        "pop r10",
        "pop rdx",
        "pop rsi",
        "pop rdi",
        "pop rax", // rust have modified it to be the return value
        "pop r11",
        "pop rcx",
        // Loads the userspace rsp from gs
        "mov rsp, gs:[0x8]",
        "swapgs",
        // resume the state
        "sysretq"
    )
}
