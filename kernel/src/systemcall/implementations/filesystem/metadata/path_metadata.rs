use super::*;

#[derive(Clone)]
pub(in crate::systemcall::implementations::filesystem) struct PathLookup {
    pub(in crate::systemcall::implementations::filesystem) stat: LinuxStat,
    pub(in crate::systemcall::implementations::filesystem) mount_id: u64,
    pub(in crate::systemcall::implementations::filesystem) mount_root: bool,
}

#[derive(Clone, Copy)]
pub(in crate::systemcall::implementations::filesystem) struct PathLookupPhases {
    pub(in crate::systemcall::implementations::filesystem) resolve: HotSyscallPhase,
    pub(in crate::systemcall::implementations::filesystem) empty_path: HotSyscallPhase,
    pub(in crate::systemcall::implementations::filesystem) resolve_final: HotSyscallPhase,
    pub(in crate::systemcall::implementations::filesystem) build_stat: HotSyscallPhase,
    pub(in crate::systemcall::implementations::filesystem) mount_info: HotSyscallPhase,
}

pub(in crate::systemcall::implementations::filesystem) fn linux_stat_from_file_like_info(
    info: FileLikeInfo,
    path: &Path,
) -> LinuxStat {
    let rdev = info.rdev;
    let mut stat = info.as_linux();
    stat.st_dev = mount_device_id_for_path(path);
    stat.st_rdev = rdev;
    stat
}

pub(in crate::systemcall::implementations::filesystem) fn mount_info_from_object(
    object: &ObjectRef,
) -> Result<(u64, bool), SyscallError> {
    Ok((mount_id_for_object(object)?, mount_root_for_object(object)?))
}

pub(in crate::systemcall::implementations::filesystem) fn lookup_path_metadata(
    dirfd: i32,
    path_str: &str,
    nofollow: bool,
    allow_empty_path: bool,
    phases: PathLookupPhases,
) -> Result<PathLookup, SyscallError> {
    if path_str.is_empty() && allow_empty_path {
        let empty_path_start = profile::scope_start();
        let object = get_object_current_process(dirfd as u64).map_err(SyscallError::from)?;
        profile::record_hot_syscall_phase(
            phases.empty_path,
            profile::scope_start().saturating_sub(empty_path_start),
        );

        let build_stat_start = profile::scope_start();
        let stat = object.clone().as_statable()?.stat();
        profile::record_hot_syscall_phase(
            phases.build_stat,
            profile::scope_start().saturating_sub(build_stat_start),
        );

        let mount_info_start = profile::scope_start();
        let (mount_id, mount_root) = mount_info_from_object(&object)?;
        profile::record_hot_syscall_phase(
            phases.mount_info,
            profile::scope_start().saturating_sub(mount_info_start),
        );
        return Ok(PathLookup {
            stat,
            mount_id,
            mount_root,
        });
    }

    let resolve_start = profile::scope_start();
    let normalized_path = resolve_path_at(dirfd, path_str)?.normalize();
    profile::record_hot_syscall_phase(
        phases.resolve,
        profile::scope_start().saturating_sub(resolve_start),
    );

    let resolve_final_start = profile::scope_start();
    let (info, resolved_path, mount_id, mount_root) =
        resolve_path_with_mount_info(normalized_path, !nofollow)?;
    profile::record_hot_syscall_phase(
        phases.resolve_final,
        profile::scope_start().saturating_sub(resolve_final_start),
    );

    let build_stat_start = profile::scope_start();
    let stat = linux_stat_from_file_like_info(info, &resolved_path);
    profile::record_hot_syscall_phase(
        phases.build_stat,
        profile::scope_start().saturating_sub(build_stat_start),
    );

    Ok(PathLookup {
        stat,
        mount_id,
        mount_root,
    })
}
