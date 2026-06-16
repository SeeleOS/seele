use crate::{
    define_syscall,
    filesystem::{
        tmpfs::{TmpFs, TmpfsQuota},
        vfs::VirtualFS,
    },
    memory::user_safe,
    object::misc::get_object_current_process,
    systemcall::utils::{SyscallError, SyscallImpl},
};

const SUBCMD_MASK: u32 = 0x00ff;
const SUBCMD_SHIFT: u32 = 8;
const USRQUOTA: u32 = 0;
const GRPQUOTA: u32 = 1;
const PRJQUOTA: u32 = 2;
const Q_SYNC: u32 = 0x800001;
const Q_GETFMT: u32 = 0x800004;
const Q_GETINFO: u32 = 0x800005;
const Q_SETINFO: u32 = 0x800006;
const Q_GETQUOTA: u32 = 0x800007;
const Q_SETQUOTA: u32 = 0x800008;
const Q_GETNEXTQUOTA: u32 = 0x800009;
const QIF_BLIMITS_B: u32 = 0;
const QIF_SPACE_B: u32 = 1;
const QIF_ILIMITS_B: u32 = 2;
const QIF_INODES_B: u32 = 3;
const QIF_BTIME_B: u32 = 4;
const QIF_ITIME_B: u32 = 5;
const QIF_BLIMITS: u32 = 1 << QIF_BLIMITS_B;
const QIF_SPACE: u32 = 1 << QIF_SPACE_B;
const QIF_ILIMITS: u32 = 1 << QIF_ILIMITS_B;
const QIF_INODES: u32 = 1 << QIF_INODES_B;
const QIF_BTIME: u32 = 1 << QIF_BTIME_B;
const QIF_ITIME: u32 = 1 << QIF_ITIME_B;
const QIF_LIMITS: u32 = QIF_BLIMITS | QIF_ILIMITS;
const QIF_USAGE: u32 = QIF_SPACE | QIF_INODES;
const QIF_TIMES: u32 = QIF_BTIME | QIF_ITIME;
const QIF_ALL: u32 = QIF_LIMITS | QIF_USAGE | QIF_TIMES;
const QFMT_VFS_V0: u32 = 2;

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct LinuxDqblk {
    dqb_bhardlimit: u64,
    dqb_bsoftlimit: u64,
    dqb_curspace: u64,
    dqb_ihardlimit: u64,
    dqb_isoftlimit: u64,
    dqb_curinodes: u64,
    dqb_btime: u64,
    dqb_itime: u64,
    dqb_valid: u32,
    dqb_padding: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct LinuxDqinfo {
    dqi_bgrace: u64,
    dqi_igrace: u64,
    dqi_flags: u32,
    dqi_valid: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct LinuxNextDqblk {
    dqb_bhardlimit: u64,
    dqb_bsoftlimit: u64,
    dqb_curspace: u64,
    dqb_ihardlimit: u64,
    dqb_isoftlimit: u64,
    dqb_curinodes: u64,
    dqb_btime: u64,
    dqb_itime: u64,
    dqb_valid: u32,
    dqb_padding: u32,
    dqb_id: u32,
    dqb_spare: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum QuotaSubcommand {
    Sync,
    GetFmt,
    GetInfo,
    SetInfo,
    GetQuota,
    SetQuota,
    GetNextQuota,
}

fn decode_quota_cmd(cmd: u32) -> Result<(QuotaSubcommand, u32), SyscallError> {
    let quota_type = cmd & SUBCMD_MASK;
    if !matches!(quota_type, USRQUOTA | GRPQUOTA | PRJQUOTA) {
        return Err(SyscallError::InvalidArguments);
    }

    let subcmd = match cmd >> SUBCMD_SHIFT {
        Q_SYNC => QuotaSubcommand::Sync,
        Q_GETFMT => QuotaSubcommand::GetFmt,
        Q_GETINFO => QuotaSubcommand::GetInfo,
        Q_SETINFO => QuotaSubcommand::SetInfo,
        Q_GETQUOTA => QuotaSubcommand::GetQuota,
        Q_SETQUOTA => QuotaSubcommand::SetQuota,
        Q_GETNEXTQUOTA => QuotaSubcommand::GetNextQuota,
        _ => return Err(SyscallError::InvalidArguments),
    };

    Ok((subcmd, quota_type))
}

fn tmpfs_from_fd(fd: u64) -> Result<FileSystemQuotaTarget, SyscallError> {
    let object = get_object_current_process(fd).map_err(SyscallError::from)?;
    let file_like = object.as_file_like()?;
    let (_, fs, _, _) = VirtualFS
        .lock()
        .mount_metadata(file_like.path())
        .map_err(SyscallError::from)?;
    if !fs.lock().as_any().is::<TmpFs>() {
        return Err(SyscallError::NoData);
    }
    Ok(FileSystemQuotaTarget { fs })
}

struct FileSystemQuotaTarget {
    fs: crate::filesystem::vfs::FileSystemRef,
}

impl FileSystemQuotaTarget {
    fn quota(&self, quota_type: u32, id: u32) -> Option<TmpfsQuota> {
        let guard = self.fs.lock();
        let tmpfs = guard.as_any().downcast_ref::<TmpFs>()?;
        tmpfs.quota(quota_type, id)
    }

    fn set_quota(&self, quota_type: u32, id: u32, quota: TmpfsQuota) -> bool {
        let guard = self.fs.lock();
        let Some(tmpfs) = guard.as_any().downcast_ref::<TmpFs>() else {
            return false;
        };
        tmpfs.set_quota(quota_type, id, quota)
    }
}

fn dqblk_from_tmpfs_quota(quota: TmpfsQuota) -> LinuxDqblk {
    LinuxDqblk {
        dqb_bhardlimit: quota.block_hardlimit,
        dqb_bsoftlimit: quota.block_softlimit,
        dqb_curspace: quota.current_space,
        dqb_ihardlimit: quota.inode_hardlimit,
        dqb_isoftlimit: quota.inode_softlimit,
        dqb_curinodes: quota.current_inodes,
        dqb_btime: quota.block_time,
        dqb_itime: quota.inode_time,
        dqb_valid: quota.valid,
        dqb_padding: 0,
    }
}

fn apply_dqblk_update(current: TmpfsQuota, update: LinuxDqblk) -> TmpfsQuota {
    let mut next = current;
    if (update.dqb_valid & QIF_BLIMITS) != 0 {
        next.block_hardlimit = update.dqb_bhardlimit;
        next.block_softlimit = update.dqb_bsoftlimit;
    }
    if (update.dqb_valid & QIF_ILIMITS) != 0 {
        next.inode_hardlimit = update.dqb_ihardlimit;
        next.inode_softlimit = update.dqb_isoftlimit;
    }
    if (update.dqb_valid & QIF_SPACE) != 0 {
        next.current_space = update.dqb_curspace;
    }
    if (update.dqb_valid & QIF_INODES) != 0 {
        next.current_inodes = update.dqb_curinodes;
    }
    if (update.dqb_valid & QIF_BTIME) != 0 {
        next.block_time = update.dqb_btime;
    }
    if (update.dqb_valid & QIF_ITIME) != 0 {
        next.inode_time = update.dqb_itime;
    }
    next.valid |= update.dqb_valid & QIF_ALL;
    next
}

define_syscall!(QuotactlFd, |fd: u64, cmd: u32, id: u32, addr: *mut u8| {
    let (subcmd, quota_type) = decode_quota_cmd(cmd)?;
    let tmpfs = tmpfs_from_fd(fd)?;

    match subcmd {
        QuotaSubcommand::Sync => Ok(0),
        QuotaSubcommand::GetFmt => {
            user_safe::write(addr.cast::<u32>(), &QFMT_VFS_V0)?;
            Ok(0)
        }
        QuotaSubcommand::GetInfo => {
            user_safe::write(addr.cast::<LinuxDqinfo>(), &LinuxDqinfo::default())?;
            Ok(0)
        }
        QuotaSubcommand::SetInfo => {
            let _ = user_safe::read(addr.cast_const().cast::<LinuxDqinfo>())?;
            Ok(0)
        }
        QuotaSubcommand::GetQuota => {
            let quota = tmpfs.quota(quota_type, id).ok_or(SyscallError::NoProcess)?;
            user_safe::write(addr.cast::<LinuxDqblk>(), &dqblk_from_tmpfs_quota(quota))?;
            Ok(0)
        }
        QuotaSubcommand::SetQuota => {
            let request = user_safe::read(addr.cast_const().cast::<LinuxDqblk>())?;
            let current = tmpfs.quota(quota_type, id).unwrap_or_default();
            let next = apply_dqblk_update(current, request);
            if !tmpfs.set_quota(quota_type, id, next) {
                return Err(SyscallError::InvalidArguments);
            }
            Ok(0)
        }
        QuotaSubcommand::GetNextQuota => {
            let quota = tmpfs.quota(quota_type, id).ok_or(SyscallError::NoProcess)?;
            let dqblk = dqblk_from_tmpfs_quota(quota);
            let next = LinuxNextDqblk {
                dqb_bhardlimit: dqblk.dqb_bhardlimit,
                dqb_bsoftlimit: dqblk.dqb_bsoftlimit,
                dqb_curspace: dqblk.dqb_curspace,
                dqb_ihardlimit: dqblk.dqb_ihardlimit,
                dqb_isoftlimit: dqblk.dqb_isoftlimit,
                dqb_curinodes: dqblk.dqb_curinodes,
                dqb_btime: dqblk.dqb_btime,
                dqb_itime: dqblk.dqb_itime,
                dqb_valid: dqblk.dqb_valid,
                dqb_padding: 0,
                dqb_id: id,
                dqb_spare: 0,
            };
            user_safe::write(addr.cast::<LinuxNextDqblk>(), &next)?;
            Ok(0)
        }
    }
});

#[cfg(test)]
mod tests {
    use crate::{
        filesystem::{path::Path, vfs::VirtualFS},
        systemcall::{
            implementations::{OpenAt, OpenFlags, QuotactlFd},
            test::{close_test_fd, expect_fd, write_user_cstr},
            test_helpers::{
                SyscallArgs, allocate_user_test_page, expect_errno, expect_ok, read_user_value,
                write_user_value,
            },
            utils::SyscallError,
        },
    };

    crate::test!(
        quotactl_fd_syscalls,
        "quotactl_fd syscalls follow linux rules",
        quotactl_fd_syscalls_follow_linux_rules
    );

    fn quotactl_fd_syscalls_follow_linux_rules() {
        const AT_FDCWD: u64 = (-100i32) as u64;
        const USRQUOTA: u64 = 0;
        const Q_GETFMT: u64 = 0x800004;
        const Q_GETINFO: u64 = 0x800005;
        const Q_GETQUOTA: u64 = 0x800007;
        const Q_SETQUOTA: u64 = 0x800008;
        const Q_GETNEXTQUOTA: u64 = 0x800009;
        const QIF_BLIMITS: u32 = 1 << 0;
        const QCMD_SHIFT: u64 = 8;

        #[repr(C)]
        #[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
        struct TestLinuxDqblk {
            dqb_bhardlimit: u64,
            dqb_bsoftlimit: u64,
            dqb_curspace: u64,
            dqb_ihardlimit: u64,
            dqb_isoftlimit: u64,
            dqb_curinodes: u64,
            dqb_btime: u64,
            dqb_itime: u64,
            dqb_valid: u32,
            dqb_padding: u32,
        }

        #[repr(C)]
        #[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
        struct TestLinuxDqinfo {
            dqi_bgrace: u64,
            dqi_igrace: u64,
            dqi_flags: u32,
            dqi_valid: u32,
        }

        #[repr(C)]
        #[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
        struct TestLinuxNextDqblk {
            dqb_bhardlimit: u64,
            dqb_bsoftlimit: u64,
            dqb_curspace: u64,
            dqb_ihardlimit: u64,
            dqb_isoftlimit: u64,
            dqb_curinodes: u64,
            dqb_btime: u64,
            dqb_itime: u64,
            dqb_valid: u32,
            dqb_padding: u32,
            dqb_id: u32,
            dqb_spare: u32,
        }

        let page = allocate_user_test_page();
        write_user_cstr(page, b"/tmp/syscall-quotactl-fd-test\0");
        write_user_cstr(page + 128, b"/\0");

        let _ = VirtualFS
            .lock()
            .delete_file(Path::new("/tmp/syscall-quotactl-fd-test"));
        VirtualFS
            .lock()
            .create_dir(Path::new("/tmp/syscall-quotactl-fd-test"))
            .unwrap();

        let tmpfd = expect_fd(
            SyscallArgs::new([AT_FDCWD, page, OpenFlags::DIRECTORY.bits() as u64, 0, 0, 0])
                .call::<OpenAt>(),
        );
        let rootfd = expect_fd(
            SyscallArgs::new([
                AT_FDCWD,
                page + 128,
                OpenFlags::DIRECTORY.bits() as u64,
                0,
                0,
                0,
            ])
            .call::<OpenAt>(),
        );

        let uid = 1000u64;
        let getquota_cmd = (Q_GETQUOTA << QCMD_SHIFT) | USRQUOTA;
        let setquota_cmd = (Q_SETQUOTA << QCMD_SHIFT) | USRQUOTA;
        let getfmt_cmd = (Q_GETFMT << QCMD_SHIFT) | USRQUOTA;
        let getinfo_cmd = (Q_GETINFO << QCMD_SHIFT) | USRQUOTA;
        let getnextquota_cmd = (Q_GETNEXTQUOTA << QCMD_SHIFT) | USRQUOTA;

        expect_errno(
            SyscallArgs::new([tmpfd as u64, getquota_cmd, uid, page + 256, 0, 0])
                .call::<QuotactlFd>(),
            SyscallError::NoProcess,
        );
        expect_errno(
            SyscallArgs::new([rootfd as u64, getquota_cmd, uid, page + 256, 0, 0])
                .call::<QuotactlFd>(),
            SyscallError::NoData,
        );

        write_user_value(
            page + 256,
            &TestLinuxDqblk {
                dqb_bhardlimit: 123,
                dqb_bsoftlimit: 77,
                dqb_valid: QIF_BLIMITS,
                ..Default::default()
            },
        );
        expect_ok(
            SyscallArgs::new([tmpfd as u64, setquota_cmd, uid, page + 256, 0, 0])
                .call::<QuotactlFd>(),
            0,
        );
        expect_ok(
            SyscallArgs::new([tmpfd as u64, getquota_cmd, uid, page + 320, 0, 0])
                .call::<QuotactlFd>(),
            0,
        );
        let dqblk = read_user_value::<TestLinuxDqblk>(page + 320);
        assert_eq!(dqblk.dqb_bhardlimit, 123);
        assert_eq!(dqblk.dqb_bsoftlimit, 77);
        assert_eq!(dqblk.dqb_valid & QIF_BLIMITS, QIF_BLIMITS);

        expect_ok(
            SyscallArgs::new([tmpfd as u64, getfmt_cmd, 0, page + 400, 0, 0]).call::<QuotactlFd>(),
            0,
        );
        assert_eq!(read_user_value::<u32>(page + 400), 2);

        expect_ok(
            SyscallArgs::new([tmpfd as u64, getinfo_cmd, 0, page + 448, 0, 0]).call::<QuotactlFd>(),
            0,
        );
        assert_eq!(
            read_user_value::<TestLinuxDqinfo>(page + 448),
            TestLinuxDqinfo::default()
        );

        expect_ok(
            SyscallArgs::new([tmpfd as u64, getnextquota_cmd, uid, page + 512, 0, 0])
                .call::<QuotactlFd>(),
            0,
        );
        let next = read_user_value::<TestLinuxNextDqblk>(page + 512);
        assert_eq!(next.dqb_id as u64, uid);
        assert_eq!(next.dqb_bhardlimit, 123);
        assert_eq!(next.dqb_bsoftlimit, 77);

        expect_errno(
            SyscallArgs::new([tmpfd as u64, getfmt_cmd, 0, 0, 0, 0]).call::<QuotactlFd>(),
            SyscallError::BadAddress,
        );
        expect_errno(
            SyscallArgs::new([tmpfd as u64, 0xdead_beef, uid, page + 256, 0, 0])
                .call::<QuotactlFd>(),
            SyscallError::InvalidArguments,
        );

        close_test_fd(rootfd);
        close_test_fd(tmpfd);
        let _ = VirtualFS
            .lock()
            .delete_file(Path::new("/tmp/syscall-quotactl-fd-test"));
    }
}
