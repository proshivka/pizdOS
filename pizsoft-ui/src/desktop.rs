use std::cell::RefCell;
use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

use crate::apps::{
    CalcState, KeyEvent, Lang, MatrixState, NotepadState, PaintState, SettingsState, SnakeState,
    TerminalState, ThemePreset, tr,
};
use crate::gfx::{Canvas, lerp_rgb};
use crate::sys::{
    CpuSample, MemInfo, PkgSummary, ProcInfo, list_processes, load_packages, read_cpu_sample,
    read_kernel_version, read_loadavg, read_meminfo, read_uptime_str,
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
    pub windows: Vec<Window>,
    pub tick: u64,
    pub status: String,

    // System metrics
    pub cpu_pct: u8,
    pub cpu_history: [u8; 32],
    pub last_cpu: Option<CpuSample>,
    pub mem: MemInfo,
    pub uptime: String,
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
            windows: vec![
                Window::new(AppId::Files, 150, 70, 400, 300, false),
                Window::new(AppId::PizGet, 190, 90, 440, 310, false),
                Window::new(AppId::Terminal, 120, 60, 450, 290, false),
                Window::new(AppId::Paint, 240, 110, 368, 268, false),
                Window::new(AppId::Snake, 280, 100, 320, 278, false),
                Window::new(AppId::Notepad, 210, 130, 430, 270, false),
                Window::new(AppId::Calc, 300, 90, 260, 310, false),
                Window::new(AppId::Matrix, 240, 80, 380, 280, false),
                Window::new(AppId::System, 190, 46, 390, 290, true),
                Window::new(AppId::Settings, 370, 90, 520, 344, true),
            ],
            tick: 0,
            status: String::from("0% GNU | 0 Deps"),

            cpu_pct: 12,
            cpu_history: [8, 12, 10, 14, 18, 15, 11, 9, 13, 16, 20, 14, 10, 12, 15, 18, 14, 11, 9, 13, 15, 12, 10, 14, 17, 13, 11, 10, 12, 15, 14, 12],
            last_cpu: read_cpu_sample(),
            mem: read_meminfo(),
            uptime: read_uptime_str(),
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
        self.loadavg = read_loadavg();
        self.procs = list_processes();
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
        let Some(top) = self.windows.iter().rev().find(|w| w.open && !w.minimized) else {
            return;
        };
        match top.id {
            AppId::Terminal => self.terminal.on_key(key, &self.kernel, &self.mem),
            AppId::Snake => self.snake.on_key(key),
            AppId::Notepad => self.notepad.on_key(key),
            AppId::Calc => self.calc.on_key(key),
            _ => {}
        }
    }

    pub fn on_mouse_move(&mut self, x: i32, y: i32, canvas_w: i32, canvas_h: i32) {
        self.mouse_x = x.clamp(0, canvas_w.saturating_sub(1));
        self.mouse_y = y.clamp(0, canvas_h.saturating_sub(1));

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

        // 3. Topmost visible window hit test
        let mut hit_idx = None;
        for (idx, win) in self.windows.iter().enumerate().rev() {
            if win.open
                && !win.minimized
                && x >= win.x
                && x < win.x + win.w
                && y >= win.y
                && y < win.y + win.h
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
                    return;
                }
                // Maximize/Restore button [□]
                if x >= win.x + win.w - 49 && x <= win.x + win.w - 29 {
                    self.toggle_maximize(top_idx, canvas_w, canvas_h);
                    self.dragging = None;
                    return;
                }
                // Minimize button [_]
                if x >= win.x + win.w - 72 && x <= win.x + win.w - 52 {
                    self.windows[top_idx].minimized = true;
                    self.dragging = None;
                    return;
                }
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
            AppId::Settings => {
                if self.settings.on_click(lx, ly) {
                    self.refresh_metrics();
                }
            }
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

        // 4. Frosted-glass Taskbar (ONLY open windows)
        let task_y = if self.settings.taskbar_top { 0 } else { h - 36 };
        canvas.blend_rect(0, task_y, w, 36, 0x090D14, 222);
        let rim_y = if self.settings.taskbar_top { task_y + 35 } else { task_y };
        canvas.fill_rect(0, rim_y, w, 1, lerp_rgb(0x283240, accent, 55, 255));
        canvas.blend_rect(0, task_y + 1, w, 1, 0xFFFFFF, 26);

        let start_hovered =
            self.mouse_x >= 8 && self.mouse_x <= 94 && self.mouse_y >= task_y + 6 && self.mouse_y <= task_y + 30;
        let start_top = if self.start_menu_open {
            0xF85149
        } else if start_hovered {
            0xE03E3B
        } else {
            0xCF222E
        };
        canvas.fill_vgradient_rounded_rect(8, task_y + 6, 86, 24, 6, start_top, 0x991B1B);
        canvas.draw_rounded_rect(8, task_y + 6, 86, 24, 6, 0xFF7B72);
        // 4-pane PizdOS Start emblem
        canvas.fill_rect(17, task_y + 13, 4, 4, 0xFFFFFF);
        canvas.fill_rect(22, task_y + 13, 4, 4, 0xFFE4E6);
        canvas.fill_rect(17, task_y + 18, 4, 4, 0xFFE4E6);
        canvas.fill_rect(22, task_y + 18, 4, 4, 0xFCA5A5);
        canvas.draw_text_clipped(32, task_y + 12, 58, tr(lang, "Пуск", "Start"), 0xFFFFFF, 1);

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
            let (top_c, bot_c, border) = if is_top {
                (lerp_rgb(accent, 0xFFFFFF, 35, 255), accent, 0x79C0FF)
            } else if win.minimized {
                (0x161B22, 0x11151B, 0x262C36)
            } else if pill_hovered {
                (0x2D3542, 0x21262D, 0x484F58)
            } else {
                (0x232A35, 0x1A2029, 0x333B48)
            };
            canvas.fill_vgradient_rounded_rect(bx, task_y + 6, pill_w, 24, 6, top_c, bot_c);
            canvas.draw_rounded_rect(bx, task_y + 6, pill_w, 24, 6, border);

            let fg = if win.minimized { 0x8B949E } else { 0xFFFFFF };
            if pill_w > 34 {
                let dot_col = if is_top { 0xFFFFFF } else { win.id.color() };
                canvas.fill_rounded_rect(bx + 7, task_y + 15, 5, 5, 2, dot_col);
                canvas.draw_text_clipped(bx + 15, task_y + 12, pill_w - 20, win.id.title(lang), fg, 1);
            } else {
                canvas.draw_text_clipped(bx + 5, task_y + 12, pill_w - 8, win.id.title(lang), fg, 1);
            }
            if is_top {
                canvas.fill_rounded_rect(bx + 10, task_y + 27, (pill_w - 20).max(8), 2, 1, 0xFFFFFF);
            }
            bx += pill_w + 4;
        }

        let lang_x = (w - 144).max(bx + 8);
        // System Tray pill
        let tray_w = (w - lang_x - 4).max(40);
        canvas.fill_rounded_rect(lang_x - 4, task_y + 4, tray_w + 2, 28, 7, 0x151B24);
        canvas.draw_rounded_rect(lang_x - 4, task_y + 4, tray_w + 2, 28, 7, 0x2C3545);

        let lang_lbl = match lang {
            Lang::Ru => "RU",
            Lang::En => "EN",
        };
        canvas.draw_button(lang_x, task_y + 6, 36, 24, lang_lbl, 0x21262D, 0xFFFFFF, accent);
        canvas.fill_rounded_rect(lang_x + 43, task_y + 15, 5, 5, 2, 0x3FB950);
        canvas.draw_text_clipped(lang_x + 52, task_y + 12, (w - lang_x - 58).max(20), &self.uptime, 0xE6EDF3, 1);

        // 5. Start Menu popup
        if self.start_menu_open {
            self.draw_start_menu(canvas, task_y, accent);
        }

        // 6. Right-click Context Menu
        if let Some((cx, cy)) = self.ctx_menu {
            self.draw_context_menu(canvas, cx, cy, accent);
        }

        // 7. Mouse cursor
        if self.settings.crosshair_cursor {
            canvas.fill_rect(self.mouse_x - 6, self.mouse_y, 13, 1, 0xFFFFFF);
            canvas.fill_rect(self.mouse_x, self.mouse_y - 6, 1, 13, 0xFFFFFF);
        } else {
            canvas.draw_cursor(self.mouse_x, self.mouse_y);
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
        canvas.draw_soft_shadow(cx, cy, 184, 134, 7);
        canvas.blend_rounded_rect(cx, cy, 184, 134, 7, 0x131922, 242);
        canvas.blend_rect(cx + 8, cy + 1, 168, 1, 0xFFFFFF, 32);
        canvas.draw_rounded_rect(cx, cy, 184, 134, 7, lerp_rgb(accent, 0xFFFFFF, 35, 255));
        for (i, (label, dot_col)) in items.iter().enumerate() {
            let iy = cy + 4 + (i as i32) * 25;
            if i == 4 {
                canvas.fill_rect(cx + 10, iy - 1, 164, 1, 0x2D3542);
            }
            if self.mouse_x >= cx
                && self.mouse_x <= cx + 184
                && self.mouse_y >= iy
                && self.mouse_y < iy + 23
            {
                canvas.fill_hgradient_rounded_rect(
                    cx + 4,
                    iy,
                    176,
                    23,
                    5,
                    accent,
                    lerp_rgb(accent, 0xFFFFFF, 35, 255),
                );
            }
            canvas.fill_rounded_rect(cx + 11, iy + 9, 5, 5, 2, *dot_col);
            let col = if i == 4 { 0xFF7B72 } else { 0xF0F6FC };
            canvas.draw_text_clipped(cx + 22, iy + 5, 152, label, col, 1);
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

        canvas.draw_soft_shadow(menu_x, menu_y, menu_w, menu_h, 9);
        canvas.blend_rounded_rect(menu_x, menu_y, menu_w, menu_h, 9, 0x131922, 240);
        canvas.draw_rounded_rect(menu_x, menu_y, menu_w, menu_h, 9, accent);

        canvas.fill_hgradient_rounded_rect(
            menu_x + 2,
            menu_y + 2,
            menu_w - 4,
            28,
            7,
            lerp_rgb(accent, 0x0B0F17, 45, 255),
            accent,
        );
        canvas.blend_rect(menu_x + 8, menu_y + 3, menu_w - 16, 1, 0xFFFFFF, 45);
        canvas.draw_text_clipped(
            menu_x + 12,
            menu_y + 9,
            menu_w - 76,
            tr(lang, "pizdOS 2.0 • Пуск", "pizdOS 2.0 • Start"),
            0xFFFFFF,
            1,
        );
        canvas.draw_text_clipped(menu_x + menu_w - 58, menu_y + 9, 50, "NO GNU", 0xDCE6F2, 1);

        for (i, app_opt) in START_ITEMS.iter().enumerate() {
            let iy = menu_y + 34 + (i as i32) * 24;
            if app_opt.is_none() {
                canvas.fill_rect(menu_x + 10, iy - 1, menu_w - 20, 1, 0x2D3542);
            }
            let hovered = self.mouse_x >= menu_x
                && self.mouse_x <= menu_x + menu_w
                && self.mouse_y >= iy
                && self.mouse_y < iy + 22;
            if hovered {
                canvas.fill_rounded_rect(menu_x + 6, iy, menu_w - 12, 22, 5, 0x26303E);
                canvas.fill_rounded_rect(menu_x + 6, iy + 4, 3, 14, 1, accent);
            }
            let (label, col, dot_col) = match app_opt {
                Some(id) => (id.title(lang), 0xE6EDF3, id.color()),
                None => (tr(lang, "Выход", "Exit"), 0xFF7B72, 0xF85149),
            };
            canvas.fill_rounded_rect(menu_x + 15, iy + 8, 6, 6, 3, dot_col);
            canvas.draw_text_clipped(menu_x + 28, iy + 5, menu_w - 38, label, col, 1);
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
            canvas.blend_rounded_rect(x - 2, y - 4, 72, 62, 9, 0xFFFFFF, 34);
            canvas.draw_rounded_rect(x - 2, y - 4, 72, 62, 9, lerp_rgb(accent, 0xFFFFFF, 90, 255));
        }
        canvas.draw_soft_shadow(x + 12, y, 42, 38, 9);
        canvas.fill_vgradient_rounded_rect(
            x + 12,
            y,
            42,
            38,
            9,
            lerp_rgb(0x1E2836, accent, 42, 255),
            0x111720,
        );
        canvas.blend_rounded_rect(x + 14, y + 2, 38, 16, 6, 0xFFFFFF, 22);
        canvas.draw_rounded_rect(x + 12, y, 42, 38, 9, lerp_rgb(accent, 0xFFFFFF, 45, 255));

        // Mini pixel-art glyph inside icon
        let ix = x + 22;
        let iy = y + 9;
        match id {
            AppId::Terminal => {
                canvas.fill_rounded_rect(ix, iy, 22, 18, 3, 0x090C10);
                canvas.fill_rect(ix + 2, iy + 2, 2, 2, 0xF85149);
                canvas.fill_rect(ix + 5, iy + 2, 2, 2, 0xF59E0B);
                canvas.fill_rect(ix + 8, iy + 2, 2, 2, 0x3FB950);
                canvas.draw_text(ix + 3, iy + 5, ">_", accent, 1);
            }
            AppId::PizGet => {
                canvas.fill_rounded_rect(ix + 2, iy + 2, 18, 16, 3, accent);
                canvas.fill_rect(ix + 2, iy + 2, 18, 4, lerp_rgb(accent, 0xFFFFFF, 65, 255));
                canvas.fill_rect(ix + 2, iy + 8, 18, 2, 0x161B22);
                canvas.fill_rect(ix + 10, iy + 2, 2, 16, 0x161B22);
            }
            AppId::Paint => {
                canvas.fill_rounded_rect(ix + 2, iy + 2, 18, 16, 7, 0x38414E);
                canvas.fill_rect(ix + 5, iy + 5, 4, 4, 0xF85149);
                canvas.fill_rect(ix + 12, iy + 5, 4, 4, 0x3FB950);
                canvas.fill_rect(ix + 6, iy + 11, 4, 4, 0x58A6FF);
                canvas.fill_rect(ix + 13, iy + 12, 3, 3, 0x0D1117);
            }
            AppId::Snake => {
                canvas.fill_rect(ix + 2, iy + 12, 12, 4, accent);
                canvas.fill_rect(ix + 10, iy + 6, 4, 8, accent);
                canvas.fill_rect(ix + 10, iy + 4, 8, 4, 0x56D364);
                canvas.fill_rect(ix + 16, iy + 5, 1, 1, 0x090C10);
                canvas.fill_rect(ix + 17, iy + 12, 3, 3, 0xF85149);
            }
            AppId::Notepad => {
                canvas.fill_rounded_rect(ix + 3, iy + 1, 16, 18, 2, 0xE6EDF3);
                canvas.fill_rect(ix + 14, iy + 1, 5, 5, 0x94A3B8);
                canvas.fill_rect(ix + 6, iy + 6, 9, 2, 0x1F6FEB);
                canvas.fill_rect(ix + 6, iy + 10, 10, 2, 0x6E7681);
                canvas.fill_rect(ix + 6, iy + 14, 7, 2, 0x6E7681);
            }
            AppId::Settings => {
                canvas.fill_rounded_rect(ix + 3, iy + 1, 16, 16, 8, accent);
                canvas.fill_rect(ix + 9, iy - 1, 4, 2, accent);
                canvas.fill_rect(ix + 9, iy + 17, 4, 2, accent);
                canvas.fill_rect(ix + 1, iy + 7, 2, 4, accent);
                canvas.fill_rect(ix + 19, iy + 7, 2, 4, accent);
                canvas.fill_rounded_rect(ix + 7, iy + 5, 8, 8, 4, 0x1B222C);
            }
            AppId::Calc => {
                canvas.fill_rounded_rect(ix + 3, iy + 1, 16, 18, 3, 0x30363D);
                canvas.fill_rect(ix + 5, iy + 3, 12, 4, accent);
                canvas.fill_rect(ix + 5, iy + 9, 3, 3, 0xE6EDF3);
                canvas.fill_rect(ix + 10, iy + 9, 3, 3, 0xE6EDF3);
                canvas.fill_rect(ix + 5, iy + 14, 3, 3, 0xE6EDF3);
                canvas.fill_rect(ix + 10, iy + 14, 3, 3, 0xE6EDF3);
                canvas.fill_rect(ix + 14, iy + 9, 3, 8, 0xF59E0B);
            }
            AppId::Matrix => {
                canvas.fill_rect(ix + 4, iy + 2, 2, 13, 0x2EA043);
                canvas.fill_rect(ix + 4, iy + 15, 2, 2, 0xBBF7D0);
                canvas.fill_rect(ix + 10, iy + 4, 2, 11, 0x56D364);
                canvas.fill_rect(ix + 10, iy + 15, 2, 2, 0xFFFFFF);
                canvas.fill_rect(ix + 16, iy + 1, 2, 10, 0x3FB950);
                canvas.fill_rect(ix + 16, iy + 11, 2, 2, 0xFFFFFF);
            }
            _ => {}
        }

        canvas.draw_text_clipped(x + 3, y + 43, 76, label, 0x05080D, 1);
        canvas.draw_text_clipped(x + 2, y + 42, 76, label, 0xF0F6FC, 1);
    }

    fn draw_window(&self, canvas: &mut Canvas, win: &Window, focused: bool) {
        let lang = self.settings.lang;
        let accent = self.settings.theme.accent();

        canvas.draw_soft_shadow(win.x, win.y, win.w, win.h, 8);
        canvas.fill_rounded_rect(win.x, win.y, win.w, win.h, 8, 0x151B23);
        let border = if focused {
            lerp_rgb(accent, 0xFFFFFF, 45, 255)
        } else {
            0x2C3542
        };
        canvas.draw_rounded_rect(win.x, win.y, win.w, win.h, 8, border);

        let (tb_left, tb_right) = if focused {
            (lerp_rgb(accent, 0x0D1117, 50, 255), accent)
        } else {
            (0x212731, 0x191E26)
        };
        canvas.fill_hgradient_rounded_rect(win.x + 2, win.y + 2, win.w - 4, 24, 6, tb_left, tb_right);
        canvas.blend_rect(win.x + 8, win.y + 3, win.w - 16, 1, 0xFFFFFF, if focused { 52 } else { 22 });
        canvas.fill_rounded_rect(win.x + 10, win.y + 10, 7, 7, 3, win.id.color());
        canvas.draw_text_clipped(
            win.x + 22,
            win.y + 8,
            win.w - 102,
            win.id.title(lang),
            if focused { 0xFFFFFF } else { 0x9CA3AF },
            1,
        );

        // Minimize [_], Maximize [+], Close [×] with hover highlights
        let btn_y = win.y + 5;
        let hover_y = self.mouse_y >= btn_y && self.mouse_y <= btn_y + 18;
        let min_x = win.x + win.w - 72;
        let max_x = win.x + win.w - 49;
        let cls_x = win.x + win.w - 26;
        let min_bg = if hover_y && (min_x..=min_x + 20).contains(&self.mouse_x) {
            0x384252
        } else {
            0x21262D
        };
        let max_bg = if hover_y && (max_x..=max_x + 20).contains(&self.mouse_x) {
            0x384252
        } else {
            0x21262D
        };
        let cls_bg = if hover_y && (cls_x..=cls_x + 20).contains(&self.mouse_x) {
            0xF85149
        } else {
            0xDA3633
        };
        canvas.draw_button(min_x, btn_y, 20, 18, "_", min_bg, 0xFFFFFF, 0x484F58);
        canvas.draw_button(max_x, btn_y, 20, 18, "+", max_bg, 0xFFFFFF, 0x484F58);
        canvas.draw_button(cls_x, btn_y, 20, 18, "×", cls_bg, 0xFFFFFF, 0xFF7B72);

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
            ),
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
        canvas.fill_rounded_rect(gx, gy, iw, gh, 5, 0x0A0F16);
        for pct_line in [1, 2, 3] {
            let ly = gy + (gh * pct_line) / 4;
            canvas.fill_rect(gx + 2, ly, iw - 4, 1, 0x1A222D);
        }
        canvas.draw_rounded_rect(gx, gy, iw, gh, 5, 0x30363D);
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
