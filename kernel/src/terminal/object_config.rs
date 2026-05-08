use crate::{
    memory::user_safe,
    object::{
        config::ConfigurateRequest, error::ObjectError, misc::ObjectResult, traits::Configuratable,
    },
    terminal::{TerminalObject, linux_kd::handle_kd_request, linux_vt::handle_vt_request},
};

impl Configuratable for TerminalObject {
    fn configure(&self, request: ConfigurateRequest) -> ObjectResult<isize> {
        if let Some(result) = handle_kd_request(self.linux_console.as_ref(), &request)? {
            return Ok(result);
        }

        if let Some(result) = handle_vt_request(self.linux_console.as_ref(), &request)? {
            return Ok(result);
        }

        match request {
            ConfigurateRequest::LinuxTcGets(termios) => {
                user_safe::write(termios, &self.termios.lock().as_linux_termios())
                    .map_err(|_| ObjectError::BadAddress)?;
            }
            ConfigurateRequest::LinuxTcSets(termios) => {
                let termios = user_safe::read(termios).map_err(|_| ObjectError::BadAddress)?;
                self.termios.lock().apply_linux_termios(&termios);
            }
            ConfigurateRequest::LinuxTcGets2(termios) => {
                user_safe::write(termios, &*self.termios.lock())
                    .map_err(|_| ObjectError::BadAddress)?;
            }
            ConfigurateRequest::LinuxTcSets2(termios) => {
                let termios = user_safe::read(termios).map_err(|_| ObjectError::BadAddress)?;
                self.termios.lock().apply_linux_termios2(&termios);
            }
            ConfigurateRequest::LinuxTiocgwinsz(winsize) => {
                user_safe::write(winsize, &*self.winsize.lock())
                    .map_err(|_| ObjectError::BadAddress)?;
            }
            ConfigurateRequest::LinuxTiocswinsz(winsize) => {
                let winsize = user_safe::read(winsize).map_err(|_| ObjectError::BadAddress)?;
                let mut current = self.winsize.lock();
                if winsize.ws_row != 0 {
                    current.ws_row = winsize.ws_row;
                }
                if winsize.ws_col != 0 {
                    current.ws_col = winsize.ws_col;
                }
            }
            _ => return Err(ObjectError::InvalidArguments),
        }
        Ok(0)
    }
}
