use crate::memory::utils::Mut;
use alloc::{collections::BTreeMap, collections::BTreeSet, string::String, sync::Arc};
use core::fmt::{Debug, Formatter, Result as FmtResult};
use num_enum::TryFromPrimitive;

use crate::{
    filesystem::{
        block_device::BlockDevice,
        block_device::cache::CachedBlockDevice,
        cgroupfs::CgroupFs,
        devfs::{DevFs, DevPtsFs},
        impls::ext4::{EXT4, operator::Ext4BlockOperator},
        info::LinuxStat,
        path::Path,
        procfs::ProcFs,
        sysfs::SysFs,
        tmpfs::TmpFs,
        vfs::{FileSystemRef, VirtualFS},
        vfs_traits::MountFlags,
    },
    impl_cast_function, impl_cast_function_non_trait,
    object::{FileFlags, Object, misc::ObjectResult, traits::Statable},
    systemcall::utils::SyscallError,
};
use alloc::boxed::Box;
use ext4plus::Ext4 as Ext4Inner;

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
    picked_mount_path: Option<Path>,
}

pub struct FsContextObject {
    fs_type: String,
    state: Mut<FsContextState>,
}

impl FsContextObject {
    pub fn new(fs_type: String) -> Arc<Self> {
        Arc::new(Self {
            fs_type,
            state: Mut::new(FsContextState {
                flags: BTreeSet::new(),
                strings: BTreeMap::new(),
                created_fs: None,
                picked_mount_path: None,
            }),
        })
    }

    pub fn new_picked(fs_type: String, fs: FileSystemRef, mount_path: Path) -> Arc<Self> {
        Arc::new(Self {
            fs_type,
            state: Mut::new(FsContextState {
                flags: BTreeSet::new(),
                strings: BTreeMap::new(),
                created_fs: Some(fs),
                picked_mount_path: Some(mount_path),
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
                let mut state = self.state.lock();
                if state.picked_mount_path.is_some() {
                    match key {
                        "ro" => {
                            state.flags.remove("rw");
                        }
                        "rw" => {
                            state.flags.remove("ro");
                        }
                        _ => {}
                    }
                }
                state.flags.insert(key.into());
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
        if self.state.lock().created_fs.is_some() {
            return Ok(());
        }
        let fs = self.instantiate_filesystem()?;
        self.state.lock().created_fs = Some(fs);
        Ok(())
    }

    fn reconfigure_filesystem(&self) -> Result<(), SyscallError> {
        let state = self.state.lock();
        if state.created_fs.is_none() {
            return Err(SyscallError::InvalidArguments);
        }
        if let Some(path) = state.picked_mount_path.clone() {
            let mut flags = MountFlags::empty();
            let mut mask = MountFlags::empty();
            if state.flags.contains("ro") {
                flags.insert(MountFlags::MS_RDONLY);
                mask.insert(MountFlags::MS_RDONLY);
            }
            if state.flags.contains("rw") {
                mask.insert(MountFlags::MS_RDONLY);
            }
            drop(state);
            if !mask.is_empty() {
                VirtualFS
                    .lock()
                    .remount_bind(path, flags, mask, false)
                    .map_err(SyscallError::from)?;
            }
        }
        Ok(())
    }

    fn instantiate_filesystem(&self) -> Result<FileSystemRef, SyscallError> {
        match self.fs_type.as_str() {
            "proc" => Ok(Arc::new(Mut::new(ProcFs::new()))),
            "sysfs" => Ok(Arc::new(Mut::new(SysFs::new()))),
            "devtmpfs" => Ok(Arc::new(Mut::new(DevFs::new()))),
            "devpts" => Ok(Arc::new(Mut::new(DevPtsFs::new()))),
            "cgroup2" => Ok(Arc::new(Mut::new(CgroupFs::new()))),
            "tmpfs" => Ok(Arc::new(Mut::new(TmpFs::new()))),
            "ramfs" => Ok(Arc::new(Mut::new(TmpFs::ramfs()))),
            "ext2" | "ext3" | "ext4" => self.instantiate_ext_filesystem(),
            _ => Err(SyscallError::NoDevice),
        }
    }

    fn instantiate_ext_filesystem(&self) -> Result<FileSystemRef, SyscallError> {
        let source = self
            .state
            .lock()
            .strings
            .get("source")
            .cloned()
            .ok_or(SyscallError::InvalidArguments)?;
        let source_object = VirtualFS.lock().open(Path::new(&source))?;
        let block_device = source_object
            .device_backing_object()
            .ok_or(SyscallError::NoDevice)?
            .as_block_device()?;
        let device: Arc<dyn BlockDevice> =
            Arc::new(CachedBlockDevice::new(block_device.backing_device()));
        let reader = Ext4BlockOperator::new(device.clone());
        let writer = Ext4BlockOperator::new(device);
        let ext4 = Ext4Inner::load_with_writer(Box::new(reader), Some(Box::new(writer)))
            .map_err(|_| SyscallError::IOError)?;
        let ext4 = EXT4::new(ext4).map_err(|_| SyscallError::IOError)?;
        Ok(Arc::new(Mut::new(ext4)))
    }

    fn flag_supported(&self, key: &str) -> bool {
        if self.state.lock().picked_mount_path.is_some() {
            return matches!(key, "ro" | "rw");
        }
        match self.fs_type.as_str() {
            "tmpfs" | "ramfs" => matches!(key, "noswap" | "ro"),
            "proc" => false,
            _ => false,
        }
    }

    fn string_supported(&self, key: &str) -> bool {
        if self.state.lock().picked_mount_path.is_some() {
            return key == "sync";
        }
        if key == "source" {
            return true;
        }
        match self.fs_type.as_str() {
            "tmpfs" => matches!(
                key,
                "mode" | "size" | "nr_inodes" | "uid" | "gid" | "nr_blocks"
            ),
            "ramfs" => matches!(key, "mode"),
            "proc" => matches!(key, "hidepid" | "subset"),
            "ext2" | "ext3" | "ext4" => key == "source",
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

    fn set_flags(self: Arc<Self>, _flags: FileFlags) -> ObjectResult<()> {
        Ok(())
    }

    impl_cast_function!("statable", Statable);
    impl_cast_function_non_trait!("fs_context", FsContextObject);
}

impl Statable for FsContextObject {
    fn stat(&self) -> LinuxStat {
        LinuxStat::char_device(0o600)
    }
}
