use chrono::{DateTime, Utc};
use control_core::{Artifact, Event, Report};
use serde::{Deserialize, Serialize};

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
