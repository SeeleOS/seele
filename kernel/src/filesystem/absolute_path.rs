use alloc::{string::String, vec::Vec};

use crate::{
    filesystem::path::{Path, PathPart},
    process::manager::get_current_process,
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
                new_path.parts.push(PathPart::Normal(str.clone()));
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
        let path = Path::new(string);

        if path.is_absolute() {
            *self = path.as_absolute();
            return;
        }

        for part in path.parts {
            match part {
                PathPart::Root | PathPart::CurrentDir => {}
                PathPart::Normal(str) => self.0.push(AbsolutePathPart::Normal(str)),
                PathPart::ParentDir => {
                    if matches!(self.0.last(), Some(AbsolutePathPart::Normal(_))) {
                        self.0.pop();
                    }
                }
            }
        }
    }

    pub fn as_string(self) -> String {
        self.as_normal().as_string()
    }
}

impl Path {
    pub fn as_absolute(&self) -> AbsolutePath {
        let mut new_path = if self.is_absolute() {
            AbsolutePath(Vec::new())
        } else {
            get_current_process().lock().current_directory.clone()
        };

        for part in self.parts.iter() {
            match part {
                PathPart::Normal(str) => new_path.0.push(AbsolutePathPart::Normal(str.clone())),
                PathPart::Root => new_path.0.push(AbsolutePathPart::Root),
                PathPart::CurrentDir => {}
                PathPart::ParentDir => {
                    let _ = new_path.0.pop();
                }
            }
        }

        new_path
    }
}
