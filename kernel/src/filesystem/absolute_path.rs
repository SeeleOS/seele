use alloc::{string::String, vec::Vec};

use crate::{
    filesystem::{
        path::{Path, PathPart},
        vfs::{FSResult, WrappedDirectory},
        vfs_traits::FileLike,
    },
    multitasking::process::manager::get_current_process,
};

#[derive(Clone, Debug)]
enum AbsolutePathPart {
    Root,
    Normal(String),
}

#[derive(Clone, Debug)]
pub struct AbsolutePath(Vec<AbsolutePathPart>);

impl Default for AbsolutePath {
    fn default() -> Self {
        Path::default().as_absolute()
    }
}

impl AbsolutePath {
    pub fn as_normal(&self) -> Path {
        let mut new_path = Path::default();

        for part in &self.0 {
            if let AbsolutePathPart::Normal(str) = part {
                new_path.0.push(PathPart::Normal(str.clone()));
            }
        }

        new_path
    }

    pub fn push_path(&mut self, path: AbsolutePath) {
        for ele in path.0 {
            self.0.push(ele);
        }
    }

    pub fn push_path_str(&mut self, string: &str) {
        self.push_path(Path::new(string).as_absolute());
    }

    pub fn as_string(self) -> String {
        self.as_normal().as_string()
    }

    // Wrappers for the normal Path

    pub fn navigate(&mut self, root: WrappedDirectory) -> FSResult<FileLike> {
        self.as_normal().navigate(root)
    }

    pub fn is_valid(&self, root: WrappedDirectory) -> bool {
        self.as_normal().is_valid(root)
    }
}

impl Path {
    pub fn as_absolute(&self) -> AbsolutePath {
        let mut new_path = AbsolutePath(Vec::new());

        for (i, part) in self.0.iter().enumerate() {
            match part {
                PathPart::Normal(str) => new_path.0.push(AbsolutePathPart::Normal(str.clone())),
                PathPart::Root => new_path.0.push(AbsolutePathPart::Root),
                PathPart::CurrentDir => {
                    if i == 0 {
                        new_path.push_path(get_current_process().lock().current_directory.clone());
                    }
                }
                PathPart::ParentDir => {
                    if i == 0 {
                        new_path.push_path(get_current_process().lock().current_directory.clone());
                    }
                    let _ = new_path.0.pop();
                }
            }
        }

        new_path
    }
}
