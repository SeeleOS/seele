use alloc::{sync::Arc, vec, vec::Vec};
use core::{mem, slice};

use super::{
    AF_UNIX, SO_ACCEPTCONN, SO_DOMAIN, SO_ERROR, SO_PASSCRED, SO_PASSPIDFD, SO_PASSRIGHTS,
    SO_PASSSEC, SO_PEERCRED, SO_PEERGROUPS, SO_PEERPIDFD, SO_PEERSEC, SO_PROTOCOL, SO_RCVBUF,
    SO_RCVBUFFORCE, SO_RCVTIMEO_NEW, SO_RCVTIMEO_OLD, SO_REUSEADDR, SO_SNDBUF, SO_SNDBUFFORCE,
    SO_SNDTIMEO_NEW, SO_SNDTIMEO_OLD, SO_TIMESTAMP_NEW, SO_TIMESTAMP_OLD, SO_TIMESTAMPNS_NEW,
    SO_TIMESTAMPNS_OLD, SO_TYPE, SOCK_DGRAM, SOCK_SEQPACKET, SOCK_STREAM, SOL_SOCKET, SocketError,
    SocketLike, SocketPeerCred, SocketResult, UnixSocketKind, UnixSocketObject, UnixSocketState,
    socket_timeout_option_len,
};
use crate::{
    object::{Object, linux_anon::PidFdObject},
    process::{
        FdFlags,
        manager::get_current_process,
        misc::{ProcessID, get_process_with_pid},
    },
};

const DEFAULT_SOCKET_BUFFER_SIZE: i32 = 64 * 1024;
const DEFAULT_PEER_SECURITY_LABEL: &[u8] = b"unconfined\0";

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
            SO_RCVTIMEO_OLD | SO_SNDTIMEO_OLD | SO_RCVTIMEO_NEW | SO_SNDTIMEO_NEW => {
                let expected_len =
                    socket_timeout_option_len(option_name).ok_or(SocketError::InvalidArguments)?;
                if option_value.len() < expected_len {
                    return Err(SocketError::InvalidArguments);
                }
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
            SO_PEERSEC | SO_PEERGROUPS | SO_PEERPIDFD => Err(SocketError::OperationNotSupported),
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
            SO_RCVTIMEO_OLD | SO_SNDTIMEO_OLD | SO_RCVTIMEO_NEW | SO_SNDTIMEO_NEW => {
                let expected_len =
                    socket_timeout_option_len(option_name).ok_or(SocketError::InvalidArguments)?;
                Self::encode_zeroed_bytes(option_len, expected_len)
            }
            SO_PEERGROUPS => self.encode_peer_groups(option_len),
            SO_PEERPIDFD => self.encode_peer_pidfd(option_len),
            SO_PEERCRED => {
                let cred = self.peer_cred()?;
                Self::encode_ucred(
                    option_len,
                    SocketUcred {
                        pid: i32::try_from(cred.pid).unwrap_or(i32::MAX),
                        uid: cred.uid,
                        gid: cred.gid,
                    },
                )
            }
            SO_PEERSEC => Self::encode_bytes(option_len, DEFAULT_PEER_SECURITY_LABEL),
            _ => Err(SocketError::InvalidArguments),
        }
    }

    fn peer_cred(&self) -> SocketResult<SocketPeerCred> {
        match &*self.state.lock() {
            UnixSocketState::Datagram(datagram) => Ok(*datagram.peer_cred.lock()),
            UnixSocketState::Stream(stream) => Ok(*stream.peer_cred.lock()),
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

    fn encode_zeroed_bytes(option_len: usize, expected_len: usize) -> SocketResult<Vec<u8>> {
        if option_len < expected_len {
            return Err(SocketError::InvalidArguments);
        }

        Ok(vec![0; expected_len])
    }

    fn encode_bytes(option_len: usize, value: &[u8]) -> SocketResult<Vec<u8>> {
        if option_len < value.len() {
            return Err(SocketError::InvalidArguments);
        }

        Ok(value.to_vec())
    }

    fn encode_u32_slice(option_len: usize, values: &[u32]) -> SocketResult<Vec<u8>> {
        let expected_len = mem::size_of_val(values);
        if option_len < expected_len {
            return Err(SocketError::InvalidArguments);
        }

        let mut encoded = Vec::with_capacity(expected_len);
        for value in values {
            encoded.extend_from_slice(&value.to_ne_bytes());
        }
        Ok(encoded)
    }

    fn encode_peer_groups(&self, option_len: usize) -> SocketResult<Vec<u8>> {
        let cred = self.peer_cred()?;
        let mut groups = if let Ok(process) = get_process_with_pid(ProcessID(cred.pid)) {
            let process = process.lock();
            let mut groups = process.supplementary_groups.clone();
            groups.push(process.effective_gid);
            groups
        } else {
            vec![cred.gid]
        };
        groups.sort_unstable();
        groups.dedup();
        Self::encode_u32_slice(option_len, &groups)
    }

    fn encode_peer_pidfd(&self, option_len: usize) -> SocketResult<Vec<u8>> {
        let cred = self.peer_cred()?;
        let pidfd: Arc<dyn Object> = PidFdObject::new(cred.pid);
        let fd = get_current_process()
            .lock()
            .push_object_with_flags(pidfd, FdFlags::CLOEXEC);
        Self::encode_i32(
            option_len,
            i32::try_from(fd).map_err(|_| SocketError::InvalidArguments)?,
        )
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
