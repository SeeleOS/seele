use vte::Perform;

use crate::{
    graphics::tty::{Tty, ansi_color::AnsiColor},
    s_println,
};

impl<'a> Perform for Tty<'a> {
    fn print(&mut self, c: char) {
        if self.cursor_x >= self.screen_width_char() as u32 {
            self.new_line();
        }

        if self.cursor_y >= self.screen_height_chars() as u32 {
            self.scroll_up();
        }

        self.push_char(c);
    }

    fn execute(&mut self, byte: u8) {
        match byte {
            b'\n' => self.new_line(),
            _ => s_println!(
                "Unimplemented ansi escape code or something: {}",
                byte as char
            ),
        }
    }

    fn csi_dispatch(
        &mut self,
        _params: &vte::Params,
        _intermediates: &[u8],
        _ignore: bool,
        action: char,
    ) {
        match action {
            'm' => {
                for param in _params {
                    match param[0] {
                        0 => self.reset_color(),
                        1 => self.bold = true,
                        30..=37 | 90..=97 => {
                            self.current_background =
                                AnsiColor::from_ansi_code(param[0]).unwrap().as_rgb()
                        }
                        40..=47 | 100..=107 => {
                            self.current_foreground =
                                AnsiColor::from_ansi_code(param[0]).unwrap().as_rgb()
                        }
                        _ => s_println!("unimplemented color thing: {}", param[0]),
                    }
                }
            }
            _ => s_println!("Unimplemented csi dispatch asni escape code {}", action),
        }
    }
}
