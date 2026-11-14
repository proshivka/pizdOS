use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

use crate::apps::{
    KeyEvent, NotepadState, PaintState, SettingsState, SnakeState, TerminalState,
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

#[derive(Debug, Clone)]
pub struct Window {
    pub id: AppId,
    pub title: &'static str,
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

const START_ITEMS: [(Option<AppId>, &str); 9] = [
    (Some(AppId::Terminal), "Terminal"),
    (Some(AppId::Paint), "PizPaint"),
    (Some(AppId::Snake), "Zmeyka"),
    (Some(AppId::Notepad), "PizPad (Bloknot)"),
    (Some(AppId::Files), "Fayly"),
    (Some(AppId::PizGet), "piz-get Store"),
    (Some(AppId::System), "Sistema"),
    (Some(AppId::Settings), "Nastroyki / About OS"),
    (None, "Vyhod (Exit)"),
];

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
}

impl Desktop {
    pub fn new(width: i32, height: i32) -> Self {
        let cwd = env::current_dir().unwrap_or_else(|_| PathBuf::from("/"));
        let piz_root = env::var("PIZ_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("/"));

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
                    title: "Fayly",
                    x: 150,
                    y: 70,
                    w: 400,
                    h: 300,
                    open: false,
                },
                Window {
                    id: AppId::PizGet,
                    title: "piz-get Store",
                    x: 190,
                    y: 90,
                    w: 440,
                    h: 310,
                    open: false,
                },
                Window {
                    id: AppId::Terminal,
                    title: "Terminal",
                    x: 120,
                    y: 60,
                    w: 440,
                    h: 280,
                    open: false,
                },
                Window {
                    id: AppId::Paint,
                    title: "PizPaint",
                    x: 240,
                    y: 110,
                    w: 364,
                    h: 266,
                    open: false,
                },
                Window {
                    id: AppId::Snake,
                    title: "Zmeyka",
                    x: 280,
                    y: 100,
                    w: 320,
                    h: 278,
                    open: false,
                },
                Window {
                    id: AppId::Notepad,
                    title: "PizPad",
                    x: 210,
                    y: 130,
                    w: 420,
                    h: 270,
                    open: false,
                },
                Window {
                    id: AppId::System,
                    title: "Sistema",
                    x: 110,
                    y: 45,
                    w: 380,
                    h: 270,
                    open: true,
                },
                Window {
                    id: AppId::Settings,
                    title: "Nastroyki",
                    x: 370,
                    y: 90,
                    w: 520,
                    h: 340,
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

            terminal: TerminalState::new(),
            paint: PaintState::new(),
            snake: SnakeState::new(),
            notepad: NotepadState::new(),
            settings: SettingsState::new(),
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
            Ok(_) => format!("piz-get {}: oshibka", args.join(" ")),
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
        let avail = (canvas_w - 250).max(120);
        ((avail / open_count as i32) - 4).clamp(64, 120)
    }

    pub fn on_mouse_down(&mut self, x: i32, y: i32, canvas_w: i32, canvas_h: i32) {
        self.mouse_x = x.clamp(0, canvas_w.saturating_sub(1));
        self.mouse_y = y.clamp(0, canvas_h.saturating_sub(1));
        self.mouse_down = true;

        let task_y = if self.settings.taskbar_top { 0 } else { canvas_h - 34 };

        // 1. Start Menu popup hit test
        if self.start_menu_open {
            let menu_h = (START_ITEMS.len() as i32) * 24 + 34;
            let menu_y = if self.settings.taskbar_top { 34 } else { task_y - menu_h };
            if (6..=216).contains(&x) && y >= menu_y && y < menu_y + menu_h {
                let rel_y = y - (menu_y + 28);
                if rel_y >= 0 {
                    let idx = (rel_y / 24) as usize;
                    if let Some((app_opt, _)) = START_ITEMS.get(idx) {
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

        // 2. Taskbar hit test (only currently open windows)
        if y >= task_y && y < task_y + 34 {
            if (6..=96).contains(&x) {
                self.start_menu_open = !self.start_menu_open;
                return;
            }
            let open_ids: Vec<AppId> = self
                .windows
                .iter()
                .filter(|w| w.open)
                .map(|w| w.id)
                .collect();
            let pill_w = self.taskbar_pill_width(canvas_w, open_ids.len());
            let mut bx = 104;
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

            if x >= win.x + win.w - 26 && x <= win.x + win.w - 6 && y >= win.y + 4 && y <= win.y + 22
            {
                self.windows[top_idx].open = false;
                self.dragging = None;
                return;
            }

            if y < win.y + 26 {
                self.dragging = Some((win.id, x - win.x, y - win.y));
                return;
            }

            self.handle_window_click(&win, x - win.x, y - win.y);
            return;
        }

        // 4. Desktop icons on the left
        if (14..=92).contains(&x) {
            if (20..=76).contains(&y) {
                self.focus_or_open(AppId::Terminal);
            } else if (92..=148).contains(&y) {
                self.focus_or_open(AppId::PizGet);
            } else if (164..=220).contains(&y) {
                self.focus_or_open(AppId::Paint);
            } else if (236..=292).contains(&y) {
                self.focus_or_open(AppId::Snake);
            } else if (308..=364).contains(&y) {
                self.focus_or_open(AppId::Notepad);
            } else if (380..=436).contains(&y) {
                self.focus_or_open(AppId::Settings);
            }
        }
    }

    pub fn on_mouse_up(&mut self) {
        self.mouse_down = false;
        self.dragging = None;
    }

    fn handle_window_click(&mut self, win: &Window, lx: i32, ly: i32) {
        match win.id {
            AppId::System => {
                if ly >= win.h - 36 && ly <= win.h - 12 && (14..=114).contains(&lx) {
                    self.refresh_metrics();
                    self.status = String::from("Metriki obnovleny");
                }
            }
            AppId::PizGet => {
                if ly >= 36 && ly < win.h - 46 {
                    let row = ((ly - 36) / 18) as usize;
                    if row < self.packages.len() {
                        self.pkg_sel = row;
                    }
                } else if ly >= win.h - 36 && ly <= win.h - 12 {
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
            AppId::Notepad => self.notepad.on_click(lx, ly, win.h),
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
        let (bg_col, grid_col, accent_col) = self.settings.theme.colors();

        // 1. Desktop background + optional grid
        canvas.fill_rect(0, 0, w, h, bg_col);
        if self.settings.show_grid {
            for gx in (0..w).step_by(32) {
                canvas.fill_rect(gx, 0, 1, h, grid_col);
            }
            for gy in (0..h).step_by(32) {
                canvas.fill_rect(0, gy, w, 1, grid_col);
            }
        }

        // 2. Desktop Icons
        if self.settings.show_icons {
            self.draw_desktop_icon(canvas, 18, 42, "Terminal", 0x3FB950);
            self.draw_desktop_icon(canvas, 18, 114, "piz-get", 0xD29922);
            self.draw_desktop_icon(canvas, 18, 186, "PizPaint", 0xBC8CFF);
            self.draw_desktop_icon(canvas, 18, 258, "Zmeyka", 0x2EA043);
            self.draw_desktop_icon(canvas, 18, 330, "PizPad", 0x58A6FF);
            self.draw_desktop_icon(canvas, 18, 402, "Nastroyki", 0xF85149);
        }

        // 3. Open Windows
        for (idx, win) in self.windows.iter().enumerate() {
            if !win.open {
                continue;
            }
            let focused = idx + 1 == self.windows.len();
            self.draw_window(canvas, win, focused);
        }

        // 4. Taskbar (shows ONLY currently open windows)
        let task_y = if self.settings.taskbar_top { 0 } else { h - 34 };
        canvas.fill_rect(0, task_y, w, 34, 0x161B22);
        canvas.fill_rect(0, task_y, w, 1, 0x30363D);
        let start_bg = if self.start_menu_open { 0xDA3633 } else { 0xB62324 };
        canvas.draw_button(6, task_y + 5, 90, 24, "Pusk", start_bg, 0xFFFFFF, 0xFF7B72);

        let open_wins: Vec<&Window> = self.windows.iter().filter(|win| win.open).collect();
        let pill_w = self.taskbar_pill_width(w, open_wins.len());
        let mut bx = 104;
        for (i, win) in open_wins.iter().enumerate() {
            let is_top = i + 1 == open_wins.len();
            let bg = if is_top { accent_col } else { 0x21262D };
            let border = if is_top { 0x79C0FF } else { 0x30363D };
            canvas.draw_button(bx, task_y + 5, pill_w, 24, win.title, bg, 0xFFFFFF, border);
            bx += pill_w + 4;
        }

        canvas.draw_text_clipped(
            (w - 132).max(bx + 8),
            task_y + 13,
            126,
            &self.uptime,
            0x8B949E,
            1,
        );

        // 5. Start Menu popup
        if self.start_menu_open {
            self.draw_start_menu(canvas, task_y);
        }

        // 6. Mouse cursor
        if self.settings.crosshair_cursor {
            canvas.fill_rect(self.mouse_x - 6, self.mouse_y, 13, 1, 0xFFFFFF);
            canvas.fill_rect(self.mouse_x, self.mouse_y - 6, 1, 13, 0xFFFFFF);
        } else {
            canvas.draw_cursor(self.mouse_x, self.mouse_y);
        }
    }

    fn draw_start_menu(&self, canvas: &mut Canvas, task_y: i32) {
        let menu_w = 210;
        let menu_h = (START_ITEMS.len() as i32) * 24 + 34;
        let menu_x = 6;
        let menu_y = if self.settings.taskbar_top { 34 } else { task_y - menu_h };

        canvas.fill_rect(menu_x + 4, menu_y + 4, menu_w, menu_h, 0x090C10);
        canvas.fill_rect(menu_x, menu_y, menu_w, menu_h, 0x161B22);
        canvas.draw_rect(menu_x, menu_y, menu_w, menu_h, 0x58A6FF);
        canvas.fill_rect(menu_x + 1, menu_y + 1, menu_w - 2, 24, 0x1F6FEB);
        canvas.draw_text_clipped(
            menu_x + 10,
            menu_y + 9,
            menu_w - 20,
            "pizdOS 2.0 | Pusk",
            0xFFFFFF,
            1,
        );

        for (i, (app_opt, label)) in START_ITEMS.iter().enumerate() {
            let iy = menu_y + 28 + (i as i32) * 24;
            let hovered = self.mouse_x >= menu_x
                && self.mouse_x <= menu_x + menu_w
                && self.mouse_y >= iy
                && self.mouse_y < iy + 22;
            if hovered {
                canvas.fill_rect(menu_x + 4, iy, menu_w - 8, 22, 0x21262D);
            }
            let col = if app_opt.is_none() { 0xFF7B72 } else { 0xE6EDF3 };
            canvas.draw_text_clipped(menu_x + 12, iy + 7, menu_w - 24, label, col, 1);
        }
    }

    fn draw_desktop_icon(&self, canvas: &mut Canvas, x: i32, y: i32, label: &str, accent: u32) {
        canvas.fill_rect(x + 12, y, 40, 36, 0x21262D);
        canvas.fill_rect(x + 12, y, 40, 8, accent);
        canvas.draw_rect(x + 12, y, 40, 36, 0x30363D);
        canvas.draw_text_clipped(x, y + 42, 76, label, 0xE6EDF3, 1);
    }

    fn draw_window(&self, canvas: &mut Canvas, win: &Window, focused: bool) {
        canvas.fill_rect(win.x + 4, win.y + 4, win.w, win.h, 0x090C10);
        canvas.fill_rect(win.x, win.y, win.w, win.h, 0x161B22);
        let border = if focused { 0x58A6FF } else { 0x30363D };
        canvas.draw_rect(win.x, win.y, win.w, win.h, border);

        let title_bg = if focused { 0x1F6FEB } else { 0x21262D };
        canvas.fill_rect(win.x + 1, win.y + 1, win.w - 2, 24, title_bg);
        canvas.draw_text_clipped(win.x + 10, win.y + 9, win.w - 44, win.title, 0xFFFFFF, 1);
        canvas.draw_button(
            win.x + win.w - 26,
            win.y + 4,
            20,
            18,
            "X",
            0xDA3633,
            0xFFFFFF,
            0xFF7B72,
        );

        match win.id {
            AppId::System => self.draw_system_contents(canvas, win),
            AppId::PizGet => self.draw_pizget_contents(canvas, win),
            AppId::Files => self.draw_files_contents(canvas, win),
            AppId::Terminal => self.terminal.draw(canvas, win.x, win.y, win.w, win.h),
            AppId::Paint => self.paint.draw(canvas, win.x, win.y, win.w, win.h),
            AppId::Snake => self.snake.draw(canvas, win.x, win.y, win.w, win.h),
            AppId::Notepad => self.notepad.draw(canvas, win.x, win.y, win.w, win.h),
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
        let x = win.x + 14;
        let y = win.y + 36;
        let iw = win.w - 28;

        canvas.draw_text_clipped(x, y, iw, &format!("Kernel: {}", self.kernel), 0xE6EDF3, 1);
        canvas.draw_text_clipped(x, y + 16, iw, &format!("Uptime: {}", self.uptime), 0xE6EDF3, 1);
        canvas.draw_text_clipped(x, y + 32, iw, &format!("Load  : {}", self.loadavg), 0xE6EDF3, 1);

        canvas.draw_text_clipped(x, y + 54, iw, &format!("CPU: {}%", self.cpu_pct), 0x79C0FF, 1);
        canvas.draw_progress_bar(x, y + 66, iw, 14, self.cpu_pct, 0x2EA043, 0x0D1117, 0x30363D);

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
            y + 102,
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
            &format!("Processes: {}", self.procs.len()),
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
            100,
            24,
            "Obnovit",
            0x238636,
            0xFFFFFF,
            0x3FB950,
        );
    }

    fn draw_pizget_contents(&self, canvas: &mut Canvas, win: &Window) {
        let x = win.x + 14;
        let y = win.y + 36;
        let iw = win.w - 28;

        if self.packages.is_empty() {
            canvas.draw_text_clipped(x, y + 8, iw, "Net paketov (nazhmi Obnovi)", 0x8B949E, 1);
        } else {
            let max_rows = ((win.h - 86) / 18).max(1) as usize;
            for (i, p) in self.packages.iter().take(max_rows).enumerate() {
                let ry = y + (i as i32) * 18;
                let selected = i == self.pkg_sel;
                if selected {
                    canvas.fill_rect(x, ry, iw, 16, 0x1F6FEB);
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
                    x + 4,
                    ry + 4,
                    iw - 8,
                    &format!("{tag} {} {} - {}", p.name, p.version, p.desc),
                    color,
                    1,
                );
            }
        }

        let by = win.y + win.h - 36;
        canvas.draw_button(x, by, 90, 24, "Obnovi", 0x21262D, 0xFFFFFF, 0x58A6FF);
        canvas.draw_button(x + 98, by, 90, 24, "Ebash", 0x238636, 0xFFFFFF, 0x3FB950);
        canvas.draw_button(x + 196, by, 90, 24, "Snesti", 0xB62324, 0xFFFFFF, 0xFF7B72);
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
                canvas.fill_rect(x, ry, iw, 16, 0x21262D);
            }
            let prefix = if item.is_dir { "[DIR]" } else { "     " };
            let color = if item.is_dir { 0x58A6FF } else { 0xE6EDF3 };
            canvas.draw_text_clipped(
                x + 4,
                ry + 4,
                iw - 8,
                &format!("{prefix} {}", item.name),
                color,
                1,
            );
        }
    }
}
