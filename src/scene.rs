//! The background scene: a gradient backdrop with geometric artifacts and
//! big bitmap text — plenty of detail for the lens to bend.

use crate::bigtext;
use crate::framebuffer::{Framebuffer, Rgb};

#[derive(Clone, Debug)]
pub struct TextLine {
    pub text: String,
    pub scale: usize,
    pub color: Rgb,
}

#[derive(Clone, Debug)]
pub struct Scene {
    pub lines: Vec<TextLine>,
}

impl Default for Scene {
    fn default() -> Self {
        Self {
            lines: vec![
                TextLine {
                    text: "LIQUID".into(),
                    scale: 4,
                    color: Rgb::new(240, 232, 210),
                },
                TextLine {
                    text: "GLASS".into(),
                    scale: 4,
                    color: Rgb::new(130, 215, 255),
                },
                TextLine {
                    text: "drag the lens".into(),
                    scale: 1,
                    color: Rgb::new(180, 180, 200),
                },
            ],
        }
    }
}

impl Scene {
    /// Grow or shrink all text lines by `delta` scale steps (clamped to
    /// 1..=10). Returns true if anything changed.
    pub fn adjust_scale(&mut self, delta: isize) -> bool {
        let mut changed = false;
        for line in &mut self.lines {
            let ns = (line.scale as isize + delta).clamp(1, 10) as usize;
            if ns != line.scale {
                line.scale = ns;
                changed = true;
            }
        }
        changed
    }

    pub fn render(&self, fb: &mut Framebuffer) {
        let w = fb.width();
        let h = fb.height();
        if w == 0 || h == 0 {
            return;
        }

        self.draw_backdrop(fb);
        self.draw_artifacts(fb);
        self.draw_text(fb);
    }

    fn draw_backdrop(&self, fb: &mut Framebuffer) {
        let w = fb.width() as f32;
        let h = fb.height() as f32;
        let top = Rgb::new(16, 20, 52);
        let bottom = Rgb::new(8, 56, 68);
        for y in 0..fb.height() {
            let t = y as f32 / h.max(1.0);
            let base = top.lerp(bottom, t);
            for x in 0..fb.width() {
                // Soft diagonal banding so even "empty" areas refract visibly.
                let stripe = (((x as f32 / w.max(1.0)) * 14.0 + t * 10.0).sin() * 9.0) as i16;
                let add = |c: u8| (c as i16 + stripe).clamp(0, 255) as u8;
                fb.set(x, y, Rgb::new(add(base.r), add(base.g), add(base.b)));
            }
        }
    }

    fn draw_artifacts(&self, fb: &mut Framebuffer) {
        let w = fb.width();
        let h = fb.height();

        // Thin grid lines.
        for y in (0..h).step_by(12) {
            for x in 0..w {
                let c = fb.get(x as isize, y as isize);
                fb.set(x, y, c.lerp(Rgb::new(255, 255, 255), 0.10));
            }
        }
        for x in (0..w).step_by(24) {
            for y in 0..h {
                let c = fb.get(x as isize, y as isize);
                fb.set(x, y, c.lerp(Rgb::new(255, 255, 255), 0.10));
            }
        }

        // Checkerboard band along the bottom.
        let band = 10.min(h);
        for y in h.saturating_sub(band)..h {
            for x in 0..w {
                if (x / 5 + y / 5) % 2 == 0 {
                    fb.set(x, y, Rgb::new(222, 120, 60));
                } else {
                    fb.set(x, y, Rgb::new(36, 28, 40));
                }
            }
        }

        // A few glowing discs.
        let discs: [(f32, f32, f32, Rgb); 3] = [
            (
                w as f32 * 0.16,
                h as f32 * 0.24,
                9.0,
                Rgb::new(235, 90, 120),
            ),
            (
                w as f32 * 0.85,
                h as f32 * 0.30,
                12.0,
                Rgb::new(90, 220, 140),
            ),
            (
                w as f32 * 0.72,
                h as f32 * 0.78,
                7.0,
                Rgb::new(250, 210, 90),
            ),
        ];
        for (cx, cy, r, color) in discs {
            let x0 = (cx - r).max(0.0) as usize;
            let x1 = ((cx + r) as usize).min(w.saturating_sub(1));
            let y0 = (cy - r).max(0.0) as usize;
            let y1 = ((cy + r) as usize).min(h.saturating_sub(1));
            if w == 0 || h == 0 {
                continue;
            }
            for y in y0..=y1 {
                for x in x0..=x1 {
                    let dx = x as f32 + 0.5 - cx;
                    let dy = y as f32 + 0.5 - cy;
                    let d = (dx * dx + dy * dy).sqrt();
                    if d < r {
                        let t = 1.0 - (d / r) * (d / r);
                        let bg = fb.get(x as isize, y as isize);
                        fb.set(x, y, bg.lerp(color, t));
                    }
                }
            }
        }
    }

    fn draw_text(&self, fb: &mut Framebuffer) {
        let w = fb.width() as isize;
        let h = fb.height() as isize;
        let gap = 4isize;
        let total: isize = self
            .lines
            .iter()
            .map(|l| bigtext::text_height(l.scale) as isize + gap)
            .sum::<isize>()
            - gap;
        let mut y = (h - total) / 2;
        for line in &self.lines {
            let tw = bigtext::text_width(&line.text, line.scale) as isize;
            let x = (w - tw) / 2;
            bigtext::draw_text(
                fb,
                &line.text,
                x,
                y,
                line.scale,
                line.color,
                Some(Rgb::new(6, 8, 18)),
            );
            y += bigtext::text_height(line.scale) as isize + gap;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_fills_buffer() {
        let scene = Scene::default();
        let mut fb = Framebuffer::new(200, 120);
        scene.render(&mut fb);
        // Backdrop must have replaced the default black everywhere on top row.
        let mut nonblack = 0;
        for x in 0..200 {
            if fb.get(x, 0) != Rgb::default() {
                nonblack += 1;
            }
        }
        assert!(nonblack > 150);
    }

    #[test]
    fn render_draws_title_text_pixels() {
        let scene = Scene::default();
        let mut fb = Framebuffer::new(260, 160);
        scene.render(&mut fb);
        let title = scene.lines[0].color;
        let mut count = 0;
        for y in 0..160 {
            for x in 0..260 {
                if fb.get(x, y) == title {
                    count += 1;
                }
            }
        }
        assert!(count > 100, "expected big text pixels, found {count}");
    }

    #[test]
    fn adjust_scale_clamps_and_reports_change() {
        let mut scene = Scene::default();
        assert!(scene.adjust_scale(1));
        assert_eq!(scene.lines[0].scale, 5);
        // Shrink to the floor: caption (scale 2 now) hits 1 and stops.
        for _ in 0..20 {
            scene.adjust_scale(-1);
        }
        assert!(scene.lines.iter().all(|l| l.scale == 1));
        assert!(!scene.adjust_scale(-1), "at floor, nothing changes");
        for _ in 0..20 {
            scene.adjust_scale(1);
        }
        assert!(scene.lines.iter().all(|l| l.scale == 10));
    }

    #[test]
    fn render_survives_tiny_buffers() {
        let scene = Scene::default();
        for (w, h) in [(0, 0), (1, 1), (3, 2), (10, 4)] {
            let mut fb = Framebuffer::new(w, h);
            scene.render(&mut fb); // must not panic
        }
    }
}
