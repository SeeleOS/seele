use crate::memory::utils::Mut;
use alloc::{boxed::Box, sync::Arc, vec::Vec};
use anyhow::Context;
use core::cmp::Reverse;

use crate::filesystem::{
    block_device::BlockDevice,
    block_device::cache::CachedBlockDevice,
    errors::FSError,
    impls::ext4::{EXT4, operator::Ext4BlockOperator},
    path::Path,
    vfs_traits::{Directory, File, FileSystem, MountFlags, MountPropagation, Symlink},
};
use ext4plus::Ext4 as Ext4Inner;
use lazy_static::lazy_static;

use crate::drivers::virtio::block::root_device as virtio_root_device;
use crate::misc::error::KernelError;

lazy_static! {
    pub static ref VirtualFS: Mut<VFS> = Mut::new(VFS::new());
}

pub type FSResult<T> = Result<T, FSError>;
pub type WrappedDirectory = Arc<Mut<dyn Directory>>;
pub type WrappedFile = Arc<Mut<dyn File>>;
pub type WrappedSymlink = Arc<Mut<dyn Symlink>>;
pub type FileSystemRef = Arc<Mut<dyn FileSystem>>;

pub struct Mount {
    pub path: Path,
    pub fs: FileSystemRef,
    pub source_path: Path,
    pub flags: MountFlags,
    pub propagation: MountPropagation,
    pub device_id: u64,
    pub mount_id: u64,
    pub expire_pending: bool,
}

pub struct VFS {
    pub(super) mounts: Vec<Mount>,
    next_mount_device_id: u64,
    next_mount_id: u64,
    next_peer_group_id: u64,
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

        let mut removed_exact = false;
        self.mounts.retain(|mount| {
            let mount_path = mount.path.clone();
            let mount_path_string = mount_path.clone().as_string();
            if mount_path_string == normalized_path_string {
                if include_children || !removed_exact {
                    removed_exact = true;
                    return false;
                }
                return true;
            }

            !(include_children && mount_path.starts_with(&normalized_path))
        });
        Ok(())
    }

    pub fn new() -> Self {
        Self {
            mounts: Vec::new(),
            next_mount_device_id: 1,
            next_mount_id: 1,
            next_peer_group_id: 1,
        }
    }

    pub fn init(&mut self) -> FSResult<()> {
        log::debug!("vfs: init start");
        let block_device: Arc<dyn BlockDevice> = Arc::new(CachedBlockDevice::new(
            virtio_root_device().ok_or(FSError::NotFound)?,
        ));
        log::info!("vfs: loading ext4 from root block device");
        let reader = Ext4BlockOperator::new(block_device.clone());
        let writer = Ext4BlockOperator::new(block_device.clone());
        let ext4 = Ext4Inner::load_with_writer(Box::new(reader), Some(Box::new(writer)))
            .context("failed to load root ext4 filesystem")
            .map_err(|err| {
                let err = KernelError::from(err);
                log::error!("vfs: {err:?}");
                FSError::Other
            })?;
        log::info!("vfs: ext4 loaded");
        let ext4 = EXT4::new_with_device(ext4, block_device).map_err(|err| {
            let err = KernelError::from(err);
            log::error!("vfs: {err:?}");
            FSError::Other
        })?;
        self.mount(Path::new("/"), ext4)?;

        log::debug!("vfs: init done");
        Ok(())
    }

    pub fn mount(&mut self, path: Path, fs: impl FileSystem + 'static) -> FSResult<()> {
        let fs: FileSystemRef = Arc::new(Mut::new(fs));
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
        self.attach_mount_with_propagation(path, fs, source_path, flags, MountPropagation::Private)
    }

    pub fn attach_mount_with_propagation(
        &mut self,
        path: Path,
        fs: FileSystemRef,
        source_path: Path,
        flags: MountFlags,
        propagation: MountPropagation,
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
        let mount_id = self
            .mounts
            .iter()
            .find(|mount| mount.path == normalized_path)
            .map(|mount| mount.mount_id)
            .unwrap_or_else(|| {
                let mount_id = self.next_mount_id;
                self.next_mount_id += 1;
                mount_id
            });
        self.mounts
            .retain(|mount| mount.path.clone().as_string() != normalized_path_string);
        self.mounts.push(Mount {
            path: normalized_path,
            fs,
            source_path: source_path.normalize(),
            flags,
            propagation,
            device_id,
            mount_id,
            expire_pending: false,
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
                propagation: mount.propagation,
                device_id: mount.device_id,
                mount_id: mount.mount_id,
                expire_pending: false,
            })
            .collect::<Vec<_>>();

        let (source_mount, source_relative) = self.find_mount(&source)?;
        self.attach_mount_with_propagation(
            target.clone(),
            source_mount.fs.clone(),
            source_relative,
            source_mount.flags,
            source_mount.propagation,
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
            self.attach_mount_with_propagation(
                target_path,
                mount.fs.clone(),
                mount.source_path.clone(),
                mount.flags,
                mount.propagation,
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

    pub fn remount_bind_in_current_namespace(
        &mut self,
        path: Path,
        flags: MountFlags,
        mask: MountFlags,
        recursive: bool,
    ) -> FSResult<()> {
        if crate::process::manager::get_current_process()
            .lock()
            .mount_namespace_snapshot
            .is_none()
        {
            return self.remount_bind(path, flags, mask, recursive);
        }

        let mount_path = self.mount_path(path)?;
        let mount_path_string = mount_path.clone().as_string();
        let targets = self
            .mounts
            .iter()
            .filter(|mount| {
                let is_target = mount.path.clone().as_string() == mount_path_string;
                is_target || recursive && mount.path.starts_with(&mount_path)
            })
            .map(|mount| (mount.mount_id, mount.flags))
            .collect::<Vec<_>>();
        if targets.is_empty() {
            return Err(FSError::NotFound);
        }

        let process = crate::process::manager::get_current_process();
        let mut process = process.lock();
        for (mount_id, current_flags) in targets {
            let current_flags = process
                .mount_namespace_flag_overrides
                .get(&mount_id)
                .copied()
                .unwrap_or(current_flags);
            let mut next_flags = current_flags;
            next_flags.remove(mask);
            next_flags.insert(flags & mask);
            process
                .mount_namespace_flag_overrides
                .insert(mount_id, next_flags);
        }

        Ok(())
    }

    pub fn set_mount_propagation(
        &mut self,
        path: Path,
        propagation: MountPropagationUpdate,
        recursive: bool,
    ) -> FSResult<()> {
        let mount_path = self.mount_path(path)?;
        let mut updated = false;
        let shared_id = if propagation == MountPropagationUpdate::Shared {
            let id = self.next_peer_group_id;
            self.next_peer_group_id += 1;
            Some(id)
        } else {
            None
        };

        for mount in &mut self.mounts {
            if !(mount.path == mount_path || recursive && mount.path.starts_with(&mount_path)) {
                continue;
            }

            mount.propagation = match propagation {
                MountPropagationUpdate::Shared => MountPropagation::Shared(shared_id.unwrap()),
                MountPropagationUpdate::Slave => {
                    let master = match mount.propagation {
                        MountPropagation::Shared(id) => id,
                        MountPropagation::Slave { master } => master,
                        MountPropagation::Private | MountPropagation::Unbindable => {
                            let id = self.next_peer_group_id;
                            self.next_peer_group_id += 1;
                            id
                        }
                    };
                    MountPropagation::Slave { master }
                }
                MountPropagationUpdate::Private => MountPropagation::Private,
                MountPropagationUpdate::Unbindable => MountPropagation::Unbindable,
            };
            updated = true;
        }

        if !updated {
            return Err(FSError::NotFound);
        }

        Ok(())
    }

    pub fn stack_mount_beneath(&mut self, source: Path, target: Path) -> FSResult<()> {
        let source = self.normalize_path(source);
        let target = self.normalize_path(target);
        let source_index = self
            .mounts
            .iter()
            .position(|mount| mount.path == source)
            .ok_or(FSError::NotFound)?;
        let target_index = self
            .mounts
            .iter()
            .position(|mount| mount.path == target)
            .ok_or(FSError::NotFound)?;

        let mut source_mount = self.mounts.remove(source_index);
        let target_index = if source_index < target_index {
            target_index - 1
        } else {
            target_index
        };
        source_mount.path = target;
        source_mount.mount_id = self.next_mount_id;
        self.next_mount_id += 1;
        self.mounts.insert(target_index + 1, source_mount);
        self.mounts
            .sort_by_key(|mount| Reverse(mount.path.clone().as_string().len()));
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

    pub fn begin_expire_mount(&mut self, path: Path) -> FSResult<bool> {
        let normalized_path = self.normalize_path(path);
        let mount = self
            .mounts
            .iter_mut()
            .find(|mount| mount.path == normalized_path)
            .ok_or(FSError::NotFound)?;
        if mount.expire_pending {
            return Ok(true);
        }
        mount.expire_pending = true;
        Ok(false)
    }

    pub fn mark_mount_accessed(&mut self, path: Path) -> FSResult<()> {
        let normalized_path = self.normalize_path(path);
        let (mount, _) = self.find_mount(&normalized_path)?;
        let mount_path = mount.path.clone();
        if let Some(mount) = self
            .mounts
            .iter_mut()
            .find(|mount| mount.path == mount_path)
        {
            mount.expire_pending = false;
        }
        Ok(())
    }

    pub fn contains_mount_at(&self, path: Path) -> bool {
        let normalized_path = self.normalize_path(path);
        self.mounts
            .iter()
            .any(|mount| mount.path == normalized_path)
    }

    pub fn is_mount_busy(&self, path: Path) -> FSResult<bool> {
        let normalized_path = self.normalize_path(path);
        let target_mount_id = self
            .mounts
            .iter()
            .find(|mount| mount.path == normalized_path)
            .ok_or(FSError::NotFound)?
            .mount_id;
        Ok(crate::process::manager::MANAGER
            .lock()
            .processes
            .values()
            .any(|process| {
                process
                    .lock()
                    .fd_table
                    .lock()
                    .iter()
                    .flatten()
                    .any(|entry| {
                        entry.object.clone().as_file_like().is_ok_and(|file| {
                            file.mount_id() == target_mount_id && !file.mount_root()
                        })
                    })
            }))
    }

    pub fn detach_mount(&mut self, path: Path) -> FSResult<()> {
        let normalized_path = self.normalize_path(path);
        self.remove_mounts_at(&normalized_path, true)
    }

    pub fn detach_mounts_created_after_snapshot(&mut self, snapshot_mount_ids: &[u64]) {
        self.mounts
            .retain(|mount| snapshot_mount_ids.contains(&mount.mount_id));
    }

    pub fn mount_metadata(&self, path: Path) -> FSResult<(Path, FileSystemRef, Path, MountFlags)> {
        let (mount, _) = self.find_mount(&self.normalize_path(path))?;
        Ok((
            mount.path.clone(),
            mount.fs.clone(),
            mount.source_path.clone(),
            mount.flags,
        ))
    }

    pub fn sync_all(&self) -> FSResult<()> {
        for mount in &self.mounts {
            mount.fs.lock().sync()?;
        }
        Ok(())
    }

    pub fn sync_path(&self, path: Path) -> FSResult<()> {
        let (mount, _) = self.find_mount(&self.normalize_path(path))?;
        mount.fs.lock().sync()
    }

    pub fn mount_metadata_with_propagation(
        &self,
        path: Path,
    ) -> FSResult<(Path, FileSystemRef, Path, MountFlags, MountPropagation)> {
        let (mount, _) = self.find_mount(&self.normalize_path(path))?;
        Ok((
            mount.path.clone(),
            mount.fs.clone(),
            mount.source_path.clone(),
            mount.flags,
            mount.propagation,
        ))
    }

    pub fn mount_metadata_for_path_with_propagation(
        &self,
        path: Path,
    ) -> FSResult<(FileSystemRef, Path, MountFlags, MountPropagation)> {
        let (mount, source_path) = self.find_mount(&self.normalize_path(path))?;
        Ok((
            mount.fs.clone(),
            source_path,
            mount.flags,
            mount.propagation,
        ))
    }

    pub fn ensure_writable_mount(&self, path: Path) -> FSResult<()> {
        let (mount, _) = self.find_mount(&self.normalize_path(path))?;
        if mount.flags.contains(MountFlags::MS_RDONLY) {
            return Err(FSError::Readonly);
        }
        Ok(())
    }

    pub fn mount_device_id(&self, path: Path) -> FSResult<u64> {
        let (mount, _) = self.find_mount(&self.normalize_path(path))?;
        Ok(mount.device_id)
    }

    pub fn mount_path_and_ids(&self, path: Path) -> FSResult<(Path, u64, u64)> {
        let (mount, _) = self.find_mount(&self.normalize_path(path))?;
        Ok((mount.path.clone(), mount.device_id, mount.mount_id))
    }

    pub fn mount_id(&self, path: Path) -> FSResult<u64> {
        let (mount, _) = self.find_mount(&self.normalize_path(path))?;
        Ok(mount.mount_id)
    }

    pub fn mount_count(&self) -> usize {
        self.mounts.len()
    }

    pub fn mount_ids(&self) -> Vec<u64> {
        self.mounts.iter().map(|mount| mount.mount_id).collect()
    }

    pub fn mount_snapshots(&self) -> Vec<(Path, FileSystemRef, Path, MountFlags, u64, u64)> {
        self.mounts
            .iter()
            .map(|mount| {
                (
                    mount.path.clone(),
                    mount.fs.clone(),
                    mount.source_path.clone(),
                    mount.flags,
                    mount.device_id,
                    mount.mount_id,
                )
            })
            .collect()
    }

    pub fn mount_snapshots_with_propagation(
        &self,
    ) -> Vec<(
        Path,
        FileSystemRef,
        Path,
        MountFlags,
        MountPropagation,
        u64,
        u64,
    )> {
        self.mounts
            .iter()
            .map(|mount| {
                (
                    mount.path.clone(),
                    mount.fs.clone(),
                    mount.source_path.clone(),
                    mount.flags,
                    mount.propagation,
                    mount.device_id,
                    mount.mount_id,
                )
            })
            .collect()
    }

    pub fn normalize_path(&self, path: Path) -> Path {
        path.normalize()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MountPropagationUpdate {
    Private,
    Shared,
    Slave,
    Unbindable,
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
