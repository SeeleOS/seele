use crate::{
    filesystem::{
        info::{DirectoryContentInfo, FileLikeInfo},
        object::FileLikeObject,
        vfs::{FSResult, VFS, VirtualFS},
    },
    object::traits::Readable,
};

use alloc::vec::Vec;

use crate::filesystem::{
    errors::FSError,
    impls::ext4::{directory::Ext4Directory, file::Ext4File},
    path::Path,
    vfs_traits::{DirectoryContentType, FileLike},
};

impl VFS {
    pub fn create_file(&mut self, path: Path) -> FSResult<()> {
        let (parent_dir, name) = path.navigate_to_parent(self.root.clone().unwrap())?;

        parent_dir
            .clone()
            .lock()
            .create(DirectoryContentInfo::new(name, DirectoryContentType::File))
    }

    pub fn create_dir(&mut self, path: Path) -> FSResult<()> {
        let (parent_dir, name) = path.navigate_to_parent(self.root.clone().unwrap())?;

        parent_dir.clone().lock().create(DirectoryContentInfo::new(
            name,
            DirectoryContentType::Directory,
        ))
    }

    pub fn open(&mut self, path: Path) -> FSResult<FileLikeObject> {
        log::trace!("vfs: open {}", path.clone().as_string());
        Ok(FileLikeObject::new(
            path.navigate(self.root.clone().unwrap())?,
        ))
    }

    pub fn file_info(&mut self, path: Path) -> FSResult<FileLikeInfo> {
        log::trace!("vfs: file_info {}", path.clone().as_string());
        path.navigate(self.root.clone().unwrap())?.info()
    }

    pub fn delete_file(&mut self, path: Path) -> FSResult<()> {
        let (dir, name) = path.navigate_to_parent(self.root.clone().unwrap())?;
        dir.lock().delete(&name)?;

        Ok(())
    }

    pub fn link_file(&mut self, old_path: Path, new_path: Path) -> FSResult<()> {
        log::trace!(
            "vfs: link_file {} -> {}",
            old_path.clone().as_string(),
            new_path.clone().as_string()
        );

        let source = old_path.navigate(self.root.clone().unwrap())?;
        let source_inode = match source {
            FileLike::File(file) => {
                let file = file.lock();
                let ext4_file = file.as_any().downcast_ref::<Ext4File>().ok_or(FSError::Other)?;
                ext4_file.inode()
            }
            FileLike::Directory(_) => return Err(FSError::Other),
        };

        let (parent_dir, name) = new_path.navigate_to_parent(self.root.clone().unwrap())?;
        let parent = parent_dir.lock();
        let ext4_parent = parent
            .as_any()
            .downcast_ref::<Ext4Directory>()
            .ok_or(FSError::Other)?;

        let parent_inode = ext4_parent
            .fs()
            .path_to_inode(ext4plus::path::Path::new(ext4_parent.path()), ext4plus::FollowSymlinks::All)
            .map_err(FSError::from)?;
        let parent_dir = ext4plus::dir::Dir::open_inode(ext4_parent.fs(), parent_inode)
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

    pub fn list_contents(&self, path: Path) -> FSResult<Vec<DirectoryContentInfo>> {
        log::trace!("vfs: list_contents {}", path.clone().as_string());
        let dir = path.navigate(self.root.clone().unwrap())?;

        if let FileLike::Directory(dir) = dir {
            Ok(dir.lock().contents()?)
        } else {
            Err(FSError::NotADirectory)
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
