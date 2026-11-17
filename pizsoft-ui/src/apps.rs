use std::cell::RefCell;
use std::fs;
use std::path::{Path, PathBuf};
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeyEvent {
    Char(char),
    Enter,
    Backspace,
    Escape,
    Up,
    Down,
    Left,
    Right,
    Tab,
    Copy,
    Paste(String),
    Save,
    Find,
    Undo,
    ShowDesktop,
    Workspace(u8),
    ToggleLauncher,
    ToggleControlCenter,
    SwitchWindows,
    LockScreen,
    ScaleDelta(i8),
}

// ---------------------------------------------------------------------------
// 1. Terminal
// ---------------------------------------------------------------------------
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TermAction {
    None,
    SetTheme(u8),
    SetScale(u16),
    SetResolution(usize),
    LaunchX11Demo,
}

pub struct TerminalState {
    pub lines: Vec<String>,
    pub input: String,
    pub history: Vec<String>,
    pub hist_idx: Option<usize>,
    pub cwd: PathBuf,
    pub display_env: Vec<(&'static str, String)>,
}

impl TerminalState {
    pub fn new(lang: Lang) -> Self {
        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        Self {
            lines: vec![
                String::from(tr(
                    lang,
                    "pizdOS 2.1(00) Терминал (0% GNU)",
                    "pizdOS 2.1(00) Terminal (0% GNU)",
                )),
                String::from(tr(
                    lang,
                    "Введи 'help', 'pizfetch', 'x11demo' или любую команду (Up/Down/Tab):",
                    "Type 'help', 'pizfetch', 'x11demo' or any command (Up/Down/Tab):",
                )),
            ],
            input: String::new(),
            history: Vec::new(),
            hist_idx: None,
            cwd,
            display_env: Vec::new(),
        }
    }

    pub fn push_line(&mut self, line: String) {
        self.lines.push(line);
        if self.lines.len() > 96 {
            let drain = self.lines.len() - 96;
            self.lines.drain(0..drain);
        }
    }

    pub fn on_key(&mut self, key: KeyEvent, kernel: &str, mem: &MemInfo) -> TermAction {
        match key {
            KeyEvent::Char(c) => {
                self.hist_idx = None;
                if self.input.chars().count() < 64 {
                    self.input.push(c);
                }
            }
            KeyEvent::Paste(txt) => {
                self.hist_idx = None;
                let line = txt.lines().next().unwrap_or("");
                for c in line.chars() {
                    if !c.is_control() && self.input.chars().count() < 64 {
                        self.input.push(c);
                    }
                }
            }
            KeyEvent::Backspace => {
                self.hist_idx = None;
                self.input.pop();
            }
            KeyEvent::Up => {
                if !self.history.is_empty() {
                    let next = match self.hist_idx {
                        None => self.history.len() - 1,
                        Some(i) => i.saturating_sub(1),
                    };
                    self.hist_idx = Some(next);
                    self.input = self.history[next].clone();
                }
            }
            KeyEvent::Down => {
                if let Some(i) = self.hist_idx {
                    if i + 1 < self.history.len() {
                        self.hist_idx = Some(i + 1);
                        self.input = self.history[i + 1].clone();
                    } else {
                        self.hist_idx = None;
                        self.input.clear();
                    }
                }
            }
            KeyEvent::Tab => {
                self.autocomplete();
            }
            KeyEvent::Enter => {
                let cmd = self.input.trim().to_string();
                self.input.clear();
                self.hist_idx = None;
                self.push_line(format!("pizdOS# {cmd}"));
                if !cmd.is_empty() && self.history.last() != Some(&cmd) {
                    self.history.push(cmd.clone());
                    if self.history.len() > 40 {
                        self.history.remove(0);
                    }
                }
                return self.exec_cmd(&cmd, kernel, mem);
            }
            _ => {}
        }
        TermAction::None
    }

    fn autocomplete(&mut self) {
        let builtins = [
            "help", "pizfetch", "poyasni", "clear", "nahuy", "whoami", "pwd", "ls", "cd", "cat",
            "theme", "scale", "res", "x11demo",
        ];
        if let Some(space) = self.input.rfind(' ') {
            let prefix = &self.input[space + 1..];
            if let Ok(rd) = fs::read_dir(&self.cwd) {
                let mut matches: Vec<String> = rd
                    .flatten()
                    .filter_map(|e| e.file_name().into_string().ok())
                    .filter(|n| n.starts_with(prefix))
                    .collect();
                matches.sort();
                if let Some(first) = matches.first() {
                    self.input.truncate(space + 1);
                    self.input.push_str(first);
                }
            }
        } else {
            let prefix = self.input.trim();
            if let Some(found) = builtins.iter().find(|b| b.starts_with(prefix)) {
                self.input = String::from(*found);
            }
        }
    }

    fn exec_cmd(&mut self, cmd: &str, kernel: &str, mem: &MemInfo) -> TermAction {
        if cmd.is_empty() {
            return TermAction::None;
        }
        let mut parts = cmd.splitn(2, char::is_whitespace);
        let head = parts.next().unwrap_or("");
        let rest = parts.next().map(str::trim).unwrap_or("");

        match head {
            "help" | "huelp" => {
                self.push_line(String::from("help, pizfetch, x11demo, clear/nahuy, pwd, ls, cd, cat"));
                self.push_line(String::from("theme <0-3>, scale <100|125|150|175>, res <800x600|1024x768|1280x800|1440x900>, <cmd &>, whoami"));
            }
            "x11demo" => {
                self.push_line(String::from("Launching live MIT X11 wire client..."));
                return TermAction::LaunchX11Demo;
            }
            "clear" | "nahuy" | "cls" => {
                self.lines.clear();
            }
            "poyasni" | "pizfetch" => {
                self.push_line(String::from("  /\\_/\\   OS: pizdOS 2.1(00) [NO GNU]"));
                self.push_line(format!(" ( o.o )  Kernel: {kernel}"));
                self.push_line(format!("  > ^ <   RAM: {}/{} MB ({}%)", mem.used_mb, mem.total_mb, mem.percent));
                self.push_line(format!("          CWD: {}", self.cwd.display()));
            }
            "whoami" => {
                self.push_line(String::from("root (bratukha)"));
            }
            "pwd" => {
                self.push_line(format!("{}", self.cwd.display()));
            }
            "cd" => {
                let target = if rest.is_empty() {
                    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
                } else {
                    self.cwd.join(rest)
                };
                if target.is_dir() {
                    if let Ok(canon) = fs::canonicalize(&target) {
                        self.cwd = canon;
                    } else {
                        self.cwd = target;
                    }
                    self.push_line(format!("-> {}", self.cwd.display()));
                } else {
                    self.push_line(format!("cd: not found: {}", target.display()));
                }
            }
            "ls" | "dir" => {
                let dir = if rest.is_empty() { self.cwd.clone() } else { self.cwd.join(rest) };
                match fs::read_dir(&dir) {
                    Ok(rd) => {
                        let mut names: Vec<String> = rd
                            .flatten()
                            .map(|e| {
                                let suffix = if e.file_type().map(|t| t.is_dir()).unwrap_or(false) { "/" } else { "" };
                                format!("{}{}", e.file_name().to_string_lossy(), suffix)
                            })
                            .collect();
                        names.sort();
                        for chunk in names.chunks(4).take(6) {
                            self.push_line(chunk.join("  "));
                        }
                    }
                    Err(e) => self.push_line(format!("ls: {e}")),
                }
            }
            "cat" => {
                if rest.is_empty() {
                    self.push_line(String::from("usage: cat <file>"));
                } else {
                    let p = self.cwd.join(rest);
                    match fs::read_to_string(&p) {
                        Ok(txt) => {
                            for l in txt.lines().take(14) {
                                self.push_line(l.to_string());
                            }
                        }
                        Err(e) => self.push_line(format!("cat: {e}")),
                    }
                }
            }
            "theme" => {
                if let Ok(n) = rest.parse::<u8>()
                    && n < 4
                {
                    self.push_line(format!("theme -> #{n}"));
                    return TermAction::SetTheme(n);
                }
                self.push_line(String::from("usage: theme <0..3>"));
            }
            "scale" => {
                if let Ok(pct) = rest.parse::<u16>()
                    && [100, 125, 150, 175].contains(&pct)
                {
                    self.push_line(format!("scale -> {pct}%"));
                    return TermAction::SetScale(pct);
                }
                self.push_line(String::from("usage: scale <100|125|150|175>"));
            }
            "res" => {
                if let Some((ws, hs)) = rest.split_once('x').or_else(|| rest.split_once('X'))
                    && let (Ok(w), Ok(h)) = (ws.trim().parse::<i32>(), hs.trim().parse::<i32>())
                    && let Some(idx) = SCREEN_RES_PRESETS.iter().position(|&(pw, ph)| pw == w && ph == h)
                {
                    self.push_line(format!("res -> {w}x{h}"));
                    return TermAction::SetResolution(idx);
                }
                if let Ok(idx) = rest.parse::<usize>()
                    && let Some(&(w, h)) = SCREEN_RES_PRESETS.get(idx)
                {
                    self.push_line(format!("res -> {w}x{h}"));
                    return TermAction::SetResolution(idx);
                }
                self.push_line(String::from("usage: res <800x600|1024x768|1280x800|1440x900>"));
            }
            _ => {
                let bg = cmd.ends_with('&');
                let clean = cmd.trim_end_matches('&').trim();
                #[cfg(windows)]
                let mut command = {
                    let mut c = Command::new("cmd");
                    c.args(["/C", clean]);
                    c
                };
                #[cfg(not(windows))]
                let mut command = {
                    let mut c = Command::new("sh");
                    c.args(["-c", clean]);
                    c
                };
                command.current_dir(&self.cwd);
                for (k, v) in &self.display_env {
                    command.env(k, v);
                }

                if bg {
                    match command.spawn() {
                        Ok(ch) => self.push_line(format!("[spawned pid {}]", ch.id())),
                        Err(e) => self.push_line(format!("spawn err: {e}")),
                    }
                } else {
                    match command.output() {
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
        TermAction::None
    }

    #[allow(clippy::too_many_arguments)]
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaintShape {
    Free,
    Line,
    Rect,
    Circle,
}

impl PaintShape {
    fn next(self) -> Self {
        match self {
            PaintShape::Free => PaintShape::Line,
            PaintShape::Line => PaintShape::Rect,
            PaintShape::Rect => PaintShape::Circle,
            PaintShape::Circle => PaintShape::Free,
        }
    }

    fn label(self) -> &'static str {
        match self {
            PaintShape::Free => "Free",
            PaintShape::Line => "Line",
            PaintShape::Rect => "Rect",
            PaintShape::Circle => "Circ",
        }
    }
}

pub struct PaintState {
    pub buf: Vec<u32>,
    pub undo_buf: Option<Vec<u32>>,
    pub color_idx: usize,
    pub custom_color: Option<u32>,
    pub brush: i32,
    pub eraser: bool,
    pub fill_mode: bool,
    pub eyedropper: bool,
    pub shape: PaintShape,
    pub shape_start: Option<(i32, i32)>,
    pub brush_tier: u8,
    pub last_pos: Option<(i32, i32)>,
}

impl PaintState {
    pub fn new() -> Self {
        Self {
            buf: vec![0x0D1117; PAINT_W * PAINT_H],
            undo_buf: None,
            color_idx: 1,
            custom_color: None,
            brush: 4,
            eraser: false,
            fill_mode: false,
            eyedropper: false,
            shape: PaintShape::Free,
            shape_start: None,
            brush_tier: 1,
            last_pos: None,
        }
    }

    fn snapshot_undo(&mut self) {
        self.undo_buf = Some(self.buf.clone());
    }

    pub fn undo(&mut self) -> bool {
        if let Some(prev) = self.undo_buf.take() {
            self.buf = prev;
            true
        } else {
            false
        }
    }

    pub fn end_stroke(&mut self) {
        if let Some((x0, y0)) = self.shape_start.take() {
            let (x1, y1) = self.last_pos.unwrap_or((x0, y0));
            let col = self.active_color();
            match self.shape {
                PaintShape::Line => self.draw_line_buf(x0, y0, x1, y1, col),
                PaintShape::Rect => {
                    let (lx, rx) = (x0.min(x1), x0.max(x1));
                    let (ty, by) = (y0.min(y1), y0.max(y1));
                    self.draw_line_buf(lx, ty, rx, ty, col);
                    self.draw_line_buf(lx, by, rx, by, col);
                    self.draw_line_buf(lx, ty, lx, by, col);
                    self.draw_line_buf(rx, ty, rx, by, col);
                }
                PaintShape::Circle => {
                    let r = (((x1 - x0) * (x1 - x0) + (y1 - y0) * (y1 - y0)) as f32).sqrt().round() as i32;
                    let steps = (r * 6).clamp(16, 180);
                    let mut prev = None;
                    for i in 0..=steps {
                        let a = (i as f32 / steps as f32) * std::f32::consts::TAU;
                        let px = x0 + (a.cos() * r as f32).round() as i32;
                        let py = y0 + (a.sin() * r as f32).round() as i32;
                        if let Some((qx, qy)) = prev {
                            self.draw_line_buf(qx, qy, px, py, col);
                        }
                        prev = Some((px, py));
                    }
                }
                PaintShape::Free => {}
            }
        }
        self.last_pos = None;
    }

    fn draw_line_buf(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, color: u32) {
        let dx = x1 - x0;
        let dy = y1 - y0;
        let steps = (dx.abs().max(dy.abs()) * 2).max(1);
        for i in 0..=steps {
            let px = x0 + (dx * i) / steps;
            let py = y0 + (dy * i) / steps;
            self.stamp_brush(px, py, color);
        }
    }

    fn active_color(&self) -> u32 {
        if self.eraser {
            0x0D1117
        } else {
            self.custom_color.unwrap_or(PALETTE[self.color_idx])
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
        if self.eyedropper {
            self.custom_color = Some(self.buf[(cy as usize) * PAINT_W + (cx as usize)]);
            self.eraser = false;
            self.eyedropper = false;
            return;
        }
        let color = self.active_color();
        if self.fill_mode {
            if self.last_pos.is_none() {
                self.snapshot_undo();
                self.flood_fill(cx, cy, color);
                self.last_pos = Some((cx, cy));
            }
            return;
        }
        if self.shape != PaintShape::Free {
            if self.shape_start.is_none() {
                self.snapshot_undo();
                self.shape_start = Some((cx, cy));
            }
            self.last_pos = Some((cx, cy));
            return;
        }
        if self.last_pos.is_none() {
            self.snapshot_undo();
        }
        if let Some((x0, y0)) = self.last_pos {
            self.draw_line_buf(x0, y0, cx, cy, color);
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
        if ly >= wh - 66 && ly <= wh - 40 {
            if (14..=66).contains(&lx) {
                self.undo();
            } else if (70..=126).contains(&lx) {
                self.eyedropper = !self.eyedropper;
                if self.eyedropper {
                    self.eraser = false;
                    self.fill_mode = false;
                }
            } else if (130..=214).contains(&lx) {
                self.shape = self.shape.next();
                self.eraser = false;
                self.fill_mode = false;
                self.eyedropper = false;
            }
            return PaintAction::None;
        }
        if ly >= wh - 38 && ly <= wh - 8 {
            for i in 0..PALETTE.len() {
                let bx = 14 + (i as i32) * 20;
                if lx >= bx && lx <= bx + 18 {
                    self.color_idx = i;
                    self.custom_color = None;
                    self.eraser = false;
                    self.eyedropper = false;
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
                    self.eyedropper = false;
                }
                return PaintAction::None;
            }
            if (240..=280).contains(&lx) {
                self.fill_mode = !self.fill_mode;
                if self.fill_mode {
                    self.eraser = false;
                    self.eyedropper = false;
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
                self.snapshot_undo();
                self.buf.fill(0x0D1117);
                return PaintAction::None;
            }
        }
        self.paint_at(lx, ly);
        PaintAction::None
    }

    #[allow(clippy::too_many_arguments)]
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

        if let Some((x0, y0)) = self.shape_start {
            let (x1, y1) = self.last_pos.unwrap_or((x0, y0));
            let col = self.active_color();
            match self.shape {
                PaintShape::Line => {
                    canvas.draw_aa_line((ox + x0) as f32, (oy + y0) as f32, (ox + x1) as f32, (oy + y1) as f32, self.brush as f32, col, 210);
                }
                PaintShape::Rect => {
                    let (rx0, rx1) = (x0.min(x1), x0.max(x1));
                    let (ry0, ry1) = (y0.min(y1), y0.max(y1));
                    canvas.draw_rounded_rect_alpha(
                        ox + rx0,
                        oy + ry0,
                        (rx1 - rx0).max(1),
                        (ry1 - ry0).max(1),
                        0,
                        col,
                        220,
                    );
                }
                PaintShape::Circle => {
                    let r = (((x1 - x0) * (x1 - x0) + (y1 - y0) * (y1 - y0)) as f32).sqrt().max(2.0);
                    canvas.fill_aa_circle((ox + x0) as f32, (oy + y0) as f32, r, col, 110);
                }
                PaintShape::Free => {}
            }
        }

        // Live brush ring preview over canvas
        let cx = hover_lx - 14;
        let cy = hover_ly - 36;
        if cx >= 0 && cy >= 0 && cx < PAINT_W as i32 && cy < PAINT_H as i32 {
            let preview_col = self.active_color();
            let r = (self.brush as f32 * 0.5).max(1.5);
            canvas.fill_aa_circle((wx + hover_lx) as f32, (wy + hover_ly) as f32, r + 1.0, 0xFFFFFF, 90);
            canvas.fill_aa_circle((wx + hover_lx) as f32, (wy + hover_ly) as f32, r, preview_col, 145);
        }

        let r2_y = wy + wh - 62;
        canvas.draw_button(
            wx + 14,
            r2_y,
            52,
            20,
            tr(lang, "Отм", "Undo"),
            if self.undo_buf.is_some() { 0x1E293B } else { 0x111520 },
            if self.undo_buf.is_some() { 0xF8FAFC } else { 0x64748B },
            if self.undo_buf.is_some() { accent } else { 0x334155 },
        );
        canvas.draw_button(
            wx + 70,
            r2_y,
            56,
            20,
            tr(lang, "Пип.", "Pick"),
            if self.eyedropper { accent } else { 0x1E293B },
            0xFFFFFF,
            if self.eyedropper { 0xFFFFFF } else { 0x475569 },
        );
        canvas.draw_button(
            wx + 130,
            r2_y,
            84,
            20,
            &format!("{} {}", tr(lang, "Ф:", "Sh:"), self.shape.label()),
            if self.shape != PaintShape::Free { accent } else { 0x1E293B },
            0xFFFFFF,
            accent,
        );
        let active_c = self.active_color();
        canvas.fill_rounded_rect(wx + 220, r2_y + 2, 16, 16, 4, active_c);
        canvas.draw_rounded_rect_alpha(wx + 220, r2_y + 2, 16, 16, 4, 0xFFFFFF, 85);
        canvas.draw_text_clipped(wx + 242, r2_y + 4, 120, &format!("#{active_c:06X}"), 0xCBD5E1, 1);

        let by = wy + wh - 34;
        for (i, &col) in PALETTE.iter().enumerate() {
            let cx_p = (wx + 23 + (i as i32) * 20) as f32;
            let cy_p = (by + 11) as f32;
            let sel = !self.eraser && self.custom_color.is_none() && i == self.color_idx;
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

    #[allow(clippy::too_many_arguments)]
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
// 4. PizPad (Pro Text Editor)
// ---------------------------------------------------------------------------
pub struct NotepadState {
    pub lines: Vec<String>,
    pub saved_msg: String,
    pub file_path: Option<PathBuf>,
    pub undo_stack: Vec<Vec<String>>,
    pub search_query: String,
    pub search_mode: bool,
    pub scroll: usize,
    pub dirty: bool,
}

impl NotepadState {
    pub fn new(lang: Lang) -> Self {
        Self {
            lines: vec![
                String::from("=== PIZPAD v2.1(00) ==="),
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
            saved_msg: String::from(tr(lang, "Печатай или Ctrl+S...", "Type or Ctrl+S...")),
            file_path: None,
            undo_stack: Vec::new(),
            search_query: String::new(),
            search_mode: false,
            scroll: 0,
            dirty: false,
        }
    }

    fn push_undo(&mut self) {
        self.undo_stack.push(self.lines.clone());
        if self.undo_stack.len() > 24 {
            self.undo_stack.remove(0);
        }
    }

    pub fn undo(&mut self) -> bool {
        if let Some(prev) = self.undo_stack.pop() {
            self.lines = prev;
            self.dirty = true;
            true
        } else {
            false
        }
    }

    pub fn open_file(&mut self, path: &Path, lang: Lang) -> Result<String, String> {
        let data = fs::read_to_string(path).map_err(|e| format!("Read err: {e}"))?;
        self.push_undo();
        let mut lines: Vec<String> = data
            .lines()
            .take(300)
            .map(|l| l.trim_end_matches('\r').chars().take(64).collect())
            .collect();
        if lines.is_empty() {
            lines.push(String::new());
        }
        self.lines = lines;
        self.file_path = Some(path.to_path_buf());
        self.scroll = 0;
        self.dirty = false;
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| String::from("file"));
        let msg = format!("{}: {name}", tr(lang, "Открыт", "Opened"));
        self.saved_msg = msg.clone();
        Ok(msg)
    }

    pub fn save_current(&mut self, lang: Lang) -> Option<String> {
        let target = self
            .file_path
            .clone()
            .unwrap_or_else(|| std::env::temp_dir().join("pizpad_note.txt"));
        let text = self.lines.join("\n");
        match fs::write(&target, text) {
            Ok(()) => {
                self.dirty = false;
                let fname = target
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| String::from("pizpad_note.txt"));
                let msg = format!("{}: {fname}", tr(lang, "Сохранено", "Saved"));
                self.saved_msg = msg.clone();
                Some(msg)
            }
            Err(e) => {
                self.saved_msg = format!("Err: {e}");
                None
            }
        }
    }

    pub fn on_key(&mut self, key: KeyEvent, lang: Lang) -> Option<String> {
        if self.search_mode {
            match key {
                KeyEvent::Escape | KeyEvent::Find | KeyEvent::Enter => {
                    self.search_mode = false;
                }
                KeyEvent::Backspace => {
                    self.search_query.pop();
                }
                KeyEvent::Char(c) if !c.is_control() && self.search_query.chars().count() < 28 => {
                    self.search_query.push(c);
                }
                _ => {}
            }
            return None;
        }
        match key {
            KeyEvent::Save => return self.save_current(lang),
            KeyEvent::Find => {
                self.search_mode = true;
            }
            KeyEvent::Undo => {
                if self.undo() {
                    self.saved_msg = String::from(tr(lang, "Отменено", "Undone"));
                }
            }
            KeyEvent::Up => {
                self.scroll = self.scroll.saturating_sub(1);
            }
            KeyEvent::Down => {
                if self.scroll + 1 < self.lines.len() {
                    self.scroll += 1;
                }
            }
            KeyEvent::Paste(txt) => {
                self.push_undo();
                for (i, line) in txt.lines().enumerate() {
                    let clean: String = line.chars().filter(|c| !c.is_control()).take(60).collect();
                    if i == 0 {
                        if self.lines.is_empty() {
                            self.lines.push(String::new());
                        }
                        if let Some(last) = self.lines.last_mut() {
                            for ch in clean.chars() {
                                if last.chars().count() < 60 {
                                    last.push(ch);
                                }
                            }
                        }
                    } else if self.lines.len() < 300 {
                        self.lines.push(clean);
                    }
                }
                self.dirty = true;
            }
            KeyEvent::Char(c) => {
                self.push_undo();
                if self.lines.is_empty() {
                    self.lines.push(String::new());
                }
                if let Some(last) = self.lines.last_mut()
                    && last.chars().count() < 60
                {
                    last.push(c);
                    self.dirty = true;
                }
            }
            KeyEvent::Enter => {
                if self.lines.len() < 300 {
                    self.push_undo();
                    self.lines.push(String::new());
                    self.dirty = true;
                }
            }
            KeyEvent::Backspace => {
                self.push_undo();
                if let Some(last) = self.lines.last_mut()
                    && last.pop().is_none()
                    && self.lines.len() > 1
                {
                    self.lines.pop();
                }
                self.dirty = true;
            }
            _ => {}
        }
        None
    }

    pub fn on_click(&mut self, lx: i32, ly: i32, wh: i32, lang: Lang) -> Option<String> {
        if ly >= wh - 34 && ly <= wh - 10 {
            if (14..=86).contains(&lx) {
                return self.save_current(lang);
            } else if (92..=154).contains(&lx) {
                self.search_mode = !self.search_mode;
            } else if (160..=218).contains(&lx) {
                self.undo();
            } else if (224..=284).contains(&lx) {
                self.push_undo();
                self.lines = vec![String::new()];
                self.scroll = 0;
                self.dirty = true;
                self.saved_msg = String::from(tr(lang, "Очищено!", "Cleared!"));
            }
        }
        None
    }

    #[allow(clippy::too_many_arguments)]
    pub fn draw(&self, canvas: &mut Canvas, wx: i32, wy: i32, ww: i32, wh: i32, lang: Lang) {
        let x = wx + 12;
        let top_off = if self.search_mode || !self.search_query.is_empty() { 58 } else { 34 };
        let y = wy + top_off;
        let iw = ww - 24;
        let ih = (wh - top_off - 42).max(40);
        if top_off > 34 {
            canvas.fill_rounded_rect(x, wy + 33, iw, 20, 6, 0x161D2C);
            canvas.draw_rounded_rect_alpha(
                x,
                wy + 33,
                iw,
                20,
                6,
                if self.search_mode { 0xF59E0B } else { 0x475569 },
                140,
            );
            canvas.draw_text_clipped(
                x + 8,
                wy + 36,
                iw - 16,
                &format!("{}: {}_", tr(lang, "Поиск", "Find"), self.search_query),
                0xFCD34D,
                1,
            );
        }
        canvas.fill_rounded_rect(x, y, iw, ih, 10, 0x0B0E15);
        canvas.draw_rounded_rect_alpha(x, y, iw, ih, 10, 0xFFFFFF, 24);
        canvas.blend_rect(x + 12, y + 1, (iw - 24).max(0), 1, 0xFFFFFF, 16);
        canvas.blend_rect(x + 28, y + 4, 1, (ih - 8).max(0), 0xFFFFFF, 14);

        let max_lines = ((ih - 8) / 15).max(1) as usize;
        let start = self.scroll.min(self.lines.len().saturating_sub(1));
        for (row, line) in self.lines.iter().skip(start).take(max_lines).enumerate() {
            let line_no = start + row + 1;
            let ry = y + 8 + (row as i32) * 15;
            let hit = !self.search_query.is_empty()
                && line.to_lowercase().contains(&self.search_query.to_lowercase());
            if hit {
                canvas.blend_rect(x + 30, ry - 1, (iw - 34).max(0), 14, 0xF59E0B, 42);
            }
            canvas.draw_text_clipped(x + 4, ry, 22, &format!("{line_no:>2}"), 0x64748B, 1);
            let cursor = if line_no == self.lines.len() && !self.search_mode { "_" } else { "" };
            canvas.draw_text_clipped(
                x + 34,
                ry,
                iw - 40,
                &format!("{line}{cursor}"),
                if hit { 0xFDE68A } else { 0xE6EDF3 },
                1,
            );
        }

        let by = wy + wh - 34;
        canvas.draw_button(wx + 14, by, 72, 22, tr(lang, "Сохр.", "Save"), 0x238636, 0xFFFFFF, 0x3FB950);
        canvas.draw_button(
            wx + 92,
            by,
            62,
            22,
            tr(lang, "Поиск", "Find"),
            if self.search_mode { 0xD97706 } else { 0x21262D },
            0xFFFFFF,
            0xF59E0B,
        );
        canvas.draw_button(wx + 160, by, 58, 22, tr(lang, "Отм.", "Undo"), 0x21262D, 0xFFFFFF, 0x8B5CF6);
        canvas.draw_button(wx + 224, by, 60, 22, tr(lang, "Сбр.", "Clr"), 0x21262D, 0xFFFFFF, 0x58A6FF);

        let chars: usize = self.lines.iter().map(|l| l.chars().count()).sum();
        let words: usize = self.lines.iter().map(|l| l.split_whitespace().count()).sum();
        let mark = if self.dirty { "*" } else { "" };
        let stat = format!("{}L{} {}W {}C · {}", mark, self.lines.len(), words, chars, self.saved_msg);
        canvas.draw_text_clipped(wx + 290, by + 5, (ww - 298).max(40), &stat, 0x94A3B8, 1);
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
    ResetDefaults,
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
pub const SCREEN_RES_PRESETS: [(i32, i32); 4] = [
    (800, 600),
    (1024, 768),
    (1280, 800),
    (1440, 900),
];

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
    pub res_idx: usize,
    pub ai_api_key: String,
    pub ai_model_idx: usize,
    pub ai_provider: u8,
    pub ai_base_url: String,
    pub ai_custom_model: String,
    pub ai_thinking_idx: usize,
    pub ai_thinking_mask: u8,
    pub ai_show_full_thinking: bool,
    pub ai_temp_idx: u8,
    pub ai_system_prompt: String,
    pub crosshair_cursor: bool,
    pub show_clock: bool,
    pub persistent_island: bool,
    pub aero_stars: bool,
    pub vista_sidebar: bool,
    pub night_shift: bool,
    pub crt_scanlines: bool,
    pub macos_buttons: bool,
    pub snake_tick_div: u64,
    pub device_name: String,
    pub editing_device: bool,
    pub update_taps: u8,
    pub license_alt: bool,
    pub tab_pill_y: f32,
    pub tab_pill_vel: f32,
    pub tab_fade: f32,
    pub toggle_anim: [f32; 10],
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
            res_idx: 0,
            ai_api_key: String::new(),
            ai_model_idx: 0,
            ai_provider: 0,
            ai_base_url: String::new(),
            ai_custom_model: String::new(),
            ai_thinking_idx: 2,
            ai_thinking_mask: 0b0011_1111,
            ai_show_full_thinking: false,
            ai_temp_idx: 1,
            ai_system_prompt: String::new(),
            crosshair_cursor: false,
            show_clock: true,
            persistent_island: false,
            aero_stars: true,
            vista_sidebar: true,
            night_shift: false,
            crt_scanlines: false,
            macos_buttons: false,
            snake_tick_div: 10,
            device_name: String::from("pizdOS PC"),
            editing_device: false,
            update_taps: 0,
            license_alt: false,
            tab_pill_y: 90.0,
            tab_pill_vel: 0.0,
            tab_fade: 1.0,
            toggle_anim: [0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 1.0, 1.0, 0.0, 0.0],
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
    pub fn resolution(&self) -> (i32, i32) {
        SCREEN_RES_PRESETS
            .get(self.res_idx)
            .copied()
            .unwrap_or((800, 600))
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
            self.persistent_island,
            self.macos_buttons,
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
                } else if (254..=277).contains(&ly) && iw >= 160 {
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
                } else if (280..=308).contains(&ly) {
                    let pill_x0 = (ww - 196).max(146);
                    if lx >= pill_x0 {
                        let idx = (((lx - pill_x0) * 4) / 184).clamp(0, 3) as usize;
                        self.ui_scale_pct = UI_SCALES[idx];
                    } else {
                        self.step_ui_scale(1);
                    }
                } else if (311..=341).contains(&ly) {
                    let seg_w = 248;
                    let seg_x0 = (ww - 260).max(232);
                    if lx >= seg_x0 {
                        let idx = (((lx - seg_x0) * 4) / seg_w).clamp(0, 3) as usize;
                        self.res_idx = idx;
                    } else {
                        self.res_idx = (self.res_idx + 1) % SCREEN_RES_PRESETS.len();
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
                } else if (264..298).contains(&ly) {
                    self.persistent_island = !self.persistent_island;
                } else if (298..332).contains(&ly) {
                    self.macos_buttons = !self.macos_buttons;
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
                } else if (282..310).contains(&ly) {
                    return SettingsAction::ResetDefaults;
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
                let sw_h = 22;
                if sw_y + sw_h <= wy + wh - 6 && iw >= 160 {
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
                        canvas.fill_luminous_card(sx, sw_y, sw_w, sw_h, 8, tl, br, ba, rib);
                        let selected = *preset == self.theme;
                        canvas.draw_rounded_rect_alpha(
                            sx,
                            sw_y,
                            sw_w,
                            sw_h,
                            8,
                            0xFFFFFF,
                            if selected { 175 } else { 28 },
                        );
                        if selected {
                            canvas.fill_aa_circle(
                                (sx + sw_w - 10) as f32,
                                (sw_y + 11) as f32,
                                3.0,
                                0xFFFFFF,
                                240,
                            );
                        }
                    }
                }

                // Card 4: Segmented UI Scale Switcher (100% / 125% / 150% / 175%)
                let sc_y = wy + 280;
                if sc_y + 28 <= wy_base + wh - 4 {
                    Self::draw_card_frame(canvas, x, sc_y, iw, 28, 10);
                    let mid_y = sc_y + 14;
                    canvas.fill_aa_circle((x + 19) as f32, mid_y as f32, 5.5, 0x2DD4BF, 32);
                    canvas.fill_aa_circle((x + 19) as f32, mid_y as f32, 2.2, 0x2DD4BF, 240);
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
                    canvas.fill_rounded_rect(seg_x0, sc_y + 3, seg_w, 22, 7, 0x0E121B);
                    let cell_w = seg_w / 4;
                    for (idx, &pct) in UI_SCALES.iter().enumerate() {
                        let px = seg_x0 + (idx as i32) * cell_w + 2;
                        let pw = cell_w - 4;
                        let active = self.ui_scale_pct == pct;
                        if active {
                            canvas.blend_rounded_rect(px, sc_y + 5, pw, 18, 5, accent, 170);
                            canvas.draw_rounded_rect_alpha(px, sc_y + 5, pw, 18, 5, 0xFFFFFF, 85);
                        } else if in_content
                            && (280..=308).contains(&hover_ly)
                            && hover_lx >= (px - wx)
                            && hover_lx < (px - wx + pw)
                        {
                            canvas.blend_rounded_rect(px, sc_y + 5, pw, 18, 5, 0xFFFFFF, 18);
                        }
                        let lbl = format!("{pct}%");
                        let tw = canvas.measure_text(&lbl, 1);
                        canvas.draw_text_clipped(
                            px + ((pw - tw) / 2).max(2),
                            sc_y + 7,
                            pw - 4,
                            &lbl,
                            if active { 0xFFFFFF } else { 0x94A3B8 },
                            1,
                        );
                    }
                }

                // Card 5: Screen Resolution Switcher (800x600 default)
                let res_y = wy + 312;
                if res_y + 28 <= wy_base + wh - 4 {
                    Self::draw_card_frame(canvas, x, res_y, iw, 28, 10);
                    let mid_y = res_y + 14;
                    canvas.fill_aa_circle((x + 19) as f32, mid_y as f32, 5.5, 0x38BDF8, 32);
                    canvas.fill_aa_circle((x + 19) as f32, mid_y as f32, 2.2, 0x38BDF8, 240);
                    canvas.draw_text_clipped(
                        x + 32,
                        mid_y - 7,
                        56,
                        tr(lang, "Экран", "Screen"),
                        0xE2E8F0,
                        1,
                    );
                    let seg_w = 248;
                    let seg_x0 = (wx + ww - 260).max(x + 86);
                    canvas.fill_rounded_rect(seg_x0, res_y + 3, seg_w, 22, 7, 0x0E121B);
                    let cell_w = seg_w / 4;
                    for (idx, &(rw, rh)) in SCREEN_RES_PRESETS.iter().enumerate() {
                        let px = seg_x0 + (idx as i32) * cell_w + 2;
                        let pw = cell_w - 4;
                        let active = self.res_idx == idx;
                        if active {
                            canvas.blend_rounded_rect(px, res_y + 5, pw, 18, 5, accent, 170);
                            canvas.draw_rounded_rect_alpha(px, res_y + 5, pw, 18, 5, 0xFFFFFF, 85);
                        } else if in_content
                            && (311..=341).contains(&hover_ly)
                            && hover_lx >= (px - wx)
                            && hover_lx < (px - wx + pw)
                        {
                            canvas.blend_rounded_rect(px, res_y + 5, pw, 18, 5, 0xFFFFFF, 18);
                        }
                        let lbl = format!("{rw}x{rh}");
                        let tw = canvas.measure_text(&lbl, 1);
                        canvas.draw_text_clipped(
                            px + ((pw - tw) / 2).max(1),
                            res_y + 7,
                            pw - 2,
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
                Self::draw_card_frame(canvas, x, c_y, iw, 272, 12);
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
                Self::draw_toggle_row(
                    canvas,
                    x,
                    c_y + 204,
                    iw,
                    34,
                    0x10B981,
                    tr(lang, "Dynamic Island всегда", "Always-On Dynamic Island"),
                    self.toggle_anim[8],
                    accent,
                    in_content && (264..298).contains(&hover_ly),
                    true,
                );
                Self::draw_toggle_row(
                    canvas,
                    x,
                    c_y + 238,
                    iw,
                    34,
                    0xFB7185,
                    tr(lang, "Кнопки окон macOS", "macOS Traffic Lights"),
                    self.toggle_anim[9],
                    accent,
                    in_content && (298..332).contains(&hover_ly),
                    true,
                );
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
                if wy + 310 <= wy + wh {
                    canvas.draw_button(
                        x,
                        wy + 282,
                        iw.min(180),
                        26,
                        tr(lang, "Сбросить настройки", "Reset Defaults"),
                        0x7F1D1D,
                        0xFECACA,
                        0xF87171,
                    );
                }
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
                if pill_x + 62 < x + iw - 96 {
                    canvas.blend_rounded_rect(pill_x, y + 16, 56, 19, 9, 0xFFFFFF, 28);
                    canvas.draw_rounded_rect_alpha(pill_x, y + 16, 56, 19, 9, 0xFFFFFF, 82);
                    canvas.draw_text_clipped(pill_x + 7, y + 18, 46, "2.1(00)", 0xFFFFFF, 1);
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
// 6. Calculator (Pro + Sci + History + OxygenOS Never Settle)
// ---------------------------------------------------------------------------
pub struct CalcState {
    pub display: String,
    pub acc: Option<i64>,
    pub op: Option<char>,
    pub clear_next: bool,
    pub never_settle: bool,
    pub flash: Option<(usize, f32)>,
    pub history: Vec<String>,
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
            never_settle: false,
            flash: None,
            history: Vec::new(),
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
                self.never_settle = false;
            }
            "x2" => {
                self.never_settle = false;
                let v = self.display.parse::<i64>().unwrap_or(0);
                let sq = v.saturating_mul(v);
                self.push_hist(format!("{v}^2={sq}"));
                self.display = sq.to_string();
                self.clear_next = true;
            }
            "sq" => {
                self.never_settle = false;
                let v = self.display.parse::<i64>().unwrap_or(0);
                if v < 0 {
                    self.display = String::from("ERR");
                } else {
                    let r = (v as f64).sqrt().round() as i64;
                    self.push_hist(format!("sqrt({v})={r}"));
                    self.display = r.to_string();
                }
                self.clear_next = true;
            }
            "+-" => {
                self.never_settle = false;
                let v = self.display.parse::<i64>().unwrap_or(0);
                self.display = (-v).to_string();
            }
            "%" => {
                self.never_settle = false;
                self.eval_pending();
                self.acc = Some(self.display.parse::<i64>().unwrap_or(0));
                self.op = Some('%');
                self.clear_next = true;
            }
            "+" | "-" | "*" | "/" => {
                self.never_settle = false;
                self.eval_pending();
                self.acc = Some(self.display.parse::<i64>().unwrap_or(0));
                self.op = token.chars().next();
                self.clear_next = true;
            }
            "=" => {
                if self.acc == Some(1) && self.op == Some('+') && self.clear_next {
                    self.display = String::from("Never Settle");
                    self.never_settle = true;
                    self.push_hist(String::from("1+=NeverSettle"));
                } else {
                    self.never_settle = false;
                    let lhs = self.acc;
                    let op = self.op;
                    let rhs = self.display.parse::<i64>().unwrap_or(0);
                    self.eval_pending();
                    if let (Some(a), Some(c)) = (lhs, op) {
                        self.push_hist(format!("{a}{c}{rhs}={}", self.display));
                    }
                }
                self.op = None;
                self.acc = None;
                self.clear_next = true;
            }
            digit => {
                self.never_settle = false;
                if self.clear_next || self.display == "0" || self.display == "ERR" {
                    self.display = digit.to_string();
                    self.clear_next = false;
                } else if self.display.len() < 16 {
                    self.display.push_str(digit);
                }
            }
        }
    }

    fn push_hist(&mut self, entry: String) {
        self.history.push(entry);
        if self.history.len() > 6 {
            self.history.remove(0);
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
            '%' => lhs.checked_rem(rhs),
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
            KeyEvent::Char('%') => self.press("%"),
            KeyEvent::Char('=') | KeyEvent::Enter => self.press("="),
            KeyEvent::Char('c') | KeyEvent::Char('C') => self.press("C"),
            KeyEvent::Backspace => {
                if self.never_settle {
                    self.never_settle = false;
                    self.clear_next = false;
                    self.display = String::from("0");
                } else if self.display.len() > 1 {
                    self.display.pop();
                } else {
                    self.display = String::from("0");
                }
            }
            _ => {}
        }
    }

    pub fn on_click(&mut self, lx: i32, ly: i32, ww: i32, wh: i32) {
        let x0 = 12;
        let iw = ww - 24;
        if iw >= 40 && (78..=98).contains(&ly) {
            let sw = iw / 4;
            let idx = ((lx - x0) / sw).clamp(0, 3);
            match idx {
                0 => self.press("x2"),
                1 => self.press("sq"),
                2 => self.press("%"),
                _ => self.press("+-"),
            }
            return;
        }
        let gx = 12;
        let gy = 102;
        let gw = ww - 24;
        let gh = wh - 112;
        if gw < 4 || gh < 4 || lx < gx || ly < gy || lx >= gx + gw || ly >= gy + gh {
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

    #[allow(clippy::too_many_arguments)]
    pub fn draw(&self, canvas: &mut Canvas, wx: i32, wy: i32, ww: i32, wh: i32, accent: u32) {
        let x = wx + 12;
        let y = wy + 34;
        let iw = ww - 24;
        if self.never_settle {
            canvas.fill_rounded_rect(x, y, iw, 40, 10, 0x22080E);
            canvas.draw_rounded_rect_alpha(x, y, iw, 40, 10, 0xEF4444, 185);
            canvas.blend_rect(x + 12, y + 1, (iw - 24).max(0), 1, 0xF87171, 80);
            canvas.draw_text_clipped(x + 12, y + 3, iw - 24, "OnePlus · OxygenOS", 0xFCA5A5, 1);
            let scale = if canvas.measure_text(&self.display, 2) <= iw - 24 {
                2
            } else {
                1
            };
            let ty = if scale == 2 { y + 15 } else { y + 20 };
            canvas.draw_text_clipped(x + 12, ty, iw - 24, &self.display, 0xEF4444, scale);
        } else {
            canvas.fill_rounded_rect(x, y, iw, 40, 10, 0x090D14);
            canvas.draw_rounded_rect_alpha(x, y, iw, 40, 10, 0xFFFFFF, 26);
            if let (Some(a), Some(op)) = (self.acc, self.op) {
                canvas.draw_text_clipped(x + 12, y + 3, iw / 2, &format!("{a} {op}"), 0x79C0FF, 1);
            } else if let Some(last) = self.history.last() {
                canvas.draw_text_clipped(x + 12, y + 3, iw - 24, last, 0x64748B, 1);
            }
            canvas.draw_text_clipped(x + 12, y + 15, iw - 24, &self.display, 0xFFFFFF, 2);
        }

        let sci_y = wy + 78;
        let sw = iw / 4;
        for (i, lbl) in ["x^2", "sqrt", "mod", "+/-"].iter().enumerate() {
            canvas.draw_button(
                x + (i as i32) * sw + 2,
                sci_y,
                sw - 4,
                20,
                lbl,
                0x161D2C,
                0x93C5FD,
                0x334155,
            );
        }

        let gy = wy + 102;
        let gh = wh - 112;
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

// ---------------------------------------------------------------------------
// 8. Game 2048
// ---------------------------------------------------------------------------
pub struct Game2048State {
    pub grid: [[u32; 4]; 4],
    pub score: u32,
    pub best: u32,
    pub game_over: bool,
    rng: u64,
}

impl Game2048State {
    pub fn new() -> Self {
        let mut g = Self {
            grid: [[0; 4]; 4],
            score: 0,
            best: 0,
            game_over: false,
            rng: 0x2048_1337_CAFE_BABE,
        };
        g.spawn();
        g.spawn();
        g
    }

    pub fn reset(&mut self) {
        self.grid = [[0; 4]; 4];
        self.score = 0;
        self.game_over = false;
        self.spawn();
        self.spawn();
    }

    fn next_rand(&mut self) -> u64 {
        let mut x = self.rng;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.rng = x;
        x
    }

    fn spawn(&mut self) {
        let mut empty = Vec::with_capacity(16);
        for r in 0..4 {
            for c in 0..4 {
                if self.grid[r][c] == 0 {
                    empty.push((r, c));
                }
            }
        }
        if empty.is_empty() {
            return;
        }
        let rnd = self.next_rand();
        let (r, c) = empty[(rnd as usize) % empty.len()];
        self.grid[r][c] = if (rnd >> 8).is_multiple_of(10) { 4 } else { 2 };
    }

    fn slide_line(line: [u32; 4]) -> ([u32; 4], u32) {
        let mut vals = [0u32; 4];
        let mut len = 0usize;
        for &v in &line {
            if v != 0 {
                vals[len] = v;
                len += 1;
            }
        }
        let mut out = [0u32; 4];
        let mut out_len = 0usize;
        let mut gain = 0u32;
        let mut i = 0usize;
        while i < len {
            if i + 1 < len && vals[i] == vals[i + 1] {
                let merged = vals[i] * 2;
                out[out_len] = merged;
                gain += merged;
                out_len += 1;
                i += 2;
            } else {
                out[out_len] = vals[i];
                out_len += 1;
                i += 1;
            }
        }
        (out, gain)
    }

    fn can_move(&self) -> bool {
        for r in 0..4 {
            for c in 0..4 {
                let v = self.grid[r][c];
                if v == 0 {
                    return true;
                }
                if r + 1 < 4 && self.grid[r + 1][c] == v {
                    return true;
                }
                if c + 1 < 4 && self.grid[r][c + 1] == v {
                    return true;
                }
            }
        }
        false
    }

    /// dir: 0=Left, 1=Right, 2=Up, 3=Down. Returns true if the board changed.
    pub fn move_dir(&mut self, dir: u8) -> bool {
        if self.game_over {
            return false;
        }
        let prev = self.grid;
        let mut gained = 0u32;
        for i in 0..4 {
            let line = match dir {
                0 => self.grid[i],
                1 => [self.grid[i][3], self.grid[i][2], self.grid[i][1], self.grid[i][0]],
                2 => [self.grid[0][i], self.grid[1][i], self.grid[2][i], self.grid[3][i]],
                _ => [self.grid[3][i], self.grid[2][i], self.grid[1][i], self.grid[0][i]],
            };
            let (slid, g) = Self::slide_line(line);
            gained += g;
            match dir {
                0 => self.grid[i] = slid,
                1 => self.grid[i] = [slid[3], slid[2], slid[1], slid[0]],
                2 => {
                    for (r, &v) in slid.iter().enumerate() {
                        self.grid[r][i] = v;
                    }
                }
                _ => {
                    for (r, &v) in slid.iter().enumerate() {
                        self.grid[3 - r][i] = v;
                    }
                }
            }
        }
        if self.grid != prev {
            self.score += gained;
            if self.score > self.best {
                self.best = self.score;
            }
            self.spawn();
            self.game_over = !self.can_move();
            true
        } else {
            self.game_over = !self.can_move();
            false
        }
    }

    pub fn on_key(&mut self, key: KeyEvent) {
        match key {
            KeyEvent::Left | KeyEvent::Char('a') | KeyEvent::Char('A') => {
                self.move_dir(0);
            }
            KeyEvent::Right | KeyEvent::Char('d') | KeyEvent::Char('D') => {
                self.move_dir(1);
            }
            KeyEvent::Up | KeyEvent::Char('w') | KeyEvent::Char('W') => {
                self.move_dir(2);
            }
            KeyEvent::Down | KeyEvent::Char('s') | KeyEvent::Char('S') => {
                self.move_dir(3);
            }
            KeyEvent::Char('r') | KeyEvent::Char('R') => self.reset(),
            _ => {}
        }
    }

    pub fn on_click(&mut self, lx: i32, ly: i32, ww: i32, wh: i32) {
        if ly >= wh - 34 && ly <= wh - 8 && (12..=112).contains(&lx) {
            self.reset();
            return;
        }
        // Optional D-pad click zones inside board quadrants when mouse-playing
        let gx = 12;
        let gy = 58;
        let gw = ww - 24;
        let gh = wh - 98;
        if lx >= gx && lx < gx + gw && ly >= gy && ly < gy + gh {
            let dx = lx - (gx + gw / 2);
            let dy = ly - (gy + gh / 2);
            if dx.abs() > dy.abs() {
                self.move_dir(if dx < 0 { 0 } else { 1 });
            } else {
                self.move_dir(if dy < 0 { 2 } else { 3 });
            }
        }
    }

    fn tile_style(v: u32) -> (u32, u32) {
        match v {
            0 => (0x161D2B, 0x475569),
            2 => (0x1E293B, 0xF8FAFC),
            4 => (0x283548, 0xF8FAFC),
            8 => (0x9A3412, 0xFFFFFF),
            16 => (0xC2410C, 0xFFFFFF),
            32 => (0xB91C1C, 0xFFFFFF),
            64 => (0xDC2626, 0xFFFFFF),
            128 => (0xCA8A04, 0xFFFFFF),
            256 => (0xEAB308, 0x0F172A),
            512 => (0x2563EB, 0xFFFFFF),
            1024 => (0x7C3AED, 0xFFFFFF),
            _ => (0x10B981, 0xFFFFFF),
        }
    }

    #[allow(clippy::too_many_arguments)]
    #[allow(clippy::too_many_arguments)]
    #[allow(clippy::too_many_arguments)]
    pub fn draw(&self, canvas: &mut Canvas, wx: i32, wy: i32, ww: i32, wh: i32, accent: u32, lang: Lang) {
        let x = wx + 12;
        let y = wy + 34;
        let iw = (ww - 24).max(160);
        canvas.draw_text_clipped(
            x,
            y,
            iw - 90,
            &format!("{}: {}   Best: {}", tr(lang, "Счёт", "Score"), self.score, self.best),
            0xF8FAFC,
            1,
        );
        if self.game_over {
            canvas.draw_text_clipped(x + iw - 82, y, 80, "GAME OVER", 0xF85149, 1);
        }

        let gx = x;
        let gy = wy + 58;
        let gw = iw;
        let gh = (wh - 98).max(140);
        canvas.fill_rounded_rect(gx, gy, gw, gh, 12, 0x0B0F19);
        canvas.draw_rounded_rect_alpha(gx, gy, gw, gh, 12, 0xFFFFFF, 26);

        let gap = 6;
        let cw = ((gw - gap * 5) / 4).max(20);
        let ch = ((gh - gap * 5) / 4).max(20);
        for r in 0..4 {
            for c in 0..4 {
                let tx = gx + gap + (c as i32) * (cw + gap);
                let ty = gy + gap + (r as i32) * (ch + gap);
                let val = self.grid[r][c];
                let (bg, fg) = Self::tile_style(val);
                canvas.fill_rounded_rect(tx, ty, cw, ch, 8, bg);
                canvas.draw_rounded_rect_alpha(tx, ty, cw, ch, 8, 0xFFFFFF, if val == 0 { 16 } else { 42 });
                if val > 0 {
                    let s = val.to_string();
                    let scale = if val < 100 && cw >= 42 { 2 } else { 1 };
                    let tw = canvas.measure_text(&s, scale);
                    let th = if scale == 2 { 20 } else { 12 };
                    canvas.draw_text_clipped(
                        tx + ((cw - tw) / 2).max(2),
                        ty + ((ch - th) / 2).max(2),
                        cw - 4,
                        &s,
                        fg,
                        scale,
                    );
                }
            }
        }

        canvas.draw_button(
            wx + 12,
            wy + wh - 34,
            100,
            24,
            tr(lang, "Новая (R)", "New (R)"),
            0x1E293B,
            0xF8FAFC,
            accent,
        );
        canvas.draw_text_clipped(
            wx + 122,
            wy + wh - 28,
            (ww - 134).max(40),
            tr(lang, "Стрелки / WASD", "Arrows / WASD"),
            0x94A3B8,
            1,
        );
    }
}

// ---------------------------------------------------------------------------
// 9. PizView (Zero-Dependency BMP Image Viewer)
// ---------------------------------------------------------------------------
pub struct PizViewState {
    pub pixels: Vec<u32>,
    pub img_w: usize,
    pub img_h: usize,
    pub title: String,
    pub rot90: u8,
    pub zoom2x: bool,
    pub status: String,
}

impl PizViewState {
    pub fn new(lang: Lang) -> Self {
        let mut s = Self {
            pixels: Vec::new(),
            img_w: 64,
            img_h: 64,
            title: String::from("Sample"),
            rot90: 0,
            zoom2x: false,
            status: String::from(tr(lang, "Открой .bmp в Файлах", "Open .bmp in Files")),
        };
        s.load_demo();
        s
    }

    fn load_demo(&mut self) {
        self.img_w = 64;
        self.img_h = 64;
        self.pixels.resize(64 * 64, 0);
        for y in 0..64 {
            for x in 0..64 {
                let r = (x * 4) as u32;
                let g = (y * 3) as u32;
                let b = (140 + ((x ^ y) & 63) * 2).min(255) as u32;
                self.pixels[y * 64 + x] = (r << 16) | (g << 8) | b;
            }
        }
    }

    pub fn open_bmp(&mut self, path: &Path, lang: Lang) -> Result<String, String> {
        let data = fs::read(path).map_err(|e| format!("Read err: {e}"))?;
        if data.len() < 54 || &data[0..2] != b"BM" {
            return Err(String::from("Not a valid BMP"));
        }
        let off = u32::from_le_bytes([data[10], data[11], data[12], data[13]]) as usize;
        let w = i32::from_le_bytes([data[18], data[19], data[20], data[21]]);
        let h_raw = i32::from_le_bytes([data[22], data[23], data[24], data[25]]);
        let bpp = u16::from_le_bytes([data[28], data[29]]);
        let comp = u32::from_le_bytes([data[30], data[31], data[32], data[33]]);
        if w <= 0 || h_raw == 0 || (bpp != 24 && bpp != 32) || (comp != 0 && comp != 3) {
            return Err(String::from("Unsupported BMP format"));
        }
        let iw = w as usize;
        let ih = h_raw.unsigned_abs() as usize;
        if iw > 4096 || ih > 4096 || iw * ih > 16_000_000 {
            return Err(String::from("BMP too large"));
        }
        let top_down = h_raw < 0;
        let bytes_pp = (bpp / 8) as usize;
        let row_stride = (iw * bytes_pp).div_ceil(4) * 4;
        if off + row_stride * ih > data.len() {
            return Err(String::from("Truncated BMP data"));
        }
        let mut pix = vec![0u32; iw * ih];
        for r in 0..ih {
            let src_r = if top_down { r } else { ih - 1 - r };
            let base = off + src_r * row_stride;
            for c in 0..iw {
                let p = base + c * bytes_pp;
                let b = data[p] as u32;
                let g = data[p + 1] as u32;
                let red = data[p + 2] as u32;
                pix[r * iw + c] = (red << 16) | (g << 8) | b;
            }
        }
        self.pixels = pix;
        self.img_w = iw;
        self.img_h = ih;
        self.rot90 = 0;
        self.title = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| String::from("image.bmp"));
        self.status = format!("{} ({}x{})", self.title, iw, ih);
        Ok(format!("{}: {}", tr(lang, "Открыт BMP", "Opened BMP"), self.title))
    }

    pub fn on_click(&mut self, lx: i32, ly: i32, wh: i32) {
        if ly >= wh - 34 && ly <= wh - 10 {
            if (14..=96).contains(&lx) {
                self.rot90 = (self.rot90 + 1) % 4;
            } else if (102..=184).contains(&lx) {
                self.zoom2x = !self.zoom2x;
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    #[allow(clippy::too_many_arguments)]
    pub fn draw(&self, canvas: &mut Canvas, wx: i32, wy: i32, ww: i32, wh: i32, accent: u32, lang: Lang) {
        let vx = wx + 12;
        let vy = wy + 34;
        let vw = (ww - 24).max(40);
        let vh = (wh - 76).max(40);
        canvas.fill_rounded_rect(vx, vy, vw, vh, 10, 0x090D16);
        canvas.draw_rounded_rect_alpha(vx, vy, vw, vh, 10, 0xFFFFFF, 26);

        let (rw, rh) = if self.rot90 % 2 == 1 {
            (self.img_h, self.img_w)
        } else {
            (self.img_w, self.img_h)
        };
        if rw > 0 && rh > 0 && !self.pixels.is_empty() {
            let avail_w = (vw - 12).max(8) as f32;
            let avail_h = (vh - 12).max(8) as f32;
            let mut scale = (avail_w / rw as f32).min(avail_h / rh as f32);
            if self.zoom2x {
                scale *= 1.65;
            }
            let dw = ((rw as f32 * scale).round() as i32).clamp(4, vw - 8);
            let dh = ((rh as f32 * scale).round() as i32).clamp(4, vh - 8);
            let dx0 = vx + (vw - dw) / 2;
            let dy0 = vy + (vh - dh) / 2;
            for py in 0..dh {
                for px in 0..dw {
                    let rx = (px as usize * rw) / (dw as usize);
                    let ry = (py as usize * rh) / (dh as usize);
                    let (sx, sy) = match self.rot90 {
                        1 => (ry, self.img_h - 1 - rx),
                        2 => (self.img_w - 1 - rx, self.img_h - 1 - ry),
                        3 => (self.img_w - 1 - ry, rx),
                        _ => (rx, ry),
                    };
                    if sx < self.img_w && sy < self.img_h {
                        canvas.fill_rect(dx0 + px, dy0 + py, 1, 1, self.pixels[sy * self.img_w + sx]);
                    }
                }
            }
        }

        let by = wy + wh - 34;
        canvas.draw_button(wx + 14, by, 82, 22, tr(lang, "Повор. 90", "Rot 90"), 0x1E293B, 0xFFFFFF, accent);
        canvas.draw_button(
            wx + 102,
            by,
            82,
            22,
            if self.zoom2x { "Zoom 1x" } else { "Zoom 2x" },
            if self.zoom2x { accent } else { 0x1E293B },
            0xFFFFFF,
            accent,
        );
        canvas.draw_text_clipped(wx + 192, by + 5, (ww - 204).max(40), &self.status, 0x94A3B8, 1);
    }
}

// ---------------------------------------------------------------------------
// 10. PizSweeper (Minesweeper 9x9)
// ---------------------------------------------------------------------------
pub const MS_N: usize = 9;
pub const MS_MINES: usize = 10;

pub struct MinesweeperState {
    pub mines: [bool; MS_N * MS_N],
    pub revealed: [bool; MS_N * MS_N],
    pub flagged: [bool; MS_N * MS_N],
    pub adj: [u8; MS_N * MS_N],
    pub started: bool,
    pub won: bool,
    pub lost: bool,
    pub flag_mode: bool,
    rng: u64,
}

impl MinesweeperState {
    pub fn new() -> Self {
        Self {
            mines: [false; MS_N * MS_N],
            revealed: [false; MS_N * MS_N],
            flagged: [false; MS_N * MS_N],
            adj: [0; MS_N * MS_N],
            started: false,
            won: false,
            lost: false,
            flag_mode: false,
            rng: 0xBADC_0FFE_EE11_2233,
        }
    }

    pub fn reset(&mut self) {
        self.mines = [false; MS_N * MS_N];
        self.revealed = [false; MS_N * MS_N];
        self.flagged = [false; MS_N * MS_N];
        self.adj = [0; MS_N * MS_N];
        self.started = false;
        self.won = false;
        self.lost = false;
    }

    fn next_rand(&mut self) -> usize {
        let mut x = self.rng;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.rng = x;
        x as usize
    }

    fn place_mines(&mut self, safe_idx: usize) {
        let mut placed = 0;
        while placed < MS_MINES {
            let idx = self.next_rand() % (MS_N * MS_N);
            if idx != safe_idx && !self.mines[idx] {
                self.mines[idx] = true;
                placed += 1;
            }
        }
        for r in 0..MS_N {
            for c in 0..MS_N {
                let mut cnt = 0u8;
                for dr in -1i32..=1 {
                    for dc in -1i32..=1 {
                        if dr == 0 && dc == 0 {
                            continue;
                        }
                        let nr = r as i32 + dr;
                        let nc = c as i32 + dc;
                        if (0..MS_N as i32).contains(&nr)
                            && (0..MS_N as i32).contains(&nc)
                            && self.mines[nr as usize * MS_N + nc as usize]
                        {
                            cnt += 1;
                        }
                    }
                }
                self.adj[r * MS_N + c] = cnt;
            }
        }
        self.started = true;
    }

    pub fn reveal_cell(&mut self, r: usize, c: usize) {
        if self.won || self.lost || r >= MS_N || c >= MS_N {
            return;
        }
        let idx = r * MS_N + c;
        if self.flagged[idx] || self.revealed[idx] {
            return;
        }
        if !self.started {
            self.place_mines(idx);
        }
        if self.mines[idx] {
            self.revealed[idx] = true;
            self.lost = true;
            for i in 0..(MS_N * MS_N) {
                if self.mines[i] {
                    self.revealed[i] = true;
                }
            }
            return;
        }
        let mut stack = vec![(r, c)];
        while let Some((cr, cc)) = stack.pop() {
            let ci = cr * MS_N + cc;
            if self.revealed[ci] || self.flagged[ci] || self.mines[ci] {
                continue;
            }
            self.revealed[ci] = true;
            if self.adj[ci] == 0 {
                for dr in -1i32..=1 {
                    for dc in -1i32..=1 {
                        let nr = cr as i32 + dr;
                        let nc = cc as i32 + dc;
                        if (0..MS_N as i32).contains(&nr) && (0..MS_N as i32).contains(&nc) {
                            stack.push((nr as usize, nc as usize));
                        }
                    }
                }
            }
        }
        let safe_left = (0..(MS_N * MS_N)).any(|i| !self.mines[i] && !self.revealed[i]);
        if !safe_left {
            self.won = true;
        }
    }

    pub fn toggle_flag(&mut self, r: usize, c: usize) {
        if self.won || self.lost || r >= MS_N || c >= MS_N {
            return;
        }
        let idx = r * MS_N + c;
        if !self.revealed[idx] {
            self.flagged[idx] = !self.flagged[idx];
        }
    }

    pub fn on_click(&mut self, lx: i32, ly: i32, ww: i32, wh: i32, right: bool) {
        if !right && ly >= wh - 34 && ly <= wh - 8 {
            if (12..=94).contains(&lx) {
                self.reset();
            } else if (100..=188).contains(&lx) {
                self.flag_mode = !self.flag_mode;
            }
            return;
        }
        let gx = 12;
        let gy = 56;
        let gw = (ww - 24).max(36);
        let gh = (wh - 96).max(36);
        let cw = gw / MS_N as i32;
        let ch = gh / MS_N as i32;
        if lx >= gx && ly >= gy && lx < gx + cw * MS_N as i32 && ly < gy + ch * MS_N as i32 {
            let c = ((lx - gx) / cw) as usize;
            let r = ((ly - gy) / ch) as usize;
            if right || self.flag_mode {
                self.toggle_flag(r, c);
            } else {
                self.reveal_cell(r, c);
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    #[allow(clippy::too_many_arguments)]
    pub fn draw(&self, canvas: &mut Canvas, wx: i32, wy: i32, ww: i32, wh: i32, accent: u32, lang: Lang) {
        let flags = self.flagged.iter().filter(|&&f| f).count() as i32;
        let mines_left = MS_MINES as i32 - flags;
        let st = if self.won {
            tr(lang, "ПОБЕДА!", "YOU WIN!")
        } else if self.lost {
            tr(lang, "БАБАХ!", "BOOM!")
        } else {
            tr(lang, "Сапёр 9x9", "Minesweeper")
        };
        let st_col = if self.won {
            0x34D399
        } else if self.lost {
            0xFB7185
        } else {
            0xE2E8F0
        };
        canvas.draw_text_clipped(wx + 14, wy + 36, ww - 28, &format!("{st}  ·  M:{mines_left}"), st_col, 1);

        let gx = wx + 12;
        let gy = wy + 56;
        let gw = (ww - 24).max(36);
        let gh = (wh - 96).max(36);
        let cw = (gw / MS_N as i32).max(10);
        let ch = (gh / MS_N as i32).max(10);

        for r in 0..MS_N {
            for c in 0..MS_N {
                let idx = r * MS_N + c;
                let cx = gx + (c as i32) * cw + 1;
                let cy = gy + (r as i32) * ch + 1;
                let bw = (cw - 2).max(6);
                let bh = (ch - 2).max(6);
                if self.revealed[idx] {
                    if self.mines[idx] {
                        canvas.fill_rounded_rect(cx, cy, bw, bh, 4, 0x991B1B);
                        canvas.draw_text_clipped(cx + bw / 3, cy + bh / 4, bw, "*", 0xFECACA, 1);
                    } else {
                        canvas.fill_rounded_rect(cx, cy, bw, bh, 4, 0x0B101B);
                        canvas.draw_rounded_rect_alpha(cx, cy, bw, bh, 4, 0xFFFFFF, 16);
                        let n = self.adj[idx];
                        if n > 0 {
                            let col = match n {
                                1 => 0x60A5FA,
                                2 => 0x4ADE80,
                                3 => 0xF87171,
                                _ => 0xFBBF24,
                            };
                            canvas.draw_text_clipped(cx + bw / 3, cy + bh / 4, bw, &n.to_string(), col, 1);
                        }
                    }
                } else {
                    canvas.fill_rounded_rect(cx, cy, bw, bh, 4, 0x1E293B);
                    canvas.draw_rounded_rect_alpha(cx, cy, bw, bh, 4, 0xFFFFFF, 42);
                    if self.flagged[idx] {
                        canvas.fill_aa_circle((cx + bw / 2) as f32, (cy + bh / 2) as f32, 3.6, 0xF97316, 255);
                    }
                }
            }
        }

        let by = wy + wh - 34;
        canvas.draw_button(wx + 12, by, 82, 22, tr(lang, "Новая", "New"), 0x1E293B, 0xFFFFFF, accent);
        canvas.draw_button(
            wx + 100,
            by,
            88,
            22,
            tr(lang, "Флаг", "Flag"),
            if self.flag_mode { accent } else { 0x1E293B },
            0xFFFFFF,
            0xF97316,
        );
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AiProvider {
    Gemini,
    OpenAi,
    Anthropic,
}

impl AiProvider {
    pub fn from_u8(v: u8) -> Self {
        match v {
            1 => Self::OpenAi,
            2 => Self::Anthropic,
            _ => Self::Gemini,
        }
    }

    pub fn as_u8(self) -> u8 {
        match self {
            Self::Gemini => 0,
            Self::OpenAi => 1,
            Self::Anthropic => 2,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Gemini => "Google AI Studio",
            Self::OpenAi => "OpenAI-compat",
            Self::Anthropic => "Anthropic-compat",
        }
    }

    pub fn default_base_url(self) -> &'static str {
        match self {
            Self::Gemini => "https://generativelanguage.googleapis.com/v1beta",
            Self::OpenAi => "https://api.openai.com/v1",
            Self::Anthropic => "https://api.anthropic.com/v1",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThinkingLevel {
    Off,
    Low,
    Medium,
    High,
    XHigh,
    Max,
}

pub const ALL_THINKING_LEVELS: [ThinkingLevel; 6] = [
    ThinkingLevel::Off,
    ThinkingLevel::Low,
    ThinkingLevel::Medium,
    ThinkingLevel::High,
    ThinkingLevel::XHigh,
    ThinkingLevel::Max,
];

impl ThinkingLevel {
    pub fn from_idx(i: usize) -> Self {
        ALL_THINKING_LEVELS[i.min(ALL_THINKING_LEVELS.len() - 1)]
    }

    pub fn idx(self) -> usize {
        match self {
            Self::Off => 0,
            Self::Low => 1,
            Self::Medium => 2,
            Self::High => 3,
            Self::XHigh => 4,
            Self::Max => 5,
        }
    }

    pub fn bit(self) -> u8 {
        1u8 << self.idx()
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Off => "Off",
            Self::Low => "Low",
            Self::Medium => "Med",
            Self::High => "High",
            Self::XHigh => "XHigh",
            Self::Max => "Max",
        }
    }
}

pub struct AiModelSpec {
    pub label: &'static str,
    pub model_id: &'static str,
    pub provider: AiProvider,
    pub thinking: &'static [ThinkingLevel],
}

pub const AI_MODELS: [AiModelSpec; 9] = [
    AiModelSpec {
        label: "Gemini 3.5 Flash-Lite",
        model_id: "gemini-flash-lite-latest",
        provider: AiProvider::Gemini,
        thinking: &[ThinkingLevel::Off, ThinkingLevel::Low, ThinkingLevel::Medium, ThinkingLevel::High],
    },
    AiModelSpec {
        label: "Gemini 4 Hydrogen",
        model_id: "gemini-flash-latest",
        provider: AiProvider::Gemini,
        thinking: &[ThinkingLevel::Low, ThinkingLevel::Medium, ThinkingLevel::High, ThinkingLevel::XHigh],
    },
    AiModelSpec {
        label: "DeepSeek V4.1 Flash",
        model_id: "deepseek-flash",
        provider: AiProvider::OpenAi,
        thinking: &[ThinkingLevel::Off, ThinkingLevel::Low, ThinkingLevel::Medium, ThinkingLevel::High],
    },
    AiModelSpec {
        label: "DeepSeek V4 Pro",
        model_id: "deepseek-v4-pro",
        provider: AiProvider::OpenAi,
        thinking: &[ThinkingLevel::Low, ThinkingLevel::Medium, ThinkingLevel::High, ThinkingLevel::Max],
    },
    AiModelSpec {
        label: "Kimi K3.1 Pro",
        model_id: "kimi-k3.1-pro",
        provider: AiProvider::OpenAi,
        thinking: &[ThinkingLevel::Low, ThinkingLevel::Medium, ThinkingLevel::High],
    },
    AiModelSpec {
        label: "GLM-5.3",
        model_id: "GLM-5.3",
        provider: AiProvider::OpenAi,
        thinking: &[ThinkingLevel::Off, ThinkingLevel::Low, ThinkingLevel::Medium, ThinkingLevel::High],
    },
    AiModelSpec {
        label: "Claude Opus 5.5",
        model_id: "claude-opus-5-5",
        provider: AiProvider::Anthropic,
        thinking: &[ThinkingLevel::Off, ThinkingLevel::Low, ThinkingLevel::Medium, ThinkingLevel::High, ThinkingLevel::Max],
    },
    AiModelSpec {
        label: "Claude Sonnet 5.5",
        model_id: "claude-sonnet-5-5",
        provider: AiProvider::Anthropic,
        thinking: &[ThinkingLevel::Off, ThinkingLevel::Low, ThinkingLevel::Medium, ThinkingLevel::High],
    },
    AiModelSpec {
        label: "Claude Fable 5.2",
        model_id: "claude-fable-5-2",
        provider: AiProvider::Anthropic,
        thinking: &[ThinkingLevel::Low, ThinkingLevel::High, ThinkingLevel::Max],
    },
];

pub fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 8);
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {}
            c => out.push(c),
        }
    }
    out
}

fn unescape_json_str(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut chars = raw.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('n') => out.push('\n'),
                Some('r') => {}
                Some('t') => out.push('\t'),
                Some('"') => out.push('"'),
                Some('\\') => out.push('\\'),
                Some('/') => out.push('/'),
                Some('u') => {
                    let hex: String = chars.by_ref().take(4).collect();
                    if let Ok(code) = u32::from_str_radix(&hex, 16)
                        && let Some(uc) = char::from_u32(code)
                    {
                        out.push(uc);
                    }
                }
                Some(other) => out.push(other),
                None => break,
            }
        } else {
            out.push(c);
        }
    }
    out
}

fn extract_json_field_all(json: &str, key: &str) -> Vec<String> {
    let pat = format!("\"{key}\"");
    let mut results = Vec::new();
    let mut scan = json;
    while let Some(pos) = scan.find(&pat) {
        let rest = &scan[pos + pat.len()..];
        let Some(colon) = rest.find(':') else {
            scan = rest;
            continue;
        };
        let after = rest[colon + 1..].trim_start();
        if let Some(stripped) = after.strip_prefix('"') {
            let mut end = 0usize;
            let bytes = stripped.as_bytes();
            while end < bytes.len() {
                if bytes[end] == b'\\' {
                    end += 2;
                    continue;
                }
                if bytes[end] == b'"' {
                    results.push(unescape_json_str(&stripped[..end]));
                    scan = &stripped[end + 1..];
                    break;
                }
                end += 1;
            }
            if end >= bytes.len() {
                break;
            }
        } else {
            scan = after;
        }
    }
    results
}

fn extract_json_field(json: &str, key: &str) -> Option<String> {
    extract_json_field_all(json, key).into_iter().next()
}

fn extract_json_int(json: &str, key: &str) -> Option<i32> {
    let pat = format!("\"{key}\"");
    let pos = json.find(&pat)?;
    let rest = &json[pos + pat.len()..];
    let colon = rest.find(':')?;
    let after = rest[colon + 1..].trim_start();
    let num: String = after
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '-')
        .collect();
    num.parse::<i32>().ok()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AiReply {
    pub answer: String,
    pub thought: Option<String>,
}

fn split_gemini_parts(json: &str) -> (String, Option<String>) {
    let Some(pos) = json.find("\"parts\"") else {
        return (
            extract_json_field_all(json, "text").into_iter().last().unwrap_or_default(),
            None,
        );
    };
    let tail = &json[pos..];
    let Some(arr_s) = tail.find('[') else {
        return (
            extract_json_field_all(json, "text").into_iter().last().unwrap_or_default(),
            None,
        );
    };
    let mut depth = 0i32;
    let mut in_str = false;
    let mut esc = false;
    let mut obj_start: Option<usize> = None;
    let mut thoughts = Vec::new();
    let mut answers = Vec::new();
    for (idx, &b) in tail.as_bytes()[arr_s..].iter().enumerate() {
        let abs = arr_s + idx;
        if esc {
            esc = false;
            continue;
        }
        if in_str {
            if b == b'\\' {
                esc = true;
            } else if b == b'"' {
                in_str = false;
            }
            continue;
        }
        match b {
            b'"' => in_str = true,
            b'[' if depth == 0 => depth = 1,
            b']' if depth == 1 => break,
            b'{' => {
                if depth == 1 {
                    obj_start = Some(abs);
                }
                depth += 1;
            }
            b'}' => {
                depth -= 1;
                if depth == 1
                    && let Some(os) = obj_start.take()
                {
                    let slice = &tail[os..=abs];
                    if let Some(t) = extract_json_field(slice, "text")
                        && !t.trim().is_empty()
                    {
                        let is_thought = slice.contains("\"thought\":true") || slice.contains("\"thought\": true");
                        if is_thought {
                            thoughts.push(t);
                        } else {
                            answers.push(t);
                        }
                    }
                }
            }
            _ => {}
        }
    }
    let thought = if thoughts.is_empty() {
        None
    } else {
        Some(thoughts.join("\n"))
    };
    let answer = if answers.is_empty() {
        extract_json_field_all(json, "text").into_iter().last().unwrap_or_default()
    } else {
        answers.join("")
    };
    (answer, thought)
}

pub fn extract_ai_parts(provider: AiProvider, json: &str) -> Result<AiReply, String> {
    if json.contains("\"error\"")
        && let Some(err_msg) = extract_json_field(json, "message")
    {
        return Err(err_msg);
    }
    let (answer, thought) = match provider {
        AiProvider::Gemini => split_gemini_parts(json),
        AiProvider::OpenAi => {
            let th = extract_json_field(json, "reasoning_content")
                .or_else(|| extract_json_field(json, "reasoning"));
            let ans = extract_json_field(json, "content").unwrap_or_default();
            (ans, th)
        }
        AiProvider::Anthropic => {
            // Never expose Anthropic thinking per policy/preference
            let ans = extract_json_field_all(json, "text")
                .into_iter()
                .last()
                .unwrap_or_default();
            (ans, None)
        }
    };
    if answer.is_empty() && thought.is_none() {
        return Err(String::from("Empty or invalid response from API"));
    }
    Ok(AiReply { answer, thought })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CuAction {
    Move(i32, i32),
    Click(i32, i32),
    DoubleClick(i32, i32),
    Type(String),
    Key(String),
    Wait,
    Done(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CuResponse {
    pub action: CuAction,
    pub say: Option<String>,
}

fn find_action_json_slice(reply: &str) -> Option<&str> {
    let act_pos = reply.find("\"action\"")?;
    let start = reply[..act_pos].rfind('{')?;
    let bytes = reply.as_bytes();
    let mut depth = 0i32;
    let mut in_str = false;
    let mut esc = false;
    for (rel, &b) in bytes[start..].iter().enumerate() {
        if esc {
            esc = false;
            continue;
        }
        if in_str {
            if b == b'\\' {
                esc = true;
            } else if b == b'"' {
                in_str = false;
            }
            continue;
        }
        match b {
            b'"' => in_str = true,
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(&reply[start..=start + rel]);
                }
            }
            _ => {}
        }
    }
    None
}

pub fn parse_cu_response(reply: &str) -> Option<CuResponse> {
    let obj = find_action_json_slice(reply).or_else(|| {
        let s = reply.find('{')?;
        let e = reply.rfind('}')?;
        (e > s).then_some(&reply[s..=e])
    })?;
    let say = extract_json_field(obj, "say")
        .or_else(|| extract_json_field(obj, "note"))
        .filter(|s| !s.trim().is_empty());
    let act = extract_json_field(obj, "action")?.to_ascii_lowercase();
    let action = match act.as_str() {
        "click" | "left_click" | "tap" => CuAction::Click(
            extract_json_int(obj, "x").unwrap_or(0),
            extract_json_int(obj, "y").unwrap_or(0),
        ),
        "double_click" => CuAction::DoubleClick(
            extract_json_int(obj, "x").unwrap_or(0),
            extract_json_int(obj, "y").unwrap_or(0),
        ),
        "move" | "mouse_move" => CuAction::Move(
            extract_json_int(obj, "x").unwrap_or(0),
            extract_json_int(obj, "y").unwrap_or(0),
        ),
        "type" | "input" | "write" => CuAction::Type(extract_json_field(obj, "text")?),
        "key" | "press" => CuAction::Key(
            extract_json_field(obj, "name").or_else(|| extract_json_field(obj, "key"))?,
        ),
        "wait" => CuAction::Wait,
        "done" | "finish" => CuAction::Done(
            extract_json_field(obj, "summary").unwrap_or_else(|| String::from("Done.")),
        ),
        _ => return None,
    };
    Some(CuResponse { action, say })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AiEditField {
    Prompt,
    ApiKey,
    BaseUrl,
    CustomModel,
    SystemPrompt,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AiPopup {
    None,
    Effort,
    Models,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AiMsgKind {
    User,
    Assistant,
    Thought,
    CuStep,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AiMsg {
    pub kind: AiMsgKind,
    pub text: String,
}

const AI_TEMPS: [f32; 3] = [0.2, 0.7, 1.0];

pub struct AiChatState {
    pub provider: AiProvider,
    pub api_key: String,
    pub base_url: String,
    pub custom_model: String,
    pub system_prompt: String,
    pub model_idx: usize,
    pub thinking: ThinkingLevel,
    pub custom_thinking_mask: u8,
    pub show_full_thinking: bool,
    pub temp_idx: u8,
    pub key_visible: bool,
    pub cfg_open: bool,
    pub popup: AiPopup,
    pub model_scroll: usize,
    pub edit_field: AiEditField,
    pub cu_enabled: bool,
    pub cu_active: bool,
    pub cu_goal: String,
    pub cu_history: Vec<String>,
    pub cu_steps: u8,
    pub cu_wait_ticks: u8,
    pub cu_needs_capture: bool,
    pub input: String,
    pub messages: Vec<AiMsg>,
    pub busy: bool,
    pub spin_tick: u64,
    rx: Option<std::sync::mpsc::Receiver<Result<AiReply, String>>>,
}

impl AiChatState {
    pub fn new(settings: &SettingsState) -> Self {
        let model_idx = settings.ai_model_idx.min(AI_MODELS.len() - 1);
        let provider = AiProvider::from_u8(settings.ai_provider);
        let mut s = Self {
            provider,
            api_key: settings.ai_api_key.clone(),
            base_url: settings.ai_base_url.clone(),
            custom_model: settings.ai_custom_model.clone(),
            system_prompt: settings.ai_system_prompt.clone(),
            model_idx,
            thinking: ThinkingLevel::from_idx(settings.ai_thinking_idx),
            custom_thinking_mask: if settings.ai_thinking_mask == 0 {
                0x3F
            } else {
                settings.ai_thinking_mask & 0x3F
            },
            show_full_thinking: settings.ai_show_full_thinking,
            temp_idx: settings.ai_temp_idx.min(2),
            key_visible: false,
            cfg_open: false,
            popup: AiPopup::None,
            model_scroll: 0,
            edit_field: AiEditField::Prompt,
            cu_enabled: false,
            cu_active: false,
            cu_goal: String::new(),
            cu_history: Vec::new(),
            cu_steps: 0,
            cu_wait_ticks: 0,
            cu_needs_capture: false,
            input: String::new(),
            messages: vec![AiMsg {
                kind: AiMsgKind::Assistant,
                text: String::from(
                    "PizAI (Google AI Studio / OpenAI / Anthropic + Computer Use). Жми пилюлю модели снизу или Opts!",
                ),
            }],
            busy: false,
            spin_tick: 0,
            rx: None,
        };
        s.clamp_thinking();
        s
    }

    pub fn push_msg(&mut self, kind: AiMsgKind, text: impl Into<String>) {
        self.messages.push(AiMsg {
            kind,
            text: text.into(),
        });
    }

    pub fn active_model_id(&self) -> String {
        let cm = self.custom_model.trim();
        if cm.is_empty() {
            String::from(AI_MODELS[self.model_idx.min(AI_MODELS.len() - 1)].model_id)
        } else {
            String::from(cm)
        }
    }

    pub fn active_model_label(&self) -> String {
        let cm = self.custom_model.trim();
        if cm.is_empty() {
            String::from(AI_MODELS[self.model_idx.min(AI_MODELS.len() - 1)].label)
        } else {
            String::from(cm)
        }
    }

    pub fn supported_thinking(&self) -> Vec<ThinkingLevel> {
        let preset = AI_MODELS[self.model_idx.min(AI_MODELS.len() - 1)].thinking;
        if !self.custom_model.trim().is_empty() || preset.is_empty() {
            let list: Vec<ThinkingLevel> = ALL_THINKING_LEVELS
                .iter()
                .copied()
                .filter(|t| (self.custom_thinking_mask & t.bit()) != 0)
                .collect();
            if !list.is_empty() {
                return list;
            }
            return vec![ThinkingLevel::Low, ThinkingLevel::High];
        }
        preset.to_vec()
    }

    pub fn clamp_thinking(&mut self) {
        let allowed = self.supported_thinking();
        if !allowed.contains(&self.thinking) {
            let cur = self.thinking.idx() as i32;
            if let Some(&best) = allowed.iter().min_by_key(|t| (t.idx() as i32 - cur).abs()) {
                self.thinking = best;
            }
        }
    }

    pub fn select_model(&mut self, idx: usize) {
        self.model_idx = idx.min(AI_MODELS.len() - 1);
        self.provider = AI_MODELS[self.model_idx].provider;
        self.custom_model.clear();
        self.clamp_thinking();
    }

    pub fn poll(&mut self) -> Option<String> {
        self.spin_tick = self.spin_tick.wrapping_add(1);
        let rx = self.rx.as_ref()?;
        match rx.try_recv() {
            Ok(res) => {
                self.busy = false;
                self.rx = None;
                match res {
                    Ok(reply) => {
                        if let Some(th) = reply.thought
                            && self.provider != AiProvider::Anthropic
                            && !th.trim().is_empty()
                        {
                            self.push_msg(AiMsgKind::Thought, th);
                        }
                        if !self.cu_active && !reply.answer.is_empty() {
                            self.push_msg(AiMsgKind::Assistant, reply.answer.clone());
                        }
                        Some(reply.answer)
                    }
                    Err(err) => {
                        self.cu_active = false;
                        self.push_msg(AiMsgKind::Assistant, format!("[ERR] {err}"));
                        None
                    }
                }
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => None,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                self.busy = false;
                self.cu_active = false;
                self.rx = None;
                self.push_msg(AiMsgKind::Assistant, "[ERR] Worker disconnected");
                None
            }
        }
    }

    pub fn begin_prompt(&mut self) {
        let prompt = self.input.trim().to_string();
        if prompt.is_empty() || self.busy {
            return;
        }
        if self.api_key.trim().is_empty() && self.base_url.trim().is_empty() {
            self.push_msg(AiMsgKind::Assistant, "Сначала введи API Key в Opts.");
            self.cfg_open = true;
            self.popup = AiPopup::None;
            self.edit_field = AiEditField::ApiKey;
            return;
        }
        self.popup = AiPopup::None;
        self.cfg_open = false;
        self.input.clear();
        self.push_msg(AiMsgKind::User, prompt.clone());
        if self.cu_enabled {
            self.cu_active = true;
            self.cu_goal = prompt;
            self.cu_history.clear();
            self.cu_steps = 0;
            self.cu_wait_ticks = 0;
            self.cu_needs_capture = true;
            self.busy = true;
        } else {
            let sys = self.system_prompt.trim().to_string();
            let full = if sys.is_empty() {
                prompt
            } else {
                format!("{sys}\n\n{prompt}")
            };
            self.dispatch_api(&full, None);
        }
    }

    pub fn dispatch_cu_screen(&mut self, png_b64: &str, sw: i32, sh: i32, ui_hint: &str) {
        self.cu_needs_capture = false;
        self.cu_steps += 1;
        let hist = if self.cu_history.is_empty() {
            String::from("none")
        } else {
            self.cu_history.join("; ")
        };
        let prompt = format!(
            "You control pizdOS desktop ({sw}x{sh} px). Coordinates MUST be exact pixel values in x=0..{sw}, y=0..{sh} (NOT 0..1000). \
             Desktop icons open with a SINGLE click. {ui_hint}\n\
             Goal: {}\n\
             Previous steps ({}/10): {hist}\n\
             Inspect the attached PNG screenshot and output ONLY ONE JSON object (no markdown). \
             Optionally include \"say\":\"brief note to user\" to comment on what you are doing right now:\n\
             {{\"action\":\"click\",\"x\":100,\"y\":200,\"say\":\"Opening...\"}} OR {{\"action\":\"type\",\"text\":\"...\",\"say\":\"Typing...\"}} OR \
             {{\"action\":\"key\",\"name\":\"enter\"}} OR {{\"action\":\"wait\",\"say\":\"Waiting...\"}} OR {{\"action\":\"done\",\"summary\":\"...\"}}",
            self.cu_goal, self.cu_steps
        );
        self.dispatch_api(&prompt, Some(png_b64));
    }

    fn dispatch_api(&mut self, prompt: &str, png_b64: Option<&str>) {
        self.busy = true;
        let provider = self.provider;
        let key = self.api_key.trim().to_string();
        let base = if self.base_url.trim().is_empty() {
            provider.default_base_url().to_string()
        } else {
            self.base_url.trim().trim_end_matches('/').to_string()
        };
        let model_id = self.active_model_id();
        let thinking = self.thinking;
        let temp = AI_TEMPS[self.temp_idx.min(2) as usize];

        let body = build_ai_request_json(provider, &model_id, thinking, prompt, png_b64, temp);
        let url = match provider {
            AiProvider::Gemini => format!("{base}/models/{model_id}:generateContent?key={key}"),
            AiProvider::OpenAi => format!("{base}/chat/completions"),
            AiProvider::Anthropic => format!("{base}/messages"),
        };

        let (tx, rx) = std::sync::mpsc::channel();
        self.rx = Some(rx);
        std::thread::spawn(move || {
            let tmp = std::env::temp_dir().join("piz_ai_req.json");
            if let Err(e) = fs::write(&tmp, body.as_bytes()) {
                let _ = tx.send(Err(format!("Write temp req failed: {e}")));
                return;
            }
            let at_path = format!("@{}", tmp.display());
            let mut cmd = Command::new("curl");
            cmd.args([
                "-sS",
                "--max-time",
                "45",
                "-X",
                "POST",
                "-H",
                "Content-Type: application/json",
            ]);
            match provider {
                AiProvider::Gemini => {}
                AiProvider::OpenAi => {
                    if !key.is_empty() {
                        cmd.arg("-H").arg(format!("Authorization: Bearer {key}"));
                    }
                }
                AiProvider::Anthropic => {
                    cmd.arg("-H")
                        .arg(format!("x-api-key: {key}"))
                        .arg("-H")
                        .arg("anthropic-version: 2023-06-01");
                }
            }
            cmd.arg("-d").arg(&at_path).arg(&url);
            let out = cmd.output();
            let _ = fs::remove_file(&tmp);
            let res = match out {
                Ok(o) if o.status.success() => {
                    let s = String::from_utf8_lossy(&o.stdout);
                    extract_ai_parts(provider, &s)
                }
                Ok(o) => {
                    let err = String::from_utf8_lossy(&o.stderr).trim().to_string();
                    Err(if err.is_empty() {
                        format!("curl exit {}", o.status)
                    } else {
                        err
                    })
                }
                Err(e) => Err(format!("curl not found ({e}). Install: piz-get install curl")),
            };
            let _ = tx.send(res);
        });
    }

    pub fn on_char(&mut self, ch: char) {
        if ch.is_control() {
            return;
        }
        match self.edit_field {
            AiEditField::ApiKey if self.api_key.len() < 160 => self.api_key.push(ch),
            AiEditField::BaseUrl if self.base_url.len() < 160 => self.base_url.push(ch),
            AiEditField::CustomModel if self.custom_model.len() < 80 => {
                self.custom_model.push(ch);
                self.clamp_thinking();
            }
            AiEditField::SystemPrompt if self.system_prompt.len() < 320 => self.system_prompt.push(ch),
            AiEditField::Prompt if self.input.len() < 512 => self.input.push(ch),
            _ => {}
        }
    }

    pub fn on_backspace(&mut self) {
        match self.edit_field {
            AiEditField::ApiKey => {
                self.api_key.pop();
            }
            AiEditField::BaseUrl => {
                self.base_url.pop();
            }
            AiEditField::CustomModel => {
                self.custom_model.pop();
                self.clamp_thinking();
            }
            AiEditField::SystemPrompt => {
                self.system_prompt.pop();
            }
            AiEditField::Prompt => {
                self.input.pop();
            }
        }
    }

    pub fn on_enter(&mut self) -> bool {
        if self.edit_field != AiEditField::Prompt {
            self.edit_field = AiEditField::Prompt;
            return true;
        }
        self.begin_prompt();
        false
    }

    pub fn paste(&mut self, text: &str) {
        for c in text.chars().filter(|c| !c.is_control()) {
            self.on_char(c);
        }
    }

    fn popover_rect(&self, ww: i32, wh: i32) -> (i32, i32, i32, i32) {
        let pw = 250.min(ww - 20).max(200);
        let ph = match self.popup {
            AiPopup::Models => 188,
            _ => 126,
        };
        let px = (ww - pw - 10).max(10);
        let py = (wh - 38 - ph).max(58);
        (px, py, pw, ph)
    }

    pub fn on_click(&mut self, lx: i32, ly: i32, ww: i32, wh: i32) -> bool {
        let mut changed = false;

        if self.popup != AiPopup::None {
            let (px, py, pw, ph) = self.popover_rect(ww, wh);
            if lx >= px && lx <= px + pw && ly >= py && ly <= py + ph {
                match self.popup {
                    AiPopup::Effort => {
                        if ly >= py + 28 && ly <= py + 56 {
                            self.popup = AiPopup::Models;
                            return false;
                        }
                        if ly >= py + 58 && ly <= py + 104 {
                            let allowed = self.supported_thinking();
                            if !allowed.is_empty() {
                                let tx0 = px + 14;
                                let tw = (pw - 28).max(40);
                                if allowed.len() == 1 {
                                    self.thinking = allowed[0];
                                } else {
                                    let rel = (lx - tx0).clamp(0, tw) as f32 / tw as f32;
                                    let idx = (rel * (allowed.len() - 1) as f32).round() as usize;
                                    self.thinking = allowed[idx.min(allowed.len() - 1)];
                                }
                                changed = true;
                            }
                        }
                        return changed;
                    }
                    AiPopup::Models => {
                        if ly <= py + 32 {
                            if lx <= px + 46 {
                                self.popup = AiPopup::Effort;
                            }
                            return false;
                        }
                        let row_y = ly - (py + 34);
                        if row_y >= 0 {
                            if lx >= px + pw - 26 {
                                if row_y < 70 {
                                    self.model_scroll = self.model_scroll.saturating_sub(1);
                                } else if self.model_scroll + 5 < AI_MODELS.len() {
                                    self.model_scroll += 1;
                                }
                                return false;
                            }
                            let idx = self.model_scroll + (row_y / 28) as usize;
                            if idx < AI_MODELS.len() {
                                self.select_model(idx);
                                self.popup = AiPopup::Effort;
                                return true;
                            }
                        }
                        return false;
                    }
                    AiPopup::None => {}
                }
            }
        }

        if (34..=56).contains(&ly) {
            if lx >= ww - 144 && lx <= ww - 82 {
                self.cu_enabled = !self.cu_enabled;
                if !self.cu_enabled {
                    self.cu_active = false;
                }
                return false;
            }
            if lx >= ww - 76 && lx <= ww - 10 {
                self.cfg_open = !self.cfg_open;
                self.popup = AiPopup::None;
                if !self.cfg_open {
                    self.edit_field = AiEditField::Prompt;
                    changed = true;
                }
                return changed;
            }
        }

        if self.cfg_open && ly >= 62 && ly <= wh - 42 {
            let cx = 10;
            let cw = (ww - 20).max(80);
            if (68..=90).contains(&ly) {
                let bw = (cw - 8) / 3;
                let idx = ((lx - cx) / (bw + 4)).clamp(0, 2) as u8;
                self.provider = AiProvider::from_u8(idx);
                if let Some((mi, _)) = AI_MODELS
                    .iter()
                    .enumerate()
                    .find(|(_, m)| m.provider == self.provider)
                {
                    self.model_idx = mi;
                }
                self.clamp_thinking();
                return true;
            }
            if (96..=120).contains(&ly) {
                if lx >= cx + cw - 44 {
                    self.base_url.clear();
                    return true;
                }
                self.edit_field = AiEditField::BaseUrl;
            } else if (124..=148).contains(&ly) {
                if lx >= cx + cw - 44 {
                    self.key_visible = !self.key_visible;
                    return false;
                }
                self.edit_field = AiEditField::ApiKey;
            } else if (152..=176).contains(&ly) {
                if lx >= cx + cw - 44 {
                    self.custom_model.clear();
                    self.clamp_thinking();
                    return true;
                }
                self.edit_field = AiEditField::CustomModel;
            } else if (180..=204).contains(&ly) {
                self.edit_field = AiEditField::SystemPrompt;
            } else if (210..=232).contains(&ly) {
                if lx < cx + cw / 2 {
                    self.temp_idx = (self.temp_idx + 1) % 3;
                    return true;
                } else if self.provider != AiProvider::Anthropic {
                    self.show_full_thinking = !self.show_full_thinking;
                    return true;
                }
            } else if (238..=260).contains(&ly) {
                let chip_w = (cw / 6).max(36);
                let ti = ((lx - cx) / chip_w).clamp(0, 5) as usize;
                let bit = ALL_THINKING_LEVELS[ti].bit();
                let next = self.custom_thinking_mask ^ bit;
                if next != 0 {
                    self.custom_thinking_mask = next;
                    self.clamp_thinking();
                    changed = true;
                }
            }
            return changed;
        }

        if ly >= wh - 34 && ly <= wh - 10 {
            let send_x = ww - 58;
            let pill_x = ww - 224;
            if lx >= send_x && lx <= ww - 10 {
                if self.busy || self.cu_active {
                    self.cu_active = false;
                    self.cu_needs_capture = false;
                    self.busy = false;
                    self.rx = None;
                    self.push_msg(AiMsgKind::Assistant, "[Stopped]");
                } else {
                    self.begin_prompt();
                }
            } else if lx >= pill_x && lx < send_x - 4 {
                self.popup = match self.popup {
                    AiPopup::None => AiPopup::Effort,
                    AiPopup::Effort | AiPopup::Models => AiPopup::None,
                };
            } else {
                self.popup = AiPopup::None;
                self.edit_field = AiEditField::Prompt;
            }
            return changed;
        }

        self.popup = AiPopup::None;
        changed
    }

    #[allow(clippy::too_many_arguments)]
    pub fn draw(
        &self,
        canvas: &mut Canvas,
        wx: i32,
        wy: i32,
        ww: i32,
        wh: i32,
        accent: u32,
        lang: Lang,
    ) {
        canvas.draw_text_clipped(
            wx + 12,
            wy + 37,
            ww - 160,
            &format!("PizAI · {}", self.provider.label()),
            0x94A3B8,
            1,
        );
        let cu_lbl = if self.cu_enabled { "CU: ON" } else { "CU:OFF" };
        canvas.draw_button(
            wx + ww - 144,
            wy + 34,
            62,
            22,
            cu_lbl,
            if self.cu_enabled { 0x059669 } else { 0x1E293B },
            0xFFFFFF,
            0x10B981,
        );
        canvas.draw_button(
            wx + ww - 76,
            wy + 34,
            66,
            22,
            if self.cfg_open { tr(lang, "Чат", "Chat") } else { tr(lang, "Опции", "Opts") },
            if self.cfg_open { accent } else { 0x1E293B },
            0xFFFFFF,
            accent,
        );

        let cx = wx + 10;
        let cy = wy + 62;
        let cw = (ww - 20).max(80);
        let ch = (wh - 102).max(60);
        canvas.fill_rounded_rect(cx, cy, cw, ch, 8, 0x0B101B);
        canvas.draw_rounded_rect_alpha(cx, cy, cw, ch, 8, 0xFFFFFF, 20);

        if self.cfg_open {
            self.draw_settings(canvas, cx, wy, cw, accent, lang);
        } else {
            self.draw_chat_history(canvas, cx, cy, cw, ch, lang);
        }

        let iy = wy + wh - 34;
        let pill_x = wx + ww - 224;
        let iw = (ww - 238).max(40);
        canvas.fill_rounded_rect(wx + 10, iy, iw, 24, 6, 0x0F172A);
        canvas.draw_rounded_rect_alpha(wx + 10, iy, iw, 24, 6, accent, 80);
        let disp = if self.input.is_empty() {
            String::from(if self.cu_enabled {
                tr(lang, "Задача для CU...", "Task for CU...")
            } else {
                tr(lang, "Сообщение...", "Ask AI...")
            })
        } else if self.edit_field == AiEditField::Prompt {
            format!("{}_", self.input)
        } else {
            self.input.clone()
        };
        let dcol = if self.input.is_empty() { 0x64748B } else { 0xF8FAFC };
        canvas.draw_text_clipped(wx + 16, iy + 5, iw - 12, &disp, dcol, 1);

        let pill_lbl = format!("{} · {}", self.active_model_label(), self.thinking.label());
        canvas.draw_button(
            pill_x,
            iy,
            160,
            24,
            &pill_lbl,
            if self.popup != AiPopup::None { accent } else { 0x1E293B },
            0xE2E8F0,
            accent,
        );
        canvas.draw_button(
            wx + ww - 58,
            iy,
            48,
            24,
            if self.busy || self.cu_active { "Stop" } else { ">" },
            if self.busy || self.cu_active { 0xEF4444 } else { accent },
            0xFFFFFF,
            accent,
        );

        if self.popup != AiPopup::None {
            self.draw_popover(canvas, wx, wy, ww, wh, accent);
        }
    }

    fn draw_chat_history(&self, canvas: &mut Canvas, cx: i32, cy: i32, cw: i32, ch: i32, lang: Lang) {
        let spin = ['-', '\\', '|', '/'][(self.spin_tick / 5) as usize % 4];
        let mut lines: Vec<(u32, String)> = Vec::new();
        let max_chars = ((cw - 20) / 7).max(16) as usize;
        for m in &self.messages {
            match m.kind {
                AiMsgKind::Thought => {
                    if self.provider == AiProvider::Anthropic {
                        continue;
                    }
                    if self.show_full_thinking {
                        for (idx, raw_line) in m.text.lines().enumerate() {
                            let pre = if idx == 0 { "[~] " } else { "    " };
                            Self::push_wrapped(&mut lines, 0x94A3B8, pre, raw_line, max_chars);
                        }
                    } else {
                        let first = m.text.lines().next().unwrap_or("Thought");
                        Self::push_wrapped(&mut lines, 0x94A3B8, "[~] ", first, max_chars);
                    }
                }
                AiMsgKind::User => Self::push_wrapped(&mut lines, 0x38BDF8, "> ", &m.text, max_chars),
                AiMsgKind::CuStep => Self::push_wrapped(&mut lines, 0x34D399, "* ", &m.text, max_chars),
                AiMsgKind::Assistant => Self::push_wrapped(&mut lines, 0xE2E8F0, "", &m.text, max_chars),
            }
        }
        if self.busy {
            let status = if self.cu_active {
                format!("[{spin}] Thinking & acting (step {})...", self.cu_steps.max(1))
            } else {
                format!(
                    "[{spin}] {} ({})...",
                    tr(lang, "Размышляю", "Thinking"),
                    self.thinking.label()
                )
            };
            lines.push((0x94A3B8, status));
        }

        let max_vis = ((ch - 12) / 16).max(1) as usize;
        let start = lines.len().saturating_sub(max_vis);
        for (i, (col, line)) in lines[start..].iter().enumerate() {
            let ly = cy + 8 + (i as i32) * 16;
            canvas.draw_text_clipped(cx + 8, ly, cw - 16, line, *col, 1);
        }
    }

    fn push_wrapped(out: &mut Vec<(u32, String)>, col: u32, prefix: &str, text: &str, max_chars: usize) {
        for (li, raw_line) in text.lines().enumerate() {
            let mut cur = if li == 0 {
                String::from(prefix)
            } else {
                String::new()
            };
            for word in raw_line.split_whitespace() {
                if cur.len() + word.len() + 1 > max_chars && !cur.is_empty() {
                    out.push((col, cur));
                    cur = String::new();
                }
                if !cur.is_empty() && !cur.ends_with(' ') {
                    cur.push(' ');
                }
                cur.push_str(word);
            }
            out.push((col, cur));
        }
    }

    fn draw_settings(&self, canvas: &mut Canvas, cx: i32, wy: i32, cw: i32, accent: u32, lang: Lang) {
        let bw = (cw - 8) / 3;
        for (i, prov) in [AiProvider::Gemini, AiProvider::OpenAi, AiProvider::Anthropic]
            .iter()
            .enumerate()
        {
            let act = *prov == self.provider;
            canvas.draw_button(
                cx + (i as i32) * (bw + 4),
                wy + 68,
                bw,
                22,
                prov.label(),
                if act { accent } else { 0x1E293B },
                0xFFFFFF,
                accent,
            );
        }
        let mut draw_field = |ly: i32, lbl: &str, val: &str, ph: &str, active: bool, btn: Option<&str>| {
            let fw = if btn.is_some() { cw - 48 } else { cw - 4 };
            canvas.fill_rounded_rect(cx + 2, ly, fw, 22, 5, 0x0F172A);
            canvas.draw_rounded_rect_alpha(
                cx + 2,
                ly,
                fw,
                22,
                5,
                if active { 0xF59E0B } else { 0xFFFFFF },
                if active { 140 } else { 25 },
            );
            let txt = if val.is_empty() && !active {
                format!("{lbl}: {ph}")
            } else if active {
                format!("{lbl}: {val}_")
            } else {
                format!("{lbl}: {val}")
            };
            canvas.draw_text_clipped(
                cx + 8,
                ly + 4,
                fw - 12,
                &txt,
                if val.is_empty() && !active { 0x64748B } else { 0xE2E8F0 },
                1,
            );
            if let Some(bl) = btn {
                canvas.draw_button(cx + cw - 42, ly, 40, 22, bl, 0x1E293B, 0x94A3B8, accent);
            }
        };

        draw_field(wy + 96, "URL", &self.base_url, self.provider.default_base_url(), self.edit_field == AiEditField::BaseUrl, Some("Def"));
        let key_disp = if self.key_visible || self.edit_field == AiEditField::ApiKey {
            self.api_key.clone()
        } else if self.api_key.len() > 8 {
            format!("{}...{}", &self.api_key[..4], &self.api_key[self.api_key.len() - 4..])
        } else {
            self.api_key.clone()
        };
        draw_field(wy + 124, "Key", &key_disp, "(click + Ctrl+V)", self.edit_field == AiEditField::ApiKey, Some("Eye"));
        draw_field(wy + 152, "Model", &self.custom_model, "(preset or custom ID)", self.edit_field == AiEditField::CustomModel, Some("Clr"));
        draw_field(wy + 180, "Sys", &self.system_prompt, "(optional system prompt)", self.edit_field == AiEditField::SystemPrompt, None);

        let half = (cw - 6) / 2;
        let temp_lbl = format!("Temp: {:.1}", AI_TEMPS[self.temp_idx.min(2) as usize]);
        canvas.draw_button(cx + 2, wy + 210, half, 22, &temp_lbl, 0x1E293B, 0xE2E8F0, accent);

        let (ft_lbl, ft_col) = if self.provider == AiProvider::Anthropic {
            ("Think: Brief (Anth.)", 0x111827)
        } else if self.show_full_thinking {
            ("Full Think: ON", accent)
        } else {
            ("Full Think: Brief", 0x1E293B)
        };
        canvas.draw_button(cx + half + 6, wy + 210, cw - half - 8, 22, ft_lbl, ft_col, 0xFFFFFF, accent);

        let chip_w = (cw / 6).max(36);
        for (i, tl) in ALL_THINKING_LEVELS.iter().enumerate() {
            let on = (self.custom_thinking_mask & tl.bit()) != 0;
            canvas.draw_button(
                cx + (i as i32) * chip_w,
                wy + 238,
                chip_w - 4,
                22,
                tl.label(),
                if on { 0x1D4ED8 } else { 0x1E293B },
                0xFFFFFF,
                accent,
            );
        }
        let hint = tr(
            lang,
            "Чипы: доступные уровни раздумия для Custom/OpenAI/Anthropic.",
            "Chips: allowed thinking levels for Custom/OpenAI/Anthropic.",
        );
        canvas.draw_text_clipped(cx + 4, wy + 264, cw - 8, hint, 0x64748B, 1);
    }

    fn draw_popover(&self, canvas: &mut Canvas, wx: i32, wy: i32, ww: i32, wh: i32, accent: u32) {
        let (rx, ry, pw, ph) = self.popover_rect(ww, wh);
        let px = wx + rx;
        let py = wy + ry;
        canvas.fill_rounded_rect(px - 2, py - 2, pw + 4, ph + 4, 14, 0x020617);
        canvas.fill_rounded_rect(px, py, pw, ph, 12, 0x0F172A);
        canvas.draw_rounded_rect_alpha(px, py, pw, ph, 12, 0xFFFFFF, 45);

        match self.popup {
            AiPopup::Effort => {
                let lvl_txt = self.thinking.label();
                let lw = canvas.measure_text(lvl_txt, 2);
                canvas.draw_text_clipped(px + (pw - lw) / 2, py + 8, pw - 16, lvl_txt, 0xA855F7, 2);

                let mod_txt = format!("{} >", self.active_model_label());
                let mw = canvas.measure_text(&mod_txt, 1);
                canvas.draw_text_clipped(px + (pw - mw) / 2, py + 36, pw - 20, &mod_txt, 0xCBD5E1, 1);

                canvas.draw_text_clipped(px + 14, py + 54, 60, "Faster", 0x64748B, 1);
                canvas.draw_text_clipped(px + pw - 64, py + 54, 54, "Smarter", 0x64748B, 1);

                let tx = px + 14;
                let ty = py + 74;
                let tw = (pw - 28).max(40);
                let th = 18;
                let allowed = self.supported_thinking();
                let pos_idx = allowed.iter().position(|t| *t == self.thinking).unwrap_or(0);
                let frac = if allowed.len() <= 1 {
                    1.0
                } else {
                    pos_idx as f32 / (allowed.len() - 1) as f32
                };
                let fill_w = ((tw as f32) * frac).round().max(10.0) as i32;

                canvas.fill_rounded_rect(tx, ty, tw, th, 9, 0x1E293B);
                for col_x in 0..fill_w {
                    let c = lerp_rgb(0x2563EB, 0xA855F7, col_x as u32, tw.max(1) as u32);
                    canvas.fill_rect(tx + col_x, ty + 1, 1, th - 2, c);
                }
                for s in 0..8i32 {
                    let sx = tx + ((s * 31 + 11) % fill_w.max(12));
                    let sy = ty + 3 + ((s * 17) % 12);
                    canvas.fill_rect(sx, sy, 2, 2, 0xE9D5FF);
                }
                for i in 0..allowed.len() {
                    let df = if allowed.len() <= 1 { 0.5 } else { i as f32 / (allowed.len() - 1) as f32 };
                    let dx = tx + (tw as f32 * df).round() as i32;
                    canvas.fill_aa_circle(dx as f32, (ty + th / 2) as f32, 2.0, 0xCBD5E1, 190);
                }
                let knob_x = tx + ((tw - 14) as f32 * frac).round() as i32;
                canvas.fill_aa_circle((knob_x + 7) as f32, (ty + th / 2) as f32, 10.0, 0xFFFFFF, 255);
                canvas.draw_text_clipped(px + 14, py + 102, pw - 28, "Click track to set reasoning depth", 0x64748B, 1);
            }
            AiPopup::Models => {
                canvas.draw_button(px + 8, py + 6, 28, 22, "<-", 0x1E293B, 0xE2E8F0, accent);
                let title = "Select model";
                let tw = canvas.measure_text(title, 1);
                canvas.draw_text_clipped(px + (pw - tw) / 2, py + 10, pw - 76, title, 0x94A3B8, 1);
                canvas.fill_rect(px + 8, py + 31, pw - 16, 1, 0x1E293B);

                let visible = 5usize;
                for (row, idx) in (self.model_scroll..AI_MODELS.len()).take(visible).enumerate() {
                    let ry_row = py + 36 + (row as i32) * 28;
                    let active = self.custom_model.trim().is_empty() && self.model_idx == idx;
                    if active {
                        canvas.fill_rounded_rect(px + 8, ry_row, pw - 36, 24, 6, 0x1E293B);
                    }
                    canvas.draw_text_clipped(
                        px + 14,
                        ry_row + 4,
                        pw - 56,
                        AI_MODELS[idx].label,
                        if active { 0xFFFFFF } else { 0xCBD5E1 },
                        1,
                    );
                    if active {
                        canvas.draw_text_clipped(px + pw - 44, ry_row + 4, 16, "v", accent, 1);
                    }
                }
                canvas.draw_text_clipped(px + pw - 20, py + 40, 14, "^", 0x64748B, 1);
                canvas.draw_text_clipped(px + pw - 20, py + ph - 22, 14, "v", 0x64748B, 1);
            }
            AiPopup::None => {}
        }
    }
}

pub fn build_ai_request_json(
    provider: AiProvider,
    model_id: &str,
    thinking: ThinkingLevel,
    prompt: &str,
    png_b64: Option<&str>,
    temp: f32,
) -> String {
    let ep = json_escape(prompt);
    match provider {
        AiProvider::Gemini => {
            let parts = if let Some(b64) = png_b64 {
                format!(
                    "[{{\"text\":\"{ep}\"}},{{\"inlineData\":{{\"mimeType\":\"image/png\",\"data\":\"{b64}\"}}}}]"
                )
            } else {
                format!("[{{\"text\":\"{ep}\"}}]")
            };
            let mime = if png_b64.is_some() {
                ",\"responseMimeType\":\"application/json\""
            } else {
                ""
            };
            let tcfg = if model_id.contains("2.5") {
                let budget = match thinking {
                    ThinkingLevel::Off => 0,
                    ThinkingLevel::Low => 1024,
                    ThinkingLevel::Medium => 8192,
                    ThinkingLevel::High => 16384,
                    ThinkingLevel::XHigh => 24576,
                    ThinkingLevel::Max => 32768,
                };
                let inc = if thinking == ThinkingLevel::Off { "" } else { ",\"includeThoughts\":true" };
                format!(
                    ",\"generationConfig\":{{\"temperature\":{temp:.1},\"thinkingConfig\":{{\"thinkingBudget\":{budget}{inc}}}{mime}}}"
                )
            } else if thinking == ThinkingLevel::Off {
                format!(
                    ",\"generationConfig\":{{\"temperature\":{temp:.1},\"thinkingConfig\":{{\"thinkingBudget\":0}}{mime}}}"
                )
            } else {
                let lvl = match thinking {
                    ThinkingLevel::Low => "LOW",
                    ThinkingLevel::Medium => "MEDIUM",
                    ThinkingLevel::High => "HIGH",
                    ThinkingLevel::XHigh => "XHIGH",
                    ThinkingLevel::Max => "MAX",
                    ThinkingLevel::Off => "LOW",
                };
                format!(
                    ",\"generationConfig\":{{\"temperature\":{temp:.1},\"thinkingConfig\":{{\"thinkingLevel\":\"{lvl}\",\"includeThoughts\":true}}{mime}}}"
                )
            };
            format!("{{\"contents\":[{{\"parts\":{parts}}}]{tcfg}}}")
        }
        AiProvider::OpenAi => {
            let content = if let Some(b64) = png_b64 {
                format!(
                    "[{{\"type\":\"text\",\"text\":\"{ep}\"}},{{\"type\":\"image_url\",\"image_url\":{{\"url\":\"data:image/png;base64,{b64}\"}}}}]"
                )
            } else {
                format!("\"{ep}\"")
            };
            let eff = match thinking {
                ThinkingLevel::Off => String::new(),
                ThinkingLevel::Low => String::from(",\"reasoning_effort\":\"low\""),
                ThinkingLevel::Medium => String::from(",\"reasoning_effort\":\"medium\""),
                ThinkingLevel::High => String::from(",\"reasoning_effort\":\"high\""),
                ThinkingLevel::XHigh => String::from(",\"reasoning_effort\":\"xhigh\""),
                ThinkingLevel::Max => String::from(",\"reasoning_effort\":\"max\""),
            };
            format!(
                "{{\"model\":\"{}\",\"temperature\":{temp:.1},\"messages\":[{{\"role\":\"user\",\"content\":{content}}}]{eff}}}",
                json_escape(model_id)
            )
        }
        AiProvider::Anthropic => {
            let content = if let Some(b64) = png_b64 {
                format!(
                    "[{{\"type\":\"image\",\"source\":{{\"type\":\"base64\",\"media_type\":\"image/png\",\"data\":\"{b64}\"}}}},{{\"type\":\"text\",\"text\":\"{ep}\"}}]"
                )
            } else {
                format!("\"{ep}\"")
            };
            let tcfg = match thinking {
                ThinkingLevel::Off => format!(",\"temperature\":{temp:.1}"),
                ThinkingLevel::Low => String::from(",\"thinking\":{\"type\":\"enabled\",\"budget_tokens\":1024}"),
                ThinkingLevel::Medium => String::from(",\"thinking\":{\"type\":\"enabled\",\"budget_tokens\":4096}"),
                ThinkingLevel::High => String::from(",\"thinking\":{\"type\":\"enabled\",\"budget_tokens\":10000}"),
                ThinkingLevel::XHigh => String::from(",\"thinking\":{\"type\":\"enabled\",\"budget_tokens\":16384}"),
                ThinkingLevel::Max => String::from(",\"thinking\":{\"type\":\"enabled\",\"budget_tokens\":24576}"),
            };
            format!(
                "{{\"model\":\"{}\",\"max_tokens\":4096,\"messages\":[{{\"role\":\"user\",\"content\":{content}}}]{tcfg}}}",
                json_escape(model_id)
            )
        }
    }
}



// ---------------------------------------------------------------------------
// 11. PizSurf Browser
// ---------------------------------------------------------------------------
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WebBlock {
    Heading(u8, String),
    Paragraph(String),
    Link(String, String),
    Code(String),
}

#[derive(Debug, Clone)]
pub struct WebPage {
    pub title: String,
    pub url: String,
    pub blocks: Vec<WebBlock>,
}

pub fn decode_html_entities(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(amp) = rest.find('&') {
        out.push_str(&rest[..amp]);
        let tail = &rest[amp + 1..];
        if let Some(semi) = tail.find(';').filter(|&p| p <= 10) {
            let ent = &tail[..semi];
            let decoded = match ent {
                "amp" => Some('&'),
                "lt" => Some('<'),
                "gt" => Some('>'),
                "quot" => Some('"'),
                "apos" | "#39" => Some('\''),
                "nbsp" => Some(' '),
                "ndash" => Some('-'),
                "mdash" => Some('-'),
                "hellip" => Some('.'),
                "bull" => Some('*'),
                _ if ent.starts_with("#x") || ent.starts_with("#X") => {
                    u32::from_str_radix(&ent[2..], 16).ok().and_then(char::from_u32)
                }
                _ if ent.starts_with('#') => ent[1..].parse::<u32>().ok().and_then(char::from_u32),
                _ => None,
            };
            if let Some(ch) = decoded {
                out.push(ch);
                rest = &tail[semi + 1..];
                continue;
            }
        }
        out.push('&');
        rest = tail;
    }
    out.push_str(rest);
    out
}

fn extract_attr(attrs: &str, name: &str) -> Option<String> {
    let lower = attrs.to_ascii_lowercase();
    let needle = format!("{name}=");
    let pos = lower.find(&needle)?;
    let val_start = attrs[pos + needle.len()..].trim_start();
    if let Some(first) = val_start.bytes().next() {
        if first == b'"' || first == b'\'' {
            let inner = &val_start[1..];
            let end = inner.find(first as char)?;
            return Some(decode_html_entities(&inner[..end]));
        }
        let end = val_start.find(char::is_whitespace).unwrap_or(val_start.len());
        return Some(decode_html_entities(&val_start[..end]));
    }
    None
}

pub fn normalize_address(raw: &str) -> String {
    let s = raw.trim();
    if s.is_empty() || s.eq_ignore_ascii_case("about:home") || s.eq_ignore_ascii_case("about:pizdos") {
        return String::from("about:pizdos");
    }
    if s.starts_with("https://") || s.starts_with("http://") || s.starts_with("about:") {
        return String::from(s);
    }
    if !s.contains(' ') && (s.contains('.') || s.starts_with("localhost")) {
        return format!("https://{s}");
    }
    let mut enc = String::with_capacity(s.len() * 2);
    for b in s.as_bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => enc.push(*b as char),
            b' ' => enc.push('+'),
            other => enc.push_str(&format!("%{other:02X}")),
        }
    }
    format!("https://lite.duckduckgo.com/lite/?q={enc}")
}

pub fn percent_decode(raw: &str) -> String {
    let bytes = raw.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let (Some(h), Some(l)) = (
                (bytes[i + 1] as char).to_digit(16),
                (bytes[i + 2] as char).to_digit(16),
            )
        {
            out.push(((h << 4) | l) as u8);
            i += 3;
            continue;
        }
        out.push(if bytes[i] == b'+' { b' ' } else { bytes[i] });
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

pub fn resolve_url(base: &str, href: &str) -> String {
    let h = href.trim();
    if h.is_empty() || h.starts_with('#') || h.starts_with("javascript:") {
        return String::from(base);
    }
    if let Some(pos) = h.find("uddg=") {
        let val = &h[pos + 5..];
        let end = val.find('&').unwrap_or(val.len());
        let decoded = percent_decode(&val[..end]);
        if decoded.starts_with("http://") || decoded.starts_with("https://") {
            return decoded;
        }
    }
    if h.starts_with("https://") || h.starts_with("http://") || h.starts_with("about:") {
        return String::from(h);
    }
    let scheme_end = base.find("://").unwrap_or(0);
    let after_scheme = if scheme_end > 0 { scheme_end + 3 } else { 0 };
    if h.starts_with("//") {
        let scheme = if scheme_end > 0 { &base[..scheme_end] } else { "https" };
        return format!("{scheme}:{h}");
    }
    let host_end = base[after_scheme..]
        .find('/')
        .map(|i| after_scheme + i)
        .unwrap_or(base.len());
    let origin = &base[..host_end];
    if h.starts_with('/') {
        return format!("{origin}{h}");
    }
    let dir_end = base[host_end..]
        .rfind('/')
        .map(|i| host_end + i + 1)
        .unwrap_or(host_end);
    if dir_end <= host_end {
        format!("{origin}/{h}")
    } else {
        format!("{}{h}", &base[..dir_end])
    }
}

fn push_clean_block(blocks: &mut Vec<WebBlock>, text: &mut String, heading: Option<u8>, is_pre: bool) {
    let trimmed = if is_pre {
        text.trim_matches('\n').to_string()
    } else {
        text.split_whitespace().collect::<Vec<_>>().join(" ")
    };
    text.clear();
    if trimmed.is_empty() {
        return;
    }
    if let Some(lvl) = heading {
        blocks.push(WebBlock::Heading(lvl, trimmed));
    } else if is_pre {
        blocks.push(WebBlock::Code(trimmed));
    } else {
        blocks.push(WebBlock::Paragraph(trimmed));
    }
}

pub fn parse_html_page(url: &str, html: &str) -> WebPage {
    let bytes = html.as_bytes();
    let mut blocks = Vec::new();
    let mut title = String::new();
    let mut text_buf = String::new();
    let mut cur_href: Option<String> = None;
    let mut link_buf = String::new();
    let mut heading: Option<u8> = None;
    let mut in_title = false;
    let mut in_pre = false;
    let mut skip_tag: Option<&'static str> = None;
    let mut i = 0usize;

    while i < bytes.len() {
        if bytes[i] == b'<' {
            let text_chunk = &html[i..];
            if text_chunk.starts_with("<!--") {
                if let Some(cend) = text_chunk.find("-->") {
                    i += cend + 3;
                    continue;
                }
                break;
            }
            if let Some(rel_end) = text_chunk.find('>') {
                let raw_tag = &text_chunk[1..rel_end];
                i += rel_end + 1;
                let trimmed = raw_tag.trim();
                let is_close = trimmed.starts_with('/');
                let name_raw = if is_close { trimmed[1..].trim() } else { trimmed };
                let name_end = name_raw
                    .find(|c: char| c.is_whitespace() || c == '/')
                    .unwrap_or(name_raw.len());
                let tag = name_raw[..name_end].to_ascii_lowercase();
                let attrs = &name_raw[name_end..];

                if !is_close && matches!(tag.as_str(), "script" | "style" | "noscript" | "svg") {
                    let close_pat = format!("</{tag}");
                    let lower_tail = html[i..].to_ascii_lowercase();
                    if let Some(cp) = lower_tail.find(&close_pat)
                        && let Some(gt) = html[i + cp..].find('>')
                    {
                        i += cp + gt + 1;
                        continue;
                    }
                    skip_tag = Some(match tag.as_str() {
                        "script" => "script",
                        "style" => "style",
                        "noscript" => "noscript",
                        _ => "svg",
                    });
                    continue;
                }
                if let Some(sk) = skip_tag {
                    if is_close && tag == sk {
                        skip_tag = None;
                    }
                    continue;
                }
                match tag.as_str() {
                    "title" => {
                        in_title = !is_close;
                    }
                    "h1" | "h2" | "h3" | "h4" => {
                        let lvl = tag.as_bytes()[1].saturating_sub(b'0').clamp(1, 3);
                        if !is_close {
                            push_clean_block(&mut blocks, &mut text_buf, heading.take(), in_pre);
                            heading = Some(lvl);
                        } else {
                            push_clean_block(&mut blocks, &mut text_buf, heading.take(), in_pre);
                        }
                    }
                    "pre" => {
                        if !is_close {
                            push_clean_block(&mut blocks, &mut text_buf, heading.take(), in_pre);
                            in_pre = true;
                        } else {
                            push_clean_block(&mut blocks, &mut text_buf, None, true);
                            in_pre = false;
                        }
                    }
                    "a" => {
                        if !is_close {
                            if let Some(href) = extract_attr(attrs, "href") {
                                push_clean_block(&mut blocks, &mut text_buf, heading, in_pre);
                                cur_href = Some(href);
                                link_buf.clear();
                            }
                        } else if let Some(href) = cur_href.take() {
                            let label = link_buf.split_whitespace().collect::<Vec<_>>().join(" ");
                            link_buf.clear();
                            if !label.is_empty() {
                                blocks.push(WebBlock::Link(label, resolve_url(url, &href)));
                            }
                        }
                    }
                    "p" | "div" | "li" | "br" | "tr" | "td" | "th" | "section" | "article" | "header" | "footer" | "blockquote" | "dd" | "dt" => {
                        if cur_href.is_none() {
                            push_clean_block(&mut blocks, &mut text_buf, heading, in_pre);
                        }
                        if tag == "li" && !is_close {
                            text_buf.push_str("* ");
                        }
                    }
                    _ => {}
                }
                continue;
            }
        }
        let next_lt = if bytes[i] == b'<' {
            html[i + 1..].find('<').map(|p| i + 1 + p).unwrap_or(html.len())
        } else {
            html[i..].find('<').map(|p| i + p).unwrap_or(html.len())
        };
        let plain = decode_html_entities(&html[i..next_lt]);
        if in_title {
            title.push_str(&plain);
        } else if cur_href.is_some() {
            link_buf.push_str(&plain);
        } else if !in_title && skip_tag.is_none() {
            text_buf.push_str(&plain);
        }
        i = next_lt.max(i + 1);
    }
    push_clean_block(&mut blocks, &mut text_buf, heading, in_pre);

    if !html.contains('<') && blocks.is_empty() {
        for line in html.lines() {
            let l = line.trim();
            if !l.is_empty() {
                blocks.push(WebBlock::Paragraph(String::from(l)));
            }
        }
    }
    let clean_title = title.split_whitespace().collect::<Vec<_>>().join(" ");
    WebPage {
        title: if clean_title.is_empty() {
            String::from(url)
        } else {
            clean_title
        },
        url: String::from(url),
        blocks,
    }
}

pub fn built_in_home_page() -> WebPage {
    WebPage {
        title: String::from("PizSurf Home"),
        url: String::from("about:pizdos"),
        blocks: vec![
            WebBlock::Heading(1, String::from("PizSurf Browser - pizdOS 2.1(00)")),
            WebBlock::Paragraph(String::from(
                "Type a URL or search query in the address bar and press Enter.",
            )),
            WebBlock::Heading(2, String::from("Quick Bookmarks")),
            WebBlock::Link(
                String::from("[HTTP] info.cern.ch (First Website)"),
                String::from("http://info.cern.ch/"),
            ),
            WebBlock::Link(
                String::from("[Test] example.com"),
                String::from("https://example.com/"),
            ),
            WebBlock::Link(
                String::from("[Search] DuckDuckGo Lite"),
                String::from("https://lite.duckduckgo.com/lite/?q=pizdOS"),
            ),
            WebBlock::Link(
                String::from("[News] NPR Text"),
                String::from("https://text.npr.org/"),
            ),
            WebBlock::Link(
                String::from("[News] Hacker News"),
                String::from("https://news.ycombinator.com/"),
            ),
            WebBlock::Link(
                String::from("[Weather] wttr.in"),
                String::from("https://wttr.in/?format=4"),
            ),
        ],
    }
}

fn fetch_http_get(url: &str) -> Result<String, String> {
    const UA: &str = "Mozilla/5.0 (X11; Linux x86_64; rv:128.0) Gecko/20100101 Firefox/128.0";
    let try_curl = |insecure: bool| {
        let mut args = vec![
            "-sSL",
            "--max-time",
            "15",
            "-A",
            UA,
            "-H",
            "Accept: text/html,application/xhtml+xml,text/plain,*/*",
            "-H",
            "Accept-Language: en-US,en;q=0.9",
        ];
        if insecure {
            args.push("-k");
        }
        args.push(url);
        Command::new("curl").args(&args).output()
    };

    match try_curl(false) {
        Ok(o) if o.status.success() && !o.stdout.is_empty() => {
            return Ok(String::from_utf8_lossy(&o.stdout).into_owned());
        }
        Ok(o) => {
            // Retry with -k when live-ISO RTC/CA bundle rejects TLS certs
            if let Ok(o2) = try_curl(true)
                && o2.status.success()
                && !o2.stdout.is_empty()
            {
                return Ok(String::from_utf8_lossy(&o2.stdout).into_owned());
            }
            let err = String::from_utf8_lossy(&o.stderr).trim().to_string();
            if !err.is_empty() {
                return Err(err);
            }
        }
        Err(_) => {}
    }

    // Fallback to BusyBox / GNU wget if curl is absent
    match Command::new("wget")
        .args(["-qO-", "-T", "15", "-U", UA, url])
        .output()
    {
        Ok(o) if o.status.success() && !o.stdout.is_empty() => {
            Ok(String::from_utf8_lossy(&o.stdout).into_owned())
        }
        Ok(o) => {
            let err = String::from_utf8_lossy(&o.stderr).trim().to_string();
            Err(if err.is_empty() {
                format!("HTTP fetch returned empty body ({})", o.status)
            } else {
                err
            })
        }
        Err(e) => Err(format!("Network fetch needs curl or wget ({e})")),
    }
}

type LinkHitbox = (i32, i32, i32, i32, String);

pub struct BrowserState {
    pub url_input: String,
    pub url_focused: bool,
    pub current_url: String,
    pub history: Vec<String>,
    pub history_pos: usize,
    pub page: WebPage,
    pub scroll: usize,
    pub loading: bool,
    rx: Option<std::sync::mpsc::Receiver<Result<WebPage, String>>>,
    rendered_links: RefCell<Vec<LinkHitbox>>,
}

impl Default for BrowserState {
    fn default() -> Self {
        Self::new()
    }
}

impl BrowserState {
    pub fn new() -> Self {
        let home = built_in_home_page();
        Self {
            url_input: home.url.clone(),
            url_focused: false,
            current_url: home.url.clone(),
            history: vec![home.url.clone()],
            history_pos: 0,
            page: home,
            scroll: 0,
            loading: false,
            rx: None,
            rendered_links: RefCell::new(Vec::new()),
        }
    }

    pub fn navigate(&mut self, raw: &str, push_hist: bool) {
        let url = normalize_address(raw);
        self.url_input = url.clone();
        self.current_url = url.clone();
        self.url_focused = false;
        self.scroll = 0;
        if push_hist {
            if self.history_pos + 1 < self.history.len() {
                self.history.truncate(self.history_pos + 1);
            }
            if self.history.last() != Some(&url) {
                self.history.push(url.clone());
                self.history_pos = self.history.len() - 1;
            }
        }
        if url == "about:pizdos" {
            self.loading = false;
            self.rx = None;
            self.page = built_in_home_page();
            return;
        }
        self.loading = true;
        let (tx, rx) = std::sync::mpsc::channel();
        self.rx = Some(rx);
        std::thread::spawn(move || {
            let res = fetch_http_get(&url).map(|html| {
                let mut page = parse_html_page(&url, &html);
                if page.blocks.is_empty() {
                    page.blocks.push(WebBlock::Paragraph(String::from(
                        "Page loaded but contained no readable text blocks.",
                    )));
                }
                page
            });
            let _ = tx.send(res);
        });
    }

    pub fn back(&mut self) {
        if self.history_pos > 0 {
            self.history_pos -= 1;
            let url = self.history[self.history_pos].clone();
            self.navigate(&url, false);
        }
    }

    pub fn forward(&mut self) {
        if self.history_pos + 1 < self.history.len() {
            self.history_pos += 1;
            let url = self.history[self.history_pos].clone();
            self.navigate(&url, false);
        }
    }

    pub fn poll(&mut self) -> bool {
        let Some(ref rx) = self.rx else {
            return false;
        };
        match rx.try_recv() {
            Ok(res) => {
                self.loading = false;
                self.rx = None;
                match res {
                    Ok(p) => {
                        self.page = p;
                        self.scroll = 0;
                    }
                    Err(err) => {
                        self.page = WebPage {
                            title: String::from("Error"),
                            url: self.current_url.clone(),
                            blocks: vec![
                                WebBlock::Heading(1, String::from("Failed to load page")),
                                WebBlock::Paragraph(err),
                                WebBlock::Link(
                                    String::from("Back to about:pizdos"),
                                    String::from("about:pizdos"),
                                ),
                            ],
                        };
                    }
                }
                true
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => false,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                self.loading = false;
                self.rx = None;
                true
            }
        }
    }

    pub fn on_key(&mut self, key: &KeyEvent) {
        if self.url_focused {
            match key {
                KeyEvent::Char(c) if !c.is_control() => {
                    if self.url_input.len() < 256 {
                        self.url_input.push(*c);
                    }
                }
                KeyEvent::Backspace => {
                    self.url_input.pop();
                }
                KeyEvent::Enter => {
                    let target = self.url_input.clone();
                    self.navigate(&target, true);
                }
                KeyEvent::Escape => self.url_focused = false,
                KeyEvent::Paste(s) => {
                    for c in s.chars().filter(|c| !c.is_control()) {
                        self.url_input.push(c);
                    }
                }
                _ => {}
            }
            return;
        }
        match key {
            KeyEvent::Down => self.scroll = self.scroll.saturating_add(3),
            KeyEvent::Up => self.scroll = self.scroll.saturating_sub(3),
            KeyEvent::Char('r') | KeyEvent::Char('R') => {
                let u = self.current_url.clone();
                self.navigate(&u, false);
            }
            KeyEvent::Char('l') => self.url_focused = true,
            _ => {}
        }
    }

    pub fn on_click(&mut self, lx: i32, ly: i32, ww: i32, wh: i32) {
        // Toolbar row (32..56)
        if (32..=56).contains(&ly) {
            if (8..=34).contains(&lx) {
                self.back();
                return;
            }
            if (38..=64).contains(&lx) {
                self.forward();
                return;
            }
            if (68..=94).contains(&lx) {
                let u = self.current_url.clone();
                self.navigate(&u, false);
                return;
            }
            if lx >= ww - 44 && lx <= ww - 8 {
                let target = self.url_input.clone();
                self.navigate(&target, true);
                return;
            }
            if (98..=ww - 50).contains(&lx) {
                self.url_focused = true;
                return;
            }
        }
        self.url_focused = false;
        // Scroll buttons at right edge of viewport
        if lx >= ww - 28 && ly >= 62 && ly <= wh - 10 {
            if ly < (62 + wh - 10) / 2 {
                self.scroll = self.scroll.saturating_sub(4);
            } else {
                self.scroll = self.scroll.saturating_add(4);
            }
            return;
        }
        let clicked = {
            let links = self.rendered_links.borrow();
            links
                .iter()
                .find(|(rx, ry, rw, rh, _)| lx >= *rx && lx <= rx + rw && ly >= *ry && ly <= ry + rh)
                .map(|(_, _, _, _, href)| href.clone())
        };
        if let Some(href) = clicked {
            self.navigate(&href, true);
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn draw(
        &self,
        canvas: &mut Canvas,
        wx: i32,
        wy: i32,
        ww: i32,
        wh: i32,
        accent: u32,
        lang: Lang,
    ) {
        // Toolbar
        canvas.draw_button(
            wx + 8,
            wy + 32,
            26,
            24,
            "<",
            0x1E293B,
            if self.history_pos > 0 { 0xE2E8F0 } else { 0x475569 },
            accent,
        );
        canvas.draw_button(
            wx + 38,
            wy + 32,
            26,
            24,
            ">",
            0x1E293B,
            if self.history_pos + 1 < self.history.len() { 0xE2E8F0 } else { 0x475569 },
            accent,
        );
        canvas.draw_button(wx + 68, wy + 32, 26, 24, "R", 0x1E293B, 0xE2E8F0, accent);

        let bar_w = (ww - 152).max(60);
        canvas.fill_rounded_rect(wx + 98, wy + 32, bar_w, 24, 6, 0x0B101B);
        canvas.draw_rounded_rect_alpha(
            wx + 98,
            wy + 32,
            bar_w,
            24,
            6,
            if self.url_focused { accent } else { 0xFFFFFF },
            if self.url_focused { 160 } else { 30 },
        );
        let url_txt = if self.url_focused {
            format!("{}_", self.url_input)
        } else {
            self.url_input.clone()
        };
        canvas.draw_text_clipped(wx + 104, wy + 37, bar_w - 12, &url_txt, 0xE2E8F0, 1);
        canvas.draw_button(wx + ww - 44, wy + 32, 36, 24, "Go", accent, 0xFFFFFF, accent);

        // Viewport
        let vx = wx + 8;
        let vy = wy + 62;
        let vw = (ww - 16).max(80);
        let vh = (wh - 70).max(60);
        canvas.fill_rounded_rect(vx, vy, vw, vh, 8, 0x090D16);
        canvas.draw_rounded_rect_alpha(vx, vy, vw, vh, 8, 0xFFFFFF, 20);

        if self.loading {
            canvas.draw_text_clipped(
                vx + 14,
                vy + 14,
                vw - 28,
                tr(lang, "Загрузка страницы...", "Loading page..."),
                accent,
                1,
            );
            return;
        }

        // Flatten blocks into visual rows
        enum VisRow {
            Text { text: String, col: u32, scale: i32, bg: Option<u32> },
            Link { text: String, href: String },
        }
        let max_c1 = ((vw - 36) / 7).max(16) as usize;
        let mut rows: Vec<VisRow> = Vec::new();
        for blk in &self.page.blocks {
            match blk {
                WebBlock::Heading(lvl, s) => {
                    let scale = if *lvl == 1 { 2 } else { 1 };
                    let cap = if scale == 2 { (max_c1 / 2).max(10) } else { max_c1 };
                    let mut cur = String::new();
                    for w in s.split_whitespace() {
                        if cur.len() + w.len() + 1 > cap && !cur.is_empty() {
                            rows.push(VisRow::Text { text: cur, col: 0xF8FAFC, scale, bg: None });
                            cur = String::new();
                        }
                        if !cur.is_empty() {
                            cur.push(' ');
                        }
                        cur.push_str(w);
                    }
                    if !cur.is_empty() {
                        rows.push(VisRow::Text { text: cur, col: 0xF8FAFC, scale, bg: None });
                    }
                }
                WebBlock::Paragraph(s) => {
                    let mut cur = String::new();
                    for w in s.split_whitespace() {
                        if cur.len() + w.len() + 1 > max_c1 && !cur.is_empty() {
                            rows.push(VisRow::Text { text: cur, col: 0xCBD5E1, scale: 1, bg: None });
                            cur = String::new();
                        }
                        if !cur.is_empty() {
                            cur.push(' ');
                        }
                        cur.push_str(w);
                    }
                    if !cur.is_empty() {
                        rows.push(VisRow::Text { text: cur, col: 0xCBD5E1, scale: 1, bg: None });
                    }
                }
                WebBlock::Code(s) => {
                    for l in s.lines().take(12) {
                        rows.push(VisRow::Text {
                            text: String::from(l),
                            col: 0x86EFAC,
                            scale: 1,
                            bg: Some(0x111827),
                        });
                    }
                }
                WebBlock::Link(lbl, href) => {
                    rows.push(VisRow::Link {
                        text: format!("-> {lbl}"),
                        href: href.clone(),
                    });
                }
            }
        }

        let start = self.scroll.min(rows.len().saturating_sub(1));
        let mut hit_links = self.rendered_links.borrow_mut();
        hit_links.clear();
        let mut cy = vy + 10;
        let max_y = vy + vh - 12;
        for row in &rows[start..] {
            let rh = match row {
                VisRow::Text { scale: 2, .. } => 26,
                _ => 18,
            };
            if cy + rh > max_y {
                break;
            }
            match row {
                VisRow::Text { text, col, scale, bg } => {
                    if let Some(bgc) = bg {
                        canvas.fill_rect(vx + 10, cy - 1, vw - 36, 16, *bgc);
                    }
                    canvas.draw_text_clipped(vx + 12, cy, vw - 38, text, *col, *scale);
                }
                VisRow::Link { text, href } => {
                    let tw = canvas.measure_text(text, 1).min(vw - 40);
                    canvas.draw_text_clipped(vx + 12, cy, vw - 38, text, 0x38BDF8, 1);
                    canvas.fill_rect(vx + 12, cy + 14, tw, 1, 0x38BDF8);
                    hit_links.push((20, cy - wy, tw + 8, 17, href.clone()));
                }
            }
            cy += rh;
        }
        // Subtle scroll up/down buttons on right edge
        if rows.len() > 1 {
            canvas.draw_text_clipped(wx + ww - 22, vy + 6, 16, "^", 0x94A3B8, 1);
            canvas.draw_text_clipped(wx + ww - 22, vy + vh - 20, 16, "v", 0x94A3B8, 1);
        }
    }
}
