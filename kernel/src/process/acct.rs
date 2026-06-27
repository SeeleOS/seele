use alloc::{string::String, sync::Arc};
use core::mem;

use lazy_static::lazy_static;

use crate::{
    filesystem::{object::FileLikeObject, path::Path, vfs_operations::open_path},
    memory::utils::Mut,
    misc::time::Time,
    object::traits::Statable,
    process::{Process, ProcessExitStatus},
    systemcall::utils::SyscallError,
};

lazy_static! {
    static ref ACCOUNTING_FILE: Mut<Option<Arc<FileLikeObject>>> = Mut::new(None);
}

#[repr(C)]
#[derive(Clone, Copy)]
struct LinuxAcctV3 {
    ac_flag: u8,
    ac_version: u8,
    ac_tty: u16,
    ac_exitcode: u32,
    ac_uid: u32,
    ac_gid: u32,
    ac_pid: u32,
    ac_ppid: u32,
    ac_btime: u32,
    ac_etime: f32,
    ac_utime: u16,
    ac_stime: u16,
    ac_mem: u16,
    ac_io: u16,
    ac_rw: u16,
    ac_minflt: u16,
    ac_majflt: u16,
    ac_swaps: u16,
    ac_comm: [u8; 16],
}

pub fn set_accounting_file(path: Option<String>) -> Result<(), SyscallError> {
    let Some(path) = path else {
        *ACCOUNTING_FILE.lock() = None;
        return Ok(());
    };

    let file = open_path(Path::new(&path))
        .map_err(SyscallError::from)
        .and_then(|file| {
            if file.stat().st_mode & 0o170000 != 0o100000 {
                Err(SyscallError::AccessDenied)
            } else {
                Ok(file)
            }
        })?;

    *ACCOUNTING_FILE.lock() = Some(Arc::new(file));
    Ok(())
}

pub fn write_process_accounting_record(process: &Process, exit_status: ProcessExitStatus) {
    let Some(file) = ACCOUNTING_FILE.lock().clone() else {
        return;
    };

    let mut comm = [0u8; 16];
    if let Some(name) = process.command_line.first() {
        let basename = name.rsplit('/').next().unwrap_or(name);
        for (dst, src) in comm.iter_mut().zip(basename.as_bytes()) {
            *dst = *src;
        }
    }

    let record = LinuxAcctV3 {
        ac_flag: 0,
        ac_version: 3,
        ac_tty: 0,
        ac_exitcode: exit_status.wait_status() as u32,
        ac_uid: process.real_uid,
        ac_gid: process.real_gid,
        ac_pid: process.pid.0 as u32,
        ac_ppid: process
            .parent
            .as_ref()
            .map(|parent| parent.lock().pid.0 as u32)
            .unwrap_or(0),
        ac_btime: Time::current().as_seconds() as u32,
        ac_etime: 0.0,
        ac_utime: 0,
        ac_stime: 0,
        ac_mem: 0,
        ac_io: 0,
        ac_rw: 0,
        ac_minflt: 0,
        ac_majflt: 0,
        ac_swaps: 0,
        ac_comm: comm,
    };
    let bytes = unsafe {
        core::slice::from_raw_parts(
            (&record as *const LinuxAcctV3).cast::<u8>(),
            mem::size_of::<LinuxAcctV3>(),
        )
    };
    let offset = u64::try_from(file.stat().st_size).unwrap_or(0);
    let _ = file.write_at(bytes, offset);
}
