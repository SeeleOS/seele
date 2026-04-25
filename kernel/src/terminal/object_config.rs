use core::ptr::{read_volatile, write_volatile};

use crate::{
    object::{
        config::ConfigurateRequest, error::ObjectError, misc::ObjectResult, traits::Configuratable,
    },
    terminal::{TerminalObject, linux_kd::handle_kd_request, linux_vt::handle_vt_request},
};

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
                write_volatile(termios, self.termios.lock().as_linux_termios());
            },
            ConfigurateRequest::LinuxTcSets(termios) => unsafe {
                let termios = read_volatile(termios);
                self.termios.lock().apply_linux_termios(&termios);
            },
            ConfigurateRequest::LinuxTcGets2(termios) => unsafe {
                write_volatile(termios, *self.termios.lock());
            },
            ConfigurateRequest::LinuxTcSets2(termios) => unsafe {
                let termios = read_volatile(termios);
                self.termios.lock().apply_linux_termios2(&termios);
            },
            ConfigurateRequest::LinuxTiocgwinsz(winsize) => unsafe {
                write_volatile(winsize, *self.winsize.lock());
            },
            ConfigurateRequest::LinuxTiocswinsz(winsize) => unsafe {
                let winsize = read_volatile(winsize);
                let mut current = self.winsize.lock();
                if winsize.ws_row != 0 {
                    current.ws_row = winsize.ws_row;
                }
                if winsize.ws_col != 0 {
                    current.ws_col = winsize.ws_col;
                }
            },
            _ => return Err(ObjectError::InvalidArguments),
        }
        Ok(0)
    }
}
