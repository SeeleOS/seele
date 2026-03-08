#[unsafe(no_mangle)]
#[unsafe(naked)]
pub extern "C" fn syscall_entry() {
    core::arch::naked_asm!(
        "swapgs",
        // 1. 保存用户态 RSP，切换到内核栈
        "mov gs:[0x8], rsp",
        "mov rsp, gs:[0x0]",
        // 2. 构造 Snapshot 结构体的后半部分 (Iret 帧)
        // 顺序：ss, rsp, rflags, cs, rip (由高地址向低地址压栈)
        "push 0x1b",     // ss: User Data Segment
        "push gs:[0x8]", // rsp: 保存的用户态 RSP
        "push r11",      // rflags: syscall 指令将 rflags 存在了 r11
        "push 0x23",     // cs: User Code Segment
        "push rcx",      // rip: syscall 指令将 rip 存在了 rcx
        // 3. 构造 Snapshot 结构体的前半部分 (通用寄存器)
        // 顺序：rax, rcx, rdx, rbx, rbp, rsi, rdi, r8-r15
        "push rax",
        "push rcx",
        "push rdx",
        "push rbx",
        "push rbp",
        "push rsi",
        "push rdi",
        "push r8",
        "push r9",
        "push r10",
        "push r11",
        "push r12",
        "push r13",
        "push r14",
        "push r15",
        // 4. 此时 RSP 正好指向 Snapshot 结构体的起始地址 (r15)
        // 且一共压入了 20 个 u64 (160 字节)，16 字节对齐已满足，无需 sub rsp, 8
        "mov rdi, rsp", // 将 Snapshot 指针作为第一个参数传递给 Rust
        "call syscall_handler",
        // 5. 从 Snapshot 恢复现场
        // 如果 syscall_handler 修改了 snapshot.rax，pop 出来的就是新值
        "pop r15",
        "pop r14",
        "pop r13",
        "pop r12",
        "pop r11",
        "pop r10",
        "pop r9",
        "pop r8",
        "pop rdi",
        "pop rsi",
        "pop rbp",
        "pop rbx",
        "pop rdx",
        "pop rcx",
        "pop rax",
        // 6. 跳过 rip, cs, rflags, rsp, ss，这些将由 iretq 处理
        // 但我们已经在栈上了，直接准备好 iretq 即可
        "swapgs",
        "iretq"
    )
}
