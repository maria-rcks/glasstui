//! Frame composition: scene -> lens warp -> half-block canvas, plus the
//! header bar and the settings popup.

use crate::app::{App, BAR_LEN, Mode, NAME_W, panel_rect};
use crate::framebuffer::Framebuffer;
use crate::glass::{self, PARAMS};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, Paragraph};

const HEADER_BG: Color = Color::Rgb(10, 12, 26);
const HEADER_FG: Color = Color::Rgb(150, 160, 190);
const PANEL_BG: Color = Color::Rgb(14, 16, 30);
const ACCENT: Color = Color::Rgb(120, 210, 255);

/// Owns the framebuffers so they are reused across frames; the static scene
/// is only re-rendered when the terminal size changes.
pub struct Renderer {
    scene_fb: Framebuffer,
    out_fb: Framebuffer,
}

impl Default for Renderer {
    fn default() -> Self {
        Self::new()
    }
}

impl Renderer {
    pub fn new() -> Self {
        Self {
            scene_fb: Framebuffer::new(0, 0),
            out_fb: Framebuffer::new(0, 0),
        }
    }
}

pub fn draw(frame: &mut Frame, app: &mut App, r: &mut Renderer) {
    let area = frame.area();
    if area.width == 0 || area.height == 0 {
        return;
    }
    let (pw, ph) = (area.width as usize, area.height as usize * 2);

    if r.scene_fb.width() != pw || r.scene_fb.height() != ph || app.scene_dirty {
        r.scene_fb.resize(pw, ph);
        app.scene.render(&mut r.scene_fb);
        app.scene_dirty = false;
    }
    app.set_canvas_size(pw, ph);

    glass::apply(
        &r.scene_fb,
        &mut r.out_fb,
        app.lens_x,
        app.lens_y,
        &app.params,
    );
    r.out_fb.render_to_buffer(area, frame.buffer_mut());

    draw_header(frame, app, area);
    if app.mode == Mode::Settings {
        draw_settings(frame, app, area);
    }
}

fn draw_header(frame: &mut Frame, app: &App, area: Rect) {
    let header = Line::from(vec![
        Span::styled(
            " glasstui ",
            Style::new().fg(ACCENT).add_modifier(Modifier::BOLD),
        ),
        Span::raw("drag lens · double-click lens: settings · scroll: radius · +/-: text · q: quit"),
    ]);
    frame.render_widget(
        Paragraph::new(header).style(Style::new().bg(HEADER_BG).fg(HEADER_FG)),
        Rect::new(area.x, area.y, area.width, 1),
    );

    if area.height > 1 {
        let status = format!(
            " lens ({:.0},{:.0})  r={:.0}px  depth={:.0}  ior={:.2}{} ",
            app.lens_x,
            app.lens_y,
            app.params.radius,
            app.params.depth,
            app.params.ior,
            if app.is_dragging() {
                "  [dragging]"
            } else {
                ""
            },
        );
        frame.render_widget(
            Paragraph::new(status).style(Style::new().bg(HEADER_BG).fg(HEADER_FG)),
            Rect::new(area.x, area.y + area.height - 1, area.width, 1),
        );
    }
}

fn format_value(idx: usize, v: f32) -> String {
    if PARAMS[idx].step >= 1.0 {
        format!("{v:>6.0}")
    } else {
        format!("{v:>6.2}")
    }
}

fn draw_settings(frame: &mut Frame, app: &App, area: Rect) {
    let pr = panel_rect(area.width, area.height);
    let rect = Rect::new(pr.x, pr.y, pr.w, pr.h);
    if rect.width < 10 || rect.height < 4 {
        return;
    }

    frame.render_widget(Clear, rect);
    let block = Block::bordered()
        .title(Line::from(Span::styled(
            " Liquid Glass ",
            Style::new().fg(ACCENT).add_modifier(Modifier::BOLD),
        )))
        .style(Style::new().bg(PANEL_BG).fg(Color::Rgb(200, 205, 220)));
    let inner = block.inner(rect);
    frame.render_widget(block, rect);

    let mut lines: Vec<Line> = Vec::with_capacity(PARAMS.len() + 1);
    for (i, spec) in PARAMS.iter().enumerate() {
        let selected = i == app.selected;
        let t = spec.normalized(&app.params);
        let filled = (t * BAR_LEN as f32).round() as usize;
        let bar: String = "█".repeat(filled.min(BAR_LEN as usize))
            + &"░".repeat((BAR_LEN as usize).saturating_sub(filled));
        let value = format_value(i, (spec.get)(&app.params));
        // Column layout must match the hit zones in app.rs:
        // " {name:<12} ◀ {bar:18} {value:>6} ▶"
        let row_style = if selected {
            Style::new().fg(ACCENT).add_modifier(Modifier::BOLD)
        } else {
            Style::new().fg(Color::Rgb(170, 175, 195))
        };
        let marker = if selected { "▸" } else { " " };
        lines.push(Line::from(vec![
            Span::styled(format!("{marker}{:<w$}", spec.name, w = NAME_W), row_style),
            Span::styled(" ◀ ", row_style),
            Span::styled(
                bar,
                if selected {
                    Style::new().fg(ACCENT)
                } else {
                    Style::new().fg(Color::Rgb(110, 120, 150))
                },
            ),
            Span::styled(format!(" {value} "), row_style),
            Span::styled("▶", row_style),
        ]));
    }
    lines.push(Line::from(Span::styled(
        " ↑↓ select · ←→/scroll adjust · click bar · r reset",
        Style::new().fg(Color::Rgb(110, 115, 140)),
    )));

    frame.render_widget(Paragraph::new(lines), inner);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::{BAR_COL, PANEL_W};
    use crate::framebuffer::HALF_BLOCK;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    fn render(app: &mut App, w: u16, h: u16) -> ratatui::buffer::Buffer {
        let backend = TestBackend::new(w, h);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut renderer = Renderer::new();
        terminal.draw(|f| draw(f, app, &mut renderer)).unwrap();
        terminal.backend().buffer().clone()
    }

    #[test]
    fn draw_fills_canvas_with_half_blocks() {
        let mut app = App::new();
        let buf = render(&mut app, 80, 30);
        let mut half_blocks = 0;
        for y in 0..30u16 {
            for x in 0..80u16 {
                if buf.cell((x, y)).unwrap().symbol() == HALF_BLOCK {
                    half_blocks += 1;
                }
            }
        }
        // Everything except the header/status rows should be pixel canvas.
        assert!(half_blocks > 80 * 26, "got {half_blocks}");
    }

    #[test]
    fn draw_sets_canvas_size_on_app() {
        let mut app = App::new();
        render(&mut app, 64, 20);
        assert_eq!(app.canvas_w, 64);
        assert_eq!(app.canvas_h, 40);
    }

    #[test]
    fn settings_panel_appears_in_settings_mode() {
        let mut app = App::new();
        app.mode = Mode::Settings;
        let buf = render(&mut app, 80, 30);
        let mut text = String::new();
        for y in 0..30u16 {
            for x in 0..80u16 {
                text.push_str(buf.cell((x, y)).unwrap().symbol());
            }
        }
        assert!(text.contains("Liquid Glass"));
        assert!(text.contains("Radius"));
        assert!(text.contains("Distortion"));
    }

    #[test]
    fn panel_arrows_align_with_hit_zones() {
        use crate::app::{DEC_COL, INC_COL};
        let mut app = App::new();
        app.mode = Mode::Settings;
        let buf = render(&mut app, 80, 30);
        let pr = panel_rect(80, 30);
        // Row of the first parameter:
        let row = pr.y + 1;
        let mut dec_found = false;
        let mut inc_found = false;
        for col in DEC_COL..BAR_COL {
            if buf.cell((pr.x + col, row)).unwrap().symbol() == "◀" {
                dec_found = true;
            }
        }
        for col in INC_COL..(pr.w - 1) {
            if buf.cell((pr.x + col, row)).unwrap().symbol() == "▶" {
                inc_found = true;
            }
        }
        assert!(dec_found, "◀ must sit inside its click zone");
        assert!(inc_found, "▶ must sit inside its click zone");
    }

    #[test]
    fn draw_survives_tiny_terminals() {
        for (w, h) in [(1, 1), (5, 2), (12, 3), (PANEL_W, 4)] {
            let mut app = App::new();
            app.mode = Mode::Settings;
            render(&mut app, w, h);
        }
    }

    #[test]
    fn lens_changes_rendered_output() {
        let mut app = App::new();
        let buf1 = render(&mut app, 80, 30);
        app.lens_x += 14.0;
        let buf2 = render(&mut app, 80, 30);
        let mut diff = 0;
        for y in 1..29u16 {
            for x in 0..80u16 {
                if buf1.cell((x, y)).unwrap() != buf2.cell((x, y)).unwrap() {
                    diff += 1;
                }
            }
        }
        assert!(diff > 10, "moving the lens must change pixels: {diff}");
    }
}
