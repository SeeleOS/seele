use crate::{
    define_syscall,
    filesystem::{
        absolute_path::AbsolutePath,
        cgroupfs::CgroupFs,
        devfs::{DevFs, DevPtsFs},
        errors::FSError,
        fusefs::FuseFs,
        info::{DirectoryContentInfo, FileLikeInfo, LinuxStat},
        object::{FileLikeObject, mount_device_id_for_path},
        path::Path,
        procfs::ProcFs,
        sysfs::SysFs,
        tmpfs::TmpFs,
        vfs::{FileSystemRef, VirtualFS},
        vfs_operations::{
            file_info_path, open_path, open_path_nofollow, resolve_dir_path,
            resolve_path_with_mount_info,
        },
        vfs_traits::{DirectoryContentType, FileLikeType, MountFlags},
    },
    memory::{user_safe, utils::Mut},
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
    use super::*;
    use crate::systemcall::implementations::{Close, Lseek, Read};
    use crate::systemcall::test::*;
    use alloc::{string::ToString, vec};

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
    fn filesystem_path_state_syscalls_follow_linux_rules() {
        const AT_FDCWD: u64 = (-100i32) as u64;
        const AT_EMPTY_PATH: u64 = 0x1000;
        const AT_EACCESS: u64 = 0x200;

        let process = get_current_process();
        let saved_fs_context = process.lock().fs_context.lock().clone();
        let base_path = Path::new("/tmp/syscall-path-state-test");
        let subdir_path = Path::new("/tmp/syscall-path-state-test/subdir");
        let locked_file_path = Path::new("/tmp/syscall-path-state-test/locked");
        let existing_file_path = Path::new("/tmp/syscall-path-state-test/existing");
        let _ = VirtualFS.lock().delete_file(existing_file_path.clone());
        let _ = VirtualFS.lock().delete_file(locked_file_path.clone());
        let _ = VirtualFS.lock().delete_file(subdir_path.clone());
        let _ = VirtualFS.lock().delete_file(base_path.clone());
        VirtualFS.lock().create_dir(base_path.clone()).unwrap();
        VirtualFS.lock().create_dir(subdir_path.clone()).unwrap();
        VirtualFS
            .lock()
            .create_file(locked_file_path.clone())
            .unwrap();
        VirtualFS
            .lock()
            .open(locked_file_path.clone())
            .unwrap()
            .chmod(0)
            .unwrap();
        VirtualFS
            .lock()
            .create_file(existing_file_path.clone())
            .unwrap();

        let user_page = allocate_user_test_page();
        write_user_cstr(user_page, b"/tmp/syscall-path-state-test/locked\0");
        expect_ok(
            SyscallArgs::new([user_page, 0, 0, 0, 0, 0]).call::<Access>(),
            0,
        );
        expect_errno(
            SyscallArgs::new([user_page, 4, 0, 0, 0, 0]).call::<Access>(),
            SyscallError::AccessDenied,
        );
        expect_errno(
            SyscallArgs::new([user_page, 8, 0, 0, 0, 0]).call::<Access>(),
            SyscallError::InvalidArguments,
        );

        write_user_cstr(user_page, b"/tmp/syscall-path-state-test/existing\0");
        let file_fd = expect_fd(
            SyscallArgs::new([
                AT_FDCWD,
                user_page,
                OpenFlags::empty().bits() as u64,
                0,
                0,
                0,
            ])
            .call::<OpenAt>(),
        );
        expect_ok(
            SyscallArgs::new([file_fd as u64, user_page + 128, 0, AT_EMPTY_PATH, 0, 0])
                .call::<Faccessat>(),
            0,
        );
        expect_ok(
            SyscallArgs::new([
                file_fd as u64,
                user_page + 128,
                0,
                AT_EMPTY_PATH | AT_EACCESS,
                0,
                0,
            ])
            .call::<Faccessat2>(),
            0,
        );
        expect_errno(
            SyscallArgs::new([file_fd as u64, user_page + 128, 0, 0, 0, 0]).call::<Faccessat>(),
            SyscallError::InvalidArguments,
        );
        expect_errno(
            SyscallArgs::new([file_fd as u64, user_page + 128, 0, 0x8000_0000, 0, 0])
                .call::<Faccessat2>(),
            SyscallError::NoSyscall,
        );
        close_test_fd(file_fd);

        write_user_cstr(user_page, b"/tmp/syscall-path-state-test/subdir\0");
        expect_ok(
            SyscallArgs::new([user_page, 0, 0, 0, 0, 0]).call::<Chdir>(),
            0,
        );
        {
            let current = process.lock().fs_context.lock().current_directory.clone();
            assert_eq!(current.as_string(), "/tmp/syscall-path-state-test/subdir");
        }
        expect_ok(
            SyscallArgs::new([user_page + 256, 64, 0, 0, 0, 0]).call::<Getcwd>(),
            b"/tmp/syscall-path-state-test/subdir\0".len(),
        );
        assert_user_bytes(user_page + 256, b"/tmp/syscall-path-state-test/subdir\0");
        expect_errno(
            SyscallArgs::new([user_page + 384, 4, 0, 0, 0, 0]).call::<Getcwd>(),
            SyscallError::RangeError,
        );
        expect_errno(
            SyscallArgs::new([0, 64, 0, 0, 0, 0]).call::<Getcwd>(),
            SyscallError::BadAddress,
        );

        let dir_fd = expect_fd(
            SyscallArgs::new([
                AT_FDCWD,
                user_page,
                OpenFlags::empty().bits() as u64,
                0,
                0,
                0,
            ])
            .call::<OpenAt>(),
        );
        write_user_cstr(user_page, b"/tmp/syscall-path-state-test/existing\0");
        let non_dir_fd = expect_fd(
            SyscallArgs::new([
                AT_FDCWD,
                user_page,
                OpenFlags::empty().bits() as u64,
                0,
                0,
                0,
            ])
            .call::<OpenAt>(),
        );
        expect_errno(
            SyscallArgs::new([non_dir_fd as u64, 0, 0, 0, 0, 0]).call::<Fchdir>(),
            SyscallError::NotADirectory,
        );
        expect_ok(
            SyscallArgs::new([dir_fd as u64, 0, 0, 0, 0, 0]).call::<Fchdir>(),
            0,
        );
        {
            let current = process.lock().fs_context.lock().current_directory.clone();
            assert_eq!(current.as_string(), "/tmp/syscall-path-state-test/subdir");
        }
        close_test_fd(non_dir_fd);
        close_test_fd(dir_fd);

        {
            let process = process.lock();
            process.fs_context.lock().current_directory =
                AbsolutePath::from_root_path(&Path::new("/tmp/syscall-path-state-test/subdir"));
        }
        write_user_cstr(user_page, b"/tmp\0");
        expect_ok(
            SyscallArgs::new([user_page, 0, 0, 0, 0, 0]).call::<Chroot>(),
            0,
        );
        {
            let fs_context = process.lock().fs_context.lock().clone();
            assert_eq!(fs_context.root_directory.clone().as_string(), "/tmp");
            assert_eq!(
                fs_context
                    .current_directory
                    .display_string(&fs_context.root_directory),
                "/syscall-path-state-test/subdir"
            );
        }
        write_user_cstr(user_page, b"/syscall-path-state-test/existing\0");
        expect_errno(
            SyscallArgs::new([user_page, 0, 0, 0, 0, 0]).call::<Chroot>(),
            SyscallError::NotADirectory,
        );

        {
            *process.lock().fs_context.lock() = saved_fs_context;
        }
        let _ = VirtualFS.lock().delete_file(existing_file_path);
        let _ = VirtualFS.lock().delete_file(locked_file_path);
        let _ = VirtualFS.lock().delete_file(subdir_path);
        let _ = VirtualFS.lock().delete_file(base_path);
    }

    fn filesystem_create_link_syscalls_follow_linux_rules() {
        const AT_FDCWD: u64 = (-100i32) as u64;
        const AT_EMPTY_PATH: u64 = 0x1000;
        const AT_REMOVEDIR: u64 = 0x200;

        let base_path = Path::new("/tmp/syscall-create-link-test");
        let cleanup_paths = [
            "/tmp/syscall-create-link-test/fdhard",
            "/tmp/syscall-create-link-test/hard",
            "/tmp/syscall-create-link-test/src",
            "/tmp/syscall-create-link-test/atlink",
            "/tmp/syscall-create-link-test/link",
            "/tmp/syscall-create-link-test/nonempty/child",
            "/tmp/syscall-create-link-test/nonempty",
            "/tmp/syscall-create-link-test/atdir",
            "/tmp/syscall-create-link-test/dir",
            "/tmp/syscall-create-link-test",
        ];
        for path in cleanup_paths {
            let _ = VirtualFS.lock().delete_file(Path::new(path));
        }
        VirtualFS.lock().create_dir(base_path.clone()).unwrap();
        VirtualFS
            .lock()
            .create_file(Path::new("/tmp/syscall-create-link-test/src"))
            .unwrap();

        let user_page = allocate_user_test_page();
        write_user_cstr(user_page, b"/tmp/syscall-create-link-test/dir\0");
        expect_ok(
            SyscallArgs::new([user_page, 0o755, 0, 0, 0, 0]).call::<Mkdir>(),
            0,
        );
        expect_errno(
            SyscallArgs::new([user_page, 0o755, 0, 0, 0, 0]).call::<Mkdir>(),
            SyscallError::FileAlreadyExists,
        );
        let dir_object = {
            let mut vfs = VirtualFS.lock();
            vfs.open(Path::new("/tmp/syscall-create-link-test/dir"))
                .unwrap()
        };
        let dir_stat = dir_object.stat();
        assert_eq!(dir_stat.st_mode & 0o777, 0o755);

        write_user_cstr(user_page, b"/tmp/syscall-create-link-test/src\0");
        expect_errno(
            SyscallArgs::new([user_page, 0, 0, 0, 0, 0]).call::<Rmdir>(),
            SyscallError::NotADirectory,
        );
        write_user_cstr(user_page, b"/tmp/syscall-create-link-test/dir\0");
        expect_errno(
            SyscallArgs::new([AT_FDCWD, user_page, 0, 0, 0, 0]).call::<UnlinkAt>(),
            SyscallError::IsADirectory,
        );

        VirtualFS
            .lock()
            .create_dir(Path::new("/tmp/syscall-create-link-test/nonempty"))
            .unwrap();
        VirtualFS
            .lock()
            .create_file(Path::new("/tmp/syscall-create-link-test/nonempty/child"))
            .unwrap();
        write_user_cstr(user_page, b"/tmp/syscall-create-link-test/nonempty\0");
        expect_errno(
            SyscallArgs::new([user_page, 0, 0, 0, 0, 0]).call::<Rmdir>(),
            SyscallError::DirectoryNotEmpty,
        );

        write_user_cstr(user_page, b"/tmp/syscall-create-link-test/dir\0");
        expect_errno(
            SyscallArgs::new([user_page, 0, 0, 0, 0, 0]).call::<Unlink>(),
            SyscallError::IsADirectory,
        );
        expect_ok(
            SyscallArgs::new([AT_FDCWD, user_page, AT_REMOVEDIR, 0, 0, 0]).call::<UnlinkAt>(),
            0,
        );

        write_user_cstr(user_page, b"/tmp/syscall-create-link-test/src\0");
        write_user_cstr(user_page + 128, b"/tmp/syscall-create-link-test/hard\0");
        expect_ok(
            SyscallArgs::new([user_page, user_page + 128, 0, 0, 0, 0]).call::<Link>(),
            0,
        );
        expect_errno(
            SyscallArgs::new([user_page, user_page + 128, 0, 0, 0, 0]).call::<Link>(),
            SyscallError::FileAlreadyExists,
        );
        expect_ok(
            SyscallArgs::new([user_page + 128, 0, 0, 0, 0, 0]).call::<Unlink>(),
            0,
        );

        write_user_cstr(user_page, b"/tmp/syscall-create-link-test\0");
        let dir_fd = expect_fd(
            SyscallArgs::new([
                AT_FDCWD,
                user_page,
                OpenFlags::DIRECTORY.bits() as u64,
                0,
                0,
                0,
            ])
            .call::<OpenAt>(),
        );
        write_user_cstr(user_page + 128, b"atdir\0");
        expect_ok(
            SyscallArgs::new([dir_fd as u64, user_page + 128, 0o700, 0, 0, 0]).call::<MkdirAt>(),
            0,
        );
        expect_ok(
            SyscallArgs::new([dir_fd as u64, user_page + 128, AT_REMOVEDIR, 0, 0, 0])
                .call::<UnlinkAt>(),
            0,
        );

        write_user_cstr(user_page, b"/tmp/syscall-create-link-test/src\0");
        let src_fd = expect_fd(
            SyscallArgs::new([
                AT_FDCWD,
                user_page,
                OpenFlags::empty().bits() as u64,
                0,
                0,
                0,
            ])
            .call::<OpenAt>(),
        );
        write_user_cstr(user_page, b"\0");
        write_user_cstr(user_page + 128, b"fdhard\0");
        expect_ok(
            SyscallArgs::new([
                src_fd as u64,
                user_page,
                dir_fd as u64,
                user_page + 128,
                AT_EMPTY_PATH,
                0,
            ])
            .call::<LinkAt>(),
            0,
        );
        expect_ok(
            SyscallArgs::new([dir_fd as u64, user_page + 128, 0, 0, 0, 0]).call::<UnlinkAt>(),
            0,
        );
        close_test_fd(src_fd);

        write_user_cstr(user_page, b"/target/without/nul\0");
        write_user_cstr(user_page + 128, b"/tmp/syscall-create-link-test/link\0");
        expect_ok(
            SyscallArgs::new([user_page, user_page + 128, 0, 0, 0, 0]).call::<Symlink>(),
            0,
        );
        expect_errno(
            SyscallArgs::new([user_page, user_page + 128, 0, 0, 0, 0]).call::<Symlink>(),
            SyscallError::FileAlreadyExists,
        );
        expect_ok(
            SyscallArgs::new([user_page + 128, user_page + 256, 7, 0, 0, 0]).call::<Readlink>(),
            7,
        );
        assert_user_bytes(user_page + 256, b"/target");

        write_user_cstr(user_page, b"relative-target\0");
        write_user_cstr(user_page + 128, b"atlink\0");
        expect_ok(
            SyscallArgs::new([user_page, dir_fd as u64, user_page + 128, 0, 0, 0])
                .call::<SymlinkAt>(),
            0,
        );
        expect_ok(
            SyscallArgs::new([dir_fd as u64, user_page + 128, user_page + 256, 64, 0, 0])
                .call::<ReadlinkAt>(),
            b"relative-target".len(),
        );
        assert_user_bytes(user_page + 256, b"relative-target");
        expect_errno(
            SyscallArgs::new([dir_fd as u64, user_page + 128, 0x8000_0000, 0, 0, 0])
                .call::<UnlinkAt>(),
            SyscallError::InvalidArguments,
        );
        expect_ok(
            SyscallArgs::new([dir_fd as u64, user_page + 128, 0, 0, 0, 0]).call::<UnlinkAt>(),
            0,
        );

        close_test_fd(dir_fd);
        for path in cleanup_paths {
            let _ = VirtualFS.lock().delete_file(Path::new(path));
        }
    }

    fn opened_file_object_stat_keeps_mount_device_id_without_reborrowing_vfs() {
        let base_path = Path::new("/tmp/opened-file-object-stat-test");
        let file_path = Path::new("/tmp/opened-file-object-stat-test/file");
        let _ = VirtualFS.lock().delete_file(file_path.clone());
        let _ = VirtualFS.lock().delete_file(base_path.clone());
        VirtualFS.lock().create_dir(base_path.clone()).unwrap();
        VirtualFS.lock().create_file(file_path.clone()).unwrap();

        let opened = {
            let mut vfs = VirtualFS.lock();
            vfs.open(file_path.clone()).unwrap()
        };

        let stat = opened.stat();
        assert_eq!(stat.st_dev, mount_device_id_for_path(&file_path));
        assert_eq!(stat.st_mode & 0o170000, 0o100000);

        let _ = VirtualFS.lock().delete_file(file_path);
        let _ = VirtualFS.lock().delete_file(base_path);
    }

    fn filesystem_fd_state_syscalls_follow_linux_rules() {
        const AT_FDCWD: u64 = (-100i32) as u64;
        const O_RDONLY: u64 = 0;
        const O_WRONLY: u64 = 1;
        const O_CREAT: u64 = 0x40;
        const O_EXCL: u64 = 0x80;
        const O_TRUNC: u64 = 0x200;
        const O_DIRECTORY: u64 = 0o200000;
        const SEEK_SET: u64 = 0;
        const SEEK_CUR: u64 = 1;
        const SEEK_END: u64 = 2;

        let base_path = Path::new("/tmp/syscall-fd-state-test");
        let cleanup_paths = [
            "/tmp/syscall-fd-state-test/file",
            "/tmp/syscall-fd-state-test/dir",
            "/tmp/syscall-fd-state-test",
        ];
        for path in cleanup_paths {
            let _ = VirtualFS.lock().delete_file(Path::new(path));
        }
        VirtualFS.lock().create_dir(base_path.clone()).unwrap();
        VirtualFS
            .lock()
            .create_dir(Path::new("/tmp/syscall-fd-state-test/dir"))
            .unwrap();

        let user_page = allocate_user_test_page();
        let stat_ptr = (user_page + 512) as *mut LinuxStat;

        write_user_cstr(user_page, b"/tmp/syscall-fd-state-test/file\0");
        let create_fd = expect_fd(
            SyscallArgs::new([
                AT_FDCWD,
                user_page,
                O_WRONLY | O_CREAT | O_EXCL,
                0o640,
                0,
                0,
            ])
            .call::<OpenAt>(),
        );
        let created_object = get_object_current_process(create_fd as u64).unwrap();
        let created_stat = created_object.as_statable().unwrap().stat();
        assert_eq!(created_stat.st_mode & 0o170000, 0o100000);
        assert_eq!(created_stat.st_mode & 0o777, 0o640);
        expect_errno(
            SyscallArgs::new([
                AT_FDCWD,
                user_page,
                O_WRONLY | O_CREAT | O_EXCL,
                0o640,
                0,
                0,
            ])
            .call::<OpenAt>(),
            SyscallError::FileAlreadyExists,
        );
        expect_ok(
            SyscallArgs::new([create_fd as u64, 0, 0, 0, 0, 0]).call::<Close>(),
            0,
        );
        expect_errno(
            SyscallArgs::new([create_fd as u64, 0, 0, 0, 0, 0]).call::<Close>(),
            SyscallError::BadFileDescriptor,
        );

        let reopen_fd =
            expect_fd(SyscallArgs::new([user_page, O_RDONLY, 0, 0, 0, 0]).call::<Open>());
        expect_ok(
            SyscallArgs::new([reopen_fd as u64, stat_ptr as u64, 0, 0, 0, 0]).call::<Fstat>(),
            0,
        );
        let linux_stat = read_user_value::<LinuxStat>(stat_ptr as u64);
        assert_eq!(linux_stat.st_mode & 0o170000, 0o100000);
        assert_eq!(linux_stat.st_mode & 0o777, 0o640);
        expect_errno(
            SyscallArgs::new([usize::MAX as u64, stat_ptr as u64, 0, 0, 0, 0]).call::<Fstat>(),
            SyscallError::BadFileDescriptor,
        );

        expect_ok(
            SyscallArgs::new([reopen_fd as u64, 0, SEEK_SET, 0, 0, 0]).call::<Lseek>(),
            0,
        );
        expect_ok(
            SyscallArgs::new([reopen_fd as u64, 0, SEEK_END, 0, 0, 0]).call::<Lseek>(),
            0,
        );
        expect_ok(
            SyscallArgs::new([reopen_fd as u64, 0, SEEK_CUR, 0, 0, 0]).call::<Lseek>(),
            0,
        );
        expect_errno(
            SyscallArgs::new([reopen_fd as u64, (-1i64) as u64, SEEK_SET, 0, 0, 0]).call::<Lseek>(),
            SyscallError::InvalidArguments,
        );
        expect_errno(
            SyscallArgs::new([reopen_fd as u64, (-1i64) as u64, SEEK_END, 0, 0, 0]).call::<Lseek>(),
            SyscallError::InvalidArguments,
        );
        expect_errno(
            SyscallArgs::new([reopen_fd as u64, 0, 99, 0, 0, 0]).call::<Lseek>(),
            SyscallError::InvalidArguments,
        );
        expect_ok(
            SyscallArgs::new([reopen_fd as u64, 0, 0, 0, 0, 0]).call::<Close>(),
            0,
        );

        write_user_cstr(user_page, b"/tmp/syscall-fd-state-test/dir\0");
        expect_errno(
            SyscallArgs::new([AT_FDCWD, user_page, O_CREAT | O_DIRECTORY, 0o755, 0, 0])
                .call::<OpenAt>(),
            SyscallError::InvalidArguments,
        );
        expect_errno(
            SyscallArgs::new([AT_FDCWD, user_page, O_WRONLY | O_TRUNC, 0, 0, 0]).call::<OpenAt>(),
            SyscallError::IsADirectory,
        );
        let dir_fd = expect_fd(
            SyscallArgs::new([AT_FDCWD, user_page, O_DIRECTORY, 0, 0, 0]).call::<OpenAt>(),
        );
        expect_ok(
            SyscallArgs::new([dir_fd as u64, 0, 0, 0, 0, 0]).call::<Close>(),
            0,
        );
        write_user_cstr(user_page, b"/tmp/syscall-fd-state-test/file\0");
        expect_errno(
            SyscallArgs::new([AT_FDCWD, user_page, O_DIRECTORY, 0, 0, 0]).call::<OpenAt>(),
            SyscallError::NotADirectory,
        );

        for path in cleanup_paths {
            let _ = VirtualFS.lock().delete_file(Path::new(path));
        }
    }

    fn filesystem_metadata_syscalls_follow_linux_rules() {
        const AT_FDCWD: u64 = (-100i32) as u64;
        const AT_SYMLINK_NOFOLLOW: u64 = 0x100;
        const AT_EMPTY_PATH: u64 = 0x1000;
        const AT_NO_AUTOMOUNT: u64 = 0x800;

        let base_path = Path::new("/tmp/syscall-metadata-test");
        let cleanup_paths = [
            "/tmp/syscall-metadata-test/link",
            "/tmp/syscall-metadata-test/file",
            "/tmp/syscall-metadata-test",
        ];
        for path in cleanup_paths {
            let _ = VirtualFS.lock().delete_file(Path::new(path));
        }
        VirtualFS.lock().create_dir(base_path.clone()).unwrap();
        VirtualFS
            .lock()
            .create_file(Path::new("/tmp/syscall-metadata-test/file"))
            .unwrap();
        VirtualFS
            .lock()
            .create_symlink(
                Path::new("/tmp/syscall-metadata-test/link"),
                "/tmp/syscall-metadata-test/file",
            )
            .unwrap();

        let user_page = allocate_user_test_page();
        let stat_ptr = (user_page + 512) as *mut LinuxStat;

        write_user_cstr(user_page, b"/tmp/syscall-metadata-test/file\0");
        expect_ok(
            SyscallArgs::new([user_page, 0o640, 0, 0, 0, 0])
                .call::<crate::systemcall::implementations::Chmod>(),
            0,
        );
        let file_object = {
            let mut vfs = VirtualFS.lock();
            vfs.open(Path::new("/tmp/syscall-metadata-test/file"))
                .unwrap()
        };
        assert_eq!(file_object.stat().st_mode & 0o777, 0o640);

        let file_fd = expect_fd(
            SyscallArgs::new([
                AT_FDCWD,
                user_page,
                OpenFlags::empty().bits() as u64,
                0,
                0,
                0,
            ])
            .call::<OpenAt>(),
        );
        expect_ok(
            SyscallArgs::new([file_fd as u64, 0o600, 0, 0, 0, 0]).call::<Fchmod>(),
            0,
        );
        let file_stat_after_fchmod = get_object_current_process(file_fd as u64)
            .unwrap()
            .as_statable()
            .unwrap()
            .stat();
        assert_eq!(file_stat_after_fchmod.st_mode & 0o777, 0o600);

        write_user_cstr(user_page + 128, b"\0");
        expect_ok(
            SyscallArgs::new([file_fd as u64, user_page + 128, 0o644, AT_EMPTY_PATH, 0, 0])
                .call::<crate::systemcall::implementations::Fchmodat2>(),
            0,
        );
        let file_stat_after_empty_path = get_object_current_process(file_fd as u64)
            .unwrap()
            .as_statable()
            .unwrap()
            .stat();
        assert_eq!(file_stat_after_empty_path.st_mode & 0o777, 0o644);

        expect_errno(
            SyscallArgs::new([file_fd as u64, 0, 0o644, 0, 0, 0])
                .call::<crate::systemcall::implementations::Fchmodat2>(),
            SyscallError::BadAddress,
        );
        expect_errno(
            SyscallArgs::new([file_fd as u64, user_page + 128, 0o644, 0x4000_0000, 0, 0])
                .call::<crate::systemcall::implementations::Fchmodat2>(),
            SyscallError::InvalidArguments,
        );

        write_user_cstr(user_page, b"/tmp/syscall-metadata-test/link\0");
        expect_errno(
            SyscallArgs::new([AT_FDCWD, user_page, 0o700, AT_SYMLINK_NOFOLLOW, 0, 0])
                .call::<crate::systemcall::implementations::Fchmodat2>(),
            SyscallError::OperationNotSupported,
        );
        let target_object_after_link_nofollow = {
            let mut vfs = VirtualFS.lock();
            vfs.open(Path::new("/tmp/syscall-metadata-test/file"))
                .unwrap()
        };
        let target_stat_after_link_nofollow = target_object_after_link_nofollow.stat();
        assert_eq!(target_stat_after_link_nofollow.st_mode & 0o777, 0o644);

        expect_ok(
            SyscallArgs::new([AT_FDCWD, user_page, 0o700, 0, 0, 0]).call::<Fchmodat>(),
            0,
        );
        let target_object_after_follow = {
            let mut vfs = VirtualFS.lock();
            vfs.open(Path::new("/tmp/syscall-metadata-test/file"))
                .unwrap()
        };
        let target_stat_after_follow = target_object_after_follow.stat();
        assert_eq!(target_stat_after_follow.st_mode & 0o777, 0o700);

        expect_ok(
            SyscallArgs::new([
                AT_FDCWD,
                user_page,
                stat_ptr as u64,
                AT_SYMLINK_NOFOLLOW,
                0,
                0,
            ])
            .call::<Newfstatat>(),
            0,
        );
        let symlink_stat = read_user_value::<LinuxStat>(stat_ptr as u64);
        assert_eq!(symlink_stat.st_mode & 0o170000, 0o120000);

        expect_ok(
            SyscallArgs::new([AT_FDCWD, user_page, stat_ptr as u64, 0, 0, 0]).call::<Newfstatat>(),
            0,
        );
        let followed_stat = read_user_value::<LinuxStat>(stat_ptr as u64);
        assert_eq!(followed_stat.st_mode & 0o170000, 0o100000);
        assert_eq!(followed_stat.st_mode & 0o777, 0o700);

        expect_ok(
            SyscallArgs::new([user_page, stat_ptr as u64, 0, 0, 0, 0]).call::<Lstat>(),
            0,
        );
        let lstat_link_stat = read_user_value::<LinuxStat>(stat_ptr as u64);
        assert_eq!(lstat_link_stat.st_mode & 0o170000, 0o120000);
        assert_eq!(lstat_link_stat.st_ino, symlink_stat.st_ino);

        write_user_cstr(user_page, b"/tmp/syscall-metadata-test/file\0");
        expect_ok(
            SyscallArgs::new([user_page, stat_ptr as u64, 0, 0, 0, 0]).call::<Stat>(),
            0,
        );
        let stat_stat = read_user_value::<LinuxStat>(stat_ptr as u64);
        assert_eq!(stat_stat.st_mode & 0o170000, 0o100000);
        assert_eq!(stat_stat.st_mode & 0o777, 0o700);
        assert_eq!(stat_stat.st_ino, followed_stat.st_ino);
        expect_errno(
            SyscallArgs::new([0, stat_ptr as u64, 0, 0, 0, 0]).call::<Stat>(),
            SyscallError::BadAddress,
        );
        expect_errno(
            SyscallArgs::new([0, stat_ptr as u64, 0, 0, 0, 0]).call::<Lstat>(),
            SyscallError::BadAddress,
        );
        write_user_cstr(user_page + 256, b"\0");
        expect_errno(
            SyscallArgs::new([user_page + 256, stat_ptr as u64, 0, 0, 0, 0]).call::<Stat>(),
            SyscallError::FileNotFound,
        );
        expect_errno(
            SyscallArgs::new([user_page + 256, stat_ptr as u64, 0, 0, 0, 0]).call::<Lstat>(),
            SyscallError::FileNotFound,
        );
        write_user_cstr(user_page + 320, b"/tmp/syscall-metadata-test/missing\0");
        expect_errno(
            SyscallArgs::new([user_page + 320, stat_ptr as u64, 0, 0, 0, 0]).call::<Stat>(),
            SyscallError::FileNotFound,
        );
        expect_errno(
            SyscallArgs::new([user_page + 320, stat_ptr as u64, 0, 0, 0, 0]).call::<Lstat>(),
            SyscallError::FileNotFound,
        );

        expect_ok(
            SyscallArgs::new([
                file_fd as u64,
                user_page + 128,
                stat_ptr as u64,
                AT_EMPTY_PATH | AT_NO_AUTOMOUNT,
                0,
                0,
            ])
            .call::<Newfstatat>(),
            0,
        );
        let empty_path_stat = read_user_value::<LinuxStat>(stat_ptr as u64);
        assert_eq!(empty_path_stat.st_mode & 0o170000, 0o100000);
        assert_eq!(empty_path_stat.st_mode & 0o777, 0o700);

        expect_errno(
            SyscallArgs::new([file_fd as u64, 0, stat_ptr as u64, 0, 0, 0]).call::<Newfstatat>(),
            SyscallError::BadAddress,
        );
        expect_errno(
            SyscallArgs::new([
                file_fd as u64,
                user_page + 128,
                stat_ptr as u64,
                0x4000_0000,
                0,
                0,
            ])
            .call::<Newfstatat>(),
            SyscallError::NoSyscall,
        );

        close_test_fd(file_fd);
        for path in cleanup_paths {
            let _ = VirtualFS.lock().delete_file(Path::new(path));
        }
    }

    fn filesystem_io_syscalls_follow_linux_rules() {
        const AT_FDCWD: u64 = (-100i32) as u64;

        let base_path = Path::new("/tmp/syscall-io-test");
        let cleanup_paths = ["/tmp/syscall-io-test/file", "/tmp/syscall-io-test"];
        for path in cleanup_paths {
            let _ = VirtualFS.lock().delete_file(Path::new(path));
        }
        VirtualFS.lock().create_dir(base_path.clone()).unwrap();
        VirtualFS
            .lock()
            .create_file(Path::new("/tmp/syscall-io-test/file"))
            .unwrap();

        let user_page = allocate_user_test_page();
        write_user_cstr(user_page, b"/tmp/syscall-io-test/file\0");
        let fd = expect_fd(
            SyscallArgs::new([
                AT_FDCWD,
                user_page,
                OpenFlags::empty().bits() as u64,
                0,
                0,
                0,
            ])
            .call::<OpenAt>(),
        );

        get_object_current_process(fd as u64)
            .unwrap()
            .as_file_like()
            .unwrap()
            .truncate(0)
            .unwrap();

        get_current_process()
            .lock()
            .addrspace
            .write_buffer(user_page as *mut u8, b"abcdef")
            .unwrap();
        expect_ok(
            SyscallArgs::new([fd as u64, user_page, 6, 0, 0, 0]).call::<Write>(),
            6,
        );

        expect_ok(
            SyscallArgs::new([fd as u64, 0, 0, 0, 0, 0]).call::<Lseek>(),
            0,
        );
        get_current_process()
            .lock()
            .addrspace
            .write_buffer((user_page + 128) as *mut u8, &[0; 6])
            .unwrap();
        expect_ok(
            SyscallArgs::new([fd as u64, user_page + 128, 6, 0, 0, 0]).call::<Read>(),
            6,
        );
        assert_user_bytes(user_page + 128, b"abcdef");

        #[repr(C)]
        #[derive(Clone, Copy)]
        struct TestLinuxIovec {
            iov_base: *const u8,
            iov_len: usize,
        }

        expect_ok(
            SyscallArgs::new([fd as u64, 0, 0, 0, 0, 0]).call::<Lseek>(),
            0,
        );
        get_current_process()
            .lock()
            .addrspace
            .write_buffer((user_page + 768) as *mut u8, &[0; 6])
            .unwrap();
        write_user_value(
            user_page + 896,
            &[
                TestLinuxIovec {
                    iov_base: (user_page + 768) as *const u8,
                    iov_len: 2,
                },
                TestLinuxIovec {
                    iov_base: (user_page + 770) as *const u8,
                    iov_len: 4,
                },
            ],
        );
        expect_ok(
            SyscallArgs::new([fd as u64, user_page + 896, 2, 0, 0, 0]).call::<Readv>(),
            6,
        );
        assert_user_bytes(user_page + 768, b"abcdef");
        expect_ok(
            SyscallArgs::new([fd as u64, 0, 0, 0, 0, 0]).call::<Lseek>(),
            0,
        );
        write_user_value(
            user_page + 960,
            &[
                TestLinuxIovec {
                    iov_base: (user_page + 832) as *const u8,
                    iov_len: 0,
                },
                TestLinuxIovec {
                    iov_base: core::ptr::null(),
                    iov_len: 1,
                },
            ],
        );
        expect_errno(
            SyscallArgs::new([fd as u64, user_page + 960, 2, 0, 0, 0]).call::<Readv>(),
            SyscallError::BadAddress,
        );
        expect_errno(
            SyscallArgs::new([fd as u64, 0, 1, 0, 0, 0]).call::<Readv>(),
            SyscallError::BadAddress,
        );
        expect_errno(
            SyscallArgs::new([fd as u64, user_page + 896, u64::MAX, 0, 0, 0]).call::<Readv>(),
            SyscallError::InvalidArguments,
        );

        get_current_process()
            .lock()
            .addrspace
            .write_buffer((user_page + 256) as *mut u8, b"ZZ")
            .unwrap();
        expect_ok(
            SyscallArgs::new([fd as u64, user_page + 256, 2, 2, 0, 0]).call::<Pwrite64>(),
            2,
        );
        expect_ok(
            SyscallArgs::new([fd as u64, 0, 0, 0, 0, 0]).call::<Lseek>(),
            0,
        );
        get_current_process()
            .lock()
            .addrspace
            .write_buffer((user_page + 384) as *mut u8, &[0; 6])
            .unwrap();
        expect_ok(
            SyscallArgs::new([fd as u64, user_page + 384, 6, 0, 0, 0]).call::<Read>(),
            6,
        );
        assert_user_bytes(user_page + 384, b"abZZef");

        get_current_process()
            .lock()
            .addrspace
            .write_buffer((user_page + 512) as *mut u8, &[0; 3])
            .unwrap();
        expect_ok(
            SyscallArgs::new([fd as u64, user_page + 512, 3, 1, 0, 0]).call::<Pread64>(),
            3,
        );
        assert_user_bytes(user_page + 512, b"bZZ");

        let current_offset = get_object_current_process(fd as u64)
            .unwrap()
            .as_seekable()
            .unwrap()
            .seek(0, crate::filesystem::vfs_traits::Whence::Current)
            .unwrap();
        assert_eq!(current_offset, 6);

        expect_errno(
            SyscallArgs::new([fd as u64, user_page + 640, 1, (-1i64) as u64, 0, 0])
                .call::<Pread64>(),
            SyscallError::InvalidArguments,
        );
        expect_errno(
            SyscallArgs::new([fd as u64, user_page + 640, 1, (-1i64) as u64, 0, 0])
                .call::<Pwrite64>(),
            SyscallError::InvalidArguments,
        );
        expect_ok(
            SyscallArgs::new([fd as u64, 0, 0, 0, 0, 0]).call::<Lseek>(),
            0,
        );
        expect_errno(
            SyscallArgs::new([fd as u64, 0, 1, 0, 0, 0]).call::<Read>(),
            SyscallError::BadAddress,
        );
        expect_errno(
            SyscallArgs::new([fd as u64, 0, 1, 0, 0, 0]).call::<Write>(),
            SyscallError::BadAddress,
        );

        close_test_fd(fd);
        for path in cleanup_paths {
            let _ = VirtualFS.lock().delete_file(Path::new(path));
        }
    }

    fn filesystem_rename_syscalls_follow_linux_rules() {
        const AT_FDCWD: u64 = (-100i32) as u64;

        let base_path = Path::new("/tmp/syscall-rename-test");
        let cleanup_paths = [
            "/tmp/syscall-rename-test/dst",
            "/tmp/syscall-rename-test/src",
            "/tmp/syscall-rename-test/subdir/child",
            "/tmp/syscall-rename-test/subdir/renamed",
            "/tmp/syscall-rename-test/subdir",
            "/tmp/syscall-rename-test",
        ];
        for path in cleanup_paths {
            let _ = VirtualFS.lock().delete_file(Path::new(path));
        }
        VirtualFS.lock().create_dir(base_path.clone()).unwrap();
        VirtualFS
            .lock()
            .create_file(Path::new("/tmp/syscall-rename-test/src"))
            .unwrap();
        VirtualFS
            .lock()
            .create_dir(Path::new("/tmp/syscall-rename-test/subdir"))
            .unwrap();
        VirtualFS
            .lock()
            .create_file(Path::new("/tmp/syscall-rename-test/subdir/child"))
            .unwrap();

        let user_page = allocate_user_test_page();
        write_user_cstr(user_page, b"/tmp/syscall-rename-test/src\0");
        write_user_cstr(user_page + 128, b"/tmp/syscall-rename-test/dst\0");
        expect_ok(
            SyscallArgs::new([user_page, user_page + 128, 0, 0, 0, 0]).call::<Rename>(),
            0,
        );
        {
            let mut vfs = VirtualFS.lock();
            assert!(vfs.open(Path::new("/tmp/syscall-rename-test/dst")).is_ok());
            assert!(matches!(
                vfs.open(Path::new("/tmp/syscall-rename-test/src")),
                Err(crate::filesystem::errors::FSError::NotFound)
            ));
        }

        VirtualFS
            .lock()
            .create_file(Path::new("/tmp/syscall-rename-test/src"))
            .unwrap();
        write_user_cstr(user_page, b"/tmp/syscall-rename-test/src\0");
        write_user_cstr(user_page + 128, b"/tmp/syscall-rename-test/dst\0");
        expect_ok(
            SyscallArgs::new([user_page, user_page + 128, 0, 0, 0, 0]).call::<Rename>(),
            0,
        );
        {
            let mut vfs = VirtualFS.lock();
            assert!(vfs.open(Path::new("/tmp/syscall-rename-test/dst")).is_ok());
            assert!(matches!(
                vfs.open(Path::new("/tmp/syscall-rename-test/src")),
                Err(crate::filesystem::errors::FSError::NotFound)
            ));
        }

        expect_ok(
            SyscallArgs::new([user_page + 128, user_page + 128, 0, 0, 0, 0]).call::<Rename>(),
            0,
        );

        write_user_cstr(user_page, b"/tmp/syscall-rename-test/missing\0");
        expect_errno(
            SyscallArgs::new([user_page, user_page + 128, 0, 0, 0, 0]).call::<Rename>(),
            SyscallError::FileNotFound,
        );

        write_user_cstr(user_page, b"/tmp/syscall-rename-test\0");
        let dir_fd = expect_fd(
            SyscallArgs::new([
                AT_FDCWD,
                user_page,
                OpenFlags::DIRECTORY.bits() as u64,
                0,
                0,
                0,
            ])
            .call::<OpenAt>(),
        );
        write_user_cstr(user_page, b"subdir/child\0");
        write_user_cstr(user_page + 128, b"subdir/renamed\0");
        expect_ok(
            SyscallArgs::new([
                dir_fd as u64,
                user_page,
                dir_fd as u64,
                user_page + 128,
                0,
                0,
            ])
            .call::<RenameAt>(),
            0,
        );
        {
            let mut vfs = VirtualFS.lock();
            assert!(
                vfs.open(Path::new("/tmp/syscall-rename-test/subdir/renamed"))
                    .is_ok()
            );
            assert!(matches!(
                vfs.open(Path::new("/tmp/syscall-rename-test/subdir/child")),
                Err(crate::filesystem::errors::FSError::NotFound)
            ));
        }

        expect_errno(
            SyscallArgs::new([
                dir_fd as u64,
                user_page,
                dir_fd as u64,
                user_page + 128,
                1,
                0,
            ])
            .call::<RenameAt2>(),
            SyscallError::NoSyscall,
        );
        close_test_fd(dir_fd);

        for path in cleanup_paths {
            let _ = VirtualFS.lock().delete_file(Path::new(path));
        }
    }

    fn filesystem_getdents_syscalls_follow_linux_rules() {
        const AT_FDCWD: u64 = (-100i32) as u64;
        const DT_DIR: u8 = 4;
        const DT_REG: u8 = 8;
        const DT_LNK: u8 = 10;

        let base_path = Path::new("/tmp/syscall-getdents-test");
        let cleanup_paths = [
            "/tmp/syscall-getdents-test/file",
            "/tmp/syscall-getdents-test/link",
            "/tmp/syscall-getdents-test/subdir",
            "/tmp/syscall-getdents-test",
        ];
        for path in cleanup_paths {
            let _ = VirtualFS.lock().delete_file(Path::new(path));
        }
        VirtualFS.lock().create_dir(base_path.clone()).unwrap();
        VirtualFS
            .lock()
            .create_file(Path::new("/tmp/syscall-getdents-test/file"))
            .unwrap();
        VirtualFS
            .lock()
            .create_dir(Path::new("/tmp/syscall-getdents-test/subdir"))
            .unwrap();
        VirtualFS
            .lock()
            .create_symlink(Path::new("/tmp/syscall-getdents-test/link"), "file")
            .unwrap();

        let user_page = allocate_user_test_page();
        write_user_cstr(user_page, b"/tmp/syscall-getdents-test\0");
        let dir_fd = expect_fd(
            SyscallArgs::new([
                AT_FDCWD,
                user_page,
                OpenFlags::DIRECTORY.bits() as u64,
                0,
                0,
                0,
            ])
            .call::<OpenAt>(),
        );

        expect_errno(
            SyscallArgs::new([dir_fd as u64, 0, 256, 0, 0, 0])
                .call::<crate::systemcall::implementations::Getdents64>(),
            SyscallError::BadAddress,
        );
        expect_errno(
            SyscallArgs::new([dir_fd as u64, user_page + 128, 8, 0, 0, 0])
                .call::<crate::systemcall::implementations::Getdents64>(),
            SyscallError::InvalidArguments,
        );

        let bytes_result = SyscallArgs::new([dir_fd as u64, user_page + 128, 512, 0, 0, 0])
            .call::<crate::systemcall::implementations::Getdents64>();
        let bytes = bytes_result.expect("getdents64 should return byte count");
        assert!(bytes > 0);

        let mut offset = 0usize;
        let mut saw_file = false;
        let mut saw_dir = false;
        let mut saw_link = false;
        while offset < bytes {
            let entry =
                read_user_value::<LinuxDirent64Header>((user_page + 128 + offset as u64) as u64);
            assert!(entry.d_reclen as usize >= 24);
            assert!(entry.d_off >= 1);
            let name_len = entry.d_reclen as usize - 19;
            let raw_name = get_current_process()
                .lock()
                .addrspace
                .read_buffer(
                    (user_page + 128 + offset as u64 + 19) as *const u8,
                    name_len,
                )
                .unwrap();
            let nul = raw_name.iter().position(|byte| *byte == 0).unwrap();
            let name = core::str::from_utf8(&raw_name[..nul]).unwrap();
            match (name, entry.d_type) {
                ("file", DT_REG) => saw_file = true,
                ("subdir", DT_DIR) => saw_dir = true,
                ("link", DT_LNK) => saw_link = true,
                _ => {}
            }
            offset += entry.d_reclen as usize;
        }
        assert!(saw_file);
        assert!(saw_dir);
        assert!(saw_link);

        expect_ok(
            SyscallArgs::new([dir_fd as u64, user_page + 128, 512, 0, 0, 0])
                .call::<crate::systemcall::implementations::Getdents>(),
            0,
        );
        expect_ok(
            SyscallArgs::new([dir_fd as u64, user_page + 128, 8, 0, 0, 0])
                .call::<crate::systemcall::implementations::Getdents64>(),
            0,
        );

        close_test_fd(dir_fd);
        expect_errno(
            SyscallArgs::new([dir_fd as u64, user_page + 128, 512, 0, 0, 0])
                .call::<crate::systemcall::implementations::Getdents64>(),
            SyscallError::BadFileDescriptor,
        );

        for path in cleanup_paths {
            let _ = VirtualFS.lock().delete_file(Path::new(path));
        }
    }

    fn filesystem_file_object_syscalls_follow_linux_rules() {
        const AT_FDCWD: u64 = (-100i32) as u64;
        const O_APPEND: u64 = 0o2_000;
        const SEEK_CUR: u64 = 1;
        const LOCK_SH: u64 = 1;
        const LOCK_EX: u64 = 2;
        const LOCK_NB: u64 = 4;
        const LOCK_UN: u64 = 8;
        const F_GETFD: u64 = 1;
        const F_SETFD: u64 = 2;
        const F_GETFL: u64 = 3;
        const F_SETFL: u64 = 4;
        const FD_CLOEXEC: u64 = 1;
        const POSIX_FADV_RANDOM: u64 = 1;
        const FALLOC_FL_KEEP_SIZE: u64 = 0x01;
        const FALLOC_FL_PUNCH_HOLE: u64 = 0x02;

        let base_path = Path::new("/tmp/syscall-file-object-test");
        let cleanup_paths = [
            "/tmp/syscall-file-object-test/file",
            "/tmp/syscall-file-object-test/out",
            "/tmp/syscall-file-object-test",
        ];
        for path in cleanup_paths {
            let _ = VirtualFS.lock().delete_file(Path::new(path));
        }
        VirtualFS.lock().create_dir(base_path.clone()).unwrap();
        VirtualFS
            .lock()
            .create_file(Path::new("/tmp/syscall-file-object-test/file"))
            .unwrap();
        VirtualFS
            .lock()
            .create_file(Path::new("/tmp/syscall-file-object-test/out"))
            .unwrap();

        let user_page = allocate_user_test_page();
        write_user_cstr(user_page, b"/tmp/syscall-file-object-test/file\0");
        let fd = expect_fd(
            SyscallArgs::new([
                AT_FDCWD,
                user_page,
                OpenFlags::empty().bits() as u64,
                0,
                0,
                0,
            ])
            .call::<OpenAt>(),
        );
        write_user_cstr(user_page + 128, b"/tmp/syscall-file-object-test/file\0");
        let out_fd = expect_fd(
            SyscallArgs::new([
                AT_FDCWD,
                user_page + 128,
                OpenFlags::empty().bits() as u64,
                0,
                0,
                0,
            ])
            .call::<OpenAt>(),
        );
        write_user_cstr(user_page + 192, b"/tmp/syscall-file-object-test/out\0");
        let copy_out_fd = expect_fd(
            SyscallArgs::new([
                AT_FDCWD,
                user_page + 192,
                OpenFlags::empty().bits() as u64,
                0,
                0,
                0,
            ])
            .call::<OpenAt>(),
        );

        get_object_current_process(fd as u64)
            .unwrap()
            .as_file_like()
            .unwrap()
            .truncate(0)
            .unwrap();
        get_object_current_process(copy_out_fd as u64)
            .unwrap()
            .as_file_like()
            .unwrap()
            .truncate(0)
            .unwrap();

        #[repr(C)]
        #[derive(Clone, Copy)]
        struct TestLinuxIovec {
            iov_base: *const u8,
            iov_len: usize,
        }

        let chunk_a = user_page + 256;
        let chunk_b = user_page + 320;
        get_current_process()
            .lock()
            .addrspace
            .write_buffer(chunk_a as *mut u8, b"ab")
            .unwrap();
        get_current_process()
            .lock()
            .addrspace
            .write_buffer(chunk_b as *mut u8, b"cdef")
            .unwrap();
        write_user_value(
            user_page + 384,
            &[
                TestLinuxIovec {
                    iov_base: chunk_a as *const u8,
                    iov_len: 2,
                },
                TestLinuxIovec {
                    iov_base: chunk_b as *const u8,
                    iov_len: 4,
                },
            ],
        );
        expect_ok(
            SyscallArgs::new([fd as u64, user_page + 384, 2, 0, 0, 0]).call::<Writev>(),
            6,
        );
        expect_ok(
            SyscallArgs::new([fd as u64, 0, 0, 0, 0, 0]).call::<Lseek>(),
            0,
        );
        get_current_process()
            .lock()
            .addrspace
            .write_buffer((user_page + 512) as *mut u8, &[0; 6])
            .unwrap();
        expect_ok(
            SyscallArgs::new([fd as u64, user_page + 512, 6, 0, 0, 0]).call::<Read>(),
            6,
        );
        assert_user_bytes(user_page + 512, b"abcdef");

        expect_errno(
            SyscallArgs::new([fd as u64, 0, 1, 0, 0, 0]).call::<Writev>(),
            SyscallError::BadAddress,
        );
        expect_errno(
            SyscallArgs::new([fd as u64, user_page + 384, u64::MAX, 0, 0, 0]).call::<Writev>(),
            SyscallError::InvalidArguments,
        );
        write_user_value(
            user_page + 448,
            &[TestLinuxIovec {
                iov_base: core::ptr::null(),
                iov_len: 1,
            }],
        );
        expect_errno(
            SyscallArgs::new([fd as u64, user_page + 448, 1, 0, 0, 0]).call::<Writev>(),
            SyscallError::BadAddress,
        );

        expect_ok(
            SyscallArgs::new([fd as u64, F_GETFD, 0, 0, 0, 0]).call::<Fcntl>(),
            0,
        );
        expect_ok(
            SyscallArgs::new([fd as u64, F_SETFD, FD_CLOEXEC, 0, 0, 0]).call::<Fcntl>(),
            0,
        );
        assert_fd_flags(fd, FdFlags::CLOEXEC);
        expect_ok(
            SyscallArgs::new([fd as u64, F_GETFD, 0, 0, 0, 0]).call::<Fcntl>(),
            1,
        );
        expect_ok(
            SyscallArgs::new([fd as u64, F_SETFL, O_APPEND, 0, 0, 0]).call::<Fcntl>(),
            0,
        );
        assert_object_flags(fd, FileFlags::APPEND);
        assert_eq!(
            SyscallArgs::new([fd as u64, F_GETFL, 0, 0, 0, 0])
                .call::<Fcntl>()
                .unwrap() as u64
                & O_APPEND,
            O_APPEND
        );
        expect_errno(
            SyscallArgs::new([fd as u64, 9999, 0, 0, 0, 0]).call::<Fcntl>(),
            SyscallError::InvalidArguments,
        );

        expect_ok(
            SyscallArgs::new([fd as u64, LOCK_EX | LOCK_NB, 0, 0, 0, 0]).call::<Flock>(),
            0,
        );
        expect_errno(
            SyscallArgs::new([out_fd as u64, LOCK_EX | LOCK_NB, 0, 0, 0, 0]).call::<Flock>(),
            SyscallError::TryAgain,
        );
        expect_ok(
            SyscallArgs::new([fd as u64, LOCK_UN, 0, 0, 0, 0]).call::<Flock>(),
            0,
        );
        expect_ok(
            SyscallArgs::new([out_fd as u64, LOCK_SH | LOCK_NB, 0, 0, 0, 0]).call::<Flock>(),
            0,
        );
        expect_errno(
            SyscallArgs::new([fd as u64, LOCK_EX | LOCK_NB, 0, 0, 0, 0]).call::<Flock>(),
            SyscallError::TryAgain,
        );
        expect_ok(
            SyscallArgs::new([out_fd as u64, LOCK_UN, 0, 0, 0, 0]).call::<Flock>(),
            0,
        );
        expect_errno(
            SyscallArgs::new([fd as u64, 0, 0, 0, 0, 0]).call::<Flock>(),
            SyscallError::InvalidArguments,
        );

        expect_ok(
            SyscallArgs::new([fd as u64, 2, 0, 0, 0, 0]).call::<Ftruncate>(),
            0,
        );
        let truncated_stat = get_object_current_process(fd as u64)
            .unwrap()
            .as_statable()
            .unwrap()
            .stat();
        assert_eq!(truncated_stat.st_size, 2);
        expect_errno(
            SyscallArgs::new([fd as u64, (-1i64) as u64, 0, 0, 0, 0]).call::<Ftruncate>(),
            SyscallError::InvalidArguments,
        );

        expect_ok(
            SyscallArgs::new([fd as u64, 0, 0, POSIX_FADV_RANDOM, 0, 0]).call::<Fadvise64>(),
            0,
        );
        expect_errno(
            SyscallArgs::new([fd as u64, 0, 0, 6, 0, 0]).call::<Fadvise64>(),
            SyscallError::InvalidArguments,
        );

        expect_ok(
            SyscallArgs::new([fd as u64, 0, 1, 2, 0, 0]).call::<Fallocate>(),
            0,
        );
        expect_ok(
            SyscallArgs::new([
                fd as u64,
                FALLOC_FL_KEEP_SIZE | FALLOC_FL_PUNCH_HOLE,
                0,
                0,
                0,
                0,
            ])
            .call::<Fallocate>(),
            0,
        );
        expect_errno(
            SyscallArgs::new([fd as u64, 0x10, 0, 1, 0, 0]).call::<Fallocate>(),
            SyscallError::OperationNotSupported,
        );
        expect_errno(
            SyscallArgs::new([fd as u64, 0, (-1i64) as u64, 1, 0, 0]).call::<Fallocate>(),
            SyscallError::InvalidArguments,
        );

        expect_ok(
            SyscallArgs::new([fd as u64, 0, 0, 0, 0, 0]).call::<Fsync>(),
            0,
        );
        expect_ok(
            SyscallArgs::new([fd as u64, 0, 0, 0, 0, 0]).call::<Fdatasync>(),
            0,
        );

        write_user_value(user_page + 704, b"abcdef");
        get_object_current_process(fd as u64)
            .unwrap()
            .as_file_like()
            .unwrap()
            .truncate(0)
            .unwrap();
        get_object_current_process(out_fd as u64)
            .unwrap()
            .as_file_like()
            .unwrap()
            .truncate(0)
            .unwrap();
        expect_ok(
            SyscallArgs::new([fd as u64, user_page + 704, 6, 0, 0, 0]).call::<Write>(),
            6,
        );
        expect_ok(
            SyscallArgs::new([fd as u64, 0, 0, 0, 0, 0]).call::<Lseek>(),
            0,
        );
        expect_ok(
            SyscallArgs::new([copy_out_fd as u64, 0, 0, 0, 0, 0]).call::<Lseek>(),
            0,
        );
        assert_eq!(
            SyscallArgs::new([copy_out_fd as u64, fd as u64, 0, 3, 0, 0]).call::<Sendfile>(),
            Ok(3),
            "sendfile result",
        );
        expect_ok(
            SyscallArgs::new([copy_out_fd as u64, 0, 0, 0, 0, 0]).call::<Lseek>(),
            0,
        );
        get_current_process()
            .lock()
            .addrspace
            .write_buffer((user_page + 576) as *mut u8, &[0; 6])
            .unwrap();
        assert_eq!(
            SyscallArgs::new([copy_out_fd as u64, user_page + 576, 6, 0, 0, 0]).call::<Read>(),
            Ok(3),
            "sendfile readback",
        );
        assert_user_bytes(user_page + 576, b"abc");
        write_user_value(user_page + 608, &1i64);
        assert_eq!(
            SyscallArgs::new([copy_out_fd as u64, fd as u64, user_page + 608, 2, 0, 0])
                .call::<Sendfile>(),
            Ok(2),
            "sendfile offset result",
        );
        assert_eq!(read_user_value::<i64>(user_page + 608), 3);
        expect_ok(
            SyscallArgs::new([fd as u64, 0, SEEK_CUR, 0, 0, 0]).call::<Lseek>(),
            3,
        );

        get_object_current_process(fd as u64)
            .unwrap()
            .as_file_like()
            .unwrap()
            .truncate(0)
            .unwrap();
        get_object_current_process(copy_out_fd as u64)
            .unwrap()
            .as_file_like()
            .unwrap()
            .truncate(0)
            .unwrap();
        let pipe_page = user_page + 800;
        expect_ok(
            SyscallArgs::new([pipe_page, 0, 0, 0, 0, 0]).call::<Pipe>(),
            0,
        );
        let pipe_fds = read_user_value::<[i32; 2]>(pipe_page);
        let pipe_read_fd = pipe_fds[0] as usize;
        let pipe_write_fd = pipe_fds[1] as usize;
        expect_ok(
            SyscallArgs::new([fd as u64, user_page + 704, 6, 0, 0, 0]).call::<Write>(),
            6,
        );
        write_user_value(user_page + 616, &1i64);
        assert_eq!(
            SyscallArgs::new([fd as u64, user_page + 616, pipe_write_fd as u64, 0, 3, 0,])
                .call::<Splice>(),
            Ok(3),
            "splice result",
        );
        assert_eq!(read_user_value::<i64>(user_page + 616), 4);
        expect_ok(
            SyscallArgs::new([fd as u64, 0, SEEK_CUR, 0, 0, 0]).call::<Lseek>(),
            6,
        );
        get_current_process()
            .lock()
            .addrspace
            .write_buffer((user_page + 632) as *mut u8, &[0; 6])
            .unwrap();
        assert_eq!(
            SyscallArgs::new([pipe_read_fd as u64, user_page + 632, 6, 0, 0, 0]).call::<Read>(),
            Ok(3),
            "splice readback",
        );
        assert_user_bytes(user_page + 632, b"bcd");
        expect_errno(
            SyscallArgs::new([fd as u64, user_page + 616, pipe_write_fd as u64, 0, 1, 1])
                .call::<Splice>(),
            SyscallError::InvalidArguments,
        );

        get_object_current_process(fd as u64)
            .unwrap()
            .as_file_like()
            .unwrap()
            .truncate(0)
            .unwrap();
        get_object_current_process(copy_out_fd as u64)
            .unwrap()
            .as_file_like()
            .unwrap()
            .truncate(0)
            .unwrap();
        expect_ok(
            SyscallArgs::new([fd as u64, user_page + 704, 6, 0, 0, 0]).call::<Write>(),
            6,
        );
        write_user_value(user_page + 640, &2i64);
        write_user_value(user_page + 648, &1i64);
        assert_eq!(
            SyscallArgs::new([
                fd as u64,
                user_page + 640,
                copy_out_fd as u64,
                user_page + 648,
                2,
                0,
            ])
            .call::<CopyFileRange>(),
            Ok(2),
            "copy_file_range result",
        );
        assert_eq!(read_user_value::<i64>(user_page + 640), 4);
        assert_eq!(read_user_value::<i64>(user_page + 648), 3);
        expect_ok(
            SyscallArgs::new([copy_out_fd as u64, 0, 0, 0, 0, 0]).call::<Lseek>(),
            0,
        );
        get_current_process()
            .lock()
            .addrspace
            .write_buffer((user_page + 656) as *mut u8, &[0; 6])
            .unwrap();
        assert_eq!(
            SyscallArgs::new([copy_out_fd as u64, user_page + 656, 6, 0, 0, 0]).call::<Read>(),
            Ok(3),
            "copy_file_range readback",
        );
        assert_user_bytes(user_page + 656, b"\0cd");
        expect_ok(
            SyscallArgs::new([fd as u64, 0, SEEK_CUR, 0, 0, 0]).call::<Lseek>(),
            6,
        );
        expect_ok(
            SyscallArgs::new([copy_out_fd as u64, 0, 0, 0, 0, 0]).call::<Lseek>(),
            0,
        );
        write_user_value(user_page + 640, &0i64);
        assert_eq!(
            SyscallArgs::new([fd as u64, user_page + 640, copy_out_fd as u64, 0, 1, 0])
                .call::<CopyFileRange>(),
            Ok(1),
            "copy_file_range mixed offset result",
        );
        assert_eq!(read_user_value::<i64>(user_page + 640), 1);
        expect_ok(
            SyscallArgs::new([fd as u64, 0, SEEK_CUR, 0, 0, 0]).call::<Lseek>(),
            6,
        );
        expect_ok(
            SyscallArgs::new([copy_out_fd as u64, 0, SEEK_CUR, 0, 0, 0]).call::<Lseek>(),
            1,
        );
        expect_errno(
            SyscallArgs::new([fd as u64, 0, copy_out_fd as u64, 0, 1, 1]).call::<CopyFileRange>(),
            SyscallError::InvalidArguments,
        );

        close_test_fd(pipe_write_fd);
        close_test_fd(pipe_read_fd);
        close_test_fd(copy_out_fd);
        close_test_fd(out_fd);
        close_test_fd(fd);
        for path in cleanup_paths {
            let _ = VirtualFS.lock().delete_file(Path::new(path));
        }
    }

    fn filesystem_file_metadata_syscalls_follow_linux_rules() {
        const AT_FDCWD: u64 = (-100i32) as u64;
        const AT_SYMLINK_NOFOLLOW: u64 = 0x100;
        const TMPFS_MAGIC: i64 = 0x0102_1994;

        let base_path = Path::new("/tmp/syscall-file-metadata-test");
        let cleanup_paths = [
            "/tmp/syscall-file-metadata-test/link",
            "/tmp/syscall-file-metadata-test/file",
            "/tmp/syscall-file-metadata-test/node",
            "/tmp/syscall-file-metadata-test",
        ];
        for path in cleanup_paths {
            let _ = VirtualFS.lock().delete_file(Path::new(path));
        }
        VirtualFS.lock().create_dir(base_path.clone()).unwrap();
        VirtualFS
            .lock()
            .create_file(Path::new("/tmp/syscall-file-metadata-test/file"))
            .unwrap();
        VirtualFS
            .lock()
            .create_symlink(
                Path::new("/tmp/syscall-file-metadata-test/link"),
                "/tmp/syscall-file-metadata-test/file",
            )
            .unwrap();

        #[repr(C)]
        #[derive(Clone, Copy, Default)]
        struct TestLinuxStatFs {
            f_type: i64,
            f_bsize: i64,
            f_blocks: u64,
            f_bfree: u64,
            f_bavail: u64,
            f_files: u64,
            f_ffree: u64,
            f_fsid: i64,
            f_namelen: i64,
            f_frsize: i64,
            f_flags: i64,
            f_spare: [i64; 4],
        }

        let user_page = allocate_user_test_page();
        write_user_cstr(user_page, b"/tmp/syscall-file-metadata-test/file\0");
        let fd = expect_fd(
            SyscallArgs::new([
                AT_FDCWD,
                user_page,
                OpenFlags::empty().bits() as u64,
                0,
                0,
                0,
            ])
            .call::<OpenAt>(),
        );

        expect_ok(
            SyscallArgs::new([user_page, user_page + 256, 0, 0, 0, 0]).call::<Statfs>(),
            0,
        );
        let statfs = read_user_value::<TestLinuxStatFs>(user_page + 256);
        assert_eq!(statfs.f_type, TMPFS_MAGIC);
        assert_eq!(statfs.f_bsize, 4096);
        assert_eq!(statfs.f_namelen, 255);

        expect_ok(
            SyscallArgs::new([fd as u64, user_page + 384, 0, 0, 0, 0]).call::<Fstatfs>(),
            0,
        );
        let fstatfs = read_user_value::<TestLinuxStatFs>(user_page + 384);
        assert_eq!(fstatfs.f_type, TMPFS_MAGIC);
        expect_errno(
            SyscallArgs::new([4096, user_page + 384, 0, 0, 0, 0]).call::<Fstatfs>(),
            SyscallError::BadFileDescriptor,
        );
        expect_errno(
            SyscallArgs::new([fd as u64, 0, 0, 0, 0, 0]).call::<Fstatfs>(),
            SyscallError::BadAddress,
        );

        expect_ok(
            SyscallArgs::new([user_page, 123, 456, 0, 0, 0])
                .call::<crate::systemcall::implementations::Chown>(),
            0,
        );
        expect_ok(
            SyscallArgs::new([fd as u64, 123, 456, 0, 0, 0]).call::<Fchown>(),
            0,
        );
        write_user_cstr(user_page + 128, b"/tmp/syscall-file-metadata-test/link\0");
        expect_ok(
            SyscallArgs::new([AT_FDCWD, user_page + 128, 1, 2, AT_SYMLINK_NOFOLLOW, 0])
                .call::<Fchownat>(),
            0,
        );
        expect_errno(
            SyscallArgs::new([AT_FDCWD, 0, 1, 2, 0, 0]).call::<Fchownat>(),
            SyscallError::BadAddress,
        );
        expect_errno(
            SyscallArgs::new([fd as u64, user_page + 128, 1, 2, 0x4000_0000, 0]).call::<Fchownat>(),
            SyscallError::InvalidArguments,
        );

        write_user_cstr(user_page + 192, b"/tmp/syscall-file-metadata-test/node\0");
        expect_ok(
            SyscallArgs::new([AT_FDCWD, user_page + 192, 0o100600, 0, 0, 0]).call::<Mknodat>(),
            0,
        );
        let node_object = {
            let mut vfs = VirtualFS.lock();
            vfs.open(Path::new("/tmp/syscall-file-metadata-test/node"))
                .unwrap()
        };
        assert_eq!(node_object.stat().st_mode & 0o170000, 0o100000);
        assert_eq!(node_object.stat().st_mode & 0o777, 0o600);
        expect_errno(
            SyscallArgs::new([AT_FDCWD, user_page + 192, 0o040755, 0, 0, 0]).call::<Mknodat>(),
            SyscallError::NoSyscall,
        );

        close_test_fd(fd);
        for path in cleanup_paths {
            let _ = VirtualFS.lock().delete_file(Path::new(path));
        }
    }

    fn filesystem_xattr_syscalls_follow_linux_rules() {
        const AT_FDCWD: u64 = (-100i32) as u64;
        const XATTR_CREATE: u64 = 0x1;
        const XATTR_REPLACE: u64 = 0x2;

        let base_path = Path::new("/tmp/syscall-xattr-test");
        let cleanup_paths = [
            "/tmp/syscall-xattr-test/link",
            "/tmp/syscall-xattr-test/file",
            "/tmp/syscall-xattr-test",
        ];
        for path in cleanup_paths {
            let _ = VirtualFS.lock().delete_file(Path::new(path));
        }
        VirtualFS.lock().create_dir(base_path.clone()).unwrap();
        VirtualFS
            .lock()
            .create_file(Path::new("/tmp/syscall-xattr-test/file"))
            .unwrap();
        VirtualFS
            .lock()
            .create_symlink(
                Path::new("/tmp/syscall-xattr-test/link"),
                "/tmp/syscall-xattr-test/file",
            )
            .unwrap();

        let user_page = allocate_user_test_page();
        write_user_cstr(user_page, b"/tmp/syscall-xattr-test/file\0");
        write_user_cstr(user_page + 128, b"user.test\0");
        let fd = expect_fd(
            SyscallArgs::new([
                AT_FDCWD,
                user_page,
                OpenFlags::empty().bits() as u64,
                0,
                0,
                0,
            ])
            .call::<OpenAt>(),
        );

        expect_ok(
            SyscallArgs::new([user_page, user_page + 128, user_page + 256, 4, 0, 0])
                .call::<Setxattr>(),
            0,
        );
        expect_ok(
            SyscallArgs::new([
                user_page,
                user_page + 128,
                user_page + 256,
                4,
                XATTR_CREATE,
                0,
            ])
            .call::<Setxattr>(),
            0,
        );
        expect_ok(
            SyscallArgs::new([
                user_page,
                user_page + 128,
                user_page + 256,
                4,
                XATTR_REPLACE,
                0,
            ])
            .call::<Setxattr>(),
            0,
        );
        expect_errno(
            SyscallArgs::new([
                user_page,
                user_page + 128,
                user_page + 256,
                4,
                XATTR_CREATE | XATTR_REPLACE,
                0,
            ])
            .call::<Setxattr>(),
            SyscallError::InvalidArguments,
        );
        expect_errno(
            SyscallArgs::new([user_page, user_page + 128, user_page + 256, 4, 0x4, 0])
                .call::<Setxattr>(),
            SyscallError::InvalidArguments,
        );

        write_user_cstr(user_page + 64, b"/tmp/syscall-xattr-test/link\0");
        expect_ok(
            SyscallArgs::new([user_page + 64, user_page + 128, user_page + 256, 4, 0, 0])
                .call::<Lsetxattr>(),
            0,
        );
        expect_ok(
            SyscallArgs::new([fd as u64, user_page + 128, user_page + 256, 4, 0, 0])
                .call::<Fsetxattr>(),
            0,
        );

        expect_errno(
            SyscallArgs::new([user_page, user_page + 128, user_page + 384, 16, 0, 0])
                .call::<Getxattr>(),
            SyscallError::NoData,
        );
        expect_errno(
            SyscallArgs::new([user_page + 64, user_page + 128, user_page + 384, 16, 0, 0])
                .call::<Lgetxattr>(),
            SyscallError::NoData,
        );
        expect_errno(
            SyscallArgs::new([fd as u64, user_page + 128, user_page + 384, 16, 0, 0])
                .call::<Fgetxattr>(),
            SyscallError::NoData,
        );

        expect_ok(
            SyscallArgs::new([user_page, user_page + 512, 0, 0, 0, 0]).call::<Listxattr>(),
            0,
        );
        expect_ok(
            SyscallArgs::new([user_page + 64, user_page + 512, 0, 0, 0, 0]).call::<Llistxattr>(),
            0,
        );
        expect_ok(
            SyscallArgs::new([fd as u64, user_page + 512, 0, 0, 0, 0]).call::<Flistxattr>(),
            0,
        );

        expect_errno(
            SyscallArgs::new([user_page, user_page + 128, 0, 0, 0, 0]).call::<Removexattr>(),
            SyscallError::NoData,
        );
        expect_errno(
            SyscallArgs::new([user_page + 64, user_page + 128, 0, 0, 0, 0]).call::<Lremovexattr>(),
            SyscallError::NoData,
        );
        expect_errno(
            SyscallArgs::new([fd as u64, user_page + 128, 0, 0, 0, 0]).call::<Fremovexattr>(),
            SyscallError::NoData,
        );

        write_user_cstr(user_page + 192, b"/tmp/syscall-xattr-test/missing\0");
        expect_errno(
            SyscallArgs::new([user_page + 192, user_page + 128, user_page + 256, 4, 0, 0])
                .call::<Setxattr>(),
            SyscallError::FileNotFound,
        );
        expect_errno(
            SyscallArgs::new([user_page + 192, user_page + 128, user_page + 384, 16, 0, 0])
                .call::<Getxattr>(),
            SyscallError::FileNotFound,
        );

        close_test_fd(fd);
        for path in cleanup_paths {
            let _ = VirtualFS.lock().delete_file(Path::new(path));
        }
    }

    fn filesystem_statx_syscalls_follow_linux_rules() {
        const AT_FDCWD: u64 = (-100i32) as u64;
        const AT_EMPTY_PATH: u64 = 0x1000;
        const AT_SYMLINK_NOFOLLOW: u64 = 0x100;
        const AT_STATX_FORCE_SYNC: u64 = 0x2000;
        const AT_STATX_DONT_SYNC: u64 = 0x4000;
        const STATX_BASIC_STATS: u64 = 0x0000_07ff;
        const STATX_MNT_ID: u32 = 0x0000_1000;
        const STATX_ATTR_MOUNT_ROOT: u64 = 0x0000_2000;

        let base_path = Path::new("/tmp/syscall-statx-test");
        let cleanup_paths = [
            "/tmp/syscall-statx-test/link",
            "/tmp/syscall-statx-test/file",
            "/tmp/syscall-statx-test",
        ];
        for path in cleanup_paths {
            let _ = VirtualFS.lock().delete_file(Path::new(path));
        }
        VirtualFS.lock().create_dir(base_path.clone()).unwrap();
        VirtualFS
            .lock()
            .create_file(Path::new("/tmp/syscall-statx-test/file"))
            .unwrap();
        VirtualFS
            .lock()
            .create_symlink(
                Path::new("/tmp/syscall-statx-test/link"),
                "/tmp/syscall-statx-test/file",
            )
            .unwrap();

        assert_linux_layout::<TestLinuxStatxTimestamp>(16, 8);
        assert_linux_layout::<TestLinuxStatx>(256, 8);
        assert_linux_layout::<TestLinuxFileHandle>(8, 4);

        let user_page = allocate_user_test_page();
        write_user_cstr(user_page, b"/tmp/syscall-statx-test/file\0");
        write_user_cstr(user_page + 64, b"/tmp/syscall-statx-test/link\0");
        let file_fd = expect_fd(
            SyscallArgs::new([
                AT_FDCWD,
                user_page,
                OpenFlags::empty().bits() as u64,
                0,
                0,
                0,
            ])
            .call::<OpenAt>(),
        );

        expect_ok(
            SyscallArgs::new([
                AT_FDCWD,
                user_page,
                0,
                STATX_BASIC_STATS,
                user_page + 256,
                0,
            ])
            .call::<Statx>(),
            0,
        );
        let statx = read_user_value::<TestLinuxStatx>(user_page + 256);
        let file_stat = {
            let file = {
                let mut vfs = VirtualFS.lock();
                vfs.open(Path::new("/tmp/syscall-statx-test/file")).unwrap()
            };
            file.stat()
        };
        assert_eq!(statx.stx_mask, STATX_BASIC_STATS as u32 | STATX_MNT_ID);
        assert_eq!(statx.stx_mode, file_stat.st_mode as u16);
        assert_eq!(statx.stx_nlink, file_stat.st_nlink as u32);
        assert_eq!(statx.stx_size, file_stat.st_size as u64);
        assert_eq!(statx.stx_ino, file_stat.st_ino);
        assert_eq!(statx.stx_attributes_mask, STATX_ATTR_MOUNT_ROOT);
        assert_eq!(statx.stx_attributes & STATX_ATTR_MOUNT_ROOT, 0);
        assert!(statx.stx_mnt_id >= 1);
        assert_eq!(statx.stx_btime.tv_sec, 0);
        assert_eq!(statx.stx_btime.tv_nsec, 0);

        expect_ok(
            SyscallArgs::new([
                AT_FDCWD,
                user_page + 64,
                AT_SYMLINK_NOFOLLOW,
                STATX_BASIC_STATS,
                user_page + 256,
                0,
            ])
            .call::<Statx>(),
            0,
        );
        let link_statx = read_user_value::<TestLinuxStatx>(user_page + 256);
        assert_ne!(link_statx.stx_ino, statx.stx_ino);

        expect_ok(
            SyscallArgs::new([
                file_fd as u64,
                0,
                AT_EMPTY_PATH,
                STATX_BASIC_STATS,
                user_page + 256,
                0,
            ])
            .call::<Statx>(),
            0,
        );
        let empty_path_statx = read_user_value::<TestLinuxStatx>(user_page + 256);
        assert_eq!(empty_path_statx.stx_ino, statx.stx_ino);

        expect_errno(
            SyscallArgs::new([
                AT_FDCWD,
                user_page,
                0x8000_0000,
                STATX_BASIC_STATS,
                user_page + 256,
                0,
            ])
            .call::<Statx>(),
            SyscallError::InvalidArguments,
        );
        expect_errno(
            SyscallArgs::new([
                AT_FDCWD,
                user_page,
                AT_STATX_FORCE_SYNC | AT_STATX_DONT_SYNC,
                STATX_BASIC_STATS,
                user_page + 256,
                0,
            ])
            .call::<Statx>(),
            SyscallError::InvalidArguments,
        );
        expect_errno(
            SyscallArgs::new([file_fd as u64, 0, 0, STATX_BASIC_STATS, user_page + 256, 0])
                .call::<Statx>(),
            SyscallError::BadAddress,
        );
        expect_errno(
            SyscallArgs::new([file_fd as u64, 0, AT_EMPTY_PATH, STATX_BASIC_STATS, 0, 0])
                .call::<Statx>(),
            SyscallError::BadAddress,
        );

        close_test_fd(file_fd);
        for path in cleanup_paths {
            let _ = VirtualFS.lock().delete_file(Path::new(path));
        }
    }

    fn filesystem_name_to_handle_short_buffer_syscalls_follow_linux_rules() {
        const AT_FDCWD: u64 = (-100i32) as u64;

        let base_path = Path::new("/tmp/syscall-name-handle-test");
        let cleanup_paths = [
            "/tmp/syscall-name-handle-test/file",
            "/tmp/syscall-name-handle-test",
        ];
        for path in cleanup_paths {
            let _ = VirtualFS.lock().delete_file(Path::new(path));
        }
        VirtualFS.lock().create_dir(base_path.clone()).unwrap();
        VirtualFS
            .lock()
            .create_file(Path::new("/tmp/syscall-name-handle-test/file"))
            .unwrap();

        assert_linux_layout::<TestLinuxFileHandle>(8, 4);

        let user_page = allocate_user_test_page();
        write_user_cstr(user_page, b"/tmp/syscall-name-handle-test/file\0");
        write_user_value(
            user_page + 512,
            &TestLinuxFileHandle {
                handle_bytes: 4,
                handle_type: 0,
            },
        );
        expect_errno(
            SyscallArgs::new([AT_FDCWD, user_page, user_page + 512, user_page + 520, 0, 0])
                .call::<NameToHandleAt>(),
            SyscallError::ValueTooLarge,
        );
        let short_handle = read_user_value::<TestLinuxFileHandle>(user_page + 512);
        assert_eq!(short_handle.handle_bytes, 8);
        assert_eq!(short_handle.handle_type, 1);
        assert!(read_user_value::<i32>(user_page + 520) >= 1);

        for path in cleanup_paths {
            let _ = VirtualFS.lock().delete_file(Path::new(path));
        }
    }

    fn filesystem_name_to_handle_success_syscalls_follow_linux_rules() {
        const AT_FDCWD: u64 = (-100i32) as u64;

        let base_path = Path::new("/tmp/syscall-name-handle-test");
        let cleanup_paths = [
            "/tmp/syscall-name-handle-test/file",
            "/tmp/syscall-name-handle-test",
        ];
        for path in cleanup_paths {
            let _ = VirtualFS.lock().delete_file(Path::new(path));
        }
        VirtualFS.lock().create_dir(base_path.clone()).unwrap();
        VirtualFS
            .lock()
            .create_file(Path::new("/tmp/syscall-name-handle-test/file"))
            .unwrap();

        assert_linux_layout::<TestLinuxFileHandle>(8, 4);

        let user_page = allocate_user_test_page();
        write_user_cstr(user_page, b"/tmp/syscall-name-handle-test/file\0");
        let file_stat = {
            let file = {
                let mut vfs = VirtualFS.lock();
                vfs.open(Path::new("/tmp/syscall-name-handle-test/file"))
                    .unwrap()
            };
            file.stat()
        };

        write_user_value(
            user_page + 512,
            &TestLinuxFileHandle {
                handle_bytes: 8,
                handle_type: 0,
            },
        );
        expect_ok(
            SyscallArgs::new([AT_FDCWD, user_page, user_page + 512, user_page + 520, 0, 0])
                .call::<NameToHandleAt>(),
            0,
        );
        let full_handle = read_user_value::<TestLinuxFileHandle>(user_page + 512);
        assert_eq!(full_handle.handle_bytes, 8);
        assert_eq!(full_handle.handle_type, 1);
        assert_eq!(
            read_user_value::<u64>(user_page + 512 + 8),
            file_stat.st_ino
        );

        for path in cleanup_paths {
            let _ = VirtualFS.lock().delete_file(Path::new(path));
        }
    }

    fn filesystem_name_to_handle_null_handle_syscalls_follow_linux_rules() {
        const AT_FDCWD: u64 = (-100i32) as u64;

        let base_path = Path::new("/tmp/syscall-name-handle-test");
        let cleanup_paths = [
            "/tmp/syscall-name-handle-test/file",
            "/tmp/syscall-name-handle-test",
        ];
        for path in cleanup_paths {
            let _ = VirtualFS.lock().delete_file(Path::new(path));
        }
        VirtualFS.lock().create_dir(base_path.clone()).unwrap();
        VirtualFS
            .lock()
            .create_file(Path::new("/tmp/syscall-name-handle-test/file"))
            .unwrap();

        let user_page = allocate_user_test_page();
        write_user_cstr(user_page, b"/tmp/syscall-name-handle-test/file\0");
        write_user_value(
            user_page + 512,
            &TestLinuxFileHandle {
                handle_bytes: 8,
                handle_type: 0,
            },
        );

        expect_errno(
            SyscallArgs::new([AT_FDCWD, user_page, 0, user_page + 520, 0, 0])
                .call::<NameToHandleAt>(),
            SyscallError::BadAddress,
        );

        for path in cleanup_paths {
            let _ = VirtualFS.lock().delete_file(Path::new(path));
        }
    }

    fn filesystem_name_to_handle_null_mount_id_syscalls_follow_linux_rules() {
        const AT_FDCWD: u64 = (-100i32) as u64;

        let base_path = Path::new("/tmp/syscall-name-handle-test");
        let cleanup_paths = [
            "/tmp/syscall-name-handle-test/file",
            "/tmp/syscall-name-handle-test",
        ];
        for path in cleanup_paths {
            let _ = VirtualFS.lock().delete_file(Path::new(path));
        }
        VirtualFS.lock().create_dir(base_path.clone()).unwrap();
        VirtualFS
            .lock()
            .create_file(Path::new("/tmp/syscall-name-handle-test/file"))
            .unwrap();

        let user_page = allocate_user_test_page();
        write_user_cstr(user_page, b"/tmp/syscall-name-handle-test/file\0");
        write_user_value(
            user_page + 512,
            &TestLinuxFileHandle {
                handle_bytes: 8,
                handle_type: 0,
            },
        );

        expect_errno(
            SyscallArgs::new([AT_FDCWD, user_page, user_page + 512, 0, 0, 0])
                .call::<NameToHandleAt>(),
            SyscallError::BadAddress,
        );

        for path in cleanup_paths {
            let _ = VirtualFS.lock().delete_file(Path::new(path));
        }
    }

    fn filesystem_name_to_handle_bad_flag_syscalls_follow_linux_rules() {
        const AT_FDCWD: u64 = (-100i32) as u64;

        let base_path = Path::new("/tmp/syscall-name-handle-test");
        let cleanup_paths = [
            "/tmp/syscall-name-handle-test/file",
            "/tmp/syscall-name-handle-test",
        ];
        for path in cleanup_paths {
            let _ = VirtualFS.lock().delete_file(Path::new(path));
        }
        VirtualFS.lock().create_dir(base_path.clone()).unwrap();
        VirtualFS
            .lock()
            .create_file(Path::new("/tmp/syscall-name-handle-test/file"))
            .unwrap();

        let user_page = allocate_user_test_page();
        write_user_cstr(user_page, b"/tmp/syscall-name-handle-test/file\0");
        write_user_value(
            user_page + 512,
            &TestLinuxFileHandle {
                handle_bytes: 8,
                handle_type: 0,
            },
        );

        expect_errno(
            SyscallArgs::new([
                AT_FDCWD,
                user_page,
                user_page + 512,
                user_page + 520,
                0x4000_0000,
                0,
            ])
            .call::<NameToHandleAt>(),
            SyscallError::InvalidArguments,
        );

        for path in cleanup_paths {
            let _ = VirtualFS.lock().delete_file(Path::new(path));
        }
    }

    fn filesystem_utimensat_success_syscalls_follow_linux_rules() {
        const AT_FDCWD: u64 = (-100i32) as u64;
        const AT_EMPTY_PATH: u64 = 0x1000;
        const UTIME_OMIT: i64 = 0x3fff_ffff;

        let base_path = Path::new("/tmp/syscall-utimensat-test");
        let cleanup_paths = [
            "/tmp/syscall-utimensat-test/file",
            "/tmp/syscall-utimensat-test",
        ];
        for path in cleanup_paths {
            let _ = VirtualFS.lock().delete_file(Path::new(path));
        }
        VirtualFS.lock().create_dir(base_path.clone()).unwrap();
        VirtualFS
            .lock()
            .create_file(Path::new("/tmp/syscall-utimensat-test/file"))
            .unwrap();

        let user_page = allocate_user_test_page();
        write_user_cstr(user_page, b"/tmp/syscall-utimensat-test/file\0");
        let file_fd = expect_fd(
            SyscallArgs::new([
                AT_FDCWD,
                user_page,
                OpenFlags::empty().bits() as u64,
                0,
                0,
                0,
            ])
            .call::<OpenAt>(),
        );

        let valid_times = [[0i64, 0i64], [0i64, UTIME_OMIT]];
        write_user_value(user_page + 640, &valid_times);
        expect_ok(
            SyscallArgs::new([file_fd as u64, 0, user_page + 640, 0, 0, 0]).call::<Utimensat>(),
            0,
        );
        expect_ok(
            SyscallArgs::new([file_fd as u64, user_page, user_page + 640, 0, 0, 0])
                .call::<Utimensat>(),
            0,
        );
        write_user_cstr(user_page + 704, b"\0");
        expect_ok(
            SyscallArgs::new([
                file_fd as u64,
                user_page + 704,
                user_page + 640,
                AT_EMPTY_PATH,
                0,
                0,
            ])
            .call::<Utimensat>(),
            0,
        );

        close_test_fd(file_fd);
        for path in cleanup_paths {
            let _ = VirtualFS.lock().delete_file(Path::new(path));
        }
    }

    fn prepare_utimensat_test_file() -> (usize, [u64; 2]) {
        const AT_FDCWD: u64 = (-100i32) as u64;

        let base_path = Path::new("/tmp/syscall-utimensat-test");
        let cleanup_paths = [
            "/tmp/syscall-utimensat-test/file",
            "/tmp/syscall-utimensat-test",
        ];
        for path in cleanup_paths {
            let _ = VirtualFS.lock().delete_file(Path::new(path));
        }
        VirtualFS.lock().create_dir(base_path.clone()).unwrap();
        VirtualFS
            .lock()
            .create_file(Path::new("/tmp/syscall-utimensat-test/file"))
            .unwrap();

        let user_page = allocate_user_test_page();
        write_user_cstr(user_page, b"/tmp/syscall-utimensat-test/file\0");
        write_user_cstr(user_page + 704, b"\0");
        let file_fd = expect_fd(
            SyscallArgs::new([
                AT_FDCWD,
                user_page,
                OpenFlags::empty().bits() as u64,
                0,
                0,
                0,
            ])
            .call::<OpenAt>(),
        );

        (file_fd, [user_page, user_page + 640])
    }

    fn cleanup_utimensat_test_file(file_fd: usize) {
        let cleanup_paths = [
            "/tmp/syscall-utimensat-test/file",
            "/tmp/syscall-utimensat-test",
        ];
        close_test_fd(file_fd);
        for path in cleanup_paths {
            let _ = VirtualFS.lock().delete_file(Path::new(path));
        }
    }

    fn filesystem_utimensat_negative_nsec_syscalls_follow_linux_rules() {
        let (file_fd, pages) = prepare_utimensat_test_file();
        let [user_page, times_page] = pages;

        write_user_value(times_page, &[[0i64, 0i64], [0i64, -1i64]]);
        expect_errno(
            SyscallArgs::new([file_fd as u64, user_page, times_page, 0, 0, 0]).call::<Utimensat>(),
            SyscallError::InvalidArguments,
        );
        write_user_value(times_page, &[[-1i64, -1i64], [0i64, 0i64]]);
        expect_errno(
            SyscallArgs::new([file_fd as u64, user_page, times_page, 0, 0, 0]).call::<Utimensat>(),
            SyscallError::InvalidArguments,
        );

        cleanup_utimensat_test_file(file_fd);
    }

    fn filesystem_utimensat_null_path_empty_path_syscalls_follow_linux_rules() {
        const AT_EMPTY_PATH: u64 = 0x1000;

        let (file_fd, [_user_page, times_page]) = prepare_utimensat_test_file();
        write_user_value(times_page, &[[0i64, 0i64], [0i64, 0i64]]);
        expect_errno(
            SyscallArgs::new([file_fd as u64, 0, times_page, AT_EMPTY_PATH, 0, 0])
                .call::<Utimensat>(),
            SyscallError::InvalidArguments,
        );

        cleanup_utimensat_test_file(file_fd);
    }

    fn filesystem_utimensat_empty_path_without_flag_syscalls_follow_linux_rules() {
        let (file_fd, pages) = prepare_utimensat_test_file();
        let [_user_page, times_page] = pages;

        write_user_value(times_page, &[[0i64, 0i64], [0i64, 0i64]]);
        expect_errno(
            SyscallArgs::new([file_fd as u64, times_page + 64, times_page, 0, 0, 0])
                .call::<Utimensat>(),
            SyscallError::FileNotFound,
        );

        cleanup_utimensat_test_file(file_fd);
    }

    fn filesystem_utimensat_at_fdcwd_null_path_syscalls_follow_linux_rules() {
        const AT_FDCWD: u64 = (-100i32) as u64;

        let (file_fd, [_user_page, times_page]) = prepare_utimensat_test_file();
        write_user_value(times_page, &[[0i64, 0i64], [0i64, 0i64]]);
        expect_errno(
            SyscallArgs::new([AT_FDCWD, 0, times_page, 0, 0, 0]).call::<Utimensat>(),
            SyscallError::BadAddress,
        );

        cleanup_utimensat_test_file(file_fd);
    }

    fn filesystem_utimensat_invalid_flag_syscalls_follow_linux_rules() {
        const AT_FDCWD: u64 = (-100i32) as u64;

        let (file_fd, [user_page, times_page]) = prepare_utimensat_test_file();
        write_user_value(times_page, &[[0i64, 0i64], [0i64, 0i64]]);
        expect_errno(
            SyscallArgs::new([AT_FDCWD, user_page, times_page, 0x200, 0, 0]).call::<Utimensat>(),
            SyscallError::InvalidArguments,
        );

        cleanup_utimensat_test_file(file_fd);
    }

    fn procfs_syscalls_follow_linux_proc_abi_rules() {
        const AT_FDCWD: u64 = (-100i32) as u64;
        const O_WRONLY: u64 = 1;
        const O_DIRECTORY: u64 = 0o200000;
        const STATX_BASIC_STATS: u64 = 0x0000_07ff;
        const AT_SYMLINK_NOFOLLOW: u64 = 0x100;

        let page = allocate_large_user_test_region(4);
        let current_pid = get_current_process().lock().pid.0;
        let proc_pid_path = format!("/proc/{current_pid}/status\0");

        write_user_cstr(page, b"/proc/self/status\0");
        write_user_cstr(page + 128, proc_pid_path.as_bytes());
        write_user_cstr(page + 256, b"/proc/self\0");
        write_user_cstr(page + 384, b"/proc/self/root\0");
        write_user_cstr(page + 512, b"/proc/self/ns/net\0");
        write_user_cstr(page + 640, b"/proc/self/fd\0");
        write_user_cstr(page + 768, b"/proc/self/fdinfo\0");
        write_user_cstr(page + 896, b"/proc\0");
        write_user_cstr(page + 1024, b"/proc/pressure\0");
        write_user_cstr(page + 1152, b"/proc/sys/kernel/random\0");
        write_user_cstr(page + 1280, b"/proc/sys/kernel/hostname\0");
        write_user_cstr(page + 1408, b"/proc/sys/kernel/domainname\0");
        write_user_cstr(page + 1536, b"/proc/sys/fs/file-max\0");
        write_user_cstr(page + 1664, b"/proc/sys/fs/nr_open\0");
        write_user_cstr(page + 1792, b"/proc/self/oom_score_adj\0");
        write_user_cstr(page + 1920, b"/proc/self/uid_map\0");
        write_user_cstr(page + 2048, b"/proc/self/gid_map\0");
        write_user_cstr(page + 2176, b"/proc/self/setgroups\0");
        write_user_cstr(page + 2304, b"/proc/pressure/cpu\0");
        write_user_cstr(page + 2432, b"/proc/stat\0");
        write_user_cstr(page + 2560, b"/proc/uptime\0");
        write_user_cstr(page + 2688, b"/proc/mounts\0");
        write_user_cstr(page + 2816, b"/proc/self/mountinfo\0");
        write_user_cstr(page + 2944, b"/proc/sys/kernel/tainted\0");

        let self_status_fd = openat_fd(AT_FDCWD, page, OpenFlags::empty());
        let pid_status_fd = openat_fd(AT_FDCWD, page + 128, OpenFlags::empty());
        let self_status = read_file_via_fd(self_status_fd, page, 3072, 512);
        let pid_status = read_file_via_fd(pid_status_fd, page, 3584, 512);
        let self_status = core::str::from_utf8(&self_status).unwrap();
        let pid_status = core::str::from_utf8(&pid_status).unwrap();
        assert!(self_status.contains(&format!("Pid:\t{current_pid}\n")));
        assert!(pid_status.contains(&format!("Pid:\t{current_pid}\n")));
        close_test_fd(self_status_fd);
        close_test_fd(pid_status_fd);
        let tainted_fd = openat_fd(AT_FDCWD, page + 2944, OpenFlags::empty());
        let tainted = read_file_via_fd(tainted_fd, page, 3072, 16);
        assert_eq!(core::str::from_utf8(&tainted).unwrap(), "0\n");
        close_test_fd(tainted_fd);

        let self_target = readlink_bytes((-1i32) as u64, page + 256, page + 3200, 64);
        assert_eq!(
            core::str::from_utf8(&self_target).unwrap(),
            format!("{current_pid}")
        );
        let root_target = readlink_bytes((-1i32) as u64, page + 384, page + 3264, 64);
        assert_eq!(core::str::from_utf8(&root_target).unwrap(), "/");

        expect_ok(
            SyscallArgs::new([
                AT_FDCWD,
                page + 512,
                AT_SYMLINK_NOFOLLOW,
                STATX_BASIC_STATS,
                page + 3328,
                0,
            ])
            .call::<Statx>(),
            0,
        );
        let net_ns_statx = read_user_value::<TestLinuxStatx>(page + 3328);
        assert_eq!(net_ns_statx.stx_mode & 0o170000, 0o100000);
        assert!(net_ns_statx.stx_ino != 0);

        let known_fd = expect_fd(SyscallArgs::new([0, 0, 0, 0, 0, 0]).call::<Eventfd>());
        let fd_path = format!("/proc/self/fd/{known_fd}\0");
        let fdinfo_path = format!("/proc/self/fdinfo/{known_fd}\0");
        write_user_cstr(page + 3840, fd_path.as_bytes());
        write_user_cstr(page + 3968, fdinfo_path.as_bytes());
        let fd_target = readlink_bytes((-1i32) as u64, page + 3840, page + 4096, 128);
        assert_eq!(
            core::str::from_utf8(&fd_target).unwrap(),
            "anon_inode:[kernel::object::anon::eventfd::EventFdObject]"
        );
        let fdinfo_fd = openat_fd(AT_FDCWD, page + 3968, OpenFlags::empty());
        let fdinfo = read_file_via_fd(fdinfo_fd, page, 4224, 256);
        let fdinfo = core::str::from_utf8(&fdinfo).unwrap();
        assert!(fdinfo.contains("pos:\t0\n"));
        assert!(fdinfo.contains("flags:\t0\n"));
        assert!(fdinfo.contains("mnt_id:\t0\n"));
        assert!(fdinfo.contains("ino:\t0\n"));
        close_test_fd(fdinfo_fd);
        close_test_fd(known_fd);

        for (path_addr, expected) in [
            (
                page + 896,
                vec!["self".to_string(), current_pid.to_string()],
            ),
            (
                page + 1024,
                vec!["cpu".to_string(), "io".to_string(), "memory".to_string()],
            ),
            (page + 1152, vec!["boot_id".to_string(), "uuid".to_string()]),
        ] {
            let dir_fd = openat_fd(AT_FDCWD, path_addr, OpenFlags::DIRECTORY);
            let names = getdents_names(dir_fd, page, 4480, 1024)
                .into_iter()
                .map(|(name, _)| name)
                .collect::<Vec<_>>();
            for item in expected {
                assert!(names.contains(&item), "missing {item} in getdents output");
            }
            close_test_fd(dir_fd);
        }

        let hostname_snapshot_fd = openat_fd(AT_FDCWD, page + 1280, OpenFlags::empty());
        let hostname_before =
            core::str::from_utf8(&read_file_via_fd(hostname_snapshot_fd, page, 5632, 128))
                .unwrap()
                .trim()
                .to_string();
        close_test_fd(hostname_snapshot_fd);
        let domain_snapshot_fd = openat_fd(AT_FDCWD, page + 1408, OpenFlags::empty());
        let domain_before =
            core::str::from_utf8(&read_file_via_fd(domain_snapshot_fd, page, 5760, 128))
                .unwrap()
                .trim()
                .to_string();
        close_test_fd(domain_snapshot_fd);

        let rw_cases = [
            (
                page + 1280,
                b"proc-syscall-host\n".as_slice(),
                "proc-syscall-host\n",
            ),
            (
                page + 1408,
                b"proc-syscall-domain\n".as_slice(),
                "proc-syscall-domain\n",
            ),
            (page + 1536, b"456789\n".as_slice(), "456789\n"),
            (page + 1664, b"654321\n".as_slice(), "654321\n"),
            (page + 1792, b"321\n".as_slice(), "321\n"),
            (page + 1920, b"0 1000 1".as_slice(), "0 1000 1\n"),
            (page + 2048, b"0 1000 1".as_slice(), "0 1000 1\n"),
            (page + 2176, b"deny".as_slice(), "deny\n"),
        ];
        for (index, (path_addr, payload, expected)) in rw_cases.into_iter().enumerate() {
            let payload_addr = page + 5888 + (index as u64 * 64);
            let read_addr = page + 6656 + (index as u64 * 64);
            get_current_process()
                .lock()
                .addrspace
                .write_buffer(payload_addr as *mut u8, payload)
                .expect("test payload should be writable");
            let fd = openat_fd(AT_FDCWD, path_addr, OpenFlags::empty());
            expect_ok(
                SyscallArgs::new([fd as u64, payload_addr, payload.len() as u64, 0, 0, 0])
                    .call::<Write>(),
                payload.len(),
            );
            let rendered = read_file_via_fd(fd, page, read_addr - page, 128);
            assert_eq!(core::str::from_utf8(&rendered).unwrap(), expected);
            close_test_fd(fd);
        }

        let restore_hostname_fd = openat_fd(AT_FDCWD, page + 1280, OpenFlags::empty());
        get_current_process()
            .lock()
            .addrspace
            .write_buffer((page + 7424) as *mut u8, hostname_before.as_bytes())
            .expect("hostname restore payload should be writable");
        expect_ok(
            SyscallArgs::new([
                restore_hostname_fd as u64,
                page + 7424,
                hostname_before.len() as u64,
                0,
                0,
                0,
            ])
            .call::<Write>(),
            hostname_before.len(),
        );
        close_test_fd(restore_hostname_fd);
        let restore_domain_fd = openat_fd(AT_FDCWD, page + 1408, OpenFlags::empty());
        get_current_process()
            .lock()
            .addrspace
            .write_buffer((page + 7552) as *mut u8, domain_before.as_bytes())
            .expect("domain restore payload should be writable");
        expect_ok(
            SyscallArgs::new([
                restore_domain_fd as u64,
                page + 7552,
                domain_before.len() as u64,
                0,
                0,
                0,
            ])
            .call::<Write>(),
            domain_before.len(),
        );
        close_test_fd(restore_domain_fd);

        let invalid_numeric_fd = expect_fd(
            SyscallArgs::new([AT_FDCWD, page + 1536, O_WRONLY, 0, 0, 0]).call::<OpenAt>(),
        );
        get_current_process()
            .lock()
            .addrspace
            .write_buffer((page + 7680) as *mut u8, b"not-a-number")
            .expect("invalid numeric payload should be writable");
        expect_errno(
            SyscallArgs::new([invalid_numeric_fd as u64, page + 7680, 12, 0, 0, 0]).call::<Write>(),
            SyscallError::IOError,
        );
        close_test_fd(invalid_numeric_fd);
        let invalid_oom_fd = expect_fd(
            SyscallArgs::new([AT_FDCWD, page + 1792, O_WRONLY, 0, 0, 0]).call::<OpenAt>(),
        );
        get_current_process()
            .lock()
            .addrspace
            .write_buffer((page + 7808) as *mut u8, b"1001")
            .expect("invalid oom payload should be writable");
        expect_errno(
            SyscallArgs::new([invalid_oom_fd as u64, page + 7808, 4, 0, 0, 0]).call::<Write>(),
            SyscallError::IOError,
        );
        close_test_fd(invalid_oom_fd);

        let pressure_fd = openat_fd(AT_FDCWD, page + 2304, OpenFlags::empty());
        get_current_process()
            .lock()
            .addrspace
            .write_buffer((page + 7936) as *mut u8, b"some 150000 1000000")
            .expect("pressure payload should be writable");
        expect_ok(
            SyscallArgs::new([pressure_fd as u64, page + 7936, 19, 0, 0, 0]).call::<Write>(),
            19,
        );
        let pressure = read_file_via_fd(pressure_fd, page, 8064, 128);
        let pressure = core::str::from_utf8(&pressure).unwrap();
        assert!(pressure.contains("some avg10=0.00"));
        assert!(pressure.contains("full avg10=0.00"));
        close_test_fd(pressure_fd);

        for (path_addr, expected_fragments) in [
            (page + 2432, vec!["cpu  ", "btime ", "processes "]),
            (page + 2560, vec![".", "\n"]),
            (page + 2688, vec![" proc ", " sysfs ", " devtmpfs "]),
            (page + 2816, vec![" /proc ", " /sys ", " /dev "]),
        ] {
            let fd = openat_fd(AT_FDCWD, path_addr, OpenFlags::empty());
            let rendered = read_file_via_fd(fd, page, 8192, 2048);
            let rendered = core::str::from_utf8(&rendered).unwrap();
            for fragment in expected_fragments {
                assert!(
                    rendered.contains(fragment),
                    "missing {fragment} in {rendered}"
                );
            }
            close_test_fd(fd);
        }

        expect_errno(
            SyscallArgs::new([AT_FDCWD, page + 2432, O_DIRECTORY, 0, 0, 0]).call::<OpenAt>(),
            SyscallError::NotADirectory,
        );
    }

    fn sysfs_syscalls_follow_linux_sysfs_abi_rules() {
        const AT_FDCWD: u64 = (-100i32) as u64;
        const O_WRONLY: u64 = 1;
        const O_DIRECTORY: u64 = 0o200000;
        const AF_NETLINK: u64 = 16;
        const SOCK_DGRAM: u64 = 2;
        const SOL_NETLINK: u64 = 270;
        const NETLINK_KOBJECT_UEVENT: u64 = 15;
        const NETLINK_ADD_MEMBERSHIP: u64 = 1;
        const STATX_BASIC_STATS: u64 = 0x0000_07ff;
        const AT_SYMLINK_NOFOLLOW: u64 = 0x100;

        let page = allocate_user_test_page();
        write_user_cstr(page, b"/sys/class/tty/tty0/active\0");
        write_user_cstr(page + 128, b"/sys/devices/platform/i8042/uevent\0");
        write_user_cstr(page + 256, b"/sys/class/graphics/fb0/device/subsystem\0");
        write_user_cstr(page + 384, b"/sys/class/input/event0/device/subsystem\0");
        write_user_cstr(page + 512, b"/sys/class\0");
        write_user_cstr(page + 640, b"/sys/devices/platform\0");
        write_user_cstr(page + 768, b"/sys/dev/char\0");
        write_user_cstr(page + 896, b"/sys/devices/platform/uevent\0");
        write_user_cstr(page + 1024, b"/sys/devices\0");

        let active_fd = openat_fd(AT_FDCWD, page, OpenFlags::empty());
        let active = read_file_via_fd(active_fd, page, 1152, 64);
        let active = core::str::from_utf8(&active).unwrap();
        assert!(active.starts_with("tty"));
        assert!(active.ends_with('\n'));
        close_test_fd(active_fd);

        let i8042_fd = openat_fd(AT_FDCWD, page + 128, OpenFlags::empty());
        let i8042 = read_file_via_fd(i8042_fd, page, 1216, 128);
        let i8042 = core::str::from_utf8(&i8042).unwrap();
        assert!(i8042.contains("DRIVER=i8042"));
        assert!(i8042.contains("SUBSYSTEM=platform"));
        close_test_fd(i8042_fd);

        let fb_subsystem = readlink_bytes((-1i32) as u64, page + 256, page + 1344, 128);
        assert_eq!(
            core::str::from_utf8(&fb_subsystem).unwrap(),
            "/sys/bus/platform"
        );
        let input_subsystem = readlink_bytes((-1i32) as u64, page + 384, page + 1472, 128);
        assert_eq!(
            core::str::from_utf8(&input_subsystem).unwrap(),
            "/sys/class/input"
        );
        expect_ok(
            SyscallArgs::new([
                AT_FDCWD,
                page + 384,
                AT_SYMLINK_NOFOLLOW,
                STATX_BASIC_STATS,
                page + 1600,
                0,
            ])
            .call::<Statx>(),
            0,
        );
        let link_statx = read_user_value::<TestLinuxStatx>(page + 1600);
        assert_eq!(link_statx.stx_mode & 0o170000, 0o120000);

        for (path_addr, expected) in [
            (
                page + 512,
                vec![
                    "drm".to_string(),
                    "graphics".to_string(),
                    "input".to_string(),
                    "tty".to_string(),
                ],
            ),
            (
                page + 640,
                vec![
                    "uevent".to_string(),
                    "i8042".to_string(),
                    "seele-drm".to_string(),
                ],
            ),
            (
                page + 768,
                vec![
                    "13:64".to_string(),
                    "13:65".to_string(),
                    "226:0".to_string(),
                ],
            ),
        ] {
            let fd = openat_fd(AT_FDCWD, path_addr, OpenFlags::DIRECTORY);
            let names = getdents_names(fd, page, 1856, 1024)
                .into_iter()
                .map(|(name, _)| name)
                .collect::<Vec<_>>();
            for item in expected {
                assert!(names.contains(&item), "missing {item} in sysfs getdents");
            }
            close_test_fd(fd);
        }

        let uevent_sock = expect_fd(
            SyscallArgs::new([AF_NETLINK, SOCK_DGRAM, NETLINK_KOBJECT_UEVENT, 0, 0, 0])
                .call::<Socket>(),
        );
        write_user_value(page + 1984, &1i32);
        expect_ok(
            SyscallArgs::new([
                uevent_sock as u64,
                SOL_NETLINK,
                NETLINK_ADD_MEMBERSHIP,
                page + 1984,
                4,
                0,
            ])
            .call::<Setsockopt>(),
            0,
        );
        let uevent_fd =
            expect_fd(SyscallArgs::new([AT_FDCWD, page + 896, O_WRONLY, 0, 0, 0]).call::<OpenAt>());
        let uevent_payload = b"add synthetic-uuid ACTION=spoof DEVPATH=/fake KEY=VALUE";
        get_current_process()
            .lock()
            .addrspace
            .write_buffer((page + 2048) as *mut u8, uevent_payload)
            .expect("uevent payload should be writable");
        expect_ok(
            SyscallArgs::new([
                uevent_fd as u64,
                page + 2048,
                uevent_payload.len() as u64,
                0,
                0,
                0,
            ])
            .call::<Write>(),
            uevent_payload.len(),
        );
        write_user_value(page + 2816, &12u32);
        let recv_len = SyscallArgs::new([
            uevent_sock as u64,
            page + 2112,
            512,
            0,
            page + 2688,
            page + 2816,
        ])
        .call::<Recvfrom>()
        .expect("uevent recvfrom should succeed");
        let uevent_bytes = read_user_bytes(page + 2112, recv_len);
        let uevent_text = uevent_bytes
            .split(|byte| *byte == 0)
            .filter(|segment| !segment.is_empty())
            .map(|segment| core::str::from_utf8(segment).unwrap().to_string())
            .collect::<Vec<_>>();
        assert_eq!(uevent_text[0], "add@/devices/platform");
        assert!(uevent_text.iter().any(|line| line == "ACTION=add"));
        assert!(
            uevent_text
                .iter()
                .any(|line| line == "DEVPATH=/devices/platform")
        );
        assert!(uevent_text.iter().any(|line| line == "SUBSYSTEM=platform"));
        assert!(uevent_text.iter().any(|line| line == "SYNTH_ARG_KEY=VALUE"));
        assert!(
            uevent_text
                .iter()
                .any(|line| line == "SYNTH_ARG_ACTION=spoof")
        );
        assert!(
            uevent_text
                .iter()
                .any(|line| line == "SYNTH_ARG_DEVPATH=/fake")
        );
        assert!(
            uevent_text
                .iter()
                .any(|line| line == "SYNTH_UUID=synthetic-uuid")
        );
        let seq_line = uevent_text
            .iter()
            .find(|line| line.starts_with("SEQNUM="))
            .expect("uevent should include seqnum");
        assert!(seq_line[7..].parse::<u64>().is_ok());
        close_test_fd(uevent_fd);
        close_test_fd(uevent_sock);

        expect_errno(
            SyscallArgs::new([AT_FDCWD, page, O_DIRECTORY, 0, 0, 0]).call::<OpenAt>(),
            SyscallError::NotADirectory,
        );
        let readonly_active_fd = openat_fd(AT_FDCWD, page, OpenFlags::empty());
        get_current_process()
            .lock()
            .addrspace
            .write_buffer((page + 2432) as *mut u8, b"tty2")
            .expect("readonly payload should be writable");
        expect_errno(
            SyscallArgs::new([readonly_active_fd as u64, page + 2432, 4, 0, 0, 0]).call::<Write>(),
            SyscallError::ReadOnlyFileSystem,
        );
        close_test_fd(readonly_active_fd);
    }
}
