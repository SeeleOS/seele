use os_terminal::Palette;

pub type Color = (u8, u8, u8);

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

