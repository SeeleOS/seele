use os_terminal::DrawTarget;
use spin::{Mutex, MutexGuard};

use crate::misc::framebuffer::Canvas;

pub struct TermRenderer<'a> {
    pub(crate) canvas: &'a Mutex<Canvas>,
    frame_canvas: Option<MutexGuard<'a, Canvas>>,
    pub width: u32,
    pub height: u32,
}

impl<'a> TermRenderer<'a> {
    pub fn new(canvas: &'a Mutex<Canvas>) -> Self {
        let canvas_guard = canvas.lock();
        let width = canvas_guard.width;
        let height = canvas_guard.height;
        drop(canvas_guard);
        Self {
            canvas,
            frame_canvas: None,
            width,
            height,
        }
    }
}

impl<'a> DrawTarget for TermRenderer<'a> {
    fn size(&self) -> (usize, usize) {
        (self.width as usize, self.height as usize)
    }

    fn begin_draw(&mut self) {
        if self.frame_canvas.is_none() {
            self.frame_canvas = Some(self.canvas.lock());
        }
    }

    fn end_draw(&mut self) {
        self.frame_canvas = None;
    }

    #[inline(always)]
    fn draw_pixel(&mut self, x: usize, y: usize, rgb: os_terminal::Rgb) {
        let canvas = self
            .frame_canvas
            .as_mut()
            .expect("begin_draw must be called before draw_pixel");
        canvas.write_pixel(x, y, (rgb.0, rgb.1, rgb.2));
    }
}
