use super::model::{Artifact, Event, JobKind, JobState, JobStatus, Report};
use anyhow::{Result, anyhow};
use chrono::Utc;
use std::{
    collections::HashMap,
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

#[derive(Debug, Clone, Default)]
pub struct JobManager {
    inner: Arc<Inner>,
}

#[derive(Debug, Default)]
struct Inner {
    next_id: AtomicU64,
    jobs: Mutex<HashMap<u64, JobEntry>>,
    changed: Condvar,
}

#[derive(Debug)]
struct JobEntry {
    status: JobStatus,
    cancel: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

#[derive(Debug, Clone)]
pub struct JobContext {
    id: u64,
    inner: Arc<Inner>,
    cancel: Arc<AtomicBool>,
}

impl JobManager {
    pub fn start<F>(&self, kind: JobKind, run: F) -> JobStatus
    where
        F: FnOnce(JobContext) -> Result<i32> + Send + 'static,
    {
        let id = self.inner.next_id.fetch_add(1, Ordering::Relaxed) + 1;
        let cancel = Arc::new(AtomicBool::new(false));
        let status = JobStatus::new(id, kind);
        let context = JobContext {
            id,
            inner: self.inner.clone(),
            cancel: cancel.clone(),
        };

        {
            let mut jobs = self.inner.jobs.lock().expect("job lock poisoned");
            jobs.insert(
                id,
                JobEntry {
                    status: status.clone(),
                    cancel,
                    handle: None,
                },
            );
        }

        let inner = self.inner.clone();
        let handle = thread::spawn(move || {
            context.mark_running();
            let result = run(context.clone());
            let mut jobs = inner.jobs.lock().expect("job lock poisoned");
            if let Some(entry) = jobs.get_mut(&id) {
                entry.status.finished_at = Some(Utc::now());
                match result {
                    Ok(exit_code) if context.is_cancelled() => {
                        entry.status.state = JobState::Cancelled;
                        entry.status.exit_code = Some(exit_code);
                    }
                    Ok(0) => {
                        entry.status.state = JobState::Finished;
                        entry.status.exit_code = Some(0);
                    }
                    Ok(exit_code) => {
                        entry.status.state = JobState::Failed;
                        entry.status.exit_code = Some(exit_code);
                    }
                    Err(err) if context.is_cancelled() => {
                        entry.status.state = JobState::Cancelled;
                        entry.status.error = Some(err.to_string());
                    }
                    Err(err) => {
                        entry.status.state = JobState::Failed;
                        entry.status.exit_code = Some(1);
                        entry.status.error = Some(format!("{err:#}"));
                    }
                }
            }
            inner.changed.notify_all();
        });

        let mut jobs = self.inner.jobs.lock().expect("job lock poisoned");
        if let Some(entry) = jobs.get_mut(&id) {
            entry.handle = Some(handle);
            entry.status.clone()
        } else {
            status
        }
    }

    pub fn status(&self, id: u64) -> Result<JobStatus> {
        let jobs = self.inner.jobs.lock().expect("job lock poisoned");
        jobs.get(&id)
            .map(|entry| entry.status.clone())
            .ok_or_else(|| anyhow!("unknown job id {id}"))
    }

    pub fn wait(&self, id: u64, timeout_ms: Option<u64>) -> Result<JobStatus> {
        let deadline = timeout_ms.map(|ms| Instant::now() + Duration::from_millis(ms));
        let mut jobs = self.inner.jobs.lock().expect("job lock poisoned");
        loop {
            let state = jobs
                .get(&id)
                .ok_or_else(|| anyhow!("unknown job id {id}"))?
                .status
                .state;
            if matches!(
                state,
                JobState::Finished | JobState::Failed | JobState::Cancelled | JobState::TimedOut
            ) {
                let handle = jobs.get_mut(&id).and_then(|entry| entry.handle.take());
                let status = jobs.get(&id).expect("job disappeared").status.clone();
                drop(jobs);
                if let Some(handle) = handle {
                    let _ = handle.join();
                }
                return Ok(status);
            }

            if let Some(deadline) = deadline {
                let now = Instant::now();
                if now >= deadline {
                    return Ok(jobs.get(&id).expect("job disappeared").status.clone());
                }
                let wait_for = deadline.saturating_duration_since(now);
                let (next_jobs, _) = self
                    .inner
                    .changed
                    .wait_timeout(jobs, wait_for)
                    .expect("job condvar poisoned");
                jobs = next_jobs;
            } else {
                jobs = self.inner.changed.wait(jobs).expect("job condvar poisoned");
            }
        }
    }

    pub fn cancel(&self, id: u64) -> Result<JobStatus> {
        let mut jobs = self.inner.jobs.lock().expect("job lock poisoned");
        let entry = jobs
            .get_mut(&id)
            .ok_or_else(|| anyhow!("unknown job id {id}"))?;
        entry.cancel.store(true, Ordering::Relaxed);
        if !matches!(
            entry.status.state,
            JobState::Finished | JobState::Failed | JobState::Cancelled | JobState::TimedOut
        ) {
            entry.status.state = JobState::Cancelled;
            entry.status.finished_at = Some(Utc::now());
        }
        self.inner.changed.notify_all();
        Ok(entry.status.clone())
    }
}

impl JobContext {
    pub fn id(&self) -> u64 {
        self.id
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }

    pub fn mark_running(&self) {
        self.update(|status| {
            status.state = JobState::Running;
            status.started_at = Some(Utc::now());
        });
    }

    pub fn event(&self, event: Event) {
        self.update(|status| status.events.push(event));
    }

    pub fn artifact(&self, artifact: Artifact) {
        self.update(|status| status.artifacts.push(artifact));
    }

    pub fn report(&self, report: Report) {
        self.update(|status| status.reports.push(report));
    }

    fn update(&self, f: impl FnOnce(&mut JobStatus)) {
        let mut jobs = self.inner.jobs.lock().expect("job lock poisoned");
        if let Some(entry) = jobs.get_mut(&self.id) {
            f(&mut entry.status);
        }
        self.inner.changed.notify_all();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Event, JobKind};

    #[test]
    fn job_reaches_finished_state_with_events() {
        let manager = JobManager::default();
        let status = manager.start(JobKind::Cleanup, |context| {
            context.event(Event::Progress {
                stage: "test".to_string(),
                message: "running".to_string(),
            });
            Ok(0)
        });

        let status = manager.wait(status.id, Some(5_000)).unwrap();
        assert_eq!(status.state, JobState::Finished);
        assert_eq!(status.exit_code, Some(0));
        assert_eq!(status.events.len(), 1);
    }

    #[test]
    fn cancel_marks_pending_job_cancelled() {
        let manager = JobManager::default();
        let status = manager.start(JobKind::Cleanup, |context| {
            while !context.is_cancelled() {
                std::thread::sleep(Duration::from_millis(1));
            }
            Ok(0)
        });

        let status = manager.cancel(status.id).unwrap();
        assert_eq!(status.state, JobState::Cancelled);
        let status = manager.wait(status.id, Some(5_000)).unwrap();
        assert_eq!(status.state, JobState::Cancelled);
    }
}
