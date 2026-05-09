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
