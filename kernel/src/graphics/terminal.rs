use core::fmt::{Arguments, Write};

use alloc::boxed::Box;
use conquer_once::spin::OnceCell;
use os_terminal::{Palette, Terminal, font::TrueTypeFont};
use spin::Mutex;

use crate::{
    filesystem::{path::Path, vfs_operations::read_all},
    graphics::{
        framebuffer::{Canvas, FRAME_BUFFER},
        terminal,
    },
};

const FONT_PATH: &str = "/misc/fonts/maplem~1.ttf";

pub fn init_font() {
    let font_path = Path::new(FONT_PATH);
    let font: &'static mut [u8] =
        Box::leak(Box::new(read_all(font_path).unwrap()).into_boxed_slice());

    let font_manager = TrueTypeFont::new(13.0, font);

    TERMINAL
        .get()
        .unwrap()
        .lock()
        .set_font_manager(Box::new(font_manager));
}

pub const COLOR_SCHEME: Palette = Palette {
    background: (30, 34, 51),
    foreground: (237, 239, 246),
    ansi_colors: [
        // 0-7 normal
        (30, 34, 51),    // black (ink)
        (192, 124, 138), // red
        (95, 159, 161),  // green
        (230, 210, 167), // yellow
        (108, 141, 212), // blue
        (76, 86, 141),   // magenta (indigo)
        (164, 206, 244), // cyan (sky)
        (237, 239, 246), // white (cloud)
        // 8-15 bright
        (45, 51, 72),    // bright black
        (217, 162, 173), // bright red
        (127, 185, 187), // bright green
        (241, 227, 194), // bright yellow
        (148, 174, 230), // bright blue
        (107, 121, 176), // bright magenta
        (195, 224, 250), // bright cyan
        (255, 255, 255), // bright white
    ],
};

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
    let mut term = TERMINAL.get().unwrap().lock();

    term.write_fmt(args).unwrap();
    term.flush();

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
