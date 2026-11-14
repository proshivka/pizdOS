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
// 5. Settings — ColorOS-style "About OS"
// ---------------------------------------------------------------------------
pub fn draw_settings_about_os(
    canvas: &mut Canvas,
    wx: i32,
    wy: i32,
    ww: i32,
    wh: i32,
    kernel: &str,
    mem: &MemInfo,
) {
    let x = wx + 14;
    let y = wy + 34;
    let iw = ww - 28;

    // ColorOS-style Hero Banner Card
    canvas.fill_rect(x, y, iw, 74, 0x162436);
    canvas.fill_rect(x, y, 6, 74, 0x1F6FEB);
    canvas.draw_rect(x, y, iw, 74, 0x388BFD);

    canvas.draw_text_clipped(x + 18, y + 12, iw - 28, "pizdOS 2.0", 0xFFFFFF, 2);
    canvas.draw_text_clipped(
        x + 18,
        y + 34,
        iw - 28,
        "Bratukha Edition | Pizsoft UI",
        0x79C0FF,
        1,
    );
    canvas.draw_text_clipped(
        x + 18,
        y + 50,
        iw - 28,
        "by prosh1vka, Lowyk, KiryxaTipa",
        0x3FB950,
        1,
    );

    // 2-Column ColorOS Info Tiles
    let col_w = (iw - 10) / 2;
    let tiles = [
        ("Stack", "musl + BusyBox (NO GNU)"),
        ("License", "MIT (GNU Nahuy)"),
        ("Kernel", kernel),
        ("RAM", &format!("{} MB Total", mem.total_mb)),
        ("Pkg Mgr", "piz-get 0.1.0 (0 deps)"),
        ("UI Engine", "Pizsoft 2D (0 crates)"),
    ];

    for (idx, (label, val)) in tiles.iter().enumerate() {
        let col = (idx % 2) as i32;
        let row = (idx / 2) as i32;
        let tx = x + col * (col_w + 10);
        let ty = y + 84 + row * 50;
        if ty + 44 > wy + wh - 12 {
            break;
        }
        canvas.fill_rect(tx, ty, col_w, 44, 0x21262D);
        canvas.draw_rect(tx, ty, col_w, 44, 0x30363D);
        canvas.draw_text_clipped(tx + 10, ty + 8, col_w - 20, label, 0x8B949E, 1);
        canvas.draw_text_clipped(tx + 10, ty + 24, col_w - 20, val, 0xE6EDF3, 1);
    }
}
