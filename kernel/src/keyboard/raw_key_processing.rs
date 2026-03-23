use pc_keyboard::KeyCode;

pub fn raw_key_to_escape_sequence(key: KeyCode) -> Option<&'static [u8]> {
    let sequence: &'static [u8] = match key {
        KeyCode::ArrowUp => b"\x1b[A" as &[u8],
        KeyCode::ArrowDown => b"\x1b[B" as &[u8],
        KeyCode::ArrowRight => b"\x1b[C" as &[u8],
        KeyCode::ArrowLeft => b"\x1b[D" as &[u8],
        KeyCode::Home => b"\x1b[H" as &[u8],
        KeyCode::End => b"\x1b[F" as &[u8],
        KeyCode::Insert => b"\x1b[2~" as &[u8],
        KeyCode::Delete => b"\x1b[3~" as &[u8],
        KeyCode::PageUp => b"\x1b[5~" as &[u8],
        KeyCode::PageDown => b"\x1b[6~" as &[u8],
        KeyCode::F1 => b"\x1bOP" as &[u8],
        KeyCode::F2 => b"\x1bOQ" as &[u8],
        KeyCode::F3 => b"\x1bOR" as &[u8],
        KeyCode::F4 => b"\x1bOS" as &[u8],
        KeyCode::F5 => b"\x1b[15~" as &[u8],
        KeyCode::F6 => b"\x1b[17~" as &[u8],
        KeyCode::F7 => b"\x1b[18~" as &[u8],
        KeyCode::F8 => b"\x1b[19~" as &[u8],
        KeyCode::F9 => b"\x1b[20~" as &[u8],
        KeyCode::F10 => b"\x1b[21~" as &[u8],
        KeyCode::F11 => b"\x1b[23~" as &[u8],
        KeyCode::F12 => b"\x1b[24~" as &[u8],
        _ => return None,
    };

    Some(sequence)
}
