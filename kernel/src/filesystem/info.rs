use alloc::string::String;

use crate::filesystem::vfs_traits::{DirectoryContentType, FileLikeType};

#[derive(Clone, Debug)]
pub struct DirectoryContentInfo {
    pub name: String,
    pub content_type: DirectoryContentType,
    pub inode: u64,
    pub permission: Option<UnixPermission>,
    pub rdev: u64,
}

#[derive(Clone, Debug)]
pub struct FileLikeInfo {
    pub name: String,
    pub size: usize,
    pub inode: u64,
    pub uid: u32,
    pub gid: u32,
    pub rdev: u64,
    pub times: FileTimes,
    pub file_like_type: FileLikeType,
    pub permission: UnixPermission,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct FileTimes {
    pub atime_sec: i64,
    pub atime_nsec: i64,
    pub mtime_sec: i64,
    pub mtime_nsec: i64,
    pub ctime_sec: i64,
    pub ctime_nsec: i64,
}

impl FileTimes {
    pub fn now() -> Self {
        Self::from_unix_ns(crate::misc::time::unix_timestamp_nanoseconds().max(1_000_000_000))
    }

    pub const fn from_unix_ns(ns: u64) -> Self {
        let sec = (ns / 1_000_000_000) as i64;
        let nsec = (ns % 1_000_000_000) as i64;
        Self::from_parts(sec, nsec, sec, nsec, sec, nsec)
    }

    pub const fn from_parts(
        atime_sec: i64,
        atime_nsec: i64,
        mtime_sec: i64,
        mtime_nsec: i64,
        ctime_sec: i64,
        ctime_nsec: i64,
    ) -> Self {
        Self {
            atime_sec,
            atime_nsec,
            mtime_sec,
            mtime_nsec,
            ctime_sec,
            ctime_nsec,
        }
    }
}

#[derive(Clone, Debug)]
pub struct UnixPermission(pub u32);

impl UnixPermission {
    pub fn symlink() -> UnixPermission {
        Self(0o777)
    }

    pub fn directory() -> Self {
        Self(0o755)
    }
}

#[derive(Clone, Copy, Default, Debug)]
#[repr(C)]
pub struct LinuxStat {
    pub st_dev: u64,     // 随便填个 1
    pub st_ino: u64,     // 填文件在 VFS 里的唯一 ID，或者随便填个数
    pub st_nlink: u64,   // 【重要】填 1
    pub st_mode: u32,    // 【最重要】类型与权限
    pub st_uid: u32,     // 填 0 (Root)
    pub st_gid: u32,     // 填 0 (Root)
    pub __pad0: u32,     // 必须保留，用来对齐 8 字节
    pub st_rdev: u64,    // 填 0
    pub st_size: i64,    // 【重要】填文件字节数
    pub st_blksize: i64, // 填 512 或 4096
    pub st_blocks: i64,  // 填 (size + 511) / 512

    // 时间戳部分（如果不想管，全都填 0，但结构体位置要留够）
    pub st_atime: i64,
    pub st_atime_nsec: i64,
    pub st_mtime: i64,
    pub st_mtime_nsec: i64,
    pub st_ctime: i64,
    pub st_ctime_nsec: i64,
    pub __unused: [i64; 3],
}

impl LinuxStat {
    pub fn linux_makedev(major: u64, minor: u64) -> u64 {
        ((major & 0xfff) << 8) | (minor & 0xff) | ((minor & !0xff) << 12) | ((major & !0xfff) << 32)
    }

    pub fn new(info: FileLikeInfo) -> Self {
        pub const S_IFMT: u32 = 0o170000;
        pub const S_IFDIR: u32 = 0o040000;
        pub const S_IFREG: u32 = 0o100000;
        pub const S_IFLNK: u32 = 0o120000;

        let file_type_bits = match info.file_like_type {
            FileLikeType::File => S_IFREG,
            FileLikeType::Directory => S_IFDIR,
            FileLikeType::Symlink => S_IFLNK,
        };
        let st_mode = if info.permission.0 & S_IFMT == 0 {
            info.permission.0 | file_type_bits
        } else {
            info.permission.0
        };

        Self {
            st_dev: 1,
            st_ino: info.inode,
            st_nlink: 1,
            st_mode,
            st_uid: info.uid,
            st_gid: info.gid,
            st_rdev: info.rdev,
            st_size: info.size as i64,
            st_blksize: 4096,
            st_blocks: (info.size as i64 + 511) / 512,
            st_atime: info.times.atime_sec,
            st_atime_nsec: info.times.atime_nsec,
            st_mtime: info.times.mtime_sec,
            st_mtime_nsec: info.times.mtime_nsec,
            st_ctime: info.times.ctime_sec,
            st_ctime_nsec: info.times.ctime_nsec,
            ..Default::default()
        }
    }

    pub fn char_device(permission: u32) -> Self {
        Self::char_device_with_rdev(permission, 0)
    }

    pub fn char_device_with_rdev(permission: u32, rdev: u64) -> Self {
        pub const S_IFCHR: u32 = 0o020000;

        Self {
            st_dev: 1,
            st_nlink: 1,
            st_mode: S_IFCHR | permission,
            st_rdev: rdev,
            st_blksize: 4096,
            ..Default::default()
        }
    }

    pub fn block_device_with_rdev(permission: u32, rdev: u64) -> Self {
        pub const S_IFBLK: u32 = 0o060000;

        Self {
            st_dev: 1,
            st_nlink: 1,
            st_mode: S_IFBLK | permission,
            st_rdev: rdev,
            st_blksize: 4096,
            ..Default::default()
        }
    }
}

impl FileLikeInfo {
    pub fn new(
        name: String,
        size: usize,
        permission: UnixPermission,
        file_like_type: FileLikeType,
    ) -> Self {
        Self {
            name,
            size,
            inode: 0,
            uid: 0,
            gid: 0,
            rdev: 0,
            times: FileTimes::default(),
            file_like_type,
            permission,
        }
    }

    pub fn with_owner(mut self, uid: u32, gid: u32) -> Self {
        self.uid = uid;
        self.gid = gid;
        self
    }

    pub fn with_inode(mut self, inode: u64) -> Self {
        self.inode = inode;
        self
    }

    pub fn with_rdev(mut self, rdev: u64) -> Self {
        self.rdev = rdev;
        self
    }

    pub fn with_times(mut self, times: FileTimes) -> Self {
        self.times = times;
        self
    }

    pub fn as_linux(self) -> LinuxStat {
        LinuxStat::new(self)
    }
}

impl DirectoryContentInfo {
    pub fn new(name: String, content_type: DirectoryContentType) -> Self {
        Self {
            name,
            content_type,
            inode: 0,
            permission: None,
            rdev: 0,
        }
    }

    pub fn with_inode(mut self, inode: u64) -> Self {
        self.inode = inode;
        self
    }

    pub fn with_permission(mut self, permission: UnixPermission) -> Self {
        self.permission = Some(permission);
        self
    }

    pub fn with_rdev(mut self, rdev: u64) -> Self {
        self.rdev = rdev;
        self
    }
}
