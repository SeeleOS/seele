use crate::graphics::tty::Color;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnsiColor {
    Black = 0,
    Red = 1,
    Green = 2,
    Yellow = 3,
    Blue = 4,
    Magenta = 5,
    Cyan = 6,
    White = 7,
    BrightBlack = 8,
    BrightRed = 9,
    BrightGreen = 10,
    BrightYellow = 11,
    BrightBlue = 12,
    BrightMagenta = 13,
    BrightCyan = 14,
    BrightWhite = 15,
}

impl AnsiColor {
    pub fn from_ansi_code(code: u16) -> Option<Self> {
        match code {
            30 | 40 => Some(AnsiColor::Black),
            31 | 41 => Some(AnsiColor::Red),
            32 | 42 => Some(AnsiColor::Green),
            33 | 43 => Some(AnsiColor::Yellow),
            34 | 44 => Some(AnsiColor::Blue),
            35 | 45 => Some(AnsiColor::Magenta),
            36 | 46 => Some(AnsiColor::Cyan),
            37 | 47 => Some(AnsiColor::White),
            90 | 100 => Some(AnsiColor::BrightBlack),
            91 | 101 => Some(AnsiColor::BrightRed),
            92 | 102 => Some(AnsiColor::BrightGreen),
            93 | 103 => Some(AnsiColor::BrightYellow),
            94 | 104 => Some(AnsiColor::BrightBlue),
            95 | 105 => Some(AnsiColor::BrightMagenta),
            96 | 106 => Some(AnsiColor::BrightCyan),
            97 | 107 => Some(AnsiColor::BrightWhite),
            _ => None,
        }
    }

    pub fn as_rgb(self) -> Color {
        match self {
            AnsiColor::Black => (0, 0, 0),
            AnsiColor::Red => (170, 0, 0),
            AnsiColor::Green => (0, 170, 0),
            AnsiColor::Yellow => (170, 85, 0),
            AnsiColor::Blue => (0, 0, 170),
            AnsiColor::Magenta => (170, 0, 170),
            AnsiColor::Cyan => (0, 170, 170),
            AnsiColor::White => (170, 170, 170),
            AnsiColor::BrightBlack => (85, 85, 85),
            AnsiColor::BrightRed => (255, 85, 85),
            AnsiColor::BrightGreen => (85, 255, 85),
            AnsiColor::BrightYellow => (255, 255, 85),
            AnsiColor::BrightBlue => (85, 85, 255),
            AnsiColor::BrightMagenta => (255, 85, 255),
            AnsiColor::BrightCyan => (85, 255, 255),
            AnsiColor::BrightWhite => (255, 255, 255),
        }
    }
}
