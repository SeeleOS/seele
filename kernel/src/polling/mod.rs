use crate::polling::poller::PollerObject;

pub mod event;
pub mod poller;
pub mod wake;

impl PollerObject {
    pub fn wait(&mut self) {}
}
