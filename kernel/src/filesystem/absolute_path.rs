use alloc::{string::String, vec::Vec};

use crate::filesystem::path::{Path, PathPart};

#[derive(Clone, Debug, PartialEq, Eq)]
enum AbsolutePathPart {
    Normal(String),
}

#[derive(Clone, Debug)]
pub struct AbsolutePath(Vec<AbsolutePathPart>);

impl Default for AbsolutePath {
    fn default() -> Self {
        Self::root()
    }
}

impl AbsolutePath {
    pub fn root() -> Self {
        Self(Vec::new())
    }

    pub fn as_normal(&self) -> Path {
        let mut new_path = Path::default();

        for part in &self.0 {
            let AbsolutePathPart::Normal(str) = part;
            new_path.parts.push(PathPart::Normal(str.clone()));
        }

        new_path
    }

    pub fn push_path(&mut self, path: AbsolutePath) {
        for ele in path.0 {
            self.0.push(ele);
        }
    }

    pub fn starts_with(&self, prefix: &Self) -> bool {
        self.0.starts_with(&prefix.0)
    }

    pub fn strip_prefix(&self, prefix: &Self) -> Option<Self> {
        if !self.starts_with(prefix) {
            return None;
        }
        Some(Self(self.0[prefix.0.len()..].to_vec()))
    }

    pub fn from_root_path(path: &Path) -> Self {
        let mut new_path = Self::root();

        for part in path.parts.iter() {
            match part {
                PathPart::Normal(str) => new_path.0.push(AbsolutePathPart::Normal(str.clone())),
                PathPart::Root | PathPart::CurrentDir => {}
                PathPart::ParentDir => {
                    let _ = new_path.0.pop();
                }
            }
        }

        new_path
    }

    pub fn join_under_root(root: &Self, current: &Self, path: &Path) -> Self {
        let mut new_path = if path.is_absolute() {
            root.clone()
        } else {
            current.clone()
        };

        for part in path.parts.iter() {
            match part {
                PathPart::Normal(str) => new_path.0.push(AbsolutePathPart::Normal(str.clone())),
                PathPart::Root | PathPart::CurrentDir => {}
                PathPart::ParentDir => {
                    if new_path.0.len() > root.0.len() {
                        let _ = new_path.0.pop();
                    }
                }
            }
        }

        new_path
    }

    pub fn push_path_str(&mut self, string: &str) {
        let path = Path::new(string);

        if path.is_absolute() {
            *self = Self::from_root_path(&path);
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

    pub fn display_string(&self, root: &Self) -> String {
        self.strip_prefix(root)
            .unwrap_or_else(AbsolutePath::root)
            .as_string()
    }
}

impl Path {
    pub fn as_absolute_from(&self, root: &AbsolutePath, current: &AbsolutePath) -> AbsolutePath {
        AbsolutePath::join_under_root(root, current, self)
    }
}
