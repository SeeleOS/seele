use core::fmt::{Arguments, Write};

use conquer_once::spin::OnceCell;
use os_terminal::Terminal;
use spin::Mutex;

use crate::graphics::framebuffer::{Canvas, FRAME_BUFFER};

pub type Color = (u8, u8, u8);

pub static TERMINAL: OnceCell<Mutex<Terminal<TermRenderer>>> = OnceCell::uninit();

pub struct TermRenderer<'a> {
    canvas: &'a Mutex<Canvas>,
    pub width: u32,
    pub height: u32,
}

impl<'a> TermRenderer<'a> {
    pub fn new(canvas: &'a Mutex<Canvas>) -> Self {
        let width = canvas.lock().width;
        let height = canvas.lock().height;
        Self {
            canvas,
            width,
            height,
        }
    }
}

#[macro_export]
macro_rules! print {
    ($($arg:tt)*) => ($crate::graphics::terminal::term_print(format_args!($($arg)*)));
}

#[macro_export]
macro_rules! println {
    () => ($crate::print!("\n"));
    ($($arg:tt)*) => ($crate::print!("{}\n", format_args!($($arg)*)));
}

#[doc(hidden)]
pub fn term_print(args: Arguments) {
    TERMINAL.get().unwrap().lock().write_fmt(args).unwrap();
    FRAME_BUFFER.get().unwrap().lock().flush();
}

use os_terminal::DrawTarget;

impl<'a> DrawTarget for TermRenderer<'a> {
    fn size(&self) -> (usize, usize) {
        (self.width as usize, self.height as usize)
    }

    #[inline(always)]
    fn draw_pixel(&mut self, x: usize, y: usize, rgb: os_terminal::Rgb) {
        self.canvas.lock().write_pixel(x, y, (rgb.0, rgb.1, rgb.2));
    }
}
