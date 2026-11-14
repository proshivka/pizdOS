use std::cell::RefCell;
use std::fs;
use std::process::Command;

use crate::gfx::{Canvas, lerp_rgb};
use crate::sys::MemInfo;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    Ru,
    En,
}

#[inline]
pub fn tr<'a>(lang: Lang, ru: &'a str, en: &'a str) -> &'a str {
    match lang {
        Lang::Ru => ru,
        Lang::En => en,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyEvent {
    Char(char),
    Enter,
    Backspace,
    Escape,
    Up,
    Down,
    Left,
    Right,
    ToggleLauncher,
    ToggleControlCenter,
    ScaleDelta(i8),
}

// ---------------------------------------------------------------------------
// 1. Terminal
// ---------------------------------------------------------------------------
pub struct TerminalState {
    pub lines: Vec<String>,
    pub input: String,
}

impl TerminalState {
    pub fn new(lang: Lang) -> Self {
        Self {
            lines: vec![
                String::from(tr(
                    lang,
                    "pizdOS 2.0 Терминал (0% GNU)",
                    "pizdOS 2.0 Terminal (0% GNU)",
                )),
                String::from(tr(
                    lang,
                    "Введи 'help', 'poyasni' или любую команду:",
                    "Type 'help', 'poyasni' or any shell command:",
                )),
            ],
            input: String::new(),
        }
    }

    pub fn push_line(&mut self, line: String) {
        self.lines.push(line);
        if self.lines.len() > 64 {
            let drain = self.lines.len() - 64;
            self.lines.drain(0..drain);
        }
    }

    pub fn on_key(&mut self, key: KeyEvent, kernel: &str, mem: &MemInfo) {
        match key {
            KeyEvent::Char(c) => {
                if self.input.chars().count() < 56 {
                    self.input.push(c);
                }
            }
            KeyEvent::Backspace => {
                self.input.pop();
            }
            KeyEvent::Enter => {
                let cmd = self.input.trim().to_string();
                self.input.clear();
                self.push_line(format!("pizdOS# {cmd}"));
                self.exec_cmd(&cmd, kernel, mem);
            }
            _ => {}
        }
    }

    fn exec_cmd(&mut self, cmd: &str, kernel: &str, mem: &MemInfo) {
        if cmd.is_empty() {
            return;
        }
        match cmd {
            "help" | "huelp" => {
                self.push_line(String::from("help, poyasni, clear/nahuy, whoami, <cmd>"));
            }
            "clear" | "nahuy" => {
                self.lines.clear();
            }
            "poyasni" => {
                self.push_line(format!("pizdOS 2.0 | {kernel} | RAM {}MB", mem.total_mb));
            }
            "whoami" => {
                self.push_line(String::from("root (bratukha)"));
            }
            _ => {
                #[cfg(windows)]
                let out = Command::new("cmd").args(["/C", cmd]).output();
                #[cfg(not(windows))]
                let out = Command::new("sh").args(["-c", cmd]).output();

                match out {
                    Ok(res) => {
                        let txt = String::from_utf8_lossy(&res.stdout);
                        let err = String::from_utf8_lossy(&res.stderr);
                        let mut any = false;
                        for l in txt.lines().chain(err.lines()).take(12) {
                            self.push_line(l.to_string());
                            any = true;
                        }
                        if !any {
                            self.push_line(format!("[exit {}]", res.status.code().unwrap_or(0)));
                        }
                    }
                    Err(e) => self.push_line(format!("err: {e}")),
                }
            }
        }
    }

    pub fn draw(&self, canvas: &mut Canvas, wx: i32, wy: i32, ww: i32, wh: i32, accent: u32) {
        let x = wx + 10;
        let y = wy + 34;
        let iw = ww - 20;
        let ih = wh - 44;
        canvas.fill_rounded_rect(x, y, iw, ih, 10, 0x090C12);
        canvas.draw_rounded_rect_alpha(x, y, iw, ih, 10, 0xFFFFFF, 24);
        canvas.blend_rect(x + 12, y + 1, (iw - 24).max(0), 1, 0xFFFFFF, 16);

        let max_lines = ((ih - 34) / 15).max(1) as usize;
        let skip = self.lines.len().saturating_sub(max_lines);
        for (i, line) in self.lines.iter().skip(skip).enumerate() {
            let col = if line.starts_with("pizdOS#") { accent } else { 0x6EE7B7 };
            canvas.draw_text_clipped(x + 10, y + 8 + (i as i32) * 15, iw - 20, line, col, 1);
        }

        let prompt_y = y + ih - 24;
        canvas.blend_rounded_rect(x + 4, prompt_y - 2, iw - 8, 22, 7, 0xFFFFFF, 14);
        canvas.draw_rounded_rect_alpha(x + 4, prompt_y - 2, iw - 8, 22, 7, accent, 55);
        canvas.draw_text_clipped(
            x + 10,
            prompt_y + 2,
            iw - 20,
            &format!("pizdOS# {}_", self.input),
            0xF8FAFC,
            1,
        );
    }
}

// ---------------------------------------------------------------------------
// 2. PizPaint
// ---------------------------------------------------------------------------
pub const PAINT_W: usize = 336;
pub const PAINT_H: usize = 180;
pub const PALETTE: [u32; 7] = [
    0xFFFFFF, 0xF85149, 0x3FB950, 0x58A6FF, 0xD29922, 0xBC8CFF, 0x090C10,
];

pub enum PaintAction {
    None,
    Saved(String),
    SaveErr(String),
}

pub struct PaintState {
    pub buf: Vec<u32>,
    pub color_idx: usize,
    pub brush: i32,
    pub eraser: bool,
    pub fill_mode: bool,
    pub brush_tier: u8,
    pub last_pos: Option<(i32, i32)>,
}

impl PaintState {
    pub fn new() -> Self {
        Self {
            buf: vec![0x0D1117; PAINT_W * PAINT_H],
            color_idx: 1,
            brush: 4,
            eraser: false,
            fill_mode: false,
            brush_tier: 1,
            last_pos: None,
        }
    }

    pub fn end_stroke(&mut self) {
        self.last_pos = None;
    }

    fn active_color(&self) -> u32 {
        if self.eraser {
            0x0D1117
        } else {
            PALETTE[self.color_idx]
        }
    }

    fn stamp_brush(&mut self, cx: i32, cy: i32, color: u32) {
        let rf = (self.brush as f32) * 0.5;
        let ri = rf.ceil() as i32 + 1;
        for dy in -ri..=ri {
            for dx in -ri..=ri {
                let px = cx + dx;
                let py = cy + dy;
                if px < 0 || py < 0 || px >= PAINT_W as i32 || py >= PAINT_H as i32 {
                    continue;
                }
                let dist = ((dx * dx + dy * dy) as f32).sqrt() - rf;
                if dist <= -0.35 {
                    self.buf[(py as usize) * PAINT_W + (px as usize)] = color;
                } else if dist < 0.65 {
                    let cov = ((0.65 - dist).clamp(0.0, 1.0) * 255.0).round() as u8;
                    if cov > 8 {
                        let idx = (py as usize) * PAINT_W + (px as usize);
                        self.buf[idx] = crate::gfx::blend_rgb(self.buf[idx], color, cov);
                    }
                }
            }
        }
    }

    fn flood_fill(&mut self, sx: i32, sy: i32, replacement: u32) {
        if sx < 0 || sy < 0 || sx >= PAINT_W as i32 || sy >= PAINT_H as i32 {
            return;
        }
        let target = self.buf[(sy as usize) * PAINT_W + (sx as usize)];
        if target == replacement {
            return;
        }
        let mut stack = vec![(sx, sy)];
        while let Some((x, y)) = stack.pop() {
            if x < 0 || y < 0 || x >= PAINT_W as i32 || y >= PAINT_H as i32 {
                continue;
            }
            let idx = (y as usize) * PAINT_W + (x as usize);
            if self.buf[idx] != target {
                continue;
            }
            self.buf[idx] = replacement;
            stack.push((x + 1, y));
            stack.push((x - 1, y));
            stack.push((x, y + 1));
            stack.push((x, y - 1));
        }
    }

    pub fn paint_at(&mut self, lx: i32, ly: i32) {
        let cx = lx - 14;
        let cy = ly - 36;
        if cx < 0 || cy < 0 || cx >= PAINT_W as i32 || cy >= PAINT_H as i32 {
            self.last_pos = None;
            return;
        }
        let color = self.active_color();
        if self.fill_mode {
            if self.last_pos.is_none() {
                self.flood_fill(cx, cy, color);
                self.last_pos = Some((cx, cy));
            }
            return;
        }
        if let Some((x0, y0)) = self.last_pos {
            let dx = cx - x0;
            let dy = cy - y0;
            let steps = (dx.abs().max(dy.abs()) * 2).max(1);
            for i in 1..=steps {
                let px = x0 + (dx * i) / steps;
                let py = y0 + (dy * i) / steps;
                self.stamp_brush(px, py, color);
            }
        } else {
            self.stamp_brush(cx, cy, color);
        }
        self.last_pos = Some((cx, cy));
    }

    pub fn save_bmp(&self) -> Result<String, String> {
        let mut c = Canvas::new(PAINT_W, PAINT_H);
        c.pixels.copy_from_slice(&self.buf);
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let fname = format!("pizpaint_{ts}.bmp");
        let path = std::env::temp_dir().join(&fname);
        c.write_bmp(&path)
            .map(|()| fname)
            .map_err(|e| e.to_string())
    }

    pub fn on_click(&mut self, lx: i32, ly: i32, wh: i32) -> PaintAction {
        if ly >= wh - 38 && ly <= wh - 8 {
            for i in 0..PALETTE.len() {
                let bx = 14 + (i as i32) * 20;
                if lx >= bx && lx <= bx + 18 {
                    self.color_idx = i;
                    self.eraser = false;
                    return PaintAction::None;
                }
            }
            if (156..=186).contains(&lx) {
                self.brush_tier = (self.brush_tier + 1) % 3;
                self.brush = match self.brush_tier {
                    0 => 2,
                    1 => 4,
                    _ => 7,
                };
                return PaintAction::None;
            }
            if (190..=236).contains(&lx) {
                self.eraser = !self.eraser;
                if self.eraser {
                    self.fill_mode = false;
                }
                return PaintAction::None;
            }
            if (240..=280).contains(&lx) {
                self.fill_mode = !self.fill_mode;
                if self.fill_mode {
                    self.eraser = false;
                }
                return PaintAction::None;
            }
            if (284..=318).contains(&lx) {
                return match self.save_bmp() {
                    Ok(f) => PaintAction::Saved(f),
                    Err(e) => PaintAction::SaveErr(e),
                };
            }
            if (322..=362).contains(&lx) {
                self.buf.fill(0x0D1117);
                return PaintAction::None;
            }
        }
        self.paint_at(lx, ly);
        PaintAction::None
    }

    #[allow(clippy::too_many_arguments)]
    pub fn draw(
        &self,
        canvas: &mut Canvas,
        wx: i32,
        wy: i32,
        _ww: i32,
        wh: i32,
        lang: Lang,
        accent: u32,
        hover_lx: i32,
        hover_ly: i32,
    ) {
        let ox = wx + 14;
        let oy = wy + 36;
        canvas.blit_rgb_rect(ox, oy, PAINT_W as i32, PAINT_H as i32, &self.buf);
        canvas.draw_rounded_rect_alpha(ox - 1, oy - 1, PAINT_W as i32 + 2, PAINT_H as i32 + 2, 6, 0xFFFFFF, 32);

        // Live brush ring preview over canvas
        let cx = hover_lx - 14;
        let cy = hover_ly - 36;
        if cx >= 0 && cy >= 0 && cx < PAINT_W as i32 && cy < PAINT_H as i32 {
            let preview_col = if self.eraser { 0xF87171 } else { PALETTE[self.color_idx] };
            let r = (self.brush as f32 * 0.5).max(1.5);
            canvas.fill_aa_circle((wx + hover_lx) as f32, (wy + hover_ly) as f32, r + 1.0, 0xFFFFFF, 90);
            canvas.fill_aa_circle((wx + hover_lx) as f32, (wy + hover_ly) as f32, r, preview_col, 145);
        }

        let by = wy + wh - 34;
        for (i, &col) in PALETTE.iter().enumerate() {
            let cx_p = (wx + 23 + (i as i32) * 20) as f32;
            let cy_p = (by + 11) as f32;
            let sel = !self.eraser && i == self.color_idx;
            if sel {
                canvas.fill_aa_circle(cx_p, cy_p, 8.8, 0xFFFFFF, 225);
                canvas.fill_aa_circle(cx_p, cy_p, 7.2, 0x131720, 255);
            }
            canvas.fill_aa_circle(cx_p, cy_p, 6.0, col, 255);
        }
        let b_lbl = match self.brush_tier {
            0 => "S",
            1 => "M",
            _ => "L",
        };
        canvas.draw_button(wx + 156, by, 30, 22, b_lbl, 0x21262D, 0xFFFFFF, accent);
        let erase_bg = if self.eraser { accent } else { 0x21262D };
        canvas.draw_button(
            wx + 190,
            by,
            46,
            22,
            tr(lang, "Ласт", "Eras"),
            erase_bg,
            0xFFFFFF,
            if self.eraser { 0xFFFFFF } else { 0x475569 },
        );
        let fill_bg = if self.fill_mode { accent } else { 0x21262D };
        canvas.draw_button(
            wx + 240,
            by,
            40,
            22,
            tr(lang, "Зал", "Fill"),
            fill_bg,
            0xFFFFFF,
            if self.fill_mode { 0xFFFFFF } else { 0x475569 },
        );
        canvas.draw_button(wx + 284, by, 34, 22, "BMP", 0x1E293B, 0x93C5FD, 0x38BDF8);
        canvas.draw_button(
            wx + 322,
            by,
            40,
            22,
            tr(lang, "Сбр", "Clr"),
            0x9F1239,
            0xFFFFFF,
            0xFB7185,
        );
    }
}

// ---------------------------------------------------------------------------
// 3. Pizmeyka / Piznake
// ---------------------------------------------------------------------------
const SNAKE_COLS: i32 = 20;
const SNAKE_ROWS: i32 = 14;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnakeEvent {
    None,
    AteBonus,
    NewBest(u32),
    GameOver,
}

pub struct SnakeState {
    pub body: Vec<(i32, i32)>,
    pub prev_body: Vec<(i32, i32)>,
    pub dir: (i32, i32),
    pub food: (i32, i32),
    pub bonus_food: Option<(i32, i32, u16)>,
    pub score: u32,
    pub best_score: u32,
    pub wrap_walls: bool,
    pub move_t: f32,
    pub game_over: bool,
    pub new_best_notified: bool,
    rng: u64,
}

impl SnakeState {
    pub fn new() -> Self {
        let mut s = Self {
            body: Vec::new(),
            prev_body: Vec::new(),
            dir: (1, 0),
            food: (12, 7),
            bonus_food: None,
            score: 0,
            best_score: 0,
            wrap_walls: true,
            move_t: 1.0,
            game_over: false,
            new_best_notified: false,
            rng: 0x1337_BEEF_CAFE_0001,
        };
        s.reset();
        s
    }

    pub fn reset(&mut self) {
        self.body = vec![(6, 7), (5, 7), (4, 7)];
        self.prev_body = self.body.clone();
        self.dir = (1, 0);
        self.food = (14, 7);
        self.bonus_food = None;
        self.score = 0;
        self.move_t = 1.0;
        self.game_over = false;
        self.new_best_notified = false;
    }

    fn next_rand(&mut self) -> i32 {
        let mut x = self.rng;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.rng = x;
        (x & 0x7FFF) as i32
    }

    fn spawn_free_cell(&mut self) -> (i32, i32) {
        for _ in 0..32 {
            let fx = self.next_rand() % SNAKE_COLS;
            let fy = self.next_rand() % SNAKE_ROWS;
            if !self.body.contains(&(fx, fy)) {
                return (fx, fy);
            }
        }
        (0, 0)
    }

    pub fn on_key(&mut self, key: KeyEvent) {
        let nd = match key {
            KeyEvent::Up | KeyEvent::Char('w') | KeyEvent::Char('W') => (0, -1),
            KeyEvent::Down | KeyEvent::Char('s') | KeyEvent::Char('S') => (0, 1),
            KeyEvent::Left | KeyEvent::Char('a') | KeyEvent::Char('A') => (-1, 0),
            KeyEvent::Right | KeyEvent::Char('d') | KeyEvent::Char('D') => (1, 0),
            KeyEvent::Char('r') | KeyEvent::Enter if self.game_over => {
                self.reset();
                return;
            }
            _ => return,
        };
        if nd.0 + self.dir.0 != 0 || nd.1 + self.dir.1 != 0 {
            self.dir = nd;
        }
    }

    pub fn step_interp(&mut self, div: u64, animated: bool) {
        if !animated || self.game_over {
            self.move_t = 1.0;
            return;
        }
        let inc = 1.0 / (div.max(1) as f32);
        self.move_t = (self.move_t + inc).min(1.0);
    }

    pub fn step(&mut self) -> SnakeEvent {
        if self.game_over || self.body.is_empty() {
            return SnakeEvent::None;
        }
        self.prev_body = self.body.clone();
        self.move_t = 0.0;

        if let Some((bx, by, ttl)) = self.bonus_food {
            if ttl <= 1 {
                self.bonus_food = None;
            } else {
                self.bonus_food = Some((bx, by, ttl - 1));
            }
        }

        let head = self.body[0];
        let mut nx = head.0 + self.dir.0;
        let mut ny = head.1 + self.dir.1;
        if self.wrap_walls {
            nx = nx.rem_euclid(SNAKE_COLS);
            ny = ny.rem_euclid(SNAKE_ROWS);
        } else if nx < 0 || ny < 0 || nx >= SNAKE_COLS || ny >= SNAKE_ROWS {
            self.game_over = true;
            return SnakeEvent::GameOver;
        }
        if self.body.contains(&(nx, ny)) {
            self.game_over = true;
            return SnakeEvent::GameOver;
        }

        let mut ev = SnakeEvent::None;
        self.body.insert(0, (nx, ny));
        if (nx, ny) == self.food {
            self.score += 10;
            self.food = self.spawn_free_cell();
            if self.score.is_multiple_of(40) && self.bonus_food.is_none() {
                let (bx, by) = self.spawn_free_cell();
                self.bonus_food = Some((bx, by, 45));
            }
        } else if let Some((bx, by, _)) = self.bonus_food
            && (nx, ny) == (bx, by)
        {
            self.score += 30;
            self.bonus_food = None;
            ev = SnakeEvent::AteBonus;
        } else {
            self.body.pop();
        }

        if self.score > self.best_score {
            let had_prev = self.best_score > 0;
            self.best_score = self.score;
            if had_prev && !self.new_best_notified {
                self.new_best_notified = true;
                return SnakeEvent::NewBest(self.best_score);
            }
        }
        ev
    }

    pub fn on_click(&mut self, lx: i32, ly: i32, wh: i32) {
        if ly >= wh - 36 && ly <= wh - 8 {
            if (14..=104).contains(&lx) {
                self.reset();
            } else if (110..=186).contains(&lx) {
                self.wrap_walls = !self.wrap_walls;
            }
        }
    }

    pub fn draw(
        &self,
        canvas: &mut Canvas,
        wx: i32,
        wy: i32,
        wh: i32,
        lang: Lang,
        tick: u64,
    ) {
        let gx = wx + 18;
        let gy = wy + 36;
        let cell = 14;
        let bw = SNAKE_COLS * cell;
        let bh = SNAKE_ROWS * cell;
        canvas.fill_rounded_rect(gx, gy, bw, bh, 10, 0x090D14);
        canvas.draw_rounded_rect_alpha(gx - 1, gy - 1, bw + 2, bh + 2, 10, 0xFFFFFF, 30);

        // Subtle dot matrix grid on board
        for ry in 1..SNAKE_ROWS {
            for rx in 1..SNAKE_COLS {
                if (rx + ry) % 2 == 0 {
                    canvas.blend_pixel(gx + rx * cell, gy + ry * cell, 0x475569, 40);
                }
            }
        }

        // Glowing food orb
        let pulse = 0.88 + 0.12 * ((tick as f32) * 0.16).sin();
        let fx = gx as f32 + self.food.0 as f32 * cell as f32 + 7.0;
        let fy = gy as f32 + self.food.1 as f32 * cell as f32 + 7.0;
        canvas.fill_aa_circle(fx, fy, 7.5 * pulse, 0xF43F5E, 48);
        canvas.fill_aa_circle(fx, fy, 4.8 * pulse, 0xFB7185, 255);
        canvas.fill_aa_circle(fx - 1.3, fy - 1.3, 1.5, 0xFFFFFF, 190);

        // Golden bonus apple orb
        if let Some((bx, by, _)) = self.bonus_food {
            let bpx = gx as f32 + bx as f32 * cell as f32 + 7.0;
            let bpy = gy as f32 + by as f32 * cell as f32 + 7.0;
            let gp = 0.85 + 0.15 * ((tick as f32) * 0.25).cos();
            canvas.fill_aa_circle(bpx, bpy, 8.5 * gp, 0xF59E0B, 68);
            canvas.fill_aa_circle(bpx, bpy, 5.2 * gp, 0xFBBF24, 255);
            canvas.fill_aa_circle(bpx - 1.2, bpy - 1.2, 1.6, 0xFFFFFF, 220);
        }

        // Smoothly interpolated snake segments + connected joints
        let t = self.move_t.clamp(0.0, 1.0);
        let len = self.body.len();
        for (i, &(sx, sy)) in self.body.iter().enumerate().rev() {
            let (px, py) = self.prev_body.get(i).copied().unwrap_or((sx, sy));
            let dx = sx - px;
            let dy = sy - py;
            let (ix, iy) = if dx.abs() <= 1 && dy.abs() <= 1 {
                (px as f32 + (dx as f32) * t, py as f32 + (dy as f32) * t)
            } else {
                (sx as f32, sy as f32)
            };
            let cx = gx as f32 + ix * cell as f32 + 7.0;
            let cy = gy as f32 + iy * cell as f32 + 7.0;
            let radius = if i == 0 {
                6.3
            } else if i + 1 == len {
                4.2
            } else {
                5.4
            };
            let col = if i == 0 {
                0x4ADE80
            } else {
                lerp_rgb(0x22C55E, 0x15803D, i as u32, len.max(2) as u32)
            };
            canvas.fill_aa_circle(cx, cy, radius, col, 255);
            if i == 0 {
                // Cute eyes facing current direction
                let (ex1, ey1, ex2, ey2) = match self.dir {
                    (1, 0) => (cx + 2.2, cy - 2.3, cx + 2.2, cy + 2.3),
                    (-1, 0) => (cx - 2.2, cy - 2.3, cx - 2.2, cy + 2.3),
                    (0, -1) => (cx - 2.3, cy - 2.2, cx + 2.3, cy - 2.2),
                    _ => (cx - 2.3, cy + 2.2, cx + 2.3, cy + 2.2),
                };
                canvas.fill_aa_circle(ex1, ey1, 1.4, 0x090D14, 255);
                canvas.fill_aa_circle(ex2, ey2, 1.4, 0x090D14, 255);
            }
        }

        if self.game_over {
            canvas.blend_rounded_rect(gx + 34, gy + 74, bw - 68, 42, 10, 0x0B0F1A, 225);
            canvas.draw_rounded_rect_alpha(gx + 34, gy + 74, bw - 68, 42, 10, 0xFB7185, 165);
            canvas.draw_text(
                gx + 54,
                gy + 88,
                tr(lang, "ПИЗДЕЦ! ИГРА ОКОНЧЕНА", "GAME OVER!"),
                0xFDA4AF,
                1,
            );
        }

        let by = wy + wh - 34;
        canvas.draw_button(
            wx + 14,
            by,
            90,
            22,
            tr(lang, "Заново", "New Game"),
            0x166534,
            0xFFFFFF,
            0x4ADE80,
        );
        let mode_lbl = if self.wrap_walls {
            tr(lang, "Портал", "Portal")
        } else {
            tr(lang, "Стены", "Walls")
        };
        canvas.draw_button(
            wx + 110,
            by,
            76,
            22,
            mode_lbl,
            if self.wrap_walls { 0x1E3A8A } else { 0x21262D },
            0xE2E8F0,
            if self.wrap_walls { 0x38BDF8 } else { 0x475569 },
        );
        let score_lbl = match lang {
            Lang::Ru => format!("{} (Р:{})", self.score, self.best_score),
            Lang::En => format!("{} (B:{})", self.score, self.best_score),
        };
        canvas.draw_text_clipped(wx + 194, by + 4, 120, &score_lbl, 0xE6EDF3, 1);
    }
}

// ---------------------------------------------------------------------------
// 4. PizPad (Funny Notepad)
// ---------------------------------------------------------------------------
pub struct NotepadState {
    pub lines: Vec<String>,
    pub saved_msg: String,
}

impl NotepadState {
    pub fn new(lang: Lang) -> Self {
        Self {
            lines: vec![
                String::from("=== PIZPAD v0.2 (Top Secret) ==="),
                String::from(tr(
                    lang,
                    "1. Не трогай то, что работает.",
                    "1. Never touch what already works.",
                )),
                String::from(tr(
                    lang,
                    "2. Если не работает - это фича.",
                    "2. If it breaks - it's a feature.",
                )),
                String::from(tr(lang, "3. GNU пошёл нахуй!", "3. Zero GNU, zero bugs!")),
                String::new(),
            ],
            saved_msg: String::from(tr(lang, "Печатай прямо сюда...", "Type right here...")),
        }
    }

    pub fn on_key(&mut self, key: KeyEvent) {
        match key {
            KeyEvent::Char(c) => {
                if self.lines.is_empty() {
                    self.lines.push(String::new());
                }
                if let Some(last) = self.lines.last_mut()
                    && last.chars().count() < 50
                {
                    last.push(c);
                }
            }
            KeyEvent::Enter => {
                if self.lines.len() < 14 {
                    self.lines.push(String::new());
                }
            }
            KeyEvent::Backspace => {
                if let Some(last) = self.lines.last_mut()
                    && last.pop().is_none()
                    && self.lines.len() > 1
                {
                    self.lines.pop();
                }
            }
            _ => {}
        }
    }

    pub fn on_click(&mut self, lx: i32, ly: i32, wh: i32, lang: Lang) -> Option<String> {
        if ly >= wh - 34 && ly <= wh - 10 {
            if (14..=114).contains(&lx) {
                let text = self.lines.join("\n");
                let path = std::env::temp_dir().join("pizpad_note.txt");
                match fs::write(&path, text) {
                    Ok(()) => {
                        let msg = tr(lang, "Сохранено в pizpad_note.txt", "Saved to pizpad_note.txt");
                        self.saved_msg = String::from(msg);
                        return Some(String::from(msg));
                    }
                    Err(e) => self.saved_msg = format!("Err: {e}"),
                }
            } else if (122..=212).contains(&lx) {
                self.lines = vec![String::new()];
                self.saved_msg = String::from(tr(lang, "Очищено!", "Cleared!"));
            }
        }
        None
    }

    pub fn draw(&self, canvas: &mut Canvas, wx: i32, wy: i32, ww: i32, wh: i32, lang: Lang) {
        let x = wx + 12;
        let y = wy + 34;
        let iw = ww - 24;
        let ih = wh - 76;
        canvas.fill_rounded_rect(x, y, iw, ih, 10, 0x0B0E15);
        canvas.draw_rounded_rect_alpha(x, y, iw, ih, 10, 0xFFFFFF, 24);
        canvas.blend_rect(x + 12, y + 1, (iw - 24).max(0), 1, 0xFFFFFF, 16);

        let max_lines = ((ih - 8) / 15).max(1) as usize;
        for (i, line) in self.lines.iter().take(max_lines).enumerate() {
            let cursor = if i + 1 == self.lines.len() { "_" } else { "" };
            canvas.draw_text_clipped(
                x + 10,
                y + 8 + (i as i32) * 15,
                iw - 20,
                &format!("{line}{cursor}"),
                0xE6EDF3,
                1,
            );
        }

        let by = wy + wh - 34;
        canvas.draw_button(
            wx + 14,
            by,
            100,
            22,
            tr(lang, "Сохранить", "Save"),
            0x238636,
            0xFFFFFF,
            0x3FB950,
        );
        canvas.draw_button(
            wx + 122,
            by,
            90,
            22,
            tr(lang, "Очистить", "Clear"),
            0x21262D,
            0xFFFFFF,
            0x58A6FF,
        );
        canvas.draw_text_clipped(wx + 220, by + 5, ww - 230, &self.saved_msg, 0x8B949E, 1);
    }
}

// ---------------------------------------------------------------------------
// 5. Wallpapers & Full Settings App (with ColorOS-style About OS)
// ---------------------------------------------------------------------------
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsPage {
    Display,
    Taskbar,
    System,
    About,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThemePreset {
    AuroraPeaks,
    CrimsonSunset,
    MatrixGlow,
    OceanGlass,
}

impl ThemePreset {
    pub fn name(self, lang: Lang) -> &'static str {
        match (self, lang) {
            (ThemePreset::AuroraPeaks, Lang::Ru) => "Северное сияние",
            (ThemePreset::AuroraPeaks, Lang::En) => "Aurora Peaks",
            (ThemePreset::CrimsonSunset, Lang::Ru) => "Багровый закат",
            (ThemePreset::CrimsonSunset, Lang::En) => "Crimson Sunset",
            (ThemePreset::MatrixGlow, Lang::Ru) => "Кибер Матрица",
            (ThemePreset::MatrixGlow, Lang::En) => "Matrix Glow",
            (ThemePreset::OceanGlass, Lang::Ru) => "Океан ColorOS",
            (ThemePreset::OceanGlass, Lang::En) => "Ocean Glass",
        }
    }

    pub fn accent(self) -> u32 {
        match self {
            ThemePreset::AuroraPeaks => 0x388BFD,
            ThemePreset::CrimsonSunset => 0xF85149,
            ThemePreset::MatrixGlow => 0x2EA043,
            ThemePreset::OceanGlass => 0x2DD4BF,
        }
    }

    pub fn mesh_palette(self) -> (u32, u32, u32, u32, u32, u32) {
        // Minimalist silk mesh gradient palettes: base_tl, base_br, blob_a, blob_b, blob_c, ribbon
        match self {
            ThemePreset::AuroraPeaks => (
                0x07090F, 0x0C1220, 0x1E3A8A, 0x0F766E, 0x4338CA, 0x38BDF8,
            ),
            ThemePreset::CrimsonSunset => (
                0x0C070C, 0x1A0B14, 0x9F1239, 0xB45309, 0x581C87, 0xFB7185,
            ),
            ThemePreset::MatrixGlow => (
                0x050A07, 0x08170E, 0x065F46, 0x15803D, 0x0F766E, 0x34D399,
            ),
            ThemePreset::OceanGlass => (
                0x050B12, 0x0A1A26, 0x0369A1, 0x0D9488, 0x1D4ED8, 0x2DD4BF,
            ),
        }
    }

    pub fn render_wallpaper(self, buf: &mut [u32], w: usize, h: usize, show_grid: bool) {
        if w == 0 || h == 0 {
            return;
        }
        let (base_tl, base_br, blob_a, blob_b, blob_c, ribbon) = self.mesh_palette();

        let split_rgb = |c: u32| -> (f32, f32, f32) {
            (((c >> 16) & 0xFF) as f32, ((c >> 8) & 0xFF) as f32, (c & 0xFF) as f32)
        };
        let (tl_r, tl_g, tl_b) = split_rgb(base_tl);
        let (br_r, br_g, br_b) = split_rgb(base_br);
        let (ar, ag, ab) = split_rgb(blob_a);
        let (br2, bg2, bb2) = split_rgb(blob_b);
        let (cr, cg, cb) = split_rgb(blob_c);
        let (rr, rg, rb) = split_rgb(ribbon);

        let inv_w = 1.0 / (w as f32).max(1.0);
        let inv_h = 1.0 / (h as f32).max(1.0);

        for y in 0..h {
            let ny = y as f32 * inv_h;
            let row_off = y * w;

            for x in 0..w {
                let nx = x as f32 * inv_w;

                // Diagonal calm dark ambient base
                let diag = (nx * 0.55 + ny * 0.65).clamp(0.0, 1.0);
                let mut r = tl_r + (br_r - tl_r) * diag;
                let mut g = tl_g + (br_g - tl_g) * diag;
                let mut b = tl_b + (br_b - tl_b) * diag;

                // 3 large ultra-soft organic radial mesh clouds (cubic falloff = zero hard edges)
                let d1 = ((nx - 0.78) * 1.15).powi(2) + ((ny - 0.24) * 1.35).powi(2);
                if d1 < 1.0 {
                    let k = 1.0 - d1;
                    let w1 = k * k * k * 0.44;
                    r += (ar - r) * w1;
                    g += (ag - g) * w1;
                    b += (ab - b) * w1;
                }

                let d2 = ((nx - 0.22) * 1.1).powi(2) + ((ny - 0.72) * 1.3).powi(2);
                if d2 < 1.0 {
                    let k = 1.0 - d2;
                    let w2 = k * k * k * 0.36;
                    r += (br2 - r) * w2;
                    g += (bg2 - g) * w2;
                    b += (bb2 - b) * w2;
                }

                let d3 = ((nx - 0.52) * 1.35).powi(2) + ((ny - 0.44) * 1.55).powi(2);
                if d3 < 1.0 {
                    let k = 1.0 - d3;
                    let w3 = k * k * k * 0.26;
                    r += (cr - r) * w3;
                    g += (cg - g) * w3;
                    b += (cb - b) * w3;
                }

                // Flowing minimalist translucent silk waves
                let wave1 = 0.54 + 0.14 * (nx * 4.2 + 0.5).sin() + 0.04 * (nx * 8.6).cos();
                let dist1 = (ny - wave1).abs() * 7.5;
                if dist1 < 1.0 {
                    let k = 1.0 - dist1 * dist1;
                    let wr = k * k * k * (0.14 + 0.06 * (nx * 3.1).cos());
                    r += (rr - r) * wr;
                    g += (rg - g) * wr;
                    b += (rb - b) * wr;
                }

                let wave2 = 0.62 - 0.11 * (nx * 3.5 + 1.4).sin();
                let dist2 = (ny - wave2).abs() * 6.0;
                if dist2 < 1.0 {
                    let k = 1.0 - dist2 * dist2;
                    let wr = k * k * k * 0.08;
                    r += (ar - r) * wr;
                    g += (ag - g) * wr;
                    b += (ab - b) * wr;
                }

                // Subtle edge vignette for minimalist focus
                let vx = (nx - 0.5) * 1.4;
                let vy = (ny - 0.5) * 1.4;
                let vig = (1.0 - (vx * vx + vy * vy) * 0.16).clamp(0.75, 1.0);
                r *= vig;
                g *= vig;
                b *= vig;

                // Subtle optional minimalist dot matrix
                if show_grid && x % 32 == 0 && y % 32 == 0 {
                    r += (255.0 - r) * 0.12;
                    g += (255.0 - g) * 0.12;
                    b += (255.0 - b) * 0.12;
                }

                // Micro film dither to eliminate 8-bit gradient banding completely
                let mut hsh = ((x as u32).wrapping_mul(374761393)) ^ ((y as u32).wrapping_mul(668265263));
                hsh ^= hsh >> 13;
                hsh = hsh.wrapping_mul(1274126177);
                let dither = (((hsh >> 24) & 3) as f32) * 0.35 - 0.52;

                let ur = (r + dither).clamp(0.0, 255.0) as u32;
                let ug = (g + dither).clamp(0.0, 255.0) as u32;
                let ub = (b + dither).clamp(0.0, 255.0) as u32;
                buf[row_off + x] = (ur << 16) | (ug << 8) | ub;
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DisplayPicker {
    Theme,
    Lang,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsAction {
    None,
    RefreshMetrics,
    OpenTerminal,
    OpenPizGet,
}

pub const ACCENT_PALETTE: [u32; 5] = [
    0x388BFD, // Cobalt
    0x2DD4BF, // Mint
    0xA855F7, // Orchid
    0xF43F5E, // Coral
    0xF59E0B, // Amber
];

struct HeroCache {
    pw: i32,
    ph: i32,
    mw: i32,
    mh: i32,
    pr: i32,
    mr: i32,
    theme: ThemePreset,
    accent: u32,
    hero: Vec<u32>,
    mini: Vec<u32>,
}

pub const UI_SCALES: [u16; 4] = [100, 125, 150, 175];

pub struct SettingsState {
    pub page: SettingsPage,
    pub lang: Lang,
    pub theme: ThemePreset,
    pub accent_idx: usize,
    pub show_grid: bool,
    pub show_icons: bool,
    pub taskbar_top: bool,
    pub floating_dock: bool,
    pub animations: bool,
    pub genie_minimize: bool,
    pub glass_level: u8,
    pub ui_scale_pct: u16,
    pub crosshair_cursor: bool,
    pub show_clock: bool,
    pub snake_tick_div: u64,
    pub device_name: String,
    pub editing_device: bool,
    pub update_taps: u8,
    pub license_alt: bool,
    pub tab_pill_y: f32,
    pub tab_pill_vel: f32,
    pub tab_fade: f32,
    pub toggle_anim: [f32; 8],
    pub display_picker: Option<DisplayPicker>,
    pub picker_anim: f32,
    pub last_picker: Option<DisplayPicker>,
    hero_cache: RefCell<Option<HeroCache>>,
}

impl SettingsState {
    pub fn new() -> Self {
        Self {
            page: SettingsPage::About,
            lang: Lang::Ru,
            theme: ThemePreset::AuroraPeaks,
            accent_idx: 0,
            show_grid: false,
            show_icons: true,
            taskbar_top: false,
            floating_dock: false,
            animations: true,
            genie_minimize: true,
            glass_level: 0,
            ui_scale_pct: 100,
            crosshair_cursor: false,
            show_clock: true,
            snake_tick_div: 10,
            device_name: String::from("pizdOS PC"),
            editing_device: false,
            update_taps: 0,
            license_alt: false,
            tab_pill_y: 90.0,
            tab_pill_vel: 0.0,
            tab_fade: 1.0,
            toggle_anim: [0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 1.0, 1.0],
            display_picker: None,
            picker_anim: 0.0,
            last_picker: None,
            hero_cache: RefCell::new(None),
        }
    }

    #[inline]
    pub fn ui_scale(&self) -> f32 {
        (self.ui_scale_pct as f32 / 100.0).clamp(0.75, 2.5)
    }

    pub fn step_ui_scale(&mut self, delta: i8) {
        let cur = UI_SCALES
            .iter()
            .position(|&s| s == self.ui_scale_pct)
            .unwrap_or(0) as i32;
        let next = (cur + delta as i32).clamp(0, UI_SCALES.len() as i32 - 1) as usize;
        self.ui_scale_pct = UI_SCALES[next];
    }

    #[inline]
    pub fn active_accent(&self) -> u32 {
        if self.accent_idx == 0 {
            self.theme.accent()
        } else {
            ACCENT_PALETTE
                .get(self.accent_idx - 1)
                .copied()
                .unwrap_or_else(|| self.theme.accent())
        }
    }

    /// Returns `(panel_alpha, border_alpha_boost)` for the selected glass density.
    #[inline]
    pub fn glass_alphas(&self) -> (u8, u8) {
        match self.glass_level {
            1 => (182, 42), // Frosted / Airy Glass
            2 => (246, 18), // Dense / Solid Dark
            _ => (228, 28), // Standard Liquid Glass
        }
    }

    #[inline]
    fn page_index(&self) -> usize {
        match self.page {
            SettingsPage::Display => 0,
            SettingsPage::Taskbar => 1,
            SettingsPage::System => 2,
            SettingsPage::About => 3,
        }
    }

    pub fn step_anim(&mut self) {
        let target_pill = (self.page_index() as f32) * 30.0;
        let targets = [
            self.show_grid,
            self.show_icons,
            self.taskbar_top,
            self.floating_dock,
            self.animations,
            self.crosshair_cursor,
            self.show_clock,
            self.genie_minimize,
        ];
        let p_tgt = if self.display_picker.is_some() { 1.0 } else { 0.0 };
        if !self.animations {
            self.tab_pill_y = target_pill;
            self.tab_pill_vel = 0.0;
            self.tab_fade = 1.0;
            self.picker_anim = p_tgt;
            if self.display_picker.is_none() {
                self.last_picker = None;
            }
            for (slot, &on) in self.toggle_anim.iter_mut().zip(targets.iter()) {
                *slot = if on { 1.0 } else { 0.0 };
            }
            return;
        }
        let pd = p_tgt - self.picker_anim;
        if pd.abs() < 0.015 {
            self.picker_anim = p_tgt;
            if self.display_picker.is_none() {
                self.last_picker = None;
            }
        } else {
            self.picker_anim += pd * 0.34;
        }
        if self.tab_fade < 1.0 {
            self.tab_fade = (self.tab_fade + 0.14).min(1.0);
        }
        Canvas::step_spring(
            &mut self.tab_pill_y,
            &mut self.tab_pill_vel,
            target_pill,
            0.26,
            0.66,
            0.18,
        );
        for (slot, &on) in self.toggle_anim.iter_mut().zip(targets.iter()) {
            let tgt = if on { 1.0 } else { 0.0 };
            let d = tgt - *slot;
            if d.abs() < 0.012 {
                *slot = tgt;
            } else {
                *slot += d * 0.32;
            }
        }
    }

    fn blit_cached_hero(
        &self,
        canvas: &mut Canvas,
        hero_rect: (i32, i32, i32),
        scr_pos: (i32, i32),
        accent: u32,
    ) {
        let (x, y, iw) = hero_rect;
        let (scr_x, scr_y) = scr_pos;
        let cw = iw.clamp(16, 1200);
        let (hx, pw) = canvas.span(x, cw);
        let (hy, ph) = canvas.span(y, 90);
        let (mx, mw) = canvas.span(scr_x, 32);
        let (my, mh) = canvas.span(scr_y, 52);
        let pr = canvas.sx(14);
        let mr = canvas.sx(6);
        let mut cache_ref = self.hero_cache.borrow_mut();
        let valid = cache_ref.as_ref().is_some_and(|c| {
            c.pw == pw && c.ph == ph && c.mw == mw && c.mh == mh && c.theme == self.theme && c.accent == accent
        });
        if !valid && pw > 0 && ph > 0 && mw > 0 && mh > 0 {
            let (wp_tl, wp_br, wp_a, wp_b, _, wp_rib) = self.theme.mesh_palette();
            let mut hc = Canvas::new(pw as usize, ph as usize);
            hc.pixels.fill(0x131720);
            hc.fill_luminous_card_phys(
                0,
                0,
                pw,
                ph,
                pr,
                0x101521,
                lerp_rgb(0x161E30, accent, 52, 255),
                accent,
                wp_rib,
            );
            let mut mc = Canvas::new(mw as usize, mh as usize);
            mc.pixels.fill(0x161B26);
            mc.fill_luminous_card_phys(0, 0, mw, mh, mr, wp_tl, wp_br, wp_a, wp_b);
            *cache_ref = Some(HeroCache {
                pw,
                ph,
                mw,
                mh,
                pr,
                mr,
                theme: self.theme,
                accent,
                hero: hc.pixels,
                mini: mc.pixels,
            });
        }
        if let Some(c) = cache_ref.as_ref() {
            let _ = (c.pr, c.mr);
            canvas.blit_phys_rgb_rect(hx, hy, c.pw, c.ph, &c.hero);
            canvas.blit_phys_rgb_rect(mx, my, c.mw, c.mh, &c.mini);
        }
    }

    pub fn on_key(&mut self, key: KeyEvent) {
        if !self.editing_device {
            return;
        }
        match key {
            KeyEvent::Char(c) => {
                if self.device_name.chars().count() < 18 {
                    self.device_name.push(c);
                }
            }
            KeyEvent::Backspace => {
                self.device_name.pop();
            }
            KeyEvent::Enter | KeyEvent::Escape => {
                if self.device_name.trim().is_empty() {
                    self.device_name = String::from("pizdOS PC");
                }
                self.editing_device = false;
            }
            _ => {}
        }
    }

    pub fn on_click(&mut self, lx: i32, ly: i32, ww: i32, wh: i32) -> SettingsAction {
        // Left sidebar (x: 10..136)
        if (10..=136).contains(&lx) && ly >= 36 {
            let idx = (ly - 36) / 30;
            self.editing_device = false;
            self.display_picker = None;
            let prev_page = self.page;
            match idx {
                0 => self.page = SettingsPage::Display,
                1 => self.page = SettingsPage::Taskbar,
                2 => self.page = SettingsPage::System,
                3 => self.page = SettingsPage::About,
                _ => {}
            }
            if self.page != prev_page {
                self.tab_fade = if self.animations { 0.0 } else { 1.0 };
            }
            return SettingsAction::None;
        }

        if lx < 146 {
            self.display_picker = None;
            return SettingsAction::None;
        }

        let iw = ww - 158;
        match self.page {
            SettingsPage::Display => {
                self.editing_device = false;
                if let Some(picker) = self.display_picker {
                    match picker {
                        DisplayPicker::Theme => {
                            let pw = 192;
                            let ph = 132;
                            let px = (ww - pw - 16).max(146);
                            let py = 94;
                            if (px..px + pw).contains(&lx) && (py..py + ph).contains(&ly) {
                                let idx = ((ly - (py + 6)) / 30).clamp(0, 3) as usize;
                                let presets = [
                                    ThemePreset::AuroraPeaks,
                                    ThemePreset::CrimsonSunset,
                                    ThemePreset::MatrixGlow,
                                    ThemePreset::OceanGlass,
                                ];
                                self.theme = presets[idx];
                                self.display_picker = None;
                                return SettingsAction::None;
                            }
                        }
                        DisplayPicker::Lang => {
                            let pw = 172;
                            let ph = 76;
                            let px = (ww - pw - 16).max(146);
                            let py = 128;
                            if (px..px + pw).contains(&lx) && (py..py + ph).contains(&ly) {
                                let idx = ((ly - (py + 8)) / 30).clamp(0, 1);
                                self.lang = if idx == 0 { Lang::Ru } else { Lang::En };
                                self.display_picker = None;
                                return SettingsAction::None;
                            }
                        }
                    }
                    // Dismiss on outside click; switch directly if clicking the other picker row
                    if (60..94).contains(&ly) && picker != DisplayPicker::Theme {
                        self.display_picker = Some(DisplayPicker::Theme);
                        self.last_picker = Some(DisplayPicker::Theme);
                    } else if (94..128).contains(&ly) && picker != DisplayPicker::Lang {
                        self.display_picker = Some(DisplayPicker::Lang);
                        self.last_picker = Some(DisplayPicker::Lang);
                    } else {
                        self.display_picker = None;
                    }
                    return SettingsAction::None;
                }

                if (60..94).contains(&ly) {
                    self.display_picker = Some(DisplayPicker::Theme);
                    self.last_picker = Some(DisplayPicker::Theme);
                } else if (94..128).contains(&ly) {
                    self.display_picker = Some(DisplayPicker::Lang);
                    self.last_picker = Some(DisplayPicker::Lang);
                } else if (138..172).contains(&ly) {
                    self.show_grid = !self.show_grid;
                } else if (172..206).contains(&ly) {
                    self.show_icons = !self.show_icons;
                } else if (214..248).contains(&ly) {
                    let dot_x0 = ww - 136;
                    if lx >= dot_x0 - 11 {
                        let idx = ((lx - (dot_x0 - 11)) / 22) as usize;
                        if idx < 6 {
                            self.accent_idx = idx;
                        }
                    } else {
                        self.accent_idx = (self.accent_idx + 1) % 6;
                    }
                } else if (254..=288).contains(&ly) && iw >= 160 {
                    let sw_w = ((iw - 18) / 4).max(20);
                    let rel_x = lx - 146;
                    let idx = (rel_x / (sw_w + 6)) as usize;
                    let presets = [
                        ThemePreset::AuroraPeaks,
                        ThemePreset::CrimsonSunset,
                        ThemePreset::MatrixGlow,
                        ThemePreset::OceanGlass,
                    ];
                    if let Some(&preset) = presets.get(idx) {
                        self.theme = preset;
                    }
                } else if (294..=328).contains(&ly) {
                    let pill_x0 = (ww - 196).max(146);
                    if lx >= pill_x0 {
                        let idx = (((lx - pill_x0) * 4) / 184).clamp(0, 3) as usize;
                        self.ui_scale_pct = UI_SCALES[idx];
                    } else {
                        self.step_ui_scale(1);
                    }
                }
            }
            SettingsPage::Taskbar => {
                self.editing_device = false;
                if (60..94).contains(&ly) {
                    self.taskbar_top = !self.taskbar_top;
                } else if (94..128).contains(&ly) {
                    self.floating_dock = !self.floating_dock;
                } else if (128..162).contains(&ly) {
                    self.animations = !self.animations;
                } else if (162..196).contains(&ly) {
                    self.crosshair_cursor = !self.crosshair_cursor;
                } else if (196..230).contains(&ly) {
                    self.show_clock = !self.show_clock;
                } else if (230..264).contains(&ly) {
                    self.genie_minimize = !self.genie_minimize;
                }
            }
            SettingsPage::System => {
                self.editing_device = false;
                if (138..172).contains(&ly) {
                    self.snake_tick_div = match self.snake_tick_div {
                        14 => 10,
                        10 => 5,
                        _ => 14,
                    };
                } else if (172..206).contains(&ly) {
                    self.glass_level = (self.glass_level + 1) % 3;
                } else if (240..274).contains(&ly) {
                    return SettingsAction::RefreshMetrics;
                }
            }
            SettingsPage::About => {
                // Hero OS Banner click: check updates / dev-mode easter egg
                if (36..=126).contains(&ly) {
                    self.editing_device = false;
                    self.update_taps = self.update_taps.saturating_add(1);
                    return SettingsAction::None;
                }
                // Middle Bento Row (ly: 134..=208)
                let w1 = ((iw - 8) * 45) / 100;
                if (134..=208).contains(&ly) {
                    if lx <= 146 + w1 {
                        if self.editing_device && self.device_name.trim().is_empty() {
                            self.device_name = String::from("pizdOS PC");
                        }
                        self.editing_device = !self.editing_device;
                    } else {
                        self.editing_device = false;
                        self.page = SettingsPage::System;
                        return SettingsAction::RefreshMetrics;
                    }
                    return SettingsAction::None;
                }
                self.editing_device = false;
                // Bottom Grouped ColorOS List Card
                let grp_ly = 216;
                let grp_h = (wh - 12 - grp_ly).clamp(96, 114);
                if ly >= grp_ly && ly < grp_ly + grp_h {
                    let row_h = (grp_h / 4).max(1);
                    let row = (ly - grp_ly) / row_h;
                    match row {
                        0 => {
                            self.page = SettingsPage::System;
                            return SettingsAction::RefreshMetrics;
                        }
                        1 => return SettingsAction::OpenTerminal,
                        2 => return SettingsAction::OpenPizGet,
                        3 => self.license_alt = !self.license_alt,
                        _ => {}
                    }
                }
            }
        }
        SettingsAction::None
    }

    fn draw_card_frame(canvas: &mut Canvas, x: i32, y: i32, w: i32, h: i32, r: i32) {
        canvas.fill_rounded_rect(x, y, w, h, r, 0x161B26);
        canvas.draw_rounded_rect_alpha(x, y, w, h, r, 0xFFFFFF, 22);
        canvas.blend_rect(x + r, y + 1, (w - 2 * r).max(0), 1, 0xFFFFFF, 18);
    }

    fn draw_chevron(canvas: &mut Canvas, ch_x: f32, ch_y: f32) {
        canvas.draw_aa_line(ch_x - 2.0, ch_y - 3.0, ch_x + 1.5, ch_y, 1.2, 0x64748B, 175);
        canvas.draw_aa_line(ch_x + 1.5, ch_y, ch_x - 2.0, ch_y + 3.0, 1.2, 0x64748B, 175);
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_spec_row(
        canvas: &mut Canvas,
        x: i32,
        y: i32,
        w: i32,
        row_h: i32,
        dot_col: u32,
        label: &str,
        value: &str,
        hovered: bool,
        divider_top: bool,
    ) {
        if divider_top {
            canvas.blend_rect(x + 14, y, w - 28, 1, 0xFFFFFF, 12);
        }
        if hovered {
            canvas.blend_rounded_rect(x + 4, y + 2, w - 8, row_h - 4, 8, 0xFFFFFF, 12);
        }
        let mid_y = y + row_h / 2;
        canvas.fill_aa_circle((x + 19) as f32, mid_y as f32, 6.0, dot_col, 32);
        canvas.fill_aa_circle((x + 19) as f32, mid_y as f32, 2.4, dot_col, 240);
        canvas.draw_text_clipped(x + 32, mid_y - 7, 150, label, 0xE2E8F0, 1);

        let max_vw = (w - 190).max(40);
        let vw = canvas.measure_text(value, 1).min(max_vw);
        canvas.draw_text_clipped(x + w - 24 - vw, mid_y - 7, max_vw, value, 0x94A3B8, 1);
        Self::draw_chevron(canvas, (x + w - 14) as f32, mid_y as f32);
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_toggle_row(
        canvas: &mut Canvas,
        x: i32,
        y: i32,
        w: i32,
        row_h: i32,
        dot_col: u32,
        label: &str,
        anim_t: f32,
        accent: u32,
        hovered: bool,
        divider_top: bool,
    ) {
        if divider_top {
            canvas.blend_rect(x + 14, y, w - 28, 1, 0xFFFFFF, 12);
        }
        if hovered {
            canvas.blend_rounded_rect(x + 4, y + 2, w - 8, row_h - 4, 8, 0xFFFFFF, 12);
        }
        let mid_y = y + row_h / 2;
        canvas.fill_aa_circle((x + 19) as f32, mid_y as f32, 6.0, dot_col, 32);
        canvas.fill_aa_circle((x + 19) as f32, mid_y as f32, 2.4, dot_col, 240);
        canvas.draw_text_clipped(x + 32, mid_y - 7, w - 82, label, 0xE2E8F0, 1);
        canvas.draw_toggle_anim(x + w - 48, mid_y - 9, anim_t, accent);
    }

    #[allow(clippy::too_many_arguments)]
    pub fn draw(
        &self,
        canvas: &mut Canvas,
        wx: i32,
        wy: i32,
        ww: i32,
        wh: i32,
        kernel: &str,
        mem: &MemInfo,
        cpu_pct: u8,
        uptime: &str,
        tick: u64,
        hover_lx: i32,
        hover_ly: i32,
    ) {
        let lang = self.lang;
        let accent = self.active_accent();

        // Minimalist flat sidebar with smooth sliding active indicator pill
        let sb_x = wx + 10;
        let sb_y = wy + 34;
        let sb_w = 126;
        let sb_h = wh - 44;
        canvas.fill_rounded_rect(sb_x, sb_y, sb_w, sb_h, 8, 0x11151E);
        canvas.draw_rounded_rect_alpha(sb_x, sb_y, sb_w, sb_h, 8, 0xFFFFFF, 18);

        let pill_y = sb_y + 6 + self.tab_pill_y.round() as i32;
        canvas.blend_rounded_rect(sb_x + 5, pill_y, sb_w - 10, 25, 6, accent, 42);
        canvas.fill_rounded_rect(sb_x + 7, pill_y + 6, 3, 13, 1, accent);

        let tabs = [
            (SettingsPage::Display, tr(lang, "Экран и язык", "Display & Lang")),
            (SettingsPage::Taskbar, tr(lang, "Панель задач", "Taskbar")),
            (SettingsPage::System, tr(lang, "Система", "System")),
            (SettingsPage::About, tr(lang, "О системе", "About OS")),
        ];
        for (i, (p, label)) in tabs.iter().enumerate() {
            let ty = sb_y + 6 + (i as i32) * 30;
            let active = self.page == *p;
            let local_ty = 40 + (i as i32) * 30;
            let hovered = (10..=136).contains(&hover_lx)
                && (local_ty..local_ty + 25).contains(&hover_ly);
            if !active && hovered {
                canvas.blend_rounded_rect(sb_x + 5, ty, sb_w - 10, 25, 6, 0xFFFFFF, 12);
            }
            let fg = if active { 0xF8FAFC } else { 0x94A3B8 };
            canvas.draw_text_clipped(sb_x + 15, ty + 6, sb_w - 22, label, fg, 1);
        }

        let x = wx + 146;
        let tab_dy = if self.animations && self.tab_fade < 0.99 {
            ((1.0 - Canvas::ease_out_cubic(self.tab_fade)) * 7.0).round() as i32
        } else {
            0
        };
        let wy_base = wy;
        let wy = wy + tab_dy;
        let y = wy + 36;
        let iw = ww - 158;
        let in_content = hover_lx >= 146 && hover_lx < ww - 12;

        match self.page {
            SettingsPage::Display => {
                canvas.draw_text_clipped(
                    x + 2,
                    y + 2,
                    iw,
                    tr(lang, "Экран, обои и акцент", "Display, Wallpaper & Accent"),
                    0x94A3B8,
                    1,
                );

                // Grouped ColorOS Card 1: Theme + Language
                let c1_y = wy + 60;
                Self::draw_card_frame(canvas, x, c1_y, iw, 68, 12);
                Self::draw_spec_row(
                    canvas,
                    x,
                    c1_y,
                    iw,
                    34,
                    accent,
                    tr(lang, "Тема и обои", "Wallpaper"),
                    self.theme.name(lang),
                    (in_content && (60..94).contains(&hover_ly))
                        || self.display_picker == Some(DisplayPicker::Theme),
                    false,
                );
                Self::draw_spec_row(
                    canvas,
                    x,
                    c1_y + 34,
                    iw,
                    34,
                    0x38BDF8,
                    tr(lang, "Язык интерфейса", "Language"),
                    tr(lang, "Русский (RU)", "English (EN)"),
                    (in_content && (94..128).contains(&hover_ly))
                        || self.display_picker == Some(DisplayPicker::Lang),
                    true,
                );

                // Grouped ColorOS Card 2: Desktop Toggles
                let c2_y = wy + 138;
                Self::draw_card_frame(canvas, x, c2_y, iw, 68, 12);
                Self::draw_toggle_row(
                    canvas,
                    x,
                    c2_y,
                    iw,
                    34,
                    0xA855F7,
                    tr(lang, "Сетка поверх обоев", "Wallpaper Grid"),
                    self.toggle_anim[0],
                    accent,
                    in_content && (138..172).contains(&hover_ly),
                    false,
                );
                Self::draw_toggle_row(
                    canvas,
                    x,
                    c2_y + 34,
                    iw,
                    34,
                    0x3FB950,
                    tr(lang, "Иконки на рабочем столе", "Desktop Icons"),
                    self.toggle_anim[1],
                    accent,
                    in_content && (172..206).contains(&hover_ly),
                    true,
                );

                // Card 3: System Accent Color Palette
                let c3_y = wy + 214;
                Self::draw_card_frame(canvas, x, c3_y, iw, 34, 12);
                if in_content && (214..248).contains(&hover_ly) {
                    canvas.blend_rounded_rect(x + 4, c3_y + 2, iw - 8, 30, 8, 0xFFFFFF, 10);
                }
                let mid_y = c3_y + 17;
                canvas.fill_aa_circle((x + 19) as f32, mid_y as f32, 6.0, accent, 32);
                canvas.fill_aa_circle((x + 19) as f32, mid_y as f32, 2.4, accent, 240);
                canvas.draw_text_clipped(
                    x + 32,
                    mid_y - 7,
                    (iw - 165).max(40),
                    tr(lang, "Цвет акцента", "Accent Color"),
                    0xE2E8F0,
                    1,
                );
                let dot_x0 = x + iw - 124;
                for i in 0..6 {
                    let cx = (dot_x0 + (i as i32) * 22) as f32;
                    let cy = mid_y as f32;
                    let col = if i == 0 {
                        self.theme.accent()
                    } else {
                        ACCENT_PALETTE[i - 1]
                    };
                    if self.accent_idx == i {
                        canvas.fill_aa_circle(cx, cy, 9.2, 0xFFFFFF, 215);
                        canvas.fill_aa_circle(cx, cy, 7.6, 0x161B26, 255);
                    }
                    canvas.fill_aa_circle(cx, cy, 6.2, col, 255);
                    if i == 0 {
                        // Subtle inner auto-ring
                        canvas.fill_aa_circle(cx, cy, 2.1, 0xFFFFFF, 225);
                    }
                }

                // Interactive ColorOS Luminous Theme Swatches
                let sw_y = wy + 254;
                let sw_h = if wy + wh >= sw_y + 78 { 32 } else { (wy + wh - 10 - sw_y).clamp(24, 34) };
                if sw_y + 24 <= wy + wh - 8 && iw >= 160 {
                    let sw_w = ((iw - 18) / 4).max(20);
                    let presets = [
                        ThemePreset::AuroraPeaks,
                        ThemePreset::CrimsonSunset,
                        ThemePreset::MatrixGlow,
                        ThemePreset::OceanGlass,
                    ];
                    for (i, preset) in presets.iter().enumerate() {
                        let sx = x + (i as i32) * (sw_w + 6);
                        let (tl, br, ba, _, _, rib) = preset.mesh_palette();
                        canvas.fill_luminous_card(sx, sw_y, sw_w, sw_h, 10, tl, br, ba, rib);
                        let selected = *preset == self.theme;
                        canvas.draw_rounded_rect_alpha(
                            sx,
                            sw_y,
                            sw_w,
                            sw_h,
                            10,
                            0xFFFFFF,
                            if selected { 175 } else { 28 },
                        );
                        if selected {
                            canvas.fill_aa_circle(
                                (sx + sw_w - 10) as f32,
                                (sw_y + 10) as f32,
                                3.0,
                                0xFFFFFF,
                                240,
                            );
                        }
                    }
                }

                // Card 4: Segmented UI Scale Switcher (100% / 125% / 150% / 175%)
                let sc_y = wy + 294;
                if sc_y + 32 <= wy_base + wh - 4 {
                    Self::draw_card_frame(canvas, x, sc_y, iw, 34, 12);
                    let mid_y = sc_y + 17;
                    canvas.fill_aa_circle((x + 19) as f32, mid_y as f32, 6.0, 0x2DD4BF, 32);
                    canvas.fill_aa_circle((x + 19) as f32, mid_y as f32, 2.4, 0x2DD4BF, 240);
                    canvas.draw_text_clipped(
                        x + 32,
                        mid_y - 7,
                        (iw - 210).max(40),
                        tr(lang, "Масштаб UI", "UI Scale"),
                        0xE2E8F0,
                        1,
                    );
                    let seg_x0 = (wx + ww - 196).max(x + 110);
                    let seg_w = 184;
                    canvas.fill_rounded_rect(seg_x0, sc_y + 5, seg_w, 24, 8, 0x0E121B);
                    let cell_w = seg_w / 4;
                    for (idx, &pct) in UI_SCALES.iter().enumerate() {
                        let px = seg_x0 + (idx as i32) * cell_w + 2;
                        let pw = cell_w - 4;
                        let active = self.ui_scale_pct == pct;
                        if active {
                            canvas.blend_rounded_rect(px, sc_y + 7, pw, 20, 6, accent, 170);
                            canvas.draw_rounded_rect_alpha(px, sc_y + 7, pw, 20, 6, 0xFFFFFF, 85);
                        } else if in_content
                            && (294..=328).contains(&hover_ly)
                            && hover_lx >= (px - wx)
                            && hover_lx < (px - wx + pw)
                        {
                            canvas.blend_rounded_rect(px, sc_y + 7, pw, 20, 6, 0xFFFFFF, 18);
                        }
                        let lbl = format!("{pct}%");
                        let tw = canvas.measure_text(&lbl, 1);
                        canvas.draw_text_clipped(
                            px + ((pw - tw) / 2).max(2),
                            sc_y + 10,
                            pw - 4,
                            &lbl,
                            if active { 0xFFFFFF } else { 0x94A3B8 },
                            1,
                        );
                    }
                }
            }
            SettingsPage::Taskbar => {
                canvas.draw_text_clipped(
                    x + 2,
                    y + 2,
                    iw,
                    tr(lang, "Панель задач и эффекты", "Taskbar & Effects"),
                    0x94A3B8,
                    1,
                );

                let c_y = wy + 60;
                Self::draw_card_frame(canvas, x, c_y, iw, 204, 12);
                Self::draw_toggle_row(
                    canvas,
                    x,
                    c_y,
                    iw,
                    34,
                    accent,
                    tr(lang, "Панель задач сверху", "Taskbar on Top"),
                    self.toggle_anim[2],
                    accent,
                    in_content && (60..94).contains(&hover_ly),
                    false,
                );
                Self::draw_toggle_row(
                    canvas,
                    x,
                    c_y + 34,
                    iw,
                    34,
                    0x38BDF8,
                    tr(lang, "Плавающий док-бар", "Floating Minimal Dock"),
                    self.toggle_anim[3],
                    accent,
                    in_content && (94..128).contains(&hover_ly),
                    true,
                );
                Self::draw_toggle_row(
                    canvas,
                    x,
                    c_y + 68,
                    iw,
                    34,
                    0xA855F7,
                    tr(lang, "Плавные анимации UI", "Smooth UI Animations"),
                    self.toggle_anim[4],
                    accent,
                    in_content && (128..162).contains(&hover_ly),
                    true,
                );
                Self::draw_toggle_row(
                    canvas,
                    x,
                    c_y + 102,
                    iw,
                    34,
                    0xF59E0B,
                    tr(lang, "Курсор-прицел", "Crosshair Cursor"),
                    self.toggle_anim[5],
                    accent,
                    in_content && (162..196).contains(&hover_ly),
                    true,
                );
                Self::draw_toggle_row(
                    canvas,
                    x,
                    c_y + 136,
                    iw,
                    34,
                    0x3FB950,
                    tr(lang, "Часы в трее (выкл = аптайм)", "Clock in Tray (off = uptime)"),
                    self.toggle_anim[6],
                    accent,
                    in_content && (196..230).contains(&hover_ly),
                    true,
                );
                Self::draw_toggle_row(
                    canvas,
                    x,
                    c_y + 170,
                    iw,
                    34,
                    0xF43F5E,
                    tr(lang, "Эффект Джинна (macOS)", "macOS Genie Minimize"),
                    self.toggle_anim[7],
                    accent,
                    in_content && (230..264).contains(&hover_ly),
                    true,
                );

                let tip_y = wy + 272;
                if tip_y + 42 <= wy + wh - 6 {
                    Self::draw_card_frame(canvas, x, tip_y, iw, 42, 12);
                    canvas.fill_aa_circle((x + 19) as f32, (tip_y + 21) as f32, 6.0, accent, 36);
                    canvas.fill_aa_circle((x + 19) as f32, (tip_y + 21) as f32, 2.5, accent, 240);
                    canvas.draw_text_clipped(
                        x + 32,
                        tip_y + 7,
                        iw - 44,
                        tr(lang, "Управление окнами", "Window Controls"),
                        0xF1F5F9,
                        1,
                    );
                    canvas.draw_text_clipped(
                        x + 32,
                        tip_y + 23,
                        iw - 44,
                        tr(
                            lang,
                            "Тяни любой край или угол окна для изменения размера",
                            "Drag any window edge or corner to resize dynamically",
                        ),
                        0x8B949E,
                        1,
                    );
                }
            }
            SettingsPage::System => {
                canvas.draw_text_clipped(
                    x + 2,
                    y + 2,
                    iw,
                    tr(lang, "Производительность и стекло", "Performance & Glass"),
                    0x94A3B8,
                    1,
                );

                // Top 2 ColorOS Metric Bento Cards: CPU & RAM
                let b_y = wy + 60;
                let bw = (iw - 8) / 2;
                Self::draw_card_frame(canvas, x, b_y, bw, 68, 12);
                canvas.draw_text_clipped(x + 13, b_y + 9, bw - 50, "CPU Load", 0x8B949E, 1);
                canvas.draw_text_clipped(
                    x + bw - 38,
                    b_y + 9,
                    28,
                    &format!("{cpu_pct}%"),
                    accent,
                    1,
                );
                canvas.draw_text_clipped(x + 13, b_y + 25, bw - 24, "x86_64 Native", 0xF8FAFC, 1);
                canvas.draw_progress_bar(x + 13, b_y + 49, bw - 26, 7, cpu_pct, accent, 0x0B0E16, 0x30363D);

                let rx = x + bw + 8;
                Self::draw_card_frame(canvas, rx, b_y, bw, 68, 12);
                canvas.draw_text_clipped(rx + 13, b_y + 9, bw - 50, "RAM Usage", 0x8B949E, 1);
                canvas.draw_text_clipped(
                    rx + bw - 38,
                    b_y + 9,
                    28,
                    &format!("{}%", mem.percent),
                    0xD29922,
                    1,
                );
                canvas.draw_text_clipped(
                    rx + 13,
                    b_y + 25,
                    bw - 24,
                    &format!("{} / {} MB", mem.used_mb, mem.total_mb),
                    0xF8FAFC,
                    1,
                );
                canvas.draw_progress_bar(
                    rx + 13,
                    b_y + 49,
                    bw - 26,
                    7,
                    mem.percent,
                    0xD29922,
                    0x0B0E16,
                    0x30363D,
                );

                // Bottom Grouped ColorOS Spec & Action Card (4 rows)
                let c_y = wy + 138;
                Self::draw_card_frame(canvas, x, c_y, iw, 136, 12);
                let spd = match (self.snake_tick_div, lang) {
                    (14, Lang::Ru) => "Медленно",
                    (14, Lang::En) => "Slow",
                    (5, Lang::Ru) => "Турбо",
                    (5, Lang::En) => "Turbo",
                    (_, Lang::Ru) => "Норма",
                    (_, Lang::En) => "Normal",
                };
                let glass_lbl = match (self.glass_level, lang) {
                    (1, Lang::Ru) => "Воздушное",
                    (1, Lang::En) => "Airy Glass",
                    (2, Lang::Ru) => "Плотное",
                    (2, Lang::En) => "Solid Dark",
                    (_, Lang::Ru) => "Стандарт",
                    (_, Lang::En) => "Standard",
                };
                Self::draw_spec_row(
                    canvas,
                    x,
                    c_y,
                    iw,
                    34,
                    0x3FB950,
                    tr(lang, "Скорость Pizмейки", "Piznake Speed"),
                    spd,
                    in_content && (138..172).contains(&hover_ly),
                    false,
                );
                Self::draw_spec_row(
                    canvas,
                    x,
                    c_y + 34,
                    iw,
                    34,
                    0xA855F7,
                    tr(lang, "Интенсивность стекла", "Glass Intensity"),
                    glass_lbl,
                    in_content && (172..206).contains(&hover_ly),
                    true,
                );
                Self::draw_spec_row(
                    canvas,
                    x,
                    c_y + 68,
                    iw,
                    34,
                    0x38BDF8,
                    tr(lang, "Аптайм системы", "System Uptime"),
                    uptime,
                    false,
                    true,
                );
                Self::draw_spec_row(
                    canvas,
                    x,
                    c_y + 102,
                    iw,
                    34,
                    accent,
                    tr(lang, "Обновить метрики", "Refresh Metrics"),
                    tr(lang, "Нажать", "Run"),
                    in_content && (240..274).contains(&hover_ly),
                    true,
                );
            }
            SettingsPage::About => {
                let (_, _, _, _, _, wp_rib) = self.theme.mesh_palette();
                let bento_y = y + 98;
                let bento_h = 74;
                let w1 = ((iw - 8) * 45) / 100;
                let w2 = iw - 8 - w1;
                let scr_x = x + w1 - 44;
                let scr_y = bento_y + 11;

                // 1. ColorOS 15/16/17 Luminous Liquid-Glass Hero Poster (cached RGB buffer!)
                //    + Bento Device Frame drawn first, then cached Mini Wallpaper blitted inside
                Self::draw_card_frame(canvas, x, bento_y, w1, bento_h, 12);
                self.blit_cached_hero(canvas, (x, y, iw), (scr_x, scr_y), accent);

                let hero_hov = in_content && (36..=126).contains(&hover_ly);
                canvas.draw_rounded_rect_alpha(
                    x,
                    y,
                    iw,
                    90,
                    14,
                    0xFFFFFF,
                    if hero_hov { 64 } else { 34 },
                );

                // Wordmark + Floating Liquid-Glass Version Pill
                canvas.draw_text_clipped(x + 18, y + 13, iw - 126, "pizdOS", 0xFFFFFF, 2);
                let title_w = canvas.measure_text("pizdOS", 2);
                let pill_x = x + 18 + title_w + 8;
                if pill_x + 42 < x + iw - 96 {
                    canvas.blend_rounded_rect(pill_x, y + 16, 36, 19, 9, 0xFFFFFF, 28);
                    canvas.draw_rounded_rect_alpha(pill_x, y + 16, 36, 19, 9, 0xFFFFFF, 82);
                    canvas.draw_text_clipped(pill_x + 9, y + 18, 24, "2.0", 0xFFFFFF, 1);
                }

                let sub_msg = if self.update_taps >= 5 {
                    tr(
                        lang,
                        "Bratukha Dev Mode ON • Root разблокирован",
                        "Bratukha Dev Mode ON • Root unlocked",
                    )
                } else if self.update_taps > 0 {
                    tr(
                        lang,
                        "Актуальная сборка • GNU удалён навсегда",
                        "Up to date • GNU permanently removed",
                    )
                } else {
                    tr(
                        lang,
                        "Bratukha Edition • Официальная сборка",
                        "Bratukha Edition • Official Build",
                    )
                };
                canvas.draw_text_clipped(
                    x + 18,
                    y + 43,
                    iw - 108,
                    sub_msg,
                    if self.update_taps >= 5 { 0xFBBF24 } else { 0xCBD5E1 },
                    1,
                );
                canvas.draw_text_clipped(
                    x + 18,
                    y + 62,
                    iw - 108,
                    "by prosh1vka, Lowyk, KiryxaTipa",
                    0x6EE7B7,
                    1,
                );

                // Top-right Official Status Capsule + Gently Orbiting Luminous Emblem
                if iw > 265 {
                    let bx = x + iw - 92;
                    let by = y + 13;
                    canvas.blend_rounded_rect(bx, by, 80, 23, 11, 0x090D14, 195);
                    canvas.draw_rounded_rect_alpha(bx, by, 80, 23, 11, 0xFFFFFF, 46);
                    let dot_c = if self.update_taps >= 5 { 0xF59E0B } else { 0x3FB950 };
                    let badge_txt = if self.update_taps >= 5 { "DEV ON" } else { "NO GNU" };
                    canvas.fill_aa_circle((bx + 13) as f32, (by + 11) as f32 + 0.5, 3.0, dot_c, 255);
                    canvas.draw_text_clipped(bx + 22, by + 5, 54, badge_txt, 0xF8FAFC, 1);

                    let ex = (x + iw - 42) as f32;
                    let ey = (y + 62) as f32;
                    let phase = if self.animations {
                        (tick as f32) * 0.045
                    } else {
                        0.45
                    };
                    let ox1 = phase.cos() * 5.2;
                    let oy1 = phase.sin() * 2.4;
                    let ox2 = (phase + 2.8).cos() * 5.2;
                    let oy2 = (phase + 2.8).sin() * 2.4;
                    canvas.fill_aa_circle(ex, ey, 19.5, 0xFFFFFF, 18);
                    canvas.fill_aa_circle(ex + ox1, ey + oy1, 10.5, accent, 165);
                    canvas.fill_aa_circle(ex + ox2, ey + oy2, 9.5, wp_rib, 155);
                    canvas.fill_aa_circle(ex, ey, 5.2, 0x0F1522, 225);
                    canvas.fill_aa_circle(ex, ey, 2.2, 0xFFFFFF, 240);
                }

                // 2. ColorOS 15/16 Asymmetric Bento Duo: Interactive Device Preview + Memory Card
                let left_hov = in_content && (134..=208).contains(&hover_ly) && hover_lx <= 146 + w1;
                let right_hov = in_content && (134..=208).contains(&hover_ly) && hover_lx > 146 + w1;

                if self.editing_device {
                    canvas.draw_rounded_rect_alpha(x, bento_y, w1, bento_h, 12, accent, 180);
                } else if left_hov {
                    canvas.draw_rounded_rect_alpha(x, bento_y, w1, bento_h, 12, 0xFFFFFF, 55);
                }

                canvas.draw_text_clipped(
                    x + 13,
                    bento_y + 11,
                    w1 - 56,
                    if self.editing_device {
                        tr(lang, "Имя (Enter)", "Name (Enter)")
                    } else {
                        tr(lang, "Устройство", "Device")
                    },
                    if self.editing_device { accent } else { 0x8B949E },
                    1,
                );
                let shown_name = if self.editing_device {
                    format!("{}_", self.device_name)
                } else {
                    self.device_name.clone()
                };
                canvas.draw_text_clipped(x + 13, bento_y + 28, w1 - 56, &shown_name, 0xF8FAFC, 1);
                canvas.blend_rounded_rect(x + 13, bento_y + 47, 50, 17, 8, accent, 38);
                canvas.draw_text_clipped(x + 18, bento_y + 49, 44, "x86_64", 0xCBD5E1, 1);

                // Miniature ColorOS Screen Border + Speaker Notch (Cached wallpaper already blitted above)
                canvas.draw_rounded_rect_alpha(scr_x, scr_y, 32, 52, 6, 0xFFFFFF, 95);
                canvas.fill_rounded_rect(scr_x + 11, scr_y + 4, 10, 2, 1, 0x0A0E16);

                // Right Card: RAM + Signature ColorOS 15/16 Multi-Segment Pill Bar
                let rx = x + w1 + 8;
                Self::draw_card_frame(canvas, rx, bento_y, w2, bento_h, 12);
                if right_hov {
                    canvas.draw_rounded_rect_alpha(rx, bento_y, w2, bento_h, 12, 0xFFFFFF, 55);
                }

                canvas.draw_text_clipped(
                    rx + 13,
                    bento_y + 11,
                    w2 - 58,
                    tr(lang, "Память (RAM)", "Memory"),
                    0x8B949E,
                    1,
                );
                canvas.draw_text_clipped(
                    rx + w2 - 38,
                    bento_y + 11,
                    28,
                    &format!("{}%", mem.percent),
                    0x38BDF8,
                    1,
                );
                canvas.draw_text_clipped(
                    rx + 13,
                    bento_y + 28,
                    w2 - 26,
                    &format!("{} / {} MB", mem.used_mb, mem.total_mb),
                    0xF8FAFC,
                    1,
                );

                // ColorOS 15/16 Multi-Category Segmented Memory Pill Bar
                let bar_x = rx + 13;
                let bar_y = bento_y + 51;
                let bar_w = (w2 - 26).max(24);
                canvas.fill_rounded_rect(bar_x, bar_y, bar_w, 8, 4, 0x0B0E16);
                let total_fill = ((bar_w * (mem.percent as i32)) / 100).clamp(18, bar_w);
                let seg1 = (total_fill * 52) / 100;
                let seg2 = (total_fill * 30) / 100;
                let seg3 = (total_fill - seg1 - seg2).max(4);
                canvas.fill_rounded_rect(bar_x, bar_y, seg1.max(6), 8, 4, accent);
                if bar_x + seg1 + 2 < bar_x + bar_w {
                    canvas.fill_rounded_rect(bar_x + seg1 + 1, bar_y, seg2.max(4), 8, 3, 0x2DD4BF);
                }
                if bar_x + seg1 + seg2 + 3 < bar_x + bar_w {
                    canvas.fill_rounded_rect(bar_x + seg1 + seg2 + 2, bar_y, seg3, 8, 3, 0xA855F7);
                }

                // 3. ColorOS 15/16 Interactive Grouped Spec List Card
                let grp_y = y + 180;
                let grp_h = (wy + wh - 12 - grp_y).clamp(96, 114);
                if grp_y + 84 <= wy + wh - 6 {
                    Self::draw_card_frame(canvas, x, grp_y, iw, grp_h, 13);

                    let lic_val = if self.license_alt {
                        tr(lang, "0% GNU • Чистый Rust", "0% GNU • Pure Rust")
                    } else {
                        "MIT • NO GNU (0 Deps)"
                    };
                    let rows = [
                        (tr(lang, "Ядро", "Kernel"), kernel, 0xA855F7),
                        (tr(lang, "Базовый стек", "Core Stack"), "musl + BusyBox", 0x38BDF8),
                        (tr(lang, "Пакетный менеджер", "Packages"), "piz-get 0.1.0", 0xF43F5E),
                        (tr(lang, "Лицензия", "License"), lic_val, 0x3FB950),
                    ];
                    let row_h = (grp_h / 4).max(1);
                    let grp_ly = 216;
                    for (idx, (lbl, val, dot_col)) in rows.iter().enumerate() {
                        let ry = grp_y + (idx as i32) * row_h;
                        let r_ly0 = grp_ly + (idx as i32) * row_h;
                        let hov = in_content && hover_ly >= r_ly0 && hover_ly < r_ly0 + row_h;
                        Self::draw_spec_row(
                            canvas,
                            x,
                            ry,
                            iw,
                            row_h,
                            *dot_col,
                            lbl,
                            val,
                            hov,
                            idx > 0,
                        );
                    }
                }
            }
        }

        // Animated ColorOS 15/16 Floating Dropdown Picker on Display Tab
        if self.page == SettingsPage::Display
            && self.picker_anim > 0.02
            && let Some(picker) = self.display_picker.or(self.last_picker)
        {
                let p = Canvas::smoothstep(self.picker_anim);
                let bg_alpha = (242.0 * p).round() as u8;
                let slide_y = ((1.0 - p) * -6.0).round() as i32;
                match picker {
                    DisplayPicker::Theme => {
                        let pw = 192;
                        let ph = 132;
                        let px = wx + (ww - pw - 16).max(146);
                        let py = wy + 94 + slide_y;
                        canvas.draw_soft_shadow(px, py, pw, ph, 12);
                        canvas.blend_rounded_rect(px, py, pw, ph, 12, 0x161D2E, bg_alpha);
                        canvas.draw_rounded_rect_alpha(
                            px,
                            py,
                            pw,
                            ph,
                            12,
                            0x475569,
                            (185.0 * p).round() as u8,
                        );
                        let presets = [
                            (ThemePreset::AuroraPeaks, 0x38BDF8),
                            (ThemePreset::CrimsonSunset, 0xF43F5E),
                            (ThemePreset::MatrixGlow, 0x3FB950),
                            (ThemePreset::OceanGlass, 0x6366F1),
                        ];
                        for (idx, (preset, swatch)) in presets.iter().enumerate() {
                            let ry = py + 6 + (idx as i32) * 30;
                            let r_ly = 94 + 6 + (idx as i32) * 30;
                            let active = self.theme == *preset;
                            let hov = hover_lx >= (px - wx)
                                && hover_lx < (px - wx + pw)
                                && hover_ly >= r_ly
                                && hover_ly < r_ly + 28;
                            if active {
                                canvas.blend_rounded_rect(
                                    px + 6,
                                    ry,
                                    pw - 12,
                                    28,
                                    8,
                                    accent,
                                    (54.0 * p).round() as u8,
                                );
                            } else if hov {
                                canvas.blend_rounded_rect(
                                    px + 6,
                                    ry,
                                    pw - 12,
                                    28,
                                    8,
                                    0x334155,
                                    (140.0 * p).round() as u8,
                                );
                            }
                            let dot_a = (255.0 * p).round() as u8;
                            canvas.fill_aa_circle((px + 19) as f32, (ry + 14) as f32, 5.0, *swatch, dot_a);
                            canvas.draw_text(
                                px + 32,
                                ry + 8,
                                preset.name(lang),
                                if active { 0xFFFFFF } else { 0xE2E8F0 },
                                1,
                            );
                            if active {
                                canvas.fill_aa_circle((px + pw - 16) as f32, (ry + 14) as f32, 3.0, accent, dot_a);
                            }
                        }
                    }
                    DisplayPicker::Lang => {
                        let pw = 172;
                        let ph = 76;
                        let px = wx + (ww - pw - 16).max(146);
                        let py = wy + 128 + slide_y;
                        canvas.draw_soft_shadow(px, py, pw, ph, 12);
                        canvas.blend_rounded_rect(px, py, pw, ph, 12, 0x161D2E, bg_alpha);
                        canvas.draw_rounded_rect_alpha(
                            px,
                            py,
                            pw,
                            ph,
                            12,
                            0x475569,
                            (185.0 * p).round() as u8,
                        );
                        let langs = [
                            (Lang::Ru, "RU", "Русский (RU)"),
                            (Lang::En, "EN", "English (EN)"),
                        ];
                        for (idx, (item_lang, code, label)) in langs.iter().enumerate() {
                            let ry = py + 8 + (idx as i32) * 30;
                            let r_ly = 128 + 8 + (idx as i32) * 30;
                            let active = self.lang == *item_lang;
                            let hov = hover_lx >= (px - wx)
                                && hover_lx < (px - wx + pw)
                                && hover_ly >= r_ly
                                && hover_ly < r_ly + 28;
                            if active {
                                canvas.blend_rounded_rect(
                                    px + 6,
                                    ry,
                                    pw - 12,
                                    28,
                                    8,
                                    accent,
                                    (54.0 * p).round() as u8,
                                );
                            } else if hov {
                                canvas.blend_rounded_rect(
                                    px + 6,
                                    ry,
                                    pw - 12,
                                    28,
                                    8,
                                    0x334155,
                                    (140.0 * p).round() as u8,
                                );
                            }
                            canvas.fill_rounded_rect(
                                px + 13,
                                ry + 6,
                                22,
                                16,
                                5,
                                if active { accent } else { 0x263147 },
                            );
                            canvas.draw_text(px + 18, ry + 8, code, 0xFFFFFF, 1);
                            canvas.draw_text(
                                px + 42,
                                ry + 8,
                                label,
                                if active { 0xFFFFFF } else { 0xE2E8F0 },
                                1,
                            );
                            if active {
                                let dot_a = (255.0 * p).round() as u8;
                                canvas.fill_aa_circle((px + pw - 16) as f32, (ry + 14) as f32, 3.0, accent, dot_a);
                            }
                        }
                    }
                }
        }
    }
}


// ---------------------------------------------------------------------------
// 6. Calculator
// ---------------------------------------------------------------------------
pub struct CalcState {
    pub display: String,
    pub acc: Option<i64>,
    pub op: Option<char>,
    pub clear_next: bool,
    pub flash: Option<(usize, f32)>,
}

const CALC_BUTTONS: [&str; 16] = [
    "7", "8", "9", "/",
    "4", "5", "6", "*",
    "1", "2", "3", "-",
    "C", "0", "=", "+",
];

impl CalcState {
    pub fn new() -> Self {
        Self {
            display: String::from("0"),
            acc: None,
            op: None,
            clear_next: false,
            flash: None,
        }
    }

    pub fn step_anim(&mut self) {
        if let Some((idx, v)) = self.flash {
            let next = v - 0.14;
            if next <= 0.01 {
                self.flash = None;
            } else {
                self.flash = Some((idx, next));
            }
        }
    }

    pub fn press(&mut self, token: &str) {
        if let Some(pos) = CALC_BUTTONS.iter().position(|&b| b == token) {
            self.flash = Some((pos, 1.0));
        }
        match token {
            "C" => {
                self.display = String::from("0");
                self.acc = None;
                self.op = None;
                self.clear_next = false;
            }
            "+" | "-" | "*" | "/" => {
                self.eval_pending();
                self.acc = Some(self.display.parse::<i64>().unwrap_or(0));
                self.op = token.chars().next();
                self.clear_next = true;
            }
            "=" => {
                self.eval_pending();
                self.op = None;
                self.acc = None;
                self.clear_next = true;
            }
            digit => {
                if self.clear_next || self.display == "0" || self.display == "ERR" {
                    self.display = digit.to_string();
                    self.clear_next = false;
                } else if self.display.len() < 16 {
                    self.display.push_str(digit);
                }
            }
        }
    }

    fn eval_pending(&mut self) {
        let Some(lhs) = self.acc else { return };
        let Some(op) = self.op else { return };
        let rhs = self.display.parse::<i64>().unwrap_or(0);
        let res = match op {
            '+' => Some(lhs.saturating_add(rhs)),
            '-' => Some(lhs.saturating_sub(rhs)),
            '*' => Some(lhs.saturating_mul(rhs)),
            '/' => lhs.checked_div(rhs),
            _ => Some(rhs),
        };
        self.display = match res {
            Some(v) => v.to_string(),
            None => String::from("ERR"),
        };
    }

    pub fn on_key(&mut self, key: KeyEvent) {
        match key {
            KeyEvent::Char(c) if c.is_ascii_digit() => {
                let mut buf = [0u8; 1];
                let s = c.encode_utf8(&mut buf);
                self.press(s);
            }
            KeyEvent::Char('+') => self.press("+"),
            KeyEvent::Char('-') => self.press("-"),
            KeyEvent::Char('*') => self.press("*"),
            KeyEvent::Char('/') => self.press("/"),
            KeyEvent::Char('=') | KeyEvent::Enter => self.press("="),
            KeyEvent::Char('c') | KeyEvent::Char('C') => self.press("C"),
            KeyEvent::Backspace => {
                if self.display.len() > 1 {
                    self.display.pop();
                } else {
                    self.display = String::from("0");
                }
            }
            _ => {}
        }
    }

    pub fn on_click(&mut self, lx: i32, ly: i32, ww: i32, wh: i32) {
        let gx = 12;
        let gy = 82;
        let gw = ww - 24;
        let gh = wh - 94;
        if lx < gx || ly < gy || lx >= gx + gw || ly >= gy + gh {
            return;
        }
        let col_w = gw / 4;
        let row_h = gh / 4;
        let col = ((lx - gx) / col_w).clamp(0, 3);
        let row = ((ly - gy) / row_h).clamp(0, 3);
        let idx = (row * 4 + col) as usize;
        if let Some(tok) = CALC_BUTTONS.get(idx) {
            self.press(tok);
        }
    }

    pub fn draw(&self, canvas: &mut Canvas, wx: i32, wy: i32, ww: i32, wh: i32, accent: u32) {
        let x = wx + 12;
        let y = wy + 34;
        let iw = ww - 24;
        canvas.fill_rounded_rect(x, y, iw, 42, 10, 0x090D14);
        canvas.draw_rounded_rect_alpha(x, y, iw, 42, 10, 0xFFFFFF, 26);
        if let (Some(a), Some(op)) = (self.acc, self.op) {
            canvas.draw_text_clipped(x + 12, y + 3, iw - 24, &format!("{a} {op}"), 0x79C0FF, 1);
        }
        canvas.draw_text_clipped(x + 12, y + 15, iw - 24, &self.display, 0xFFFFFF, 2);

        let gy = wy + 82;
        let gh = wh - 94;
        let col_w = iw / 4;
        let row_h = gh / 4;

        for (idx, tok) in CALC_BUTTONS.iter().enumerate() {
            let col = (idx % 4) as i32;
            let row = (idx / 4) as i32;
            let bx = x + col * col_w + 2;
            let by = gy + row * row_h + 2;
            let bw = col_w - 4;
            let bh = row_h - 4;
            let (base_bg, border) = match *tok {
                "=" => (accent, 0x93C5FD),
                "C" => (0x9F1239, 0xFB7185),
                "+" | "-" | "*" | "/" => (0x1B2333, accent),
                _ => (0x1A202E, 0x475569),
            };
            let bg = if let Some((f_idx, fv)) = self.flash
                && f_idx == idx
            {
                lerp_rgb(base_bg, 0xFFFFFF, (fv * 90.0) as u32, 255)
            } else {
                base_bg
            };
            canvas.draw_button(bx, by, bw, bh, tok, bg, 0xFFFFFF, border);
        }
    }
}

// ---------------------------------------------------------------------------
// 7. Matrix Rain App
// ---------------------------------------------------------------------------
pub struct MatrixState {
    pub drops: Vec<(i32, i32, i32)>, // (y_px, speed, len)
    pub color_idx: usize,
    rng: u64,
}

const MATRIX_COLORS: [u32; 4] = [0x3FB950, 0x38BDF8, 0xF85149, 0xD29922];

impl MatrixState {
    pub fn new() -> Self {
        let mut m = Self {
            drops: Vec::new(),
            color_idx: 0,
            rng: 0xC0FF_EE00_DEAD_BEEF,
        };
        m.init(34);
        m
    }

    fn next_rand(&mut self) -> u64 {
        let mut x = self.rng;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.rng = x;
        x
    }

    fn init(&mut self, cols: usize) {
        self.drops.clear();
        for _ in 0..cols {
            let r = self.next_rand();
            self.drops.push((
                (r % 180) as i32,
                3 + ((r >> 8) % 5) as i32,
                5 + ((r >> 16) % 7) as i32,
            ));
        }
    }

    pub fn step(&mut self, max_h: i32) {
        for drop in &mut self.drops {
            drop.0 += drop.1;
            if drop.0 - drop.2 * 12 > max_h {
                drop.0 = 0;
            }
        }
    }

    pub fn cycle_color(&mut self) {
        self.color_idx = (self.color_idx + 1) % MATRIX_COLORS.len();
    }

    #[allow(clippy::too_many_arguments)]
    pub fn draw(&self, canvas: &mut Canvas, wx: i32, wy: i32, ww: i32, wh: i32, tick: u64, lang: Lang) {
        let x = wx + 10;
        let y = wy + 34;
        let iw = ww - 20;
        let ih = wh - 70;
        canvas.fill_rounded_rect(x, y, iw, ih, 10, 0x050A06);
        canvas.draw_rounded_rect_alpha(x, y, iw, ih, 10, 0xFFFFFF, 26);

        let glyphs = ['0', '1', 'P', 'Z', 'Я', 'Ж', 'Ф', '@', '#', '7'];
        let base_col = MATRIX_COLORS[self.color_idx];
        let cols = (iw / 10).max(1) as usize;
        for col in 0..cols.min(self.drops.len()) {
            let (head_y, _, len) = self.drops[col];
            for k in 0..len {
                let py = y + 4 + head_y - k * 12;
                if py >= y + 4 && py + 12 < y + ih - 2 {
                    let g_idx = ((tick as usize) / 3 + col * 7 + k as usize) % glyphs.len();
                    let col_c = if k == 0 {
                        0xFFFFFF
                    } else {
                        lerp_rgb(base_col, 0x050A06, k as u32, len as u32)
                    };
                    canvas.draw_char(x + 6 + (col as i32) * 10, py, glyphs[g_idx], col_c, 1);
                }
            }
        }

        canvas.draw_button(
            wx + 12,
            wy + wh - 30,
            130,
            22,
            tr(lang, "Сменить цвет", "Cycle Color"),
            0x21262D,
            0xFFFFFF,
            base_col,
        );
    }
}
