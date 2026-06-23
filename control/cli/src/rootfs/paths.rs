use crate::target_dir;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct RootfsPaths {
    pub image: PathBuf,
    pub mount: PathBuf,
    pub artifact_dir: PathBuf,
}

pub fn paths(repo: &Path) -> RootfsPaths {
    let target = target_dir(repo);
    RootfsPaths {
        image: target.join("rootfs.img"),
        mount: target.join("rootfs_mnt"),
        artifact_dir: target.join("control-cli").join("rootfs"),
    }
}
