use alloc::vec::Vec;
use core::{mem, slice};

use super::{
    AF_UNIX, SO_ACCEPTCONN, SO_DOMAIN, SO_ERROR, SO_PASSCRED, SO_PEERCRED, SO_PROTOCOL, SO_RCVBUF,
    SO_REUSEADDR, SO_SNDBUF, SO_TYPE, SOCK_STREAM, SOL_SOCKET, SocketError, SocketResult,
    UnixSocketObject, UnixSocketState,
};

const DEFAULT_SOCKET_BUFFER_SIZE: i32 = 64 * 1024;

#[repr(C)]
#[derive(Clone, Copy)]
struct SocketUcred {
    pid: i32,
    uid: u32,
    gid: u32,
}

impl UnixSocketObject {
    pub fn setsockopt(
        &self,
        level: u64,
        option_name: u64,
        option_value: &[u8],
    ) -> SocketResult<()> {
        if level != SOL_SOCKET {
            return Err(SocketError::InvalidArguments);
        }

        match option_name {
            SO_REUSEADDR | SO_PASSCRED | SO_SNDBUF | SO_RCVBUF => {
                let _ = Self::decode_i32(option_value)?;
                Ok(())
            }
            SO_ERROR | SO_TYPE | SO_ACCEPTCONN | SO_DOMAIN | SO_PROTOCOL | SO_PEERCRED => {
                Err(SocketError::InvalidArguments)
            }
            _ => Err(SocketError::InvalidArguments),
        }
    }

    pub fn getsockopt(
        &self,
        level: u64,
        option_name: u64,
        option_len: usize,
    ) -> SocketResult<Vec<u8>> {
        if level != SOL_SOCKET {
            return Err(SocketError::InvalidArguments);
        }

        match option_name {
            SO_ERROR => Self::encode_i32(option_len, 0),
            SO_TYPE => Self::encode_i32(option_len, SOCK_STREAM as i32),
            SO_ACCEPTCONN => Self::encode_i32(
                option_len,
                matches!(&*self.state.lock(), UnixSocketState::Listener(_)) as i32,
            ),
            SO_DOMAIN => Self::encode_i32(option_len, AF_UNIX as i32),
            SO_PROTOCOL => Self::encode_i32(option_len, 0),
            SO_SNDBUF | SO_RCVBUF => Self::encode_i32(option_len, DEFAULT_SOCKET_BUFFER_SIZE),
            SO_REUSEADDR | SO_PASSCRED => Self::encode_i32(option_len, 0),
            SO_PEERCRED => match &*self.state.lock() {
                UnixSocketState::Stream(stream) => {
                    let cred = *stream.peer_cred.lock();
                    Self::encode_ucred(
                        option_len,
                        SocketUcred {
                            pid: i32::try_from(cred.pid).unwrap_or(i32::MAX),
                            uid: cred.uid,
                            gid: cred.gid,
                        },
                    )
                }
                _ => Err(SocketError::InvalidArguments),
            },
            _ => Err(SocketError::InvalidArguments),
        }
    }

    fn encode_i32(option_len: usize, value: i32) -> SocketResult<Vec<u8>> {
        if option_len < mem::size_of::<i32>() {
            return Err(SocketError::InvalidArguments);
        }

        Ok(value.to_ne_bytes().to_vec())
    }

    fn decode_i32(option_value: &[u8]) -> SocketResult<i32> {
        if option_value.len() < mem::size_of::<i32>() {
            return Err(SocketError::InvalidArguments);
        }

        Ok(i32::from_ne_bytes(
            option_value[..mem::size_of::<i32>()]
                .try_into()
                .map_err(|_| SocketError::InvalidArguments)?,
        ))
    }

    fn encode_ucred(option_len: usize, value: SocketUcred) -> SocketResult<Vec<u8>> {
        if option_len < mem::size_of::<SocketUcred>() {
            return Err(SocketError::InvalidArguments);
        }

        Ok(unsafe {
            slice::from_raw_parts(
                (&value as *const SocketUcred).cast::<u8>(),
                mem::size_of::<SocketUcred>(),
            )
        }
        .to_vec())
    }
}
