use vte::Perform;

use crate::graphics::tty::Tty;

impl<'a> Perform for Tty<'a> {
    fn print(&mut self, c: char) {
        if self.cursor_x >= self.screen_width_char() as u32 {
            self.new_line();
        }

        if self.cursor_y >= self.screen_height_chars() as u32 {
            self.scroll_up();
        }

        self.push_char(c);
        self.render();
    }
}
