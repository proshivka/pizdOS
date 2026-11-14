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
        let (top_c, mid_c, bot_c, ridge1_c, ridge2_c, orb_c) = match self {
            ThemePreset::AuroraPeaks => (0x0A1024, 0x162447, 0x1F4068, 0x111C33, 0x0B1220, 0x2DD4BF),
            ThemePreset::CrimsonSunset => (0x1B0B1E, 0x4A152B, 0x8F2D36, 0x28101C, 0x14080F, 0xF59E0B),
            ThemePreset::MatrixGlow => (0x040E08, 0x082013, 0x0E3B21, 0x07190E, 0x030B06, 0x3FB950),
            ThemePreset::OceanGlass => (0x081C2E, 0x0E3A53, 0x155E75, 0x0B2538, 0x061522, 0x38BDF8),
        };

        let h_u32 = h as u32;
        let w_i32 = w as i32;
        let h_i32 = h as i32;
        let orb_x = (w_i32 * 2) / 3;
        let orb_y = h_i32 / 3;
        let orb_r = (h_i32 / 2).max(80);
        let orb_r2 = orb_r * orb_r;

        for y in 0..h {
            let y_u32 = y as u32;
            let base = if y * 2 < h {
                lerp_rgb(top_c, mid_c, y_u32 * 2, h_u32)
            } else {
                lerp_rgb(mid_c, bot_c, (y_u32 - h_u32 / 2) * 2, h_u32)
            };

            // Mountain ridge profiles
            let y_i = y as i32;
            let row_off = y * w;
            for x in 0..w {
                let x_i = x as i32;

                // Soft glowing sun/aurora orb
                let dx = x_i - orb_x;
                let dy = y_i - orb_y;
                let d2 = dx * dx + dy * dy;
                let mut px = if d2 < orb_r2 {
                    let strength = ((orb_r2 - d2) * 110 / orb_r2) as u32;
                    lerp_rgb(base, orb_c, strength, 255)
                } else {
                    base
                };

                // Layered mountain silhouettes
                let m1_h = h_i32 - 80 - ((x_i * 37 / 60 - 10).abs() % 90) - ((x_i / 13) % 18);
                let m2_h = h_i32 - 42 - ((x_i * 53 / 80 + 25).abs() % 70);
                if y_i >= m2_h {
                    px = ridge2_c;
                } else if y_i >= m1_h {
                    px = ridge1_c;
                } else {
                    // Subtle stars in upper sky
                    let hash = (x as u32).wrapping_mul(73856093) ^ (y as u32).wrapping_mul(19349663);
                    if y * 2 < h && (hash & 0x3FF) == 0 {
                        px = lerp_rgb(px, 0xFFFFFF, 160, 255);
                    }
                }

                if show_grid && (x % 40 == 0 || y % 40 == 0) {
                    px = lerp_rgb(px, 0xFFFFFF, 14, 255);
                }

                buf[row_off + x] = px;
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

        // Sidebar
        let sb_x = wx + 10;
        let sb_y = wy + 34;
        let sb_w = 126;
        let sb_h = wh - 44;
        canvas.fill_rounded_rect(sb_x, sb_y, sb_w, sb_h, 6, 0x0D1117);
        canvas.draw_rounded_rect(sb_x, sb_y, sb_w, sb_h, 6, 0x30363D);

        let tabs = [
            (SettingsPage::Display, tr(lang, "Экран и язык", "Display & Lang")),
            (SettingsPage::Taskbar, tr(lang, "Панель задач", "Taskbar")),
            (SettingsPage::System, tr(lang, "Система", "System")),
            (SettingsPage::About, tr(lang, "О системе", "About OS")),
        ];
        for (i, (p, label)) in tabs.iter().enumerate() {
            let ty = sb_y + 6 + (i as i32) * 30;
            let active = self.page == *p;
            let bg = if active { accent } else { 0x161B22 };
            let fg = if active { 0xFFFFFF } else { 0xC9D1D9 };
            canvas.fill_rounded_rect(sb_x + 5, ty, sb_w - 10, 24, 5, bg);
            canvas.draw_text_clipped(sb_x + 12, ty + 6, sb_w - 20, label, fg, 1);
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
                // ColorOS-style Hero Banner Card
                canvas.fill_rounded_rect(x, y, iw, 78, 8, 0x162436);
                canvas.fill_rounded_rect(x, y, 6, 78, 3, accent);
                canvas.draw_rounded_rect(x, y, iw, 78, 8, accent);

                canvas.draw_text_clipped(x + 16, y + 10, iw - 24, "pizdOS 2.0", 0xFFFFFF, 2);
                canvas.draw_text_clipped(
                    x + 16,
                    y + 38,
                    iw - 24,
                    "Bratukha Edition | Pizsoft UI",
                    0x79C0FF,
                    1,
                );
                canvas.draw_text_clipped(
                    x + 16,
                    y + 56,
                    iw - 24,
                    "by prosh1vka, Lowyk, KiryxaTipa",
                    0x3FB950,
                    1,
                );

                // 2-Column ColorOS Info Tiles
                let col_w = (iw - 8) / 2;
                let ram_str = format!("{} MB Total", mem.total_mb);
                let tiles = [
                    (tr(lang, "Стек", "Stack"), "musl + BusyBox"),
                    (tr(lang, "Принцип", "Policy"), "NO GNU / 0 Deps"),
                    (tr(lang, "Лицензия", "License"), "MIT (GNU Nahuy)"),
                    (tr(lang, "Ядро", "Kernel"), kernel),
                    ("RAM", ram_str.as_str()),
                    (tr(lang, "Пакеты", "Pkg Mgr"), "piz-get 0.1.0"),
                ];

                for (idx, (label, val)) in tiles.iter().enumerate() {
                    let col = (idx % 2) as i32;
                    let row = (idx / 2) as i32;
                    let tx = x + col * (col_w + 8);
                    let ty = y + 88 + row * 50;
                    if ty + 44 > wy + wh - 10 {
                        break;
                    }
                    canvas.fill_rounded_rect(tx, ty, col_w, 44, 6, 0x21262D);
                    canvas.draw_rounded_rect(tx, ty, col_w, 44, 6, 0x30363D);
                    canvas.draw_text_clipped(tx + 10, ty + 6, col_w - 18, label, 0x8B949E, 1);
                    canvas.draw_text_clipped(tx + 10, ty + 23, col_w - 18, val, 0xE6EDF3, 1);
                }
            }
        }
    }
}
