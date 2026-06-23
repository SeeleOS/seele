use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BuildRootfsConfig {
    pub override_rootfs: bool,
    pub rebuild_aur: bool,
    pub rebuild_aur_packages: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct RebuildAur {
    pub all: bool,
    pub packages: Vec<String>,
}

impl BuildRootfsConfig {
    pub fn rebuild_aur(&self) -> RebuildAur {
        let mut packages = self.rebuild_aur_packages.clone();
        packages.sort();
        packages.dedup();
        RebuildAur {
            all: self.rebuild_aur,
            packages,
        }
    }
}
