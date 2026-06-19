use core::fmt::Write;

use crate::memory::utils::Mut;
use alloc::{boxed::Box, vec::Vec};
use os_terminal::{
    DrawTarget, Terminal,
    font::{BitmapFont, FontManager},
};

use crate::{
    misc::framebuffer::{Canvas, framebuffer_user_controlled},
    terminal::term_trait::PtyWriter,
};

pub struct KernelTerminal {
    terminal: Terminal<FramebufferDisplay>,
    canvas: &'static Mut<Canvas>,
    cursor_row: usize,
    cursor_col: usize,
}

struct FramebufferDisplay {
    canvas: &'static Mut<Canvas>,
}

impl KernelTerminal {
    pub fn new(canvas: &'static Mut<Canvas>) -> Self {
        let display = FramebufferDisplay { canvas };
        let font: Box<dyn FontManager> = Box::new(BitmapFont);
        let mut terminal = Terminal::new(display, font);
        terminal.set_auto_flush(false);
        terminal.set_crnl_mapping(true);
        terminal.flush();

        Self {
            terminal,
            canvas,
            cursor_row: 0,
            cursor_col: 0,
        }
    }

    pub fn write_str(&mut self, text: &str) {
        let _ = self.terminal.write_str(text);
        self.track_cursor(text);
        self.flush();
    }

    pub fn flush(&mut self) {
        if !framebuffer_user_controlled() {
            self.terminal.flush();
            self.canvas.lock().flush();
        }
    }

    pub fn rows(&self) -> usize {
        self.terminal.rows()
    }

    pub fn columns(&self) -> usize {
        self.terminal.columns()
    }

    pub fn cursor_position(&self) -> (usize, usize) {
        (self.cursor_row, self.cursor_col)
    }

    pub fn set_pty_writer(&mut self, writer: PtyWriter) {
        self.terminal.set_pty_writer(writer);
    }

    pub fn clear(&mut self) {
        self.terminal.process(b"\x1b[2J\x1b[H");
        self.cursor_row = 0;
        self.cursor_col = 0;
        self.flush();
    }

    fn track_cursor(&mut self, text: &str) {
        let mut bytes = text.bytes().peekable();
        while let Some(byte) = bytes.next() {
            match byte {
                b'\x1b' => self.track_escape_sequence(&mut bytes),
                b'\n' => {
                    self.cursor_row = (self.cursor_row + 1).min(self.rows().saturating_sub(1));
                    self.cursor_col = 0;
                }
                b'\r' => self.cursor_col = 0,
                0x08 => self.cursor_col = self.cursor_col.saturating_sub(1),
                0x20..=0x7e => {
                    self.cursor_col += 1;
                    if self.cursor_col >= self.columns() {
                        self.cursor_col = 0;
                        self.cursor_row = (self.cursor_row + 1).min(self.rows().saturating_sub(1));
                    }
                }
                _ => {}
            }
        }
    }

    fn track_escape_sequence<I>(&mut self, bytes: &mut core::iter::Peekable<I>)
    where
        I: Iterator<Item = u8>,
    {
        match bytes.next() {
            Some(b'[') => self.track_csi_sequence(bytes),
            Some(b'c') => {
                self.cursor_row = 0;
                self.cursor_col = 0;
            }
            _ => {}
        }
    }

    fn track_csi_sequence<I>(&mut self, bytes: &mut core::iter::Peekable<I>)
    where
        I: Iterator<Item = u8>,
    {
        let mut params = Vec::new();
        let mut current = 0usize;
        let mut has_current = false;

        for byte in bytes.by_ref() {
            match byte {
                b'0'..=b'9' => {
                    current = current * 10 + (byte - b'0') as usize;
                    has_current = true;
                }
                b';' => {
                    params.push(if has_current { current } else { 0 });
                    current = 0;
                    has_current = false;
                }
                0x40..=0x7e => {
                    params.push(if has_current { current } else { 0 });
                    self.apply_csi_cursor_effect(byte, &params);
                    break;
                }
                _ => {}
            }
        }
    }

    fn apply_csi_cursor_effect(&mut self, final_byte: u8, params: &[usize]) {
        let count = |index| {
            params
                .get(index)
                .copied()
                .filter(|value| *value != 0)
                .unwrap_or(1)
        };
        match final_byte {
            b'A' => self.cursor_row = self.cursor_row.saturating_sub(count(0)),
            b'B' => {
                self.cursor_row = (self.cursor_row + count(0)).min(self.rows().saturating_sub(1))
            }
            b'C' => {
                self.cursor_col =
                    (self.cursor_col + count(0)).min(self.columns().saturating_sub(1));
            }
            b'D' => self.cursor_col = self.cursor_col.saturating_sub(count(0)),
            b'E' => {
                self.cursor_row = (self.cursor_row + count(0)).min(self.rows().saturating_sub(1));
                self.cursor_col = 0;
            }
            b'F' => {
                self.cursor_row = self.cursor_row.saturating_sub(count(0));
                self.cursor_col = 0;
            }
            b'G' => {
                self.cursor_col = count(0)
                    .saturating_sub(1)
                    .min(self.columns().saturating_sub(1))
            }
            b'H' | b'f' => {
                self.cursor_row = count(0)
                    .saturating_sub(1)
                    .min(self.rows().saturating_sub(1));
                self.cursor_col = count(1)
                    .saturating_sub(1)
                    .min(self.columns().saturating_sub(1));
            }
            b'J' if params.first().copied().unwrap_or(0) == 2 => {
                self.cursor_row = 0;
                self.cursor_col = 0;
            }
            _ => {}
        }
    }
}

impl DrawTarget for FramebufferDisplay {
    fn size(&self) -> (usize, usize) {
        let canvas = self.canvas.lock();
        (canvas.info.width, canvas.info.height)
    }

    fn draw_pixel(&mut self, x: usize, y: usize, rgb: os_terminal::Rgb) {
        let mut canvas = self.canvas.lock();
        if x < canvas.info.width && y < canvas.info.height {
            canvas.write_pixel(x, y, rgb);
        }
    }
}
