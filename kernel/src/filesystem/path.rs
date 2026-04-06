use alloc::{string::String, vec::Vec};

use crate::{
    filesystem::{
        errors::FSError,
        vfs::{FSResult, VirtualFS, WrappedDirectory},
        vfs_traits::FileLike,
    },
    process::manager::get_current_process,
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
pub struct Path {
    pub parts: Vec<PathPart>,
    ends_with_slash: bool,
}

impl Path {
    pub fn new(path: &str) -> Self {
        if path.is_empty() {
            Self::new("/")
        } else {
            Self {
                parts: Self::parse(path),
                ends_with_slash: path.len() > 1 && path.ends_with('/'),
            }
        }
    }

    pub fn is_valid(&self, root: WrappedDirectory) -> bool {
        self.navigate(root).is_ok()
    }

    fn parse(path: &str) -> Vec<PathPart> {
        let mut vec = Vec::new();

        if path.starts_with('/') {
            vec.push(PathPart::Root);
        }

        for component in path.split('/') {
            if component.is_empty() {
                continue;
            }

            match component {
                "." => vec.push(PathPart::CurrentDir),
                ".." => vec.push(PathPart::ParentDir),
                _ => vec.push(PathPart::Normal(component.into())),
            }
        }

        vec
    }

    /// NOTE:
    /// If you do navigate_with_depth with a depth of 1 and a
    /// path len of 6, the actrual depth that will be 5 (6 - 1)
    fn navigate_with_depth(&self, root: WrappedDirectory, depth: usize) -> FSResult<FileLike> {
        let first = self.parts.first().ok_or(FSError::NotFound)?;
        let mut current = match first {
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

        let end = self.parts.len().saturating_sub(depth);

        for i in 0..end {
            let part = &self.parts[i];
            match part {
                PathPart::Root => continue,
                PathPart::Normal(name) => {
                    while let FileLike::Symlink(symlink) = &current {
                        let target = symlink.lock().target()?;
                        current = FileLike::Directory(VirtualFS.lock().resolve_dir(target)?);
                    }

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
        let current = self.navigate_with_depth(root, 0)?;
        if self.ends_with_slash && matches!(current, FileLike::File(_)) {
            return Err(FSError::NotADirectory);
        }
        Ok(current)
    }

    pub fn navigate_to_parent(
        &self,
        root: WrappedDirectory,
    ) -> FSResult<(WrappedDirectory, String)> {
        let name = self.parts.last().ok_or(FSError::NotFound)?;
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

            FileLike::Symlink(symlink) => {
                let target = symlink.lock().target()?;
                let dir = VirtualFS.lock().resolve_dir(target)?;
                Ok((
                    dir,
                    match name {
                        PathPart::Normal(s) => s.clone(),
                        PathPart::Root => todo!("Find a proper error name for this case"),
                        PathPart::CurrentDir => todo!(),
                        PathPart::ParentDir => todo!(),
                    },
                ))
            }
        }
    }

    pub fn as_string(self) -> String {
        let mut segments = Vec::new();
        let mut is_absolute = false;

        for part in self.parts {
            match part {
                PathPart::Root => is_absolute = true,
                PathPart::Normal(part) => {
                    if !part.is_empty() {
                        segments.push(part);
                    }
                }
                PathPart::CurrentDir => segments.push(".".into()),
                PathPart::ParentDir => segments.push("..".into()),
            }
        }

        let mut string = if is_absolute {
            String::from("/")
        } else {
            String::new()
        };

        if !segments.is_empty() {
            if !string.is_empty() && !string.ends_with('/') {
                string.push('/');
            }
            string.push_str(&segments.join("/"));
        }

        if self.ends_with_slash && !string.ends_with('/') {
            string.push('/');
        }

        if string.is_empty() {
            string.push('/');
        }

        string
    }
}
