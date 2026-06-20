use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BuildRootfsConfig {
    pub override_rootfs: bool,
    pub rebuild_aur: bool,
    pub rebuild_aur_packages: Vec<String>,
}
