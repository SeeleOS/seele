use super::{
    qemu::{self, RunOptions},
    terminal::drain_serial_log,
};
use anyhow::{Context, Result};
use std::{
    fs,
    io::{BufRead, BufReader, Write},
    os::unix::net::UnixStream,
    path::Path,
    thread,
    time::{Duration, Instant},
};

pub struct InteractiveQemuResult {
    pub exit_code: i32,
    pub serial_output: String,
    pub failure: Option<String>,
}

pub fn run_qemu_interactive_capture(
    iso_path: &Path,
    options: &RunOptions,
    timeout: Duration,
    condition: impl FnMut(&str) -> bool,
) -> Result<InteractiveQemuResult> {
    let context = qemu::QemuRunContext::new(options);
    let mut cmd = qemu::build_qemu_command(iso_path, options, &context)?;
    let mut child = cmd.spawn().context("failed to start qemu-system-x86_64")?;
    let deadline = Instant::now() + timeout;
    let mut offset = 0;
    let mut serial_log = None;
    let mut captured = String::new();
    let mut condition = condition;

    let (exit_code, failure) = loop {
        if serial_log.is_none()
            && let Ok(opened) = fs::File::open(&context.serial_log)
        {
            serial_log = Some(opened);
        }
        if let Some(file) = serial_log.as_mut() {
            let output = drain_serial_log(file, &mut offset);
            if !output.is_empty() {
                print!("{output}");
                let _ = std::io::stdout().flush();
                captured.push_str(&output);
            }
            if condition(&captured) {
                let _ = child.kill();
                let _ = child.wait();
                break (0, None);
            }
        }

        match child.try_wait() {
            Ok(Some(status)) => {
                if let Some(path) = &context.debug_log {
                    qemu::report_qemu_fault(path)?;
                }
                break (
                    status.code().unwrap_or(1).max(1),
                    Some("qemu exited before serial condition was observed".to_string()),
                );
            }
            Ok(None) => {}
            Err(err) => {
                let _ = child.kill();
                let _ = child.wait();
                break (1, Some(format!("failed to poll qemu: {err}")));
            }
        }

        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            break (
                1,
                Some("timed out waiting for serial condition".to_string()),
            );
        }

        thread::sleep(Duration::from_millis(10));
    };

    qemu::cleanup_qemu_context(&context);
    qemu::cleanup_qemu_debug_log(&context);
    Ok(InteractiveQemuResult {
        exit_code,
        serial_output: captured,
        failure,
    })
}

pub fn qmp_type_text(socket: &Path, text: &str) -> Result<()> {
    if !text.is_ascii() {
        anyhow::bail!("type_text only supports ASCII text");
    }
    for byte in text.bytes() {
        let key = ascii_key(byte)?;
        if key.shift {
            qmp_send_key(socket, &["shift".to_string(), key.qcode.to_string()])?;
        } else {
            qmp_send_key(socket, &[key.qcode.to_string()])?;
        }
    }
    Ok(())
}

fn qmp_send_key(socket: &Path, keys: &[String]) -> Result<()> {
    if keys.is_empty() {
        anyhow::bail!("keys must not be empty");
    }
    let mut stream = qmp_connect(socket)?;
    let qmp_keys = keys
        .iter()
        .map(|key| serde_json::json!({ "type": "qcode", "data": key }))
        .collect::<Vec<_>>();
    qmp_send_event(
        &mut stream,
        serde_json::json!([{
            "type": "key",
            "data": { "down": true, "key": qmp_keys[0] }
        }]),
    )?;
    for key in qmp_keys.iter().skip(1) {
        qmp_send_event(
            &mut stream,
            serde_json::json!([{
                "type": "key",
                "data": { "down": true, "key": key }
            }]),
        )?;
    }
    for key in qmp_keys.iter().rev() {
        qmp_send_event(
            &mut stream,
            serde_json::json!([{
                "type": "key",
                "data": { "down": false, "key": key }
            }]),
        )?;
    }
    Ok(())
}

fn qmp_connect(socket: &Path) -> Result<BufReader<UnixStream>> {
    let stream = UnixStream::connect(socket)
        .with_context(|| format!("failed to connect QMP socket {}", socket.display()))?;
    let mut stream = BufReader::new(stream);
    let mut greeting = String::new();
    stream
        .read_line(&mut greeting)
        .context("failed to read QMP greeting")?;
    let greeting: serde_json::Value =
        serde_json::from_str(greeting.trim()).context("failed to parse QMP greeting")?;
    if greeting.get("QMP").is_none() {
        anyhow::bail!("unexpected QMP greeting: {greeting}");
    }

    qmp_write(
        stream.get_mut(),
        serde_json::json!({ "execute": "qmp_capabilities" }),
    )?;
    let _ = qmp_read_return(&mut stream)?;
    Ok(stream)
}

fn qmp_send_event(stream: &mut BufReader<UnixStream>, events: serde_json::Value) -> Result<()> {
    qmp_write(
        stream.get_mut(),
        serde_json::json!({
            "execute": "input-send-event",
            "arguments": { "events": events }
        }),
    )?;
    let _ = qmp_read_return(stream)?;
    Ok(())
}

fn qmp_write(stream: &mut UnixStream, value: serde_json::Value) -> Result<()> {
    let mut encoded = serde_json::to_vec(&value).context("failed to encode QMP command")?;
    encoded.push(b'\n');
    stream
        .write_all(&encoded)
        .context("failed to write QMP command")
}

fn qmp_read_return(stream: &mut BufReader<UnixStream>) -> Result<serde_json::Value> {
    loop {
        let mut line = String::new();
        let read = stream
            .read_line(&mut line)
            .context("failed to read QMP response")?;
        if read == 0 {
            anyhow::bail!("QMP connection closed before response");
        }
        let value: serde_json::Value =
            serde_json::from_str(line.trim()).context("failed to parse QMP response")?;
        if let Some(error) = value.get("error") {
            anyhow::bail!("QMP command failed: {error}");
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
        other => anyhow::bail!("unsupported ASCII byte: {other}"),
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
