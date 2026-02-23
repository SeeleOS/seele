use core::fmt::Write;

use alloc::fmt;

use crate::graphics::{
    framebuffer::FRAME_BUFFER,
    tty::{TTY, Tty},
};

impl<'a> Tty<'a> {
    pub fn print_string(&mut self, string: &str) {
        let mut buf = [0u8; 4]; // utf-8 is maximum 4 bytes

        for c in string.chars() {
            if c == '\n' {
                self.new_line();
                continue;
            }

            if self.col >= self.screen_width() {
                self.new_line();
            }

            if self.row >= self.screen_height() {
                self.row = 0;
                self.col = 0;
            }

            let c_slice = c.encode_utf8(&mut buf).as_bytes();

            self.print_char(c_slice);
        }

        self.canvas.lock().flush();
    }

    fn print_char(&mut self, char: &[u8]) {
        let glyph = self.font.glyph_for_utf8(char).expect("Invalid charcter");

        let base_x = (self.col * self.font.width) as usize;
        let base_y = (self.row * self.font.height) as usize;

        for (y, row) in glyph.enumerate() {
            for (x, visible) in row.enumerate() {
                if visible {
                    self.canvas
                        .lock()
                        .write_pixel(base_x + x, base_y + y, 255, 255, 255);
                    // Shadow
                    self.canvas
                        .lock()
                        .write_pixel(base_x + x + 1, base_y + y + 1, 0, 0, 0);
                }
            }
        }

        self.col += 1;
    }

    fn screen_width(&self) -> u32 {
        self.canvas.lock().width / self.font.width
    }

    fn screen_height(&self) -> u32 {
        self.canvas.lock().height / self.font.height
    }

    fn new_line(&mut self) {
        self.row += 1;
        self.col = 0;
    }
}

impl<'a> Write for Tty<'a> {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        self.print_string(s);
        Ok(())
    }
}

#[macro_export]
macro_rules! print {
    ($($arg:tt)*) => ($crate::graphics::tty::text::_print(format_args!($($arg)*)));
}

#[macro_export]
macro_rules! println {
    () => ($crate::print!("\n"));
    ($($arg:tt)*) => ($crate::print!("{}\n", format_args!($($arg)*)));
}

#[doc(hidden)]
pub fn _print(args: fmt::Arguments) {
    TTY.get().unwrap().lock().write_fmt(args);
}
