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

    pub fn mount_options(self) -> &'static str {
        match self {
            Self::TmpFs => "rw,nosuid,nodev,relatime",
            Self::RamFs => "rw,relatime",
        }
    }
}
