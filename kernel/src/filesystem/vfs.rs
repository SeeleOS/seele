use alloc::{boxed::Box, sync::Arc, vec::Vec};
use spin::Mutex;

use crate::filesystem::{
    block_device::BlockDevice,
    block_device::cache::CachedBlockDevice,
    errors::FSError,
    impls::ext4::{EXT4, operator::Ext4BlockOperator},
    path::Path,
    vfs_traits::{Directory, File, FileLike, FileSystem, Symlink},
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
    mounts: Vec<Mount>,
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
        let normalized_path = self.normalize_path(path);
        let fs: FileSystemRef = Arc::new(Mutex::new(fs));
        fs.lock().init()?;
        self.mounts.push(Mount {
            path: normalized_path,
            fs,
        });
        self.mounts
            .sort_by_key(|mount| core::cmp::Reverse(mount.path.clone().as_string().len()));
        Ok(())
    }

    pub fn normalize_path(&self, path: Path) -> Path {
        if path.is_absolute() {
            path.normalize()
        } else {
            path.as_absolute().as_normal().normalize()
        }
    }

    pub fn resolve(&self, path: Path) -> FSResult<FileLike> {
        let normalized_path = self.normalize_path(path);
        let (mount, mount_path) = self.find_mount(&normalized_path)?;
        // `mount_path` is rooted at the matched mount itself. For example,
        // resolving `/dev/null` with a `/dev` mount passes `"/null"` into
        // that filesystem's `lookup()` instead of the global `"/dev/null"`.
        mount.fs.lock().lookup(&mount_path)
    }

    pub fn mount_path(&self, path: Path) -> FSResult<Path> {
        let normalized_path = self.normalize_path(path);
        let (mount, _) = self.find_mount(&normalized_path)?;
        Ok(mount.path.clone())
    }

    fn find_mount(&self, path: &Path) -> FSResult<(&Mount, Path)> {
        for mount in &self.mounts {
            if let Some(stripped) = path.strip_prefix(&mount.path) {
                return Ok((mount, stripped));
            }
        }

        Err(FSError::NotFound)
    }
}
