use alloc::vec::Vec;

use crate::{net, object::misc::get_object_current_process};

use super::{attr, socket::*};

impl NetlinkSocketObject {
    pub(super) fn handle_route_messages(&self, mut message: &[u8]) {
        while let Some((header, payload, consumed)) = self.request_header_and_payload(message) {
            let reply_pid = self.local_address().pid;

            match header.nlmsg_type {
                RTM_NEWLINK => self.handle_new_link(header, payload),
                RTM_GETLINK => self.handle_get_link(header, payload, reply_pid),
                RTM_GETADDR => self.handle_get_addr(header, payload, reply_pid),
                _ => self.enqueue_error_response(header, 0),
            }

            if consumed >= message.len() {
                break;
            }
            message = &message[consumed..];
        }
    }

    fn request_header_and_payload<'a>(
        &self,
        message: &'a [u8],
    ) -> Option<(NetlinkMessageHeader, &'a [u8], usize)> {
        if message.len() < core::mem::size_of::<NetlinkMessageHeader>() {
            return None;
        }

        let header =
            unsafe { core::ptr::read_unaligned(message.as_ptr().cast::<NetlinkMessageHeader>()) };
        let message_len = usize::try_from(header.nlmsg_len).ok()?;
        if message_len < core::mem::size_of::<NetlinkMessageHeader>() {
            return None;
        }
        let consumed = attr::align_to_4(message_len).min(message.len());
        if message_len > consumed {
            return None;
        }

        Some((
            header,
            &message[core::mem::size_of::<NetlinkMessageHeader>()..message_len],
            consumed,
        ))
    }

    fn handle_get_link(&self, header: NetlinkMessageHeader, payload: &[u8], reply_pid: u32) {
        let request = attr::read_struct_prefix::<IfInfoMessage>(payload).unwrap_or(IfInfoMessage {
            ifi_family: 0,
            ifi_pad: 0,
            ifi_type: 0,
            ifi_index: 0,
            ifi_flags: 0,
            ifi_change: 0,
        });
        let attrs_offset = core::mem::size_of::<IfInfoMessage>().min(payload.len());
        let request_name = attr::find_attribute(payload, attrs_offset, IFLA_IFNAME)
            .and_then(attr::parse_netlink_string);
        let request_alt_name = attr::find_attribute(payload, attrs_offset, IFLA_ALT_IFNAME)
            .and_then(attr::parse_netlink_string);
        let dump = (header.nlmsg_flags & NLM_F_DUMP) != 0;

        let mut matched = Vec::new();
        for interface in net::interfaces() {
            if request.ifi_index > 0 && interface.index != request.ifi_index {
                continue;
            }
            if request_name.is_some_and(|name| interface.name != name) {
                continue;
            }
            if request_alt_name.is_some() {
                continue;
            }
            matched.push(interface);
        }

        let should_dump = dump
            || (request.ifi_index == 0 && request_name.is_none() && request_alt_name.is_none());
        if should_dump {
            for interface in matched {
                self.queue_message(Self::encode_link_message(
                    header, interface, true, reply_pid,
                ));
            }
            self.queue_message(Self::encode_done_message(header.nlmsg_seq, reply_pid));
            return;
        }

        if let Some(interface) = matched.into_iter().next() {
            self.queue_message(Self::encode_link_message(
                header, interface, false, reply_pid,
            ));
        } else {
            self.enqueue_error_response(header, -19);
        }
    }

    fn handle_new_link(&self, header: NetlinkMessageHeader, payload: &[u8]) {
        let request = attr::read_struct_prefix::<IfInfoMessage>(payload).unwrap_or(IfInfoMessage {
            ifi_family: 0,
            ifi_pad: 0,
            ifi_type: 0,
            ifi_index: 0,
            ifi_flags: 0,
            ifi_change: 0,
        });
        let attrs_offset = core::mem::size_of::<IfInfoMessage>().min(payload.len());
        let request_name = attr::find_attribute(payload, attrs_offset, IFLA_IFNAME)
            .and_then(attr::parse_netlink_string);
        let request_alt_name = attr::find_attribute(payload, attrs_offset, IFLA_ALT_IFNAME)
            .and_then(attr::parse_netlink_string);
        if request_alt_name.is_some() {
            self.enqueue_error_response(header, -22);
            return;
        }

        let interfaces = net::interfaces();
        let requested_interface = interfaces.iter().copied().find(|interface| {
            (request.ifi_index <= 0 || interface.index == request.ifi_index)
                && request_name.is_none_or(|name| interface.name == name)
        });

        let Some(namespace_fd) = attr::find_attribute(payload, attrs_offset, IFLA_NET_NS_FD)
            .and_then(attr::parse_i32_attribute)
        else {
            if requested_interface.is_some() {
                self.enqueue_ack_from_header(header);
            } else {
                self.enqueue_error_response(header, -19);
            }
            return;
        };
        if namespace_fd < 0 {
            self.enqueue_error_response(header, -22);
            return;
        }

        let Some(interface) = interfaces.into_iter().find(|interface| !interface.loopback) else {
            self.enqueue_error_response(header, -19);
            return;
        };
        if request.ifi_index > 0 && interface.index != request.ifi_index {
            self.enqueue_error_response(header, -19);
            return;
        }
        if request_name.is_some_and(|name| interface.name != name) {
            self.enqueue_error_response(header, -19);
            return;
        }

        let Ok(namespace_object) = get_object_current_process(namespace_fd as u64) else {
            self.enqueue_error_response(header, -22);
            return;
        };
        let Ok(namespace) = namespace_object.as_net_namespace() else {
            self.enqueue_error_response(header, -22);
            return;
        };

        match net::move_primary_device_to_namespace(namespace.inode()) {
            Ok(()) => self.enqueue_ack_from_header(header),
            Err(net::NetError::NoDevice) => self.enqueue_error_response(header, -19),
            Err(net::NetError::InvalidArguments) => self.enqueue_error_response(header, -22),
            Err(net::NetError::TryAgain) => self.enqueue_error_response(header, -11),
            Err(net::NetError::NotConnected) => self.enqueue_error_response(header, -107),
            Err(net::NetError::AddressInUse) => self.enqueue_error_response(header, -98),
            Err(net::NetError::ConnectionRefused) => self.enqueue_error_response(header, -111),
            Err(net::NetError::BrokenPipe) => self.enqueue_error_response(header, -32),
        }
    }

    fn handle_get_addr(&self, header: NetlinkMessageHeader, payload: &[u8], reply_pid: u32) {
        let request = attr::read_struct_prefix::<IfAddrMessage>(payload).unwrap_or(IfAddrMessage {
            ifa_family: 0,
            ifa_prefixlen: 0,
            ifa_flags: 0,
            ifa_scope: 0,
            ifa_index: 0,
        });
        let dump = (header.nlmsg_flags & NLM_F_DUMP) != 0;
        let request_index = i32::try_from(request.ifa_index).unwrap_or(0);

        let mut matched = Vec::new();
        for interface in net::interfaces() {
            let Some((addr, prefix_len)) = interface.ipv4 else {
                continue;
            };
            if request.ifa_family != 0 && request.ifa_family != AF_INET {
                continue;
            }
            if request_index > 0 && interface.index != request_index {
                continue;
            }
            matched.push((interface, addr, prefix_len));
        }

        let should_dump = dump || request_index == 0;
        if should_dump {
            for (interface, addr, prefix_len) in matched {
                self.queue_message(Self::encode_addr_message(
                    header, interface, addr, prefix_len, true, reply_pid,
                ));
            }
            self.queue_message(Self::encode_done_message(header.nlmsg_seq, reply_pid));
            return;
        }

        if let Some((interface, addr, prefix_len)) = matched.into_iter().next() {
            self.queue_message(Self::encode_addr_message(
                header, interface, addr, prefix_len, false, reply_pid,
            ));
        } else {
            self.enqueue_error_response(header, 0);
        }
    }

    fn encode_link_message(
        request: NetlinkMessageHeader,
        interface: net::NetworkInterfaceInfo,
        multipart: bool,
        reply_pid: u32,
    ) -> Vec<u8> {
        let mut bytes = Vec::new();
        attr::append_struct(
            &mut bytes,
            &NetlinkMessageHeader {
                nlmsg_len: 0,
                nlmsg_type: RTM_NEWLINK,
                nlmsg_flags: if multipart { NLM_F_MULTI } else { 0 },
                nlmsg_seq: request.nlmsg_seq,
                nlmsg_pid: reply_pid,
            },
        );
        attr::append_struct(
            &mut bytes,
            &IfInfoMessage {
                ifi_family: 0,
                ifi_pad: 0,
                ifi_type: if interface.loopback {
                    ARPHRD_LOOPBACK
                } else {
                    ARPHRD_ETHER
                },
                ifi_index: interface.index,
                ifi_flags: if interface.loopback {
                    IFF_UP | IFF_LOOPBACK | IFF_RUNNING | IFF_LOWER_UP
                } else {
                    IFF_UP | IFF_BROADCAST | IFF_RUNNING | IFF_MULTICAST | IFF_LOWER_UP
                },
                ifi_change: u32::MAX,
            },
        );
        attr::append_string_attribute(&mut bytes, IFLA_IFNAME, interface.name);
        attr::append_attribute(&mut bytes, IFLA_ADDRESS, &interface.mac);
        attr::append_attribute(&mut bytes, IFLA_PERM_ADDRESS, &interface.mac);
        if !interface.loopback {
            attr::append_attribute(&mut bytes, IFLA_BROADCAST, &[0xff; 6]);
        }
        attr::append_u32_attribute(&mut bytes, IFLA_MTU, interface.mtu);
        attr::append_string_attribute(
            &mut bytes,
            IFLA_QDISC,
            if interface.loopback {
                "noqueue"
            } else {
                "fq_codel"
            },
        );
        attr::append_u32_attribute(&mut bytes, IFLA_TXQLEN, 1_000);
        attr::append_u8_attribute(&mut bytes, IFLA_OPERSTATE, IF_OPER_UP);
        attr::append_u8_attribute(&mut bytes, IFLA_LINKMODE, 0);
        attr::append_u32_attribute(&mut bytes, IFLA_NUM_TX_QUEUES, 1);
        attr::append_u32_attribute(&mut bytes, IFLA_NUM_RX_QUEUES, 1);
        attr::finalize_message_length(&mut bytes);
        bytes
    }

    fn encode_addr_message(
        request: NetlinkMessageHeader,
        interface: net::NetworkInterfaceInfo,
        addr: [u8; 4],
        prefix_len: u8,
        multipart: bool,
        reply_pid: u32,
    ) -> Vec<u8> {
        let mut bytes = Vec::new();
        attr::append_struct(
            &mut bytes,
            &NetlinkMessageHeader {
                nlmsg_len: 0,
                nlmsg_type: RTM_NEWADDR,
                nlmsg_flags: if multipart { NLM_F_MULTI } else { 0 },
                nlmsg_seq: request.nlmsg_seq,
                nlmsg_pid: reply_pid,
            },
        );
        attr::append_struct(
            &mut bytes,
            &IfAddrMessage {
                ifa_family: AF_INET,
                ifa_prefixlen: prefix_len,
                ifa_flags: IFA_F_PERMANENT,
                ifa_scope: if interface.loopback {
                    RT_SCOPE_HOST
                } else {
                    RT_SCOPE_UNIVERSE
                },
                ifa_index: interface.index as u32,
            },
        );
        attr::append_attribute(&mut bytes, IFA_ADDRESS, &addr);
        attr::append_attribute(&mut bytes, IFA_LOCAL, &addr);
        attr::append_string_attribute(&mut bytes, IFA_LABEL, interface.name);
        attr::append_u32_attribute(&mut bytes, IFA_FLAGS, u32::from(IFA_F_PERMANENT));
        attr::finalize_message_length(&mut bytes);
        bytes
    }

    fn encode_done_message(seq: u32, reply_pid: u32) -> Vec<u8> {
        let header = NetlinkMessageHeader {
            nlmsg_len: core::mem::size_of::<NetlinkMessageHeader>() as u32,
            nlmsg_type: NLMSG_DONE,
            nlmsg_flags: NLM_F_MULTI,
            nlmsg_seq: seq,
            nlmsg_pid: reply_pid,
        };
        let mut bytes = Vec::new();
        attr::append_struct(&mut bytes, &header);
        bytes
    }
}
