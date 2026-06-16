use crate::{
    define_syscall,
    filesystem::{
        absolute_path::AbsolutePath,
        errors::FSError,
        fusefs::FuseFs,
        info::{DirectoryContentInfo, FileLikeInfo, LinuxStat},
        object::{FileLikeObject, mount_device_id_for_path},
        path::Path,
        tmpfs::TmpFs,
        vfs::VirtualFS,
        vfs_operations::{
            file_info_path, open_path, open_path_nofollow, resolve_dir_path,
            resolve_path_with_mount_info,
        },
        vfs_traits::{DirectoryContentType, FileLikeType, MountFlags},
    },
    memory::user_safe,
    misc::{
        c_types::CString,
        others::KernelFrom,
        profile::{self, HotSyscallPhase},
    },
    object::{
        FileFlags,
        error::ObjectError,
        fs_context::{FsConfigCommand, FsContextObject},
        misc::{ObjectRef, get_object_current_process},
        traits::Statable,
    },
    process::{FdFlags, manager::get_current_process},
    systemcall::utils::{SyscallError, SyscallImpl},
};
use alloc::{format, string::String, sync::Arc, vec::Vec};
use bitflags::bitflags;
use core::{
    mem::size_of,
    sync::atomic::{AtomicU64, Ordering},
};

mod directory;
mod fsinfo;
mod metadata;
mod mount;
mod mount_helpers;
mod open;
mod path_helpers;
mod path_ops;
mod stat;
mod time;
mod types;
mod xattr;
mod xattr_helpers;

use metadata::*;
use mount_helpers::*;
use path_helpers::*;
use types::*;
pub(crate) use types::{
    AtFlags, FsMountFlags, FsOpenFlags, MoveMountFlags, OpenFlags, OpenTreeFlags, UmountFlags,
    XattrFlags,
};
use xattr_helpers::*;

pub use directory::*;
pub use fsinfo::*;
pub use mount::*;
pub use open::*;
pub use path_ops::*;
pub use stat::*;
pub use time::*;
pub use xattr::*;

#[cfg(test)]
mod tests {
    use crate::systemcall::test::*;

    crate::test!(
        filesystem_path_state_syscalls,
        "filesystem path state syscalls follow linux rules",
        filesystem_path_state_syscalls_follow_linux_rules
    );
    crate::test!(
        filesystem_create_link_syscalls,
        "filesystem create link syscalls follow linux rules",
        filesystem_create_link_syscalls_follow_linux_rules
    );
    crate::test!(
        opened_file_object_stat_mount_device_id,
        "opened file object stat keeps mount device id without reborrowing vfs",
        opened_file_object_stat_keeps_mount_device_id_without_reborrowing_vfs
    );
    crate::test!(
        filesystem_fd_state_syscalls,
        "filesystem fd state syscalls follow linux rules",
        filesystem_fd_state_syscalls_follow_linux_rules
    );
    crate::test!(
        filesystem_metadata_syscalls,
        "filesystem metadata syscalls follow linux rules",
        filesystem_metadata_syscalls_follow_linux_rules
    );
    crate::test!(
        filesystem_io_syscalls,
        "filesystem io syscalls follow linux rules",
        filesystem_io_syscalls_follow_linux_rules
    );
    crate::test!(
        filesystem_rename_syscalls,
        "filesystem rename syscalls follow linux rules",
        filesystem_rename_syscalls_follow_linux_rules
    );
    crate::test!(
        filesystem_getdents_syscalls,
        "filesystem getdents syscalls follow linux rules",
        filesystem_getdents_syscalls_follow_linux_rules
    );
    crate::test!(
        filesystem_file_object_syscalls,
        "filesystem file object syscalls follow linux rules",
        filesystem_file_object_syscalls_follow_linux_rules
    );
    crate::test!(
        filesystem_file_metadata_syscalls,
        "filesystem file metadata syscalls follow linux rules",
        filesystem_file_metadata_syscalls_follow_linux_rules
    );
    crate::test!(
        filesystem_xattr_syscalls,
        "filesystem xattr syscalls follow linux rules",
        filesystem_xattr_syscalls_follow_linux_rules
    );
    crate::test!(
        filesystem_statx_syscalls,
        "statx follows linux rules",
        filesystem_statx_syscalls_follow_linux_rules
    );
    crate::test!(
        filesystem_name_to_handle_short_buffer_syscalls,
        "name_to_handle_at short buffer follows linux rules",
        filesystem_name_to_handle_short_buffer_syscalls_follow_linux_rules
    );
    crate::test!(
        filesystem_name_to_handle_success_syscalls,
        "name_to_handle_at success path follows linux rules",
        filesystem_name_to_handle_success_syscalls_follow_linux_rules
    );
    crate::test!(
        filesystem_name_to_handle_null_handle_syscalls,
        "name_to_handle_at null handle follows linux rules",
        filesystem_name_to_handle_null_handle_syscalls_follow_linux_rules
    );
    crate::test!(
        filesystem_name_to_handle_null_mount_id_syscalls,
        "name_to_handle_at null mount id follows linux rules",
        filesystem_name_to_handle_null_mount_id_syscalls_follow_linux_rules
    );
    crate::test!(
        filesystem_name_to_handle_bad_flag_syscalls,
        "name_to_handle_at invalid flag follows linux rules",
        filesystem_name_to_handle_bad_flag_syscalls_follow_linux_rules
    );
    crate::test!(
        filesystem_utimensat_success_syscalls,
        "utimensat success paths follow linux rules",
        filesystem_utimensat_success_syscalls_follow_linux_rules
    );
    crate::test!(
        filesystem_utimensat_negative_nsec_syscalls,
        "utimensat rejects invalid nanoseconds like linux",
        filesystem_utimensat_negative_nsec_syscalls_follow_linux_rules
    );
    crate::test!(
        filesystem_utimensat_null_path_empty_path_syscalls,
        "utimensat rejects null path with empty_path like linux",
        filesystem_utimensat_null_path_empty_path_syscalls_follow_linux_rules
    );
    crate::test!(
        filesystem_utimensat_empty_path_without_flag_syscalls,
        "utimensat rejects empty path without empty_path like linux",
        filesystem_utimensat_empty_path_without_flag_syscalls_follow_linux_rules
    );
    crate::test!(
        filesystem_utimensat_at_fdcwd_null_path_syscalls,
        "utimensat rejects at_fdcwd with null path like linux",
        filesystem_utimensat_at_fdcwd_null_path_syscalls_follow_linux_rules
    );
    crate::test!(
        filesystem_utimensat_invalid_flag_syscalls,
        "utimensat rejects invalid flags like linux",
        filesystem_utimensat_invalid_flag_syscalls_follow_linux_rules
    );
    crate::test!(
        procfs_syscalls,
        "procfs syscall paths follow linux proc abi rules",
        procfs_syscalls_follow_linux_proc_abi_rules
    );
    crate::test!(
        sysfs_syscalls,
        "sysfs syscall paths follow linux sysfs abi rules",
        sysfs_syscalls_follow_linux_sysfs_abi_rules
    );
}
