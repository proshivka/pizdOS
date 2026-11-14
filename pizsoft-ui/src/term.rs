use std::env;
use std::io::{self, Write};
use std::process::{Command, Stdio};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Color {
    Black,
    Red,
    Green,
    Yellow,
    Blue,
    Magenta,
    Cyan,
    White,
}

impl Color {
    pub fn fg_code(self) -> u8 {
        match self {
            Color::Black => 30,
            Color::Red => 31,
            Color::Green => 32,
            Color::Yellow => 33,
            Color::Blue => 34,
            Color::Magenta => 35,
            Color::Cyan => 36,
            Color::White => 37,
        }
    }

    pub fn bg_code(self) -> u8 {
        match self {
            Color::Black => 40,
            Color::Red => 41,
            Color::Green => 42,
            Color::Yellow => 43,
            Color::Blue => 44,
            Color::Magenta => 45,
            Color::Cyan => 46,
            Color::White => 47,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cell {
    pub ch: char,
    pub fg: Color,
    pub bg: Color,
    pub bold: bool,
}

impl Default for Cell {
    fn default() -> Self {
        Self {
            ch: ' ',
            fg: Color::White,
            bg: Color::Black,
            bold: false,
        }
    }
}

pub struct Screen {
    pub width: usize,
    pub height: usize,
    front: Vec<Cell>,
    back: Vec<Cell>,
}

impl Screen {
    pub fn new(width: usize, height: usize) -> Self {
        let len = width.saturating_mul(height);
        let blank = Cell::default();
        let dirty = Cell {
            ch: '\0',
            ..Cell::default()
        };
        Self {
            width,
            height,
            front: vec![dirty; len],
            back: vec![blank; len],
        }
    }

    pub fn resize(&mut self, width: usize, height: usize) {
        if self.width == width && self.height == height {
            return;
        }
        *self = Self::new(width, height);
    }

    pub fn clear(&mut self, bg: Color) {
        let cell = Cell {
            ch: ' ',
            fg: Color::White,
            bg,
            bold: false,
        };
        self.back.fill(cell);
    }

    pub fn put(&mut self, x: usize, y: usize, ch: char, fg: Color, bg: Color, bold: bool) {
        if x < self.width && y < self.height {
            self.back[y * self.width + x] = Cell { ch, fg, bg, bold };
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn put_str(
        &mut self,
        x: usize,
        y: usize,
        max_w: usize,
        s: &str,
        fg: Color,
        bg: Color,
        bold: bool,
    ) {
        if y >= self.height || x >= self.width || max_w == 0 {
            return;
        }
        let limit = max_w.min(self.width - x);
        for (i, ch) in s.chars().take(limit).enumerate() {
            let safe_ch = if ch == '\n' || ch == '\r' || ch == '\t' {
                ' '
            } else {
                ch
            };
            self.put(x + i, y, safe_ch, fg, bg, bold);
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn fill_rect(
        &mut self,
        x: usize,
        y: usize,
        w: usize,
        h: usize,
        ch: char,
        fg: Color,
        bg: Color,
    ) {
        for dy in 0..h {
            for dx in 0..w {
                self.put(x + dx, y + dy, ch, fg, bg, false);
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn draw_box(
        &mut self,
        x: usize,
        y: usize,
        w: usize,
        h: usize,
        title: &str,
        border_fg: Color,
        bg: Color,
        ascii: bool,
    ) {
        if w < 2 || h < 2 {
            return;
        }
        self.fill_rect(x, y, w, h, ' ', border_fg, bg);

        let (tl, tr, bl, br, hz, vt) = if ascii {
            ('+', '+', '+', '+', '-', '|')
        } else {
            ('┌', '┐', '└', '┘', '─', '│')
        };

        self.put(x, y, tl, border_fg, bg, true);
        self.put(x + w - 1, y, tr, border_fg, bg, true);
        self.put(x, y + h - 1, bl, border_fg, bg, true);
        self.put(x + w - 1, y + h - 1, br, border_fg, bg, true);

        for dx in 1..w - 1 {
            self.put(x + dx, y, hz, border_fg, bg, false);
            self.put(x + dx, y + h - 1, hz, border_fg, bg, false);
        }
        for dy in 1..h - 1 {
            self.put(x, y + dy, vt, border_fg, bg, false);
            self.put(x + w - 1, y + dy, vt, border_fg, bg, false);
        }

        if !title.is_empty() && w > 4 {
            let label = format!(" {title} ");
            self.put_str(x + 2, y, w - 4, &label, Color::Yellow, bg, true);
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn draw_gauge(
        &mut self,
        x: usize,
        y: usize,
        w: usize,
        percent: u8,
        label: &str,
        bar_fg: Color,
        bg: Color,
    ) {
        if w < 4 {
            return;
        }
        let pct = percent.min(100) as usize;
        let inner = w - 2;
        let filled = (inner * pct) / 100;

        self.put(x, y, '[', Color::White, bg, true);
        for i in 0..inner {
            let (ch, fg) = if i < filled {
                ('#', bar_fg)
            } else {
                ('.', Color::White)
            };
            self.put(x + 1 + i, y, ch, fg, bg, i < filled);
        }
        self.put(x + w - 1, y, ']', Color::White, bg, true);

        if !label.is_empty() && inner > 2 {
            let label_len = label.chars().count().min(inner);
            let start = x + 1 + inner.saturating_sub(label_len) / 2;
            self.put_str(start, y, inner, label, Color::White, bg, true);
        }
    }

    pub fn flush(&mut self, out: &mut impl Write) -> io::Result<()> {
        let mut buf = String::with_capacity(1024);
        let mut last_fg = None;
        let mut last_bg = None;
        let mut last_bold = None;
        let mut cursor_pos: Option<(usize, usize)> = None;

        for y in 0..self.height {
            for x in 0..self.width {
                let idx = y * self.width + x;
                let cell = self.back[idx];
                if self.front[idx] == cell {
                    continue;
                }
                self.front[idx] = cell;

                let need_move = match cursor_pos {
                    Some((cx, cy)) => cx != x || cy != y,
                    None => true,
                };
                if need_move {
                    use std::fmt::Write as _;
                    let _ = write!(&mut buf, "\x1b[{};{}H", y + 1, x + 1);
                }

                if last_fg != Some(cell.fg)
                    || last_bg != Some(cell.bg)
                    || last_bold != Some(cell.bold)
                {
                    use std::fmt::Write as _;
                    let weight = if cell.bold { "1" } else { "22" };
                    let _ = write!(
                        &mut buf,
                        "\x1b[{};{};{}m",
                        weight,
                        cell.fg.fg_code(),
                        cell.bg.bg_code()
                    );
                    last_fg = Some(cell.fg);
                    last_bg = Some(cell.bg);
                    last_bold = Some(cell.bold);
                }

                buf.push(cell.ch);
                cursor_pos = Some((x + 1, y));
            }
        }

        if !buf.is_empty() {
            out.write_all(buf.as_bytes())?;
            out.flush()?;
        }
        Ok(())
    }

    pub fn render_plain(&self) -> String {
        let mut out = String::with_capacity((self.width + 1) * self.height);
        for y in 0..self.height {
            let row = &self.back[y * self.width..(y + 1) * self.width];
            for cell in row {
                out.push(cell.ch);
            }
            if y + 1 < self.height {
                out.push('\n');
            }
        }
        out
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    Up,
    Down,
    Left,
    Right,
    Tab,
    Enter,
    Backspace,
    Esc,
    F(u8),
    Char(char),
    None,
}

pub fn parse_key(bytes: &[u8]) -> Key {
    if bytes.is_empty() {
        return Key::None;
    }

    if bytes.starts_with(b"\x1b[") {
        match bytes {
            b"\x1b[A" => return Key::Up,
            b"\x1b[B" => return Key::Down,
            b"\x1b[C" => return Key::Right,
            b"\x1b[D" => return Key::Left,
            b"\x1b[11~" => return Key::F(1),
            b"\x1b[12~" => return Key::F(2),
            b"\x1b[13~" => return Key::F(3),
            b"\x1b[14~" => return Key::F(4),
            b"\x1b[15~" => return Key::F(5),
            b"\x1b[21~" => return Key::F(10),
            _ => {}
        }
    }

    if bytes.starts_with(b"\x1bO") {
        match bytes {
            b"\x1bOP" => return Key::F(1),
            b"\x1bOQ" => return Key::F(2),
            b"\x1bOR" => return Key::F(3),
            b"\x1bOS" => return Key::F(4),
            _ => {}
        }
    }

    match bytes[0] {
        b'\t' => Key::Tab,
        b'\r' | b'\n' => Key::Enter,
        0x7f | 0x08 => Key::Backspace,
        0x1b => Key::Esc,
        0x03 => Key::Char('q'),
        _ => {
            if let Ok(s) = std::str::from_utf8(bytes)
                && let Some(ch) = s.chars().next()
                && !ch.is_control()
            {
                return Key::Char(ch);
            }
            Key::None
        }
    }
}

pub struct RawSession {
    saved_stty: Option<String>,
}

impl RawSession {
    pub fn enter() -> io::Result<Self> {
        let saved_stty = Command::new("stty")
            .arg("-g")
            .stdin(Stdio::inherit())
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .filter(|s| !s.is_empty());

        if saved_stty.is_some() {
            let _ = Command::new("stty")
                .args(["raw", "-echo", "min", "0", "time", "1"])
                .stdin(Stdio::inherit())
                .status();
        }

        let mut stdout = io::stdout();
        stdout.write_all(b"\x1b[?1049h\x1b[?25l\x1b[2J")?;
        stdout.flush()?;

        Ok(Self { saved_stty })
    }
}

impl Drop for RawSession {
    fn drop(&mut self) {
        let mut stdout = io::stdout();
        let _ = stdout.write_all(b"\x1b[0m\x1b[?25h\x1b[?1049l");
        let _ = stdout.flush();

        if let Some(ref state) = self.saved_stty {
            let _ = Command::new("stty")
                .arg(state)
                .stdin(Stdio::inherit())
                .status();
        }
    }
}

pub fn terminal_size() -> (usize, usize) {
    if let Ok(out) = Command::new("stty")
        .arg("size")
        .stdin(Stdio::inherit())
        .output()
        && out.status.success()
    {
        let txt = String::from_utf8_lossy(&out.stdout);
        let mut parts = txt.split_whitespace();
        if let (Some(rows_s), Some(cols_s)) = (parts.next(), parts.next())
            && let (Ok(rows), Ok(cols)) = (rows_s.parse::<usize>(), cols_s.parse::<usize>())
            && rows >= 12
            && cols >= 40
        {
            return (cols, rows);
        }
    }

    let cols = env::var("COLUMNS")
        .ok()
        .and_then(|s| s.parse::<usize>().ok())
        .filter(|&c| c >= 40)
        .unwrap_or(80);
    let rows = env::var("LINES")
        .ok()
        .and_then(|s| s.parse::<usize>().ok())
        .filter(|&r| r >= 12)
        .unwrap_or(25);

    (cols, rows)
}
