use alloc::string::String;

use crate::filesystem::vfs_traits::{DirectoryContentType, FileLikeType};

#[derive(Clone, Debug)]
pub struct DirectoryContentInfo {
    pub name: String,
    pub content_type: DirectoryContentType,
}

#[derive(Debug)]
pub struct FileLikeInfo {
    pub name: String,
    pub size: usize,
    pub inode: u64,
    pub file_like_type: FileLikeType,
    pub permission: UnixPermission,
}

#[derive(Debug)]
pub struct UnixPermission(pub u32);

impl UnixPermission {
    pub fn symlink() -> UnixPermission {
        Self(0o777)
    }

    pub fn directory() -> Self {
        Self(0o755)
    }
}

#[derive(Default, Debug)]
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
            st_size: info.size as i64,
            st_blksize: 4096,
            st_blocks: (info.size as i64 + 511) / 512,
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
            file_like_type,
            permission,
        }
    }

    pub fn with_inode(mut self, inode: u64) -> Self {
        self.inode = inode;
        self
    }

    pub fn as_linux(self) -> LinuxStat {
        LinuxStat::new(self)
    }
}

impl DirectoryContentInfo {
    pub fn new(name: String, content_type: DirectoryContentType) -> Self {
        Self { name, content_type }
    }
}
