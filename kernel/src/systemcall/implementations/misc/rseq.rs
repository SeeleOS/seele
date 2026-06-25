use super::*;

const ROBUST_LIST_HEAD_LEN_X86_64: usize = 24;

define_syscall!(SetRobustList, |head: u64, len: usize| {
    if len != ROBUST_LIST_HEAD_LEN_X86_64 {
        return Err(SyscallError::InvalidArguments);
    }

    let current = crate::thread::get_current_thread();
    let mut current = current.lock();
    current.robust_list_head = head;
    current.robust_list_len = len;
    Ok(0)
});

define_syscall!(Rseq, |rseq_ptr: *mut LinuxRseq,
                       rseq_len: u32,
                       flags: RseqFlags,
                       sig: u32| {
    if flags.bits() != flags.bits() & RseqFlags::UNREGISTER.bits() || rseq_len != RSEQ_LEN_X86_64 {
        return Err(SyscallError::InvalidArguments);
    }

    let current = crate::thread::get_current_thread();
    let mut current = current.lock();

    if flags.contains(RseqFlags::UNREGISTER) {
        if current.rseq_area != rseq_ptr as u64
            || current.rseq_len != rseq_len
            || current.rseq_sig != sig
        {
            return Err(SyscallError::InvalidArguments);
        }

        write_rseq_area(rseq_ptr, false)?;

        current.rseq_area = 0;
        current.rseq_len = 0;
        current.rseq_flags = 0;
        current.rseq_sig = 0;
        return Ok(0);
    }

    if rseq_ptr.is_null() {
        return Err(SyscallError::InvalidArguments);
    }

    if current.rseq_area != 0 {
        return Err(SyscallError::DeviceOrResourceBusy);
    }

    write_rseq_area(rseq_ptr, true)?;

    current.rseq_area = rseq_ptr as u64;
    current.rseq_len = rseq_len;
    current.rseq_flags = flags.bits();
    current.rseq_sig = sig;
    Ok(0)
});
