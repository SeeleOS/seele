use core::any::Any;

use alloc::{
    collections::{BTreeMap, BTreeSet},
    format,
    string::{String, ToString},
    sync::Arc,
    vec::Vec,
};
use lazy_static::lazy_static;
use spin::Mutex;

use crate::{
    filesystem::{
        errors::FSError,
        info::{DirectoryContentInfo, FileLikeInfo, UnixPermission},
        path::{Path, PathPart},
        vfs::FSResult,
        vfs_traits::{
            Directory, DirectoryContentType, File, FileLike, FileLikeType, FileSystem, Whence,
        },
    },
    process::{manager::MANAGER, misc::ProcessID},
};

const ROOT_INODE: u64 = 0x6000_0000;
const DEFAULT_DIR_MODE: u32 = 0o040755;
const READONLY_FILE_MODE: u32 = 0o100444;
const WRITABLE_FILE_MODE: u32 = 0o100644;

#[derive(Clone)]
struct CgroupDirectory {
    inode: u64,
    children: BTreeSet<String>,
    subtree_control: String,
    cpu_max: String,
    memory_oom_group: bool,
    memory_min: String,
    memory_low: String,
    memory_high: String,
    memory_max: String,
    memory_swap_max: String,
    pids_max: String,
}

struct CgroupState {
    next_inode: u64,
    directories: BTreeMap<String, CgroupDirectory>,
    pid_paths: BTreeMap<u64, String>,
}

impl CgroupState {
    fn new() -> Self {
        let mut directories = BTreeMap::new();
        directories.insert(
            "/".into(),
            CgroupDirectory {
                inode: ROOT_INODE,
                children: BTreeSet::new(),
                subtree_control: String::new(),
                cpu_max: String::from("max 100000"),
                memory_oom_group: false,
                memory_min: String::from("0"),
                memory_low: String::from("0"),
                memory_high: String::from("max"),
                memory_max: String::from("max"),
                memory_swap_max: String::from("max"),
                pids_max: String::from("max"),
            },
        );

        Self {
            next_inode: ROOT_INODE + 1,
            directories,
            pid_paths: BTreeMap::new(),
        }
    }

    fn normalize_dir_path(path: &str) -> String {
        if path.is_empty() || path == "/" {
            "/".into()
        } else {
            Path::new(path).normalize().as_string()
        }
    }

    fn child_path(parent: &str, name: &str) -> String {
        if parent == "/" {
            format!("/{name}")
        } else {
            format!("{parent}/{name}")
        }
    }

    fn directory(&self, path: &str) -> FSResult<&CgroupDirectory> {
        self.directories.get(path).ok_or(FSError::NotFound)
    }

    fn directory_mut(&mut self, path: &str) -> FSResult<&mut CgroupDirectory> {
        self.directories.get_mut(path).ok_or(FSError::NotFound)
    }

    fn create_directory(&mut self, parent: &str, name: &str) -> FSResult<()> {
        let parent = Self::normalize_dir_path(parent);
        let child_path = Self::child_path(&parent, name);

        if self.directories.contains_key(&child_path) {
            return Err(FSError::AlreadyExists);
        }

        self.directory(&parent)?;

        let inode = self.next_inode;
        self.next_inode += 1;
        self.directories.insert(
            child_path,
            CgroupDirectory {
                inode,
                children: BTreeSet::new(),
                subtree_control: String::new(),
                cpu_max: String::from("max 100000"),
                memory_oom_group: false,
                memory_min: String::from("0"),
                memory_low: String::from("0"),
                memory_high: String::from("max"),
                memory_max: String::from("max"),
                memory_swap_max: String::from("max"),
                pids_max: String::from("max"),
            },
        );
        self.directory_mut(&parent)?.children.insert(name.into());
        Ok(())
    }

    fn remove_directory(&mut self, parent: &str, name: &str) -> FSResult<()> {
        let parent = Self::normalize_dir_path(parent);
        let child_path = Self::child_path(&parent, name);
        self.prune_dead_pid_paths();
        let Some(directory) = self.directories.get(&child_path) else {
            return Err(FSError::NotFound);
        };
        if !directory.children.is_empty() {
            return Err(FSError::DirectoryNotEmpty);
        }
        if self
            .pid_paths
            .values()
            .any(|path| Self::normalize_dir_path(path) == child_path)
        {
            return Err(FSError::Busy);
        }

        self.directories.remove(&child_path);
        self.directory_mut(&parent)?.children.remove(name);
        Ok(())
    }

    fn pid_path(&self, pid: ProcessID) -> String {
        self.pid_paths
            .get(&pid.0)
            .cloned()
            .unwrap_or_else(|| "/".into())
    }

    fn set_pid_path(&mut self, pid: ProcessID, path: &str) -> FSResult<()> {
        let path = Self::normalize_dir_path(path);
        self.directory(&path)?;
        self.pid_paths.insert(pid.0, path);
        Ok(())
    }

    fn remove_pid_path(&mut self, pid: ProcessID) {
        self.pid_paths.remove(&pid.0);
    }

    fn prune_dead_pid_paths(&mut self) {
        self.pid_paths
            .retain(|pid, _| MANAGER.lock().processes.contains_key(&ProcessID(*pid)));
    }

    fn pids_in_path(&self, path: &str) -> Vec<ProcessID> {
        let path = Self::normalize_dir_path(path);
        MANAGER
            .lock()
            .processes
            .keys()
            .copied()
            .filter(|pid| self.pid_path(*pid) == path)
            .collect()
    }
}

lazy_static! {
    static ref CGROUP_STATE: Mutex<CgroupState> = Mutex::new(CgroupState::new());
}

#[derive(Clone, Copy)]
enum CgroupFileKind {
    Procs,
    Threads,
    Controllers,
    SubtreeControl,
    Events,
    Kill,
    Freeze,
    Type,
    CpuMax,
    CpuStat,
    MemoryCurrent,
    MemoryMin,
    MemoryLow,
    MemoryHigh,
    MemoryMax,
    MemorySwapMax,
    MemoryOomGroup,
    MemoryReclaim,
    PidsMax,
}

impl CgroupFileKind {
    fn name(self) -> &'static str {
        match self {
            Self::Procs => "cgroup.procs",
            Self::Threads => "cgroup.threads",
            Self::Controllers => "cgroup.controllers",
            Self::SubtreeControl => "cgroup.subtree_control",
            Self::Events => "cgroup.events",
            Self::Kill => "cgroup.kill",
            Self::Freeze => "cgroup.freeze",
            Self::Type => "cgroup.type",
            Self::CpuMax => "cpu.max",
            Self::CpuStat => "cpu.stat",
            Self::MemoryCurrent => "memory.current",
            Self::MemoryMin => "memory.min",
            Self::MemoryLow => "memory.low",
            Self::MemoryHigh => "memory.high",
            Self::MemoryMax => "memory.max",
            Self::MemorySwapMax => "memory.swap.max",
            Self::MemoryOomGroup => "memory.oom.group",
            Self::MemoryReclaim => "memory.reclaim",
            Self::PidsMax => "pids.max",
        }
    }

    fn inode_offset(self) -> u64 {
        match self {
            Self::Procs => 1,
            Self::Threads => 2,
            Self::Controllers => 3,
            Self::SubtreeControl => 4,
            Self::Events => 5,
            Self::Kill => 6,
            Self::Freeze => 7,
            Self::Type => 8,
            Self::CpuMax => 9,
            Self::CpuStat => 10,
            Self::MemoryCurrent => 11,
            Self::MemoryMin => 12,
            Self::MemoryLow => 13,
            Self::MemoryHigh => 14,
            Self::MemoryMax => 15,
            Self::MemorySwapMax => 16,
            Self::MemoryOomGroup => 17,
            Self::MemoryReclaim => 18,
            Self::PidsMax => 19,
        }
    }

    fn mode(self) -> u32 {
        match self {
            Self::Controllers | Self::Events | Self::Type | Self::CpuStat | Self::MemoryCurrent => {
                READONLY_FILE_MODE
            }
            Self::Procs
            | Self::Threads
            | Self::SubtreeControl
            | Self::Kill
            | Self::Freeze
            | Self::CpuMax
            | Self::MemoryMin
            | Self::MemoryLow
            | Self::MemoryHigh
            | Self::MemoryMax
            | Self::MemorySwapMax
            | Self::MemoryOomGroup
            | Self::MemoryReclaim
            | Self::PidsMax => WRITABLE_FILE_MODE,
        }
    }

    fn all() -> &'static [Self] {
        &[
            Self::Procs,
            Self::Threads,
            Self::Controllers,
            Self::SubtreeControl,
            Self::Events,
            Self::Kill,
            Self::Freeze,
            Self::Type,
            Self::CpuMax,
            Self::CpuStat,
            Self::MemoryCurrent,
            Self::MemoryMin,
            Self::MemoryLow,
            Self::MemoryHigh,
            Self::MemoryMax,
            Self::MemorySwapMax,
            Self::MemoryOomGroup,
            Self::MemoryReclaim,
            Self::PidsMax,
        ]
    }

    fn from_name(name: &str) -> Option<Self> {
        Self::all().iter().copied().find(|kind| kind.name() == name)
    }
}

fn relative_components(path: &Path) -> Vec<String> {
    path.normalize()
        .parts
        .iter()
        .filter_map(|part| match part {
            PathPart::Normal(name) => Some(name.clone()),
            _ => None,
        })
        .collect()
}

fn absolute_cgroup_path(path: &Path) -> String {
    let parts = relative_components(path);
    if parts.is_empty() {
        "/".into()
    } else {
        format!("/{}", parts.join("/"))
    }
}

fn file_info(path: &str, kind: CgroupFileKind) -> FSResult<FileLikeInfo> {
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

fn file_contents(state: &CgroupState, path: &str, kind: CgroupFileKind) -> FSResult<Vec<u8>> {
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

fn write_file(path: &str, kind: CgroupFileKind, buffer: &[u8]) -> FSResult<usize> {
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

struct CgroupDirectoryHandle {
    path: String,
}

impl CgroupDirectoryHandle {
    fn new(path: String) -> Self {
        Self { path }
    }
}

impl Directory for CgroupDirectoryHandle {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn info(&self) -> FSResult<FileLikeInfo> {
        let state = CGROUP_STATE.lock();
        let dir = state.directory(&self.path)?;
        let name = if self.path == "/" {
            "cgroup".into()
        } else {
            self.path
                .rsplit('/')
                .next()
                .filter(|name| !name.is_empty())
                .unwrap_or("cgroup")
                .into()
        };
        Ok(FileLikeInfo::new(
            name,
            0,
            UnixPermission(DEFAULT_DIR_MODE),
            FileLikeType::Directory,
        )
        .with_inode(dir.inode))
    }

    fn name(&self) -> FSResult<String> {
        Ok(if self.path == "/" {
            "cgroup".into()
        } else {
            self.path
                .rsplit('/')
                .next()
                .filter(|name| !name.is_empty())
                .unwrap_or("cgroup")
                .into()
        })
    }

    fn contents(&self) -> FSResult<Vec<DirectoryContentInfo>> {
        let state = CGROUP_STATE.lock();
        let dir = state.directory(&self.path)?;
        let mut entries = Vec::new();
        for kind in CgroupFileKind::all() {
            entries.push(DirectoryContentInfo::new(
                kind.name().into(),
                DirectoryContentType::File,
            ));
        }
        for child in &dir.children {
            entries.push(DirectoryContentInfo::new(
                child.clone(),
                DirectoryContentType::Directory,
            ));
        }
        Ok(entries)
    }

    fn create(&self, info: DirectoryContentInfo) -> FSResult<()> {
        if !matches!(info.content_type, DirectoryContentType::Directory) {
            return Err(FSError::Readonly);
        }
        CGROUP_STATE.lock().create_directory(&self.path, &info.name)
    }

    fn delete(&self, name: &str) -> FSResult<()> {
        CGROUP_STATE.lock().remove_directory(&self.path, name)
    }

    fn get(&self, name: &str) -> FSResult<FileLike> {
        let child_path = CgroupState::child_path(&self.path, name);
        if CGROUP_STATE.lock().directories.contains_key(&child_path) {
            return Ok(FileLike::Directory(Arc::new(Mutex::new(
                CgroupDirectoryHandle::new(child_path),
            ))));
        }

        if let Some(kind) = CgroupFileKind::from_name(name) {
            return Ok(FileLike::File(Arc::new(Mutex::new(CgroupFileHandle::new(
                self.path.clone(),
                kind,
            )))));
        }

        Err(FSError::NotFound)
    }

    fn chmod(&self, _mode: u32) -> FSResult<()> {
        Ok(())
    }
}

struct CgroupFileHandle {
    path: String,
    kind: CgroupFileKind,
    offset: usize,
}

impl CgroupFileHandle {
    fn new(path: String, kind: CgroupFileKind) -> Self {
        Self {
            path,
            kind,
            offset: 0,
        }
    }
}

impl File for CgroupFileHandle {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn info(&mut self) -> FSResult<FileLikeInfo> {
        file_info(&self.path, self.kind)
    }

    fn read_at(&mut self, buffer: &mut [u8], offset: u64) -> FSResult<usize> {
        let state = CGROUP_STATE.lock();
        let data = file_contents(&state, &self.path, self.kind)?;
        let offset = offset as usize;
        if offset >= data.len() {
            return Ok(0);
        }

        let len = buffer.len().min(data.len() - offset);
        buffer[..len].copy_from_slice(&data[offset..offset + len]);
        Ok(len)
    }

    fn read(&mut self, buffer: &mut [u8]) -> FSResult<usize> {
        let read = self.read_at(buffer, self.offset as u64)?;
        self.offset += read;
        Ok(read)
    }

    fn write(&mut self, buffer: &[u8]) -> FSResult<usize> {
        let written = write_file(&self.path, self.kind, buffer)?;
        self.offset += written;
        Ok(written)
    }

    fn truncate(&mut self, _length: u64) -> FSResult<()> {
        match self.kind {
            CgroupFileKind::Controllers
            | CgroupFileKind::Events
            | CgroupFileKind::Type
            | CgroupFileKind::CpuStat
            | CgroupFileKind::MemoryCurrent => Err(FSError::Readonly),
            CgroupFileKind::Procs
            | CgroupFileKind::Threads
            | CgroupFileKind::SubtreeControl
            | CgroupFileKind::Kill
            | CgroupFileKind::Freeze
            | CgroupFileKind::CpuMax
            | CgroupFileKind::MemoryMin
            | CgroupFileKind::MemoryLow
            | CgroupFileKind::MemoryHigh
            | CgroupFileKind::MemoryMax
            | CgroupFileKind::MemorySwapMax
            | CgroupFileKind::MemoryOomGroup
            | CgroupFileKind::MemoryReclaim
            | CgroupFileKind::PidsMax => Ok(()),
        }
    }

    fn seek(&mut self, offset: i64, seek_type: Whence) -> FSResult<usize> {
        let len = file_contents(&CGROUP_STATE.lock(), &self.path, self.kind)?.len() as i64;
        let next = match seek_type {
            Whence::Start => offset,
            Whence::Current => self.offset as i64 + offset,
            Whence::End => len + offset,
            Whence::Data => {
                if offset < 0 || offset >= len {
                    return Err(FSError::Other);
                }
                offset
            }
            Whence::Hole => {
                if offset < 0 || offset > len {
                    return Err(FSError::Other);
                }
                len
            }
        }
        .max(0) as usize;
        self.offset = next;
        Ok(self.offset)
    }

    fn chmod(&self, _mode: u32) -> FSResult<()> {
        Ok(())
    }
}

pub struct CgroupFs;

impl CgroupFs {
    pub fn new() -> Self {
        Self
    }
}

impl Default for CgroupFs {
    fn default() -> Self {
        Self::new()
    }
}

impl FileSystem for CgroupFs {
    fn init(&mut self) -> FSResult<()> {
        Ok(())
    }

    fn lookup(&self, path: &Path) -> FSResult<FileLike> {
        let absolute = absolute_cgroup_path(path);
        if absolute == "/" {
            return Ok(FileLike::Directory(Arc::new(Mutex::new(
                CgroupDirectoryHandle::new("/".into()),
            ))));
        }

        let parent = Path::new(&absolute)
            .parent()
            .unwrap_or_default()
            .as_string();
        let name = Path::new(&absolute)
            .file_name()
            .ok_or(FSError::NotFound)?
            .to_string();

        if CGROUP_STATE.lock().directories.contains_key(&absolute) {
            return Ok(FileLike::Directory(Arc::new(Mutex::new(
                CgroupDirectoryHandle::new(absolute),
            ))));
        }

        let kind = CgroupFileKind::from_name(&name).ok_or(FSError::NotFound)?;
        CGROUP_STATE.lock().directory(&parent)?;
        Ok(FileLike::File(Arc::new(Mutex::new(CgroupFileHandle::new(
            parent, kind,
        )))))
    }

    fn rename(&self, _old_path: &Path, _new_path: &Path) -> FSResult<()> {
        Err(FSError::Readonly)
    }

    fn link(&self, _old_path: &Path, _new_path: &Path) -> FSResult<()> {
        Err(FSError::Readonly)
    }

    fn name(&self) -> &'static str {
        "cgroup2"
    }

    fn magic(&self) -> i64 {
        0x6367_7270
    }

    fn mount_source(&self) -> &'static str {
        "cgroup2"
    }

    fn default_mount_flags(&self, _path: &Path) -> crate::filesystem::vfs_traits::MountFlags {
        crate::filesystem::vfs_traits::MountFlags::MS_NOSUID
            | crate::filesystem::vfs_traits::MountFlags::MS_NODEV
            | crate::filesystem::vfs_traits::MountFlags::MS_NOEXEC
            | crate::filesystem::vfs_traits::MountFlags::MS_RELATIME
    }
}

pub fn pid_cgroup_path(pid: ProcessID) -> String {
    CGROUP_STATE.lock().pid_path(pid)
}

pub fn set_pid_cgroup_path_from_fs_path(pid: ProcessID, path: &Path) -> FSResult<()> {
    let normalized = path.normalize().as_string();
    let cgroup_path = if normalized == "/sys/fs/cgroup" {
        "/".into()
    } else if let Some(relative) = normalized.strip_prefix("/sys/fs/cgroup/") {
        format!("/{}", relative.trim_start_matches('/'))
    } else {
        return Err(FSError::NotFound);
    };

    CGROUP_STATE.lock().set_pid_path(pid, &cgroup_path)
}

pub fn remove_pid_cgroup_path(pid: ProcessID) {
    CGROUP_STATE.lock().remove_pid_path(pid);
}
