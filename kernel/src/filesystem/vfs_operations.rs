use crate::{
    filesystem::{
        info::{DirectoryContentInfo, FileLikeInfo},
        object::FileLikeObject,
        vfs::{FSResult, VFS, VirtualFS, WrappedDirectory, WrappedFile},
    },
    object::traits::Readable,
};
use alloc::string::String;

use alloc::vec::Vec;

use crate::filesystem::{
    errors::FSError,
    impls::ext4::{directory::Ext4Directory, file::Ext4File},
    path::Path,
    vfs_traits::{DirectoryContentType, FileLike},
};

impl VFS {
    fn resolve_parent(&self, path: Path) -> FSResult<(WrappedDirectory, String)> {
        let normalized = self.normalize_path(path);
        let name = normalized.file_name().ok_or(FSError::NotFound)?;
        let parent = normalized.parent().ok_or(FSError::NotFound)?;
        Ok((self.resolve_dir(parent)?, name))
    }

    pub fn create_file(&mut self, path: Path) -> FSResult<()> {
        let (parent_dir, name) = self.resolve_parent(path)?;

        parent_dir
            .clone()
            .lock()
            .create(DirectoryContentInfo::new(name, DirectoryContentType::File))
    }

    pub fn create_dir(&mut self, path: Path) -> FSResult<()> {
        let (parent_dir, name) = self.resolve_parent(path)?;

        parent_dir.clone().lock().create(DirectoryContentInfo::new(
            name,
            DirectoryContentType::Directory,
        ))
    }

    pub fn create_symlink(&mut self, path: Path, target: &str) -> FSResult<()> {
        let (parent_dir, name) = self.resolve_parent(path)?;
        parent_dir.lock().create_symlink(&name, target)
    }

    pub fn open(&mut self, path: Path) -> FSResult<FileLikeObject> {
        let normalized = self.normalize_path(path);
        let normalized_string = normalized.clone().as_string();
        if normalized_string.contains("hostname")
            || normalized_string.contains("domainname")
            || normalized_string.contains("/systemd/inaccessible/")
        {
            crate::s_println!("vfs open {}", normalized_string);
        }
        log::trace!("vfs: open {}", normalized.clone().as_string());
        let (file, resolved_path) = self.resolve_with_path(normalized)?;
        Ok(FileLikeObject::new(file, resolved_path))
    }

    pub fn open_nofollow(&mut self, path: Path) -> FSResult<FileLikeObject> {
        let normalized = self.normalize_path(path);
        let normalized_string = normalized.clone().as_string();
        if normalized_string.contains("hostname")
            || normalized_string.contains("domainname")
            || normalized_string.contains("/systemd/inaccessible/")
        {
            crate::s_println!("vfs open_nofollow {}", normalized_string);
        }
        log::trace!("vfs: open_nofollow {}", normalized.clone().as_string());
        let (file, resolved_path) = self.resolve_nofollow_with_path(normalized)?;
        Ok(FileLikeObject::new(file, resolved_path))
    }

    pub fn file_info(&mut self, path: Path) -> FSResult<FileLikeInfo> {
        let normalized = self.normalize_path(path);
        let normalized_string = normalized.clone().as_string();
        if normalized_string.contains("hostname")
            || normalized_string.contains("domainname")
            || normalized_string.contains("/systemd/inaccessible/")
        {
            crate::s_println!("vfs file_info {}", normalized_string);
        }
        log::trace!("vfs: file_info {}", normalized.clone().as_string());
        self.resolve(normalized)?.info()
    }

    pub fn delete_file(&mut self, path: Path) -> FSResult<()> {
        let (dir, name) = self.resolve_parent(path)?;
        dir.lock().delete(&name)?;

        Ok(())
    }

    pub fn rename_file(&mut self, old_path: Path, new_path: Path) -> FSResult<()> {
        let old_path = self.normalize_path(old_path);
        let new_path = self.normalize_path(new_path);
        let (old_mount_path, old_fs, _, _) = self.mount_metadata(old_path.clone())?;
        let (new_mount_path, _, _, _) = self.mount_metadata(new_path.clone())?;
        if old_mount_path != new_mount_path {
            return Err(FSError::Other);
        }

        let old_relative = old_path
            .strip_prefix(&old_mount_path)
            .ok_or(FSError::NotFound)?;
        let new_relative = new_path
            .strip_prefix(&old_mount_path)
            .ok_or(FSError::NotFound)?;
        old_fs.lock().rename(&old_relative, &new_relative)
    }

    pub fn link_file(&mut self, old_path: Path, new_path: Path) -> FSResult<()> {
        let old_mount = self.mount_path(old_path.clone())?;
        let new_mount = self.mount_path(new_path.clone())?;
        if old_mount != new_mount {
            return Err(FSError::Other);
        }

        log::trace!(
            "vfs: link_file {} -> {}",
            old_path.clone().as_string(),
            new_path.clone().as_string()
        );

        let source = self.resolve(old_path)?;
        let source_inode = match source {
            FileLike::File(file) => {
                let file = file.lock();
                let ext4_file = file
                    .as_any()
                    .downcast_ref::<Ext4File>()
                    .ok_or(FSError::Other)?;
                ext4_file.inode()
            }
            FileLike::Symlink(_) => todo!(),
            FileLike::Directory(_) => return Err(FSError::Other),
        };

        let (parent_dir, name) = self.resolve_parent(new_path)?;
        let parent = parent_dir.lock();
        let ext4_parent = parent
            .as_any()
            .downcast_ref::<Ext4Directory>()
            .ok_or(FSError::Other)?;

        let parent_inode = ext4_parent
            .fs()
            .path_to_inode(
                ext4plus::path::Path::new(ext4_parent.path()),
                ext4plus::FollowSymlinks::All,
            )
            .map_err(FSError::from)?;
        let mut parent_dir = ext4plus::dir::Dir::open_inode(ext4_parent.fs(), parent_inode)
            .map_err(FSError::from)?;

        let mut source_inode = source_inode;
        parent_dir
            .link(
                ext4plus::DirEntryName::try_from(name.as_str()).map_err(|_| FSError::Other)?,
                &mut source_inode,
            )
            .map_err(FSError::from)?;

        Ok(())
    }

    pub fn resolve_file(&self, path: Path) -> FSResult<WrappedFile> {
        match self.resolve(path)? {
            FileLike::File(file) => Ok(file),
            FileLike::Symlink(symlink) => self.resolve_file(symlink.lock().target()?),
            FileLike::Directory(_) => Err(FSError::NotAFile),
        }
    }

    pub fn resolve_dir(&self, path: Path) -> FSResult<WrappedDirectory> {
        match self.resolve(path)? {
            FileLike::File(_) => Err(FSError::NotADirectory),
            FileLike::Directory(dir) => Ok(dir),
            FileLike::Symlink(symlink) => self.resolve_dir(symlink.lock().target()?),
        }
    }

    pub fn list_contents(&self, path: Path) -> FSResult<Vec<DirectoryContentInfo>> {
        self.resolve_dir(path)?.lock().contents()
    }

    pub fn clear_directory(&mut self, path: Path) -> FSResult<()> {
        let entries = self.list_contents(path.clone())?;
        let mut first_error = None;

        for entry in entries {
            if entry.name == "." || entry.name == ".." {
                continue;
            }

            let child_path = Path::new(&(path.clone().as_string() + "/" + &entry.name));

            match entry.content_type {
                DirectoryContentType::Directory => {
                    if let Err(err) = self.clear_directory(child_path.clone()) {
                        log::warn!(
                            "vfs: failed to clear {}: {:?}",
                            child_path.clone().as_string(),
                            err
                        );
                        if first_error.is_none() {
                            first_error = Some(err);
                        }
                    }

                    if let Err(err) = self.delete_file(child_path.clone()) {
                        log::warn!(
                            "vfs: failed to delete {}: {:?}",
                            child_path.clone().as_string(),
                            err
                        );
                        if first_error.is_none() {
                            first_error = Some(err);
                        }
                    }
                }
                DirectoryContentType::File | DirectoryContentType::Symlink => {
                    if let Err(err) = self.delete_file(child_path.clone()) {
                        log::warn!(
                            "vfs: failed to delete {}: {:?}",
                            child_path.clone().as_string(),
                            err
                        );
                        if first_error.is_none() {
                            first_error = Some(err);
                        }
                    }
                }
            }
        }

        if let Some(err) = first_error {
            Err(err)
        } else {
            Ok(())
        }
    }
}

pub fn read_all(path: Path) -> FSResult<Vec<u8>> {
    log::debug!("read_all: {}", path.clone().as_string());
    let file_object = VirtualFS.lock().open(path)?;
    let mut content = Vec::with_capacity(file_object.info().unwrap().size);

    let mut total_read = 0;

    content.resize(file_object.info().unwrap().size, 0);
    while let Ok(n) = file_object.read(&mut content[total_read..]) {
        if n == 0 {
            break;
        }
        total_read += n;
    }

    log::debug!("read_all: total {} bytes", total_read);
    Ok(content)
}
