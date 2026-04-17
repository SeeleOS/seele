use core::ptr::{read_volatile, write_volatile};

use crate::{
    object::{
        config::{ConfigurateRequest, LinuxTermios, LinuxTermios2, LinuxWinsize},
        error::ObjectError,
        misc::ObjectResult,
        traits::Configuratable,
    },
    terminal::{
        TerminalObject,
        linux_kd::handle_kd_request,
        linux_vt::handle_vt_request,
        object::TerminalSettings,
    },
};

const LINUX_ISIG: u32 = 0x0000_0001;
const LINUX_ICANON: u32 = 0x0000_0002;
const LINUX_ECHO: u32 = 0x0000_0008;
const LINUX_ECHONL: u32 = 0x0000_0040;
const LINUX_OPOST: u32 = 0x0000_0001;
const LINUX_ONLCR: u32 = 0x0000_0004;
const LINUX_CREAD: u32 = 0x0000_0080;
const LINUX_CS8: u32 = 0x0000_0030;

fn info_to_linux_termios(info: &TerminalSettings) -> LinuxTermios {
    let mut termios = LinuxTermios { c_cflag: LINUX_CREAD | LINUX_CS8, ..LinuxTermios::default() };

    if info.echo {
        termios.c_lflag |= LINUX_ECHO;
    }
    if info.canonical {
        termios.c_lflag |= LINUX_ICANON;
    }
    if info.send_sig_on_special_chars {
        termios.c_lflag |= LINUX_ISIG;
    }
    if info.echo_newline {
        termios.c_lflag |= LINUX_ECHONL;
    }
    if info.map_output_newline_to_crlf {
        termios.c_oflag |= LINUX_OPOST | LINUX_ONLCR;
    }

    termios
}

fn info_to_linux_termios2(info: &TerminalSettings) -> LinuxTermios2 {
    let mut termios = LinuxTermios2 {
        c_cflag: LINUX_CREAD | LINUX_CS8,
        c_ispeed: 38_400,
        c_ospeed: 38_400,
        ..LinuxTermios2::default()
    };

    if info.echo {
        termios.c_lflag |= LINUX_ECHO;
    }
    if info.canonical {
        termios.c_lflag |= LINUX_ICANON;
    }
    if info.send_sig_on_special_chars {
        termios.c_lflag |= LINUX_ISIG;
    }
    if info.echo_newline {
        termios.c_lflag |= LINUX_ECHONL;
    }
    if info.map_output_newline_to_crlf {
        termios.c_oflag |= LINUX_OPOST | LINUX_ONLCR;
    }

    termios
}

impl Configuratable for TerminalObject {
    fn configure(&self, request: crate::object::config::ConfigurateRequest) -> ObjectResult<isize> {
        if let Some(result) = handle_kd_request(&self.linux_console, &request)? {
            return Ok(result);
        }

        if let Some(result) = handle_vt_request(&self.linux_console, &request)? {
            return Ok(result);
        }

        match request {
            ConfigurateRequest::LinuxTcGets(termios) => unsafe {
                write_volatile(termios, info_to_linux_termios(&self.info.lock()));
            },
            ConfigurateRequest::LinuxTcSets(termios) => unsafe {
                let termios = read_volatile(termios);
                let mut info = self.info.lock();
                info.echo = (termios.c_lflag & LINUX_ECHO) != 0;
                info.canonical = (termios.c_lflag & LINUX_ICANON) != 0;
                info.send_sig_on_special_chars = (termios.c_lflag & LINUX_ISIG) != 0;
                info.echo_newline = (termios.c_lflag & LINUX_ECHONL) != 0;
                info.map_output_newline_to_crlf =
                    (termios.c_oflag & (LINUX_OPOST | LINUX_ONLCR)) != 0;
            },
            ConfigurateRequest::LinuxTcGets2(termios) => unsafe {
                write_volatile(termios, info_to_linux_termios2(&self.info.lock()));
            },
            ConfigurateRequest::LinuxTcSets2(termios) => unsafe {
                let termios = read_volatile(termios);
                let mut info = self.info.lock();
                info.echo = (termios.c_lflag & LINUX_ECHO) != 0;
                info.canonical = (termios.c_lflag & LINUX_ICANON) != 0;
                info.send_sig_on_special_chars = (termios.c_lflag & LINUX_ISIG) != 0;
                info.echo_newline = (termios.c_lflag & LINUX_ECHONL) != 0;
                info.map_output_newline_to_crlf =
                    (termios.c_oflag & (LINUX_OPOST | LINUX_ONLCR)) != 0;
            },
            ConfigurateRequest::LinuxTiocgwinsz(winsize) => unsafe {
                let info = self.info.lock();
                write_volatile(
                    winsize,
                    LinuxWinsize {
                        ws_row: info.rows as u16,
                        ws_col: info.cols as u16,
                        ws_xpixel: 0,
                        ws_ypixel: 0,
                    },
                );
            },
            _ => return Err(ObjectError::InvalidArguments),
        }
        Ok(0)
    }
}
