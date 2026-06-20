pub mod job;
pub mod plane;
pub mod process;
pub mod qemu;
pub mod rootfs;
pub mod tests;

use std::path::{Path, PathBuf};

pub use job::{
    Artifact, ArtifactKind, BuildEvent, Event, JobContext, JobKind, JobManager, JobState,
    JobStatus, KernelUnitReport, LtpCase, LtpReport, Report, RootfsEvent, StepState, TestEvent,
    TestSelector, VmEvent, VmSmokeReport,
};

pub fn repo_root() -> anyhow::Result<PathBuf> {
    Ok(std::env::current_dir()?)
}

pub fn target_dir(repo: &Path) -> PathBuf {
    repo.join("target")
}
