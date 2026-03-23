use pc_keyboard::KeyCode;

pub fn raw_key_to_escape_sequence(key: KeyCode) -> Option<&'static [u8]> {
    let sequence = match key {
        KeyCode::ArrowUp => b"\x1b[A",
        KeyCode::ArrowDown => b"\x1b[B",
        KeyCode::ArrowRight => b"\x1b[C",
        KeyCode::ArrowLeft => b"\x1b[D",
        KeyCode::Home => b"\x1b[H",
        KeyCode::End => b"\x1b[F",
        KeyCode::Insert => b"\x1b[2~",
        KeyCode::Delete => b"\x1b[3~",
        KeyCode::PageUp => b"\x1b[5~",
        KeyCode::PageDown => b"\x1b[6~",
        KeyCode::F1 => b"\x1bOP",
        KeyCode::F2 => b"\x1bOQ",
        KeyCode::F3 => b"\x1bOR",
        KeyCode::F4 => b"\x1bOS",
        KeyCode::F5 => b"\x1b[15~",
        KeyCode::F6 => b"\x1b[17~",
        KeyCode::F7 => b"\x1b[18~",
        KeyCode::F8 => b"\x1b[19~",
        KeyCode::F9 => b"\x1b[20~",
        KeyCode::F10 => b"\x1b[21~",
        KeyCode::F11 => b"\x1b[23~",
        KeyCode::F12 => b"\x1b[24~",
        _ => return None,
    };

    Some(sequence)
}
