use alloc::{vec, vec::Vec};

use super::socket::NetlinkSocketObject;
use crate::socket::{
    AF_NETLINK, NETLINK_ADD_MEMBERSHIP, NETLINK_DROP_MEMBERSHIP, NETLINK_EXT_ACK,
    NETLINK_GET_STRICT_CHK, NETLINK_LIST_MEMBERSHIPS, NETLINK_PKTINFO, SO_ATTACH_FILTER,
    SO_DETACH_FILTER, SO_DOMAIN, SO_ERROR, SO_PASSCRED, SO_PASSPIDFD, SO_PASSRIGHTS, SO_PASSSEC,
    SO_PRIORITY, SO_PROTOCOL, SO_RCVBUF, SO_RCVBUFFORCE, SO_RCVTIMEO_NEW, SO_RCVTIMEO_OLD,
    SO_REUSEADDR, SO_SNDBUF, SO_SNDBUFFORCE, SO_SNDTIMEO_NEW, SO_SNDTIMEO_OLD, SO_TIMESTAMP_NEW,
    SO_TIMESTAMP_OLD, SO_TIMESTAMPNS_NEW, SO_TIMESTAMPNS_OLD, SO_TYPE, SOL_NETLINK, SOL_SOCKET,
    SocketError, SocketResult, can_set_socket_priority, socket_timeout_option_len,
};

const DEFAULT_SOCKET_BUFFER_SIZE: i32 = 64 * 1024;

impl NetlinkSocketObject {
    pub fn setsockopt(
        &self,
        level: u64,
        option_name: u64,
        option_value: &[u8],
    ) -> SocketResult<()> {
        if level == SOL_SOCKET {
            return self.set_socket_option(option_name, option_value);
        }

        if level != SOL_NETLINK {
            return Err(SocketError::ProtocolNotSupported);
        }

        self.set_netlink_option(option_name, option_value)
    }

    pub fn getsockopt(
        &self,
        level: u64,
        option_name: u64,
        option_len: usize,
    ) -> SocketResult<Vec<u8>> {
        if level == SOL_SOCKET {
            return self.socket_option(option_name, option_len);
        }

        if level != SOL_NETLINK {
            return Err(SocketError::ProtocolNotSupported);
        }

        self.netlink_option(option_name, option_len)
    }

    fn set_socket_option(&self, option_name: u64, option_value: &[u8]) -> SocketResult<()> {
        match option_name {
            SO_PASSCRED => {
                let enabled = decode_u32(option_value)? != 0;
                *self.pass_cred.lock() = enabled;
                Ok(())
            }
            SO_PRIORITY => {
                let priority = decode_i32(option_value)?;
                can_set_socket_priority(priority)?;
                *self.priority.lock() = priority;
                Ok(())
            }
            SO_REUSEADDR | SO_SNDBUF | SO_RCVBUF | SO_SNDBUFFORCE | SO_RCVBUFFORCE
            | SO_ATTACH_FILTER | SO_DETACH_FILTER | SO_PASSSEC | SO_PASSRIGHTS | SO_PASSPIDFD
            | SO_TIMESTAMP_OLD | SO_TIMESTAMP_NEW | SO_TIMESTAMPNS_OLD | SO_TIMESTAMPNS_NEW => {
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
            _ => Err(SocketError::InvalidArguments),
        }
    }

    fn set_netlink_option(&self, option_name: u64, option_value: &[u8]) -> SocketResult<()> {
        match option_name {
            NETLINK_PKTINFO | NETLINK_EXT_ACK | NETLINK_GET_STRICT_CHK => Ok(()),
            NETLINK_ADD_MEMBERSHIP | NETLINK_DROP_MEMBERSHIP => {
                let group = decode_u32(option_value)?;
                let mut memberships = self.memberships.lock();
                if option_name == NETLINK_ADD_MEMBERSHIP {
                    if !memberships.contains(&group) {
                        memberships.push(group);
                    }
                } else {
                    memberships.retain(|existing| *existing != group);
                }
                Ok(())
            }
            _ => Err(SocketError::InvalidArguments),
        }
    }

    fn socket_option(&self, option_name: u64, option_len: usize) -> SocketResult<Vec<u8>> {
        match option_name {
            SO_ERROR => encode_i32(option_len, 0),
            SO_TYPE => encode_i32(option_len, self.socket_type as i32),
            SO_DOMAIN => encode_i32(option_len, AF_NETLINK as i32),
            SO_PROTOCOL => encode_i32(option_len, self.protocol as i32),
            SO_SNDBUF | SO_RCVBUF | SO_SNDBUFFORCE | SO_RCVBUFFORCE => {
                encode_i32(option_len, DEFAULT_SOCKET_BUFFER_SIZE)
            }
            SO_PRIORITY => encode_i32(option_len, *self.priority.lock()),
            SO_PASSCRED => encode_i32(option_len, self.pass_cred_enabled() as i32),
            SO_REUSEADDR | SO_PASSSEC | SO_PASSRIGHTS | SO_PASSPIDFD | SO_TIMESTAMP_OLD
            | SO_TIMESTAMP_NEW | SO_TIMESTAMPNS_OLD | SO_TIMESTAMPNS_NEW => {
                encode_i32(option_len, 0)
            }
            SO_RCVTIMEO_OLD | SO_SNDTIMEO_OLD | SO_RCVTIMEO_NEW | SO_SNDTIMEO_NEW => {
                let expected_len =
                    socket_timeout_option_len(option_name).ok_or(SocketError::InvalidArguments)?;
                encode_zeroed_bytes(option_len, expected_len)
            }
            _ => Err(SocketError::InvalidArguments),
        }
    }

    fn netlink_option(&self, option_name: u64, option_len: usize) -> SocketResult<Vec<u8>> {
        match option_name {
            NETLINK_LIST_MEMBERSHIPS => Ok(self.membership_bytes(option_len)),
            _ => Err(SocketError::InvalidArguments),
        }
    }

    fn membership_bytes(&self, option_len: usize) -> Vec<u8> {
        let memberships = self.memberships.lock();
        if option_len == 0 {
            return Vec::new();
        }

        let capacity = option_len / core::mem::size_of::<u32>();
        let mut out = Vec::with_capacity(capacity * core::mem::size_of::<u32>());
        for group in memberships.iter().take(capacity) {
            out.extend_from_slice(&group.to_ne_bytes());
        }
        out
    }
}

fn encode_i32(option_len: usize, value: i32) -> SocketResult<Vec<u8>> {
    if option_len < core::mem::size_of::<i32>() {
        return Err(SocketError::InvalidArguments);
    }
    Ok(value.to_ne_bytes().to_vec())
}

fn decode_i32(option_value: &[u8]) -> SocketResult<i32> {
    if option_value.len() < core::mem::size_of::<i32>() {
        return Err(SocketError::InvalidArguments);
    }

    Ok(i32::from_ne_bytes(
        option_value[..core::mem::size_of::<i32>()]
            .try_into()
            .map_err(|_| SocketError::InvalidArguments)?,
    ))
}

fn decode_u32(option_value: &[u8]) -> SocketResult<u32> {
    if option_value.len() < core::mem::size_of::<u32>() {
        return Err(SocketError::InvalidArguments);
    }

    Ok(u32::from_ne_bytes(
        option_value[..core::mem::size_of::<u32>()]
            .try_into()
            .map_err(|_| SocketError::InvalidArguments)?,
    ))
}

fn encode_zeroed_bytes(option_len: usize, expected_len: usize) -> SocketResult<Vec<u8>> {
    if option_len < expected_len {
        return Err(SocketError::InvalidArguments);
    }

    Ok(vec![0; expected_len])
}
