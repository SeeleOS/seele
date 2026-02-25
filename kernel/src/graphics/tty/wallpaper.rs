use crate::graphics::{framebuffer::Canvas, tty::Tty};
pub static WALLPAPER: &[u8] = include_bytes!("../../../../resources/wallpaper.bin");

impl<'a> Tty<'a> {
    pub fn draw_wallpaper(&mut self) {
        let width = 1280;
        let height = 720;

        for y in 0..height {
            for x in 0..width {
                self.draw_wallpaper_pixel(x, y, &mut *self.canvas.lock());
            }
        }

        self.canvas.lock().flush();
    }

    pub fn draw_wallpaper_pixel(&mut self, x: usize, y: usize, canvas: &mut Canvas) {
        // 计算该像素在 bin 文件中的起始位置
        let i = (y * 1280 + x) * 4;

        // 从静态数组中读取颜色分量
        // 注意：由于我们转换时用了 BGRA，所以顺序是 B, G, R, A
        let mut b = WALLPAPER[i];
        let mut g = WALLPAPER[i + 1];
        let mut r = WALLPAPER[i + 2];

        b = b - (b >> 1);
        g = g - (g >> 1);
        r = r - (r >> 1);

        // 第 4 位是 Alpha(i+3)，我们通常跳过它，或者用来做透明度计算

        // 调用你引以为傲的 write_pixel
        canvas.write_pixel(x, y, r, g, b);
    }
}
