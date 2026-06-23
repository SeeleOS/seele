use super::model::{JobKind, JobState, JobStatus};
use anyhow::{Result, anyhow};
use chrono::Utc;
use control_core::{Artifact, ControlContext, Event, Report};
use std::{
    collections::HashMap,
    fmt,
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

struct JobEntry {
    status: JobStatus,
    cancel: Arc<AtomicBool>,
    cancel_cleanup: Vec<Box<dyn FnOnce() + Send>>,
    handle: Option<JoinHandle<()>>,
}

impl fmt::Debug for JobEntry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("JobEntry")
            .field("status", &self.status)
            .field("cancel", &self.cancel)
            .field("cancel_cleanup_count", &self.cancel_cleanup.len())
            .field("handle", &self.handle.is_some())
            .finish()
    }
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
                    cancel_cleanup: Vec::new(),
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
                entry.cancel_cleanup.clear();
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
        let active = !matches!(
            entry.status.state,
            JobState::Finished | JobState::Failed | JobState::Cancelled | JobState::TimedOut
        );
        entry.cancel.store(true, Ordering::Relaxed);
        let cancel_cleanup = if active {
            std::mem::take(&mut entry.cancel_cleanup)
        } else {
            Vec::new()
        };
        if active {
            entry.status.state = JobState::Cancelled;
            entry.status.finished_at = Some(Utc::now());
        }
        let status = entry.status.clone();
        self.inner.changed.notify_all();
        drop(jobs);
        for cleanup in cancel_cleanup {
            cleanup();
        }
        Ok(status)
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

    pub fn on_cancel(&self, cleanup: impl FnOnce() + Send + 'static) {
        let mut jobs = self.inner.jobs.lock().expect("job lock poisoned");
        if let Some(entry) = jobs.get_mut(&self.id) {
            entry.cancel_cleanup.push(Box::new(cleanup));
        }
    }

    fn update(&self, f: impl FnOnce(&mut JobStatus)) {
        let mut jobs = self.inner.jobs.lock().expect("job lock poisoned");
        if let Some(entry) = jobs.get_mut(&self.id) {
            f(&mut entry.status);
        }
        self.inner.changed.notify_all();
    }
}

impl ControlContext for JobContext {
    fn is_cancelled(&self) -> bool {
        self.is_cancelled()
    }

    fn event(&self, event: Event) {
        self.event(event);
    }

    fn artifact(&self, artifact: Artifact) {
        self.artifact(artifact);
    }

    fn report(&self, report: Report) {
        self.report(report);
    }

    fn on_cancel(&self, cleanup: Box<dyn FnOnce() + Send>) {
        let mut jobs = self.inner.jobs.lock().expect("job lock poisoned");
        if let Some(entry) = jobs.get_mut(&self.id) {
            entry.cancel_cleanup.push(cleanup);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Event, JobKind};
    use std::sync::atomic::{AtomicBool, AtomicUsize};

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

    #[test]
    fn cancel_runs_cleanup_once_for_active_job() {
        let manager = JobManager::default();
        let cleanup_registered = Arc::new(AtomicBool::new(false));
        let cleanup_count = Arc::new(AtomicUsize::new(0));
        let cleanup_registered_for_job = cleanup_registered.clone();
        let cleanup_count_for_job = cleanup_count.clone();
        let status = manager.start(JobKind::Cleanup, move |context| {
            context.on_cancel(move || {
                cleanup_count_for_job.fetch_add(1, Ordering::Relaxed);
            });
            cleanup_registered_for_job.store(true, Ordering::Relaxed);
            while !context.is_cancelled() {
                std::thread::sleep(Duration::from_millis(1));
            }
            Ok(0)
        });

        while !cleanup_registered.load(Ordering::Relaxed) {
            std::thread::sleep(Duration::from_millis(1));
        }

        let status = manager.cancel(status.id).unwrap();
        assert_eq!(status.state, JobState::Cancelled);
        assert_eq!(cleanup_count.load(Ordering::Relaxed), 1);
        let status = manager.wait(status.id, Some(5_000)).unwrap();
        assert_eq!(status.state, JobState::Cancelled);
        let status = manager.cancel(status.id).unwrap();
        assert_eq!(status.state, JobState::Cancelled);
        assert_eq!(cleanup_count.load(Ordering::Relaxed), 1);
    }
}
