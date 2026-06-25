mod access;
mod fs_magic;
mod path_metadata;

pub(super) use access::*;
pub(in crate::systemcall::implementations) use access::{
    check_access_path_search_permissions, check_access_permissions_for_ids_with_options,
    fs_access_credentials, has_capability,
};
pub(super) use fs_magic::*;
pub(super) use path_metadata::*;

use super::*;
use crate::filesystem::vfs_traits::FileSystemStats;

pub(super) fn linux_major(dev: u64) -> u32 {
    (((dev >> 8) & 0xfff) | ((dev >> 32) & !0xfff)) as u32
}

pub(super) fn linux_minor(dev: u64) -> u32 {
    ((dev & 0xff) | ((dev >> 12) & !0xff)) as u32
}

pub(super) fn linux_statfs_with_flags(
    f_type: i64,
    stats: FileSystemStats,
    flags: MountFlags,
) -> LinuxStatFs {
    LinuxStatFs {
        f_type,
        f_bsize: stats.block_size as i64,
        f_blocks: stats.blocks,
        f_bfree: stats.blocks_free,
        f_bavail: stats.blocks_available,
        f_files: stats.files,
        f_ffree: stats.files_free,
        f_fsid: 1,
        f_namelen: stats.max_name_len as i64,
        f_frsize: stats.fragment_size as i64,
        f_flags: flags.bits() as i64,
        f_spare: [0; 4],
    }
}

pub(super) fn stat_at(
    dirfd: i32,
    path_str: &str,
    flags: AtFlags,
) -> Result<LinuxStat, SyscallError> {
    if path_str.is_empty() && flags.contains(AtFlags::EMPTY_PATH) {
        let object = get_object_current_process(dirfd as u64).map_err(SyscallError::from)?;
        return Ok(object.as_statable()?.stat());
    }

    let path = resolve_path_at(dirfd, path_str)?;
    let open_result = if flags.contains(AtFlags::SYMLINK_NOFOLLOW) {
        open_path_nofollow(path.clone())
    } else {
        open_path(path.clone())
    };
    let object: ObjectRef = Arc::new(open_result?);
    let stat = object.as_statable()?.stat();
    Ok(stat)
}
