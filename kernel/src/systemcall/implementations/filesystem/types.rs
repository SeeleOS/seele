use super::*;

pub(super) const AT_FDCWD: i32 = -100;
pub(super) const UTIME_NOW: i64 = 0x3fff_ffff;
pub(super) const UTIME_OMIT: i64 = 0x3fff_fffe;
pub(super) const STATX_BASIC_STATS: u32 = 0x0000_07ff;
pub(super) const STATX_BTIME: u32 = 0x0000_0800;
pub(super) const STATX_MNT_ID: u32 = 0x0000_1000;
pub(super) const STATX_DIOALIGN: u32 = 0x0000_2000;
pub(super) const STATX_ATTR_MOUNT_ROOT: u64 = 0x0000_2000;
pub(super) const ANON_INODE_FS_MAGIC: i64 = 0x0904_1934;
pub(super) const SOCKFS_MAGIC: i64 = 0x534f_434b;
pub(super) const PIPEFS_MAGIC: i64 = 0x5049_5045;
pub(super) const AT_STATX_FORCE_SYNC: i32 = 0x2000;
pub(super) const AT_STATX_DONT_SYNC: i32 = 0x4000;
pub(super) const S_IFMT: u32 = 0o170000;
pub(super) const SEELE_FILE_HANDLE_TYPE_INODE: i32 = 1;
pub(super) const S_IFDIR: u32 = 0o040000;
pub(super) const S_IFREG: u32 = 0o100000;
pub(super) const S_IFIFO: u32 = 0o010000;
pub(super) const S_IFCHR: u32 = 0o020000;
pub(super) const S_IFBLK: u32 = 0o060000;
pub(super) const S_IFSOCK: u32 = 0o140000;
pub(super) const API_MOUNT_ROOT: &str = "/run/.api-mounts";
pub(super) const MOUNT_ATTR_RDONLY: u64 = MountFlags::MS_RDONLY.bits();
pub(super) const MOUNT_ATTR_NOSUID: u64 = MountFlags::MS_NOSUID.bits();
pub(super) const MOUNT_ATTR_NODEV: u64 = MountFlags::MS_NODEV.bits();
pub(super) const MOUNT_ATTR_NOEXEC: u64 = MountFlags::MS_NOEXEC.bits();
pub(super) const MOUNT_ATTR__ATIME: u64 = 0x0000_0070;
pub(super) const MOUNT_ATTR_NOATIME: u64 = 0x0000_0010;
pub(super) const MOUNT_ATTR_STRICTATIME: u64 = 0x0000_0020;
pub(super) const MOUNT_ATTR_NODIRATIME: u64 = 0x0000_0080;
pub(super) const MOUNT_ATTR_IDMAP: u64 = 0x0010_0000;
pub(super) const MOUNT_ATTR_NOSYMFOLLOW: u64 = 0x0020_0000;

pub(super) static NEXT_API_MOUNT_ID: AtomicU64 = AtomicU64::new(1);
pub(super) static NEXT_TMPFILE_ID: AtomicU64 = AtomicU64::new(1);

bitflags! {
    #[derive(Clone, Copy, Debug)]
    pub(crate) struct AtFlags: i32 {
        const REMOVEDIR = 0x200;
        const SYMLINK_NOFOLLOW = 0x100;
        const SYMLINK_FOLLOW = 0x400;
        const NO_AUTOMOUNT = 0x800;
        const EMPTY_PATH = 0x1000;
        const STATX_FORCE_SYNC = 0x2000;
        const STATX_DONT_SYNC = 0x4000;
        const EACCESS = 0x200;
        const RECURSIVE = 0x8000;
    }
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(super) struct StatxTimestamp {
    pub(super) tv_sec: i64,
    pub(super) tv_nsec: u32,
    pub(super) __reserved: i32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub(super) struct LinuxTimespec {
    pub(super) tv_sec: i64,
    pub(super) tv_nsec: i64,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(super) struct LinuxStatx {
    pub(super) stx_mask: u32,
    pub(super) stx_blksize: u32,
    pub(super) stx_attributes: u64,
    pub(super) stx_nlink: u32,
    pub(super) stx_uid: u32,
    pub(super) stx_gid: u32,
    pub(super) stx_mode: u16,
    pub(super) __spare0: u16,
    pub(super) stx_ino: u64,
    pub(super) stx_size: u64,
    pub(super) stx_blocks: u64,
    pub(super) stx_attributes_mask: u64,
    pub(super) stx_atime: StatxTimestamp,
    pub(super) stx_btime: StatxTimestamp,
    pub(super) stx_ctime: StatxTimestamp,
    pub(super) stx_mtime: StatxTimestamp,
    pub(super) stx_rdev_major: u32,
    pub(super) stx_rdev_minor: u32,
    pub(super) stx_dev_major: u32,
    pub(super) stx_dev_minor: u32,
    pub(super) stx_mnt_id: u64,
    pub(super) stx_dio_mem_align: u32,
    pub(super) stx_dio_offset_align: u32,
    pub(super) __spare3: [u64; 12],
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(super) struct LinuxStatFs {
    pub(super) f_type: i64,
    pub(super) f_bsize: i64,
    pub(super) f_blocks: u64,
    pub(super) f_bfree: u64,
    pub(super) f_bavail: u64,
    pub(super) f_files: u64,
    pub(super) f_ffree: u64,
    pub(super) f_fsid: i64,
    pub(super) f_namelen: i64,
    pub(super) f_frsize: i64,
    pub(super) f_flags: i64,
    pub(super) f_spare: [i64; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(super) struct LinuxMountAttr {
    pub(super) attr_set: u64,
    pub(super) attr_clr: u64,
    pub(super) propagation: u64,
    pub(super) userns_fd: u64,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(super) struct LinuxOpenHow {
    pub(super) flags: u64,
    pub(super) mode: u64,
    pub(super) resolve: u64,
}

bitflags! {
    #[derive(Clone, Copy, Debug)]
    pub(crate) struct OpenFlags: i32 {
        const CREAT = 0x40;
        const EXCL = 0x80;
        const NOCTTY = 0x100;
        const TRUNC = 0x200;
        const APPEND = 0o2_000;
        const NONBLOCK = 0o4_000;
        const DSYNC = 0o10_000;
        const DIRECT = 0o40_000;
        const LARGEFILE = 0o100_000;
        const DIRECTORY = 0o200000;
        const NOFOLLOW = 0o400000;
        const NOATIME = 0o1000000;
        const CLOEXEC = 0o2000000;
        const SYNC = 0x101000;
        const PATH = 0o10000000;
        const TMPFILE = 0o20200000;
    }
}

bitflags! {
    #[derive(Clone, Copy, Debug)]
    pub(crate) struct OpenResolveFlags: u64 {
        const RESOLVE_NO_XDEV = 0x01;
        const RESOLVE_NO_MAGICLINKS = 0x02;
        const RESOLVE_NO_SYMLINKS = 0x04;
        const RESOLVE_BENEATH = 0x08;
        const RESOLVE_IN_ROOT = 0x10;
        const RESOLVE_CACHED = 0x20;
    }
}

bitflags! {
    #[derive(Clone, Copy, Debug)]
    pub(crate) struct XattrFlags: u32 {
        const CREATE = 0x1;
        const REPLACE = 0x2;
    }
}

bitflags! {
    #[derive(Clone, Copy, Debug)]
    pub(crate) struct UmountFlags: i32 {
        const FORCE = 0x1;
        const DETACH = 0x2;
        const EXPIRE = 0x4;
        const NOFOLLOW = 0x8;
    }
}

bitflags! {
    #[derive(Clone, Copy, Debug)]
    pub(crate) struct FsOpenFlags: u32 {
        const FSCONTEXT_CLOEXEC = 0x1;
    }
}

bitflags! {
    #[derive(Clone, Copy, Debug)]
    pub(crate) struct FsMountFlags: u32 {
        const FSMOUNT_CLOEXEC = 0x1;
    }
}

bitflags! {
    #[derive(Clone, Copy, Debug)]
    pub(crate) struct MoveMountFlags: u32 {
        const MOVE_MOUNT_F_SYMLINKS = 0x0000_0001;
        const MOVE_MOUNT_F_AUTOMOUNTS = 0x0000_0002;
        const MOVE_MOUNT_F_EMPTY_PATH = 0x0000_0004;
        const MOVE_MOUNT_F_NOFOLLOW = 0x0000_0008;
        const MOVE_MOUNT_T_SYMLINKS = 0x0000_0010;
        const MOVE_MOUNT_T_AUTOMOUNTS = 0x0000_0020;
        const MOVE_MOUNT_T_EMPTY_PATH = 0x0000_0040;
        const MOVE_MOUNT_SET_GROUP = 0x0000_0100;
        const MOVE_MOUNT_BENEATH = 0x0000_0200;
    }
}

bitflags! {
    #[derive(Clone, Copy, Debug)]
    pub(crate) struct FsPickFlags: u32 {
        const FSPICK_CLOEXEC = 0x0000_0001;
        const FSPICK_SYMLINK_NOFOLLOW = 0x0000_0002;
        const FSPICK_NO_AUTOMOUNT = 0x0000_0004;
        const FSPICK_EMPTY_PATH = 0x0000_0008;
    }
}

bitflags! {
    #[derive(Clone, Copy, Debug)]
    pub(crate) struct OpenTreeFlags: u32 {
        const OPEN_TREE_CLONE = 0x0000_0001;
        const AT_SYMLINK_NOFOLLOW = AtFlags::SYMLINK_NOFOLLOW.bits() as u32;
        const AT_NO_AUTOMOUNT = AtFlags::NO_AUTOMOUNT.bits() as u32;
        const AT_EMPTY_PATH = AtFlags::EMPTY_PATH.bits() as u32;
        const OPEN_TREE_CLOEXEC = 0x0008_0000;
    }
}

bitflags! {
    #[derive(Clone, Copy, Debug)]
    pub(crate) struct OpenTreeAttrFlags: u64 {
        const OPEN_TREE_CLONE = OpenTreeFlags::OPEN_TREE_CLONE.bits() as u64;
        const AT_SYMLINK_NOFOLLOW = AtFlags::SYMLINK_NOFOLLOW.bits() as u64;
        const AT_NO_AUTOMOUNT = AtFlags::NO_AUTOMOUNT.bits() as u64;
        const AT_EMPTY_PATH = AtFlags::EMPTY_PATH.bits() as u64;
        const OPEN_TREE_CLOEXEC = OpenTreeFlags::OPEN_TREE_CLOEXEC.bits() as u64;
        const RECURSIVE = AtFlags::RECURSIVE.bits() as u64;
    }
}

bitflags! {
    #[derive(Clone, Copy, Debug)]
    pub(super) struct MountOperationFlags: u64 {
        const MS_REMOUNT = 32;
        const MS_BIND = 4096;
        const MS_MOVE = 8192;
        const MS_REC = 16384;
        const MS_UNBINDABLE = 1 << 17;
        const MS_PRIVATE = 1 << 18;
        const MS_SLAVE = 1 << 19;
        const MS_SHARED = 1 << 20;
    }
}

#[repr(C)]
#[derive(Clone, Copy)]
pub(super) struct LinuxFileHandle {
    pub(super) handle_bytes: u32,
    pub(super) handle_type: i32,
    pub(super) f_handle: [u8; 0],
}

#[repr(C)]
#[derive(Clone, Copy)]
pub(super) struct SeeleFileHandle {
    pub(super) inode: u64,
}
