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

    fn path_from_components(components: &[String]) -> Path {
        if components.is_empty() {
            return Path::new("/");
        }

        let mut path = String::from("/");
        path.push_str(&components.join("/"));
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

            for index in 0..components.len() {
                let current = Self::path_from_components(&components[..=index]);
                let current_file = self.resolve_raw(current.clone())?;
                let is_final = index + 1 == components.len();

                match current_file {
                    FileLike::Symlink(symlink) if !is_final || follow_final_symlink => {
                        followed_symlinks += 1;
                        if followed_symlinks > MAX_SYMLINKS {
                            return Err(FSError::TooManySymlinks);
                        }

                        let remainder = &components[index + 1..];
                        path = Self::rewrite_symlink_target(
                            &current,
                            symlink.lock().target()?,
                            remainder,
                        );
                        continue 'restart;
                    }
                    FileLike::Directory(_) if !is_final => {}
                    FileLike::File(_) if !is_final => return Err(FSError::NotADirectory),
                    entry @ FileLike::Directory(_) if is_final => return Ok((entry, current)),
                    entry @ FileLike::File(_) if is_final => return Ok((entry, current)),
                    entry @ FileLike::Symlink(_) if is_final => return Ok((entry, current)),
                    FileLike::File(_) | FileLike::Directory(_) => unreachable!(),
                    FileLike::Symlink(_) => unreachable!(),
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

    fn find_mount(&self, path: &Path) -> FSResult<(&Mount, Path)> {
        for mount in &self.mounts {
            if let Some(stripped) = path.strip_prefix(&mount.path) {
                return Ok((mount, stripped));
            }
        }

        Err(FSError::NotFound)
    }
}
