use seele_sys::abi::object::ObjectFlags;
use spin::Mutex;

use super::UnixSocketState;

#[derive(Debug)]
pub struct UnixSocketObject {
    pub state: Mutex<UnixSocketState>,
    pub flags: Mutex<ObjectFlags>,
}
