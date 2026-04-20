use alloc::vec::Vec;
use core::{mem, slice};

use super::{
    AF_UNIX, SO_ACCEPTCONN, SO_DOMAIN, SO_ERROR, SO_PASSCRED, SO_PASSPIDFD, SO_PASSRIGHTS,
    SO_PASSSEC, SO_PEERCRED, SO_PROTOCOL, SO_RCVBUF, SO_RCVBUFFORCE, SO_REUSEADDR, SO_SNDBUF,
    SO_SNDBUFFORCE, SO_TIMESTAMP_NEW, SO_TIMESTAMP_OLD, SO_TIMESTAMPNS_NEW, SO_TIMESTAMPNS_OLD,
    SO_TYPE, SOCK_DGRAM, SOCK_SEQPACKET, SOCK_STREAM, SOL_SOCKET, SocketError, SocketLike,
    SocketResult, UnixSocketKind, UnixSocketObject, UnixSocketState,
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
    fn is_boolean_sockopt(option_name: u64) -> bool {
        matches!(
            option_name,
            SO_PASSCRED
                | SO_PASSSEC
                | SO_PASSRIGHTS
                | SO_PASSPIDFD
                | SO_TIMESTAMP_OLD
                | SO_TIMESTAMP_NEW
                | SO_TIMESTAMPNS_OLD
                | SO_TIMESTAMPNS_NEW
        )
    }

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
            SO_REUSEADDR | SO_SNDBUF | SO_RCVBUF | SO_SNDBUFFORCE | SO_RCVBUFFORCE => {
                let _ = Self::decode_i32(option_value)?;
                Ok(())
            }
            SO_PASSCRED => {
                *self.pass_cred.lock() = Self::decode_i32(option_value)? != 0;
                Ok(())
            }
            option_name if Self::is_boolean_sockopt(option_name) => {
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
            SO_TYPE => Self::encode_i32(
                option_len,
                match self.kind {
                    UnixSocketKind::Stream => SOCK_STREAM as i32,
                    UnixSocketKind::Datagram => SOCK_DGRAM as i32,
                    UnixSocketKind::SeqPacket => SOCK_SEQPACKET as i32,
                },
            ),
            SO_ACCEPTCONN => Self::encode_i32(
                option_len,
                matches!(&*self.state.lock(), UnixSocketState::Listener(_)) as i32,
            ),
            SO_DOMAIN => Self::encode_i32(option_len, AF_UNIX as i32),
            SO_PROTOCOL => Self::encode_i32(option_len, 0),
            SO_SNDBUF | SO_RCVBUF | SO_SNDBUFFORCE | SO_RCVBUFFORCE => {
                Self::encode_i32(option_len, DEFAULT_SOCKET_BUFFER_SIZE)
            }
            SO_REUSEADDR => Self::encode_i32(option_len, 0),
            SO_PASSCRED => Self::encode_i32(option_len, *self.pass_cred.lock() as i32),
            option_name if Self::is_boolean_sockopt(option_name) => Self::encode_i32(option_len, 0),
            SO_PEERCRED => match &*self.state.lock() {
                UnixSocketState::Datagram(datagram) => {
                    let cred = *datagram.peer_cred.lock();
                    Self::encode_ucred(
                        option_len,
                        SocketUcred {
                            pid: i32::try_from(cred.pid).unwrap_or(i32::MAX),
                            uid: cred.uid,
                            gid: cred.gid,
                        },
                    )
                }
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

impl SocketLike for UnixSocketObject {
    fn getsockname_bytes(&self) -> SocketResult<Vec<u8>> {
        UnixSocketObject::getsockname_bytes(self)
    }

    fn setsockopt(&self, level: u64, option_name: u64, option_value: &[u8]) -> SocketResult<()> {
        UnixSocketObject::setsockopt(self, level, option_name, option_value)
    }

    fn getsockopt(&self, level: u64, option_name: u64, option_len: usize) -> SocketResult<Vec<u8>> {
        UnixSocketObject::getsockopt(self, level, option_name, option_len)
    }
}
