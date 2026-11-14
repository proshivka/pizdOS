use std::cell::RefCell;
use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

use crate::apps::{
    KeyEvent, Lang, NotepadState, PaintState, SettingsState, SnakeState, TerminalState,
    ThemePreset, tr,
};
use crate::gfx::Canvas;
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
}

impl AppId {
    pub fn title(self, lang: Lang) -> &'static str {
        match (self, lang) {
            (AppId::System, Lang::Ru) => "Система",
            (AppId::System, Lang::En) => "System Monitor",
            (AppId::PizGet, Lang::Ru) => "Пакеты piz-get",
            (AppId::PizGet, Lang::En) => "piz-get Store",
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
}

#[derive(Debug, Clone)]
pub struct FileItem {
    pub name: String,
    pub is_dir: bool,
    pub path: PathBuf,
}

const START_ITEMS: [Option<AppId>; 9] = [
    Some(AppId::Terminal),
    Some(AppId::Paint),
    Some(AppId::Snake),
    Some(AppId::Notepad),
    Some(AppId::Files),
    Some(AppId::PizGet),
    Some(AppId::System),
    Some(AppId::Settings),
    None,
];

struct WallpaperCache {
    key: (usize, usize, ThemePreset, bool),
    pixels: Vec<u32>,
}

pub struct Desktop {
    pub running: bool,
    pub start_menu_open: bool,
    pub mouse_x: i32,
    pub mouse_y: i32,
    pub mouse_down: bool,
    pub dragging: Option<(AppId, i32, i32)>,
    pub windows: Vec<Window>,
    pub tick: u64,
    pub status: String,

    // System metrics
    pub cpu_pct: u8,
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
            mouse_x: width / 2,
            mouse_y: height / 2,
            mouse_down: false,
            dragging: None,
            windows: vec![
                Window {
                    id: AppId::Files,
                    x: 150,
                    y: 70,
                    w: 400,
                    h: 300,
                    open: false,
                },
                Window {
                    id: AppId::PizGet,
                    x: 190,
                    y: 90,
                    w: 440,
                    h: 310,
                    open: false,
                },
                Window {
                    id: AppId::Terminal,
                    x: 120,
                    y: 60,
                    w: 440,
                    h: 280,
                    open: false,
                },
                Window {
                    id: AppId::Paint,
                    x: 240,
                    y: 110,
                    w: 368,
                    h: 268,
                    open: false,
                },
                Window {
                    id: AppId::Snake,
                    x: 280,
                    y: 100,
                    w: 320,
                    h: 278,
                    open: false,
                },
                Window {
                    id: AppId::Notepad,
                    x: 210,
                    y: 130,
                    w: 420,
                    h: 270,
                    open: false,
                },
                Window {
                    id: AppId::System,
                    x: 110,
                    y: 45,
                    w: 380,
                    h: 270,
                    open: true,
                },
                Window {
                    id: AppId::Settings,
                    x: 370,
                    y: 90,
                    w: 520,
                    h: 344,
                    open: true,
                },
            ],
            tick: 0,
            status: String::from("0% GNU | 0 Deps"),

            cpu_pct: 12,
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
        }
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
            && self.windows.iter().any(|w| w.id == AppId::Snake && w.open)
        {
            self.snake.step();
        }
    }

    pub fn focus_or_open(&mut self, id: AppId) {
        if let Some(pos) = self.windows.iter().position(|w| w.id == id) {
            let mut win = self.windows.remove(pos);
            win.open = true;
            self.windows.push(win);
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
        let Some(top) = self.windows.iter().rev().find(|w| w.open) else {
            return;
        };
        match top.id {
            AppId::Terminal => self.terminal.on_key(key, &self.kernel, &self.mem),
            AppId::Snake => self.snake.on_key(key),
            AppId::Notepad => self.notepad.on_key(key),
            _ => {}
        }
    }

    pub fn on_mouse_move(&mut self, x: i32, y: i32, canvas_w: i32, canvas_h: i32) {
        self.mouse_x = x.clamp(0, canvas_w.saturating_sub(1));
        self.mouse_y = y.clamp(0, canvas_h.saturating_sub(1));

        if let Some((id, off_x, off_y)) = self.dragging
            && let Some(win) = self.windows.iter_mut().find(|w| w.id == id)
        {
            win.x = (self.mouse_x - off_x).clamp(0, (canvas_w - 60).max(0));
            win.y = (self.mouse_y - off_y).clamp(0, (canvas_h - 60).max(0));
            return;
        }

        if self.mouse_down
            && let Some(top) = self.windows.last()
            && top.open
            && top.id == AppId::Paint
        {
            self.paint
                .paint_at(self.mouse_x - top.x, self.mouse_y - top.y);
        }
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

        let task_y = if self.settings.taskbar_top { 0 } else { canvas_h - 36 };

        // 1. Start Menu popup hit test
        if self.start_menu_open {
            let menu_h = (START_ITEMS.len() as i32) * 26 + 38;
            let menu_y = if self.settings.taskbar_top {
                38
            } else {
                task_y - menu_h - 4
            };
            if (8..=228).contains(&x) && y >= menu_y && y < menu_y + menu_h {
                let rel_y = y - (menu_y + 32);
                if rel_y >= 0 {
                    let idx = (rel_y / 26) as usize;
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
            // Language pill [RU/EN] on right side of taskbar
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
                    self.focus_or_open(id);
                    return;
                }
                bx += pill_w + 4;
            }
            return;
        }

        // 3. Topmost open window hit test
        let mut hit_idx = None;
        for (idx, win) in self.windows.iter().enumerate().rev() {
            if win.open && x >= win.x && x < win.x + win.w && y >= win.y && y < win.y + win.h {
                hit_idx = Some(idx);
                break;
            }
        }

        if let Some(idx) = hit_idx {
            let win = self.windows.remove(idx);
            self.windows.push(win.clone());
            let top_idx = self.windows.len() - 1;

            // Close button [×]
            if x >= win.x + win.w - 28 && x <= win.x + win.w - 6 && y >= win.y + 5 && y <= win.y + 23
            {
                self.windows[top_idx].open = false;
                self.dragging = None;
                return;
            }

            // Titlebar drag
            if y < win.y + 28 {
                self.dragging = Some((win.id, x - win.x, y - win.y));
                return;
            }

            self.handle_window_click(&win, x - win.x, y - win.y);
            return;
        }

        // 4. Desktop icons on the left
        if self.settings.show_icons && (14..=94).contains(&x) {
            if (40..=98).contains(&y) {
                self.focus_or_open(AppId::Terminal);
            } else if (112..=170).contains(&y) {
                self.focus_or_open(AppId::PizGet);
            } else if (184..=242).contains(&y) {
                self.focus_or_open(AppId::Paint);
            } else if (256..=314).contains(&y) {
                self.focus_or_open(AppId::Snake);
            } else if (328..=386).contains(&y) {
                self.focus_or_open(AppId::Notepad);
            } else if (400..=458).contains(&y) {
                self.focus_or_open(AppId::Settings);
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
            self.draw_desktop_icon(canvas, 18, 42, AppId::Terminal.title(lang), 0x3FB950, ">_");
            self.draw_desktop_icon(canvas, 18, 114, "piz-get", 0xD29922, "PKG");
            self.draw_desktop_icon(canvas, 18, 186, "PizPaint", 0xBC8CFF, "ART");
            self.draw_desktop_icon(canvas, 18, 258, AppId::Snake.title(lang), 0x2EA043, "S~");
            self.draw_desktop_icon(canvas, 18, 330, AppId::Notepad.title(lang), 0x58A6FF, "TXT");
            self.draw_desktop_icon(canvas, 18, 402, AppId::Settings.title(lang), 0xF85149, "OS");
        }

        // 3. Open Windows
        for (idx, win) in self.windows.iter().enumerate() {
            if !win.open {
                continue;
            }
            let focused = idx + 1 == self.windows.len();
            self.draw_window(canvas, win, focused);
        }

        // 4. Frosted-glass Taskbar (shows ONLY currently open windows)
        let task_y = if self.settings.taskbar_top { 0 } else { h - 36 };
        canvas.blend_rect(0, task_y, w, 36, 0x0D1117, 215);
        canvas.fill_rect(0, task_y, w, 1, 0x38424F);

        let start_bg = if self.start_menu_open { 0xDA3633 } else { 0xB62324 };
        canvas.draw_button(
            8,
            task_y + 6,
            86,
            24,
            tr(lang, "Пуск", "Start"),
            start_bg,
            0xFFFFFF,
            0xFF7B72,
        );

        let open_wins: Vec<&Window> = self.windows.iter().filter(|win| win.open).collect();
        let pill_w = self.taskbar_pill_width(w, open_wins.len());
        let mut bx = 102;
        for (i, win) in open_wins.iter().enumerate() {
            let is_top = i + 1 == open_wins.len();
            let bg = if is_top { accent } else { 0x21262D };
            let border = if is_top { 0x79C0FF } else { 0x30363D };
            canvas.draw_button(
                bx,
                task_y + 6,
                pill_w,
                24,
                win.id.title(lang),
                bg,
                0xFFFFFF,
                border,
            );
            bx += pill_w + 4;
        }

        // Quick RU / EN language button + Uptime
        let lang_x = (w - 144).max(bx + 8);
        let lang_lbl = match lang {
            Lang::Ru => "RU",
            Lang::En => "EN",
        };
        canvas.draw_button(lang_x, task_y + 6, 36, 24, lang_lbl, 0x21262D, 0xFFFFFF, accent);
        canvas.draw_text_clipped(lang_x + 44, task_y + 12, 96, &self.uptime, 0xC9D1D9, 1);

        // 5. Frosted Start Menu popup
        if self.start_menu_open {
            self.draw_start_menu(canvas, task_y, accent);
        }

        // 6. Mouse cursor
        if self.settings.crosshair_cursor {
            canvas.fill_rect(self.mouse_x - 6, self.mouse_y, 13, 1, 0xFFFFFF);
            canvas.fill_rect(self.mouse_x, self.mouse_y - 6, 1, 13, 0xFFFFFF);
        } else {
            canvas.draw_cursor(self.mouse_x, self.mouse_y);
        }
    }

    fn draw_start_menu(&self, canvas: &mut Canvas, task_y: i32, accent: u32) {
        let lang = self.settings.lang;
        let menu_w = 220;
        let menu_h = (START_ITEMS.len() as i32) * 26 + 38;
        let menu_x = 8;
        let menu_y = if self.settings.taskbar_top {
            38
        } else {
            task_y - menu_h - 4
        };

        canvas.draw_soft_shadow(menu_x, menu_y, menu_w, menu_h, 8);
        canvas.blend_rounded_rect(menu_x, menu_y, menu_w, menu_h, 8, 0x161B22, 238);
        canvas.draw_rounded_rect(menu_x, menu_y, menu_w, menu_h, 8, accent);

        canvas.fill_rounded_rect(menu_x + 2, menu_y + 2, menu_w - 4, 24, 6, accent);
        canvas.draw_text_clipped(
            menu_x + 12,
            menu_y + 8,
            menu_w - 24,
            tr(lang, "pizdOS 2.0 • Пуск", "pizdOS 2.0 • Start"),
            0xFFFFFF,
            1,
        );

        for (i, app_opt) in START_ITEMS.iter().enumerate() {
            let iy = menu_y + 32 + (i as i32) * 26;
            let hovered = self.mouse_x >= menu_x
                && self.mouse_x <= menu_x + menu_w
                && self.mouse_y >= iy
                && self.mouse_y < iy + 24;
            if hovered {
                canvas.fill_rounded_rect(menu_x + 6, iy, menu_w - 12, 24, 5, 0x262D38);
            }
            let (label, col) = match app_opt {
                Some(id) => (id.title(lang), 0xE6EDF3),
                None => (tr(lang, "Выход", "Exit"), 0xFF7B72),
            };
            canvas.draw_text_clipped(menu_x + 14, iy + 6, menu_w - 28, label, col, 1);
        }
    }

    fn draw_desktop_icon(
        &self,
        canvas: &mut Canvas,
        x: i32,
        y: i32,
        label: &str,
        accent: u32,
        badge: &str,
    ) {
        canvas.draw_soft_shadow(x + 12, y, 42, 38, 8);
        canvas.blend_rounded_rect(x + 12, y, 42, 38, 8, 0x1B222C, 225);
        canvas.fill_rounded_rect(x + 12, y, 42, 9, 4, accent);
        canvas.draw_rounded_rect(x + 12, y, 42, 38, 8, 0x38424F);
        canvas.draw_text_clipped(x + 19, y + 17, 32, badge, 0xFFFFFF, 1);

        canvas.draw_text_clipped(x + 3, y + 43, 76, label, 0x090C10, 1);
        canvas.draw_text_clipped(x + 2, y + 42, 76, label, 0xF0F6FC, 1);
    }

    fn draw_window(&self, canvas: &mut Canvas, win: &Window, focused: bool) {
        let lang = self.settings.lang;
        let accent = self.settings.theme.accent();

        canvas.draw_soft_shadow(win.x, win.y, win.w, win.h, 8);
        canvas.fill_rounded_rect(win.x, win.y, win.w, win.h, 8, 0x161B22);
        let border = if focused { accent } else { 0x30363D };
        canvas.draw_rounded_rect(win.x, win.y, win.w, win.h, 8, border);

        let title_bg = if focused { accent } else { 0x21262D };
        canvas.fill_rounded_rect(win.x + 2, win.y + 2, win.w - 4, 24, 6, title_bg);
        canvas.draw_text_clipped(
            win.x + 12,
            win.y + 8,
            win.w - 48,
            win.id.title(lang),
            0xFFFFFF,
            1,
        );
        canvas.draw_button(
            win.x + win.w - 26,
            win.y + 5,
            20,
            18,
            "×",
            0xDA3633,
            0xFFFFFF,
            0xFF7B72,
        );

        match win.id {
            AppId::System => self.draw_system_contents(canvas, win),
            AppId::PizGet => self.draw_pizget_contents(canvas, win),
            AppId::Files => self.draw_files_contents(canvas, win),
            AppId::Terminal => self.terminal.draw(canvas, win.x, win.y, win.w, win.h),
            AppId::Paint => self.paint.draw(canvas, win.x, win.y, win.w, win.h, lang),
            AppId::Snake => self.snake.draw(canvas, win.x, win.y, win.w, win.h, lang),
            AppId::Notepad => self.notepad.draw(canvas, win.x, win.y, win.w, win.h, lang),
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
            &format!("{}: {}", tr(lang, "Аптайм", "Uptime"), self.uptime),
            0xE6EDF3,
            1,
        );
        canvas.draw_text_clipped(x, y + 32, iw, &format!("Load  : {}", self.loadavg), 0xE6EDF3, 1);

        canvas.draw_text_clipped(x, y + 54, iw, &format!("CPU: {}%", self.cpu_pct), 0x79C0FF, 1);
        canvas.draw_progress_bar(x, y + 68, iw, 14, self.cpu_pct, 0x2EA043, 0x0D1117, 0x30363D);

        canvas.draw_text_clipped(
            x,
            y + 90,
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
            y + 104,
            iw,
            14,
            self.mem.percent,
            0xD29922,
            0x0D1117,
            0x30363D,
        );

        canvas.draw_text_clipped(
            x,
            y + 128,
            iw,
            &format!("{}: {}", tr(lang, "Процессы", "Processes"), self.procs.len()),
            0x8B949E,
            1,
        );
        for (i, p) in self.procs.iter().take(4).enumerate() {
            canvas.draw_text_clipped(
                x,
                y + 144 + (i as i32) * 14,
                iw,
                &format!("{:<5} {} {:>5}K {}", p.pid, p.state, p.mem_kb, p.name),
                0xC9D1D9,
                1,
            );
        }

        canvas.draw_button(
            x,
            win.y + win.h - 36,
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
        canvas.draw_button(
            x,
            by,
            90,
            24,
            tr(lang, "Обновить", "Update"),
            0x21262D,
            0xFFFFFF,
            0x58A6FF,
        );
        canvas.draw_button(
            x + 98,
            by,
            90,
            24,
            tr(lang, "Поставить", "Install"),
            0x238636,
            0xFFFFFF,
            0x3FB950,
        );
        canvas.draw_button(
            x + 196,
            by,
            90,
            24,
            tr(lang, "Снести", "Remove"),
            0xB62324,
            0xFFFFFF,
            0xFF7B72,
        );
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
