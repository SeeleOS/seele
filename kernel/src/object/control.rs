use crate::object::FileFlags;
use crate::{
    memory::user_safe,
    object::{
        error::ObjectError,
        memfd::{memfd_add_seals, memfd_get_seals},
        misc::{ObjectRef, get_object_current_process},
    },
    process::{FdFlags, misc::with_current_process},
    systemcall::utils::{SyscallError, SyscallResult},
};
use bitflags::bitflags;
use num_enum::TryFromPrimitive;

#[derive(Clone, Copy, Debug, TryFromPrimitive)]
#[repr(u64)]
enum FcntlCmd {
    DupFd = 0,
    GetFd = 1,
    SetFd = 2,
    GetFl = 3,
    SetFl = 4,
    GetLk = 5,
    SetLk = 6,
    SetLkw = 7,
    OfdGetLk = 36,
    OfdSetLk = 37,
    OfdSetLkw = 38,
    DupFdCloexec = 1030,
    AddSeals = 1033,
    GetSeals = 1034,
}

const O_WRONLY: usize = 0o1;
const O_RDWR: usize = 0o2;
const F_UNLCK: i16 = 2;

bitflags! {
    #[derive(Clone, Copy, Debug)]
    struct FileStatusFlags: u64 {
        const O_NONBLOCK = 0o4_000;
    }
}

bitflags! {
    #[derive(Clone, Copy, Debug)]
    struct DescriptorFlags: u32 {
        const FD_CLOEXEC = 1;
    }
}

#[repr(C)]
#[derive(Clone, Copy)]
struct LinuxFlock {
    lock_type: i16,
    whence: i16,
    start: i64,
    len: i64,
    pid: i32,
    __reserved: i32,
}

fn access_mode_bits(object: &ObjectRef) -> usize {
    let readable = object.clone().as_readable().is_ok();
    let writable = object.clone().as_writable().is_ok();

    match (readable, writable) {
        (false, true) => O_WRONLY,
        (true, true) => O_RDWR,
        _ => 0,
    }
}

pub fn control_object(fd: u64, command: u64, arg: u64) -> SyscallResult {
    let object = get_object_current_process(fd).map_err(SyscallError::from)?;
    match FcntlCmd::try_from(command).map_err(|_| SyscallError::InvalidArguments)? {
        FcntlCmd::SetFl => {
            let mut flags = FileFlags::empty();
            let status_flags = FileStatusFlags::from_bits_truncate(arg);
            if status_flags.contains(FileStatusFlags::O_NONBLOCK) {
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
                        linux_flags |= FileStatusFlags::O_NONBLOCK.bits() as usize;
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
                .clone_object_with_min_and_flags(object, arg as usize, FdFlags::CLOEXEC)
                .map_err(Into::into)
        }),
        FcntlCmd::GetFd => {
            with_current_process(|process| Ok(process.get_fd_flags(fd as usize)?.bits() as usize))
        }
        FcntlCmd::SetFd => with_current_process(|process| {
            let descriptor_flags = DescriptorFlags::from_bits_truncate(arg as u32);
            let flags = if descriptor_flags.contains(DescriptorFlags::FD_CLOEXEC) {
                FdFlags::CLOEXEC
            } else {
                FdFlags::empty()
            };
            process.set_fd_flags(fd as usize, flags)?;
            Ok(0)
        }),
        FcntlCmd::GetLk | FcntlCmd::OfdGetLk => {
            let file_like = object.as_file_like()?;
            let flock_ptr = arg as *mut LinuxFlock;
            if flock_ptr.is_null() {
                return Err(SyscallError::BadAddress);
            }

            let mut flock = user_safe::read(flock_ptr)?;
            let _ = file_like;
            flock.lock_type = F_UNLCK;
            flock.pid = 0;
            user_safe::write(flock_ptr, &flock)?;
            Ok(0)
        }
        FcntlCmd::SetLk | FcntlCmd::SetLkw | FcntlCmd::OfdSetLk | FcntlCmd::OfdSetLkw => {
            let _ = object.as_file_like()?;
            if arg == 0 {
                return Err(SyscallError::BadAddress);
            }
            Ok(0)
        }
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
