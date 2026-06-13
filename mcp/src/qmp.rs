use anyhow::{Context, Result, bail};
use image::{ImageBuffer, ImageFormat, Rgb};
use serde_json::{Value, json};
use std::{fs, io::Cursor, path::Path};
use tempfile::NamedTempFile;
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::UnixStream,
};

pub async fn screendump_png(socket: &Path) -> Result<Vec<u8>> {
    let ppm = NamedTempFile::new().context("failed to create temporary screendump file")?;
    let ppm_path = ppm.path().to_path_buf();
    execute(socket, "screendump", Some(json!({ "filename": ppm_path }))).await?;

    let image = decode_ppm_screendump(&ppm_path)?;
    let mut png = Cursor::new(Vec::new());
    image
        .write_to(&mut png, ImageFormat::Png)
        .context("failed to encode PNG")?;
    Ok(png.into_inner())
}

fn decode_ppm_screendump(path: &Path) -> Result<ImageBuffer<Rgb<u8>, Vec<u8>>> {
    let data = fs::read(path)
        .with_context(|| format!("failed to read QMP screendump {}", path.display()))?;
    let mut offset = 0;
    let magic = read_ppm_token(&data, &mut offset).context("missing PPM magic")?;
    if magic != b"P6" {
        bail!(
            "unsupported QMP screendump format: {}",
            String::from_utf8_lossy(&magic)
        );
    }
    let width = parse_ppm_u32(&read_ppm_token(&data, &mut offset).context("missing PPM width")?)?;
    let height = parse_ppm_u32(&read_ppm_token(&data, &mut offset).context("missing PPM height")?)?;
    let max_value =
        parse_ppm_u32(&read_ppm_token(&data, &mut offset).context("missing PPM max value")?)?;
    if max_value != 255 {
        bail!("unsupported QMP screendump max value: {max_value}");
    }
    let expected_len = width as usize * height as usize * 3;
    let pixels = data
        .get(offset..offset + expected_len)
        .with_context(|| {
            format!(
                "truncated QMP screendump: expected {expected_len} bytes of pixel data after PPM header"
            )
        })?
        .to_vec();
    ImageBuffer::from_raw(width, height, pixels)
        .context("failed to build image from QMP screendump")
}

fn read_ppm_token(data: &[u8], offset: &mut usize) -> Option<Vec<u8>> {
    loop {
        while *offset < data.len() && data[*offset].is_ascii_whitespace() {
            *offset += 1;
        }
        if *offset >= data.len() || data[*offset] != b'#' {
            break;
        }
        while *offset < data.len() && data[*offset] != b'\n' {
            *offset += 1;
        }
    }

    let start = *offset;
    while *offset < data.len() && !data[*offset].is_ascii_whitespace() {
        *offset += 1;
    }
    if start == *offset {
        return None;
    }
    let token = data[start..*offset].to_vec();
    if *offset < data.len() && data[*offset].is_ascii_whitespace() {
        *offset += 1;
    }
    Some(token)
}

fn parse_ppm_u32(token: &[u8]) -> Result<u32> {
    let token = std::str::from_utf8(token).context("PPM header contains non-UTF-8 token")?;
    token
        .parse()
        .with_context(|| format!("invalid PPM integer: {token}"))
}

pub async fn send_key(socket: &Path, keys: &[String]) -> Result<()> {
    if keys.is_empty() {
        bail!("keys must not be empty");
    }
    let qmp_keys = keys
        .iter()
        .map(|key| json!({ "type": "qcode", "data": key }))
        .collect::<Vec<_>>();
    execute(
        socket,
        "input-send-event",
        Some(json!({
            "events": [{
                "type": "key",
                "data": { "down": true, "key": qmp_keys[0] }
            }]
        })),
    )
    .await?;
    for key in qmp_keys.iter().skip(1) {
        execute(
            socket,
            "input-send-event",
            Some(json!({
                "events": [{ "type": "key", "data": { "down": true, "key": key } }]
            })),
        )
        .await?;
    }
    for key in qmp_keys.iter().rev() {
        execute(
            socket,
            "input-send-event",
            Some(json!({
                "events": [{ "type": "key", "data": { "down": false, "key": key } }]
            })),
        )
        .await?;
    }
    Ok(())
}

pub async fn type_text(socket: &Path, text: &str) -> Result<()> {
    if !text.is_ascii() {
        bail!("agent_type_text only supports ASCII text");
    }
    for byte in text.bytes() {
        let key = ascii_key(byte)?;
        if key.shift {
            send_key(socket, &["shift".to_string(), key.qcode.to_string()]).await?;
        } else {
            send_key(socket, &[key.qcode.to_string()]).await?;
        }
    }
    Ok(())
}

pub async fn mouse_move(socket: &Path, x: i64, y: i64) -> Result<()> {
    if mouse_move_absolute(socket, x, y).await.is_ok() {
        return Ok(());
    }
    mouse_move_relative(socket, x, y).await
}

async fn mouse_move_absolute(socket: &Path, x: i64, y: i64) -> Result<()> {
    execute(
        socket,
        "input-send-event",
        Some(json!({
            "events": [{
                "type": "abs",
                "data": {
                    "axis": "x",
                    "value": x
                }
            }, {
                "type": "abs",
                "data": {
                    "axis": "y",
                    "value": y
                }
            }]
        })),
    )
    .await?;
    Ok(())
}

async fn mouse_move_relative(socket: &Path, x: i64, y: i64) -> Result<()> {
    execute(
        socket,
        "input-send-event",
        Some(json!({
            "events": [{
                "type": "rel",
                "data": {
                    "axis": "x",
                    "value": x
                }
            }, {
                "type": "rel",
                "data": {
                    "axis": "y",
                    "value": y
                }
            }]
        })),
    )
    .await?;
    Ok(())
}

pub async fn mouse_click(socket: &Path, button: &str) -> Result<()> {
    let button = match button {
        "left" | "right" | "middle" => button,
        other => bail!("unsupported mouse button: {other}"),
    };
    for down in [true, false] {
        execute(
            socket,
            "input-send-event",
            Some(json!({
                "events": [{
                    "type": "btn",
                    "data": {
                        "down": down,
                        "button": button
                    }
                }]
            })),
        )
        .await?;
    }
    Ok(())
}

async fn execute(socket: &Path, command: &str, arguments: Option<Value>) -> Result<Value> {
    let stream = UnixStream::connect(socket)
        .await
        .with_context(|| format!("failed to connect QMP socket {}", socket.display()))?;
    let mut stream = BufReader::new(stream);
    let mut greeting = String::new();
    stream
        .read_line(&mut greeting)
        .await
        .context("failed to read QMP greeting")?;
    let greeting: Value =
        serde_json::from_str(greeting.trim()).context("failed to parse QMP greeting")?;
    if greeting.get("QMP").is_none() {
        bail!("unexpected QMP greeting: {greeting}");
    }

    write_qmp(stream.get_mut(), json!({ "execute": "qmp_capabilities" })).await?;
    read_return(&mut stream).await?;

    let mut request = json!({ "execute": command });
    if let Some(arguments) = arguments {
        request["arguments"] = arguments;
    }
    write_qmp(stream.get_mut(), request).await?;
    read_return(&mut stream).await
}

async fn write_qmp(stream: &mut UnixStream, value: Value) -> Result<()> {
    let mut encoded = serde_json::to_vec(&value).context("failed to encode QMP command")?;
    encoded.push(b'\n');
    stream
        .write_all(&encoded)
        .await
        .context("failed to write QMP command")
}

async fn read_return(stream: &mut BufReader<UnixStream>) -> Result<Value> {
    loop {
        let mut line = String::new();
        let read = stream
            .read_line(&mut line)
            .await
            .context("failed to read QMP response")?;
        if read == 0 {
            bail!("QMP connection closed before response");
        }
        let value: Value =
            serde_json::from_str(line.trim()).context("failed to parse QMP response")?;
        if let Some(error) = value.get("error") {
            bail!("QMP command failed: {error}");
        }
        if value.get("return").is_some() {
            return Ok(value);
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct KeySpec {
    qcode: &'static str,
    shift: bool,
}

fn ascii_key(byte: u8) -> Result<KeySpec> {
    let key = match byte {
        b'a'..=b'z' => key(letter_qcode(byte), false),
        b'A'..=b'Z' => key(letter_qcode(byte + 32), true),
        b'0'..=b'9' => key(digit_qcode(byte), false),
        b'\n' => KeySpec {
            qcode: "ret",
            shift: false,
        },
        b'\t' => KeySpec {
            qcode: "tab",
            shift: false,
        },
        b' ' => KeySpec {
            qcode: "spc",
            shift: false,
        },
        b'-' => key("minus", false),
        b'_' => key("minus", true),
        b'=' => key("equal", false),
        b'+' => key("equal", true),
        b'[' => key("bracket_left", false),
        b'{' => key("bracket_left", true),
        b']' => key("bracket_right", false),
        b'}' => key("bracket_right", true),
        b';' => key("semicolon", false),
        b':' => key("semicolon", true),
        b'\'' => key("apostrophe", false),
        b'"' => key("apostrophe", true),
        b',' => key("comma", false),
        b'<' => key("comma", true),
        b'.' => key("dot", false),
        b'>' => key("dot", true),
        b'/' => key("slash", false),
        b'?' => key("slash", true),
        b'\\' => key("backslash", false),
        b'|' => key("backslash", true),
        b'`' => key("grave_accent", false),
        b'~' => key("grave_accent", true),
        b'!' => key("1", true),
        b'@' => key("2", true),
        b'#' => key("3", true),
        b'$' => key("4", true),
        b'%' => key("5", true),
        b'^' => key("6", true),
        b'&' => key("7", true),
        b'*' => key("8", true),
        b'(' => key("9", true),
        b')' => key("0", true),
        other => bail!("unsupported ASCII byte: {other}"),
    };
    Ok(key)
}

fn key(qcode: &'static str, shift: bool) -> KeySpec {
    KeySpec { qcode, shift }
}

fn letter_qcode(byte: u8) -> &'static str {
    match byte {
        b'a' => "a",
        b'b' => "b",
        b'c' => "c",
        b'd' => "d",
        b'e' => "e",
        b'f' => "f",
        b'g' => "g",
        b'h' => "h",
        b'i' => "i",
        b'j' => "j",
        b'k' => "k",
        b'l' => "l",
        b'm' => "m",
        b'n' => "n",
        b'o' => "o",
        b'p' => "p",
        b'q' => "q",
        b'r' => "r",
        b's' => "s",
        b't' => "t",
        b'u' => "u",
        b'v' => "v",
        b'w' => "w",
        b'x' => "x",
        b'y' => "y",
        b'z' => "z",
        _ => unreachable!("validated letter qcode"),
    }
}

fn digit_qcode(byte: u8) -> &'static str {
    match byte {
        b'0' => "0",
        b'1' => "1",
        b'2' => "2",
        b'3' => "3",
        b'4' => "4",
        b'5' => "5",
        b'6' => "6",
        b'7' => "7",
        b'8' => "8",
        b'9' => "9",
        _ => unreachable!("validated digit qcode"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_non_ascii_text() {
        assert!(ascii_key("中".as_bytes()[0]).is_err());
    }

    #[test]
    fn maps_common_ascii() {
        assert_eq!(ascii_key(b'a').unwrap().qcode, "a");
        assert!(ascii_key(b'A').unwrap().shift);
        assert_eq!(ascii_key(b'\n').unwrap().qcode, "ret");
    }

    #[test]
    fn decodes_qmp_ppm_screendump() {
        let ppm = tempfile::NamedTempFile::new().unwrap();
        fs::write(ppm.path(), b"P6\n2 1\n255\n\xff\x00\x00\x00\xff\x00").unwrap();

        let image = decode_ppm_screendump(ppm.path()).unwrap();

        assert_eq!(image.width(), 2);
        assert_eq!(image.height(), 1);
        assert_eq!(image.get_pixel(0, 0), &Rgb([255, 0, 0]));
        assert_eq!(image.get_pixel(1, 0), &Rgb([0, 255, 0]));
    }
}
