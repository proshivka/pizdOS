use std::cell::RefCell;
use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

use crate::apps::{
    CalcState, KeyEvent, Lang, MatrixState, NotepadState, PaintState, SettingsAction,
    SettingsState, SnakeState, TerminalState, ThemePreset, tr,
};
use crate::gfx::Canvas;
use crate::sys::{
    CpuSample, MemInfo, PkgSummary, ProcInfo, list_processes, load_packages, read_cpu_sample,
    read_kernel_version, read_loadavg, read_local_clock_str, read_meminfo, read_uptime_str,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppId {
    System,
    PizGet,
    Files,
    Terminal,
    Paint,
    Snake,
    Notepad,
    Settings,
    Calc,
    Matrix,
}

impl AppId {
    pub fn title(self, lang: Lang) -> &'static str {
        match (self, lang) {
            (AppId::System, Lang::Ru) => "Система",
            (AppId::System, Lang::En) => "System",
            (AppId::PizGet, Lang::Ru) => "Пакеты",
            (AppId::PizGet, Lang::En) => "piz-get",
            (AppId::Files, Lang::Ru) => "Файлы",
            (AppId::Files, Lang::En) => "Files",
            (AppId::Terminal, Lang::Ru) => "Терминал",
            (AppId::Terminal, Lang::En) => "Terminal",
            (AppId::Paint, Lang::Ru) => "PizPaint",
            (AppId::Paint, Lang::En) => "PizPaint",
            (AppId::Snake, Lang::Ru) => "Змейка",
            (AppId::Snake, Lang::En) => "Snake",
            (AppId::Notepad, Lang::Ru) => "Блокнот",
            (AppId::Notepad, Lang::En) => "PizPad",
            (AppId::Settings, Lang::Ru) => "Настройки",
            (AppId::Settings, Lang::En) => "Settings",
            (AppId::Calc, Lang::Ru) => "Калькулятор",
            (AppId::Calc, Lang::En) => "Calculator",
            (AppId::Matrix, Lang::Ru) => "Матрица",
            (AppId::Matrix, Lang::En) => "Matrix",
        }
    }

    pub fn color(self) -> u32 {
        match self {
            AppId::System => 0x3FB950,
            AppId::PizGet => 0xD29922,
            AppId::Files => 0x58A6FF,
            AppId::Terminal => 0x3FB950,
            AppId::Paint => 0xBC8CFF,
            AppId::Snake => 0x2EA043,
            AppId::Notepad => 0x58A6FF,
            AppId::Settings => 0xF85149,
            AppId::Calc => 0x38BDF8,
            AppId::Matrix => 0x22C55E,
        }
    }

    pub fn min_size(self) -> (i32, i32) {
        match self {
            AppId::Settings => (440, 310),
            AppId::Paint => (368, 268),
            AppId::Snake => (316, 274),
            AppId::Calc => (220, 260),
            _ => (250, 180),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResizeEdge {
    pub left: bool,
    pub right: bool,
    pub top: bool,
    pub bottom: bool,
}

impl ResizeEdge {
    pub const fn none() -> Self {
        Self {
            left: false,
            right: false,
            top: false,
            bottom: false,
        }
    }

    pub const fn any(self) -> bool {
        self.left || self.right || self.top || self.bottom
    }

    pub fn axis(self) -> (f32, f32) {
        let x = if self.right {
            1.0
        } else if self.left {
            -1.0
        } else {
            0.0
        };
        let y = if self.bottom {
            1.0
        } else if self.top {
            -1.0
        } else {
            0.0
        };
        (x, y)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct ResizeDrag {
    pub id: AppId,
    pub edge: ResizeEdge,
    pub start_mx: i32,
    pub start_my: i32,
    pub start_rect: (i32, i32, i32, i32),
}

fn detect_resize_edge(win: &Window, x: i32, y: i32) -> ResizeEdge {
    if win.maximized {
        return ResizeEdge::none();
    }
    if x < win.x - 4 || x > win.x + win.w + 4 || y < win.y - 4 || y > win.y + win.h + 4 {
        return ResizeEdge::none();
    }
    // Generous bottom-right corner grip zone (14x14)
    if x >= win.x + win.w - 14 && y >= win.y + win.h - 14 {
        return ResizeEdge {
            left: false,
            right: true,
            top: false,
            bottom: true,
        };
    }
    let left = x <= win.x + 5;
    let right = x >= win.x + win.w - 5 && y > win.y + 24;
    let bottom = y >= win.y + win.h - 5;
    // Protect close/min/max titlebar buttons in upper-right
    let top = y <= win.y + 4 && x < win.x + win.w - 74;
    ResizeEdge {
        left,
        right,
        top,
        bottom,
    }
}

#[derive(Debug, Clone)]
pub struct Window {
    pub id: AppId,
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
    pub open: bool,
    pub minimized: bool,
    pub maximized: bool,
    pub saved_rect: (i32, i32, i32, i32),
}

impl Window {
    fn new(id: AppId, x: i32, y: i32, w: i32, h: i32, open: bool) -> Self {
        Self {
            id,
            x,
            y,
            w,
            h,
            open,
            minimized: false,
            maximized: false,
            saved_rect: (x, y, w, h),
        }
    }
}

#[derive(Debug, Clone)]
pub struct FileItem {
    pub name: String,
    pub is_dir: bool,
    pub path: PathBuf,
}

const START_ITEMS: [Option<AppId>; 11] = [
    Some(AppId::Terminal),
    Some(AppId::Calc),
    Some(AppId::Paint),
    Some(AppId::Snake),
    Some(AppId::Notepad),
    Some(AppId::Matrix),
    Some(AppId::Files),
    Some(AppId::PizGet),
    Some(AppId::System),
    Some(AppId::Settings),
    None,
];

const DESKTOP_ICONS: [(AppId, i32, i32, u32); 8] = [
    (AppId::Terminal, 18, 42, 0x3FB950),
    (AppId::PizGet, 18, 114, 0xD29922),
    (AppId::Paint, 18, 186, 0xBC8CFF),
    (AppId::Snake, 18, 258, 0x2EA043),
    (AppId::Notepad, 18, 330, 0x58A6FF),
    (AppId::Settings, 18, 402, 0xF85149),
    (AppId::Calc, 102, 42, 0x38BDF8),
    (AppId::Matrix, 102, 114, 0x22C55E),
];

struct WallpaperCache {
    key: (usize, usize, ThemePreset, bool),
    pixels: Vec<u32>,
}

pub struct Desktop {
    pub running: bool,
    pub start_menu_open: bool,
    pub ctx_menu: Option<(i32, i32)>,
    pub mouse_x: i32,
    pub mouse_y: i32,
    pub mouse_down: bool,
    pub dragging: Option<(AppId, i32, i32)>,
    pub resizing: Option<ResizeDrag>,
    pub windows: Vec<Window>,
    pub tick: u64,
    pub status: String,

    // System metrics
    pub cpu_pct: u8,
    pub cpu_history: [u8; 32],
    pub last_cpu: Option<CpuSample>,
    pub mem: MemInfo,
    pub uptime: String,
    pub clock_str: String,
    pub loadavg: String,
    pub kernel: String,
    pub procs: Vec<ProcInfo>,

    // piz-get
    pub piz_root: PathBuf,
    pub packages: Vec<PkgSummary>,
    pub pkg_sel: usize,

    // files
    pub cwd: PathBuf,
    pub files: Vec<FileItem>,
    pub file_sel: usize,

    // apps
    pub terminal: TerminalState,
    pub paint: PaintState,
    pub snake: SnakeState,
    pub notepad: NotepadState,
    pub settings: SettingsState,
    pub calc: CalcState,
    pub matrix: MatrixState,

    wp_cache: RefCell<WallpaperCache>,
}

impl Desktop {
    pub fn new(width: i32, height: i32) -> Self {
        let cwd = env::current_dir().unwrap_or_else(|_| PathBuf::from("/"));
        let piz_root = env::var("PIZ_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("/"));
        let settings = SettingsState::new();
        let lang = settings.lang;

        let mut d = Self {
            running: true,
            start_menu_open: false,
            ctx_menu: None,
            mouse_x: width / 2,
            mouse_y: height / 2,
            mouse_down: false,
            dragging: None,
            resizing: None,
            windows: vec![
                Window::new(AppId::Files, 150, 70, 400, 300, false),
                Window::new(AppId::PizGet, 190, 90, 440, 310, false),
                Window::new(AppId::Terminal, 120, 60, 450, 290, false),
                Window::new(AppId::Paint, 240, 110, 368, 268, false),
                Window::new(AppId::Snake, 280, 100, 320, 278, false),
                Window::new(AppId::Notepad, 210, 130, 430, 270, false),
                Window::new(AppId::Calc, 300, 90, 260, 310, false),
                Window::new(AppId::Matrix, 240, 80, 380, 280, false),
                Window::new(AppId::System, 190, 26, 330, 268, true),
                Window::new(AppId::Settings, 370, 90, 520, 344, true),
            ],
            tick: 0,
            status: String::from("0% GNU | 0 Deps"),

            cpu_pct: 12,
            cpu_history: [8, 12, 10, 14, 18, 15, 11, 9, 13, 16, 20, 14, 10, 12, 15, 18, 14, 11, 9, 13, 15, 12, 10, 14, 17, 13, 11, 10, 12, 15, 14, 12],
            last_cpu: read_cpu_sample(),
            mem: read_meminfo(),
            uptime: read_uptime_str(),
            clock_str: read_local_clock_str(),
            loadavg: read_loadavg(),
            kernel: read_kernel_version(),
            procs: list_processes(),

            piz_root,
            packages: Vec::new(),
            pkg_sel: 0,

            cwd,
            files: Vec::new(),
            file_sel: 0,

            terminal: TerminalState::new(lang),
            paint: PaintState::new(),
            snake: SnakeState::new(),
            notepad: NotepadState::new(lang),
            settings,
            calc: CalcState::new(),
            matrix: MatrixState::new(),

            wp_cache: RefCell::new(WallpaperCache {
                key: (0, 0, ThemePreset::AuroraPeaks, false),
                pixels: Vec::new(),
            }),
        };

        if let Some(sys) = d.windows.iter_mut().find(|w| w.id == AppId::System) {
            sys.minimized = true;
        }
        d.reload_packages();
        d.reload_files();
        d
    }

    pub fn reload_packages(&mut self) {
        self.packages = load_packages(&self.piz_root);
        if !self.packages.is_empty() && self.pkg_sel >= self.packages.len() {
            self.pkg_sel = self.packages.len() - 1;
        }
    }

    pub fn reload_files(&mut self) {
        self.files.clear();
        if let Some(parent) = self.cwd.parent() {
            self.files.push(FileItem {
                name: String::from(".."),
                is_dir: true,
                path: parent.to_path_buf(),
            });
        }
        let mut dirs = Vec::new();
        let mut reg = Vec::new();
        if let Ok(rd) = fs::read_dir(&self.cwd) {
            for item in rd.flatten() {
                let path = item.path();
                let name = item.file_name().to_string_lossy().to_string();
                let is_dir = item.metadata().is_ok_and(|m| m.is_dir());
                let entry = FileItem { name, is_dir, path };
                if is_dir {
                    dirs.push(entry);
                } else {
                    reg.push(entry);
                }
            }
        }
        dirs.sort_by(|a, b| a.name.cmp(&b.name));
        reg.sort_by(|a, b| a.name.cmp(&b.name));
        self.files.extend(dirs);
        self.files.extend(reg);
        self.file_sel = 0;
    }

    pub fn refresh_metrics(&mut self) {
        if let Some(now) = read_cpu_sample() {
            if let Some(prev) = self.last_cpu {
                self.cpu_pct = now.percent_since(prev);
            }
            self.last_cpu = Some(now);
        } else {
            self.cpu_pct = 8 + ((self.tick * 5) % 24) as u8;
        }
        self.cpu_history.rotate_left(1);
        self.cpu_history[31] = self.cpu_pct;

        self.mem = read_meminfo();
        self.uptime = read_uptime_str();
        self.clock_str = read_local_clock_str();
        self.loadavg = read_loadavg();
        self.procs = list_processes();
    }

    pub fn on_viewport_resize(&mut self, canvas_w: i32, canvas_h: i32) {
        let top_bar = if self.settings.taskbar_top { 36 } else { 0 };
        for win in &mut self.windows {
            if win.maximized {
                win.x = 0;
                win.y = top_bar;
                win.w = canvas_w.max(200);
                win.h = (canvas_h - 36).max(160);
            } else {
                win.x = win.x.clamp(0, (canvas_w - 80).max(0));
                win.y = win.y.clamp(top_bar, (canvas_h - 60).max(top_bar));
            }
        }
    }

    pub fn on_escape(&mut self) -> bool {
        if self.ctx_menu.take().is_some() {
            return true;
        }
        if self.start_menu_open {
            self.start_menu_open = false;
            return true;
        }
        if self.settings.editing_device {
            self.settings.on_key(KeyEvent::Escape);
            return true;
        }
        false
    }

    pub fn on_tick(&mut self) {
        self.tick = self.tick.wrapping_add(1);
        if self.tick.is_multiple_of(30) {
            self.refresh_metrics();
        }
        let div = self.settings.snake_tick_div.max(1);
        if self.tick.is_multiple_of(div)
            && self
                .windows
                .iter()
                .any(|w| w.id == AppId::Snake && w.open && !w.minimized)
        {
            self.snake.step();
        }
        if self.tick.is_multiple_of(2)
            && let Some(m) = self
                .windows
                .iter()
                .find(|w| w.id == AppId::Matrix && w.open && !w.minimized)
        {
            self.matrix.step(m.h - 70);
        }
    }

    pub fn focus_or_open(&mut self, id: AppId) {
        if let Some(pos) = self.windows.iter().position(|w| w.id == id) {
            let mut win = self.windows.remove(pos);
            win.open = true;
            win.minimized = false;
            self.windows.push(win);
        }
    }

    fn toggle_taskbar_window(&mut self, id: AppId) {
        let is_top_visible = self
            .windows
            .iter()
            .rev()
            .find(|w| w.open && !w.minimized)
            .is_some_and(|w| w.id == id);

        if is_top_visible {
            if let Some(win) = self.windows.iter_mut().find(|w| w.id == id) {
                win.minimized = true;
            }
        } else {
            self.focus_or_open(id);
        }
    }

    fn toggle_maximize(&mut self, idx: usize, canvas_w: i32, canvas_h: i32) {
        let win = &mut self.windows[idx];
        if win.maximized {
            let (sx, sy, sw, sh) = win.saved_rect;
            win.x = sx;
            win.y = sy;
            win.w = sw;
            win.h = sh;
            win.maximized = false;
        } else {
            win.saved_rect = (win.x, win.y, win.w, win.h);
            win.x = 0;
            win.y = if self.settings.taskbar_top { 36 } else { 0 };
            win.w = canvas_w;
            win.h = (canvas_h - 36).max(160);
            win.maximized = true;
        }
    }

    fn run_piz_get(&mut self, args: &[&str]) {
        let res = Command::new("piz-get")
            .args(args)
            .env("PIZ_ROOT", &self.piz_root)
            .output();
        self.status = match res {
            Ok(o) if o.status.success() => format!("piz-get {}: OK", args.join(" ")),
            Ok(_) => format!("piz-get {}: err", args.join(" ")),
            Err(e) => format!("piz-get: {e}"),
        };
        self.reload_packages();
    }

    pub fn on_key(&mut self, key: KeyEvent) {
        if key == KeyEvent::Escape && self.on_escape() {
            return;
        }
        let Some(top) = self.windows.iter().rev().find(|w| w.open && !w.minimized) else {
            return;
        };
        match top.id {
            AppId::Terminal => self.terminal.on_key(key, &self.kernel, &self.mem),
            AppId::Snake => self.snake.on_key(key),
            AppId::Notepad => self.notepad.on_key(key),
            AppId::Calc => self.calc.on_key(key),
            AppId::Settings => self.settings.on_key(key),
            _ => {}
        }
    }

    pub fn on_mouse_move(&mut self, x: i32, y: i32, canvas_w: i32, canvas_h: i32) {
        self.mouse_x = x.clamp(0, canvas_w.saturating_sub(1));
        self.mouse_y = y.clamp(0, canvas_h.saturating_sub(1));

        if let Some(rz) = self.resizing
            && let Some(win) = self.windows.iter_mut().find(|w| w.id == rz.id)
        {
            if !win.maximized {
                let (min_w, min_h) = win.id.min_size();
                let (sx, sy, sw, sh) = rz.start_rect;
                let dx = self.mouse_x - rz.start_mx;
                let dy = self.mouse_y - rz.start_my;
                let top_min = if self.settings.taskbar_top { 36 } else { 0 };
                let bot_max = if self.settings.taskbar_top {
                    canvas_h
                } else {
                    (canvas_h - 36).max(top_min + min_h)
                };

                if rz.edge.right {
                    win.w = (sw + dx).clamp(min_w, (canvas_w - sx).max(min_w));
                }
                if rz.edge.bottom {
                    win.h = (sh + dy).clamp(min_h, (bot_max - sy).max(min_h));
                }
                if rz.edge.left {
                    let nx = (sx + dx).clamp(0, (sx + sw - min_w).max(0));
                    win.w = (sx + sw - nx).max(min_w);
                    win.x = nx;
                }
                if rz.edge.top {
                    let ny = (sy + dy).clamp(top_min, (sy + sh - min_h).max(top_min));
                    win.h = (sy + sh - ny).max(min_h);
                    win.y = ny;
                }
            }
            return;
        }

        if let Some((id, off_x, off_y)) = self.dragging
            && let Some(win) = self.windows.iter_mut().find(|w| w.id == id)
        {
            if !win.maximized {
                win.x = (self.mouse_x - off_x).clamp(0, (canvas_w - 60).max(0));
                win.y = (self.mouse_y - off_y).clamp(0, (canvas_h - 60).max(0));
            }
            return;
        }

        if self.mouse_down
            && let Some(top) = self.windows.iter().rev().find(|w| w.open && !w.minimized)
            && top.id == AppId::Paint
        {
            self.paint
                .paint_at(self.mouse_x - top.x, self.mouse_y - top.y);
        }
    }

    pub fn on_right_click(&mut self, x: i32, y: i32, canvas_w: i32, canvas_h: i32) {
        self.mouse_x = x.clamp(0, canvas_w.saturating_sub(1));
        self.mouse_y = y.clamp(0, canvas_h.saturating_sub(1));
        self.start_menu_open = false;
        let mx = x.min((canvas_w - 190).max(0));
        let my = y.min((canvas_h - 170).max(0));
        self.ctx_menu = Some((mx, my));
    }

    fn taskbar_pill_width(&self, canvas_w: i32, open_count: usize) -> i32 {
        if open_count == 0 {
            return 100;
        }
        let avail = (canvas_w - 280).max(120);
        ((avail / open_count as i32) - 4).clamp(64, 130)
    }

    pub fn on_mouse_down(&mut self, x: i32, y: i32, canvas_w: i32, canvas_h: i32) {
        self.mouse_x = x.clamp(0, canvas_w.saturating_sub(1));
        self.mouse_y = y.clamp(0, canvas_h.saturating_sub(1));
        self.mouse_down = true;

        // 0. Right-click context menu hit test
        if let Some((cx, cy)) = self.ctx_menu {
            self.ctx_menu = None;
            if x >= cx && x <= cx + 184 && y >= cy && y <= cy + 134 {
                let row = (y - cy - 4) / 25;
                match row {
                    0 => self.focus_or_open(AppId::Terminal),
                    1 => {
                        self.settings.theme = match self.settings.theme {
                            ThemePreset::AuroraPeaks => ThemePreset::CrimsonSunset,
                            ThemePreset::CrimsonSunset => ThemePreset::MatrixGlow,
                            ThemePreset::MatrixGlow => ThemePreset::OceanGlass,
                            ThemePreset::OceanGlass => ThemePreset::AuroraPeaks,
                        };
                    }
                    2 => self.focus_or_open(AppId::Settings),
                    3 => self.refresh_metrics(),
                    4 => self.running = false,
                    _ => {}
                }
                return;
            }
        }

        let task_y = if self.settings.taskbar_top { 0 } else { canvas_h - 36 };

        // 1. Start Menu popup hit test
        if self.start_menu_open {
            let menu_h = (START_ITEMS.len() as i32) * 24 + 42;
            let menu_y = if self.settings.taskbar_top {
                38
            } else {
                task_y - menu_h - 4
            };
            if (8..=236).contains(&x) && y >= menu_y && y < menu_y + menu_h {
                let rel_y = y - (menu_y + 34);
                if rel_y >= 0 {
                    let idx = (rel_y / 24) as usize;
                    if let Some(app_opt) = START_ITEMS.get(idx) {
                        self.start_menu_open = false;
                        match app_opt {
                            Some(id) => self.focus_or_open(*id),
                            None => self.running = false,
                        }
                        return;
                    }
                }
                return;
            }
            self.start_menu_open = false;
        }

        // 2. Taskbar hit test
        if y >= task_y && y < task_y + 36 {
            if (8..=94).contains(&x) {
                self.start_menu_open = !self.start_menu_open;
                return;
            }
            let lang_x = (canvas_w - 144).max(108);
            if x >= lang_x && x <= lang_x + 36 {
                self.settings.lang = match self.settings.lang {
                    Lang::Ru => Lang::En,
                    Lang::En => Lang::Ru,
                };
                return;
            }
            if x > lang_x + 38 {
                self.settings.show_clock = !self.settings.show_clock;
                return;
            }

            let open_ids: Vec<AppId> = self
                .windows
                .iter()
                .filter(|w| w.open)
                .map(|w| w.id)
                .collect();
            let pill_w = self.taskbar_pill_width(canvas_w, open_ids.len());
            let mut bx = 102;
            for id in open_ids {
                if x >= bx && x <= bx + pill_w {
                    self.toggle_taskbar_window(id);
                    return;
                }
                bx += pill_w + 4;
            }
            return;
        }

        // 3. Topmost visible window hit test (expanded 4px border zone for effortless edge-resize grabs)
        let mut hit_idx = None;
        for (idx, win) in self.windows.iter().enumerate().rev() {
            let pad = if win.maximized { 0 } else { 4 };
            if win.open
                && !win.minimized
                && x >= win.x - pad
                && x < win.x + win.w + pad
                && y >= win.y - pad
                && y < win.y + win.h + pad
            {
                hit_idx = Some(idx);
                break;
            }
        }

        if let Some(idx) = hit_idx {
            let win = self.windows.remove(idx);
            self.windows.push(win.clone());
            let top_idx = self.windows.len() - 1;

            if y >= win.y + 5 && y <= win.y + 23 {
                // Close button [×]
                if x >= win.x + win.w - 26 && x <= win.x + win.w - 6 {
                    self.windows[top_idx].open = false;
                    self.dragging = None;
                    self.resizing = None;
                    return;
                }
                // Maximize/Restore button [□]
                if x >= win.x + win.w - 49 && x <= win.x + win.w - 29 {
                    self.toggle_maximize(top_idx, canvas_w, canvas_h);
                    self.dragging = None;
                    self.resizing = None;
                    return;
                }
                // Minimize button [_]
                if x >= win.x + win.w - 72 && x <= win.x + win.w - 52 {
                    self.windows[top_idx].minimized = true;
                    self.dragging = None;
                    self.resizing = None;
                    return;
                }
            }

            let edge = detect_resize_edge(&win, x, y);
            if edge.any() {
                self.resizing = Some(ResizeDrag {
                    id: win.id,
                    edge,
                    start_mx: x,
                    start_my: y,
                    start_rect: (win.x, win.y, win.w, win.h),
                });
                return;
            }

            if y < win.y + 28 {
                self.dragging = Some((win.id, x - win.x, y - win.y));
                return;
            }

            self.handle_window_click(&win, x - win.x, y - win.y);
            return;
        }

        // 4. Desktop icons
        if self.settings.show_icons {
            for &(id, ix, iy, _) in &DESKTOP_ICONS {
                if x >= ix - 4 && x <= ix + 74 && y >= iy - 2 && y <= iy + 58 {
                    self.focus_or_open(id);
                    return;
                }
            }
        }
    }

    pub fn on_mouse_up(&mut self) {
        self.mouse_down = false;
        self.dragging = None;
        self.resizing = None;
    }

    fn handle_window_click(&mut self, win: &Window, lx: i32, ly: i32) {
        let lang = self.settings.lang;
        match win.id {
            AppId::System => {
                if ly >= win.h - 36 && ly <= win.h - 10 && (14..=124).contains(&lx) {
                    self.refresh_metrics();
                    self.status = String::from(tr(lang, "Метрики обновлены", "Metrics refreshed"));
                }
            }
            AppId::PizGet => {
                if ly >= 36 && ly < win.h - 46 {
                    let row = ((ly - 36) / 18) as usize;
                    if row < self.packages.len() {
                        self.pkg_sel = row;
                    }
                } else if ly >= win.h - 36 && ly <= win.h - 10 {
                    if (14..=104).contains(&lx) {
                        self.run_piz_get(&["obnovi"]);
                    } else if (112..=202).contains(&lx)
                        && let Some(p) = self.packages.get(self.pkg_sel).cloned()
                    {
                        self.run_piz_get(&["ebash", &p.name]);
                    } else if (210..=300).contains(&lx)
                        && let Some(p) = self.packages.get(self.pkg_sel).cloned()
                    {
                        self.run_piz_get(&["snosi", &p.name]);
                    }
                }
            }
            AppId::Files => {
                if ly >= 54 && ly < win.h - 16 {
                    let row = ((ly - 54) / 18) as usize;
                    if let Some(entry) = self.files.get(row).cloned() {
                        self.file_sel = row;
                        if entry.is_dir {
                            self.cwd = entry.path;
                            self.reload_files();
                        } else {
                            self.status = format!("Fayl: {}", entry.name);
                        }
                    }
                }
            }
            AppId::Paint => self.paint.on_click(lx, ly, win.h),
            AppId::Snake => self.snake.on_click(lx, ly, win.h),
            AppId::Notepad => self.notepad.on_click(lx, ly, win.h, lang),
            AppId::Calc => self.calc.on_click(lx, ly, win.w, win.h),
            AppId::Matrix => {
                if ly >= win.h - 32 && ly <= win.h - 8 && (12..=142).contains(&lx) {
                    self.matrix.cycle_color();
                }
            }
            AppId::Settings => match self.settings.on_click(lx, ly, win.w, win.h) {
                SettingsAction::None => {}
                SettingsAction::RefreshMetrics => self.refresh_metrics(),
                SettingsAction::OpenTerminal => self.focus_or_open(AppId::Terminal),
                SettingsAction::OpenPizGet => self.focus_or_open(AppId::PizGet),
            },
            AppId::Terminal => {}
        }
    }

    pub fn render(&self, canvas: &mut Canvas) {
        let w = canvas.width as i32;
        let h = canvas.height as i32;
        let lang = self.settings.lang;
        let accent = self.settings.theme.accent();

        // 1. Cached procedural gradient + mountain wallpaper
        {
            let mut wp = self.wp_cache.borrow_mut();
            let key = (
                canvas.width,
                canvas.height,
                self.settings.theme,
                self.settings.show_grid,
            );
            if wp.key != key || wp.pixels.len() != canvas.pixels.len() {
                wp.pixels.resize(canvas.pixels.len(), 0);
                self.settings.theme.render_wallpaper(
                    &mut wp.pixels,
                    canvas.width,
                    canvas.height,
                    self.settings.show_grid,
                );
                wp.key = key;
            }
            canvas.pixels.copy_from_slice(&wp.pixels);
        }

        // 2. Desktop Icons
        if self.settings.show_icons {
            for &(id, ix, iy, col) in &DESKTOP_ICONS {
                self.draw_desktop_icon(canvas, ix, iy, id, id.title(lang), col);
            }
        }

        // 3. Visible Windows
        let top_vis = self
            .windows
            .iter()
            .enumerate()
            .rev()
            .find(|(_, win)| win.open && !win.minimized)
            .map(|(i, _)| i);

        for (idx, win) in self.windows.iter().enumerate() {
            if !win.open || win.minimized {
                continue;
            }
            self.draw_window(canvas, win, Some(idx) == top_vis);
        }

        // 4. Minimalist Dark Frosted Taskbar (ONLY open windows)
        let task_y = if self.settings.taskbar_top { 0 } else { h - 36 };
        canvas.blend_rect(0, task_y, w, 36, 0x0B0E14, 228);
        let rim_y = if self.settings.taskbar_top { task_y + 35 } else { task_y };
        canvas.blend_rect(0, rim_y, w, 1, 0xFFFFFF, 22);

        let start_hovered =
            self.mouse_x >= 8 && self.mouse_x <= 94 && self.mouse_y >= task_y + 6 && self.mouse_y <= task_y + 30;
        if self.start_menu_open {
            canvas.blend_rounded_rect(8, task_y + 6, 86, 24, 7, accent, 55);
            canvas.draw_rounded_rect_alpha(8, task_y + 6, 86, 24, 7, accent, 115);
        } else {
            let start_alpha = if start_hovered { 28 } else { 15 };
            canvas.blend_rounded_rect(8, task_y + 6, 86, 24, 7, 0xFFFFFF, start_alpha);
            canvas.draw_rounded_rect_alpha(8, task_y + 6, 86, 24, 7, 0xFFFFFF, 28);
        }
        // Minimalist 2x2 dot-grid emblem
        canvas.fill_aa_circle(19.0, (task_y + 15) as f32, 1.7, accent, 255);
        canvas.fill_aa_circle(25.0, (task_y + 15) as f32, 1.7, 0xE2E8F0, 220);
        canvas.fill_aa_circle(19.0, (task_y + 21) as f32, 1.7, 0xE2E8F0, 220);
        canvas.fill_aa_circle(25.0, (task_y + 21) as f32, 1.7, 0x94A3B8, 220);
        canvas.draw_text_clipped(34, task_y + 11, 56, tr(lang, "Пуск", "Start"), 0xF1F5F9, 1);

        let open_wins: Vec<(usize, &Window)> = self
            .windows
            .iter()
            .enumerate()
            .filter(|(_, win)| win.open)
            .collect();
        let pill_w = self.taskbar_pill_width(w, open_wins.len());
        let mut bx = 102;
        for (orig_idx, win) in open_wins {
            let is_top = Some(orig_idx) == top_vis;
            let pill_hovered = self.mouse_x >= bx
                && self.mouse_x <= bx + pill_w
                && self.mouse_y >= task_y + 6
                && self.mouse_y <= task_y + 30;
            if is_top {
                canvas.blend_rounded_rect(bx, task_y + 6, pill_w, 24, 7, 0xFFFFFF, 24);
                canvas.draw_rounded_rect_alpha(bx, task_y + 6, pill_w, 24, 7, 0xFFFFFF, 34);
            } else if win.minimized {
                let a = if pill_hovered { 16 } else { 7 };
                canvas.blend_rounded_rect(bx, task_y + 6, pill_w, 24, 7, 0xFFFFFF, a);
            } else {
                let a = if pill_hovered { 20 } else { 12 };
                canvas.blend_rounded_rect(bx, task_y + 6, pill_w, 24, 7, 0xFFFFFF, a);
                canvas.draw_rounded_rect_alpha(bx, task_y + 6, pill_w, 24, 7, 0xFFFFFF, 18);
            }

            let fg = if win.minimized {
                0x8B949E
            } else if is_top {
                0xF8FAFC
            } else {
                0xCBD5E1
            };
            if pill_w > 34 {
                canvas.fill_aa_circle((bx + 11) as f32, (task_y + 18) as f32, 2.5, win.id.color(), 255);
                canvas.draw_text_clipped(bx + 19, task_y + 11, pill_w - 24, win.id.title(lang), fg, 1);
            } else {
                canvas.draw_text_clipped(bx + 5, task_y + 11, pill_w - 8, win.id.title(lang), fg, 1);
            }
            if is_top {
                canvas.fill_rounded_rect(bx + 14, task_y + 27, (pill_w - 28).max(8), 2, 1, accent);
            }
            bx += pill_w + 4;
        }

        let lang_x = (w - 144).max(bx + 8);
        let lang_lbl = match lang {
            Lang::Ru => "RU",
            Lang::En => "EN",
        };
        canvas.blend_rounded_rect(lang_x, task_y + 6, 36, 24, 6, 0xFFFFFF, 15);
        canvas.draw_rounded_rect_alpha(lang_x, task_y + 6, 36, 24, 6, 0xFFFFFF, 26);
        canvas.draw_text_clipped(lang_x + 10, task_y + 11, 26, lang_lbl, 0xE2E8F0, 1);
        canvas.fill_aa_circle((lang_x + 46) as f32, (task_y + 18) as f32, 2.5, 0x3FB950, 255);
        let tray_txt = if self.settings.show_clock {
            &self.clock_str
        } else {
            &self.uptime
        };
        canvas.draw_text_clipped(lang_x + 54, task_y + 11, (w - lang_x - 60).max(20), tray_txt, 0xCBD5E1, 1);

        // 5. Start Menu popup
        if self.start_menu_open {
            self.draw_start_menu(canvas, task_y, accent);
        }

        // 6. Right-click Context Menu
        if let Some((cx, cy)) = self.ctx_menu {
            self.draw_context_menu(canvas, cx, cy, accent);
        }

        // 7. Mouse cursor (morphs to crisp AA resize arrows over edges/corners)
        if self.settings.crosshair_cursor {
            canvas.fill_rect(self.mouse_x - 6, self.mouse_y, 13, 1, 0xFFFFFF);
            canvas.fill_rect(self.mouse_x, self.mouse_y - 6, 1, 13, 0xFFFFFF);
        } else if let Some(rz) = self.resizing {
            let (ux, uy) = rz.edge.axis();
            canvas.draw_resize_cursor(self.mouse_x, self.mouse_y, ux, uy);
        } else {
            let mut hover_edge = ResizeEdge::none();
            if self.ctx_menu.is_none()
                && !self.start_menu_open
                && (self.mouse_y < task_y || self.mouse_y >= task_y + 36)
            {
                for win in self.windows.iter().rev() {
                    if !win.open || win.minimized {
                        continue;
                    }
                    let pad = if win.maximized { 0 } else { 4 };
                    if self.mouse_x >= win.x - pad
                        && self.mouse_x < win.x + win.w + pad
                        && self.mouse_y >= win.y - pad
                        && self.mouse_y < win.y + win.h + pad
                    {
                        hover_edge = detect_resize_edge(win, self.mouse_x, self.mouse_y);
                        break;
                    }
                }
            }
            if hover_edge.any() {
                let (ux, uy) = hover_edge.axis();
                canvas.draw_resize_cursor(self.mouse_x, self.mouse_y, ux, uy);
            } else {
                canvas.draw_cursor(self.mouse_x, self.mouse_y);
            }
        }
    }

    fn draw_context_menu(&self, canvas: &mut Canvas, cx: i32, cy: i32, accent: u32) {
        let lang = self.settings.lang;
        let items = [
            (tr(lang, "Открыть Терминал", "Open Terminal"), 0x3FB950),
            (tr(lang, "Сменить обои", "Next Wallpaper"), accent),
            (tr(lang, "Настройки", "Settings"), 0x58A6FF),
            (tr(lang, "Обновить", "Refresh"), 0xD29922),
            (tr(lang, "Выход", "Exit"), 0xF85149),
        ];
        canvas.draw_soft_shadow(cx, cy, 184, 134, 9);
        canvas.blend_rounded_rect(cx, cy, 184, 134, 9, 0x121620, 242);
        canvas.draw_rounded_rect_alpha(cx, cy, 184, 134, 9, 0xFFFFFF, 26);
        for (i, (label, dot_col)) in items.iter().enumerate() {
            let iy = cy + 4 + (i as i32) * 25;
            if i == 4 {
                canvas.blend_rect(cx + 10, iy - 1, 164, 1, 0xFFFFFF, 18);
            }
            if self.mouse_x >= cx
                && self.mouse_x <= cx + 184
                && self.mouse_y >= iy
                && self.mouse_y < iy + 23
            {
                canvas.blend_rounded_rect(cx + 4, iy, 176, 23, 6, accent, 52);
            }
            canvas.fill_aa_circle((cx + 14) as f32, (iy + 11) as f32, 2.7, *dot_col, 255);
            let col = if i == 4 { 0xFF7B72 } else { 0xE2E8F0 };
            canvas.draw_text_clipped(cx + 24, iy + 5, 150, label, col, 1);
        }
    }

    fn draw_start_menu(&self, canvas: &mut Canvas, task_y: i32, accent: u32) {
        let lang = self.settings.lang;
        let menu_w = 228;
        let menu_h = (START_ITEMS.len() as i32) * 24 + 42;
        let menu_x = 8;
        let menu_y = if self.settings.taskbar_top {
            38
        } else {
            task_y - menu_h - 4
        };

        canvas.draw_soft_shadow(menu_x, menu_y, menu_w, menu_h, 11);
        canvas.blend_rounded_rect(menu_x, menu_y, menu_w, menu_h, 11, 0x121620, 244);
        canvas.draw_rounded_rect_alpha(menu_x, menu_y, menu_w, menu_h, 11, 0xFFFFFF, 26);

        canvas.blend_rect(menu_x + 10, menu_y + 30, menu_w - 20, 1, 0xFFFFFF, 18);
        canvas.fill_aa_circle((menu_x + 15) as f32, (menu_y + 15) as f32, 3.0, accent, 255);
        canvas.draw_text_clipped(
            menu_x + 24,
            menu_y + 8,
            menu_w - 86,
            tr(lang, "pizdOS 2.0", "pizdOS 2.0"),
            0xF8FAFC,
            1,
        );
        canvas.draw_text_clipped(menu_x + menu_w - 58, menu_y + 8, 50, "NO GNU", 0x94A3B8, 1);

        for (i, app_opt) in START_ITEMS.iter().enumerate() {
            let iy = menu_y + 34 + (i as i32) * 24;
            if app_opt.is_none() {
                canvas.blend_rect(menu_x + 10, iy - 1, menu_w - 20, 1, 0xFFFFFF, 18);
            }
            let hovered = self.mouse_x >= menu_x
                && self.mouse_x <= menu_x + menu_w
                && self.mouse_y >= iy
                && self.mouse_y < iy + 22;
            if hovered {
                canvas.blend_rounded_rect(menu_x + 6, iy, menu_w - 12, 22, 6, 0xFFFFFF, 22);
                canvas.fill_rounded_rect(menu_x + 6, iy + 5, 3, 12, 1, accent);
            }
            let (label, col, dot_col) = match app_opt {
                Some(id) => (id.title(lang), 0xE6EDF3, id.color()),
                None => (tr(lang, "Выход", "Exit"), 0xFF7B72, 0xF85149),
            };
            canvas.fill_aa_circle((menu_x + 18) as f32, (iy + 11) as f32, 2.7, dot_col, 255);
            canvas.draw_text_clipped(menu_x + 28, iy + 4, menu_w - 38, label, col, 1);
        }
    }

    fn draw_desktop_icon(
        &self,
        canvas: &mut Canvas,
        x: i32,
        y: i32,
        id: AppId,
        label: &str,
        accent: u32,
    ) {
        let hovered = self.mouse_x >= x - 4
            && self.mouse_x <= x + 74
            && self.mouse_y >= y - 2
            && self.mouse_y <= y + 58;
        if hovered {
            canvas.blend_rounded_rect(x - 2, y - 4, 72, 62, 11, 0xFFFFFF, 20);
            canvas.draw_rounded_rect_alpha(x - 2, y - 4, 72, 62, 11, 0xFFFFFF, 40);
        }

        // Cohesive minimalist dark-glass squircle container + monoline vector glyphs
        let bx = x + 14;
        let by = y;
        let cx = bx as f32 + 19.0;
        let cy = by as f32 + 19.0;

        canvas.draw_soft_shadow(bx, by, 38, 38, 11);
        canvas.blend_rounded_rect(bx, by, 38, 38, 11, 0x11151F, 224);
        canvas.fill_aa_circle(cx, cy, 12.5, accent, 26);
        canvas.draw_rounded_rect_alpha(
            bx,
            by,
            38,
            38,
            11,
            0xFFFFFF,
            if hovered { 64 } else { 30 },
        );

        match id {
            AppId::Terminal => {
                canvas.draw_aa_line(cx - 8.5, cy - 5.0, cx - 2.0, cy, 1.8, accent, 255);
                canvas.draw_aa_line(cx - 2.0, cy, cx - 8.5, cy + 5.0, 1.8, accent, 255);
                canvas.draw_aa_line(cx + 1.0, cy + 5.0, cx + 8.5, cy + 5.0, 1.8, 0xE2E8F0, 240);
            }
            AppId::PizGet => {
                // Minimalist package tray + download arrow
                canvas.draw_aa_line(cx, cy - 8.0, cx, cy + 3.0, 1.8, 0xFBBF24, 255);
                canvas.draw_aa_line(cx - 4.5, cy - 1.5, cx, cy + 3.0, 1.8, 0xFBBF24, 255);
                canvas.draw_aa_line(cx + 4.5, cy - 1.5, cx, cy + 3.0, 1.8, 0xFBBF24, 255);
                canvas.draw_aa_line(cx - 8.0, cy + 4.5, cx - 8.0, cy + 7.5, 1.7, 0xE2E8F0, 230);
                canvas.draw_aa_line(cx - 8.0, cy + 7.5, cx + 8.0, cy + 7.5, 1.7, 0xE2E8F0, 230);
                canvas.draw_aa_line(cx + 8.0, cy + 7.5, cx + 8.0, cy + 4.5, 1.7, 0xE2E8F0, 230);
            }
            AppId::Paint => {
                // Minimalist stylus stroke + tri-color palette dots
                canvas.draw_aa_line(cx - 7.5, cy + 6.5, cx + 6.5, cy - 7.5, 2.0, 0xA78BFA, 255);
                canvas.fill_aa_circle(cx + 6.5, cy - 7.5, 2.0, 0xF472B6, 255);
                canvas.fill_aa_circle(cx - 7.0, cy - 5.5, 2.2, 0x38BDF8, 255);
                canvas.fill_aa_circle(cx + 1.0, cy + 7.0, 2.2, 0xFBBF24, 255);
                canvas.fill_aa_circle(cx + 7.5, cy + 3.0, 2.2, 0x4ADE80, 255);
            }
            AppId::Snake => {
                // Sleek monoline snake path + target
                canvas.draw_aa_line(cx - 8.0, cy + 6.0, cx - 1.0, cy + 6.0, 2.6, 0x4ADE80, 255);
                canvas.draw_aa_line(cx - 1.0, cy + 6.0, cx - 1.0, cy - 5.0, 2.6, 0x4ADE80, 255);
                canvas.draw_aa_line(cx - 1.0, cy - 5.0, cx + 7.5, cy - 5.0, 2.6, 0x86EFAC, 255);
                canvas.fill_aa_circle(cx + 7.5, cy - 5.0, 2.2, 0x86EFAC, 255);
                canvas.fill_aa_circle(cx + 7.5, cy + 5.5, 2.6, 0xFB7185, 255);
            }
            AppId::Notepad => {
                canvas.draw_rounded_rect_alpha(bx + 10, by + 9, 18, 20, 4, 0x93C5FD, 215);
                canvas.draw_aa_line(cx - 5.0, cy - 4.0, cx + 5.0, cy - 4.0, 1.5, 0xE2E8F0, 235);
                canvas.draw_aa_line(cx - 5.0, cy, cx + 5.0, cy, 1.5, 0x94A3B8, 215);
                canvas.draw_aa_line(cx - 5.0, cy + 4.0, cx + 2.0, cy + 4.0, 1.5, 0x94A3B8, 215);
            }
            AppId::Settings => {
                // Clean 8-spoke monoline gear emblem
                for k in 0..8 {
                    let ang = (k as f32) * std::f32::consts::FRAC_PI_4;
                    let x1 = cx + ang.cos() * 6.5;
                    let y1 = cy + ang.sin() * 6.5;
                    let x2 = cx + ang.cos() * 10.2;
                    let y2 = cy + ang.sin() * 10.2;
                    canvas.draw_aa_line(x1, y1, x2, y2, 2.1, 0xCBD5E1, 235);
                }
                canvas.fill_aa_circle(cx, cy, 6.8, 0xE2E8F0, 255);
                canvas.fill_aa_circle(cx, cy, 3.6, 0x11151F, 255);
                canvas.fill_aa_circle(cx, cy, 2.2, accent, 255);
            }
            AppId::Calc => {
                canvas.draw_aa_line(cx - 7.5, cy - 4.5, cx - 2.5, cy - 4.5, 1.7, 0xE2E8F0, 235);
                canvas.draw_aa_line(cx - 5.0, cy - 7.0, cx - 5.0, cy - 2.0, 1.7, 0xE2E8F0, 235);
                canvas.draw_aa_line(cx + 2.5, cy - 4.5, cx + 7.5, cy - 4.5, 1.7, 0xE2E8F0, 235);
                canvas.draw_aa_line(cx - 7.0, cy + 2.5, cx - 3.0, cy + 6.5, 1.7, 0x94A3B8, 235);
                canvas.draw_aa_line(cx - 3.0, cy + 2.5, cx - 7.0, cy + 6.5, 1.7, 0x94A3B8, 235);
                canvas.draw_aa_line(cx + 2.5, cy + 3.2, cx + 7.5, cy + 3.2, 1.7, 0x38BDF8, 255);
                canvas.draw_aa_line(cx + 2.5, cy + 6.2, cx + 7.5, cy + 6.2, 1.7, 0x38BDF8, 255);
            }
            AppId::Matrix => {
                canvas.draw_aa_line(cx - 6.0, cy - 8.0, cx - 6.0, cy + 5.0, 1.6, 0x22C55E, 220);
                canvas.draw_aa_line(cx, cy - 5.0, cx, cy + 8.0, 1.8, 0x4ADE80, 255);
                canvas.draw_aa_line(cx + 6.0, cy - 9.0, cx + 6.0, cy + 2.0, 1.6, 0x34D399, 230);
                canvas.fill_aa_circle(cx, cy + 8.0, 1.8, 0xF8FAFC, 255);
                canvas.fill_aa_circle(cx + 6.0, cy + 2.0, 1.6, 0xE2E8F0, 235);
            }
            _ => {}
        }

        let tw = crate::font::measure_text(label, 1).min(78);
        let lx = x + 33 - tw / 2;
        canvas.draw_text_clipped(lx + 1, y + 43, 80, label, 0x05080D, 1);
        canvas.draw_text_clipped(lx, y + 42, 80, label, 0xE2E8F0, 1);
    }

    fn draw_window(&self, canvas: &mut Canvas, win: &Window, focused: bool) {
        let lang = self.settings.lang;
        let accent = self.settings.theme.accent();

        canvas.draw_soft_shadow(win.x, win.y, win.w, win.h, 11);
        canvas.fill_rounded_rect(win.x, win.y, win.w, win.h, 11, 0x131720);
        canvas.draw_rounded_rect_alpha(
            win.x,
            win.y,
            win.w,
            win.h,
            11,
            0xFFFFFF,
            if focused { 36 } else { 18 },
        );

        let tb_col = if focused { 0x181D29 } else { 0x141822 };
        canvas.fill_vgradient_top_rounded_rect(win.x + 1, win.y + 1, win.w - 2, 26, 10, tb_col, tb_col);
        canvas.blend_rect(win.x + 1, win.y + 27, win.w - 2, 1, 0xFFFFFF, 16);
        canvas.fill_aa_circle(
            (win.x + 14) as f32,
            (win.y + 14) as f32,
            3.0,
            win.id.color(),
            if focused { 245 } else { 125 },
        );
        canvas.draw_text_clipped(
            win.x + 24,
            win.y + 7,
            win.w - 106,
            win.id.title(lang),
            if focused { 0xF1F5F9 } else { 0x7D8590 },
            1,
        );

        // Minimalist flat monochrome window controls: ─, □, ×
        let btn_y = win.y + 5;
        let cy = btn_y + 9;
        let hover_y = self.mouse_y >= btn_y && self.mouse_y <= btn_y + 18;
        let min_x = win.x + win.w - 72;
        let max_x = win.x + win.w - 49;
        let cls_x = win.x + win.w - 26;
        let min_hov = hover_y && (min_x..=min_x + 20).contains(&self.mouse_x);
        let max_hov = hover_y && (max_x..=max_x + 20).contains(&self.mouse_x);
        let cls_hov = hover_y && (cls_x..=cls_x + 20).contains(&self.mouse_x);

        if min_hov {
            canvas.fill_aa_circle((min_x + 10) as f32, cy as f32, 9.0, 0xFFFFFF, 22);
        }
        if max_hov {
            canvas.fill_aa_circle((max_x + 10) as f32, cy as f32, 9.0, 0xFFFFFF, 22);
        }
        if cls_hov {
            canvas.fill_aa_circle((cls_x + 10) as f32, cy as f32, 9.0, 0xEF4444, 195);
        }

        let min_a = if min_hov { 245 } else { 155 };
        canvas.draw_aa_line((min_x + 6) as f32, (cy + 1) as f32, (min_x + 14) as f32, (cy + 1) as f32, 1.3, 0xE2E8F0, min_a);

        let max_a = if max_hov { 245 } else { 155 };
        canvas.draw_rounded_rect_alpha(max_x + 6, cy - 4, 8, 8, 1, 0xE2E8F0, max_a);

        let cls_col = if cls_hov { 0xFFFFFF } else { 0xE2E8F0 };
        let cls_a = if cls_hov { 255 } else { 170 };
        canvas.draw_aa_line((cls_x + 6) as f32, (cy - 4) as f32, (cls_x + 14) as f32, (cy + 4) as f32, 1.3, cls_col, cls_a);
        canvas.draw_aa_line((cls_x + 14) as f32, (cy - 4) as f32, (cls_x + 6) as f32, (cy + 4) as f32, 1.3, cls_col, cls_a);

        match win.id {
            AppId::System => self.draw_system_contents(canvas, win),
            AppId::PizGet => self.draw_pizget_contents(canvas, win),
            AppId::Files => self.draw_files_contents(canvas, win),
            AppId::Terminal => self.terminal.draw(canvas, win.x, win.y, win.w, win.h),
            AppId::Paint => self.paint.draw(canvas, win.x, win.y, win.w, win.h, lang),
            AppId::Snake => self.snake.draw(canvas, win.x, win.y, win.w, win.h, lang),
            AppId::Notepad => self.notepad.draw(canvas, win.x, win.y, win.w, win.h, lang),
            AppId::Calc => self.calc.draw(canvas, win.x, win.y, win.w, win.h, accent),
            AppId::Matrix => self.matrix.draw(canvas, win.x, win.y, win.w, win.h, self.tick, lang),
            AppId::Settings => self.settings.draw(
                canvas,
                win.x,
                win.y,
                win.w,
                win.h,
                &self.kernel,
                &self.mem,
                self.cpu_pct,
                &self.uptime,
                if focused { self.mouse_x - win.x } else { -1 },
                if focused { self.mouse_y - win.y } else { -1 },
            ),
        }

        // Subtle minimalist resize grip indicator at bottom-right corner
        if !win.maximized {
            let grip_hov = focused
                && self.mouse_x >= win.x + win.w - 16
                && self.mouse_y >= win.y + win.h - 16
                && self.mouse_x <= win.x + win.w + 4
                && self.mouse_y <= win.y + win.h + 4;
            let ga = if grip_hov || self.resizing.is_some_and(|r| r.id == win.id) {
                195
            } else {
                65
            };
            let gx = (win.x + win.w - 4) as f32;
            let gy = (win.y + win.h - 4) as f32;
            canvas.draw_aa_line(gx - 7.0, gy, gx, gy - 7.0, 1.1, 0xCBD5E1, ga);
            canvas.draw_aa_line(gx - 3.5, gy, gx, gy - 3.5, 1.1, 0xCBD5E1, ga);
        }
    }

    fn draw_system_contents(&self, canvas: &mut Canvas, win: &Window) {
        let lang = self.settings.lang;
        let x = win.x + 14;
        let y = win.y + 36;
        let iw = win.w - 28;

        canvas.draw_text_clipped(
            x,
            y,
            iw,
            &format!("{}: {}", tr(lang, "Ядро", "Kernel"), self.kernel),
            0xE6EDF3,
            1,
        );
        canvas.draw_text_clipped(
            x,
            y + 16,
            iw,
            &format!("{}: {}  |  Load: {}", tr(lang, "Аптайм", "Uptime"), self.uptime, self.loadavg),
            0xE6EDF3,
            1,
        );

        canvas.draw_text_clipped(x, y + 38, iw, &format!("CPU: {}%", self.cpu_pct), 0x79C0FF, 1);
        canvas.draw_progress_bar(x, y + 52, iw, 14, self.cpu_pct, 0x2EA043, 0x0D1117, 0x30363D);

        // Live CPU Area Chart
        let gx = x;
        let gy = y + 72;
        let gh = 42;
        canvas.fill_rounded_rect(gx, gy, iw, gh, 6, 0x0A0F16);
        for pct_line in [1, 2, 3] {
            let ly = gy + (gh * pct_line) / 4;
            canvas.fill_rect(gx + 2, ly, iw - 4, 1, 0x1A222D);
        }
        canvas.draw_rounded_rect_alpha(gx, gy, iw, gh, 6, 0xFFFFFF, 32);
        let bar_w = (iw / 32).max(2);
        for (i, &val) in self.cpu_history.iter().enumerate() {
            let bh = (((val as i32) * (gh - 6)) / 100).max(2);
            let bx = gx + 2 + (i as i32) * bar_w;
            if bx + bar_w < gx + iw {
                let (top_c, bot_c) = if val > 75 {
                    (0xFF7B72, 0xB62324)
                } else if val > 45 {
                    (0xFBBF24, 0xD97706)
                } else {
                    (0x56D364, 0x1B4332)
                };
                canvas.fill_vgradient_rounded_rect(
                    bx,
                    gy + gh - 3 - bh,
                    (bar_w - 1).max(1),
                    bh,
                    1,
                    top_c,
                    bot_c,
                );
            }
        }

        canvas.draw_text_clipped(
            x,
            y + 122,
            iw,
            &format!(
                "RAM: {} / {} MB ({}%)",
                self.mem.used_mb, self.mem.total_mb, self.mem.percent
            ),
            0xD29922,
            1,
        );
        canvas.draw_progress_bar(
            x,
            y + 136,
            iw,
            14,
            self.mem.percent,
            0xD29922,
            0x0D1117,
            0x30363D,
        );

        canvas.draw_text_clipped(
            x,
            y + 158,
            iw,
            &format!("{}: {}", tr(lang, "Процессы", "Processes"), self.procs.len()),
            0x8B949E,
            1,
        );
        for (i, p) in self.procs.iter().take(3).enumerate() {
            canvas.draw_text_clipped(
                x,
                y + 174 + (i as i32) * 14,
                iw,
                &format!("{:<5} {} {:>5}K {}", p.pid, p.state, p.mem_kb, p.name),
                0xC9D1D9,
                1,
            );
        }

        canvas.draw_button(
            x,
            win.y + win.h - 34,
            110,
            24,
            tr(lang, "Обновить", "Refresh"),
            0x238636,
            0xFFFFFF,
            0x3FB950,
        );
    }

    fn draw_pizget_contents(&self, canvas: &mut Canvas, win: &Window) {
        let lang = self.settings.lang;
        let x = win.x + 14;
        let y = win.y + 36;
        let iw = win.w - 28;

        if self.packages.is_empty() {
            canvas.draw_text_clipped(
                x,
                y + 8,
                iw,
                tr(lang, "Нет пакетов (нажми Обновить)", "No packages (click Update)"),
                0x8B949E,
                1,
            );
        } else {
            let max_rows = ((win.h - 86) / 18).max(1) as usize;
            for (i, p) in self.packages.iter().take(max_rows).enumerate() {
                let ry = y + (i as i32) * 18;
                let selected = i == self.pkg_sel;
                if selected {
                    canvas.fill_rounded_rect(x, ry, iw, 16, 4, 0x1F6FEB);
                }
                let tag = if p.installed { "[INST]" } else { "[AVAIL]" };
                let color = if selected {
                    0xFFFFFF
                } else if p.installed {
                    0x3FB950
                } else {
                    0xE6EDF3
                };
                canvas.draw_text_clipped(
                    x + 6,
                    ry + 2,
                    iw - 12,
                    &format!("{tag} {} {} — {}", p.name, p.version, p.desc),
                    color,
                    1,
                );
            }
        }

        let by = win.y + win.h - 36;
        canvas.draw_button(x, by, 90, 24, tr(lang, "Обновить", "Update"), 0x21262D, 0xFFFFFF, 0x58A6FF);
        canvas.draw_button(x + 98, by, 90, 24, tr(lang, "Поставить", "Install"), 0x238636, 0xFFFFFF, 0x3FB950);
        canvas.draw_button(x + 196, by, 90, 24, tr(lang, "Снести", "Remove"), 0xB62324, 0xFFFFFF, 0xFF7B72);
    }

    fn draw_files_contents(&self, canvas: &mut Canvas, win: &Window) {
        let x = win.x + 14;
        let y = win.y + 34;
        let iw = win.w - 28;

        canvas.draw_text_clipped(x, y, iw, &format!("CWD: {}", self.cwd.display()), 0x79C0FF, 1);

        let max_rows = ((win.h - 70) / 18).max(1) as usize;
        for (i, item) in self.files.iter().take(max_rows).enumerate() {
            let ry = win.y + 54 + (i as i32) * 18;
            let selected = i == self.file_sel;
            if selected {
                canvas.fill_rounded_rect(x, ry, iw, 16, 4, 0x21262D);
            }
            let prefix = if item.is_dir { "[DIR]" } else { "     " };
            let color = if item.is_dir { 0x58A6FF } else { 0xE6EDF3 };
            canvas.draw_text_clipped(
                x + 6,
                ry + 2,
                iw - 12,
                &format!("{prefix} {}", item.name),
                color,
                1,
            );
        }
    }
}
