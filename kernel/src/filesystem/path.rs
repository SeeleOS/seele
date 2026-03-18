use alloc::{
    string::{String, ToString},
    vec::Vec,
};
use elfloader::PAddr;

use crate::{
    filesystem::{
        errors::FSError,
        vfs::{FSResult, WrappedDirectory},
        vfs_traits::FileLike,
    },
    multitasking::process::manager::get_current_process,
};

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum PathPart {
    CurrentDir,
    ParentDir,
    Root,
    Normal(String),
}

impl Default for Path {
    fn default() -> Self {
        Self::new("/")
    }
}

#[derive(Clone, Debug)]
pub struct Path(pub Vec<PathPart>);

impl Path {
    pub fn new(path: &str) -> Self {
        if path.is_empty() {
            Self::new("/")
        } else {
            Self(Self::parse(path))
        }
    }

    pub fn is_valid(&self, root: WrappedDirectory) -> bool {
        self.navigate(root).is_ok()
    }

    fn parse(path: &str) -> Vec<PathPart> {
        let mut buf = String::new();
        let mut vec = Vec::new();

        if let Some(character) = path.chars().nth(0) {
            match character {
                '/' => vec.push(PathPart::Root),
                '.' => {
                    if let Some(character) = path.chars().nth(1)
                        && character == '.'
                    {
                        vec.push(PathPart::ParentDir);
                    } else {
                        vec.push(PathPart::CurrentDir);
                    }
                }
                _ => {}
            }
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
                '.' => {
                    if buf.is_empty() {
                        continue;
                    }

                    vec.push(PathPart::CurrentDir);
                    buf.clear();
                }
                _ => buf.push(ch),
            }
        }

        if !buf.is_empty() {
            vec.push(PathPart::Normal(buf));
        }

        vec
    }

    /// NOTE:
    /// If you do navigate_with_depth with a depth of 1 and a
    /// path len of 6, the actrual depth that will be 5 (6 - 1)
    fn navigate_with_depth(&self, root: WrappedDirectory, depth: usize) -> FSResult<FileLike> {
        let mut current = match &self.0[0] {
            PathPart::Root => FileLike::Directory(root),
            PathPart::CurrentDir => get_current_process()
                .lock()
                .current_directory
                .clone()
                .as_normal()
                .navigate(root)?,
            _ => get_current_process()
                .lock()
                .current_directory
                .clone()
                .as_normal()
                .navigate(root)?,
        };

        let end = self.0.len().saturating_sub(depth);

        for i in 0..end {
            let part = &self.0[i];
            match part {
                PathPart::Root => continue,
                PathPart::Normal(name) => {
                    current = {
                        if let FileLike::Directory(current) = current {
                            let current = current.lock();
                            current.get(name.as_str())?
                        } else {
                            return Err(FSError::NotADirectory);
                        }
                    };
                }
                PathPart::CurrentDir => {}
                PathPart::ParentDir => {
                    unimplemented!()
                }
            }
        }

        Ok(current)
    }

    pub fn navigate(&self, root: WrappedDirectory) -> FSResult<FileLike> {
        self.navigate_with_depth(root, 0)
    }

    pub fn navigate_to_parent(
        &self,
        root: WrappedDirectory,
    ) -> FSResult<(WrappedDirectory, String)> {
        let name = self.0.last().ok_or(FSError::NotFound)?;
        let nav = self.navigate_with_depth(root, 1)?;

        match nav {
            FileLike::File(_) => Err(FSError::NotADirectory),
            FileLike::Directory(dir) => Ok((
                dir,
                match name {
                    PathPart::Normal(s) => s.clone(),
                    PathPart::Root => todo!("Find a proper error name for this case"),
                    PathPart::CurrentDir => todo!(),
                    PathPart::ParentDir => todo!(),
                },
            )),
        }
    }

    pub fn as_string(self) -> String {
        let mut string = String::new();

        for part in self.0 {
            match part {
                PathPart::Root => string.push('/'),
                PathPart::Normal(part) => {
                    if !part.is_empty() {
                        string.push_str(&part);
                        string.push('/');
                    }
                }
                PathPart::CurrentDir => string.push_str("."),
                PathPart::ParentDir => string.push_str(".."),
            }
        }

        string
    }
}
