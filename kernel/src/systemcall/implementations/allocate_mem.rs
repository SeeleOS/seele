use x86_64::structures::paging::PageTableFlags;

use crate::{
    memory::manager::allocate_user_mem,
    multitasking::MANAGER,
    s_println,
    systemcall::{implementations::utils::SyscallImpl, syscall_no::SyscallNo},
};

pub struct AllocMemImpl;

impl SyscallImpl for AllocMemImpl {
    const ENTRY: crate::systemcall::syscall_no::SyscallNo = SyscallNo::AllocateMem;

    fn handle_call(
        arg1: u64,
        _arg2: u64,
        _arg3: u64,
        _arg4: u64,
        _arg5: u64,
        _arg6: u64,
    ) -> Result<usize, crate::systemcall::error::SyscallError> {
        s_println!("Allocating {} pages, requested by user via arg1", arg1);
        let manager = MANAGER.lock();
        let flags =
            PageTableFlags::USER_ACCESSIBLE | PageTableFlags::PRESENT | PageTableFlags::WRITABLE;
        let mut current = manager.current.as_ref().unwrap().lock();
        s_println!("process is {:?}", current.pid);
        s_println!(
            "Allocating mem for {:?}",
            current.addrspace.page_table.frame
        );
        let mem_start = allocate_user_mem(arg1, &mut current.addrspace.page_table.inner, flags)
            .0
            .as_u64();
        unsafe {
            use x86_64::registers::control::Cr3;
            let (frame, flags) = Cr3::read();
            Cr3::write(frame, flags); // 重新加载 CR3 会强制清空当前 CPU 的所有 TLB 缓存
        }
        Ok(mem_start as usize)
    }
}
