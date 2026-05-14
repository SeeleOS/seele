use alloc::vec::Vec;

use crate::filesystem::{
    errors::FSError,
    path::{Path, PathPart},
    vfs::{FSResult, FileSystemRef, Mount, VFS, VirtualFS},
    vfs_traits::FileLike,
};

type MountSnapshot = (Path, FileSystemRef, Path, u64);

impl VFS {
    fn resolve_raw(&self, path: Path) -> FSResult<FileLike> {
        let normalized_path = self.normalize_path(path);
        let (mount, mount_path) = self.find_mount(&normalized_path)?;
        mount.fs.lock().lookup(&mount_path)
    }

    fn append_component(base: &Path, component: &str) -> Path {
        base.join_component(component)
    }

    fn rewrite_symlink_target(current: &Path, target: Path, remainder: &[&str]) -> Path {
        let target = if target.is_absolute() {
            target
        } else {
            current.parent().unwrap_or_default().join_path(&target)
        }
        .normalize();

        let mut combined = target;
        for component in remainder {
            combined = combined.join_component(component);
        }

        combined.normalize()
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
                    PathPart::Normal(component) => Some(component.as_str()),
                    _ => None,
                })
                .collect::<alloc::vec::Vec<_>>();

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

pub fn resolve_path(path: Path, follow_final_symlink: bool) -> FSResult<(FileLike, Path)> {
    let mounts = mount_snapshots();
    resolve_with_mounts(path, follow_final_symlink, &mounts)
}

pub fn resolve_path_with_mount_device_id(
    path: Path,
    follow_final_symlink: bool,
) -> FSResult<(FileLike, Path, u64)> {
    let mounts = mount_snapshots();
    let (file, resolved_path) = resolve_with_mounts(path, follow_final_symlink, &mounts)?;
    let device_id = mount_device_id_in_snapshots(&resolved_path, &mounts)?;
    Ok((file, resolved_path, device_id))
}

fn mount_snapshots() -> Vec<MountSnapshot> {
    VirtualFS
        .lock()
        .mount_snapshots()
        .into_iter()
        .map(|(path, fs, source_path, _, device_id)| (path, fs, source_path, device_id))
        .collect()
}

fn find_mount_in_snapshots(path: &Path, mounts: &[MountSnapshot]) -> FSResult<MountSnapshot> {
    for (mount_path, fs, source_path, device_id) in mounts {
        if let Some(stripped) = path.strip_prefix(mount_path) {
            return Ok((
                mount_path.clone(),
                fs.clone(),
                join_mount_source(source_path, &stripped),
                *device_id,
            ));
        }
    }

    Err(FSError::NotFound)
}

fn resolve_raw_with_mounts(path: Path, mounts: &[MountSnapshot]) -> FSResult<FileLike> {
    let normalized_path = path.normalize();
    let (_, fs, mount_path, _) = find_mount_in_snapshots(&normalized_path, mounts)?;
    fs.lock().lookup(&mount_path)
}

fn mount_device_id_in_snapshots(path: &Path, mounts: &[MountSnapshot]) -> FSResult<u64> {
    let normalized_path = path.normalize();
    let (_, _, _, device_id) = find_mount_in_snapshots(&normalized_path, mounts)?;
    Ok(device_id)
}

fn resolve_with_mounts(
    path: Path,
    follow_final_symlink: bool,
    mounts: &[MountSnapshot],
) -> FSResult<(FileLike, Path)> {
    const MAX_SYMLINKS: usize = 40;

    let mut path = path.normalize();
    let mut followed_symlinks = 0;

    'restart: loop {
        let had_trailing_slash = path.clone().as_string().ends_with('/');
        let normalized = path.normalize();
        let components = normalized
            .parts
            .iter()
            .filter_map(|part| match part {
                PathPart::Normal(component) => Some(component.as_str()),
                _ => None,
            })
            .collect::<alloc::vec::Vec<_>>();

        if components.is_empty() {
            return Ok((
                resolve_raw_with_mounts(Path::new("/"), mounts)?,
                Path::new("/"),
            ));
        }

        let mut current = resolve_raw_with_mounts(Path::new("/"), mounts)?;
        let mut current_path = Path::new("/");

        for (index, component) in components.iter().enumerate() {
            let is_final = index + 1 == components.len();
            let next_path = VFS::append_component(&current_path, component);

                let next = match current {
                    FileLike::Directory(dir) => {
                        let (mount_path, _, _, _) = find_mount_in_snapshots(&next_path, mounts)?;
                        if mount_path == next_path {
                            resolve_raw_with_mounts(next_path.clone(), mounts)?
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
                    path = VFS::rewrite_symlink_target(
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
                        path =
                            VFS::rewrite_symlink_target(&next_path, symlink.lock().target()?, &[]);
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

fn join_mount_source(source: &Path, suffix: &Path) -> Path {
    source.join_path(suffix)
}
