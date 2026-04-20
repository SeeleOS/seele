use alloc::{collections::BTreeMap, collections::BTreeSet, string::String, sync::Arc};
use core::fmt::{Debug, Formatter, Result as FmtResult};
use num_enum::TryFromPrimitive;
use spin::Mutex;

use crate::{
    filesystem::{info::LinuxStat, tmpfs::TmpFs, vfs::FileSystemRef},
    impl_cast_function, impl_cast_function_non_trait,
    object::{FileFlags, Object, misc::ObjectResult, traits::Statable},
    systemcall::utils::SyscallError,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, TryFromPrimitive)]
#[repr(u32)]
pub enum FsConfigCommand {
    SetFlag = 0,
    SetString = 1,
    SetBinary = 2,
    SetPath = 3,
    SetPathEmpty = 4,
    SetFd = 5,
    CmdCreate = 6,
    CmdReconfigure = 7,
    CmdCreateExcl = 8,
}

struct FsContextState {
    flags: BTreeSet<String>,
    strings: BTreeMap<String, String>,
    created_fs: Option<FileSystemRef>,
}

pub struct FsContextObject {
    fs_type: String,
    state: Mutex<FsContextState>,
}

impl FsContextObject {
    pub fn new(fs_type: String) -> Arc<Self> {
        Arc::new(Self {
            fs_type,
            state: Mutex::new(FsContextState {
                flags: BTreeSet::new(),
                strings: BTreeMap::new(),
                created_fs: None,
            }),
        })
    }

    pub fn configure(
        &self,
        command: FsConfigCommand,
        key: Option<&str>,
        value: Option<&str>,
    ) -> Result<(), SyscallError> {
        match command {
            FsConfigCommand::SetFlag => {
                let key = key.ok_or(SyscallError::InvalidArguments)?;
                if !self.flag_supported(key) {
                    return Err(SyscallError::InvalidArguments);
                }
                self.state.lock().flags.insert(key.into());
                Ok(())
            }
            FsConfigCommand::SetString => {
                let key = key.ok_or(SyscallError::InvalidArguments)?;
                let value = value.ok_or(SyscallError::InvalidArguments)?;
                if !self.string_supported(key) {
                    return Err(SyscallError::InvalidArguments);
                }
                self.state.lock().strings.insert(key.into(), value.into());
                Ok(())
            }
            FsConfigCommand::CmdCreate | FsConfigCommand::CmdCreateExcl => self.create_filesystem(),
            FsConfigCommand::CmdReconfigure => self.reconfigure_filesystem(),
            FsConfigCommand::SetBinary
            | FsConfigCommand::SetPath
            | FsConfigCommand::SetPathEmpty
            | FsConfigCommand::SetFd => Err(SyscallError::InvalidArguments),
        }
    }

    pub fn created_fs(&self) -> Result<FileSystemRef, SyscallError> {
        self.state
            .lock()
            .created_fs
            .clone()
            .ok_or(SyscallError::InvalidArguments)
    }

    pub fn root_mode(&self) -> Result<Option<u32>, SyscallError> {
        let state = self.state.lock();
        state
            .strings
            .get("mode")
            .map(|value| parse_mode(value))
            .transpose()
    }

    fn create_filesystem(&self) -> Result<(), SyscallError> {
        let mut state = self.state.lock();
        if state.created_fs.is_some() {
            return Ok(());
        }
        state.created_fs = Some(self.instantiate_filesystem()?);
        Ok(())
    }

    fn reconfigure_filesystem(&self) -> Result<(), SyscallError> {
        if self.state.lock().created_fs.is_none() {
            return Err(SyscallError::InvalidArguments);
        }
        Ok(())
    }

    fn instantiate_filesystem(&self) -> Result<FileSystemRef, SyscallError> {
        match self.fs_type.as_str() {
            "tmpfs" => Ok(Arc::new(Mutex::new(TmpFs::new()))),
            "ramfs" => Ok(Arc::new(Mutex::new(TmpFs::ramfs()))),
            _ => Err(SyscallError::NoSyscall),
        }
    }

    fn flag_supported(&self, key: &str) -> bool {
        match self.fs_type.as_str() {
            "tmpfs" | "ramfs" => matches!(key, "noswap" | "ro"),
            "proc" => false,
            _ => false,
        }
    }

    fn string_supported(&self, key: &str) -> bool {
        match self.fs_type.as_str() {
            "tmpfs" => matches!(key, "mode" | "size" | "nr_inodes" | "uid" | "gid" | "nr_blocks"),
            "ramfs" => matches!(key, "mode"),
            "proc" => matches!(key, "hidepid" | "subset"),
            _ => false,
        }
    }
}

fn parse_mode(value: &str) -> Result<u32, SyscallError> {
    u32::from_str_radix(value, 8).map_err(|_| SyscallError::InvalidArguments)
}

impl Debug for FsContextObject {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        f.debug_struct("FsContextObject")
            .field("fs_type", &self.fs_type)
            .finish_non_exhaustive()
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
    fn stat(&self) -> LinuxStat {
        LinuxStat::char_device(0o600)
    }
}
