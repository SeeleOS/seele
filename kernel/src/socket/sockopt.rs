use alloc::vec::Vec;
use core::{mem, slice};

use seele_sys::abi::socket;

use super::{SocketError, SocketResult, UnixSocketObject, UnixSocketState};

const DEFAULT_SOCKET_BUFFER_SIZE: i32 = 64 * 1024;

#[repr(C)]
#[derive(Clone, Copy)]
struct SocketUcred {
    pid: i32,
    uid: u32,
    gid: u32,
}

impl UnixSocketObject {
    pub fn getsockopt(
        &self,
        level: u64,
        option_name: u64,
        option_len: usize,
    ) -> SocketResult<Vec<u8>> {
        if level != socket::SOL_SOCKET {
            return Err(SocketError::InvalidArguments);
        }

        match option_name {
            socket::SO_ERROR => Self::encode_i32(option_len, 0),
            socket::SO_TYPE => Self::encode_i32(option_len, socket::SOCK_STREAM as i32),
            socket::SO_ACCEPTCONN => Self::encode_i32(
                option_len,
                matches!(&*self.state.lock(), UnixSocketState::Listener(_)) as i32,
            ),
            socket::SO_DOMAIN => Self::encode_i32(option_len, socket::AF_UNIX as i32),
            socket::SO_PROTOCOL => Self::encode_i32(option_len, 0),
            socket::SO_SNDBUF | socket::SO_RCVBUF => {
                Self::encode_i32(option_len, DEFAULT_SOCKET_BUFFER_SIZE)
            }
            socket::SO_REUSEADDR | socket::SO_PASSCRED => Self::encode_i32(option_len, 0),
            socket::SO_PEERCRED => match &*self.state.lock() {
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
