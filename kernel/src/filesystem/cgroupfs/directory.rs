use super::*;

pub(super) struct CgroupDirectoryHandle {
    path: String,
}

impl CgroupDirectoryHandle {
    pub(super) fn new(path: String) -> Self {
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
            return Ok(FileLike::Directory(Arc::new(Mut::new(
                CgroupDirectoryHandle::new(child_path),
            ))));
        }

        if let Some(kind) = CgroupFileKind::from_name(name) {
            return Ok(FileLike::File(Arc::new(Mut::new(CgroupFileHandle::new(
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
