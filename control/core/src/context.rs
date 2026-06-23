use crate::{Artifact, Event, Report};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

pub trait ControlContext: Send + Sync {
    fn is_cancelled(&self) -> bool {
        false
    }

    fn event(&self, event: Event);

    fn artifact(&self, artifact: Artifact);

    fn report(&self, report: Report);

    fn on_cancel(&self, cleanup: Box<dyn FnOnce() + Send>);
}

#[derive(Debug, Default)]
pub struct NoopContext;

impl ControlContext for NoopContext {
    fn event(&self, _event: Event) {}

    fn artifact(&self, _artifact: Artifact) {}

    fn report(&self, _report: Report) {}

    fn on_cancel(&self, _cleanup: Box<dyn FnOnce() + Send>) {}
}

#[derive(Debug, Clone, Default)]
pub struct ConsoleContext {
    cancelled: Arc<AtomicBool>,
}

impl ConsoleContext {
    pub fn new() -> Self {
        Self::default()
    }
}

impl ControlContext for ConsoleContext {
    fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Relaxed)
    }

    fn event(&self, event: Event) {
        match event {
            Event::Progress { message, .. } => eprintln!("==> {message}"),
            Event::Rootfs(event) => {
                if matches!(event.state, crate::StepState::Started) {
                    eprintln!("==> {}", event.step.replace('_', " "));
                }
            }
            Event::Test(crate::TestEvent::Finished {
                passed,
                failed,
                skipped,
            }) => {
                eprintln!("test summary: {passed} passed, {failed} failed, {skipped} skipped");
            }
            _ => {}
        }
    }

    fn artifact(&self, artifact: Artifact) {
        match artifact.kind {
            crate::ArtifactKind::SerialLog => {
                eprintln!("    serial log: {}", artifact.path.display())
            }
            crate::ArtifactKind::IsoImage => eprintln!("    ISO: {}", artifact.path.display()),
            crate::ArtifactKind::RootfsImage => {
                eprintln!("rootfs image: {}", artifact.path.display())
            }
            _ => {}
        }
    }

    fn report(&self, report: Report) {
        if let Report::Ltp(report) = report {
            eprintln!(
                "LTP summary: {} passed, {} failed, {} skipped",
                report.passed, report.failed, report.skipped
            );
        }
    }

    fn on_cancel(&self, _cleanup: Box<dyn FnOnce() + Send>) {}
}
