use std::{
    fs,
    io::{self, Read, Seek, SeekFrom, Write},
    path::Path,
    sync::atomic::{AtomicBool, Ordering},
    thread,
    time::Duration,
};

pub fn cleanup_socket(path: &Path) {
    let _ = fs::remove_file(path);
}

pub fn stream_serial_log(serial_log: &Path, done: &AtomicBool) {
    let mut offset = 0;
    let mut file = None;

    loop {
        if file.is_none() {
            match fs::File::open(serial_log) {
                Ok(opened) => file = Some(opened),
                Err(_) if !done.load(Ordering::Acquire) => {
                    thread::sleep(Duration::from_millis(10));
                    continue;
                }
                Err(err) => {
                    eprintln!("failed to open serial log {}: {err}", serial_log.display());
                    break;
                }
            }
        }

        let drained = match file.as_mut() {
            Some(file) => {
                let output = drain_serial_log(file, &mut offset);
                print!("{output}");
                let _ = io::stdout().flush();
                output.len()
            }
            None => 0,
        };
        if done.load(Ordering::Acquire) && drained == 0 {
            break;
        }
        thread::sleep(Duration::from_millis(10));
    }
}

pub fn drain_serial_log(file: &mut fs::File, offset: &mut u64) -> String {
    if file.seek(SeekFrom::Start(*offset)).is_err() {
        return String::new();
    }

    let mut buffer = [0; 4096];
    let mut output = String::new();
    loop {
        match file.read(&mut buffer) {
            Ok(0) => break,
            Ok(read) => {
                *offset += read as u64;
                let chunk = String::from_utf8_lossy(&buffer[..read]);
                output.push_str(&chunk);
            }
            Err(_) => break,
        }
    }
    output
}
