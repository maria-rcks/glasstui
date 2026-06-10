//! Renders the scene with the lens applied to a PPM image, for visually
//! inspecting the liquid-glass optics without a terminal.
//!
//! Usage: cargo run --example snapshot [out.ppm]

use glasstui::framebuffer::Framebuffer;
use glasstui::glass::{self, GlassParams};
use glasstui::scene::Scene;
use std::io::Write;

fn main() -> std::io::Result<()> {
    let path = std::env::args().nth(1).unwrap_or("snapshot.ppm".into());
    let (w, h) = (480usize, 280usize);

    let mut scene_fb = Framebuffer::new(w, h);
    Scene::default().render(&mut scene_fb);

    let params = GlassParams {
        radius: 70.0,
        depth: 34.0,
        chroma: 0.08,
        ..Default::default()
    };
    let mut out = Framebuffer::new(w, h);
    glass::apply(&scene_fb, &mut out, 190.0, 118.0, &params);

    let mut file = std::io::BufWriter::new(std::fs::File::create(&path)?);
    writeln!(file, "P6\n{w} {h}\n255")?;
    for y in 0..h {
        for x in 0..w {
            let c = out.get(x as isize, y as isize);
            file.write_all(&[c.r, c.g, c.b])?;
        }
    }
    eprintln!("wrote {path}");
    Ok(())
}
