use crate::memory::utils::Mut;
use alloc::{
    collections::BTreeSet,
    string::{String, ToString},
    sync::Arc,
    vec::Vec,
};

use crate::{
    drivers::virtio::block::{list_devices as list_block_devices, named_device},
    drm::card::{CARD0_RDEV, RENDERD128_RDEV},
    filesystem::{
        errors::FSError,
        info::{DirectoryContentInfo, FileLikeInfo, UnixPermission},
        path::{Path, PathPart},
        tmpfs::{TmpNodeKind, TmpfsState, TmpfsStateRef, tmpfs_lookup_path},
        vfs::FSResult,
        vfs_traits::{Directory, DirectoryContentType, FileLike, FileLikeType, FileSystem},
    },
    object::device::get_device_ref,
    terminal::pty::{get_pty_slave, list_ptys},
};

const S_IFCHR: u32 = 0o020000;
const S_IFBLK: u32 = 0o060000;

struct SeedDevice {
    parent: &'static str,
    name: &'static str,
    mode: u32,
    rdev: u64,
}

struct SeedDirectory {
    parent: &'static str,
    name: &'static str,
    mode: u32,
}

struct SeedSymlink {
    parent: &'static str,
    name: &'static str,
    target: &'static str,
}

static SEED_DIRECTORIES: &[SeedDirectory] = &[
    SeedDirectory {
        parent: "/",
        name: "input",
        mode: 0o040755,
    },
    SeedDirectory {
        parent: "/",
        name: "dri",
        mode: 0o040755,
    },
    SeedDirectory {
        parent: "/",
        name: "pts",
        mode: 0o040755,
    },
    SeedDirectory {
        parent: "/",
        name: "shm",
        mode: 0o041777,
    },
];

static SEED_DEVICES: &[SeedDevice] = &[
    SeedDevice {
        parent: "/",
        name: "null",
        mode: S_IFCHR | 0o666,
        rdev: (1u64 << 8) | 3,
    },
    SeedDevice {
        parent: "/",
        name: "zero",
        mode: S_IFCHR | 0o666,
        rdev: (1u64 << 8) | 5,
    },
    SeedDevice {
        parent: "/",
        name: "random",
        mode: S_IFCHR | 0o666,
        rdev: (1u64 << 8) | 8,
    },
    SeedDevice {
        parent: "/",
        name: "urandom",
        mode: S_IFCHR | 0o666,
        rdev: (1u64 << 8) | 9,
    },
    SeedDevice {
        parent: "/",
        name: "tty",
        mode: S_IFCHR | 0o666,
        rdev: 5u64 << 8,
    },
    SeedDevice {
        parent: "/",
        name: "console",
        mode: S_IFCHR | 0o600,
        rdev: (5u64 << 8) | 1,
    },
    SeedDevice {
        parent: "/",
        name: "tty0",
        mode: S_IFCHR | 0o620,
        rdev: 4u64 << 8,
    },
    SeedDevice {
        parent: "/",
        name: "tty1",
        mode: S_IFCHR | 0o620,
        rdev: (4u64 << 8) | 1,
    },
    SeedDevice {
        parent: "/",
        name: "ttyS0",
        mode: S_IFCHR | 0o620,
        rdev: (4u64 << 8) | 64,
    },
    SeedDevice {
        parent: "/",
        name: "tty2",
        mode: S_IFCHR | 0o620,
        rdev: (4u64 << 8) | 2,
    },
    SeedDevice {
        parent: "/",
        name: "tty3",
        mode: S_IFCHR | 0o620,
        rdev: (4u64 << 8) | 3,
    },
    SeedDevice {
        parent: "/",
        name: "tty4",
        mode: S_IFCHR | 0o620,
        rdev: (4u64 << 8) | 4,
    },
    SeedDevice {
        parent: "/",
        name: "tty5",
        mode: S_IFCHR | 0o620,
        rdev: (4u64 << 8) | 5,
    },
    SeedDevice {
        parent: "/",
        name: "tty6",
        mode: S_IFCHR | 0o620,
        rdev: (4u64 << 8) | 6,
    },
    SeedDevice {
        parent: "/",
        name: "fb0",
        mode: S_IFCHR | 0o666,
        rdev: 29u64 << 8,
    },
    SeedDevice {
        parent: "/",
        name: "psaux",
        mode: S_IFCHR | 0o666,
        rdev: (10u64 << 8) | 1,
    },
    SeedDevice {
        parent: "/",
        name: "mouse",
        mode: S_IFCHR | 0o666,
        rdev: (13u64 << 8) | 32,
    },
    SeedDevice {
        parent: "/input",
        name: "event0",
        mode: S_IFCHR | 0o660,
        rdev: (13u64 << 8) | 64,
    },
    SeedDevice {
        parent: "/input",
        name: "event1",
        mode: S_IFCHR | 0o660,
        rdev: (13u64 << 8) | 65,
    },
    SeedDevice {
        parent: "/dri",
        name: "card0",
        mode: S_IFCHR | 0o660,
        rdev: CARD0_RDEV,
    },
    SeedDevice {
        parent: "/dri",
        name: "renderD128",
        mode: S_IFCHR | 0o660,
        rdev: RENDERD128_RDEV,
    },
    SeedDevice {
        parent: "/",
        name: "ptmx",
        mode: S_IFCHR | 0o666,
        rdev: (5u64 << 8) | 2,
    },
    SeedDevice {
        parent: "/",
        name: "kmsg",
        mode: S_IFCHR | 0o600,
        rdev: (1u64 << 8) | 11,
    },
    SeedDevice {
        parent: "/",
        name: "fuse",
        mode: S_IFCHR | 0o666,
        rdev: (10u64 << 8) | 229,
    },
];

static SEED_SYMLINKS: &[SeedSymlink] = &[SeedSymlink {
    parent: "/",
    name: "log",
    target: "/run/systemd/journal/dev-log",
}];

pub struct DevFs {
    state: TmpfsStateRef,
}

pub struct DevPtsFs;

struct DevDirectoryHandle {
    state: TmpfsStateRef,
    path: String,
}
struct DevPtsDirectoryHandle;

fn root_directory_file_like(state: TmpfsStateRef) -> FileLike {
    directory_file_like(state, "/".into())
}

fn directory_file_like(state: TmpfsStateRef, path: String) -> FileLike {
    FileLike::Directory(Arc::new(Mut::new(DevDirectoryHandle { state, path })))
}

fn pts_directory_file_like() -> FileLike {
    FileLike::Directory(Arc::new(Mut::new(DevPtsDirectoryHandle)))
}

fn pts_inode(number: u32) -> u64 {
    0x2000 + u64::from(number)
}

fn pts_file_like(number: u32) -> FSResult<FileLike> {
    let object = get_pty_slave(number).ok_or(FSError::NotFound)?;
    Ok(FileLike::File(Arc::new(Mut::new(
        crate::filesystem::staticfs::device::StaticDeviceHandle::from_object(
            number.to_string(),
            pts_inode(number),
            0o020620,
            Some((136u64 << 8) | number as u64),
            object,
        ),
    ))))
}

fn block_device_file_like(name: &str) -> FSResult<FileLike> {
    let device = named_device(name).ok_or(FSError::NotFound)?;
    let object = get_device_ref(name).map_err(|_| FSError::NotFound)?;
    Ok(FileLike::File(Arc::new(Mut::new(
        crate::filesystem::staticfs::device::StaticDeviceHandle::from_object(
            device.name.clone(),
            0x3000 + device.minor,
            S_IFBLK | 0o660,
            Some(device.rdev()),
            object,
        ),
    ))))
}

fn dynamic_children(state: &TmpfsStateRef, path: &str) -> FSResult<Vec<DirectoryContentInfo>> {
    let state = state.lock();
    let node = state.node(path)?;
    let children = match &node.kind {
        TmpNodeKind::Directory { children, .. } => children,
        TmpNodeKind::File { .. } | TmpNodeKind::Device { .. } | TmpNodeKind::Symlink { .. } => {
            return Err(FSError::NotADirectory);
        }
    };

    let mut entries = Vec::with_capacity(children.len());
    for child in children {
        let child_path = TmpfsState::child_path(path, child);
        let child_node = state.node(&child_path)?;
        let content_type = match child_node.kind {
            TmpNodeKind::Directory { .. } => DirectoryContentType::Directory,
            TmpNodeKind::File { .. } | TmpNodeKind::Device { .. } => DirectoryContentType::File,
            TmpNodeKind::Symlink { .. } => DirectoryContentType::Symlink,
        };
        entries.push(
            DirectoryContentInfo::new(child.clone(), content_type).with_inode(child_node.inode),
        );
    }
    Ok(entries)
}

impl Directory for DevDirectoryHandle {
    fn as_any(&self) -> &dyn core::any::Any {
        self
    }

    fn info(&self) -> FSResult<FileLikeInfo> {
        let state = self.state.lock();
        let node = state.node(&self.path)?;
        let TmpNodeKind::Directory { mode, .. } = node.kind else {
            return Err(FSError::NotADirectory);
        };
        Ok(FileLikeInfo::new(
            if self.path == "/" {
                "dev".into()
            } else {
                self.path
                    .rsplit('/')
                    .next()
                    .filter(|name| !name.is_empty())
                    .unwrap_or("dev")
                    .into()
            },
            0,
            UnixPermission(mode),
            FileLikeType::Directory,
        )
        .with_inode(node.inode)
        .with_owner(node.uid, node.gid)
        .with_nlink(node.link_count)
        .with_times(node.times))
    }

    fn name(&self) -> FSResult<String> {
        if self.path == "/" {
            Ok("dev".into())
        } else {
            Ok(self
                .path
                .rsplit('/')
                .next()
                .filter(|name| !name.is_empty())
                .unwrap_or("dev")
                .into())
        }
    }

    fn contents(&self) -> FSResult<Vec<DirectoryContentInfo>> {
        let mut seen = BTreeSet::new();
        let mut entries = Vec::new();
        for entry in dynamic_children(&self.state, &self.path)? {
            if seen.insert(entry.name.clone()) {
                entries.push(entry);
            }
        }

        if self.path == "/" {
            for device in list_block_devices() {
                if seen.insert(device.name.clone()) {
                    entries.push(
                        DirectoryContentInfo::new(device.name, DirectoryContentType::File)
                            .with_inode(0x3000 + device.minor),
                    );
                }
            }
        }

        Ok(entries)
    }

    fn create(&self, info: DirectoryContentInfo) -> FSResult<()> {
        let mut state = self.state.lock();
        match info.content_type {
            DirectoryContentType::File => state.create_file(
                &self.path,
                &info.name,
                info.permission
                    .unwrap_or(UnixPermission(crate::filesystem::tmpfs::DEFAULT_FILE_MODE))
                    .0,
                info.rdev,
            ),
            DirectoryContentType::Directory => state.create_directory(
                &self.path,
                &info.name,
                info.permission.unwrap_or(UnixPermission::directory()).0,
            ),
            DirectoryContentType::Symlink => Err(FSError::Readonly),
        }
    }

    fn create_symlink(&self, name: &str, target: &str) -> FSResult<()> {
        self.state.lock().create_symlink(&self.path, name, target)
    }

    fn delete(&self, name: &str) -> FSResult<()> {
        self.state.lock().delete_node(&self.path, name)
    }

    fn get(&self, name: &str) -> FSResult<FileLike> {
        if self.path == "/pts" {
            let FileLike::Directory(directory) = pts_directory_file_like() else {
                unreachable!();
            };
            return directory.lock().get(name);
        }

        if self.path == "/" && named_device(name).is_some() {
            return block_device_file_like(name);
        }

        let child_path = TmpfsState::child_path(&self.path, name);
        let file_like = tmpfs_lookup_path(&self.state, &child_path)?;
        if child_path == "/pts" {
            Ok(pts_directory_file_like())
        } else {
            Ok(file_like)
        }
    }

    fn chmod(&self, mode: u32) -> FSResult<()> {
        let mut state = self.state.lock();
        let node = state.node_mut(&self.path)?;
        match &mut node.kind {
            TmpNodeKind::Directory { mode: dir_mode, .. } => {
                *dir_mode = mode & 0o7777;
                Ok(())
            }
            TmpNodeKind::File { .. } | TmpNodeKind::Device { .. } | TmpNodeKind::Symlink { .. } => {
                Err(FSError::NotADirectory)
            }
        }
    }

    fn chown(&self, uid: u32, gid: u32) -> FSResult<()> {
        let mut state = self.state.lock();
        let inode = state.node(&self.path)?.inode;
        state.update_owner_by_inode(inode, uid, gid)
    }

    fn set_times(&self, times: crate::filesystem::info::FileTimes) -> FSResult<()> {
        let mut state = self.state.lock();
        let inode = state.node(&self.path)?.inode;
        state.update_times_by_inode(inode, times)
    }
}

fn seed_devfs_state(state: &mut TmpfsState) -> FSResult<()> {
    for directory in SEED_DIRECTORIES {
        state.create_directory(directory.parent, directory.name, directory.mode)?;
    }
    for device in SEED_DEVICES {
        state.create_file(device.parent, device.name, device.mode, device.rdev)?;
    }
    for symlink in SEED_SYMLINKS {
        state.create_symlink(symlink.parent, symlink.name, symlink.target)?;
    }
    Ok(())
}

impl DevFs {
    pub fn new() -> Self {
        let state = Arc::new(Mut::new(TmpfsState::new()));
        {
            let mut state_guard = state.lock();
            seed_devfs_state(&mut state_guard).expect("devfs seed should succeed");
        }
        Self { state }
    }
}

impl Default for DevFs {
    fn default() -> Self {
        Self::new()
    }
}

impl DevPtsFs {
    pub fn new() -> Self {
        Self
    }
}

impl Default for DevPtsFs {
    fn default() -> Self {
        Self::new()
    }
}

impl FileSystem for DevFs {
    fn as_any(&self) -> &dyn core::any::Any {
        self
    }

    fn init(&mut self) -> FSResult<()> {
        Ok(())
    }

    fn lookup(&self, path: &Path) -> FSResult<FileLike> {
        let normalized = path.normalize();
        let mut current = root_directory_file_like(self.state.clone());

        for component in normalized.parts.iter() {
            match component {
                PathPart::Root | PathPart::CurrentDir => {}
                PathPart::ParentDir => return Err(FSError::NotADirectory),
                PathPart::Normal(name) => {
                    let FileLike::Directory(directory) = current else {
                        return Err(FSError::NotADirectory);
                    };
                    current = directory.lock().get(name)?;
                }
            }
        }

        Ok(current)
    }

    fn rename(&self, old_path: &Path, new_path: &Path) -> FSResult<()> {
        let old_path = devfs_path(old_path);
        let new_path = devfs_path(new_path);
        self.state.lock().rename(&old_path, &new_path)
    }

    fn link(&self, old_path: &Path, new_path: &Path) -> FSResult<()> {
        let old_path = devfs_path(old_path);
        let new_path = devfs_path(new_path);
        self.state.lock().link(&old_path, &new_path)
    }

    fn name(&self) -> &'static str {
        "devtmpfs"
    }

    fn magic(&self) -> i64 {
        0x0102_1994
    }

    fn mount_source(&self) -> &'static str {
        "devtmpfs"
    }

    fn default_mount_flags(&self, _path: &Path) -> crate::filesystem::vfs_traits::MountFlags {
        crate::filesystem::vfs_traits::MountFlags::MS_NOSUID
            | crate::filesystem::vfs_traits::MountFlags::MS_RELATIME
    }
}

fn devfs_path(path: &Path) -> String {
    let components = path
        .normalize()
        .parts
        .iter()
        .filter_map(|part| match part {
            PathPart::Normal(name) => Some(name.clone()),
            _ => None,
        })
        .collect::<Vec<_>>();
    if components.is_empty() {
        "/".into()
    } else {
        alloc::format!("/{}", components.join("/"))
    }
}

impl FileSystem for DevPtsFs {
    fn as_any(&self) -> &dyn core::any::Any {
        self
    }

    fn init(&mut self) -> FSResult<()> {
        Ok(())
    }

    fn lookup(&self, path: &Path) -> FSResult<FileLike> {
        let normalized = path.normalize();
        let components = normalized
            .parts
            .iter()
            .filter_map(|part| match part {
                PathPart::Normal(name) => Some(name.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>();

        match components.as_slice() {
            [] => Ok(pts_directory_file_like()),
            [number] => pts_file_like(number.parse::<u32>().map_err(|_| FSError::NotFound)?),
            _ => Err(FSError::NotFound),
        }
    }

    fn rename(&self, _old_path: &Path, _new_path: &Path) -> FSResult<()> {
        Err(FSError::Readonly)
    }

    fn link(&self, _old_path: &Path, _new_path: &Path) -> FSResult<()> {
        Err(FSError::Readonly)
    }

    fn name(&self) -> &'static str {
        "devpts"
    }

    fn magic(&self) -> i64 {
        0x1cd1
    }

    fn mount_source(&self) -> &'static str {
        "devpts"
    }

    fn default_mount_flags(&self, _path: &Path) -> crate::filesystem::vfs_traits::MountFlags {
        crate::filesystem::vfs_traits::MountFlags::MS_NOSUID
            | crate::filesystem::vfs_traits::MountFlags::MS_NOEXEC
            | crate::filesystem::vfs_traits::MountFlags::MS_RELATIME
    }
}

impl Directory for DevPtsDirectoryHandle {
    fn as_any(&self) -> &dyn core::any::Any {
        self
    }

    fn info(&self) -> FSResult<FileLikeInfo> {
        Ok(FileLikeInfo::new(
            "pts".into(),
            0,
            UnixPermission(0o040755),
            FileLikeType::Directory,
        )
        .with_inode(0x100c))
    }

    fn name(&self) -> FSResult<String> {
        Ok("pts".into())
    }

    fn contents(&self) -> FSResult<Vec<DirectoryContentInfo>> {
        Ok(list_ptys()
            .into_iter()
            .map(|number| DirectoryContentInfo::new(number.to_string(), DirectoryContentType::File))
            .collect())
    }

    fn create(&self, _info: DirectoryContentInfo) -> FSResult<()> {
        Err(FSError::Readonly)
    }

    fn delete(&self, _name: &str) -> FSResult<()> {
        Err(FSError::Readonly)
    }

    fn get(&self, name: &str) -> FSResult<FileLike> {
        let number = name.parse::<u32>().map_err(|_| FSError::NotFound)?;
        pts_file_like(number)
    }
}
