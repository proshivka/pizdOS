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

    pub fn draw(&self, canvas: &mut Canvas, wx: i32, wy: i32, ww: i32, wh: i32) {
        let x = wx + 10;
        let y = wy + 34;
        let iw = ww - 20;
        let ih = wh - 44;
        canvas.fill_rounded_rect(x, y, iw, ih, 6, 0x090C10);
        canvas.draw_rounded_rect(x, y, iw, ih, 6, 0x30363D);

        let max_lines = ((ih - 28) / 15).max(1) as usize;
        let skip = self.lines.len().saturating_sub(max_lines);
        for (i, line) in self.lines.iter().skip(skip).enumerate() {
            canvas.draw_text_clipped(x + 8, y + 6 + (i as i32) * 15, iw - 16, line, 0x3FB950, 1);
        }

        let prompt_y = y + ih - 22;
        canvas.fill_rounded_rect(x + 2, prompt_y - 2, iw - 4, 20, 4, 0x161B22);
        canvas.draw_text_clipped(
            x + 8,
            prompt_y + 2,
            iw - 16,
            &format!("pizdOS# {}_", self.input),
            0xFFFFFF,
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

pub struct PaintState {
    pub buf: Vec<u32>,
    pub color_idx: usize,
    pub brush: i32,
}

impl PaintState {
    pub fn new() -> Self {
        Self {
            buf: vec![0x0D1117; PAINT_W * PAINT_H],
            color_idx: 1,
            brush: 3,
        }
    }

    pub fn paint_at(&mut self, lx: i32, ly: i32) {
        let cx = lx - 14;
        let cy = ly - 36;
        if cx < 0 || cy < 0 || cx >= PAINT_W as i32 || cy >= PAINT_H as i32 {
            return;
        }
        let color = PALETTE[self.color_idx];
        let r = self.brush / 2;
        for dy in -r..=r {
            for dx in -r..=r {
                let px = cx + dx;
                let py = cy + dy;
                if px >= 0 && py >= 0 && px < PAINT_W as i32 && py < PAINT_H as i32 {
                    self.buf[(py as usize) * PAINT_W + (px as usize)] = color;
                }
            }
        }
    }

    pub fn on_click(&mut self, lx: i32, ly: i32, wh: i32) {
        if ly >= wh - 36 && ly <= wh - 10 {
            for i in 0..PALETTE.len() {
                let bx = 14 + (i as i32) * 26;
                if lx >= bx && lx <= bx + 20 {
                    self.color_idx = i;
                    return;
                }
            }
            if (210..=270).contains(&lx) {
                self.brush = if self.brush == 3 { 6 } else { 3 };
                return;
            }
            if (278..=350).contains(&lx) {
                self.buf.fill(0x0D1117);
                return;
            }
        }
        self.paint_at(lx, ly);
    }

    pub fn draw(&self, canvas: &mut Canvas, wx: i32, wy: i32, _ww: i32, wh: i32, lang: Lang) {
        let ox = wx + 14;
        let oy = wy + 36;
        for py in 0..PAINT_H {
            for px in 0..PAINT_W {
                canvas.set_pixel(ox + px as i32, oy + py as i32, self.buf[py * PAINT_W + px]);
            }
        }
        canvas.draw_rect(ox - 1, oy - 1, PAINT_W as i32 + 2, PAINT_H as i32 + 2, 0x30363D);

        let by = wy + wh - 34;
        for (i, &col) in PALETTE.iter().enumerate() {
            let bx = wx + 14 + (i as i32) * 26;
            canvas.fill_rounded_rect(bx, by, 20, 22, 4, col);
            let border = if i == self.color_idx { 0xFFFFFF } else { 0x30363D };
            canvas.draw_rounded_rect(bx, by, 20, 22, 4, border);
        }
        let b_lbl = if self.brush == 3 { "1x" } else { "2x" };
        canvas.draw_button(wx + 210, by, 60, 22, b_lbl, 0x21262D, 0xFFFFFF, 0x58A6FF);
        canvas.draw_button(
            wx + 278,
            by,
            72,
            22,
            tr(lang, "Очистить", "Clear"),
            0xB62324,
            0xFFFFFF,
            0xFF7B72,
        );
    }
}

// ---------------------------------------------------------------------------
// 3. Zmeyka (Snake)
// ---------------------------------------------------------------------------
const SNAKE_COLS: i32 = 20;
const SNAKE_ROWS: i32 = 14;

pub struct SnakeState {
    pub body: Vec<(i32, i32)>,
    pub dir: (i32, i32),
    pub food: (i32, i32),
    pub score: u32,
    pub game_over: bool,
    rng: u64,
}

impl SnakeState {
    pub fn new() -> Self {
        let mut s = Self {
            body: Vec::new(),
            dir: (1, 0),
            food: (12, 7),
            score: 0,
            game_over: false,
            rng: 0x1337_BEEF_CAFE_0001,
        };
        s.reset();
        s
    }

    pub fn reset(&mut self) {
        self.body = vec![(6, 7), (5, 7), (4, 7)];
        self.dir = (1, 0);
        self.food = (14, 7);
        self.score = 0;
        self.game_over = false;
    }

    fn next_rand(&mut self) -> i32 {
        let mut x = self.rng;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.rng = x;
        (x & 0x7FFF) as i32
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

    pub fn step(&mut self) {
        if self.game_over || self.body.is_empty() {
            return;
        }
        let head = self.body[0];
        let nx = head.0 + self.dir.0;
        let ny = head.1 + self.dir.1;
        if nx < 0 || ny < 0 || nx >= SNAKE_COLS || ny >= SNAKE_ROWS || self.body.contains(&(nx, ny))
        {
            self.game_over = true;
            return;
        }
        self.body.insert(0, (nx, ny));
        if (nx, ny) == self.food {
            self.score += 10;
            let fx = self.next_rand() % SNAKE_COLS;
            let fy = self.next_rand() % SNAKE_ROWS;
            self.food = (fx, fy);
        } else {
            self.body.pop();
        }
    }

    pub fn on_click(&mut self, lx: i32, ly: i32, wh: i32) {
        if ly >= wh - 34 && ly <= wh - 10 && (14..=124).contains(&lx) {
            self.reset();
        }
    }

    pub fn draw(&self, canvas: &mut Canvas, wx: i32, wy: i32, _ww: i32, wh: i32, lang: Lang) {
        let gx = wx + 18;
        let gy = wy + 36;
        let cell = 14;
        canvas.fill_rounded_rect(gx, gy, SNAKE_COLS * cell, SNAKE_ROWS * cell, 6, 0x090C10);
        canvas.draw_rounded_rect(
            gx - 1,
            gy - 1,
            SNAKE_COLS * cell + 2,
            SNAKE_ROWS * cell + 2,
            6,
            0x30363D,
        );

        canvas.fill_rounded_rect(
            gx + self.food.0 * cell + 2,
            gy + self.food.1 * cell + 2,
            cell - 4,
            cell - 4,
            4,
            0xF85149,
        );

        for (i, &(sx, sy)) in self.body.iter().enumerate() {
            let col = if i == 0 { 0x56D364 } else { 0x2EA043 };
            canvas.fill_rounded_rect(
                gx + sx * cell + 1,
                gy + sy * cell + 1,
                cell - 2,
                cell - 2,
                3,
                col,
            );
        }

        if self.game_over {
            canvas.draw_text(
                gx + 60,
                gy + 88,
                tr(lang, "ПИЗДЕЦ! ИГРА ОКОНЧЕНА", "GAME OVER!"),
                0xFF7B72,
                1,
            );
        }

        let by = wy + wh - 34;
        canvas.draw_button(
            wx + 14,
            by,
            110,
            22,
            tr(lang, "Новая игра", "New Game"),
            0x238636,
            0xFFFFFF,
            0x3FB950,
        );
        let score_lbl = match lang {
            Lang::Ru => format!("Очки: {} (WASD/Стрл)", self.score),
            Lang::En => format!("Score: {} (WASD/Arrows)", self.score),
        };
        canvas.draw_text(wx + 134, by + 5, &score_lbl, 0xE6EDF3, 1);
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

    pub fn on_click(&mut self, lx: i32, ly: i32, wh: i32, lang: Lang) {
        if ly >= wh - 34 && ly <= wh - 10 {
            if (14..=114).contains(&lx) {
                let text = self.lines.join("\n");
                let path = std::env::temp_dir().join("pizpad_note.txt");
                self.saved_msg = match fs::write(&path, text) {
                    Ok(()) => String::from(tr(lang, "Сохранено в pizpad_note.txt", "Saved to pizpad_note.txt")),
                    Err(e) => format!("Err: {e}"),
                };
            } else if (122..=212).contains(&lx) {
                self.lines = vec![String::new()];
                self.saved_msg = String::from(tr(lang, "Очищено!", "Cleared!"));
            }
        }
    }

    pub fn draw(&self, canvas: &mut Canvas, wx: i32, wy: i32, ww: i32, wh: i32, lang: Lang) {
        let x = wx + 12;
        let y = wy + 34;
        let iw = ww - 24;
        let ih = wh - 76;
        canvas.fill_rounded_rect(x, y, iw, ih, 6, 0x0D1117);
        canvas.draw_rounded_rect(x, y, iw, ih, 6, 0x30363D);

        let max_lines = ((ih - 8) / 15).max(1) as usize;
        for (i, line) in self.lines.iter().take(max_lines).enumerate() {
            let cursor = if i + 1 == self.lines.len() { "_" } else { "" };
            canvas.draw_text_clipped(
                x + 8,
                y + 6 + (i as i32) * 15,
                iw - 16,
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
    iw: i32,
    theme: ThemePreset,
    accent: u32,
    hero: Vec<u32>,
    mini: Vec<u32>,
}

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
    pub glass_level: u8,
    pub crosshair_cursor: bool,
    pub show_clock: bool,
    pub snake_tick_div: u64,
    pub device_name: String,
    pub editing_device: bool,
    pub update_taps: u8,
    pub license_alt: bool,
    pub tab_pill_y: f32,
    pub toggle_anim: [f32; 7],
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
            glass_level: 0,
            crosshair_cursor: false,
            show_clock: true,
            snake_tick_div: 10,
            device_name: String::from("pizdOS PC"),
            editing_device: false,
            update_taps: 0,
            license_alt: false,
            tab_pill_y: 90.0,
            toggle_anim: [0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 1.0],
            hero_cache: RefCell::new(None),
        }
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
        ];
        if !self.animations {
            self.tab_pill_y = target_pill;
            for (slot, &on) in self.toggle_anim.iter_mut().zip(targets.iter()) {
                *slot = if on { 1.0 } else { 0.0 };
            }
            return;
        }
        let diff = target_pill - self.tab_pill_y;
        if diff.abs() < 0.25 {
            self.tab_pill_y = target_pill;
        } else {
            self.tab_pill_y += diff * 0.28;
        }
        for (slot, &on) in self.toggle_anim.iter_mut().zip(targets.iter()) {
            let tgt = if on { 1.0 } else { 0.0 };
            let d = tgt - *slot;
            if d.abs() < 0.015 {
                *slot = tgt;
            } else {
                *slot += d * 0.30;
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
        let mut cache_ref = self.hero_cache.borrow_mut();
        let valid = cache_ref
            .as_ref()
            .is_some_and(|c| c.iw == cw && c.theme == self.theme && c.accent == accent);
        if !valid {
            let (wp_tl, wp_br, wp_a, wp_b, _, wp_rib) = self.theme.mesh_palette();
            let cw_u = cw as usize;
            let mut hc = Canvas::new(cw_u, 90);
            hc.pixels.fill(0x131720);
            hc.fill_luminous_card(
                0,
                0,
                cw,
                90,
                14,
                0x101521,
                lerp_rgb(0x161E30, accent, 52, 255),
                accent,
                wp_rib,
            );
            let mut mc = Canvas::new(32, 52);
            mc.pixels.fill(0x161B26);
            mc.fill_luminous_card(0, 0, 32, 52, 6, wp_tl, wp_br, wp_a, wp_b);
            *cache_ref = Some(HeroCache {
                iw: cw,
                theme: self.theme,
                accent,
                hero: hc.pixels,
                mini: mc.pixels,
            });
        }
        if let Some(c) = cache_ref.as_ref() {
            canvas.blit_rgb_rect(x, y, cw, 90, &c.hero);
            canvas.blit_rgb_rect(scr_x, scr_y, 32, 52, &c.mini);
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
            match idx {
                0 => self.page = SettingsPage::Display,
                1 => self.page = SettingsPage::Taskbar,
                2 => self.page = SettingsPage::System,
                3 => self.page = SettingsPage::About,
                _ => {}
            }
            return SettingsAction::None;
        }

        if lx < 146 {
            return SettingsAction::None;
        }

        let iw = ww - 158;
        match self.page {
            SettingsPage::Display => {
                self.editing_device = false;
                if (60..94).contains(&ly) {
                    self.theme = match self.theme {
                        ThemePreset::AuroraPeaks => ThemePreset::CrimsonSunset,
                        ThemePreset::CrimsonSunset => ThemePreset::MatrixGlow,
                        ThemePreset::MatrixGlow => ThemePreset::OceanGlass,
                        ThemePreset::OceanGlass => ThemePreset::AuroraPeaks,
                    };
                } else if (94..128).contains(&ly) {
                    self.lang = match self.lang {
                        Lang::Ru => Lang::En,
                        Lang::En => Lang::Ru,
                    };
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
                } else if (256..=302).contains(&ly) && iw >= 160 {
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
        let vw = crate::font::measure_text(value, 1).min(max_vw);
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
                    in_content && (60..94).contains(&hover_ly),
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
                    in_content && (94..128).contains(&hover_ly),
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
                let sw_y = wy + 256;
                let sw_h = (wy + wh - 10 - sw_y).clamp(32, 46);
                if sw_y + 32 <= wy + wh - 8 && iw >= 160 {
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
                                (sx + sw_w - 11) as f32,
                                (sw_y + 11) as f32,
                                3.2,
                                0xFFFFFF,
                                240,
                            );
                        }
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
                Self::draw_card_frame(canvas, x, c_y, iw, 170, 12);
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

                let tip_y = wy + 240;
                if tip_y + 46 <= wy + wh - 8 {
                    Self::draw_card_frame(canvas, x, tip_y, iw, 46, 12);
                    canvas.fill_aa_circle((x + 19) as f32, (tip_y + 23) as f32, 6.0, accent, 36);
                    canvas.fill_aa_circle((x + 19) as f32, (tip_y + 23) as f32, 2.5, accent, 240);
                    canvas.draw_text_clipped(
                        x + 32,
                        tip_y + 8,
                        iw - 44,
                        tr(lang, "Управление окнами", "Window Controls"),
                        0xF1F5F9,
                        1,
                    );
                    canvas.draw_text_clipped(
                        x + 32,
                        tip_y + 25,
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
                    tr(lang, "Скорость Змейки", "Snake Speed"),
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
                let title_w = crate::font::measure_text("pizdOS", 2);
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
        }
    }

    pub fn press(&mut self, token: &str) {
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
        canvas.fill_rounded_rect(x, y, iw, 42, 8, 0x0B0E14);
        canvas.draw_rounded_rect_alpha(x, y, iw, 42, 8, 0xFFFFFF, 22);
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
            let (bg, border) = match *tok {
                "=" => (accent, 0x79C0FF),
                "C" => (0xB62324, 0xFF7B72),
                "+" | "-" | "*" | "/" => (0x1F2937, 0x388BFD),
                _ => (0x21262D, 0x30363D),
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
        canvas.fill_rounded_rect(x, y, iw, ih, 6, 0x050A06);
        canvas.draw_rounded_rect(x, y, iw, ih, 6, 0x30363D);

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
