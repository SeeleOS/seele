use crate::object::FileFlags;
use crate::{
    object::{
        error::ObjectError,
        linux_anon::{memfd_add_seals, memfd_get_seals},
        misc::ObjectRef,
    },
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
    AddSeals = 1033,
    GetSeals = 1034,
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
        FcntlCmd::SetFl => {
            let mut flags = FileFlags::empty();
            if (arg & O_NONBLOCK as u64) != 0 {
                flags.insert(FileFlags::NONBLOCK);
            }
            match object.set_flags(flags) {
                Ok(()) | Err(ObjectError::Unimplemented) => Ok(0),
                Err(err) => Err(err.into()),
            }
        }
        FcntlCmd::GetFl => {
            let flags = match object.clone().get_flags() {
                Ok(flags) => {
                    let mut linux_flags = 0;
                    if flags.contains(FileFlags::NONBLOCK) {
                        linux_flags |= O_NONBLOCK;
                    }
                    linux_flags
                }
                Err(ObjectError::Unimplemented) => 0,
                Err(err) => return Err(err.into()),
            };

            Ok(access_mode_bits(&object) | flags)
        }
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
        FcntlCmd::AddSeals => {
            let file_like = object.as_file_like()?;
            memfd_add_seals(&file_like.path(), arg as u32)
        }
        FcntlCmd::GetSeals => {
            let file_like = object.as_file_like()?;
            memfd_get_seals(&file_like.path())
                .map(|seals| seals as usize)
                .ok_or(SyscallError::InvalidArguments)
        }
    }
}
