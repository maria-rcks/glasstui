//! Large text rendering from an embedded 8x8 bitmap font, scaled up by an
//! integer factor and drawn into the pixel framebuffer.

use crate::framebuffer::{Framebuffer, Rgb};
use font8x8::legacy::BASIC_LEGACY;

pub const GLYPH_SIZE: usize = 8;

/// Bitmap rows for an ASCII character (LSB of each row byte = leftmost pixel).
pub fn glyph(c: char) -> [u8; GLYPH_SIZE] {
    let idx = c as usize;
    if idx < BASIC_LEGACY.len() {
        BASIC_LEGACY[idx]
    } else {
        BASIC_LEGACY['?' as usize]
    }
}

pub fn text_width(text: &str, scale: usize) -> usize {
    text.chars().count() * GLYPH_SIZE * scale
}

pub fn text_height(scale: usize) -> usize {
    GLYPH_SIZE * scale
}

/// Draw `text` with its top-left corner at (`x`, `y`) in pixels.
/// Optionally draws a drop shadow first for legibility on busy backdrops.
pub fn draw_text(
    fb: &mut Framebuffer,
    text: &str,
    x: isize,
    y: isize,
    scale: usize,
    color: Rgb,
    shadow: Option<Rgb>,
) {
    if let Some(shadow_color) = shadow {
        let off = scale.max(1) as isize / 2 + 1;
        draw_text_plain(fb, text, x + off, y + off, scale, shadow_color);
    }
    draw_text_plain(fb, text, x, y, scale, color);
}

fn draw_text_plain(fb: &mut Framebuffer, text: &str, x: isize, y: isize, scale: usize, color: Rgb) {
    let scale = scale.max(1);
    for (ci, c) in text.chars().enumerate() {
        let rows = glyph(c);
        let gx = x + (ci * GLYPH_SIZE * scale) as isize;
        for (ry, row) in rows.iter().enumerate() {
            for rx in 0..GLYPH_SIZE {
                if (row >> rx) & 1 == 0 {
                    continue;
                }
                for sy in 0..scale {
                    for sx in 0..scale {
                        let px = gx + (rx * scale + sx) as isize;
                        let py = y + (ry * scale + sy) as isize;
                        if px >= 0 && py >= 0 {
                            fb.set(px as usize, py as usize, color);
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn count_lit(fb: &Framebuffer, color: Rgb) -> usize {
        let mut n = 0;
        for y in 0..fb.height() {
            for x in 0..fb.width() {
                if fb.get(x as isize, y as isize) == color {
                    n += 1;
                }
            }
        }
        n
    }

    #[test]
    fn glyph_a_has_pixels() {
        let g = glyph('A');
        assert!(g.iter().any(|row| *row != 0));
    }

    #[test]
    fn glyph_space_is_empty() {
        let g = glyph(' ');
        assert!(g.iter().all(|row| *row == 0));
    }

    #[test]
    fn glyph_out_of_range_falls_back() {
        assert_eq!(glyph('\u{1F600}'), glyph('?'));
    }

    #[test]
    fn text_metrics() {
        assert_eq!(text_width("AB", 3), 2 * 8 * 3);
        assert_eq!(text_height(3), 24);
    }

    #[test]
    fn draw_text_scale_multiplies_pixel_count() {
        let white = Rgb::new(255, 255, 255);
        let mut fb1 = Framebuffer::new(16, 16);
        draw_text(&mut fb1, "A", 0, 0, 1, white, None);
        let n1 = count_lit(&fb1, white);
        assert!(n1 > 0);

        let mut fb2 = Framebuffer::new(32, 32);
        draw_text(&mut fb2, "A", 0, 0, 2, white, None);
        let n2 = count_lit(&fb2, white);
        assert_eq!(n2, n1 * 4);
    }

    #[test]
    fn draw_text_negative_origin_does_not_panic() {
        let mut fb = Framebuffer::new(8, 8);
        draw_text(&mut fb, "XY", -5, -5, 2, Rgb::new(255, 0, 0), None);
    }

    #[test]
    fn shadow_paints_second_color() {
        let white = Rgb::new(255, 255, 255);
        let gray = Rgb::new(40, 40, 40);
        let mut fb = Framebuffer::new(40, 40);
        draw_text(&mut fb, "I", 4, 4, 2, white, Some(gray));
        assert!(count_lit(&fb, white) > 0);
        assert!(count_lit(&fb, gray) > 0);
    }
}
