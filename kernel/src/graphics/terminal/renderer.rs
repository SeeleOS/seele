use os_terminal::DrawTarget;
use spin::Mutex;

use crate::misc::framebuffer::Canvas;

pub struct TermRenderer<'a> {
    pub(crate) canvas: &'a Mutex<Canvas>,
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

impl<'a> DrawTarget for TermRenderer<'a> {
    fn size(&self) -> (usize, usize) {
        (self.width as usize, self.height as usize)
    }

    #[inline(always)]
    fn draw_pixel(&mut self, x: usize, y: usize, rgb: os_terminal::Rgb) {
        self.canvas.lock().write_pixel(x, y, (rgb.0, rgb.1, rgb.2));
    }
}
