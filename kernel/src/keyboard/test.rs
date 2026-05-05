use crate::keyboard::encode_linux_raw_byte;

crate::test!(
    linux_raw_scancode_encoding,
    "linux raw mode remaps extended prefix bytes",
    linux_raw_mode_remaps_extended_prefix_bytes
);

fn linux_raw_mode_remaps_extended_prefix_bytes() {
    assert_eq!(encode_linux_raw_byte(0xE0), 0x60);
    assert_eq!(encode_linux_raw_byte(0xE1), 0x61);
    assert_eq!(encode_linux_raw_byte(0x1e), 0x1e);
}
