use core::ptr::{read, read_volatile, write_volatile};

use crate::{
    graphics::object::TerminalObject,
    object::{config::ConfigurateRequest, misc::ObjectResult, traits::Configuratable},
};

#[derive(Debug)]
#[repr(C)]
#[derive(Default, Copy, Clone)]
pub struct WindowSizeInfo {
    pub rows: u16,
    pub cols: u16,
    pub ws_xpixel: u16,
    pub ws_ypixel: u16,
}

#[derive(Clone, Copy, Debug)]
#[repr(C)]
pub struct TerminalInfo {
    pub c_iflag: u32,   // 输入标志 (Input modes)
    pub c_oflag: u32,   // 输出标志 (Output modes)
    pub c_cflag: u32,   // 控制标志 (Control modes)
    pub c_lflag: u32,   // 本地标志 (Local modes) - 最重要！
    pub c_line: u8,     // 线路规程 (Line discipline)
    pub c_cc: [u8; 32], // 特殊控制字符 (如 Ctrl+C, Backspace)
}
impl TerminalInfo {
    pub fn new_default() -> Self {
        let mut cc = [0u8; 32];
        // 设置常用的控制字符 (ASCII)
        cc[0] = 3; // VINTR:  Ctrl+C
        cc[1] = 28; // VQUIT:  Ctrl+\
        cc[2] = 127; // VERASE: Backspace (Linux 习惯用 127)
        cc[3] = 21; // VKILL:  Ctrl+U
        cc[4] = 4; // VEOF:   Ctrl+D

        Self {
            // ICRNL: 把输入的 \r 转成 \n
            c_iflag: 0x00000100,
            // ONLCR: 把输出的 \n 转成 \r\n
            c_oflag: 0x00000004,
            // B38400 | CS8 | CREAD: 常见的串口/终端配置
            c_cflag: 0x00000bf2,
            // ISIG | ICANON | ECHO | ECHOE | ECHOK:
            // 开启回显、规范模式、信号处理、退格擦除
            c_lflag: 0x00000001 | 0x00000002 | 0x00000008 | 0x00000010 | 0x00000020,
            c_line: 0,
            c_cc: cc,
        }
    }

    pub fn is_raw_mode(&self) -> bool {
        self.c_lflag & 0x00000002 == 0
    }
}
impl Configuratable for TerminalObject {
    fn configure(&self, request: crate::object::config::ConfigurateRequest) -> ObjectResult<isize> {
        match request {
            ConfigurateRequest::GetWindowSize(window_size) => unsafe {
                write_volatile(window_size, self.window_size);
            },
            ConfigurateRequest::GetTerminalInfo(term_info) => unsafe {
                write_volatile(term_info, *self.terminal_info.lock());
            },
            ConfigurateRequest::SetTerminalInfo(term_info) => unsafe {
                let new_info = read_volatile(term_info);

                *self.terminal_info.lock() = new_info;

                // 3. 关键逻辑：根据 ICANON 位的变化，决定是否需要清理 LineBuffer
                if (new_info.c_lflag & 0x00000002) == 0 {
                    // 进入了 Raw Mode！
                    // 如果之前 LineBuffer 里有还没敲回车的存货，
                    // 考虑是不是要把它们直接冲刷到 InputQueue 里
                }
            },
            _ => {}
        }
        Ok(0)
    }
}
