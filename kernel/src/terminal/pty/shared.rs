use alloc::{collections::vec_deque::VecDeque, sync::Weak};

use crate::object::Object;

#[derive(Debug, Default)]
pub struct PtyShared {
    pub from_master: VecDeque<u8>,
    pub from_slave: VecDeque<u8>,
    pub master: Option<Weak<dyn Object>>,
    pub slave: Option<Weak<dyn Object>>,
}
