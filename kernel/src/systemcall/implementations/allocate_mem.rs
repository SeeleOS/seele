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
        let mut manager = MANAGER.lock();
        let flags =
            PageTableFlags::USER_ACCESSIBLE | PageTableFlags::PRESENT | PageTableFlags::WRITABLE;
        Ok(
            allocate_user_mem(arg1, &mut manager.get_current().page_table.inner, flags)
                .0
                .as_u64() as usize,
        )
    }
}
