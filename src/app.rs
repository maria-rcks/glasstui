//! Application state and input handling — pure logic, no terminal I/O,
//! so the whole interaction model is unit-testable.

use crate::glass::{GlassParams, PARAMS};
use crate::scene::Scene;
use crossterm::event::{KeyCode, MouseButton, MouseEventKind};

/// Max delay between two clicks to count as a double-click.
pub const DOUBLE_CLICK_MS: u64 = 450;

// Settings panel geometry (terminal cells). Shared by hit-testing and the UI.
pub const PANEL_W: u16 = 46;
pub const NAME_W: usize = 12;
pub const DEC_COL: u16 = 14; // "◀" zone: [DEC_COL, BAR_COL)
pub const BAR_COL: u16 = 17; // gauge zone: [BAR_COL, BAR_COL + BAR_LEN)
pub const BAR_LEN: u16 = 18;
pub const VAL_COL: u16 = 36; // value text
pub const INC_COL: u16 = 42; // "▶" zone: [INC_COL, PANEL_W - 1)

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    Normal,
    Settings,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PanelRect {
    pub x: u16,
    pub y: u16,
    pub w: u16,
    pub h: u16,
}

impl PanelRect {
    pub fn contains(&self, col: u16, row: u16) -> bool {
        col >= self.x && col < self.x + self.w && row >= self.y && row < self.y + self.h
    }
}

/// Centered settings panel for a terminal of `term_w` x `term_h` cells.
pub fn panel_rect(term_w: u16, term_h: u16) -> PanelRect {
    let w = PANEL_W.min(term_w);
    let h = (PARAMS.len() as u16 + 3).min(term_h);
    PanelRect {
        x: (term_w.saturating_sub(w)) / 2,
        y: (term_h.saturating_sub(h)) / 2,
        w,
        h,
    }
}

pub struct App {
    pub params: GlassParams,
    pub scene: Scene,
    pub mode: Mode,
    pub selected: usize,
    pub quit: bool,
    /// Set when the static scene must be re-rendered (e.g. text rescaled).
    pub scene_dirty: bool,
    /// Lens center in pixel (half-block) coordinates.
    pub lens_x: f32,
    pub lens_y: f32,
    /// Canvas size in pixels; terminal cells are (w, h/2).
    pub canvas_w: usize,
    pub canvas_h: usize,
    dragging: Option<(f32, f32)>,
    last_click: Option<(u64, u16, u16)>,
}

impl Default for App {
    fn default() -> Self {
        Self::new()
    }
}

impl App {
    pub fn new() -> Self {
        Self {
            params: GlassParams::default(),
            scene: Scene::default(),
            mode: Mode::Normal,
            selected: 0,
            quit: false,
            scene_dirty: false,
            lens_x: 0.0,
            lens_y: 0.0,
            canvas_w: 0,
            canvas_h: 0,
            dragging: None,
            last_click: None,
        }
    }

    pub fn is_dragging(&self) -> bool {
        self.dragging.is_some()
    }

    /// Update canvas size (pixels). Centers the lens on first call and keeps
    /// it on-screen across resizes.
    pub fn set_canvas_size(&mut self, w: usize, h: usize) {
        let first = self.canvas_w == 0 && self.canvas_h == 0;
        self.canvas_w = w;
        self.canvas_h = h;
        if first {
            self.lens_x = w as f32 * 0.5;
            self.lens_y = h as f32 * 0.32;
        }
        self.clamp_lens();
    }

    fn clamp_lens(&mut self) {
        if self.canvas_w == 0 || self.canvas_h == 0 {
            return;
        }
        let max_x = (self.canvas_w as f32 - 1.0).max(0.0);
        let max_y = (self.canvas_h as f32 - 1.0).max(0.0);
        self.lens_x = self.lens_x.clamp(0.0, max_x);
        self.lens_y = self.lens_y.clamp(0.0, max_y);
    }

    /// Terminal cell -> pixel coordinates (center of the cell).
    pub fn cell_to_pixel(col: u16, row: u16) -> (f32, f32) {
        (col as f32 + 0.5, row as f32 * 2.0 + 1.0)
    }

    pub fn lens_contains(&self, px: f32, py: f32) -> bool {
        let dx = px - self.lens_x;
        let dy = py - self.lens_y;
        dx * dx + dy * dy <= self.params.radius * self.params.radius
    }

    fn register_click(&mut self, now_ms: u64, col: u16, row: u16) -> bool {
        let double = matches!(
            self.last_click,
            Some((t, c, r))
                if now_ms.saturating_sub(t) <= DOUBLE_CLICK_MS
                    && c.abs_diff(col) <= 1
                    && r.abs_diff(row) <= 1
        );
        // A double-click consumes the state so a triple doesn't fire twice.
        self.last_click = if double {
            None
        } else {
            Some((now_ms, col, row))
        };
        double
    }

    pub fn handle_mouse(&mut self, kind: MouseEventKind, col: u16, row: u16, now_ms: u64) {
        match kind {
            MouseEventKind::Down(MouseButton::Left) => self.mouse_down(col, row, now_ms),
            MouseEventKind::Drag(MouseButton::Left) => {
                if let Some((ox, oy)) = self.dragging {
                    let (px, py) = Self::cell_to_pixel(col, row);
                    self.lens_x = px + ox;
                    self.lens_y = py + oy;
                    self.clamp_lens();
                }
            }
            MouseEventKind::Up(MouseButton::Left) => self.dragging = None,
            MouseEventKind::ScrollUp => self.scroll(1.0),
            MouseEventKind::ScrollDown => self.scroll(-1.0),
            _ => {}
        }
    }

    fn mouse_down(&mut self, col: u16, row: u16, now_ms: u64) {
        let (px, py) = Self::cell_to_pixel(col, row);
        let on_lens = self.lens_contains(px, py);

        if self.mode == Mode::Settings {
            let rect = panel_rect(self.canvas_w as u16, (self.canvas_h / 2) as u16);
            if rect.contains(col, row) {
                self.panel_click(col, row, rect);
                return;
            }
        }

        if self.register_click(now_ms, col, row) && on_lens {
            self.mode = match self.mode {
                Mode::Normal => Mode::Settings,
                Mode::Settings => Mode::Normal,
            };
            self.dragging = None;
            return;
        }

        if on_lens {
            self.dragging = Some((self.lens_x - px, self.lens_y - py));
        } else if self.mode == Mode::Settings {
            self.mode = Mode::Normal;
        }
    }

    fn panel_click(&mut self, col: u16, row: u16, rect: PanelRect) {
        let rel_y = row.saturating_sub(rect.y);
        if rel_y == 0 || rel_y as usize > PARAMS.len() {
            return; // border / footer
        }
        let idx = rel_y as usize - 1;
        self.selected = idx;
        let rel_x = col.saturating_sub(rect.x);
        let spec = &PARAMS[idx];
        if (DEC_COL..BAR_COL).contains(&rel_x) {
            spec.adjust(&mut self.params, -1.0);
        } else if (BAR_COL..BAR_COL + BAR_LEN).contains(&rel_x) {
            let t = (rel_x - BAR_COL) as f32 / (BAR_LEN - 1) as f32;
            spec.set_normalized(&mut self.params, t);
        } else if rel_x >= INC_COL && rel_x < rect.w.saturating_sub(1) {
            spec.adjust(&mut self.params, 1.0);
        }
        self.clamp_lens();
    }

    fn scroll(&mut self, dir: f32) {
        match self.mode {
            Mode::Settings => PARAMS[self.selected].adjust(&mut self.params, dir),
            // Scrolling anywhere in normal mode resizes the lens.
            Mode::Normal => PARAMS[0].adjust(&mut self.params, dir),
        }
        self.clamp_lens();
    }

    pub fn handle_key(&mut self, code: KeyCode) {
        match code {
            KeyCode::Char('q') => self.quit = true,
            KeyCode::Esc => match self.mode {
                Mode::Settings => self.mode = Mode::Normal,
                Mode::Normal => self.quit = true,
            },
            KeyCode::Enter | KeyCode::Tab | KeyCode::Char('s') => {
                self.mode = match self.mode {
                    Mode::Normal => Mode::Settings,
                    Mode::Settings => Mode::Normal,
                };
            }
            KeyCode::Char('r') => self.params = GlassParams::default(),
            KeyCode::Char('+') | KeyCode::Char('=') => {
                self.scene_dirty |= self.scene.adjust_scale(1);
            }
            KeyCode::Char('-') => {
                self.scene_dirty |= self.scene.adjust_scale(-1);
            }
            KeyCode::Up if self.mode == Mode::Settings => {
                self.selected = (self.selected + PARAMS.len() - 1) % PARAMS.len();
            }
            KeyCode::Down if self.mode == Mode::Settings => {
                self.selected = (self.selected + 1) % PARAMS.len();
            }
            KeyCode::Left if self.mode == Mode::Settings => {
                PARAMS[self.selected].adjust(&mut self.params, -1.0);
            }
            KeyCode::Right if self.mode == Mode::Settings => {
                PARAMS[self.selected].adjust(&mut self.params, 1.0);
            }
            KeyCode::Up => self.nudge(0.0, -2.0),
            KeyCode::Down => self.nudge(0.0, 2.0),
            KeyCode::Left => self.nudge(-2.0, 0.0),
            KeyCode::Right => self.nudge(2.0, 0.0),
            _ => {}
        }
    }

    fn nudge(&mut self, dx: f32, dy: f32) {
        self.lens_x += dx;
        self.lens_y += dy;
        self.clamp_lens();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use MouseEventKind::{Down, Drag, ScrollDown, ScrollUp, Up};

    fn app() -> App {
        let mut a = App::new();
        a.set_canvas_size(160, 96); // 160x48 cells
        a
    }

    fn lens_cell(a: &App) -> (u16, u16) {
        (a.lens_x as u16, (a.lens_y / 2.0) as u16)
    }

    #[test]
    fn first_resize_centers_lens() {
        let a = app();
        assert!(a.lens_x > 0.0 && a.lens_y > 0.0);
        assert!(a.lens_x < 160.0 && a.lens_y < 96.0);
    }

    #[test]
    fn drag_moves_lens() {
        let mut a = app();
        let (c, r) = lens_cell(&a);
        a.handle_mouse(Down(MouseButton::Left), c, r, 0);
        assert!(a.is_dragging());
        let (x0, y0) = (a.lens_x, a.lens_y);
        a.handle_mouse(Drag(MouseButton::Left), c + 10, r + 5, 50);
        assert!((a.lens_x - (x0 + 10.0)).abs() < 1e-3);
        assert!((a.lens_y - (y0 + 10.0)).abs() < 1e-3); // 5 cells = 10 px
        a.handle_mouse(Up(MouseButton::Left), c + 10, r + 5, 60);
        assert!(!a.is_dragging());
    }

    #[test]
    fn click_outside_lens_does_not_drag() {
        let mut a = app();
        a.handle_mouse(Down(MouseButton::Left), 0, 0, 0);
        assert!(!a.is_dragging());
    }

    #[test]
    fn drag_clamps_to_canvas() {
        let mut a = app();
        let (c, r) = lens_cell(&a);
        a.handle_mouse(Down(MouseButton::Left), c, r, 0);
        a.handle_mouse(Drag(MouseButton::Left), 500, 500, 10);
        assert!(a.lens_x <= 159.0 && a.lens_y <= 95.0);
    }

    #[test]
    fn double_click_on_lens_opens_then_closes_settings() {
        let mut a = app();
        let (c, r) = lens_cell(&a);
        a.handle_mouse(Down(MouseButton::Left), c, r, 0);
        a.handle_mouse(Down(MouseButton::Left), c, r, 200);
        assert_eq!(a.mode, Mode::Settings);
        assert!(!a.is_dragging());
        // Closing again needs a double-click outside the panel but on the
        // lens; move the lens away from the panel center first.
        a.lens_x = 20.0;
        a.lens_y = 20.0;
        a.handle_mouse(Down(MouseButton::Left), 20, 10, 1000);
        a.handle_mouse(Down(MouseButton::Left), 20, 10, 1100);
        assert_eq!(a.mode, Mode::Normal);
    }

    #[test]
    fn slow_clicks_are_not_double() {
        let mut a = app();
        let (c, r) = lens_cell(&a);
        a.handle_mouse(Down(MouseButton::Left), c, r, 0);
        a.handle_mouse(Down(MouseButton::Left), c, r, DOUBLE_CLICK_MS + 1);
        assert_eq!(a.mode, Mode::Normal);
    }

    #[test]
    fn distant_clicks_are_not_double() {
        let mut a = app();
        let (c, r) = lens_cell(&a);
        a.handle_mouse(Down(MouseButton::Left), c, r, 0);
        a.handle_mouse(Down(MouseButton::Left), c + 8, r, 100);
        assert_eq!(a.mode, Mode::Normal);
    }

    #[test]
    fn triple_click_does_not_retoggle() {
        let mut a = app();
        a.lens_x = 20.0;
        a.lens_y = 20.0;
        a.handle_mouse(Down(MouseButton::Left), 20, 10, 0);
        a.handle_mouse(Down(MouseButton::Left), 20, 10, 100);
        assert_eq!(a.mode, Mode::Settings);
        // Third click within the window must not immediately close it.
        a.handle_mouse(Down(MouseButton::Left), 20, 10, 200);
        assert_eq!(a.mode, Mode::Settings);
    }

    #[test]
    fn click_outside_panel_closes_settings() {
        let mut a = app();
        a.mode = Mode::Settings;
        a.lens_x = 150.0;
        a.lens_y = 90.0;
        a.handle_mouse(Down(MouseButton::Left), 0, 0, 0);
        assert_eq!(a.mode, Mode::Normal);
    }

    #[test]
    fn panel_click_selects_and_adjusts() {
        let mut a = app();
        a.mode = Mode::Settings;
        let rect = panel_rect(160, 48);
        // Click the second param row on the "▶" (increase) zone.
        let before = (PARAMS[1].get)(&a.params);
        a.handle_mouse(Down(MouseButton::Left), rect.x + INC_COL, rect.y + 2, 0);
        assert_eq!(a.selected, 1);
        assert_eq!((PARAMS[1].get)(&a.params), before + PARAMS[1].step);
        assert_eq!(a.mode, Mode::Settings, "panel clicks keep settings open");

        // "◀" zone decreases back.
        a.handle_mouse(Down(MouseButton::Left), rect.x + DEC_COL, rect.y + 2, 10);
        assert_eq!((PARAMS[1].get)(&a.params), before);
    }

    #[test]
    fn panel_bar_click_sets_value_proportionally() {
        let mut a = app();
        a.mode = Mode::Settings;
        let rect = panel_rect(160, 48);
        let spec = &PARAMS[0];
        a.handle_mouse(
            Down(MouseButton::Left),
            rect.x + BAR_COL + BAR_LEN - 1,
            rect.y + 1,
            0,
        );
        assert_eq!((spec.get)(&a.params), spec.max);
        a.handle_mouse(Down(MouseButton::Left), rect.x + BAR_COL, rect.y + 1, 600);
        assert_eq!((spec.get)(&a.params), spec.min);
    }

    #[test]
    fn rapid_panel_clicks_do_not_toggle_mode() {
        let mut a = app();
        a.mode = Mode::Settings;
        let rect = panel_rect(160, 48);
        a.handle_mouse(Down(MouseButton::Left), rect.x + INC_COL, rect.y + 1, 0);
        a.handle_mouse(Down(MouseButton::Left), rect.x + INC_COL, rect.y + 1, 100);
        assert_eq!(a.mode, Mode::Settings);
    }

    #[test]
    fn scroll_resizes_lens_in_normal_mode() {
        let mut a = app();
        let r0 = a.params.radius;
        a.handle_mouse(ScrollUp, 5, 5, 0);
        assert_eq!(a.params.radius, r0 + PARAMS[0].step);
        a.handle_mouse(ScrollDown, 5, 5, 10);
        assert_eq!(a.params.radius, r0);
    }

    #[test]
    fn scroll_adjusts_selected_param_in_settings() {
        let mut a = app();
        a.mode = Mode::Settings;
        a.selected = 2;
        let v0 = (PARAMS[2].get)(&a.params);
        a.handle_mouse(ScrollUp, 5, 5, 0);
        assert!((PARAMS[2].get)(&a.params) > v0);
    }

    #[test]
    fn keyboard_navigation_and_adjust() {
        let mut a = app();
        a.handle_key(KeyCode::Enter);
        assert_eq!(a.mode, Mode::Settings);
        a.handle_key(KeyCode::Down);
        assert_eq!(a.selected, 1);
        a.handle_key(KeyCode::Up);
        a.handle_key(KeyCode::Up);
        assert_eq!(a.selected, PARAMS.len() - 1);
        let v0 = (PARAMS[a.selected].get)(&a.params);
        a.handle_key(KeyCode::Right);
        assert!((PARAMS[a.selected].get)(&a.params) >= v0);
        a.handle_key(KeyCode::Esc);
        assert_eq!(a.mode, Mode::Normal);
        a.handle_key(KeyCode::Esc);
        assert!(a.quit);
    }

    #[test]
    fn q_quits() {
        let mut a = app();
        a.handle_key(KeyCode::Char('q'));
        assert!(a.quit);
    }

    #[test]
    fn r_resets_params() {
        let mut a = app();
        a.params.depth = 33.0;
        a.handle_key(KeyCode::Char('r'));
        assert_eq!(a.params, GlassParams::default());
    }

    #[test]
    fn plus_minus_rescale_text_and_mark_scene_dirty() {
        let mut a = app();
        let s0 = a.scene.lines[0].scale;
        a.handle_key(KeyCode::Char('+'));
        assert_eq!(a.scene.lines[0].scale, s0 + 1);
        assert!(a.scene_dirty);
        a.scene_dirty = false;
        a.handle_key(KeyCode::Char('-'));
        assert_eq!(a.scene.lines[0].scale, s0);
        assert!(a.scene_dirty);
    }

    #[test]
    fn arrows_nudge_lens_in_normal_mode() {
        let mut a = app();
        let x0 = a.lens_x;
        a.handle_key(KeyCode::Right);
        assert_eq!(a.lens_x, x0 + 2.0);
    }

    #[test]
    fn panel_rect_fits_small_terminals() {
        let r = panel_rect(20, 5);
        assert!(r.w <= 20 && r.h <= 5);
        let r = panel_rect(0, 0);
        assert_eq!((r.w, r.h), (0, 0));
    }
}
