use bitflags::bitflags;
use core::ptr::{read_volatile, write_volatile};

use crate::{
    object::{
        config::{ConfigurateRequest, LinuxTermios, LinuxTermios2, LinuxWinsize},
        error::ObjectError,
        misc::ObjectResult,
        traits::Configuratable,
    },
    terminal::{
        TerminalObject, linux_kd::handle_kd_request, linux_vt::handle_vt_request,
        object::TerminalSettings,
    },
};

bitflags! {
    #[derive(Clone, Copy, Debug)]
    struct LocalFlags: u32 {
        const ISIG = 0x0000_0001;
        const ICANON = 0x0000_0002;
        const ECHO = 0x0000_0008;
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

fn info_to_linux_termios(info: &TerminalSettings) -> LinuxTermios {
    let mut termios = LinuxTermios {
        c_cflag: (ControlFlags::CREAD | ControlFlags::CS8).bits(),
        ..LinuxTermios::default()
    };

    if info.echo {
        termios.c_lflag |= LocalFlags::ECHO.bits();
    }
    if info.canonical {
        termios.c_lflag |= LocalFlags::ICANON.bits();
    }
    if info.send_sig_on_special_chars {
        termios.c_lflag |= LocalFlags::ISIG.bits();
    }
    if info.echo_newline {
        termios.c_lflag |= LocalFlags::ECHONL.bits();
    }
    if info.map_output_newline_to_crlf {
        termios.c_oflag |= (OutputFlags::OPOST | OutputFlags::ONLCR).bits();
    }

    termios
}

fn info_to_linux_termios2(info: &TerminalSettings) -> LinuxTermios2 {
    let mut termios = LinuxTermios2 {
        c_cflag: (ControlFlags::CREAD | ControlFlags::CS8).bits(),
        c_ispeed: 38_400,
        c_ospeed: 38_400,
        ..LinuxTermios2::default()
    };

    if info.echo {
        termios.c_lflag |= LocalFlags::ECHO.bits();
    }
    if info.canonical {
        termios.c_lflag |= LocalFlags::ICANON.bits();
    }
    if info.send_sig_on_special_chars {
        termios.c_lflag |= LocalFlags::ISIG.bits();
    }
    if info.echo_newline {
        termios.c_lflag |= LocalFlags::ECHONL.bits();
    }
    if info.map_output_newline_to_crlf {
        termios.c_oflag |= (OutputFlags::OPOST | OutputFlags::ONLCR).bits();
    }

    termios
}

impl Configuratable for TerminalObject {
    fn configure(&self, request: ConfigurateRequest) -> ObjectResult<isize> {
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
                let lflag = LocalFlags::from_bits_truncate(termios.c_lflag);
                let oflag = OutputFlags::from_bits_truncate(termios.c_oflag);
                info.echo = lflag.contains(LocalFlags::ECHO);
                info.canonical = lflag.contains(LocalFlags::ICANON);
                info.send_sig_on_special_chars = lflag.contains(LocalFlags::ISIG);
                info.echo_newline = lflag.contains(LocalFlags::ECHONL);
                info.map_output_newline_to_crlf =
                    oflag.contains(OutputFlags::OPOST | OutputFlags::ONLCR);
            },
            ConfigurateRequest::LinuxTcGets2(termios) => unsafe {
                write_volatile(termios, info_to_linux_termios2(&self.info.lock()));
            },
            ConfigurateRequest::LinuxTcSets2(termios) => unsafe {
                let termios = read_volatile(termios);
                let mut info = self.info.lock();
                let lflag = LocalFlags::from_bits_truncate(termios.c_lflag);
                let oflag = OutputFlags::from_bits_truncate(termios.c_oflag);
                info.echo = lflag.contains(LocalFlags::ECHO);
                info.canonical = lflag.contains(LocalFlags::ICANON);
                info.send_sig_on_special_chars = lflag.contains(LocalFlags::ISIG);
                info.echo_newline = lflag.contains(LocalFlags::ECHONL);
                info.map_output_newline_to_crlf =
                    oflag.contains(OutputFlags::OPOST | OutputFlags::ONLCR);
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
            ConfigurateRequest::LinuxTiocswinsz(winsize) => unsafe {
                let winsize = read_volatile(winsize);
                let mut info = self.info.lock();
                if winsize.ws_row != 0 {
                    info.rows = winsize.ws_row as u64;
                }
                if winsize.ws_col != 0 {
                    info.cols = winsize.ws_col as u64;
                }
            },
            _ => return Err(ObjectError::InvalidArguments),
        }
        Ok(0)
    }
}
