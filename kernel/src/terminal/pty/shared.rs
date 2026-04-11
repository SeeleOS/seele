use alloc::collections::vec_deque::VecDeque;

#[derive(Debug, Default)]
pub struct PtyShared {
    pub from_master: VecDeque<u8>,
    pub from_slave: VecDeque<u8>,
}
