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

        for (i, line) in self.lines.iter().take(12).enumerate() {
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

    pub fn render_wallpaper(self, buf: &mut [u32], w: usize, h: usize, show_grid: bool) {
        if w == 0 || h == 0 {
            return;
        }
        // Minimalist silk mesh gradient palettes: base_tl, base_br, blob_a, blob_b, blob_c, ribbon
        let (base_tl, base_br, blob_a, blob_b, blob_c, ribbon) = match self {
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
        };

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

pub struct SettingsState {
    pub page: SettingsPage,
    pub lang: Lang,
    pub theme: ThemePreset,
    pub show_grid: bool,
    pub show_icons: bool,
    pub taskbar_top: bool,
    pub crosshair_cursor: bool,
    pub snake_tick_div: u64,
}

impl SettingsState {
    pub fn new() -> Self {
        Self {
            page: SettingsPage::About,
            lang: Lang::Ru,
            theme: ThemePreset::AuroraPeaks,
            show_grid: false,
            show_icons: true,
            taskbar_top: false,
            crosshair_cursor: false,
            snake_tick_div: 10,
        }
    }

    pub fn on_click(&mut self, lx: i32, ly: i32) -> bool {
        // Left sidebar (x: 10..136)
        if (10..=136).contains(&lx) && ly >= 36 {
            let idx = (ly - 36) / 30;
            match idx {
                0 => self.page = SettingsPage::Display,
                1 => self.page = SettingsPage::Taskbar,
                2 => self.page = SettingsPage::System,
                3 => self.page = SettingsPage::About,
                _ => {}
            }
            return false;
        }

        if lx < 146 {
            return false;
        }

        match self.page {
            SettingsPage::Display => {
                if (68..=94).contains(&ly) {
                    self.theme = match self.theme {
                        ThemePreset::AuroraPeaks => ThemePreset::CrimsonSunset,
                        ThemePreset::CrimsonSunset => ThemePreset::MatrixGlow,
                        ThemePreset::MatrixGlow => ThemePreset::OceanGlass,
                        ThemePreset::OceanGlass => ThemePreset::AuroraPeaks,
                    };
                } else if (112..=138).contains(&ly) {
                    self.lang = match self.lang {
                        Lang::Ru => Lang::En,
                        Lang::En => Lang::Ru,
                    };
                } else if (156..=182).contains(&ly) {
                    self.show_grid = !self.show_grid;
                } else if (200..=226).contains(&ly) {
                    self.show_icons = !self.show_icons;
                }
            }
            SettingsPage::Taskbar => {
                if (68..=94).contains(&ly) {
                    self.taskbar_top = !self.taskbar_top;
                } else if (112..=138).contains(&ly) {
                    self.crosshair_cursor = !self.crosshair_cursor;
                }
            }
            SettingsPage::System => {
                if (68..=94).contains(&ly) {
                    self.snake_tick_div = match self.snake_tick_div {
                        14 => 10,
                        10 => 5,
                        _ => 14,
                    };
                } else if (192..=218).contains(&ly) {
                    return true;
                }
            }
            SettingsPage::About => {}
        }
        false
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
    ) {
        let lang = self.lang;
        let accent = self.theme.accent();

        // Minimalist flat sidebar
        let sb_x = wx + 10;
        let sb_y = wy + 34;
        let sb_w = 126;
        let sb_h = wh - 44;
        canvas.fill_rounded_rect(sb_x, sb_y, sb_w, sb_h, 8, 0x11151E);
        canvas.draw_rounded_rect_alpha(sb_x, sb_y, sb_w, sb_h, 8, 0xFFFFFF, 18);

        let tabs = [
            (SettingsPage::Display, tr(lang, "Экран и язык", "Display & Lang")),
            (SettingsPage::Taskbar, tr(lang, "Панель задач", "Taskbar")),
            (SettingsPage::System, tr(lang, "Система", "System")),
            (SettingsPage::About, tr(lang, "О системе", "About OS")),
        ];
        for (i, (p, label)) in tabs.iter().enumerate() {
            let ty = sb_y + 6 + (i as i32) * 30;
            let active = self.page == *p;
            if active {
                canvas.blend_rounded_rect(sb_x + 5, ty, sb_w - 10, 25, 6, accent, 42);
                canvas.fill_rounded_rect(sb_x + 7, ty + 6, 3, 13, 1, accent);
            }
            let fg = if active { 0xF8FAFC } else { 0x94A3B8 };
            canvas.draw_text_clipped(sb_x + 15, ty + 6, sb_w - 22, label, fg, 1);
        }

        let x = wx + 146;
        let y = wy + 36;
        let iw = ww - 158;

        match self.page {
            SettingsPage::Display => {
                canvas.draw_text_clipped(
                    x,
                    y,
                    iw,
                    tr(lang, "Экран, обои и язык", "Display, Wallpaper & Lang"),
                    0xFFFFFF,
                    1,
                );
                canvas.draw_button(
                    x,
                    wy + 68,
                    iw,
                    26,
                    &format!("{}: {}", tr(lang, "Обои", "Wallpaper"), self.theme.name(lang)),
                    0x21262D,
                    0xFFFFFF,
                    accent,
                );
                canvas.draw_button(
                    x,
                    wy + 112,
                    iw,
                    26,
                    tr(lang, "Язык: Русский (RU)", "Language: English (EN)"),
                    0x21262D,
                    0xFFFFFF,
                    accent,
                );

                // Toggle rows
                canvas.fill_rounded_rect(x, wy + 156, iw, 26, 5, 0x21262D);
                canvas.draw_text_clipped(
                    x + 10,
                    wy + 163,
                    iw - 56,
                    tr(lang, "Сетка поверх обоев", "Wallpaper Grid"),
                    0xE6EDF3,
                    1,
                );
                canvas.draw_toggle(x + iw - 44, wy + 160, self.show_grid, accent);

                canvas.fill_rounded_rect(x, wy + 200, iw, 26, 5, 0x21262D);
                canvas.draw_text_clipped(
                    x + 10,
                    wy + 207,
                    iw - 56,
                    tr(lang, "Иконки на рабочем столе", "Desktop Icons"),
                    0xE6EDF3,
                    1,
                );
                canvas.draw_toggle(x + iw - 44, wy + 204, self.show_icons, accent);
            }
            SettingsPage::Taskbar => {
                canvas.draw_text_clipped(
                    x,
                    y,
                    iw,
                    tr(lang, "Панель задач и курсор", "Taskbar & Cursor"),
                    0xFFFFFF,
                    1,
                );
                canvas.fill_rounded_rect(x, wy + 68, iw, 26, 5, 0x21262D);
                canvas.draw_text_clipped(
                    x + 10,
                    wy + 75,
                    iw - 56,
                    tr(lang, "Панель задач сверху", "Taskbar on Top"),
                    0xE6EDF3,
                    1,
                );
                canvas.draw_toggle(x + iw - 44, wy + 72, self.taskbar_top, accent);

                canvas.fill_rounded_rect(x, wy + 112, iw, 26, 5, 0x21262D);
                canvas.draw_text_clipped(
                    x + 10,
                    wy + 119,
                    iw - 56,
                    tr(lang, "Курсор-прицел", "Crosshair Cursor"),
                    0xE6EDF3,
                    1,
                );
                canvas.draw_toggle(x + iw - 44, wy + 116, self.crosshair_cursor, accent);
            }
            SettingsPage::System => {
                canvas.draw_text_clipped(
                    x,
                    y,
                    iw,
                    tr(lang, "Производительность", "Performance & Metrics"),
                    0xFFFFFF,
                    1,
                );
                let spd = match (self.snake_tick_div, lang) {
                    (14, Lang::Ru) => "Скорость Змейки: Медленно",
                    (14, Lang::En) => "Snake Speed: Slow",
                    (5, Lang::Ru) => "Скорость Змейки: Турбо",
                    (5, Lang::En) => "Snake Speed: Turbo",
                    (_, Lang::Ru) => "Скорость Змейки: Норма",
                    (_, Lang::En) => "Snake Speed: Normal",
                };
                canvas.draw_button(x, wy + 68, iw, 26, spd, 0x21262D, 0xFFFFFF, accent);
                canvas.draw_text_clipped(x, wy + 114, iw, &format!("CPU: {cpu_pct}%"), 0x79C0FF, 1);
                canvas.draw_text_clipped(
                    x,
                    wy + 134,
                    iw,
                    &format!("RAM: {} / {} MB", mem.used_mb, mem.total_mb),
                    0xD29922,
                    1,
                );
                canvas.draw_text_clipped(x, wy + 154, iw, &format!("Uptime: {uptime}"), 0x3FB950, 1);
                canvas.draw_button(
                    x,
                    wy + 192,
                    150,
                    26,
                    tr(lang, "Обновить метрики", "Refresh Metrics"),
                    0x238636,
                    0xFFFFFF,
                    0x3FB950,
                );
            }
            SettingsPage::About => {
                // Minimalist ColorOS-inspired Dark Hero Banner Card
                let card_right = lerp_rgb(0x161B26, accent, 38, 255);
                canvas.fill_hgradient_rounded_rect(x, y, iw, 78, 10, 0x131720, card_right);
                canvas.fill_rounded_rect(x + 6, y + 16, 3, 46, 1, accent);
                canvas.draw_rounded_rect_alpha(x, y, iw, 78, 10, 0xFFFFFF, 26);

                canvas.draw_text_clipped(x + 18, y + 10, iw - 114, "pizdOS 2.0", 0xF8FAFC, 2);
                canvas.draw_text_clipped(
                    x + 18,
                    y + 39,
                    iw - 114,
                    "Bratukha Edition • Pizsoft UI",
                    0x94A3B8,
                    1,
                );
                canvas.draw_text_clipped(
                    x + 18,
                    y + 56,
                    iw - 114,
                    "by prosh1vka, Lowyk, KiryxaTipa",
                    0x6EE7B7,
                    1,
                );

                // Clean capsule emblem badge on right of Hero Card
                if iw > 260 {
                    let bx = x + iw - 92;
                    let by = y + 25;
                    canvas.blend_rounded_rect(bx, by, 80, 26, 13, 0x0A0D13, 195);
                    canvas.draw_rounded_rect_alpha(bx, by, 80, 26, 13, 0xFFFFFF, 34);
                    canvas.fill_aa_circle((bx + 14) as f32, (by + 13) as f32, 3.5, 0x3FB950, 255);
                    canvas.draw_text_clipped(bx + 23, by + 6, 54, "NO GNU", 0xF1F5F9, 1);
                }

                // Minimalist 2-Column Info Tiles
                let col_w = (iw - 8) / 2;
                let ram_str = format!("{} MB Total", mem.total_mb);
                let tiles = [
                    (tr(lang, "Стек", "Stack"), "musl + BusyBox", 0x58A6FF),
                    (tr(lang, "Принцип", "Policy"), "NO GNU / 0 Deps", 0x3FB950),
                    (tr(lang, "Лицензия", "License"), "MIT (GNU Nahuy)", 0xF59E0B),
                    (tr(lang, "Ядро", "Kernel"), kernel, 0xA855F7),
                    ("RAM", ram_str.as_str(), 0x38BDF8),
                    (tr(lang, "Пакеты", "Pkg Mgr"), "piz-get 0.1.0", 0xF43F5E),
                ];

                for (idx, (label, val, dot_col)) in tiles.iter().enumerate() {
                    let col = (idx % 2) as i32;
                    let row = (idx / 2) as i32;
                    let tx = x + col * (col_w + 8);
                    let ty = y + 88 + row * 50;
                    if ty + 44 > wy + wh - 10 {
                        break;
                    }
                    canvas.fill_rounded_rect(tx, ty, col_w, 44, 8, 0x171C27);
                    canvas.draw_rounded_rect_alpha(tx, ty, col_w, 44, 8, 0xFFFFFF, 18);
                    canvas.fill_aa_circle((tx + 12) as f32, (ty + 13) as f32, 2.8, *dot_col, 255);
                    canvas.draw_text_clipped(tx + 20, ty + 6, col_w - 26, label, 0x8B949E, 1);
                    canvas.draw_text_clipped(tx + 11, ty + 23, col_w - 18, val, 0xF1F5F9, 1);
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
