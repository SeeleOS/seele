use crate::{ControlContext, Event, RootfsEvent, StepState};
use anyhow::Result;

pub fn run_step(
    context: &dyn ControlContext,
    step: &str,
    f: impl FnOnce() -> Result<()>,
) -> Result<()> {
    context.event(Event::Rootfs(RootfsEvent {
        step: step.to_string(),
        state: StepState::Started,
        message: "started".to_string(),
    }));
    match f() {
        Ok(()) => {
            context.event(Event::Rootfs(RootfsEvent {
                step: step.to_string(),
                state: StepState::Finished,
                message: "finished".to_string(),
            }));
            Ok(())
        }
        Err(err) => {
            context.event(Event::Rootfs(RootfsEvent {
                step: step.to_string(),
                state: StepState::Failed,
                message: err.to_string(),
            }));
            Err(err)
        }
    }
}
