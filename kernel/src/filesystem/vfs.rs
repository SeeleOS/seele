use alloc::{boxed::Box, sync::Arc, vec::Vec};
use core::cmp::Reverse;
use spin::Mutex;

use crate::filesystem::{
    block_device::BlockDevice,
    block_device::cache::CachedBlockDevice,
    cgroupfs::CgroupFs,
    devfs::DevFs,
    errors::FSError,
    impls::ext4::{EXT4, operator::Ext4BlockOperator},
    path::Path,
    procfs::ProcFs,
    sysfs::SysFs,
    tmpfs::TmpFs,
    vfs_traits::{Directory, File, FileSystem, MountFlags, Symlink},
};
use ext4plus::Ext4 as Ext4Inner;
use lazy_static::lazy_static;

use crate::drivers::virtio::block::root_device as virtio_root_device;

lazy_static! {
    pub static ref VirtualFS: Mutex<VFS> = Mutex::new(VFS::new());
}

pub type FSResult<T> = Result<T, FSError>;
pub type WrappedDirectory = Arc<Mutex<dyn Directory>>;
pub type WrappedFile = Arc<Mutex<dyn File>>;
pub type WrappedSymlink = Arc<Mutex<dyn Symlink>>;
pub type FileSystemRef = Arc<Mutex<dyn FileSystem>>;

pub struct Mount {
    pub path: Path,
    pub fs: FileSystemRef,
    pub source_path: Path,
    pub flags: MountFlags,
    pub device_id: u64,
}

pub struct VFS {
    pub(super) mounts: Vec<Mount>,
    next_mount_device_id: u64,
}

impl VFS {
    fn remove_mounts_at(&mut self, path: &Path, include_children: bool) -> FSResult<()> {
        let normalized_path = self.normalize_path(path.clone());
        let normalized_path_string = normalized_path.clone().as_string();
        let has_exact_mount = self
            .mounts
            .iter()
            .any(|mount| mount.path.clone().as_string() == normalized_path_string);
        if !has_exact_mount {
            return Err(FSError::NotFound);
        }

        self.mounts.retain(|mount| {
            let mount_path = mount.path.clone();
            let mount_path_string = mount_path.clone().as_string();
            if mount_path_string == normalized_path_string {
                return false;
            }

            !(include_children && mount_path.starts_with(&normalized_path))
        });
        Ok(())
    }

    pub fn new() -> Self {
        Self {
            mounts: Vec::new(),
            next_mount_device_id: 1,
        }
    }

    pub fn init(&mut self) -> FSResult<()> {
        log::debug!("vfs: init start");
        let block_device: Arc<dyn BlockDevice> = Arc::new(CachedBlockDevice::new(
            virtio_root_device().ok_or(FSError::NotFound)?,
        ));
        log::info!("vfs: loading ext4 from root block device");
        let reader = Ext4BlockOperator::new(block_device.clone());
        let writer = Ext4BlockOperator::new(block_device);
        let ext4 = Ext4Inner::load_with_writer(Box::new(reader), Some(Box::new(writer))).unwrap();
        log::info!("vfs: ext4 loaded");
        self.mount(Path::new("/"), EXT4::new(ext4))?;
        self.mount(Path::new("/tmp"), TmpFs::new())?;
        self.mount(Path::new("/run"), TmpFs::new())?;
        self.mount(Path::new("/dev"), DevFs::new())?;
        self.mount(Path::new("/proc"), ProcFs::new())?;
        self.mount(Path::new("/sys"), SysFs::new())?;
        self.mount(Path::new("/sys/fs/cgroup"), CgroupFs::new())?;

        log::debug!("vfs: init done");
        Ok(())
    }

    pub fn mount(&mut self, path: Path, fs: impl FileSystem + 'static) -> FSResult<()> {
        let fs: FileSystemRef = Arc::new(Mutex::new(fs));
        self.mount_ref(path, fs)
    }

    pub fn mount_ref(&mut self, path: Path, fs: FileSystemRef) -> FSResult<()> {
        let normalized_path = self.normalize_path(path);
        fs.lock().init()?;
        let flags = fs.lock().default_mount_flags(&normalized_path);
        self.attach_mount(normalized_path, fs, Path::new("/"), flags)
    }

    pub fn attach_mount(
        &mut self,
        path: Path,
        fs: FileSystemRef,
        source_path: Path,
        flags: MountFlags,
    ) -> FSResult<()> {
        let normalized_path = self.normalize_path(path);
        let normalized_path_string = normalized_path.clone().as_string();
        let device_id = self
            .mounts
            .iter()
            .find(|mount| Arc::ptr_eq(&mount.fs, &fs))
            .map(|mount| mount.device_id)
            .unwrap_or_else(|| {
                let device_id = self.next_mount_device_id;
                self.next_mount_device_id += 1;
                device_id
            });
        self.mounts
            .retain(|mount| mount.path.clone().as_string() != normalized_path_string);
        self.mounts.push(Mount {
            path: normalized_path,
            fs,
            source_path: source_path.normalize(),
            flags,
            device_id,
        });
        self.mounts
            .sort_by_key(|mount| Reverse(mount.path.clone().as_string().len()));
        Ok(())
    }

    pub fn bind_mount(&mut self, source: Path, target: Path, recursive: bool) -> FSResult<()> {
        let source = self.normalize_path(source);
        let target = self.normalize_path(target);
        let source_mounts = self
            .mounts
            .iter()
            .map(|mount| Mount {
                path: mount.path.clone(),
                fs: mount.fs.clone(),
                source_path: mount.source_path.clone(),
                flags: mount.flags,
                device_id: mount.device_id,
            })
            .collect::<Vec<_>>();

        let (source_mount, source_relative) = self.find_mount(&source)?;
        self.attach_mount(
            target.clone(),
            source_mount.fs.clone(),
            source_relative,
            source_mount.flags,
        )?;

        if !recursive {
            return Ok(());
        }

        for mount in source_mounts {
            if mount.path == source || !mount.path.starts_with(&source) {
                continue;
            }

            let Some(suffix) = mount.path.strip_prefix(&source) else {
                continue;
            };
            let target_path = join_paths(&target, &suffix);
            self.attach_mount(
                target_path,
                mount.fs.clone(),
                mount.source_path.clone(),
                mount.flags,
            )?;
        }

        Ok(())
    }

    pub fn remount_bind(
        &mut self,
        path: Path,
        flags: MountFlags,
        mask: MountFlags,
        recursive: bool,
    ) -> FSResult<()> {
        let mount_path = self.mount_path(path)?;
        let mount_path_string = mount_path.clone().as_string();
        let mut updated = false;

        for mount in &mut self.mounts {
            let is_target = mount.path.clone().as_string() == mount_path_string;
            if !(is_target || recursive && mount.path.starts_with(&mount_path)) {
                continue;
            }

            mount.flags.remove(mask);
            mount.flags.insert(flags & mask);
            updated = true;
        }

        if !updated {
            return Err(FSError::NotFound);
        }

        Ok(())
    }

    pub fn unmount(&mut self, path: Path) -> FSResult<()> {
        let normalized_path = self.normalize_path(path);
        if self
            .mounts
            .iter()
            .any(|mount| mount.path != normalized_path && mount.path.starts_with(&normalized_path))
        {
            return Err(FSError::Busy);
        }
        self.remove_mounts_at(&normalized_path, false)
    }

    pub fn detach_mount(&mut self, path: Path) -> FSResult<()> {
        let normalized_path = self.normalize_path(path);
        self.remove_mounts_at(&normalized_path, true)
    }

    pub fn mount_metadata(&self, path: Path) -> FSResult<(Path, FileSystemRef, Path, MountFlags)> {
        let mount_path = self.mount_path(path)?;
        let mount_path_string = mount_path.clone().as_string();
        let mount = self
            .mounts
            .iter()
            .find(|mount| mount.path.clone().as_string() == mount_path_string)
            .ok_or(FSError::NotFound)?;
        Ok((
            mount.path.clone(),
            mount.fs.clone(),
            mount.source_path.clone(),
            mount.flags,
        ))
    }

    pub fn mount_device_id(&self, path: Path) -> FSResult<u64> {
        let mount_path = self.mount_path(path)?;
        let mount_path_string = mount_path.clone().as_string();
        self.mounts
            .iter()
            .find(|mount| mount.path.clone().as_string() == mount_path_string)
            .map(|mount| mount.device_id)
            .ok_or(FSError::NotFound)
    }

    pub fn mount_snapshots(&self) -> Vec<(Path, FileSystemRef, Path, MountFlags)> {
        self.mounts
            .iter()
            .map(|mount| {
                (
                    mount.path.clone(),
                    mount.fs.clone(),
                    mount.source_path.clone(),
                    mount.flags,
                )
            })
            .collect()
    }

    pub fn normalize_path(&self, path: Path) -> Path {
        if path.is_absolute() {
            path.normalize()
        } else {
            path.as_absolute().as_normal().normalize()
        }
    }
}

fn join_paths(base: &Path, suffix: &Path) -> Path {
    let mut path = base.normalize().as_string();
    for part in suffix.normalize().parts {
        if let crate::filesystem::path::PathPart::Normal(component) = part {
            if !path.ends_with('/') {
                path.push('/');
            }
            path.push_str(&component);
        }
    }
    Path::new(&path).normalize()
}

impl Default for VFS {
    fn default() -> Self {
        Self::new()
    }
}
