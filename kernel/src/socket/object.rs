use spin::Mutex;

use crate::object::FileFlags;

use super::UnixSocketState;

#[derive(Debug)]
pub struct UnixSocketObject {
    pub state: Mutex<UnixSocketState>,
    pub flags: Mutex<FileFlags>,
}
