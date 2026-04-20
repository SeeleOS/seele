use alloc::vec::Vec;

use crate::object::Object;

use super::SocketResult;

pub trait SocketLike: Object {
    fn getsockname_bytes(&self) -> SocketResult<Vec<u8>>;
    fn setsockopt(&self, level: u64, option_name: u64, option_value: &[u8]) -> SocketResult<()>;
    fn getsockopt(&self, level: u64, option_name: u64, option_len: usize) -> SocketResult<Vec<u8>>;
}
