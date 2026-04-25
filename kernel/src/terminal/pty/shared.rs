use alloc::{collections::vec_deque::VecDeque, sync::Weak};

use crate::{
    object::config::{LinuxTermios2, LinuxWinsize},
    object::{Object, misc::ObjectRef},
    process::group::ProcessGroupID,
};

#[derive(Debug)]
pub struct PtyShared {
    pub from_master: VecDeque<u8>,
    pub from_slave: VecDeque<u8>,
    pub line_buffer: VecDeque<u8>,
    pub termios: LinuxTermios2,
    pub winsize: LinuxWinsize,
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
            termios: LinuxTermios2::new_default(),
            winsize: LinuxWinsize::default_terminal_size(),
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
