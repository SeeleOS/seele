use crate::syscalls::object::TerminalInfo;

pub const ICANON: u32 = 0o000_002;
pub const ECHO: u32 = 0o000_010;
pub const ECHOE: u32 = 0o000_020;
pub const ECHOK: u32 = 0o000_040;
pub const ECHONL: u32 = 0o000_100;
pub const NCCS: usize = 32;

#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct Termios {
    pub c_iflag: u32,
    pub c_oflag: u32,
    pub c_cflag: u32,
    pub c_lflag: u32,
    pub c_line: u8,
    pub c_cc: [u8; NCCS],
    pub c_ispeed: u32,
    pub c_ospeed: u32,
}

impl TerminalInfo {
    pub fn from_termios(&self, termios: &Termios) -> Self {
        let echo = termios.c_lflag & (ECHO | ECHOE | ECHOK | ECHONL) != 0;
        let raw = termios.c_lflag & ICANON == 0;

        Self {
            rows: self.rows,
            cols: self.cols,
            echo,
            raw,
        }
    }

    // Write self to terminos
    pub fn write_termios(&self, termios: &mut Termios) {
        if self.raw {
            termios.c_lflag &= !ICANON;
        } else {
            termios.c_lflag |= ICANON;
        }

        if self.echo {
            termios.c_lflag |= ECHO | ECHOE | ECHOK | ECHONL;
        } else {
            termios.c_lflag &= !(ECHO | ECHOE | ECHOK | ECHONL);
        }
    }
}
