use alloc::{string::String, vec::Vec};

use crate::filesystem::{
    errors::FSError,
    path::{Path, PathPart},
    vfs::{FSResult, Mount, VFS},
    vfs_traits::FileLike,
};

impl VFS {
    fn resolve_raw(&self, path: Path) -> FSResult<FileLike> {
        let normalized_path = self.normalize_path(path);
        let (mount, mount_path) = self.find_mount(&normalized_path)?;
        mount.fs.lock().lookup(&mount_path)
    }

    fn append_component(base: &Path, component: &str) -> Path {
        if base.clone().as_string() == "/" {
            return Path::new(&alloc::format!("/{component}"));
        }

        let mut path = base.clone().as_string();
        if !path.ends_with('/') {
            path.push('/');
        }
        path.push_str(component);
        Path::new(&path).normalize()
    }

    fn rewrite_symlink_target(current: &Path, target: Path, remainder: &[String]) -> Path {
        let target = if target.is_absolute() {
            target
        } else {
            let mut combined = current.parent().unwrap_or_default().as_string();
            if !combined.ends_with('/') {
                combined.push('/');
            }
            combined.push_str(&target.as_string());
            Path::new(&combined)
        }
        .normalize();

        let mut combined = target.as_string();
        for component in remainder {
            if !combined.ends_with('/') {
                combined.push('/');
            }
            combined.push_str(component);
        }

        Path::new(&combined).normalize()
    }

    fn resolve_internal(
        &self,
        path: Path,
        follow_final_symlink: bool,
    ) -> FSResult<(FileLike, Path)> {
        const MAX_SYMLINKS: usize = 40;

        let mut path = self.normalize_path(path);
        let mut followed_symlinks = 0;

        'restart: loop {
            let had_trailing_slash = path.clone().as_string().ends_with('/');
            let normalized = path.normalize();
            let components = normalized
                .parts
                .iter()
                .filter_map(|part| match part {
                    PathPart::Normal(component) => Some(component.clone()),
                    _ => None,
                })
                .collect::<Vec<_>>();

            if components.is_empty() {
                return Ok((self.resolve_raw(Path::new("/"))?, Path::new("/")));
            }

            let mut current = self.resolve_raw(Path::new("/"))?;
            let mut current_path = Path::new("/");

            for (index, component) in components.iter().enumerate() {
                let is_final = index + 1 == components.len();
                let next_path = Self::append_component(&current_path, component);

                let next = match current {
                    FileLike::Directory(dir) => {
                        let (mount, _) = self.find_mount(&next_path)?;
                        if mount.path == next_path {
                            self.resolve_raw(next_path.clone())?
                        } else {
                            dir.lock().get(component)?
                        }
                    }
                    FileLike::File(_) => return Err(FSError::NotADirectory),
                    FileLike::Symlink(_) => unreachable!("path walk should not keep raw symlinks"),
                };

                match next {
                    FileLike::Symlink(symlink) if !is_final || follow_final_symlink => {
                        followed_symlinks += 1;
                        if followed_symlinks > MAX_SYMLINKS {
                            return Err(FSError::TooManySymlinks);
                        }

                        let remainder = &components[index + 1..];
                        path = Self::rewrite_symlink_target(
                            &next_path,
                            symlink.lock().target()?,
                            remainder,
                        );
                        continue 'restart;
                    }
                    FileLike::Directory(_) if !is_final => {
                        current = next;
                        current_path = next_path;
                    }
                    FileLike::File(_) if !is_final => return Err(FSError::NotADirectory),
                    entry @ FileLike::Directory(_) if is_final => return Ok((entry, next_path)),
                    entry @ FileLike::File(_) if is_final => {
                        if had_trailing_slash {
                            return Err(FSError::NotADirectory);
                        }
                        return Ok((entry, next_path));
                    }
                    entry @ FileLike::Symlink(_) if is_final => {
                        if had_trailing_slash {
                            followed_symlinks += 1;
                            if followed_symlinks > MAX_SYMLINKS {
                                return Err(FSError::TooManySymlinks);
                            }

                            let FileLike::Symlink(symlink) = entry else {
                                unreachable!();
                            };
                            path = Self::rewrite_symlink_target(
                                &next_path,
                                symlink.lock().target()?,
                                &[],
                            );
                            continue 'restart;
                        }
                        return Ok((entry, next_path));
                    }
                    FileLike::File(_) | FileLike::Directory(_) | FileLike::Symlink(_) => {
                        unreachable!()
                    }
                }
            }
        }
    }

    pub fn resolve(&self, path: Path) -> FSResult<FileLike> {
        self.resolve_internal(path, true).map(|(entry, _)| entry)
    }

    pub fn resolve_nofollow(&self, path: Path) -> FSResult<FileLike> {
        self.resolve_internal(path, false).map(|(entry, _)| entry)
    }

    pub fn resolve_with_path(&self, path: Path) -> FSResult<(FileLike, Path)> {
        self.resolve_internal(path, true)
    }

    pub fn resolve_nofollow_with_path(&self, path: Path) -> FSResult<(FileLike, Path)> {
        self.resolve_internal(path, false)
    }

    pub fn mount_path(&self, path: Path) -> FSResult<Path> {
        let normalized_path = self.normalize_path(path);
        let (mount, _) = self.find_mount(&normalized_path)?;
        Ok(mount.path.clone())
    }

    pub(super) fn find_mount(&self, path: &Path) -> FSResult<(&Mount, Path)> {
        for mount in &self.mounts {
            if let Some(stripped) = path.strip_prefix(&mount.path) {
                return Ok((mount, join_mount_source(&mount.source_path, &stripped)));
            }
        }

        Err(FSError::NotFound)
    }
}

fn join_mount_source(source: &Path, suffix: &Path) -> Path {
    let mut path = source.normalize().as_string();
    for part in suffix.normalize().parts {
        if let PathPart::Normal(component) = part {
            if !path.ends_with('/') {
                path.push('/');
            }
            path.push_str(&component);
        }
    }
    Path::new(&path).normalize()
}
