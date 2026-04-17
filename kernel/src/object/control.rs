use crate::object::FileFlags;
use crate::{
    object::misc::ObjectRef,
    process::misc::with_current_process,
    systemcall::utils::{SyscallError, SyscallResult},
};
use num_enum::TryFromPrimitive;

#[derive(Clone, Copy, Debug, TryFromPrimitive)]
#[repr(u64)]
enum FcntlCmd {
    DupFd = 0,
    GetFd = 1,
    SetFd = 2,
    GetFl = 3,
    SetFl = 4,
    DupFdCloexec = 1030,
}

const O_WRONLY: usize = 0o1;
const O_RDWR: usize = 0o2;
const O_NONBLOCK: usize = 0o4_000;

fn access_mode_bits(object: &ObjectRef) -> usize {
    let readable = object.clone().as_readable().is_ok();
    let writable = object.clone().as_writable().is_ok();

    match (readable, writable) {
        (false, true) => O_WRONLY,
        (true, true) => O_RDWR,
        _ => 0,
    }
}

pub fn control_object(object: ObjectRef, command: u64, arg: u64) -> SyscallResult {
    match FcntlCmd::try_from(command).map_err(|_| SyscallError::InvalidArguments)? {
        FcntlCmd::SetFl => object
            .set_flags(
                FileFlags::from_bits(arg & O_NONBLOCK as u64)
                    .ok_or(SyscallError::InvalidArguments)?,
            )
            .map(|_| 0usize)
            .map_err(Into::into),
        FcntlCmd::GetFl => object
            .clone()
            .get_flags()
            .map_err(Into::into)
            .map(|f| access_mode_bits(&object) | f.bits() as usize),
        FcntlCmd::DupFd => with_current_process(|process| {
            process
                .clone_object_with_min(object, arg as usize)
                .map_err(Into::into)
        }),
        FcntlCmd::DupFdCloexec => with_current_process(|process| {
            process
                .clone_object_with_min(object, arg as usize)
                .map_err(Into::into)
        }),
        FcntlCmd::SetFd | FcntlCmd::GetFd => Ok(0),
    }
}
