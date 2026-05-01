use alloc::{format, string::String, vec::Vec};

#[derive(Debug, Default)]
pub struct OutputFilter {
    pending_escape_buffer: String,
}

#[derive(Debug, Default)]
pub struct FilteredOutput {
    pub display_text: String,
    pub responses: Vec<String>,
}

enum XtgettcapParse {
    Complete { next_index: usize, response: String },
    Incomplete,
    NotXtgettcap,
}

impl OutputFilter {
    pub fn filter(&mut self, text: &str) -> FilteredOutput {
        let mut input = String::with_capacity(self.pending_escape_buffer.len() + text.len());
        input.push_str(&self.pending_escape_buffer);
        input.push_str(text);
        self.pending_escape_buffer.clear();

        let mut result = FilteredOutput {
            display_text: String::with_capacity(input.len()),
            responses: Vec::new(),
        };
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
                    self.pending_escape_buffer
                        .push_str(&input[sequence_start..]);
                    break;
                }
                continue;
            }

            match parse_xtgettcap(bytes, index) {
                XtgettcapParse::Complete {
                    next_index,
                    response,
                } => {
                    result.responses.push(response);
                    index = next_index;
                    continue;
                }
                XtgettcapParse::Incomplete => {
                    self.pending_escape_buffer.push_str(&input[index..]);
                    break;
                }
                XtgettcapParse::NotXtgettcap => {}
            }

            result.display_text.push(bytes[index] as char);
            index += 1;
        }

        result
    }
}

fn parse_xtgettcap(bytes: &[u8], start: usize) -> XtgettcapParse {
    if bytes.get(start) != Some(&0x1b) || bytes.get(start + 1) != Some(&b'P') {
        return XtgettcapParse::NotXtgettcap;
    }
    if bytes.get(start + 2) != Some(&b'+') || bytes.get(start + 3) != Some(&b'q') {
        return XtgettcapParse::NotXtgettcap;
    }

    let mut end = start + 4;
    while end + 1 < bytes.len() {
        if bytes[end] == 0x1b && bytes[end + 1] == b'\\' {
            let payload = match core::str::from_utf8(&bytes[start + 4..end]) {
                Ok(payload) => payload,
                Err(_) => {
                    return XtgettcapParse::Complete {
                        next_index: end + 2,
                        response: String::from("\x1bP0+r\x1b\\"),
                    };
                }
            };

            return XtgettcapParse::Complete {
                next_index: end + 2,
                response: xtgettcap_response(payload),
            };
        }
        end += 1;
    }

    XtgettcapParse::Incomplete
}

fn xtgettcap_response(payload: &str) -> String {
    let mut pairs = Vec::new();
    for encoded_name in payload.split(';') {
        let Some(name) = decode_hex_ascii(encoded_name) else {
            return String::from("\x1bP0+r\x1b\\");
        };

        let value = match name.as_str() {
            "name" | "TN" => "linux",
            "Co" | "colors" => "8",
            "RGB" => "-1",
            _ => return String::from("\x1bP0+r\x1b\\"),
        };
        pairs.push(format!("{}={}", encoded_name, encode_hex_ascii(value)));
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
