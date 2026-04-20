use crate::filesystem::vfs_traits::MountFlags;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TmpFsVariant {
    TmpFs,
    RamFs,
}

impl TmpFsVariant {
    pub fn name(self) -> &'static str {
        match self {
            Self::TmpFs => "tmpfs",
            Self::RamFs => "ramfs",
        }
    }

    pub fn magic(self) -> i64 {
        match self {
            Self::TmpFs => 0x0102_1994,
            Self::RamFs => 0x8584_58f6,
        }
    }

    pub fn mount_source(self) -> &'static str {
        self.name()
    }

    pub fn default_mount_flags(self) -> MountFlags {
        match self {
            Self::TmpFs => MountFlags::MS_NOSUID | MountFlags::MS_NODEV | MountFlags::MS_RELATIME,
            Self::RamFs => MountFlags::MS_RELATIME,
        }
    }
}
