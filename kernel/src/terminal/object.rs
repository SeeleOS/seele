use core::str::from_utf8;

use alloc::{format, string::String, sync::Arc, vec::Vec};
use spin::Mutex;
use x86_64::instructions::interrupts::without_interrupts;

use crate::{
    impl_cast_function,
    object::{
        Object,
        config::{LinuxTermios2, LinuxWinsize},
        misc::ObjectResult,
        traits::{Configuratable, Writable},
    },
    s_print,
    object::tty_device::get_active_tty,
    terminal::term_trait::AbstractTerminal,
};

use super::linux_kd::LinuxConsoleState;

#[derive(Debug)]
pub struct TerminalObject {
    pub inner: Arc<Mutex<dyn AbstractTerminal>>,
    pub termios: Mutex<LinuxTermios2>,
    pub winsize: Mutex<LinuxWinsize>,
    pub linux_console: Arc<Mutex<LinuxConsoleState>>,
    output_escape_buffer: Mutex<String>,
}

impl TerminalObject {
    pub fn new(term: Arc<Mutex<dyn AbstractTerminal>>) -> Self {
        let window_size = term.lock().size();
        Self {
            termios: Mutex::new(LinuxTermios2::new_default()),
            winsize: Mutex::new(LinuxWinsize::from_rows_cols(
                window_size.rows,
                window_size.cols,
            )),
            inner: term,
            linux_console: Arc::new(Mutex::new(LinuxConsoleState::default())),
            output_escape_buffer: Mutex::new(String::new()),
        }
    }

    pub fn write_screen_text(&self, text: &str) {
        let filtered = self.filter_terminal_output(text);
        if !filtered.is_empty() {
            without_interrupts(|| {
                self.inner.lock().push_str(&filtered);
            });
        }
    }

    pub fn clear_screen(&self) {
        without_interrupts(|| {
            self.inner.lock().clear();
        });
    }

    fn filter_terminal_output(&self, text: &str) -> String {
        let mut pending = self.output_escape_buffer.lock();
        filter_terminal_output(text, &mut pending)
    }
}

fn filter_terminal_output(text: &str, pending: &mut String) -> String {
    let mut input = String::with_capacity(pending.len() + text.len());
    input.push_str(pending);
    input.push_str(text);
    pending.clear();

    let mut output = String::with_capacity(input.len());
    let bytes = input.as_bytes();
    let mut index = 0usize;

    while index < bytes.len() {
        if bytes[index] == 0x1b && bytes.get(index + 1) == Some(&b']') {
            let sequence_start = index;
            index += 2;
            let mut terminated = false;
            while let Some(&byte) = bytes.get(index) {
                index += 1;
                if byte == 0x07 {
                    terminated = true;
                    break;
                }
                if byte == 0x1b && bytes.get(index) == Some(&b'\\') {
                    index += 1;
                    terminated = true;
                    break;
                }
            }
            if !terminated {
                pending.push_str(&input[sequence_start..]);
                break;
            }
            continue;
        }

        if let Some((next_index, response)) = try_handle_xtgettcap(bytes, index) {
            get_active_tty().push_terminal_response_bytes(response.as_bytes());
            index = next_index;
            continue;
        }

        if bytes[index] == 0x1b
            && bytes.get(index + 1) == Some(&b'P')
            && bytes.get(index + 2) == Some(&b'+')
            && bytes.get(index + 3) == Some(&b'q')
        {
            pending.push_str(&input[index..]);
            break;
        }

        output.push(bytes[index] as char);
        index += 1;
    }

    output
}

fn try_handle_xtgettcap(bytes: &[u8], start: usize) -> Option<(usize, String)> {
    if bytes.get(start) != Some(&0x1b) || bytes.get(start + 1) != Some(&b'P') {
        return None;
    }
    if bytes.get(start + 2) != Some(&b'+') || bytes.get(start + 3) != Some(&b'q') {
        return None;
    }

    let mut end = start + 4;
    while end + 1 < bytes.len() {
        if bytes[end] == 0x1b && bytes[end + 1] == b'\\' {
            let payload = core::str::from_utf8(&bytes[start + 4..end]).ok()?;
            let response = xtgettcap_response(payload);
            return Some((end + 2, response));
        }
        end += 1;
    }

    None
}

fn xtgettcap_response(payload: &str) -> String {
    let mut pairs = Vec::new();
    for encoded_name in payload.split(';') {
        let Some(name) = decode_hex_ascii(encoded_name) else {
            return String::from("\x1bP0+r\x1b\\");
        };

        let value = match name.as_str() {
            "name" | "TN" => "linux",
            _ => return String::from("\x1bP0+r\x1b\\"),
        };
        pairs.push(format!("{}={}", encode_hex_ascii(&name), encode_hex_ascii(value)));
    }

    format!("\x1bP1+r{}\x1b\\", pairs.join(";"))
}

fn decode_hex_ascii(encoded: &str) -> Option<String> {
    if !encoded.len().is_multiple_of(2) {
        return None;
    }

    let mut out = String::with_capacity(encoded.len() / 2);
    let bytes = encoded.as_bytes();
    let mut index = 0usize;
    while index < bytes.len() {
        let high = decode_hex_nibble(bytes[index])?;
        let low = decode_hex_nibble(bytes[index + 1])?;
        out.push(((high << 4) | low) as char);
        index += 2;
    }
    Some(out)
}

fn decode_hex_nibble(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn encode_hex_ascii(text: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";

    let mut out = String::with_capacity(text.len() * 2);
    for byte in text.bytes() {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

impl Object for TerminalObject {
    impl_cast_function!("configuratable", Configuratable);
    impl_cast_function!("writable", Writable);
}

impl Writable for TerminalObject {
    fn write(&self, buffer: &[u8]) -> ObjectResult<usize> {
        let string = from_utf8(buffer).unwrap_or("Unsupported charcter");
        let filtered = self.filter_terminal_output(string);
        if !filtered.is_empty() {
            s_print!("{filtered}");
            without_interrupts(|| {
                self.inner.lock().push_str(&filtered);
            });
        }
        Ok(buffer.len())
    }
}
