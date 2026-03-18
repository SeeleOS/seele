use alloc::{collections::vec_deque::VecDeque, string::String, vec::Vec};

use crate::{
    filesystem::path::{Path, PathPart},
    multitasking::process::{manager::get_current_process, new},
};

#[derive(Clone, Debug)]
enum AbsolutePathPart {
    Root,
    Normal(String),
}

#[derive(Clone, Debug)]
pub struct AbsolutePath(pub Vec<AbsolutePathPart>, pub String);

impl Default for AbsolutePath {
    fn default() -> Self {
        Path::default().as_absolute().unwrap()
    }
}

impl AbsolutePath {
    pub fn as_normal(self) -> Path {
        let mut new_path = Path::default();

        for part in self.0 {
            if let AbsolutePathPart::Normal(str) = part {
                new_path.0.push(PathPart::Normal(str));
            }
        }

        new_path
    }

    pub fn push_path(&mut self, path: AbsolutePath) {
        for ele in path.0 {
            self.0.push(ele);
        }
    }
}

impl Path {
    pub fn as_absolute(self) -> Option<AbsolutePath> {
        let mut new_path = AbsolutePath(Vec::new(), String::new());

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
                    new_path.0.pop()?;
                }
            }
        }

        Some(new_path)
    }
}
