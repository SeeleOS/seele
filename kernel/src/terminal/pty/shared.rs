use alloc::{collections::vec_deque::VecDeque, sync::Weak};
use seele_sys::abi::object::TerminalInfo;

use crate::{object::{Object, misc::ObjectRef}, process::group::ProcessGroupID};

#[derive(Debug)]
pub struct PtyShared {
    pub from_master: VecDeque<u8>,
    pub from_slave: VecDeque<u8>,
    pub line_buffer: VecDeque<u8>,
    pub info: TerminalInfo,
    pub active_group: Option<ProcessGroupID>,
    pub master: Option<Weak<dyn Object>>,
    pub slave: Option<Weak<dyn Object>>,
}

impl Default for PtyShared {
    fn default() -> Self {
        Self {
            from_master: VecDeque::new(),
            from_slave: VecDeque::new(),
            line_buffer: VecDeque::new(),
            info: TerminalInfo::default(),
            active_group: None,
            master: None,
            slave: None,
        }
    }
}

impl PtyShared {
    pub fn get_master(&self) -> ObjectRef {
        self.master.as_ref().unwrap().upgrade().unwrap()
    }

    pub fn get_slave(&self) -> ObjectRef {
        self.slave.as_ref().unwrap().upgrade().unwrap()
    }
}
