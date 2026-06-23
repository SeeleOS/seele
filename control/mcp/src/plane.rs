use crate::{JobKind, JobManager};
use anyhow::Result;
use control_core::{
    rootfs::{self, BuildRootfsConfig},
    tests::{self, RunTestsConfig},
    vm::{self, VmConfig},
};
use std::{path::PathBuf, sync::Arc};

#[derive(Debug, Clone)]
pub struct ControlPlane {
    repo: Arc<PathBuf>,
    jobs: JobManager,
}

impl ControlPlane {
    pub fn new(repo: impl Into<PathBuf>) -> Self {
        Self {
            repo: Arc::new(repo.into()),
            jobs: JobManager::default(),
        }
    }

    pub fn jobs(&self) -> &JobManager {
        &self.jobs
    }

    pub fn start_build_rootfs(&self, config: BuildRootfsConfig) -> crate::JobStatus {
        let repo = self.repo.clone();
        self.jobs.start(JobKind::BuildRootfs, move |context| {
            rootfs::build_rootfs(&repo, &config, &context)
        })
    }

    pub fn start_tests(&self, config: RunTestsConfig) -> crate::JobStatus {
        let repo = self.repo.clone();
        self.jobs.start(JobKind::RunTests, move |context| {
            tests::run_tests(&repo, &config, &context)
        })
    }

    pub fn start_vm(&self, mut config: VmConfig) -> crate::JobStatus {
        let repo = self.repo.clone();
        if config.rootfs_image.as_os_str().is_empty() {
            config = VmConfig::for_repo(&repo);
        }
        self.jobs.start(JobKind::RunVm, move |context| {
            vm::start_vm(&repo, config, &context)
        })
    }

    pub fn stop_vm(&self) -> crate::JobStatus {
        let repo = self.repo.clone();
        self.jobs.start(JobKind::Cleanup, move |context| {
            vm::stop_vm(&repo, &context)
        })
    }

    pub fn ensure_rootfs_mounted(&self) -> crate::JobStatus {
        let repo = self.repo.clone();
        self.jobs.start(JobKind::BuildRootfs, move |context| {
            rootfs::ensure_mounted(&repo, &context)?;
            Ok(0)
        })
    }

    pub fn unmount_rootfs(&self) -> crate::JobStatus {
        let repo = self.repo.clone();
        self.jobs.start(JobKind::Cleanup, move |context| {
            rootfs::unmount(&repo, &context)
        })
    }

    pub fn vm_status(&self) -> vm::VmStatus {
        vm::vm_status(&self.repo)
    }

    pub fn serial_tail(&self, lines: Option<usize>, bytes: Option<usize>) -> Result<String> {
        vm::serial_tail(&self.repo, lines, bytes)
    }

    pub fn wait_serial(&self, pattern: String, timeout_ms: Option<u64>) -> crate::JobStatus {
        let repo = self.repo.clone();
        self.jobs.start(JobKind::RunVm, move |context| {
            vm::wait_serial(&repo, &pattern, timeout_ms, &context)?;
            Ok(0)
        })
    }

    pub fn screenshot(&self) -> Result<PathBuf> {
        vm::screenshot(&self.repo)
    }

    pub fn send_key(&self, keys: &[String]) -> Result<()> {
        vm::send_key(&self.repo, keys)
    }

    pub fn type_text(&self, text: &str) -> Result<()> {
        vm::type_text(&self.repo, text)
    }

    pub fn mouse_move(&self, x: i64, y: i64) -> Result<()> {
        vm::mouse_move(&self.repo, x, y)
    }

    pub fn mouse_click(&self, button: &str) -> Result<()> {
        vm::mouse_click(&self.repo, button)
    }
}
