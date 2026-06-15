use super::*;

pub(super) fn read_iovecs_from_user(
    iov_ptr: *const relibc_iovec,
    iov_len: usize,
) -> Result<Vec<relibc_iovec>, SyscallError> {
    if iov_len == 0 {
        return Ok(Vec::new());
    }
    if iov_ptr.is_null() {
        return Err(SyscallError::BadAddress);
    }

    let mut iovs = Vec::with_capacity(iov_len);
    for index in 0..iov_len {
        iovs.push(user_safe::read(unsafe { iov_ptr.add(index) })?);
    }
    Ok(iovs)
}

pub(super) fn copy_iovecs_from_user(iovs: &[relibc_iovec]) -> Result<Vec<u8>, SyscallError> {
    let total_len = iovs.iter().map(|iov| iov.iov_len).sum::<usize>();
    let mut buffer = Vec::with_capacity(total_len);
    for iov in iovs {
        if iov.iov_len == 0 {
            continue;
        }
        if iov.iov_base.is_null() {
            return Err(SyscallError::BadAddress);
        }
        buffer.extend_from_slice(&user_safe::read_buffer(
            iov.iov_base.cast_const(),
            iov.iov_len,
        )?);
    }
    Ok(buffer)
}

pub(super) fn cmsg_align(len: usize) -> usize {
    let align = mem::size_of::<usize>();
    (len + align - 1) & !(align - 1)
}

pub(super) fn encode_control_message(cmsg_type: i32, payload: &[u8]) -> Vec<u8> {
    let header_space = cmsg_align(mem::size_of::<LinuxCmsgHdr>());
    let control_len = header_space + cmsg_align(payload.len());
    let header = LinuxCmsgHdr {
        cmsg_len: header_space + payload.len(),
        cmsg_level: SOL_SOCKET as i32,
        cmsg_type,
    };
    let mut control = vec![0u8; control_len];
    let header_bytes = unsafe {
        slice::from_raw_parts(
            (&header as *const LinuxCmsgHdr).cast::<u8>(),
            mem::size_of::<LinuxCmsgHdr>(),
        )
    };
    control[..header_bytes.len()].copy_from_slice(header_bytes);
    control[header_space..header_space + payload.len()].copy_from_slice(payload);
    control
}

pub(super) fn rights_control_bytes_for_read(
    ready_rights: Vec<Vec<ObjectRef>>,
    cloexec: bool,
    control_capacity: usize,
) -> Result<Vec<u8>, SyscallError> {
    if ready_rights.is_empty() {
        return Ok(Vec::new());
    }

    let total_rights: usize = ready_rights.iter().map(Vec::len).sum();
    let payload_len = total_rights * mem::size_of::<i32>();
    let required_len = cmsg_align(mem::size_of::<LinuxCmsgHdr>()) + cmsg_align(payload_len);
    if control_capacity < required_len {
        return Ok(Vec::new());
    }

    let current_process = get_current_process();
    let mut current = current_process.lock();
    let fd_flags = if cloexec {
        FdFlags::CLOEXEC
    } else {
        FdFlags::empty()
    };
    let mut payload = Vec::with_capacity(total_rights * mem::size_of::<i32>());
    for rights in ready_rights {
        for right in rights {
            let fd = i32::try_from(current.push_object_with_flags(right, fd_flags))
                .map_err(|_| SyscallError::TooManyOpenFilesProcess)?;
            payload.extend_from_slice(&fd.to_ne_bytes());
        }
    }
    Ok(encode_control_message(SCM_RIGHTS, &payload))
}

pub(super) fn stream_rights_control_bytes_for_read(
    socket: &UnixSocketObject,
    bytes_read: usize,
    peek: bool,
    cloexec: bool,
    control_capacity: usize,
) -> Result<Vec<u8>, SyscallError> {
    let UnixSocketState::Stream(stream) = &*socket.state.lock() else {
        return Ok(Vec::new());
    };

    let ready_rights = if peek {
        stream.peek_ready_rights(bytes_read)
    } else {
        stream.take_ready_rights(bytes_read)
    };

    rights_control_bytes_for_read(ready_rights, cloexec, control_capacity)
}

pub(super) fn datagram_rights_control_bytes_for_read(
    socket: &UnixSocketObject,
    peek: bool,
    cloexec: bool,
    control_capacity: usize,
) -> Result<Vec<u8>, SyscallError> {
    let UnixSocketState::Datagram(datagram) = &*socket.state.lock() else {
        return Ok(Vec::new());
    };

    let rights = datagram.peer_rights.lock().clone();
    if rights.is_empty() {
        return Ok(Vec::new());
    }
    let control = rights_control_bytes_for_read(vec![rights], cloexec, control_capacity)?;
    if !peek && !control.is_empty() {
        datagram.peer_rights.lock().clear();
    }
    Ok(control)
}

pub(super) fn unix_socket_control_bytes(
    socket: &UnixSocketObject,
    bytes_read: usize,
    peek: bool,
    recv_flags: u64,
    control_capacity: usize,
) -> Result<Vec<u8>, SyscallError> {
    let cloexec = (recv_flags & MSG_CMSG_CLOEXEC) != 0;
    let mut control =
        stream_rights_control_bytes_for_read(socket, bytes_read, peek, cloexec, control_capacity)?;
    if control.is_empty() {
        control = datagram_rights_control_bytes_for_read(socket, peek, cloexec, control_capacity)?;
    }
    if !*socket.pass_cred.lock() {
        return Ok(control);
    }

    let peer_cred = match &*socket.state.lock() {
        UnixSocketState::Datagram(datagram) => *datagram.peer_cred.lock(),
        UnixSocketState::Stream(stream) => *stream.peer_cred.lock(),
        _ => return Ok(control),
    };
    let credential = LinuxUcred {
        pid: i32::try_from(peer_cred.pid).map_err(|_| SyscallError::InvalidArguments)?,
        uid: peer_cred.uid,
        gid: peer_cred.gid,
    };
    let cred_bytes = unsafe {
        slice::from_raw_parts(
            (&credential as *const LinuxUcred).cast::<u8>(),
            mem::size_of::<LinuxUcred>(),
        )
    };
    control.extend_from_slice(&encode_control_message(SCM_CREDENTIALS, cred_bytes));
    Ok(control)
}

pub(super) fn unix_seqpacket_next_len(socket: &UnixSocketObject) -> Option<usize> {
    if socket.kind != UnixSocketKind::SeqPacket {
        return None;
    }

    let UnixSocketState::Stream(stream) = &*socket.state.lock() else {
        return None;
    };
    stream.next_packet_len()
}

pub(super) fn unix_stream_has_front_rights(socket: &UnixSocketObject) -> bool {
    if socket.kind != UnixSocketKind::Stream {
        return false;
    }

    let UnixSocketState::Stream(stream) = &*socket.state.lock() else {
        return false;
    };
    stream.has_front_rights()
}

pub(super) fn netlink_socket_control_bytes(
    socket: &NetlinkSocketObject,
    source_pid: u32,
    uid: u32,
    gid: u32,
) -> Result<Vec<u8>, SyscallError> {
    if !socket.pass_cred_enabled() {
        return Ok(Vec::new());
    }

    let credential = LinuxUcred {
        pid: i32::try_from(source_pid).map_err(|_| SyscallError::InvalidArguments)?,
        uid,
        gid,
    };
    let cred_bytes = unsafe {
        slice::from_raw_parts(
            (&credential as *const LinuxUcred).cast::<u8>(),
            mem::size_of::<LinuxUcred>(),
        )
    };
    Ok(encode_control_message(SCM_CREDENTIALS, cred_bytes))
}
