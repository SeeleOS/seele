use alloc::slice;

use crate::{
    multitasking::MANAGER,
    object::traits::Writable,
    systemcall::{error::SyscallError, implementations::utils::SyscallImpl, syscall_no::SyscallNo},
};

pub struct ReadObjectImpl;
pub struct WriteObjectImpl;

impl SyscallImpl for ReadObjectImpl {
    const ENTRY: crate::systemcall::syscall_no::SyscallNo = SyscallNo::ReadObject;

    fn handle_call(
        arg1: u64,
        arg2: u64,
        arg3: u64,
        _arg4: u64,
        _arg5: u64,
        _arg6: u64,
    ) -> Result<usize, crate::systemcall::error::SyscallError> {
        let current = MANAGER.lock().current.clone().unwrap();
        let mut current = current.lock();
        unsafe {
            Ok(current
                .get_object(arg1)?
                .as_readable()
                .unwrap()
                .read(slice::from_raw_parts_mut(arg2 as *mut u8, arg3 as usize))
                .unwrap())
        }
    }
}

impl SyscallImpl for WriteObjectImpl {
    const ENTRY: SyscallNo = SyscallNo::WriteObject;

    fn handle_call(
        arg1: u64,
        arg2: u64,
        arg3: u64,
        _arg4: u64,
        _arg5: u64,
        _arg6: u64,
    ) -> Result<usize, crate::systemcall::error::SyscallError> {
        let current = MANAGER.lock().current.clone().unwrap();
        let mut current = current.lock();

        unsafe {
            Ok(current
                .get_object(arg1)?
                .as_writable()
                .unwrap()
                .write(slice::from_raw_parts(arg2 as *mut u8, arg3 as usize))
                .unwrap())
        }
    }
}

pub struct RemoveObjectImpl;

impl SyscallImpl for RemoveObjectImpl {
    const ENTRY: SyscallNo = SyscallNo::RemoveObject;

    fn handle_call(
        arg1: u64,
        arg2: u64,
        arg3: u64,
        arg4: u64,
        arg5: u64,
        arg6: u64,
    ) -> Result<usize, crate::systemcall::error::SyscallError> {
        let current_ref = MANAGER.lock().current.clone().unwrap();
        let mut current = current_ref.lock();
        let objects = &mut current.objects;

        if objects.len() > arg1 as usize {
            let object = objects[arg1 as usize].take();

            drop(object);

            Ok(0)
        } else {
            Err(SyscallError::BadFileDescriptor)
        }
    }
}
