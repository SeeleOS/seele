use spin::Mutex;

use crate::object::FileFlags;

use super::UnixSocketState;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnixSocketKind {
    Stream,
    Datagram,
}

#[derive(Debug)]
pub struct UnixSocketObject {
    pub kind: UnixSocketKind,
    pub state: Mutex<UnixSocketState>,
    pub flags: Mutex<FileFlags>,
}
