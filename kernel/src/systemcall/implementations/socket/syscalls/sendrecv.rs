use super::*;

fn sendmsg_rights(msg: &relibc_msg_hdr) -> Result<Vec<ObjectRef>, SyscallError> {
    if msg.msg_controllen == 0 {
        return Ok(Vec::new());
    }
    if msg.msg_control.is_null() || msg.msg_controllen < mem::size_of::<LinuxCmsgHdr>() {
        return Err(SyscallError::BadAddress);
    }

    let control = user_safe::read_buffer(msg.msg_control, msg.msg_controllen)?;
    let header_space = cmsg_align(mem::size_of::<LinuxCmsgHdr>());
    let mut offset = 0usize;
    let mut rights = Vec::new();

    while offset + mem::size_of::<LinuxCmsgHdr>() <= control.len() {
        let header = unsafe { &*(control[offset..].as_ptr().cast::<LinuxCmsgHdr>()) };
        if header.cmsg_len < header_space {
            return Err(SyscallError::InvalidArguments);
        }

        let end = offset + header.cmsg_len;
        if end > control.len() {
            return Err(SyscallError::InvalidArguments);
        }

        if header.cmsg_level == SOL_SOCKET as i32 && header.cmsg_type == SCM_RIGHTS {
            let payload_len = header.cmsg_len - header_space;
            if !payload_len.is_multiple_of(mem::size_of::<i32>()) {
                return Err(SyscallError::InvalidArguments);
            }

            let fd_count = payload_len / mem::size_of::<i32>();
            let fds = unsafe {
                slice::from_raw_parts(
                    control[offset + header_space..].as_ptr().cast::<i32>(),
                    fd_count,
                )
            };
            for &fd in fds {
                if fd < 0 {
                    return Err(SyscallError::InvalidArguments);
                }
                rights.push(get_object_current_process(fd as u64).map_err(SyscallError::from)?);
            }
        }

        let next = cmsg_align(end);
        if next > control.len() {
            if end != control.len() {
                return Err(SyscallError::InvalidArguments);
            }
            break;
        }

        offset = next;
    }

    Ok(rights)
}

fn sendmsg_impl(
    socket: ObjectRef,
    msg: &relibc_msg_hdr,
    flags: u64,
) -> Result<usize, SyscallError> {
    if msg.msg_iovlen > isize::MAX as usize {
        return Err(SyscallError::InvalidArguments);
    }

    let iovs = read_iovecs_from_user(msg.msg_iov, msg.msg_iovlen)?;

    if let Ok(socket) = socket.clone().as_netlink_socket() {
        let destination = if !msg.msg_name.is_null() {
            let SocketAddress::Netlink(address) =
                socket_address_from_raw(msg.msg_name.cast(), msg.msg_namelen)?
            else {
                return Err(SyscallError::InvalidArguments);
            };
            Some(address)
        } else {
            None
        };

        let buffer = copy_iovecs_from_user(&iovs)?;

        let written = socket
            .send(buffer.as_slice(), destination)
            .map_err(ObjectError::from)?;
        return Ok(written);
    }

    if let Ok(socket) = socket.clone().as_inet_socket() {
        let target_addr = if !msg.msg_name.is_null() {
            let SocketAddress::Inet(address) =
                socket_address_from_raw(msg.msg_name.cast(), msg.msg_namelen)?
            else {
                return Err(SyscallError::InvalidArguments);
            };
            Some(address)
        } else {
            None
        };

        if msg.msg_controllen != 0 && !msg.msg_control.is_null() {
            let _ = user_safe::read_buffer(msg.msg_control, msg.msg_controllen)?;
        }

        let buffer = copy_iovecs_from_user(&iovs)?;

        let written = match target_addr {
            Some(address) => socket
                .send_to(&buffer, address)
                .map_err(ObjectError::from)?,
            None => socket.send(&buffer).map_err(ObjectError::from)?,
        };
        return Ok(written);
    }

    let target_path = if !msg.msg_name.is_null() {
        let address_len = msg.msg_namelen;
        let SocketAddress::Unix(path) = socket_address_from_raw(msg.msg_name.cast(), address_len)?
        else {
            return Err(SyscallError::InvalidArguments);
        };
        Some(path)
    } else {
        None
    };

    let socket = socket
        .as_unix_socket()
        .map_err(|_| SyscallError::NotSocket)?;
    let dontwait = (flags & MSG_DONTWAIT) != 0;
    let rights = sendmsg_rights(msg)?;
    if socket.kind == UnixSocketKind::Datagram {
        let buffer = copy_iovecs_from_user(&iovs)?;
        let written = if let Some(path) = target_path.as_deref() {
            socket
                .write_socket_to_path_with_rights(&buffer, path, dontwait, rights)
                .map_err(ObjectError::from)?
        } else {
            socket
                .write_socket_with_rights(&buffer, dontwait, rights)
                .map_err(ObjectError::from)?
        };
        return Ok(written);
    }

    if let Some(path) = target_path
        && matches!(&*socket.state.lock(), UnixSocketState::Unbound)
    {
        socket.connect(path).map_err(ObjectError::from)?;
    }

    let buffer = copy_iovecs_from_user(&iovs)?;

    socket
        .write_socket_with_rights(&buffer, dontwait, rights)
        .map_err(ObjectError::from)
        .map_err(Into::into)
}

define_syscall!(Sendto, |socket: ObjectRef,
                         buffer: *const u8,
                         len: usize,
                         _flags: u64,
                         address: *const u8,
                         address_len: u32| {
    if len > 0 && buffer.is_null() {
        return Err(SyscallError::BadAddress);
    }
    if (_flags & MSG_OOB) != 0 {
        return Err(SyscallError::OperationNotSupported);
    }

    let user_buffer = if len == 0 {
        Vec::new()
    } else {
        user_safe::read_buffer(buffer, len)?
    };
    let address = (!address.is_null())
        .then(|| socket_address_bytes(address, address_len))
        .transpose()?;
    let written = socket_like(socket)?
        .sendto(user_buffer.as_slice(), address.as_deref())
        .map_err(ObjectError::from)?;

    Ok(written)
});

define_syscall!(
    Recvfrom,
    |socket: ObjectRef,
     buffer: *mut u8,
     len: usize,
     flags: u64,
     address: *mut u8,
     address_len_ptr: *mut u32| {
        if len > 0 && buffer.is_null() {
            return Err(SyscallError::BadAddress);
        }
        if (flags & MSG_OOB) != 0 {
            return Err(SyscallError::InvalidArguments);
        }
        if (flags & MSG_ERRQUEUE) != 0 {
            return Err(SyscallError::TryAgain);
        }

        if let Ok(socket) = socket.clone().as_netlink_socket()
            && (flags & (MSG_PEEK | MSG_TRUNC)) != 0
        {
            let peek = (flags & MSG_PEEK) != 0;
            let report_trunc = (flags & MSG_TRUNC) != 0;
            let message_len = socket.peek_message_len().ok_or(SyscallError::TryAgain)?;
            let mut data = vec![0; len];
            let (copied, full_len, source, _, _) = socket
                .recv_message(&mut data, peek)
                .map_err(SyscallError::from)?;

            if copied > 0 {
                user_safe::write(buffer, &data[..copied])?;
            }

            if !address.is_null() {
                if address_len_ptr.is_null() {
                    return Err(SyscallError::BadAddress);
                }
                let name = LinuxSockAddrNl {
                    nl_family: AF_NETLINK as u16,
                    nl_pad: 0,
                    nl_pid: source.pid,
                    nl_groups: source.groups,
                };
                let requested_len = user_safe::read(address_len_ptr)? as usize;
                let name_bytes = unsafe {
                    core::slice::from_raw_parts(
                        (&name as *const LinuxSockAddrNl).cast::<u8>(),
                        core::mem::size_of::<LinuxSockAddrNl>(),
                    )
                };
                let copy_len = requested_len.min(name_bytes.len());
                if copy_len > 0 {
                    user_safe::write(address, &name_bytes[..copy_len])?;
                }
                user_safe::write(address_len_ptr, &(name_bytes.len() as u32))?;
            }

            return Ok(if report_trunc || len == 0 {
                full_len.max(message_len)
            } else {
                copied
            });
        }

        let mut data = vec![0; len];
        let (read, source) = socket_like(socket.clone())?
            .recvfrom(&mut data)
            .map_err(ObjectError::from)?;

        if read > 0 {
            user_safe::write(buffer, &data[..read])?;
        }

        if !address.is_null() {
            write_socket_name(address, address_len_ptr, &source.unwrap_or_default())?;
        }

        Ok(read)
    }
);

define_syscall!(Sendmsg, |socket: ObjectRef,
                          msg: *const relibc_msg_hdr,
                          flags: u64| {
    if msg.is_null() {
        return Err(SyscallError::BadAddress);
    }

    let msg = user_safe::read(msg)?;
    sendmsg_impl(socket, &msg, flags)
});

define_syscall!(Sendmmsg, |socket: ObjectRef,
                           msgvec: *mut relibc_mmsghdr,
                           vlen: u32,
                           flags: u32| {
    if vlen > 0 && msgvec.is_null() {
        return Err(SyscallError::BadAddress);
    }

    let mut sent = 0usize;

    for index in 0..(vlen as usize) {
        let message_ptr = unsafe { msgvec.add(index) };
        let mut message = user_safe::read(message_ptr)?;
        match sendmsg_impl(socket.clone(), &message.msg_hdr, flags as u64) {
            Ok(written) => {
                message.msg_len =
                    u32::try_from(written).map_err(|_| SyscallError::InvalidArguments)?;
                user_safe::write(message_ptr, &message)?;
                sent += 1;
            }
            Err(_) if sent > 0 => break,
            Err(err) => return Err(err),
        }
    }

    Ok(sent)
});

define_syscall!(Recvmsg, |socket: ObjectRef,
                          msg: *mut relibc_msg_hdr,
                          flags: u64| {
    if msg.is_null() {
        return Err(SyscallError::BadAddress);
    }

    let msg_ptr = msg;
    let mut msg = user_safe::read(msg_ptr)?;
    if msg.msg_iovlen > isize::MAX as usize {
        return Err(SyscallError::InvalidArguments);
    }

    let iovs = read_iovecs_from_user(msg.msg_iov.cast_const(), msg.msg_iovlen)?;

    let result = (|| {
        if let Ok(socket) = socket.clone().as_netlink_socket() {
            let peek = (flags & MSG_PEEK) != 0;
            let report_trunc = (flags & MSG_TRUNC) != 0;
            let total_capacity = iovs.iter().map(|iov| iov.iov_len).sum::<usize>();
            let message_len = socket.peek_message_len().ok_or(SyscallError::TryAgain)?;
            let mut scratch = alloc::vec![0u8; total_capacity];
            let (copied, full_len, source, uid, gid) = socket
                .recv_message(&mut scratch, peek)
                .map_err(SyscallError::from)?;
            let mut copied_total = 0usize;

            for iov in iovs {
                if copied_total >= copied {
                    break;
                }
                if iov.iov_len == 0 {
                    continue;
                }
                if iov.iov_base.is_null() {
                    return Err(SyscallError::BadAddress);
                }

                let chunk_len = (copied - copied_total).min(iov.iov_len);
                user_safe::write(
                    iov.iov_base,
                    &scratch[copied_total..copied_total + chunk_len],
                )?;
                copied_total += chunk_len;
            }

            msg.msg_flags = 0;
            if !msg.msg_name.is_null() {
                let name = LinuxSockAddrNl {
                    nl_family: AF_NETLINK as u16,
                    nl_pad: 0,
                    nl_pid: source.pid,
                    nl_groups: source.groups,
                };
                let requested_len = msg.msg_namelen as usize;
                let name_bytes = unsafe {
                    core::slice::from_raw_parts(
                        (&name as *const LinuxSockAddrNl).cast::<u8>(),
                        core::mem::size_of::<LinuxSockAddrNl>(),
                    )
                };
                let copy_len = requested_len.min(name_bytes.len());
                if copy_len > 0 {
                    user_safe::write(msg.msg_name.cast::<u8>(), &name_bytes[..copy_len])?;
                }
                msg.msg_namelen = name_bytes.len() as u32;
            }
            let control = netlink_socket_control_bytes(&socket, source.pid, uid, gid)?;
            if control.is_empty() {
                msg.msg_controllen = 0;
            } else if msg.msg_control.is_null() || msg.msg_controllen == 0 {
                msg.msg_flags |= MSG_CTRUNC;
                msg.msg_controllen = 0;
            } else {
                let copy_len = msg.msg_controllen.min(control.len());
                user_safe::write(msg.msg_control, &control[..copy_len])?;
                msg.msg_controllen = copy_len;
                if copy_len < control.len() {
                    msg.msg_flags |= MSG_CTRUNC;
                }
            }
            return Ok(if report_trunc || total_capacity == 0 {
                full_len.max(message_len)
            } else {
                copied_total
            });
        }

        if let Ok(socket) = socket.clone().as_inet_socket() {
            let total_capacity = iovs.iter().map(|iov| iov.iov_len).sum::<usize>();
            let mut scratch = alloc::vec![0u8; total_capacity];
            let (read, source) = socket.recv_from(&mut scratch).map_err(ObjectError::from)?;
            let mut copied_total = 0usize;

            for iov in iovs {
                if copied_total >= read {
                    break;
                }
                if iov.iov_len == 0 {
                    continue;
                }
                if iov.iov_base.is_null() {
                    return Err(SyscallError::BadAddress);
                }

                let chunk_len = (read - copied_total).min(iov.iov_len);
                user_safe::write(
                    iov.iov_base,
                    &scratch[copied_total..copied_total + chunk_len],
                )?;
                copied_total += chunk_len;
            }

            msg.msg_flags = 0;
            if !msg.msg_name.is_null() {
                let name = source
                    .map(|address| socket_address_to_bytes(SocketAddress::Inet(address)))
                    .transpose()?
                    .unwrap_or_default();
                let copy_len = (msg.msg_namelen as usize).min(name.len());
                if copy_len > 0 {
                    user_safe::write(msg.msg_name.cast::<u8>(), &name[..copy_len])?;
                }
                msg.msg_namelen = name.len() as u32;
            } else {
                msg.msg_namelen = 0;
            }
            msg.msg_controllen = 0;
            return Ok(copied_total);
        }

        let socket = socket
            .as_unix_socket()
            .map_err(|_| SyscallError::NotSocket)?;
        let dontwait = (flags & MSG_DONTWAIT) != 0;
        let peek = (flags & MSG_PEEK) != 0;
        let report_trunc = (flags & MSG_TRUNC) != 0;
        let total_capacity = iovs.iter().map(|iov| iov.iov_len).sum::<usize>();
        let mut scratch =
            alloc::vec![0u8; unix_seqpacket_next_len(&socket).unwrap_or(total_capacity)];
        let total_read = socket
            .recv_socket_with_flags_and_mode(&mut scratch, dontwait, peek)
            .map_err(ObjectError::from)?;

        let mut copied_total = 0usize;
        for iov in iovs {
            if copied_total >= total_read {
                break;
            }
            if iov.iov_len == 0 {
                continue;
            }
            if iov.iov_base.is_null() {
                return Err(SyscallError::BadAddress);
            }

            let chunk_len = (total_read - copied_total).min(iov.iov_len);
            user_safe::write(
                iov.iov_base,
                &scratch[copied_total..copied_total + chunk_len],
            )?;
            copied_total += chunk_len;
        }

        msg.msg_flags = 0;
        if copied_total < total_read {
            msg.msg_flags |= MSG_TRUNC as i32;
        }
        if !msg.msg_name.is_null() {
            let name = socket.getpeername_bytes().map_err(ObjectError::from)?;
            let copy_len = (msg.msg_namelen as usize).min(name.len());
            if copy_len > 0 {
                user_safe::write(msg.msg_name.cast::<u8>(), &name[..copy_len])?;
            }
            msg.msg_namelen = name.len() as u32;
        } else {
            msg.msg_namelen = 0;
        }
        let control = if total_read > 0
            || socket.kind != UnixSocketKind::Stream
            || unix_stream_has_front_rights(&socket)
        {
            unix_socket_control_bytes(&socket, total_read, peek, flags, msg.msg_controllen)?
        } else {
            Vec::new()
        };
        if control.is_empty() {
            msg.msg_controllen = 0;
        } else if msg.msg_control.is_null() || msg.msg_controllen == 0 {
            msg.msg_flags |= MSG_CTRUNC;
            msg.msg_controllen = 0;
        } else {
            let copy_len = msg.msg_controllen.min(control.len());
            user_safe::write(msg.msg_control, &control[..copy_len])?;
            msg.msg_controllen = copy_len;
            if copy_len < control.len() {
                msg.msg_flags |= MSG_CTRUNC;
            }
        }

        Ok(if report_trunc || total_capacity == 0 {
            total_read
        } else {
            copied_total
        })
    })();

    if result.is_ok() {
        user_safe::write(msg_ptr, &msg)?;
    }
    result
});

#[cfg(test)]
mod tests {
    use super::*;
    use crate::systemcall::implementations::{
        Accept, Bind, Connect, Eventfd, Listen, Recvfrom, Recvmsg, Sendmmsg, Sendmsg, Sendto,
        Setsockopt, Socket, Socketpair, Write,
    };
    use crate::systemcall::test::*;

    crate::test!(
        socket_message_syscalls,
        "accept sendto recvfrom sendmsg sendmmsg and recvmsg follow linux socket rules",
        socket_message_syscalls_follow_linux_rules
    );
    fn socket_message_syscalls_follow_linux_rules() {
        const AF_UNIX: u64 = 1;
        const SOCK_STREAM: u64 = 1;
        const SOCK_DGRAM: u64 = 2;
        const SOCK_NONBLOCK: u64 = 0o0004000;
        const SOL_SOCKET: i32 = 1;
        const SO_PASSCRED: u64 = 16;
        const SCM_RIGHTS: i32 = 1;
        const SCM_CREDENTIALS: i32 = 2;
        const MSG_CTRUNC: i32 = 0x8;
        const MSG_CMSG_CLOEXEC: u64 = 0x4000_0000;

        assert_linux_layout::<TestRelibcIovec>(16, 8);
        assert_linux_layout::<TestRelibcMsgHdr>(56, 8);
        assert_linux_layout::<TestRelibcMmsghdr>(64, 8);
        assert_linux_layout::<TestLinuxCmsgHdr>(16, 8);
        assert_linux_layout::<TestRightsControlMessage>(24, 8);

        let page = allocate_user_test_page();
        let listener_path = b"/tmp/accept-linux.sock\0";
        let source_path = b"/tmp/sendto-src.sock\0";
        let target_path = b"/tmp/sendto-dst.sock\0";
        write_user_value(page, listener_path);
        write_user_value(page + 256, source_path);
        write_user_value(page + 512, target_path);

        let mut listener_addr = TestLinuxSockAddrUn::default();
        listener_addr.sun_family = AF_UNIX as u16;
        listener_addr.sun_path[..listener_path.len()].copy_from_slice(listener_path);
        write_user_value(page + 128, &listener_addr);

        let mut source_addr = TestLinuxSockAddrUn::default();
        source_addr.sun_family = AF_UNIX as u16;
        source_addr.sun_path[..source_path.len()].copy_from_slice(source_path);
        write_user_value(page + 384, &source_addr);

        let mut target_addr = TestLinuxSockAddrUn::default();
        target_addr.sun_family = AF_UNIX as u16;
        target_addr.sun_path[..target_path.len()].copy_from_slice(target_path);
        write_user_value(page + 640, &target_addr);

        let listener = expect_fd(
            SyscallArgs::new([AF_UNIX, SOCK_STREAM | SOCK_NONBLOCK, 0, 0, 0, 0]).call::<Socket>(),
        );
        expect_ok(
            SyscallArgs::new([listener as u64, page + 128, 110, 0, 0, 0]).call::<Bind>(),
            0,
        );
        expect_ok(
            SyscallArgs::new([listener as u64, 4, 0, 0, 0, 0]).call::<Listen>(),
            0,
        );
        let client =
            expect_fd(SyscallArgs::new([AF_UNIX, SOCK_STREAM, 0, 0, 0, 0]).call::<Socket>());
        expect_ok(
            SyscallArgs::new([client as u64, page + 128, 110, 0, 0, 0]).call::<Connect>(),
            0,
        );
        expect_errno(
            SyscallArgs::new([listener as u64, page + 768, 0, 0, 0, 0]).call::<Accept>(),
            SyscallError::BadAddress,
        );
        write_user_value(page + 776, &2u32);
        let accepted = expect_fd(
            SyscallArgs::new([listener as u64, page + 768, page + 776, 0, 0, 0]).call::<Accept>(),
        );
        assert_fd_flags(accepted, FdFlags::empty());
        assert_object_flags(accepted, FileFlags::empty());
        assert_eq!(read_user_value::<u32>(page + 776), 2);
        let accepted_peer = read_user_value::<TestLinuxSockAddrUn>(page + 768);
        assert_eq!(accepted_peer.sun_family, AF_UNIX as u16);

        let sender =
            expect_fd(SyscallArgs::new([AF_UNIX, SOCK_DGRAM, 0, 0, 0, 0]).call::<Socket>());
        let receiver =
            expect_fd(SyscallArgs::new([AF_UNIX, SOCK_DGRAM, 0, 0, 0, 0]).call::<Socket>());
        expect_ok(
            SyscallArgs::new([sender as u64, page + 384, 110, 0, 0, 0]).call::<Bind>(),
            0,
        );
        expect_ok(
            SyscallArgs::new([receiver as u64, page + 640, 110, 0, 0, 0]).call::<Bind>(),
            0,
        );

        write_user_value(page + 896, b"hey");
        expect_ok(
            SyscallArgs::new([sender as u64, page + 896, 3, 0, page + 640, 110]).call::<Sendto>(),
            3,
        );
        write_user_value(page + 1048, &2u32);
        expect_ok(
            SyscallArgs::new([receiver as u64, page + 1024, 8, 0, page + 1152, page + 1048])
                .call::<Recvfrom>(),
            3,
        );
        assert_user_bytes(page + 1024, b"hey");
        assert_eq!(read_user_value::<u32>(page + 1048), 110);
        let recv_source = read_user_value::<TestLinuxSockAddrUn>(page + 1152);
        assert_eq!(recv_source.sun_family, AF_UNIX as u16);
        expect_errno(
            SyscallArgs::new([sender as u64, page + 896, 1, 0, page + 640, 1]).call::<Sendto>(),
            SyscallError::InvalidArguments,
        );
        expect_errno(
            SyscallArgs::new([sender as u64, 0, 1, 0, page + 640, 110]).call::<Sendto>(),
            SyscallError::BadAddress,
        );

        let rights_fd = expect_fd(SyscallArgs::new([0, 0, 0, 0, 0, 0]).call::<Eventfd>());
        let socketpair_page = page + 1408;
        expect_ok(
            SyscallArgs::new([AF_UNIX, SOCK_STREAM, 0, socketpair_page, 0, 0]).call::<Socketpair>(),
            0,
        );
        let stream_left = read_user_value::<i32>(socketpair_page) as usize;
        let stream_right = read_user_value::<i32>(socketpair_page + 4) as usize;

        write_user_value(page + 1424, b"R");
        let send_iov = TestRelibcIovec {
            iov_base: (page + 1424) as *mut u8,
            iov_len: 1,
        };
        write_user_value(page + 1440, &send_iov);
        let send_control = TestRightsControlMessage {
            header: TestLinuxCmsgHdr {
                cmsg_len: 20,
                cmsg_level: SOL_SOCKET,
                cmsg_type: SCM_RIGHTS,
            },
            fd: rights_fd as i32,
            pad: 0,
        };
        write_user_value(page + 1472, &send_control);
        let send_msg = TestRelibcMsgHdr {
            msg_name: core::ptr::null_mut(),
            msg_namelen: 0,
            msg_iov: (page + 1440) as *mut TestRelibcIovec,
            msg_iovlen: 1,
            msg_control: (page + 1472) as *mut u8,
            msg_controllen: core::mem::size_of::<TestRightsControlMessage>(),
            msg_flags: 0,
        };
        write_user_value(page + 1504, &send_msg);
        expect_ok(
            SyscallArgs::new([stream_left as u64, page + 1504, 0, 0, 0, 0]).call::<Sendmsg>(),
            1,
        );

        write_user_value(page + 1568, &[0u8]);
        let recv_iov = TestRelibcIovec {
            iov_base: (page + 1568) as *mut u8,
            iov_len: 1,
        };
        write_user_value(page + 1584, &recv_iov);
        write_user_value(page + 1616, &TestRightsControlMessage::default());
        let recv_msg = TestRelibcMsgHdr {
            msg_name: core::ptr::null_mut(),
            msg_namelen: 0,
            msg_iov: (page + 1584) as *mut TestRelibcIovec,
            msg_iovlen: 1,
            msg_control: (page + 1616) as *mut u8,
            msg_controllen: core::mem::size_of::<TestRightsControlMessage>(),
            msg_flags: 0,
        };
        write_user_value(page + 1648, &recv_msg);
        expect_ok(
            SyscallArgs::new([stream_right as u64, page + 1648, MSG_CMSG_CLOEXEC, 0, 0, 0])
                .call::<Recvmsg>(),
            1,
        );
        assert_user_bytes(page + 1568, b"R");
        let recv_msg_after = read_user_value::<TestRelibcMsgHdr>(page + 1648);
        assert_eq!(recv_msg_after.msg_flags, 0);
        assert_eq!(
            recv_msg_after.msg_controllen,
            core::mem::size_of::<TestRightsControlMessage>()
        );
        let received_control = read_user_value::<TestRightsControlMessage>(page + 1616);
        assert_eq!(received_control.header.cmsg_len, 20);
        assert_eq!(received_control.header.cmsg_level, SOL_SOCKET);
        assert_eq!(received_control.header.cmsg_type, SCM_RIGHTS);
        let received_fd =
            usize::try_from(received_control.fd).expect("received fd should be non-negative");
        assert_ne!(received_fd, rights_fd);
        assert_fd_flags(received_fd, FdFlags::CLOEXEC);
        assert_same_object(received_fd, rights_fd);

        write_user_value(page + 1680, &1i32);
        expect_ok(
            SyscallArgs::new([
                stream_right as u64,
                SOL_SOCKET as u64,
                SO_PASSCRED,
                page + 1680,
                4,
                0,
            ])
            .call::<Setsockopt>(),
            0,
        );
        write_user_value(page + 2048, b"C");
        expect_ok(
            SyscallArgs::new([stream_left as u64, page + 2048, 1, 0, 0, 0]).call::<Write>(),
            1,
        );
        write_user_value(page + 2064, &[0u8]);
        let cred_recv_iov = TestRelibcIovec {
            iov_base: (page + 2064) as *mut u8,
            iov_len: 1,
        };
        write_user_value(page + 2080, &cred_recv_iov);
        write_user_value(page + 2112, &[0u8; 32]);
        let cred_recv_msg = TestRelibcMsgHdr {
            msg_name: core::ptr::null_mut(),
            msg_namelen: 0,
            msg_iov: (page + 2080) as *mut TestRelibcIovec,
            msg_iovlen: 1,
            msg_control: (page + 2112) as *mut u8,
            msg_controllen: 32,
            msg_flags: 0,
        };
        write_user_value(page + 2160, &cred_recv_msg);
        expect_ok(
            SyscallArgs::new([stream_right as u64, page + 2160, 0, 0, 0, 0]).call::<Recvmsg>(),
            1,
        );
        assert_user_bytes(page + 2064, b"C");
        let cred_recv_after = read_user_value::<TestRelibcMsgHdr>(page + 2160);
        assert_eq!(cred_recv_after.msg_flags, 0);
        assert_eq!(cred_recv_after.msg_controllen, 32);
        let credential_control = read_user_value::<TestLinuxCmsgHdr>(page + 2112);
        assert_eq!(credential_control.cmsg_len, 28);
        assert_eq!(credential_control.cmsg_level, SOL_SOCKET);
        assert_eq!(credential_control.cmsg_type, SCM_CREDENTIALS);
        let received_cred = read_user_value::<TestLinuxUcred>(page + 2128);
        let current = get_current_process();
        let current = current.lock();
        assert_eq!(received_cred.pid, current.pid.0 as i32);
        assert_eq!(received_cred.uid, current.effective_uid);
        assert_eq!(received_cred.gid, current.effective_gid);
        drop(current);

        write_user_value(page + 2208, b"T");
        expect_ok(
            SyscallArgs::new([stream_left as u64, page + 2208, 1, 0, 0, 0]).call::<Write>(),
            1,
        );
        write_user_value(page + 2224, &[0u8]);
        let trunc_recv_iov = TestRelibcIovec {
            iov_base: (page + 2224) as *mut u8,
            iov_len: 1,
        };
        write_user_value(page + 2240, &trunc_recv_iov);
        write_user_value(page + 2272, &TestLinuxCmsgHdr::default());
        let trunc_recv_msg = TestRelibcMsgHdr {
            msg_name: core::ptr::null_mut(),
            msg_namelen: 0,
            msg_iov: (page + 2240) as *mut TestRelibcIovec,
            msg_iovlen: 1,
            msg_control: (page + 2272) as *mut u8,
            msg_controllen: core::mem::size_of::<TestLinuxCmsgHdr>(),
            msg_flags: 0,
        };
        write_user_value(page + 2304, &trunc_recv_msg);
        expect_ok(
            SyscallArgs::new([stream_right as u64, page + 2304, 0, 0, 0, 0]).call::<Recvmsg>(),
            1,
        );
        assert_user_bytes(page + 2224, b"T");
        let trunc_recv_after = read_user_value::<TestRelibcMsgHdr>(page + 2304);
        assert_eq!(trunc_recv_after.msg_flags & MSG_CTRUNC, MSG_CTRUNC);
        assert_eq!(
            trunc_recv_after.msg_controllen,
            core::mem::size_of::<TestLinuxCmsgHdr>()
        );

        expect_errno(
            SyscallArgs::new([stream_left as u64, 0, 0, 0, 0, 0]).call::<Sendmsg>(),
            SyscallError::BadAddress,
        );
        expect_errno(
            SyscallArgs::new([stream_right as u64, 0, 0, 0, 0, 0]).call::<Recvmsg>(),
            SyscallError::BadAddress,
        );

        let dgram_pair_page = page + 1728;
        expect_ok(
            SyscallArgs::new([AF_UNIX, SOCK_DGRAM, 0, dgram_pair_page, 0, 0]).call::<Socketpair>(),
            0,
        );
        let dgram_left = read_user_value::<i32>(dgram_pair_page) as usize;
        let dgram_right = read_user_value::<i32>(dgram_pair_page + 4) as usize;
        write_user_value(page + 1744, b"go");
        write_user_value(page + 1760, b"again");
        let sendmmsg_iov = [
            TestRelibcIovec {
                iov_base: (page + 1744) as *mut u8,
                iov_len: 2,
            },
            TestRelibcIovec {
                iov_base: (page + 1760) as *mut u8,
                iov_len: 5,
            },
        ];
        write_user_value(page + 1792, &sendmmsg_iov);
        let msgvec = [
            TestRelibcMmsghdr {
                msg_hdr: TestRelibcMsgHdr {
                    msg_name: core::ptr::null_mut(),
                    msg_namelen: 0,
                    msg_iov: (page + 1792) as *mut TestRelibcIovec,
                    msg_iovlen: 1,
                    msg_control: core::ptr::null_mut(),
                    msg_controllen: 0,
                    msg_flags: 0,
                },
                msg_len: 0,
            },
            TestRelibcMmsghdr {
                msg_hdr: TestRelibcMsgHdr {
                    msg_name: core::ptr::null_mut(),
                    msg_namelen: 0,
                    msg_iov: (page + 1792 + core::mem::size_of::<TestRelibcIovec>() as u64)
                        as *mut TestRelibcIovec,
                    msg_iovlen: 1,
                    msg_control: core::ptr::null_mut(),
                    msg_controllen: 0,
                    msg_flags: 0,
                },
                msg_len: 0,
            },
        ];
        write_user_value(page + 1856, &msgvec);
        expect_ok(
            SyscallArgs::new([dgram_left as u64, page + 1856, 2, 0, 0, 0]).call::<Sendmmsg>(),
            2,
        );
        let sent_vec = read_user_value::<[TestRelibcMmsghdr; 2]>(page + 1856);
        assert_eq!(sent_vec[0].msg_len, 2);
        assert_eq!(sent_vec[1].msg_len, 5);
        expect_ok(
            SyscallArgs::new([dgram_right as u64, page + 2000, 8, 0, 0, 0]).call::<Recvfrom>(),
            2,
        );
        assert_user_bytes(page + 2000, b"go");
        expect_ok(
            SyscallArgs::new([dgram_right as u64, page + 2016, 8, 0, 0, 0]).call::<Recvfrom>(),
            5,
        );
        assert_user_bytes(page + 2016, b"again");
        expect_errno(
            SyscallArgs::new([dgram_left as u64, 0, 1, 0, 0, 0]).call::<Sendmmsg>(),
            SyscallError::BadAddress,
        );

        close_test_fd(dgram_right);
        close_test_fd(dgram_left);
        close_test_fd(received_fd);
        close_test_fd(stream_right);
        close_test_fd(stream_left);
        close_test_fd(rights_fd);
        close_test_fd(receiver);
        close_test_fd(sender);
        close_test_fd(accepted);
        close_test_fd(client);
        close_test_fd(listener);
    }
}
