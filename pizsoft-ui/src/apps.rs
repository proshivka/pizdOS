use std::fs;
use std::process::Command;

use crate::gfx::Canvas;
use crate::sys::MemInfo;

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
    pub fn new() -> Self {
        Self {
            lines: vec![
                String::from("pizdOS 2.0 Terminal (0% GNU)"),
                String::from("Vvedi 'help', 'poyasni' ili lyubuyu komandu:"),
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
                if self.input.chars().count() < 48 {
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
                self.push_line(String::from("help, poyasni, clear/nahuy, whoami, <shell cmd>"));
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
        let y = wy + 32;
        let iw = ww - 20;
        let ih = wh - 42;
        canvas.fill_rect(x, y, iw, ih, 0x090C10);
        canvas.draw_rect(x, y, iw, ih, 0x30363D);

        let max_lines = ((ih - 26) / 14).max(1) as usize;
        let skip = self.lines.len().saturating_sub(max_lines);
        for (i, line) in self.lines.iter().skip(skip).enumerate() {
            canvas.draw_text_clipped(x + 6, y + 6 + (i as i32) * 14, iw - 12, line, 0x3FB950, 1);
        }

        let prompt_y = y + ih - 18;
        canvas.fill_rect(x + 1, prompt_y - 3, iw - 2, 18, 0x161B22);
        canvas.draw_text_clipped(
            x + 6,
            prompt_y,
            iw - 12,
            &format!("pizdOS# {}_", self.input),
            0xFFFFFF,
            1,
        );
    }
}

// ---------------------------------------------------------------------------
// 2. PizPaint
// ---------------------------------------------------------------------------
pub const PAINT_W: usize = 320;
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
        let cy = ly - 34;
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
        if ly >= wh - 34 && ly <= wh - 10 {
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
            if (278..=348).contains(&lx) {
                self.buf.fill(0x0D1117);
                return;
            }
        }
        self.paint_at(lx, ly);
    }

    pub fn draw(&self, canvas: &mut Canvas, wx: i32, wy: i32, _ww: i32, wh: i32) {
        let ox = wx + 14;
        let oy = wy + 34;
        for py in 0..PAINT_H {
            for px in 0..PAINT_W {
                canvas.set_pixel(ox + px as i32, oy + py as i32, self.buf[py * PAINT_W + px]);
            }
        }
        canvas.draw_rect(ox - 1, oy - 1, PAINT_W as i32 + 2, PAINT_H as i32 + 2, 0x30363D);

        let by = wy + wh - 34;
        for (i, &col) in PALETTE.iter().enumerate() {
            let bx = wx + 14 + (i as i32) * 26;
            canvas.fill_rect(bx, by, 20, 22, col);
            let border = if i == self.color_idx { 0xFFFFFF } else { 0x30363D };
            canvas.draw_rect(bx, by, 20, 22, border);
        }
        let b_lbl = if self.brush == 3 { "1x" } else { "2x" };
        canvas.draw_button(wx + 210, by, 60, 22, b_lbl, 0x21262D, 0xFFFFFF, 0x58A6FF);
        canvas.draw_button(wx + 278, by, 70, 22, "Clear", 0xB62324, 0xFFFFFF, 0xFF7B72);
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

    pub fn draw(&self, canvas: &mut Canvas, wx: i32, wy: i32, _ww: i32, wh: i32) {
        let gx = wx + 16;
        let gy = wy + 34;
        let cell = 14;
        canvas.fill_rect(gx, gy, SNAKE_COLS * cell, SNAKE_ROWS * cell, 0x090C10);
        canvas.draw_rect(
            gx - 1,
            gy - 1,
            SNAKE_COLS * cell + 2,
            SNAKE_ROWS * cell + 2,
            0x30363D,
        );

        // Food
        canvas.fill_rect(
            gx + self.food.0 * cell + 2,
            gy + self.food.1 * cell + 2,
            cell - 4,
            cell - 4,
            0xF85149,
        );

        // Snake
        for (i, &(sx, sy)) in self.body.iter().enumerate() {
            let col = if i == 0 { 0x56D364 } else { 0x2EA043 };
            canvas.fill_rect(gx + sx * cell + 1, gy + sy * cell + 1, cell - 2, cell - 2, col);
        }

        if self.game_over {
            canvas.draw_text(gx + 54, gy + 86, "PIZDEC! GAME OVER", 0xFF7B72, 1);
        }

        let by = wy + wh - 34;
        canvas.draw_button(wx + 14, by, 110, 22, "Novaya igra", 0x238636, 0xFFFFFF, 0x3FB950);
        canvas.draw_text(
            wx + 138,
            by + 7,
            &format!("Ochki: {}  (WASD/Strl)", self.score),
            0xE6EDF3,
            1,
        );
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
    pub fn new() -> Self {
        Self {
            lines: vec![
                String::from("=== PIZPAD v0.1 (Top Sekret) ==="),
                String::from("1. Ne trogay to, chto rabotaet."),
                String::from("2. Esli slomalos - eto ne bag, a ficha."),
                String::from("3. GNU poshel nahuy!"),
                String::new(),
            ],
            saved_msg: String::from("Pechatay pryamo syuda..."),
        }
    }

    pub fn on_key(&mut self, key: KeyEvent) {
        match key {
            KeyEvent::Char(c) => {
                if self.lines.is_empty() {
                    self.lines.push(String::new());
                }
                if let Some(last) = self.lines.last_mut()
                    && last.chars().count() < 44
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

    pub fn on_click(&mut self, lx: i32, ly: i32, wh: i32) {
        if ly >= wh - 34 && ly <= wh - 10 {
            if (14..=114).contains(&lx) {
                let text = self.lines.join("\n");
                let path = std::env::temp_dir().join("pizpad_note.txt");
                self.saved_msg = match fs::write(&path, text) {
                    Ok(()) => String::from("Sohraneno v pizpad_note.txt!"),
                    Err(e) => format!("Oshibka: {e}"),
                };
            } else if (122..=212).contains(&lx) {
                self.lines = vec![String::new()];
                self.saved_msg = String::from("Ochishcheno!");
            }
        }
    }

    pub fn draw(&self, canvas: &mut Canvas, wx: i32, wy: i32, ww: i32, wh: i32) {
        let x = wx + 12;
        let y = wy + 34;
        let iw = ww - 24;
        let ih = wh - 76;
        canvas.fill_rect(x, y, iw, ih, 0x0D1117);
        canvas.draw_rect(x, y, iw, ih, 0x30363D);

        for (i, line) in self.lines.iter().take(12).enumerate() {
            let cursor = if i + 1 == self.lines.len() { "_" } else { "" };
            canvas.draw_text_clipped(
                x + 6,
                y + 6 + (i as i32) * 15,
                iw - 12,
                &format!("{line}{cursor}"),
                0xE6EDF3,
                1,
            );
        }

        let by = wy + wh - 34;
        canvas.draw_button(wx + 14, by, 100, 22, "Sohranit", 0x238636, 0xFFFFFF, 0x3FB950);
        canvas.draw_button(wx + 122, by, 90, 22, "Ochistit", 0x21262D, 0xFFFFFF, 0x58A6FF);
        canvas.draw_text_clipped(wx + 220, by + 7, ww - 230, &self.saved_msg, 0x8B949E, 1);
    }
}

// ---------------------------------------------------------------------------
// 5. Full Settings App (Sidebar + ColorOS-style About OS)
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
    CyberSlate,
    CrimsonBratukha,
    MatrixDark,
    ClassicBlue,
}

impl ThemePreset {
    pub fn name(self) -> &'static str {
        match self {
            ThemePreset::CyberSlate => "Cyber Slate",
            ThemePreset::CrimsonBratukha => "Crimson Bratukha",
            ThemePreset::MatrixDark => "Matrix Dark",
            ThemePreset::ClassicBlue => "Classic Blue",
        }
    }

    pub fn colors(self) -> (u32, u32, u32) {
        // (bg, grid, accent)
        match self {
            ThemePreset::CyberSlate => (0x101822, 0x15202E, 0x1F6FEB),
            ThemePreset::CrimsonBratukha => (0x1A0E12, 0x26141A, 0xDA3633),
            ThemePreset::MatrixDark => (0x08140C, 0x0E2214, 0x238636),
            ThemePreset::ClassicBlue => (0x004E98, 0x0059AD, 0x003B73),
        }
    }
}

pub struct SettingsState {
    pub page: SettingsPage,
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
            theme: ThemePreset::CyberSlate,
            show_grid: true,
            show_icons: true,
            taskbar_top: false,
            crosshair_cursor: false,
            snake_tick_div: 10,
        }
    }

    pub fn on_click(&mut self, lx: i32, ly: i32) -> bool {
        // Left sidebar (x: 10..130)
        if (10..=130).contains(&lx) && ly >= 36 {
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

        // Right content area (x >= 142)
        if lx < 142 {
            return false;
        }

        match self.page {
            SettingsPage::Display => {
                if (68..=92).contains(&ly) {
                    self.theme = match self.theme {
                        ThemePreset::CyberSlate => ThemePreset::CrimsonBratukha,
                        ThemePreset::CrimsonBratukha => ThemePreset::MatrixDark,
                        ThemePreset::MatrixDark => ThemePreset::ClassicBlue,
                        ThemePreset::ClassicBlue => ThemePreset::CyberSlate,
                    };
                } else if (110..=134).contains(&ly) {
                    self.show_grid = !self.show_grid;
                } else if (152..=176).contains(&ly) {
                    self.show_icons = !self.show_icons;
                }
            }
            SettingsPage::Taskbar => {
                if (68..=92).contains(&ly) {
                    self.taskbar_top = !self.taskbar_top;
                } else if (110..=134).contains(&ly) {
                    self.crosshair_cursor = !self.crosshair_cursor;
                }
            }
            SettingsPage::System => {
                if (68..=92).contains(&ly) {
                    self.snake_tick_div = match self.snake_tick_div {
                        14 => 10,
                        10 => 5,
                        _ => 14,
                    };
                } else if (188..=214).contains(&ly) {
                    return true; // signal caller to refresh metrics
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
        // Sidebar background
        let sb_x = wx + 8;
        let sb_y = wy + 32;
        let sb_w = 124;
        let sb_h = wh - 40;
        canvas.fill_rect(sb_x, sb_y, sb_w, sb_h, 0x0D1117);
        canvas.draw_rect(sb_x, sb_y, sb_w, sb_h, 0x30363D);

        let tabs = [
            (SettingsPage::Display, "Ekran / Tema"),
            (SettingsPage::Taskbar, "Panel zadach"),
            (SettingsPage::System, "Sistema"),
            (SettingsPage::About, "About OS"),
        ];
        for (i, (p, label)) in tabs.iter().enumerate() {
            let ty = sb_y + 4 + (i as i32) * 30;
            let active = self.page == *p;
            let bg = if active { 0x1F6FEB } else { 0x161B22 };
            let fg = if active { 0xFFFFFF } else { 0xC9D1D9 };
            canvas.fill_rect(sb_x + 4, ty, sb_w - 8, 24, bg);
            canvas.draw_text_clipped(sb_x + 10, ty + 8, sb_w - 16, label, fg, 1);
        }

        // Right content pane
        let x = wx + 142;
        let y = wy + 34;
        let iw = ww - 154;
        let (_, _, accent) = self.theme.colors();

        match self.page {
            SettingsPage::Display => {
                canvas.draw_text_clipped(x, y, iw, "Ekran i Tema", 0xFFFFFF, 2);
                canvas.draw_text_clipped(x, y + 34, iw, "Tema rabochego stola:", 0x8B949E, 1);
                canvas.draw_button(x, wy + 68, iw, 24, self.theme.name(), 0x21262D, 0xFFFFFF, accent);

                let grid_lbl = if self.show_grid { "Setka fona: [VKL]" } else { "Setka fona: [VYKL]" };
                canvas.draw_button(x, wy + 110, iw, 24, grid_lbl, 0x21262D, 0xE6EDF3, 0x30363D);

                let icon_lbl = if self.show_icons { "Ikonki stola: [VKL]" } else { "Ikonki stola: [VYKL]" };
                canvas.draw_button(x, wy + 152, iw, 24, icon_lbl, 0x21262D, 0xE6EDF3, 0x30363D);
            }
            SettingsPage::Taskbar => {
                canvas.draw_text_clipped(x, y, iw, "Panel i Kursor", 0xFFFFFF, 2);
                let pos_lbl = if self.taskbar_top {
                    "Poziciya paneli: [Sverhu]"
                } else {
                    "Poziciya paneli: [Snizu]"
                };
                canvas.draw_button(x, wy + 68, iw, 24, pos_lbl, 0x21262D, 0xFFFFFF, accent);

                let cur_lbl = if self.crosshair_cursor {
                    "Kursor: [Pricel]"
                } else {
                    "Kursor: [Strelka]"
                };
                canvas.draw_button(x, wy + 110, iw, 24, cur_lbl, 0x21262D, 0xE6EDF3, 0x30363D);
            }
            SettingsPage::System => {
                canvas.draw_text_clipped(x, y, iw, "Proizvoditelnost", 0xFFFFFF, 2);
                let spd = match self.snake_tick_div {
                    14 => "Skorost Zmeyki: [Medlenno]",
                    5 => "Skorost Zmeyki: [Turbo]",
                    _ => "Skorost Zmeyki: [Normalno]",
                };
                canvas.draw_button(x, wy + 68, iw, 24, spd, 0x21262D, 0xFFFFFF, accent);
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
                canvas.draw_button(x, wy + 188, 140, 26, "Obnovit metriki", 0x238636, 0xFFFFFF, 0x3FB950);
            }
            SettingsPage::About => {
                // ColorOS-style Hero Banner Card
                canvas.fill_rect(x, y, iw, 74, 0x162436);
                canvas.fill_rect(x, y, 6, 74, accent);
                canvas.draw_rect(x, y, iw, 74, accent);

                canvas.draw_text_clipped(x + 16, y + 10, iw - 24, "pizdOS 2.0", 0xFFFFFF, 2);
                canvas.draw_text_clipped(
                    x + 16,
                    y + 34,
                    iw - 24,
                    "Bratukha Edition | Pizsoft UI",
                    0x79C0FF,
                    1,
                );
                canvas.draw_text_clipped(
                    x + 16,
                    y + 50,
                    iw - 24,
                    "by prosh1vka, Lowyk, KiryxaTipa",
                    0x3FB950,
                    1,
                );

                // 2-Column ColorOS Info Tiles
                let col_w = (iw - 8) / 2;
                let tiles = [
                    ("Stack", "musl + BusyBox"),
                    ("Policy", "NO GNU / 0 Deps"),
                    ("License", "MIT (GNU Nahuy)"),
                    ("Kernel", kernel),
                    ("RAM", &format!("{} MB Total", mem.total_mb)),
                    ("Pkg Mgr", "piz-get 0.1.0"),
                ];

                for (idx, (label, val)) in tiles.iter().enumerate() {
                    let col = (idx % 2) as i32;
                    let row = (idx / 2) as i32;
                    let tx = x + col * (col_w + 8);
                    let ty = y + 84 + row * 50;
                    if ty + 44 > wy + wh - 10 {
                        break;
                    }
                    canvas.fill_rect(tx, ty, col_w, 44, 0x21262D);
                    canvas.draw_rect(tx, ty, col_w, 44, 0x30363D);
                    canvas.draw_text_clipped(tx + 8, ty + 8, col_w - 16, label, 0x8B949E, 1);
                    canvas.draw_text_clipped(tx + 8, ty + 24, col_w - 16, val, 0xE6EDF3, 1);
                }
            }
        }
    }
}
