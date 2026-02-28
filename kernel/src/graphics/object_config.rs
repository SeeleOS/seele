use core::ptr::write_volatile;

use crate::{
    graphics::{object::TtyObject, tty::TTY},
    object::{Configuratable, ConfigurateRequest},
};

#[repr(C)]
#[derive(Default, Copy, Clone)]
pub struct WindowSizeInfo {
    pub rows: u16,
    pub cols: u16,
    pub ws_xpixel: u16,
    pub ws_ypixel: u16,
}

#[repr(C)]
pub struct TerminalInfo {
    pub c_iflag: u32,   // 输入标志 (Input modes)
    pub c_oflag: u32,   // 输出标志 (Output modes)
    pub c_cflag: u32,   // 控制标志 (Control modes)
    pub c_lflag: u32,   // 本地标志 (Local modes) - 最重要！
    pub c_line: u8,     // 线路规程 (Line discipline)
    pub c_cc: [u8; 19], // 特殊控制字符 (如 Ctrl+C, Backspace)
}
impl Configuratable for TtyObject {
    fn configure(
        &self,
        request: crate::object::ConfigurateRequest,
    ) -> crate::object::ObjectResult<isize> {
        let tty = TTY.get().unwrap().lock();

        match request {
            ConfigurateRequest::GetWindowSize(window_size) => unsafe {
                write_volatile(
                    window_size,
                    WindowSizeInfo {
                        rows: tty.max_rows,
                        cols: tty.max_cols,
                        ws_xpixel: 0,
                        ws_ypixel: 0,
                    },
                );
            },
            ConfigurateRequest::GetTerminalInfo(term_info) => {}
            _ => {}
        }
        Ok(0)
    }
}
