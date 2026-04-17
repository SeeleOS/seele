use crate::{filesystem::info::LinuxStat, object::traits::Statable};

use super::UnixSocketObject;

impl Statable for UnixSocketObject {
    fn stat(&self) -> LinuxStat {
        const S_IFSOCK: u32 = 0o140000;

        LinuxStat {
            st_dev: 1,
            st_nlink: 1,
            st_mode: S_IFSOCK | 0o777,
            st_blksize: 4096,
            ..Default::default()
        }
    }
}
