use alloc::collections::vec_deque::VecDeque;

use super::object::TerminalSettings;

/// Apply the terminal input line discipline to one incoming byte.
///
/// The callbacks let tty and pty share the same state machine while choosing
/// different backing queues and echo destinations.
///
/// `QueueByte` receives bytes that should become readable from the terminal.
/// `EchoBytes` receives bytes that should be echoed back to the terminal.
/// `Interrupt` runs when the input byte should trigger a line-discipline
/// generated signal such as Ctrl-C.
pub fn process_input_byte<QueueByte, EchoBytes, Interrupt>(
    info: &TerminalSettings,
    line_buffer: &mut VecDeque<u8>,
    byte: u8,
    mut queue_byte: QueueByte,
    mut echo_bytes: EchoBytes,
    mut interrupt: Interrupt,
) where
    QueueByte: FnMut(u8),
    EchoBytes: FnMut(&[u8]),
    Interrupt: FnMut(),
{
    // We currently model Linux's default ICRNL behavior unconditionally:
    // terminal Enter keys often arrive as '\r', but userspace expects '\n'.
    let byte = if byte == b'\r' { b'\n' } else { byte };

    if byte == 0x03 {
        interrupt();
        return;
    }

    if !info.canonical {
        // In noncanonical mode, userspace consumes bytes immediately and
        // handles its own line editing.
        if info.echo {
            echo_bytes(&[byte]);
        }
        queue_byte(byte);
        return;
    }

    match byte {
        b'\n' => {
            if info.echo_newline {
                echo_bytes(b"\n");
            }
            // Canonical mode only exposes completed lines to readers.
            line_buffer.push_back(b'\n');
            while let Some(byte) = line_buffer.pop_front() {
                queue_byte(byte);
            }
        }
        0x08 | 0x7f => {
            if line_buffer.pop_back().is_some() && info.echo_delete {
                echo_bytes(b"\x08 \x08");
            }
        }
        byte => {
            if info.echo {
                echo_bytes(&[byte]);
            }
            line_buffer.push_back(byte);
        }
    }
}

/// Apply tty-style output post-processing to a byte slice.
///
/// At the moment this only models ONLCR, while preserving existing CRLF
/// sequences instead of expanding them into CRCRLF.
pub fn process_output_bytes<EmitByte>(
    info: &TerminalSettings,
    buffer: &[u8],
    mut emit_byte: EmitByte,
) where
    EmitByte: FnMut(u8),
{
    let mut prev_was_cr = false;
    for byte in buffer.iter().copied() {
        if byte == b'\n' && !prev_was_cr && info.map_output_newline_to_crlf {
            emit_byte(b'\r');
        }
        emit_byte(byte);
        prev_was_cr = byte == b'\r';
    }
}
