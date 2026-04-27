use alloc::{sync::Arc, vec::Vec};

use crate::object::{Object, misc::ObjectRef};

use super::SocketResult;

pub trait SocketLike: Object {
    fn bind_bytes(self: Arc<Self>, _address: &[u8]) -> SocketResult<()> {
        Err(super::SocketError::OperationNotSupported)
    }
    fn listen(self: Arc<Self>, _backlog: usize) -> SocketResult<()> {
        Err(super::SocketError::OperationNotSupported)
    }
    fn connect_bytes(self: Arc<Self>, _address: &[u8]) -> SocketResult<()> {
        Err(super::SocketError::OperationNotSupported)
    }
    fn accept(self: Arc<Self>) -> SocketResult<ObjectRef> {
        Err(super::SocketError::OperationNotSupported)
    }
    fn sendto(self: Arc<Self>, _buffer: &[u8], _address: Option<&[u8]>) -> SocketResult<usize> {
        Err(super::SocketError::OperationNotSupported)
    }
    fn recvfrom(&self, _buffer: &mut [u8]) -> SocketResult<(usize, Option<Vec<u8>>)> {
        Err(super::SocketError::OperationNotSupported)
    }
    fn recvfrom_with_flags(
        &self,
        buffer: &mut [u8],
        _flags: u64,
    ) -> SocketResult<(usize, Option<Vec<u8>>)> {
        self.recvfrom(buffer)
    }
    fn getsockname_bytes(&self) -> SocketResult<Vec<u8>>;
    fn getpeername_bytes(&self) -> SocketResult<Vec<u8>> {
        Err(super::SocketError::NotConnected)
    }
    fn shutdown(&self, _how: u64) -> SocketResult<()> {
        Err(super::SocketError::OperationNotSupported)
    }
    fn setsockopt(&self, level: u64, option_name: u64, option_value: &[u8]) -> SocketResult<()>;
    fn getsockopt(&self, level: u64, option_name: u64, option_len: usize) -> SocketResult<Vec<u8>>;
}
