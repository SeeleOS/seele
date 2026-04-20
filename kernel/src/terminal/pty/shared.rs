use alloc::{collections::vec_deque::VecDeque, sync::Weak};

use crate::{
    object::{Object, misc::ObjectRef},
    process::group::ProcessGroupID,
    terminal::object::TerminalSettings,
};

#[derive(Debug, Default)]
pub struct PtyShared {
    pub from_master: VecDeque<u8>,
    pub from_slave: VecDeque<u8>,
    pub line_buffer: VecDeque<u8>,
    pub info: TerminalSettings,
    pub active_group: Option<ProcessGroupID>,
    pub master: Option<Weak<dyn Object>>,
    pub slave: Option<Weak<dyn Object>>,
}

impl PtyShared {
    pub fn get_master(&self) -> ObjectRef {
        self.master.as_ref().unwrap().upgrade().unwrap()
    }

    pub fn get_slave(&self) -> ObjectRef {
        self.slave.as_ref().unwrap().upgrade().unwrap()
    }
}
