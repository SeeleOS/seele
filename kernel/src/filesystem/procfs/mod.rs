use core::any::Any;

use alloc::{format, string::String, vec, vec::Vec};

use crate::{
    filesystem::{
        errors::FSError,
        info::{DirectoryContentInfo, FileLikeInfo, UnixPermission},
        path::{Path, PathPart},
        vfs::FSResult,
        vfs_traits::{
            Directory, DirectoryContentType, File, FileLike, FileLikeType, FileSystem, Symlink,
            Whence,
        },
    },
    process::{
        manager::{MANAGER, get_current_process},
        misc::{ProcessID, get_process_with_pid},
    },
};
use alloc::sync::Arc;
use spin::Mutex;

const PROC_ROOT_INODE: u64 = 0x3000;
const PROC_CMDLINE_INODE: u64 = 0x3001;
const PROC_SELF_INODE: u64 = 0x3002;

pub struct ProcFs;

impl ProcFs {
    pub fn new() -> Self {
        Self
    }
}

impl FileSystem for ProcFs {
    fn init(&mut self) -> FSResult<()> {
        Ok(())
    }

    fn lookup(&self, path: &Path) -> FSResult<FileLike> {
        let normalized = path.normalize();
        let parts = normalized
            .parts
            .iter()
            .filter_map(|part| match part {
                PathPart::Normal(name) => Some(name.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>();

        match parts.as_slice() {
            [] => Ok(proc_dir("/", PROC_ROOT_INODE, proc_root_entries())),
            ["cmdline"] => Ok(proc_file(
                "cmdline",
                PROC_CMDLINE_INODE,
                proc_kernel_cmdline_bytes,
            )),
            ["self"] => {
                let pid = current_pid()?;
                Ok(proc_symlink("self", PROC_SELF_INODE, format!("{}", pid.0)))
            }
            ["self", "cmdline"] => {
                let pid = current_pid()?;
                Ok(proc_file("cmdline", pid_cmdline_inode(pid), move || {
                    proc_pid_cmdline_bytes(pid)
                }))
            }
            ["self", "fd"] => {
                let pid = current_pid()?;
                Ok(proc_dir("fd", pid_fd_dir_inode(pid), pid_fd_entries(pid)?))
            }
            ["self", "fd", fd] => {
                let pid = current_pid()?;
                let fd = parse_fd(fd)?;
                Ok(proc_symlink(
                    fd,
                    pid_fd_inode(pid, fd),
                    fd_target(pid, fd)?,
                ))
            }
            [pid] => {
                let pid = parse_pid(pid)?;
                ensure_pid_exists(pid)?;
                Ok(proc_dir(pid_string(pid).as_str(), pid_dir_inode(pid), pid_dir_entries()))
            }
            [pid, "cmdline"] => {
                let pid = parse_pid(pid)?;
                ensure_pid_exists(pid)?;
                Ok(proc_file(
                    "cmdline",
                    pid_cmdline_inode(pid),
                    move || proc_pid_cmdline_bytes(pid),
                ))
            }
            [pid, "fd"] => {
                let pid = parse_pid(pid)?;
                ensure_pid_exists(pid)?;
                Ok(proc_dir("fd", pid_fd_dir_inode(pid), pid_fd_entries(pid)?))
            }
            [pid, "fd", fd] => {
                let pid = parse_pid(pid)?;
                ensure_pid_exists(pid)?;
                let fd = parse_fd(fd)?;
                Ok(proc_symlink(
                    fd,
                    pid_fd_inode(pid, fd),
                    fd_target(pid, fd)?,
                ))
            }
            _ => Err(FSError::NotFound),
        }
    }
}

fn proc_root_entries() -> Vec<DirectoryContentInfo> {
    let mut entries = vec![
        DirectoryContentInfo::new("cmdline".into(), DirectoryContentType::File),
        DirectoryContentInfo::new("self".into(), DirectoryContentType::Symlink),
    ];

    for pid in MANAGER.lock().processes.keys() {
        entries.push(DirectoryContentInfo::new(
            format!("{}", pid.0),
            DirectoryContentType::Directory,
        ));
    }

    entries
}

fn pid_dir_entries() -> Vec<DirectoryContentInfo> {
    vec![
        DirectoryContentInfo::new("cmdline".into(), DirectoryContentType::File),
        DirectoryContentInfo::new("fd".into(), DirectoryContentType::Directory),
    ]
}

fn pid_fd_entries(pid: ProcessID) -> FSResult<Vec<DirectoryContentInfo>> {
    let process = get_process_with_pid(pid).map_err(|_| FSError::NotFound)?;
    let process = process.lock();
    let mut entries = Vec::new();

    for (fd, object) in process.objects.iter().enumerate() {
        if object.is_some() {
            entries.push(DirectoryContentInfo::new(
                format!("{fd}"),
                DirectoryContentType::Symlink,
            ));
        }
    }

    Ok(entries)
}

fn proc_kernel_cmdline_bytes() -> Vec<u8> {
    Vec::new()
}

fn proc_pid_cmdline_bytes(_pid: ProcessID) -> Vec<u8> {
    Vec::new()
}

fn parse_pid(pid: &str) -> FSResult<ProcessID> {
    pid.parse::<u64>()
        .map(ProcessID)
        .map_err(|_| FSError::NotFound)
}

fn parse_fd(fd: &str) -> FSResult<&str> {
    fd.parse::<usize>().map_err(|_| FSError::NotFound)?;
    Ok(fd)
}

fn ensure_pid_exists(pid: ProcessID) -> FSResult<()> {
    if MANAGER.lock().processes.contains_key(&pid) {
        Ok(())
    } else {
        Err(FSError::NotFound)
    }
}

fn current_pid() -> FSResult<ProcessID> {
    Ok(get_current_process().lock().pid)
}

fn pid_string(pid: ProcessID) -> String {
    format!("{}", pid.0)
}

fn pid_dir_inode(pid: ProcessID) -> u64 {
    0x4000_0000 + pid.0 * 2
}

fn pid_cmdline_inode(pid: ProcessID) -> u64 {
    pid_dir_inode(pid) + 1
}

fn pid_fd_dir_inode(pid: ProcessID) -> u64 {
    0x5000_0000 + pid.0 * 0x100
}

fn pid_fd_inode(pid: ProcessID, fd: &str) -> u64 {
    pid_fd_dir_inode(pid) + 1 + fd.parse::<u64>().unwrap_or(0)
}

fn fd_target(pid: ProcessID, fd: &str) -> FSResult<String> {
    let fd_index = fd.parse::<usize>().map_err(|_| FSError::NotFound)?;
    let process = get_process_with_pid(pid).map_err(|_| FSError::NotFound)?;
    let process = process.lock();
    let object = process
        .objects
        .get(fd_index)
        .and_then(|entry| entry.clone())
        .ok_or(FSError::NotFound)?;

    if let Ok(file_like) = object.clone().as_file_like() {
        if let Ok(info) = file_like.info() {
            if info.name.starts_with('/') {
                return Ok(info.name);
            }
            return Ok(format!("/{}", info.name));
        }
    }

    Ok(format!("anon_inode:[{}]", object.debug_name()))
}

fn proc_dir(name: &str, inode: u64, entries: Vec<DirectoryContentInfo>) -> FileLike {
    FileLike::Directory(Arc::new(Mutex::new(ProcDirectory::new(
        name.into(),
        inode,
        entries,
    ))))
}

fn proc_file<F>(name: &str, inode: u64, read: F) -> FileLike
where
    F: Fn() -> Vec<u8> + Send + Sync + 'static,
{
    FileLike::File(Arc::new(Mutex::new(ProcFile::new(
        name.into(),
        inode,
        Arc::new(read),
    ))))
}

fn proc_symlink(name: &str, inode: u64, target: String) -> FileLike {
    FileLike::Symlink(Arc::new(Mutex::new(ProcSymlink::new(
        name.into(),
        inode,
        target,
    ))))
}

struct ProcDirectory {
    name: String,
    inode: u64,
    entries: Vec<DirectoryContentInfo>,
}

impl ProcDirectory {
    fn new(name: String, inode: u64, entries: Vec<DirectoryContentInfo>) -> Self {
        Self {
            name,
            inode,
            entries,
        }
    }
}

impl Directory for ProcDirectory {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn info(&self) -> FSResult<FileLikeInfo> {
        Ok(FileLikeInfo::new(
            self.name.clone(),
            0,
            UnixPermission(0o040555),
            FileLikeType::Directory,
        )
        .with_inode(self.inode))
    }

    fn name(&self) -> FSResult<String> {
        Ok(self.name.clone())
    }

    fn contents(&self) -> FSResult<Vec<DirectoryContentInfo>> {
        Ok(self.entries.clone())
    }

    fn create(&self, _info: DirectoryContentInfo) -> FSResult<()> {
        Err(FSError::Readonly)
    }

    fn delete(&self, _name: &str) -> FSResult<()> {
        Err(FSError::Readonly)
    }

    fn get(&self, _name: &str) -> FSResult<FileLike> {
        Err(FSError::NotFound)
    }
}

struct ProcFile {
    name: String,
    inode: u64,
    read: Arc<dyn Fn() -> Vec<u8> + Send + Sync>,
    offset: usize,
}

impl ProcFile {
    fn new(name: String, inode: u64, read: Arc<dyn Fn() -> Vec<u8> + Send + Sync>) -> Self {
        Self {
            name,
            inode,
            read,
            offset: 0,
        }
    }
}

impl File for ProcFile {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn info(&mut self) -> FSResult<FileLikeInfo> {
        Ok(FileLikeInfo::new(
            self.name.clone(),
            (self.read)().len(),
            UnixPermission(0o100444),
            FileLikeType::File,
        )
        .with_inode(self.inode))
    }

    fn read_at(&mut self, buffer: &mut [u8], offset: u64) -> FSResult<usize> {
        let data = (self.read)();
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

    fn write(&mut self, _buffer: &[u8]) -> FSResult<usize> {
        Err(FSError::Readonly)
    }

    fn seek(&mut self, offset: i64, seek_type: Whence) -> FSResult<usize> {
        let len = (self.read)().len() as i64;
        let next = match seek_type {
            Whence::Start => offset,
            Whence::Current => self.offset as i64 + offset,
            Whence::End => len + offset,
        }
        .max(0) as usize;

        self.offset = next;
        Ok(self.offset)
    }
}

struct ProcSymlink {
    name: String,
    inode: u64,
    target: String,
}

impl ProcSymlink {
    fn new(name: String, inode: u64, target: String) -> Self {
        Self {
            name,
            inode,
            target,
        }
    }
}

impl Symlink for ProcSymlink {
    fn info(&self) -> FSResult<FileLikeInfo> {
        Ok(FileLikeInfo::new(
            self.name.clone(),
            self.target.len(),
            UnixPermission::symlink(),
            FileLikeType::Symlink,
        )
        .with_inode(self.inode))
    }

    fn target(&self) -> FSResult<Path> {
        Ok(Path::new(&self.target))
    }
}
