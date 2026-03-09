use alloc::string::String;

use crate::filesystem::vfs_traits::{DirectoryContentType, FileLikeType};

#[derive(Debug)]
pub struct DirectoryContentInfo {
    pub name: String,
    pub content_type: DirectoryContentType,
}

#[derive(Debug)]
pub struct FileLikeInfo {
    pub name: String,
    pub size: usize,
    pub file_like_type: FileLikeType,
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
        pub const S_IFDIR: u32 = 0o040000;
        pub const S_IFREG: u32 = 0o100000;

        Self {
            st_dev: 1,
            st_nlink: 1,
            st_mode: match info.file_like_type {
                FileLikeType::File => S_IFREG,
                FileLikeType::Directory => S_IFDIR,
            },
            st_size: info.size as i64,
            st_blksize: 4096,
            st_blocks: (info.size as i64 + 511) / 512,
            ..Default::default()
        }
    }
}

impl FileLikeInfo {
    pub fn new(name: String, size: usize, file_like_type: FileLikeType) -> Self {
        Self {
            name,
            size,
            file_like_type,
        }
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
