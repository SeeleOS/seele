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
    vfs_traits::{Directory, File, FileSystem, Symlink},
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
}

pub struct VFS {
    pub(super) mounts: Vec<Mount>,
}

impl VFS {
    pub fn new() -> Self {
        Self { mounts: Vec::new() }
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
        self.mount(Path::new("/"), EXT4(ext4))?;
        self.mount(Path::new("/run"), TmpFs::new())?;
        self.mount(Path::new("/dev"), DevFs::new())?;
        self.mount(Path::new("/proc"), ProcFs::new())?;
        self.mount(Path::new("/sys"), SysFs::new())?;
        self.mount(Path::new("/sys/fs/cgroup"), CgroupFs::new())?;

        for temp_dir in ["/tmp", "/var/tmp"] {
            if let Err(err) = self.clear_directory(Path::new(temp_dir)) {
                log::warn!("vfs: failed to clean {}: {:?}", temp_dir, err);
            } else {
                log::info!("vfs: cleaned {}", temp_dir);
            }
        }

        log::debug!("vfs: init done");
        Ok(())
    }

    pub fn mount(&mut self, path: Path, fs: impl FileSystem + 'static) -> FSResult<()> {
        let fs: FileSystemRef = Arc::new(Mutex::new(fs));
        self.mount_ref(path, fs)
    }

    pub fn mount_ref(&mut self, path: Path, fs: FileSystemRef) -> FSResult<()> {
        let normalized_path = self.normalize_path(path);
        let normalized_path_string = normalized_path.clone().as_string();
        self.mounts
            .retain(|mount| mount.path.clone().as_string() != normalized_path_string);
        fs.lock().init()?;
        self.mounts.push(Mount {
            path: normalized_path,
            fs,
        });
        self.mounts
            .sort_by_key(|mount| Reverse(mount.path.clone().as_string().len()));
        Ok(())
    }

    pub fn unmount(&mut self, path: Path) -> FSResult<()> {
        let normalized_path = self.normalize_path(path);
        let normalized_path_string = normalized_path.clone().as_string();
        let old_len = self.mounts.len();
        self.mounts
            .retain(|mount| mount.path.clone().as_string() != normalized_path_string);
        if self.mounts.len() == old_len {
            return Err(FSError::NotFound);
        }
        Ok(())
    }

    pub fn mount_metadata(&self, path: Path) -> FSResult<(Path, FileSystemRef)> {
        let mount_path = self.mount_path(path)?;
        let mount_path_string = mount_path.clone().as_string();
        let mount = self
            .mounts
            .iter()
            .find(|mount| mount.path.clone().as_string() == mount_path_string)
            .ok_or(FSError::NotFound)?;
        Ok((mount.path.clone(), mount.fs.clone()))
    }

    pub fn mount_snapshots(&self) -> Vec<(Path, FileSystemRef)> {
        self.mounts
            .iter()
            .map(|mount| (mount.path.clone(), mount.fs.clone()))
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

impl Default for VFS {
    fn default() -> Self {
        Self::new()
    }
}
