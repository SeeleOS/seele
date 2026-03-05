use crate::graphics::tty::{DEFAULT_FOREGROUND, EMPTY_BACKGROUND, Tty};

impl<'a> Tty<'a> {
    pub fn reset_color(&mut self) {
        self.current_background = EMPTY_BACKGROUND;
        self.current_foreground = DEFAULT_FOREGROUND
    }
}
