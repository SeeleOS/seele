use alloc::vec::Vec;
use spin::Mutex;

use crate::{
    impl_cast_function_non_trait,
    object::Object,
    polling::{PollerEntry, PollerReadyEvent},
};

#[derive(Debug)]
pub struct PollerObject {
    // Registered objects that will notify the poller when an event is triggered.
    pub entries: Mutex<Vec<PollerEntry>>,
    // Events collected for the next poller_wait call.
    pub woken_events: Mutex<Vec<PollerReadyEvent>>,
}

impl PollerObject {
    pub fn new() -> Self {
        Self {
            entries: Mutex::new(Vec::new()),
            woken_events: Mutex::new(Vec::new()),
        }
    }
}

impl Object for PollerObject {
    impl_cast_function_non_trait!(poller, PollerObject);
}
