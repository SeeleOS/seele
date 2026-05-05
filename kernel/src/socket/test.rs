use crate::{
    net::InetAddress,
    socket::{
        SO_RCVTIMEO_NEW, SO_RCVTIMEO_OLD, SO_SNDTIMEO_NEW, SO_SNDTIMEO_OLD,
        inet::InetSocketObject,
        name::{parse_unix_socket_path, serialize_unix_addr},
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
