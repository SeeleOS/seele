use bitflags::bitflags;

use crate::object::config::{LinuxTermios, LinuxTermios2, LinuxWinsize};

pub const VINTR_INDEX: usize = 0;
pub const VQUIT_INDEX: usize = 1;
pub const VERASE_INDEX: usize = 2;
pub const VKILL_INDEX: usize = 3;
pub const VEOF_INDEX: usize = 4;
pub const VTIME_INDEX: usize = 5;
pub const VMIN_INDEX: usize = 6;
pub const VSTART_INDEX: usize = 8;
pub const VSTOP_INDEX: usize = 9;
pub const VSUSP_INDEX: usize = 10;

bitflags! {
    #[derive(Clone, Copy, Debug)]
    struct InputFlags: u32 {
        const ICRNL = 0x0000_0100;
    }
}

bitflags! {
    #[derive(Clone, Copy, Debug)]
    struct LocalFlags: u32 {
        const ISIG = 0x0000_0001;
        const ICANON = 0x0000_0002;
        const ECHO = 0x0000_0008;
        const ECHOE = 0x0000_0010;
        const ECHONL = 0x0000_0040;
    }
}

bitflags! {
    #[derive(Clone, Copy, Debug)]
    struct OutputFlags: u32 {
        const OPOST = 0x0000_0001;
        const ONLCR = 0x0000_0004;
    }
}

bitflags! {
    #[derive(Clone, Copy, Debug)]
    struct ControlFlags: u32 {
        const CREAD = 0x0000_0080;
        const CS8 = 0x0000_0030;
    }
}

impl LinuxTermios2 {
    pub fn new_default() -> Self {
        let mut termios = Self {
            c_iflag: InputFlags::ICRNL.bits(),
            c_oflag: (OutputFlags::OPOST | OutputFlags::ONLCR).bits(),
            c_cflag: (ControlFlags::CREAD | ControlFlags::CS8).bits(),
            c_lflag: (LocalFlags::ISIG
                | LocalFlags::ICANON
                | LocalFlags::ECHO
                | LocalFlags::ECHOE
                | LocalFlags::ECHONL)
                .bits(),
            c_ispeed: 38_400,
            c_ospeed: 38_400,
            ..Self::default()
        };

        termios.c_cc[VINTR_INDEX] = 3;
        termios.c_cc[VQUIT_INDEX] = 28;
        termios.c_cc[VERASE_INDEX] = 127;
        termios.c_cc[VKILL_INDEX] = 21;
        termios.c_cc[VEOF_INDEX] = 4;
        termios.c_cc[VTIME_INDEX] = 0;
        termios.c_cc[VMIN_INDEX] = 1;
        termios.c_cc[VSTART_INDEX] = 17;
        termios.c_cc[VSTOP_INDEX] = 19;
        termios.c_cc[VSUSP_INDEX] = 26;

        termios
    }

    pub fn as_linux_termios(&self) -> LinuxTermios {
        LinuxTermios {
            c_iflag: self.c_iflag,
            c_oflag: self.c_oflag,
            c_cflag: self.c_cflag,
            c_lflag: self.c_lflag,
            c_line: self.c_line,
            c_cc: self.c_cc,
        }
    }

    pub fn apply_linux_termios(&mut self, termios: &LinuxTermios) {
        self.c_iflag = termios.c_iflag;
        self.c_oflag = termios.c_oflag;
        self.c_cflag = termios.c_cflag;
        self.c_lflag = termios.c_lflag;
        self.c_line = termios.c_line;
        self.c_cc = termios.c_cc;
    }

    pub fn apply_linux_termios2(&mut self, termios: &LinuxTermios2) {
        *self = *termios;
    }

    pub fn is_canonical(&self) -> bool {
        LocalFlags::from_bits_truncate(self.c_lflag).contains(LocalFlags::ICANON)
    }

    pub fn should_echo(&self) -> bool {
        LocalFlags::from_bits_truncate(self.c_lflag).contains(LocalFlags::ECHO)
    }

    pub fn should_echo_newline(&self) -> bool {
        let flags = LocalFlags::from_bits_truncate(self.c_lflag);
        flags.contains(LocalFlags::ECHO) || flags.contains(LocalFlags::ECHONL)
    }

    pub fn should_echo_erase(&self) -> bool {
        let flags = LocalFlags::from_bits_truncate(self.c_lflag);
        flags.contains(LocalFlags::ECHO) && flags.contains(LocalFlags::ECHOE)
    }

    pub fn should_signal_on_special_chars(&self) -> bool {
        LocalFlags::from_bits_truncate(self.c_lflag).contains(LocalFlags::ISIG)
    }

    pub fn map_input_cr_to_nl(&self) -> bool {
        InputFlags::from_bits_truncate(self.c_iflag).contains(InputFlags::ICRNL)
    }

    pub fn map_output_newline_to_crlf(&self) -> bool {
        let flags = OutputFlags::from_bits_truncate(self.c_oflag);
        flags.contains(OutputFlags::OPOST | OutputFlags::ONLCR)
    }

    pub fn interrupt_char(&self) -> u8 {
        self.c_cc[VINTR_INDEX]
    }

    pub fn erase_char(&self) -> u8 {
        self.c_cc[VERASE_INDEX]
    }

    pub fn eof_char(&self) -> u8 {
        self.c_cc[VEOF_INDEX]
    }
}

impl LinuxWinsize {
    pub fn from_rows_cols(rows: u64, cols: u64) -> Self {
        Self {
            ws_row: u16::try_from(rows).unwrap_or(u16::MAX),
            ws_col: u16::try_from(cols).unwrap_or(u16::MAX),
            ws_xpixel: 0,
            ws_ypixel: 0,
        }
    }

    pub fn default_terminal_size() -> Self {
        Self::from_rows_cols(25, 80)
    }
}
