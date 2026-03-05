use core::fmt::Write;

use alloc::fmt;

use crate::graphics::tty::{TTY, Tty};

#[derive(Clone, Copy, Default, Debug)]
pub struct TextCell {
    char: char,
    color: u64,
    previous_char: char,
}

pub const PADDING: u32 = 50;

impl<'a> Tty<'a> {
    pub fn print_string(&mut self, string: &str) {
        for c in string.chars() {
            if c == '\n' {
                self.new_line();
                continue;
            }

            if self.cursor_x >= self.screen_width_char() as u32 {
                self.new_line();
            }

            if self.cursor_y >= self.screen_height_chars() as u32 {
                self.scroll_up();
            }

            self.push_char(c);
        }

        self.render();
    }

    fn get_text_cell_index(&mut self, row: u32, col: u32) -> usize {
        (self.screen_width_char() * row as usize) + col as usize
    }

    pub fn scroll_up(&mut self) {
        unimplemented!()
    }

    pub fn push_char(&mut self, char: char) {
        let index = self.get_text_cell_index(self.cursor_y, self.cursor_x);
        let text_cell = &mut self.text_buf[index];

        text_cell.char = char;
        self.cursor_x += 1;
    }

    pub fn render(&mut self) {
        let rows = self.screen_height_chars();
        let cols = self.screen_width_char();

        for row in 0..rows {
            for col in 0..cols {
                let index = self.get_text_cell_index(row as u32, col as u32);
                let cell = self.text_buf[index];

                if cell.char != cell.previous_char {
                    self.render_char(col as u32, row as u32, cell.char);
                    self.text_buf[index].previous_char = cell.char;
                }
            }
        }

        self.canvas.lock().flush();
    }

    fn render_char(&mut self, col: u32, row: u32, char: char) {
        let mut buf = [0u8; 4];
        let character = char.encode_utf8(&mut buf).as_bytes();

        let glyph = self
            .font
            .glyph_for_utf8(character)
            .expect("Invalid charcter");

        let base_x = (PADDING + (col * self.font.width)) as usize;
        let base_y = (PADDING + (row * self.font.height)) as usize;

        let mut canvas = self.canvas.lock();

        for (y, row) in glyph.enumerate() {
            for (x, visible) in row.enumerate() {
                if visible && char != '\0' {
                    canvas.write_pixel(base_x + x, base_y + y, 255, 255, 255);
                    // Shadow
                    canvas.write_pixel(base_x + x + 1, base_y + y + 1, 0, 0, 0);
                } else {
                    self.draw_wallpaper_pixel(base_x + x, base_y + y, &mut canvas);
                }
            }
        }
    }

    pub fn screen_width_char(&self) -> usize {
        ((self.canvas.lock().width - PADDING * 2) / self.font.width) as usize
    }

    pub fn screen_height_chars(&self) -> usize {
        ((self.canvas.lock().height - PADDING * 2) / self.font.height) as usize
    }

    pub fn new_line(&mut self) {
        self.cursor_y += 1;
        self.cursor_x = 0;
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
    TTY.get().unwrap().lock().write_fmt(args).unwrap();
}
