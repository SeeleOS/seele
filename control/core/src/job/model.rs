use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobKind {
    BuildKernel,
    BuildIso,
    BuildRootfs,
    RunVm,
    RunTests,
    Cleanup,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobState {
    Queued,
    Running,
    Finished,
    Failed,
    Cancelled,
    TimedOut,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobStatus {
    pub id: u64,
    pub kind: JobKind,
    pub state: JobState,
    pub exit_code: Option<i32>,
    pub created_at: DateTime<Utc>,
    pub started_at: Option<DateTime<Utc>>,
    pub finished_at: Option<DateTime<Utc>>,
    pub events: Vec<Event>,
    pub reports: Vec<Report>,
    pub artifacts: Vec<Artifact>,
    pub error: Option<String>,
}

impl JobStatus {
    pub fn new(id: u64, kind: JobKind) -> Self {
        Self {
            id,
            kind,
            state: JobState::Queued,
            exit_code: None,
            created_at: Utc::now(),
            started_at: None,
            finished_at: None,
            events: Vec::new(),
            reports: Vec::new(),
            artifacts: Vec::new(),
            error: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Artifact {
    pub kind: ArtifactKind,
    pub path: PathBuf,
    pub description: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactKind {
    CargoJson,
    StdoutLog,
    StderrLog,
    SerialLog,
    QmpTranscript,
    Screenshot,
    KirkJson,
    RootfsImage,
    IsoImage,
    Other,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Event {
    Progress { stage: String, message: String },
    Build(BuildEvent),
    Rootfs(RootfsEvent),
    Vm(VmEvent),
    Test(TestEvent),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum BuildEvent {
    CargoArtifact {
        package: String,
        target: String,
        executable: Option<PathBuf>,
    },
    Diagnostic {
        level: String,
        message: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RootfsEvent {
    pub step: String,
    pub state: StepState,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StepState {
    Started,
    Finished,
    Failed,
    Skipped,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum VmEvent {
    Started {
        runner_pid: u32,
        qemu_pid: Option<u32>,
        qmp_socket: PathBuf,
        serial_log: PathBuf,
    },
    Stopped,
    SerialMatched {
        pattern: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TestEvent {
    Started {
        selector: TestSelector,
    },
    Case {
        name: String,
        status: TestCaseStatus,
    },
    Finished {
        passed: u64,
        failed: u64,
        skipped: u64,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TestCaseStatus {
    Running,
    Passed,
    Failed,
    Broken,
    Skipped,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TestSelector {
    Default,
    Full,
    KernelUnit,
    Ltp,
    Integration(String),
}

impl TestSelector {
    pub fn parse(value: Option<&str>) -> Self {
        match value {
            None | Some("") | Some("default") => Self::Default,
            Some("full") => Self::Full,
            Some("kernel_unit") | Some("kernel-unit") => Self::KernelUnit,
            Some("ltp") => Self::Ltp,
            Some(value) if value.starts_with("integration:") => {
                Self::Integration(value["integration:".len()..].to_string())
            }
            Some(value) => Self::Integration(value.to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::TestSelector;

    #[test]
    fn parses_test_selectors() {
        assert_eq!(TestSelector::parse(None), TestSelector::Default);
        assert_eq!(TestSelector::parse(Some("full")), TestSelector::Full);
        assert_eq!(
            TestSelector::parse(Some("kernel_unit")),
            TestSelector::KernelUnit
        );
        assert_eq!(
            TestSelector::parse(Some("integration:panic_handler_smoke")),
            TestSelector::Integration("panic_handler_smoke".to_string())
        );
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Report {
    KernelUnit(KernelUnitReport),
    Ltp(LtpReport),
    VmSmoke(VmSmokeReport),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KernelUnitReport {
    pub executable: PathBuf,
    pub iso: Option<PathBuf>,
    pub passed: bool,
    pub serial_log: Option<PathBuf>,
    pub stdout: String,
    pub stderr: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LtpReport {
    pub suite: Option<String>,
    pub pattern: Option<String>,
    pub passed: u64,
    pub failed: u64,
    pub skipped: u64,
    pub cases: Vec<LtpCase>,
    pub artifact: Option<PathBuf>,
    pub stdout: String,
    pub stderr: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LtpCase {
    pub name: String,
    pub status: String,
    pub duration_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VmSmokeReport {
    pub booted: bool,
    pub qmp_connectable: bool,
    pub serial_log: PathBuf,
    pub screenshot: Option<PathBuf>,
}
