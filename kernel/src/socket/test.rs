use alloc::vec;

use crate::{
    net::InetAddress,
    object::{
        FileFlags, Object,
        config::ConfigurateRequest,
        error::ObjectError,
        linux_ioctl::{RAW_IOCTL_FIOCLEX, RAW_IOCTL_FIONBIO},
        netlink::NetlinkSocketObject,
        traits::Configuratable,
    },
    polling::{event::PollableEvent, object::Pollable, poller::PollerObject},
    socket::{
        AF_INET, NETLINK_ROUTE, SO_RCVTIMEO_NEW, SO_RCVTIMEO_OLD, SO_SNDTIMEO_NEW, SO_SNDTIMEO_OLD,
        SOCK_DGRAM, SOCK_RAW, UnixSocketObject,
        inet::InetSocketObject,
        name::{parse_unix_socket_path, serialize_unix_addr},
        registry::UnixSocketRegistryKey,
        socket_timeout_option_len,
    },
};

crate::test!(
    unix_sockaddr_round_trip,
    "unix sockaddr round trips path and abstract names",
    unix_sockaddr_round_trips_path_and_abstract_names
);
crate::test!(
    inet_sockaddr_byte_order,
    "inet sockaddr uses network byte order for ports",
    inet_sockaddr_uses_network_byte_order_for_ports
);
crate::test!(
    timeout_sockopt_sizes,
    "timeout sockopts have linux timeval size",
    timeout_sockopts_have_linux_timeval_size
);
crate::test!(
    unix_registry_key_ordering,
    "unix registry keys order abstract before path by derived key ordering",
    unix_registry_keys_order_abstract_before_path_by_derived_key_ordering
);
crate::test!(
    socket_and_netlink_ioctl_semantics,
    "socket and netlink ioctls follow linux rules",
    socket_and_netlink_ioctls_follow_linux_rules
);
crate::test!(
    unix_stream_shutdown_poll_semantics,
    "unix stream shutdown keeps RDHUP separate from HUP readiness",
    unix_stream_shutdown_poll_semantics_follow_linux_rules
);
crate::test!(
    unix_listener_connect_wakes_poller,
    "unix listener connect wakes pollers without self-deadlocking on the pending queue",
    unix_listener_connect_wakes_poller_without_self_deadlock
);
crate::test!(
    unix_pathname_socket_bind_connect,
    "unix pathname sockets bind and connect without reborrowing vfs",
    unix_pathname_sockets_bind_and_connect_without_reborrowing_vfs
);
crate::test!(
    inet_listener_poll_semantics,
    "inet listener is not spuriously writable while listening",
    inet_listener_poll_semantics_follow_linux_rules
);
crate::test!(
    inet_listener_accept_readiness_semantics,
    "inet listener stays unreadable without pending accepted connections",
    inet_listener_accept_readiness_semantics_follow_linux_rules
);
crate::test!(
    object_wait_only_wakes_target_listener_readers,
    "listener wait pollers ignore unrelated socket activity",
    object_wait_only_wakes_target_listener_readers
);
crate::test!(
    object_wait_only_wakes_target_inet_listener_readers,
    "inet listener wait pollers ignore unrelated socket activity",
    object_wait_only_wakes_target_inet_listener_readers
);
crate::test!(
    object_wait_recv_ignores_unrelated_writable_events,
    "recv wait pollers ignore unrelated writable activity",
    object_wait_recv_ignores_unrelated_writable_events
);
crate::test!(
    object_wait_send_ignores_unrelated_readable_events,
    "send wait pollers ignore unrelated readable activity",
    object_wait_send_ignores_unrelated_readable_events
);

fn unix_sockaddr_round_trips_path_and_abstract_names() {
    let pathname = serialize_unix_addr(Some("/tmp/socket"));
    assert_eq!(parse_unix_socket_path(&pathname).unwrap(), "/tmp/socket");

    let mut abstract_name = serialize_unix_addr(None);
    abstract_name.extend_from_slice(b"\0display");
    assert_eq!(parse_unix_socket_path(&abstract_name).unwrap(), "\0display");

    assert!(parse_unix_socket_path(&[1]).is_err());
    assert!(parse_unix_socket_path(&[1, 0]).is_err());
}

fn inet_sockaddr_uses_network_byte_order_for_ports() {
    let addr = InetAddress::new([127, 0, 0, 1], 0x1234);
    let encoded = InetSocketObject::encode_addr(addr);

    assert_eq!(&encoded[2..4], &[0x12, 0x34]);
    assert_eq!(InetSocketObject::decode_addr(&encoded).unwrap(), addr);
    assert!(InetSocketObject::decode_addr(&encoded[..8]).is_err());
}

fn timeout_sockopts_have_linux_timeval_size() {
    for option in [
        SO_RCVTIMEO_OLD,
        SO_SNDTIMEO_OLD,
        SO_RCVTIMEO_NEW,
        SO_SNDTIMEO_NEW,
    ] {
        assert_eq!(socket_timeout_option_len(option), Some(16));
    }
    assert_eq!(socket_timeout_option_len(0), None);
}

fn unix_registry_keys_order_abstract_before_path_by_derived_key_ordering() {
    let abstract_key = UnixSocketRegistryKey::Abstract("\0x".into());
    let path_key = UnixSocketRegistryKey::Path {
        mount_device_id: 1,
        inode: 2,
    };
    assert!(abstract_key < path_key);
}

fn socket_and_netlink_ioctls_follow_linux_rules() {
    let unix = UnixSocketObject::default();
    let inet = InetSocketObject::create(AF_INET, SOCK_DGRAM, 0).unwrap();
    let netlink = NetlinkSocketObject::create(SOCK_RAW, NETLINK_ROUTE).unwrap();

    let mut nonblocking = 1i32;
    assert_eq!(
        unix.configure(ConfigurateRequest::RawIoctl {
            request: RAW_IOCTL_FIONBIO,
            arg: (&mut nonblocking as *mut i32) as u64,
        })
        .unwrap(),
        0
    );
    assert!(unix.flags.lock().contains(FileFlags::NONBLOCK));

    nonblocking = 0;
    assert_eq!(
        inet.configure(ConfigurateRequest::RawIoctl {
            request: RAW_IOCTL_FIONBIO,
            arg: (&mut nonblocking as *mut i32) as u64,
        })
        .unwrap(),
        0
    );
    assert!(
        !inet
            .clone()
            .get_flags()
            .unwrap()
            .contains(FileFlags::NONBLOCK)
    );

    nonblocking = 7;
    assert_eq!(
        netlink
            .configure(ConfigurateRequest::RawIoctl {
                request: RAW_IOCTL_FIONBIO,
                arg: (&mut nonblocking as *mut i32) as u64,
            })
            .unwrap(),
        0
    );
    assert!(
        netlink
            .clone()
            .get_flags()
            .unwrap()
            .contains(FileFlags::NONBLOCK)
    );

    for socket in [
        &unix as &dyn Configuratable,
        inet.as_ref(),
        netlink.as_ref(),
    ] {
        let mut outq = -1i32;
        assert_eq!(
            socket
                .configure(ConfigurateRequest::LinuxTiocoutq(&mut outq))
                .unwrap(),
            0
        );
        assert_eq!(outq, 0);
        assert_eq!(
            socket
                .configure(ConfigurateRequest::RawIoctl {
                    request: RAW_IOCTL_FIOCLEX,
                    arg: 0,
                })
                .unwrap(),
            0
        );
    }

    assert!(matches!(
        unix.configure(ConfigurateRequest::RawIoctl {
            request: RAW_IOCTL_FIONBIO,
            arg: 0,
        }),
        Err(ObjectError::BadAddress)
    ));
    assert!(matches!(
        inet.configure(ConfigurateRequest::LinuxTiocoutq(core::ptr::null_mut())),
        Err(ObjectError::BadAddress)
    ));
    assert!(matches!(
        netlink.configure(ConfigurateRequest::RawIoctl {
            request: 0xdead_beef,
            arg: 0,
        }),
        Err(ObjectError::InvalidRequest)
    ));
}

fn unix_stream_shutdown_poll_semantics_follow_linux_rules() {
    let (left, right) =
        UnixSocketObject::pair(crate::socket::AF_UNIX, crate::socket::SOCK_STREAM, 0)
            .expect("unix stream socketpair should be created");

    assert!(!left.is_event_ready(PollableEvent::Closed));
    assert!(!left.is_event_ready(PollableEvent::ReadClosed));

    right.shutdown(1).unwrap();

    assert!(left.is_event_ready(PollableEvent::CanBeRead));
    assert!(left.is_event_ready(PollableEvent::CanBeWritten));
    assert!(left.is_event_ready(PollableEvent::ReadClosed));
    assert!(!left.is_event_ready(PollableEvent::Closed));

    drop(right);

    assert!(left.is_event_ready(PollableEvent::Closed));
    assert!(left.is_event_ready(PollableEvent::ReadClosed));
}

fn unix_listener_connect_wakes_poller_without_self_deadlock() {
    let listener = UnixSocketObject::create(crate::socket::AF_UNIX, crate::socket::SOCK_STREAM, 0)
        .expect("listener socket should be created");
    listener
        .bind("\0poll-listener-self-deadlock".into())
        .expect("listener should bind");
    listener.listen(1).expect("listener should listen");

    let poller = PollerObject::new();
    let listener_object: crate::object::misc::ObjectRef = listener.clone();
    poller.register_obj(listener_object, PollableEvent::CanBeRead, 0x44);

    let client = UnixSocketObject::create(crate::socket::AF_UNIX, crate::socket::SOCK_STREAM, 0)
        .expect("client socket should be created");
    client
        .connect("\0poll-listener-self-deadlock".into())
        .expect("connect should succeed without deadlocking");

    assert!(listener.is_event_ready(PollableEvent::CanBeRead));
    assert!(poller.has_woken_events());

    let ready = poller.take_woken_events(1);
    assert_eq!(ready.len(), 1);
    assert_eq!(ready[0].data, 0x44);
    assert_eq!(ready[0].event, PollableEvent::CanBeRead);
}

fn unix_pathname_sockets_bind_and_connect_without_reborrowing_vfs() {
    let path = "/tmp/unix-pathname-socket-reborrow";
    let _ = crate::filesystem::vfs::VirtualFS
        .lock()
        .delete_file(crate::filesystem::path::Path::new(path));

    let listener = UnixSocketObject::create(crate::socket::AF_UNIX, crate::socket::SOCK_STREAM, 0)
        .expect("listener socket should be created");
    listener.bind(path.into()).expect("listener should bind");
    listener.listen(1).expect("listener should listen");

    let client = UnixSocketObject::create(crate::socket::AF_UNIX, crate::socket::SOCK_STREAM, 0)
        .expect("client socket should be created");
    client
        .connect(path.into())
        .expect("pathname connect should succeed");
}

fn inet_listener_poll_semantics_follow_linux_rules() {
    let listener = InetSocketObject::create(AF_INET, crate::socket::SOCK_STREAM, 0)
        .expect("inet listener should be created");
    listener
        .bind(InetAddress::new([127, 0, 0, 1], 22345))
        .expect("listener should bind");
    listener.listen(1).expect("listener should listen");

    assert!(!listener.is_event_ready(PollableEvent::CanBeWritten));

    let client = InetSocketObject::create(AF_INET, crate::socket::SOCK_STREAM, 0)
        .expect("inet client should be created");
    client
        .connect(InetAddress::new([127, 0, 0, 1], 22345))
        .expect("client connect should succeed");

    crate::net::poll();

    assert!(!listener.is_event_ready(PollableEvent::CanBeWritten));
}

fn inet_listener_accept_readiness_semantics_follow_linux_rules() {
    let listener = InetSocketObject::create(AF_INET, crate::socket::SOCK_STREAM, 0)
        .expect("inet listener should be created");
    listener
        .clone()
        .set_flags(FileFlags::NONBLOCK)
        .expect("listener should become nonblocking");
    listener
        .bind(InetAddress::new([127, 0, 0, 1], 22348))
        .expect("listener should bind");
    listener.listen(1).expect("listener should listen");

    assert!(!listener.is_event_ready(PollableEvent::CanBeRead));
    assert!(matches!(
        listener.accept(),
        Err(crate::socket::SocketError::TryAgain)
    ));
}

fn object_wait_only_wakes_target_listener_readers() {
    let listener = UnixSocketObject::create(crate::socket::AF_UNIX, crate::socket::SOCK_STREAM, 0)
        .expect("listener socket should be created");
    listener
        .bind("\0wait-target-listener".into())
        .expect("listener should bind");
    listener.listen(1).expect("listener should listen");

    let other_listener =
        UnixSocketObject::create(crate::socket::AF_UNIX, crate::socket::SOCK_STREAM, 0)
            .expect("other listener socket should be created");
    other_listener
        .bind("\0wait-other-listener".into())
        .expect("other listener should bind");
    other_listener
        .listen(1)
        .expect("other listener should listen");

    let wait_poller = PollerObject::new();
    let listener_object: crate::object::misc::ObjectRef = listener.clone();
    wait_poller.register_obj(listener_object, PollableEvent::CanBeRead, 0x11);

    let other_client =
        UnixSocketObject::create(crate::socket::AF_UNIX, crate::socket::SOCK_STREAM, 0)
            .expect("other client socket should be created");
    other_client
        .connect("\0wait-other-listener".into())
        .expect("unrelated connect should succeed");

    assert!(!wait_poller.has_woken_events());

    let client = UnixSocketObject::create(crate::socket::AF_UNIX, crate::socket::SOCK_STREAM, 0)
        .expect("client socket should be created");
    client
        .connect("\0wait-target-listener".into())
        .expect("target connect should succeed");

    assert!(wait_poller.has_woken_events());
    let ready = wait_poller.take_woken_events(1);
    assert_eq!(ready.len(), 1);
    assert_eq!(ready[0].data, 0x11);
    assert_eq!(ready[0].event, PollableEvent::CanBeRead);
}

fn object_wait_only_wakes_target_inet_listener_readers() {
    let listener = InetSocketObject::create(AF_INET, crate::socket::SOCK_STREAM, 0)
        .expect("inet listener should be created");
    listener
        .bind(InetAddress::new([127, 0, 0, 1], 22346))
        .expect("listener should bind");
    listener.listen(1).expect("listener should listen");

    let other_listener = InetSocketObject::create(AF_INET, crate::socket::SOCK_STREAM, 0)
        .expect("other inet listener should be created");
    other_listener
        .bind(InetAddress::new([127, 0, 0, 1], 22347))
        .expect("other listener should bind");
    other_listener
        .listen(1)
        .expect("other listener should listen");

    let wait_poller = PollerObject::new();
    let listener_object: crate::object::misc::ObjectRef = listener.clone();
    wait_poller.register_obj(listener_object, PollableEvent::CanBeRead, 0x22);

    let other_client = InetSocketObject::create(AF_INET, crate::socket::SOCK_STREAM, 0)
        .expect("other inet client should be created");
    other_client
        .connect(InetAddress::new([127, 0, 0, 1], 22347))
        .expect("unrelated connect should succeed");
    for _ in 0..4 {
        crate::net::poll();
        assert!(!wait_poller.push_already_ready_events());
    }
    assert!(!wait_poller.has_woken_events());
}

fn object_wait_recv_ignores_unrelated_writable_events() {
    let (reader, writer) =
        UnixSocketObject::pair(crate::socket::AF_UNIX, crate::socket::SOCK_STREAM, 0)
            .expect("unix stream socketpair should be created");
    let (other_left, _other_right) =
        UnixSocketObject::pair(crate::socket::AF_UNIX, crate::socket::SOCK_STREAM, 0)
            .expect("unrelated stream socketpair should be created");

    let wait_poller = PollerObject::new();
    let reader_object: crate::object::misc::ObjectRef = reader.clone();
    wait_poller.register_obj(reader_object, PollableEvent::CanBeRead, 0x33);

    assert!(other_left.is_event_ready(PollableEvent::CanBeWritten));
    let other_object: crate::object::misc::ObjectRef = other_left.clone();
    crate::thread::yielding::wake_pollers_for_object(other_object, PollableEvent::CanBeWritten);
    assert!(!wait_poller.has_woken_events());

    writer
        .write_socket(b"hello")
        .expect("writer should make reader readable");
    assert!(wait_poller.has_woken_events());
    let ready = wait_poller.take_woken_events(1);
    assert_eq!(ready.len(), 1);
    assert_eq!(ready[0].data, 0x33);
    assert_eq!(ready[0].event, PollableEvent::CanBeRead);
}

fn object_wait_send_ignores_unrelated_readable_events() {
    let (left, right) =
        UnixSocketObject::pair(crate::socket::AF_UNIX, crate::socket::SOCK_STREAM, 0)
            .expect("unix stream socketpair should be created");
    let (other_left, other_right) =
        UnixSocketObject::pair(crate::socket::AF_UNIX, crate::socket::SOCK_STREAM, 0)
            .expect("unrelated stream socketpair should be created");

    let filler = vec![0u8; crate::socket::STREAM_RECV_CAPACITY];
    left.write_socket(&filler)
        .expect("initial write should fill peer receive buffer");
    assert!(!left.is_event_ready(PollableEvent::CanBeWritten));

    let wait_poller = PollerObject::new();
    let left_object: crate::object::misc::ObjectRef = left.clone();
    wait_poller.register_obj(left_object, PollableEvent::CanBeWritten, 0x44);

    other_right
        .write_socket(b"x")
        .expect("unrelated write should make only the unrelated peer readable");
    let other_object: crate::object::misc::ObjectRef = other_left.clone();
    crate::thread::yielding::wake_pollers_for_object(other_object, PollableEvent::CanBeRead);
    assert!(!wait_poller.has_woken_events());

    let mut one = [0u8; 1];
    right
        .read_socket(&mut one)
        .expect("peer read should free one byte of capacity");
    assert!(wait_poller.has_woken_events());
    let ready = wait_poller.take_woken_events(1);
    assert_eq!(ready.len(), 1);
    assert_eq!(ready[0].data, 0x44);
    assert_eq!(ready[0].event, PollableEvent::CanBeWritten);
}
