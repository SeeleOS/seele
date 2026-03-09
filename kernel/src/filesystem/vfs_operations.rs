use crate::{
    filesystem::{
        info::{DirectoryContentInfo, FileLikeInfo},
        object::FileObject,
        vfs::{FSResult, VFS, VirtualFS},
    },
    object::Readable,
    s_println,
};

use alloc::vec::Vec;

use crate::filesystem::{
    errors::FSError,
    path::Path,
    vfs_traits::{Directory, DirectoryContentType, File, FileLike},
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

    pub fn open(&mut self, path: Path) -> FSResult<FileObject> {
        if let FileLike::File(file) = path.navigate(self.root.clone().unwrap())? {
            Ok(FileObject::new(file))
        } else {
            Err(FSError::NotAFile)
        }
    }

    pub fn file_info(&mut self, path: Path) -> FSResult<FileLikeInfo> {
        path.navigate(self.root.clone().unwrap())?.info()
    }

    pub fn delete_file(&mut self, _path: Path) -> FSResult<()> {
        unimplemented!("Just dont create files that your gonna delete lmao its not my problem")
    }

    pub fn list_contents(&self, path: Path) -> FSResult<Vec<DirectoryContentInfo>> {
        let dir = path.navigate(self.root.clone().unwrap())?;

        if let FileLike::Directory(dir) = dir {
            Ok(dir.lock().contents()?)
        } else {
            Err(FSError::NotADirectory)
        }
    }
}

pub fn read_all(path: Path) -> FSResult<Vec<u8>> {
    let mut content = Vec::new();
    let file_object = VirtualFS.lock().open(path)?;
    while let Ok(n) = file_object.read(&mut content) {
        if n == 0 {
            break;
        }
    }

    Ok(content)
}
