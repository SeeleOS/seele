use std::path::{Path, PathBuf};

pub fn repo_root() -> anyhow::Result<PathBuf> {
    Ok(std::env::current_dir()?)
}

pub fn target_dir(repo: &Path) -> PathBuf {
    repo.join("target")
}
