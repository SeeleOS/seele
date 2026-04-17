use alloc::{
    string::String,
    vec,
    vec::Vec,
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

#[derive(Clone, Debug, PartialEq, Eq)]
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

    pub fn is_absolute(&self) -> bool {
        matches!(self.parts.first(), Some(PathPart::Root))
    }

    pub fn normalize(&self) -> Self {
        let mut normalized = Vec::new();
        let is_absolute = self.is_absolute();

        if is_absolute {
            normalized.push(PathPart::Root);
        }

        for part in &self.parts {
            match part {
                PathPart::Root | PathPart::CurrentDir => {}
                PathPart::Normal(component) => normalized.push(PathPart::Normal(component.clone())),
                PathPart::ParentDir => match normalized.last() {
                    Some(PathPart::Normal(_)) => {
                        normalized.pop();
                    }
                    Some(PathPart::Root) | None if is_absolute => {}
                    _ => normalized.push(PathPart::ParentDir),
                },
            }
        }

        Self {
            parts: normalized,
            ends_with_slash: self.ends_with_slash,
        }
    }

    pub fn file_name(&self) -> Option<String> {
        self.normalize().parts.into_iter().rev().find_map(|part| match part {
            PathPart::Normal(name) => Some(name),
            _ => None,
        })
    }

    pub fn parent(&self) -> Option<Self> {
        let normalized = self.normalize();
        let is_absolute = normalized.is_absolute();

        if normalized.parts == [PathPart::Root] {
            return None;
        }

        let mut parts = normalized.parts;
        while let Some(part) = parts.pop() {
            if matches!(part, PathPart::Normal(_)) {
                break;
            }
        }

        if parts.is_empty() && is_absolute {
            parts.push(PathPart::Root);
        }

        Some(Self {
            parts,
            ends_with_slash: false,
        })
    }

    pub fn starts_with(&self, prefix: &Self) -> bool {
        let path = self.normalize();
        let prefix = prefix.normalize();

        if prefix.parts.len() > path.parts.len() {
            return false;
        }

        path.parts
            .iter()
            .zip(prefix.parts.iter())
            .all(|(lhs, rhs)| lhs == rhs)
    }

    pub fn strip_prefix(&self, prefix: &Self) -> Option<Self> {
        let path = self.normalize();
        let prefix = prefix.normalize();

        if !path.starts_with(&prefix) {
            return None;
        }

        let remaining = path.parts[prefix.parts.len()..].to_vec();
        let mut parts = vec![PathPart::Root];
        parts.extend(remaining);

        Some(Self {
            ends_with_slash: path.ends_with_slash,
            parts,
        })
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
