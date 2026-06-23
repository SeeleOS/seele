use serde::{Deserialize, Serialize};
use std::path::PathBuf;

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
}

impl TestSelector {
    pub fn parse(value: Option<&str>) -> anyhow::Result<Self> {
        match value.unwrap_or("default") {
            "" | "default" => Ok(Self::Default),
            "full" => Ok(Self::Full),
            "kernel_unit" | "kernel-unit" | "unit" => Ok(Self::KernelUnit),
            "ltp" => Ok(Self::Ltp),
            other => anyhow::bail!(
                "unknown test selector {other}; expected default, full, kernel_unit, or ltp"
            ),
        }
    }

    pub fn includes_kernel_unit(&self) -> bool {
        matches!(self, Self::Default | Self::Full | Self::KernelUnit)
    }

    pub fn includes_ltp(&self) -> bool {
        matches!(self, Self::Default | Self::Full | Self::Ltp)
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

#[cfg(test)]
mod tests {
    use super::TestSelector;

    #[test]
    fn parses_test_selectors() {
        assert_eq!(TestSelector::parse(None).unwrap(), TestSelector::Default);
        assert_eq!(
            TestSelector::parse(Some("kernel-unit")).unwrap(),
            TestSelector::KernelUnit
        );
        assert!(TestSelector::parse(Some("unknown")).is_err());
    }
}
