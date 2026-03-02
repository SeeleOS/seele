use alloc::{string::String, sync::Arc, vec::Vec};
use spin::Mutex;

use crate::filesystem::{
    vfs::{FSResult, VFS, WrappedDirectory},
    vfs_traits::{DirectoryContentInfo, DirectoryContentType, FileLike},
};

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum PathPart {
    // [TODO] CurrentDir,
    // [TODO] ParentDir,
    Root,
    Normal(String),
}

#[derive(Clone, Debug)]
pub struct Path(pub Vec<PathPart>);

impl Path {
    pub fn new(path: &str) -> Self {
        Self(Self::parse(path))
    }

    fn parse(path: &str) -> Vec<PathPart> {
        let mut buf = String::new();
        let mut vec = Vec::new();

        if path.chars().nth(0) == Some('/') {
            vec.push(PathPart::Root);
        }

        for ch in path.chars() {
            match ch {
                '/' => {
                    if buf.is_empty() {
                        continue;
                    }
                    vec.push(PathPart::Normal(buf.clone()));
                    buf.clear()
                }
                _ => buf.push(ch),
            }
        }

        vec.push(PathPart::Normal(buf));

        vec
    }

    pub fn navigate(&self, vfs: &VFS) -> FSResult<(WrappedDirectory, String)> {
        let mut current_dir = vfs.root.clone().unwrap();

        for ele in 0..=self.0.len() - 2 {
            let ele = self.0.get(ele).unwrap();
            match ele {
                PathPart::Normal(name) => {
                    let next_dir = {
                        let guard = current_dir.lock();
                        if let Ok(FileLike::Directory(dir)) = guard.get(name.as_str().clone()) {
                            Some(dir.clone())
                        } else {
                            None
                        }
                    };

                    if let Some(dir) = next_dir {
                        current_dir = dir.clone();
                    } else {
                        current_dir
                            .clone()
                            .lock()
                            .create(DirectoryContentInfo::new(
                                name.clone(),
                                DirectoryContentType::Directory,
                            ))?;

                        let dir = {
                            let guard = current_dir.lock();
                            if let Ok(FileLike::Directory(dir)) = guard.get(name.as_str().clone()) {
                                Some(dir.clone())
                            } else {
                                None
                            }
                        };
                        if let Some(dir) = dir {
                            current_dir = dir.clone();
                        }
                    }
                }
                PathPart::Root => continue,
            }
        }

        let name = {
            if let Some(PathPart::Normal(name)) = self.0.get(self.0.len() - 1) {
                Some(name)
            } else {
                None
            }
        };
        Ok((current_dir, name.unwrap().clone()))
    }
}
