use std::{
    fs,
    io::{self, Read, Seek, SeekFrom, Write},
    os::{fd::AsRawFd, unix::net::UnixStream},
    path::Path,
    sync::atomic::{AtomicBool, Ordering},
    thread,
    time::Duration,
};

pub fn cleanup_socket(path: &Path) {
    let _ = fs::remove_file(path);
}

pub fn forward_terminal_input(socket_path: &Path, done: &AtomicBool) {
    let mut socket = loop {
        match UnixStream::connect(socket_path) {
            Ok(socket) => break socket,
            Err(_) if !done.load(Ordering::Acquire) => thread::sleep(Duration::from_millis(10)),
            Err(err) => {
                eprintln!(
                    "failed to connect background terminal input path {}: {err}",
                    socket_path.display()
                );
                return;
            }
        }
    };

    let stdin = io::stdin();
    let stdin_fd = stdin.as_raw_fd();
    let _terminal_mode = match TerminalInputModeGuard::new(stdin_fd) {
        Ok(mode) => mode,
        Err(err) => {
            eprintln!("failed to prepare terminal input forwarding: {err}");
            None
        }
    };
    let mut stdin = stdin.lock();
    let mut buffer = [0; 1024];

    loop {
        if done.load(Ordering::Acquire) {
            break;
        }
        match poll_stdin(stdin_fd, 10) {
            Ok(false) => continue,
            Ok(true) => {}
            Err(err) => {
                eprintln!("failed to poll terminal input: {err}");
                break;
            }
        }
        match stdin.read(&mut buffer) {
            Ok(0) => break,
            Ok(read) => {
                if socket.write_all(&buffer[..read]).is_err() {
                    break;
                }
            }
            Err(err) if err.kind() == io::ErrorKind::Interrupted => continue,
            Err(err) => {
                eprintln!("failed to read terminal input: {err}");
                break;
            }
        }
    }
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
            Some(file) => drain_serial_log(file, &mut offset).len(),
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
                print!("{chunk}");
                let _ = io::stdout().flush();
                output.push_str(&chunk);
            }
            Err(_) => break,
        }
    }
    output
}

fn poll_stdin(fd: i32, timeout_ms: i32) -> io::Result<bool> {
    let mut pollfd = libc::pollfd {
        fd,
        events: libc::POLLIN,
        revents: 0,
    };
    let result = unsafe { libc::poll(&mut pollfd, 1, timeout_ms) };
    if result < 0 {
        return Err(io::Error::last_os_error());
    }
    if result == 0 {
        return Ok(false);
    }
    if pollfd.revents & (libc::POLLERR | libc::POLLHUP | libc::POLLNVAL) != 0 {
        return Ok(false);
    }
    Ok(pollfd.revents & libc::POLLIN != 0)
}

struct TerminalInputModeGuard {
    fd: i32,
    original: libc::termios,
}

impl TerminalInputModeGuard {
    fn new(fd: i32) -> io::Result<Option<Self>> {
        if unsafe { libc::isatty(fd) } != 1 {
            return Ok(None);
        }

        let mut original = unsafe { std::mem::zeroed::<libc::termios>() };
        if unsafe { libc::tcgetattr(fd, &mut original) } != 0 {
            return Err(io::Error::last_os_error());
        }

        let mut raw = original;
        raw.c_iflag &= !(libc::BRKINT | libc::ICRNL | libc::INPCK | libc::ISTRIP | libc::IXON);
        raw.c_lflag &= !(libc::ICANON | libc::ECHO | libc::IEXTEN | libc::ISIG);
        raw.c_cc[libc::VMIN] = 1;
        raw.c_cc[libc::VTIME] = 0;

        if unsafe { libc::tcsetattr(fd, libc::TCSANOW, &raw) } != 0 {
            return Err(io::Error::last_os_error());
        }

        Ok(Some(Self { fd, original }))
    }
}

impl Drop for TerminalInputModeGuard {
    fn drop(&mut self) {
        unsafe {
            libc::tcsetattr(self.fd, libc::TCSANOW, &self.original);
        }
    }
}
