use alloc::{string::String, sync::Arc};
use num_enum::TryFromPrimitive;

use crate::{
    impl_cast_function, impl_cast_function_non_trait,
    object::{FileFlags, Object, misc::ObjectResult, traits::Statable},
    systemcall::utils::SyscallError,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, TryFromPrimitive)]
#[repr(u32)]
pub enum FsConfigCommand {
    SetFlag = 0,
    SetString = 1,
    SetFd = 5,
}

#[derive(Debug)]
pub struct FsContextObject {
    fs_type: String,
}

impl FsContextObject {
    pub fn new(fs_type: String) -> Arc<Self> {
        Arc::new(Self { fs_type })
    }

    pub fn configure(
        &self,
        command: FsConfigCommand,
        key: Option<&str>,
        value: Option<&str>,
    ) -> Result<(), SyscallError> {
        match command {
            FsConfigCommand::SetFd => Err(SyscallError::InvalidArguments),
            FsConfigCommand::SetFlag | FsConfigCommand::SetString => {
                if self.option_supported(key.unwrap_or_default(), value) {
                    Ok(())
                } else {
                    Err(SyscallError::InvalidArguments)
                }
            }
        }
    }

    fn option_supported(&self, key: &str, value: Option<&str>) -> bool {
        match self.fs_type.as_str() {
            "tmpfs" => matches!(
                (key, value),
                ("mode", Some(_))
                    | ("size", Some(_))
                    | ("nr_inodes", Some(_))
                    | ("uid", Some(_))
                    | ("gid", Some(_))
                    | ("nr_blocks", Some(_))
            ),
            "proc" => matches!((key, value), ("hidepid", Some(_)) | ("subset", Some(_))),
            _ => false,
        }
    }
}

impl Object for FsContextObject {
    fn get_flags(self: Arc<Self>) -> ObjectResult<FileFlags> {
        Ok(FileFlags::empty())
    }

    impl_cast_function!("statable", Statable);
    impl_cast_function_non_trait!("fs_context", FsContextObject);
}

impl Statable for FsContextObject {
    fn stat(&self) -> crate::filesystem::info::LinuxStat {
        crate::filesystem::info::LinuxStat::char_device(0o600)
    }
}
