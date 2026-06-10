//! RGB pixel framebuffer rendered to the terminal as half-block characters.
//!
//! Each terminal cell shows two vertically stacked pixels using `▀`:
//! the foreground color is the top pixel, the background color the bottom.
//! This doubles vertical resolution and makes terminal pixels roughly square.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Color;

pub const HALF_BLOCK: &str = "▀";

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Rgb {
    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }

    pub fn as_f32(self) -> (f32, f32, f32) {
        (self.r as f32, self.g as f32, self.b as f32)
    }

    pub fn from_f32(r: f32, g: f32, b: f32) -> Self {
        Self {
            r: r.clamp(0.0, 255.0) as u8,
            g: g.clamp(0.0, 255.0) as u8,
            b: b.clamp(0.0, 255.0) as u8,
        }
    }

    pub fn lerp(self, other: Rgb, t: f32) -> Rgb {
        let t = t.clamp(0.0, 1.0);
        let (r0, g0, b0) = self.as_f32();
        let (r1, g1, b1) = other.as_f32();
        Rgb::from_f32(r0 + (r1 - r0) * t, g0 + (g1 - g0) * t, b0 + (b1 - b0) * t)
    }
}

impl From<Rgb> for Color {
    fn from(c: Rgb) -> Color {
        Color::Rgb(c.r, c.g, c.b)
    }
}

#[derive(Clone, Debug)]
pub struct Framebuffer {
    width: usize,
    height: usize,
    pixels: Vec<Rgb>,
}

impl Framebuffer {
    pub fn new(width: usize, height: usize) -> Self {
        Self {
            width,
            height,
            pixels: vec![Rgb::default(); width * height],
        }
    }

    pub fn width(&self) -> usize {
        self.width
    }

    pub fn height(&self) -> usize {
        self.height
    }

    pub fn resize(&mut self, width: usize, height: usize) {
        if width != self.width || height != self.height {
            self.width = width;
            self.height = height;
            self.pixels = vec![Rgb::default(); width * height];
        }
    }

    pub fn fill(&mut self, color: Rgb) {
        self.pixels.fill(color);
    }

    pub fn copy_from(&mut self, other: &Framebuffer) {
        self.resize(other.width, other.height);
        self.pixels.copy_from_slice(&other.pixels);
    }

    pub fn set(&mut self, x: usize, y: usize, color: Rgb) {
        if x < self.width && y < self.height {
            self.pixels[y * self.width + x] = color;
        }
    }

    /// Get a pixel, clamping coordinates to the buffer edges.
    pub fn get(&self, x: isize, y: isize) -> Rgb {
        if self.width == 0 || self.height == 0 {
            return Rgb::default();
        }
        let x = x.clamp(0, self.width as isize - 1) as usize;
        let y = y.clamp(0, self.height as isize - 1) as usize;
        self.pixels[y * self.width + x]
    }

    /// Bilinearly interpolated sample at a fractional pixel position.
    pub fn sample_bilinear(&self, fx: f32, fy: f32) -> (f32, f32, f32) {
        let x0 = fx.floor();
        let y0 = fy.floor();
        let tx = fx - x0;
        let ty = fy - y0;
        let (x0, y0) = (x0 as isize, y0 as isize);

        let c00 = self.get(x0, y0).as_f32();
        let c10 = self.get(x0 + 1, y0).as_f32();
        let c01 = self.get(x0, y0 + 1).as_f32();
        let c11 = self.get(x0 + 1, y0 + 1).as_f32();

        let mix = |a: f32, b: f32, t: f32| a + (b - a) * t;
        let top = (
            mix(c00.0, c10.0, tx),
            mix(c00.1, c10.1, tx),
            mix(c00.2, c10.2, tx),
        );
        let bot = (
            mix(c01.0, c11.0, tx),
            mix(c01.1, c11.1, tx),
            mix(c01.2, c11.2, tx),
        );
        (
            mix(top.0, bot.0, ty),
            mix(top.1, bot.1, ty),
            mix(top.2, bot.2, ty),
        )
    }

    /// Render the framebuffer into a ratatui buffer using half-blocks.
    pub fn render_to_buffer(&self, area: Rect, buf: &mut Buffer) {
        for row in 0..area.height {
            let py = row as usize * 2;
            for col in 0..area.width {
                let px = col as usize;
                if px >= self.width || py >= self.height {
                    continue;
                }
                let top = self.get(px as isize, py as isize);
                let bottom = self.get(px as isize, py as isize + 1);
                if let Some(cell) = buf.cell_mut((area.x + col, area.y + row)) {
                    cell.set_symbol(HALF_BLOCK);
                    cell.set_fg(top.into());
                    cell.set_bg(bottom.into());
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_and_get_roundtrip() {
        let mut fb = Framebuffer::new(10, 8);
        fb.set(3, 4, Rgb::new(1, 2, 3));
        assert_eq!(fb.get(3, 4), Rgb::new(1, 2, 3));
    }

    #[test]
    fn get_clamps_out_of_bounds() {
        let mut fb = Framebuffer::new(4, 4);
        fb.set(0, 0, Rgb::new(9, 9, 9));
        fb.set(3, 3, Rgb::new(7, 7, 7));
        assert_eq!(fb.get(-5, -5), Rgb::new(9, 9, 9));
        assert_eq!(fb.get(100, 100), Rgb::new(7, 7, 7));
    }

    #[test]
    fn set_out_of_bounds_is_ignored() {
        let mut fb = Framebuffer::new(4, 4);
        fb.set(100, 100, Rgb::new(1, 1, 1)); // must not panic
        assert_eq!(fb.get(3, 3), Rgb::default());
    }

    #[test]
    fn bilinear_interpolates_midpoint() {
        let mut fb = Framebuffer::new(2, 1);
        fb.set(0, 0, Rgb::new(0, 0, 0));
        fb.set(1, 0, Rgb::new(100, 200, 50));
        let (r, g, b) = fb.sample_bilinear(0.5, 0.0);
        assert!((r - 50.0).abs() < 1e-3);
        assert!((g - 100.0).abs() < 1e-3);
        assert!((b - 25.0).abs() < 1e-3);
    }

    #[test]
    fn bilinear_at_integer_returns_pixel() {
        let mut fb = Framebuffer::new(3, 3);
        fb.set(1, 1, Rgb::new(10, 20, 30));
        let (r, g, b) = fb.sample_bilinear(1.0, 1.0);
        assert_eq!((r as u8, g as u8, b as u8), (10, 20, 30));
    }

    #[test]
    fn render_to_buffer_maps_half_blocks() {
        let mut fb = Framebuffer::new(2, 4);
        fb.set(0, 0, Rgb::new(255, 0, 0)); // top pixel of cell (0,0)
        fb.set(0, 1, Rgb::new(0, 255, 0)); // bottom pixel of cell (0,0)
        let area = Rect::new(0, 0, 2, 2);
        let mut buf = Buffer::empty(area);
        fb.render_to_buffer(area, &mut buf);
        let cell = buf.cell((0, 0)).unwrap();
        assert_eq!(cell.symbol(), HALF_BLOCK);
        assert_eq!(cell.fg, Color::Rgb(255, 0, 0));
        assert_eq!(cell.bg, Color::Rgb(0, 255, 0));
    }

    #[test]
    fn resize_reallocates_and_clears() {
        let mut fb = Framebuffer::new(2, 2);
        fb.set(0, 0, Rgb::new(5, 5, 5));
        fb.resize(8, 6);
        assert_eq!(fb.width(), 8);
        assert_eq!(fb.height(), 6);
        assert_eq!(fb.get(0, 0), Rgb::default());
    }

    #[test]
    fn lerp_blends_colors() {
        let a = Rgb::new(0, 0, 0);
        let b = Rgb::new(200, 100, 50);
        assert_eq!(a.lerp(b, 0.5), Rgb::new(100, 50, 25));
        assert_eq!(a.lerp(b, 0.0), a);
        assert_eq!(a.lerp(b, 1.0), b);
    }
}
