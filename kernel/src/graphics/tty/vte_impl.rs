use vte::Perform;

use crate::graphics::tty::Tty;

impl<'a> Perform for Tty<'a> {
    fn print(&mut self, _c: char) {
        self.push_char(_c);
    }
}
