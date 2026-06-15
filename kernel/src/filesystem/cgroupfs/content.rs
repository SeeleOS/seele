use super::*;

pub(super) fn relative_components(path: &Path) -> Vec<String> {
    path.normalize()
        .parts
        .iter()
        .filter_map(|part| match part {
            PathPart::Normal(name) => Some(name.clone()),
            _ => None,
        })
        .collect()
}

pub(super) fn absolute_cgroup_path(path: &Path) -> String {
    let parts = relative_components(path);
    if parts.is_empty() {
        "/".into()
    } else {
        format!("/{}", parts.join("/"))
    }
}

pub(super) fn file_info(path: &str, kind: CgroupFileKind) -> FSResult<FileLikeInfo> {
    let state = CGROUP_STATE.lock();
    let dir = state.directory(path)?;
    let data = file_contents(&state, path, kind)?;
    Ok(FileLikeInfo::new(
        kind.name().into(),
        data.len(),
        UnixPermission(kind.mode()),
        FileLikeType::File,
    )
    .with_inode(dir.inode * 32 + kind.inode_offset()))
}

pub(super) fn file_contents(
    state: &CgroupState,
    path: &str,
    kind: CgroupFileKind,
) -> FSResult<Vec<u8>> {
    let dir = state.directory(path)?;
    let bytes = match kind {
        CgroupFileKind::Procs => {
            let pids = state.pids_in_path(path);
            let mut content = String::new();
            for pid in pids {
                content.push_str(&format!("{}\n", pid.0));
            }
            content.into_bytes()
        }
        CgroupFileKind::Threads => {
            let pids = state.pids_in_path(path);
            let mut content = String::new();
            for pid in pids {
                content.push_str(&format!("{}\n", pid.0));
            }
            content.into_bytes()
        }
        CgroupFileKind::Controllers => b"cpu memory pids\n".to_vec(),
        CgroupFileKind::SubtreeControl => {
            if dir.subtree_control.is_empty() {
                b"\n".to_vec()
            } else {
                format!("{}\n", dir.subtree_control).into_bytes()
            }
        }
        CgroupFileKind::Events => {
            let populated = if state.pids_in_path(path).is_empty() {
                0
            } else {
                1
            };
            format!("populated {populated}\nfrozen 0\n").into_bytes()
        }
        CgroupFileKind::Kill => Vec::new(),
        CgroupFileKind::Freeze => b"0\n".to_vec(),
        CgroupFileKind::Type => b"domain\n".to_vec(),
        CgroupFileKind::CpuMax => format!("{}\n", dir.cpu_max).into_bytes(),
        CgroupFileKind::CpuStat => b"usage_usec 0\nuser_usec 0\nsystem_usec 0\n".to_vec(),
        CgroupFileKind::MemoryCurrent => b"0\n".to_vec(),
        CgroupFileKind::MemoryMin => format!("{}\n", dir.memory_min).into_bytes(),
        CgroupFileKind::MemoryLow => format!("{}\n", dir.memory_low).into_bytes(),
        CgroupFileKind::MemoryHigh => format!("{}\n", dir.memory_high).into_bytes(),
        CgroupFileKind::MemoryMax => format!("{}\n", dir.memory_max).into_bytes(),
        CgroupFileKind::MemorySwapMax => format!("{}\n", dir.memory_swap_max).into_bytes(),
        CgroupFileKind::MemoryOomGroup => {
            if dir.memory_oom_group {
                b"1\n".to_vec()
            } else {
                b"0\n".to_vec()
            }
        }
        CgroupFileKind::MemoryReclaim => Vec::new(),
        CgroupFileKind::PidsMax => format!("{}\n", dir.pids_max).into_bytes(),
    };
    Ok(bytes)
}

fn normalize_cgroup_limit_write(buffer: &[u8]) -> FSResult<String> {
    let text = core::str::from_utf8(buffer).map_err(|_| FSError::Other)?;
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Err(FSError::Other);
    }
    Ok(trimmed.to_string())
}

pub(super) fn write_file(path: &str, kind: CgroupFileKind, buffer: &[u8]) -> FSResult<usize> {
    let mut state = CGROUP_STATE.lock();
    state.directory(path)?;

    match kind {
        CgroupFileKind::Procs => {
            let text = core::str::from_utf8(buffer).map_err(|_| FSError::Other)?;
            let pid = text
                .trim()
                .parse::<u64>()
                .map(ProcessID)
                .map_err(|_| FSError::Other)?;
            state.set_pid_path(pid, path)?;
        }
        CgroupFileKind::Threads => {
            let text = core::str::from_utf8(buffer).map_err(|_| FSError::Other)?;
            let pid = text
                .trim()
                .parse::<u64>()
                .map(ProcessID)
                .map_err(|_| FSError::Other)?;
            state.set_pid_path(pid, path)?;
        }
        CgroupFileKind::SubtreeControl => {
            let text = core::str::from_utf8(buffer).map_err(|_| FSError::Other)?;
            let mut enabled = state
                .directory(path)?
                .subtree_control
                .split_whitespace()
                .map(ToString::to_string)
                .collect::<BTreeSet<_>>();
            for token in text.split_whitespace() {
                if let Some(controller) = token.strip_prefix('+') {
                    enabled.insert(controller.to_string());
                } else if let Some(controller) = token.strip_prefix('-') {
                    enabled.remove(controller);
                } else if !token.is_empty() {
                    enabled.insert(token.to_string());
                }
            }
            state.directory_mut(path)?.subtree_control =
                enabled.into_iter().collect::<Vec<_>>().join(" ");
        }
        CgroupFileKind::MemoryOomGroup => {
            let text = core::str::from_utf8(buffer).map_err(|_| FSError::Other)?;
            let value = match text.trim() {
                "0" => false,
                "1" => true,
                _ => return Err(FSError::Other),
            };
            state.directory_mut(path)?.memory_oom_group = value;
        }
        CgroupFileKind::CpuMax => {
            let value = normalize_cgroup_limit_write(buffer)?;
            state.directory_mut(path)?.cpu_max = value;
        }
        CgroupFileKind::MemoryMin => {
            let value = normalize_cgroup_limit_write(buffer)?;
            state.directory_mut(path)?.memory_min = value;
        }
        CgroupFileKind::MemoryLow => {
            let value = normalize_cgroup_limit_write(buffer)?;
            state.directory_mut(path)?.memory_low = value;
        }
        CgroupFileKind::MemoryHigh => {
            let value = normalize_cgroup_limit_write(buffer)?;
            state.directory_mut(path)?.memory_high = value;
        }
        CgroupFileKind::MemoryMax => {
            let value = normalize_cgroup_limit_write(buffer)?;
            state.directory_mut(path)?.memory_max = value;
        }
        CgroupFileKind::MemorySwapMax => {
            let value = normalize_cgroup_limit_write(buffer)?;
            state.directory_mut(path)?.memory_swap_max = value;
        }
        CgroupFileKind::PidsMax => {
            let value = normalize_cgroup_limit_write(buffer)?;
            state.directory_mut(path)?.pids_max = value;
        }
        CgroupFileKind::Kill | CgroupFileKind::Freeze | CgroupFileKind::MemoryReclaim => {}
        CgroupFileKind::Controllers
        | CgroupFileKind::Events
        | CgroupFileKind::Type
        | CgroupFileKind::CpuStat
        | CgroupFileKind::MemoryCurrent => {
            return Err(FSError::Readonly);
        }
    }

    Ok(buffer.len())
}
