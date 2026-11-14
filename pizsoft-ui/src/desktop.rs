use std::cell::RefCell;
use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

use crate::apps::{
    CalcState, KeyEvent, Lang, MatrixState, NotepadState, PaintAction, PaintState, SettingsAction,
    SettingsState, SnakeEvent, SnakeState, TerminalState, ThemePreset, UI_SCALES, tr,
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
            (AppId::Snake, Lang::Ru) => "Pizмейка",
            (AppId::Snake, Lang::En) => "Piznake",
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnapZone {
    Left,
    Right,
    Maximize,
}

#[derive(Debug, Clone)]
pub struct Toast {
    pub text: String,
    pub color: u32,
    pub ttl: u16,
    pub max_ttl: u16,
    pub anim: f32,
    pub vel: f32,
}

#[derive(Debug, Clone, Copy)]
pub struct ClickRipple {
    pub x: i32,
    pub y: i32,
    pub t: f32,
    pub color: u32,
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
    pub open_anim: f32,
    pub min_anim: f32,
    pub close_anim: f32,
    pub vis_rect: [f32; 4],
    pub vis_vel: [f32; 4],
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
            open_anim: 1.0,
            min_anim: if open { 1.0 } else { 0.0 },
            close_anim: if open { 1.0 } else { 0.0 },
            vis_rect: [x as f32, y as f32, w as f32, h as f32],
            vis_vel: [0.0; 4],
        }
    }

    #[inline]
    fn sync_vis_rect(&mut self) {
        self.vis_rect = [self.x as f32, self.y as f32, self.w as f32, self.h as f32];
        self.vis_vel = [0.0; 4];
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
    pub start_anim: f32,
    pub start_vel: f32,
    pub ctx_menu: Option<(i32, i32)>,
    pub ctx_anim: f32,
    pub last_ctx_menu: Option<(i32, i32)>,
    pub cc_open: bool,
    pub cc_anim: f32,
    pub cc_vel: f32,
    pub launcher_open: bool,
    pub launcher_anim: f32,
    pub launcher_vel: f32,
    pub launcher_query: String,
    pub launcher_sel: usize,
    pub snap_zone: Option<SnapZone>,
    pub last_snap: Option<SnapZone>,
    pub snap_anim: f32,
    pub toast: Option<Toast>,
    pub island_anim: f32,
    pub island_vel: f32,
    pub ripples: Vec<ClickRipple>,
    pub dock_float_anim: f32,
    pub dock_float_vel: f32,
    pub last_canvas_size: (i32, i32),
    pub icon_hover: [f32; 8],
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
    anim_canvas: RefCell<Canvas>,
}

#[derive(Clone, Copy)]
enum LauncherKind {
    App(AppId),
    NextTheme,
    ToggleDock,
    RefreshMetrics,
    SetScale(u16),
    MathCopy(f64),
}

struct LauncherEntry {
    label: String,
    badge: &'static str,
    color: u32,
    kind: LauncherKind,
}

fn fmt_math(v: f64) -> String {
    if (v - v.round()).abs() < 1e-9 && v.abs() < 1e14 {
        format!("{}", v.round() as i64)
    } else {
        let s = format!("{v:.6}");
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    }
}

pub fn eval_quick_math(raw: &str) -> Option<f64> {
    let s: String = raw.chars().filter(|c| !c.is_whitespace()).collect();
    if s.is_empty() || !s.bytes().any(|b| b.is_ascii_digit()) {
        return None;
    }
    if !s
        .bytes()
        .all(|b| b.is_ascii_digit() || matches!(b, b'+' | b'-' | b'*' | b'/' | b'.' | b'(' | b')'))
    {
        return None;
    }
    fn parse_expr(b: &[u8], i: &mut usize) -> Option<f64> {
        let mut lhs = parse_term(b, i)?;
        while *i < b.len() && matches!(b[*i], b'+' | b'-') {
            let op = b[*i];
            *i += 1;
            let rhs = parse_term(b, i)?;
            lhs = if op == b'+' { lhs + rhs } else { lhs - rhs };
        }
        Some(lhs)
    }
    fn parse_term(b: &[u8], i: &mut usize) -> Option<f64> {
        let mut lhs = parse_factor(b, i)?;
        while *i < b.len() && matches!(b[*i], b'*' | b'/') {
            let op = b[*i];
            *i += 1;
            let rhs = parse_factor(b, i)?;
            if op == b'/' {
                if rhs.abs() < 1e-12 {
                    return None;
                }
                lhs /= rhs;
            } else {
                lhs *= rhs;
            }
        }
        Some(lhs)
    }
    fn parse_factor(b: &[u8], i: &mut usize) -> Option<f64> {
        if *i >= b.len() {
            return None;
        }
        if b[*i] == b'-' {
            *i += 1;
            return parse_factor(b, i).map(|v| -v);
        }
        if b[*i] == b'+' {
            *i += 1;
            return parse_factor(b, i);
        }
        if b[*i] == b'(' {
            *i += 1;
            let v = parse_expr(b, i)?;
            if *i >= b.len() || b[*i] != b')' {
                return None;
            }
            *i += 1;
            return Some(v);
        }
        let start = *i;
        let mut seen_dot = false;
        while *i < b.len() && (b[*i].is_ascii_digit() || (b[*i] == b'.' && !seen_dot)) {
            if b[*i] == b'.' {
                seen_dot = true;
            }
            *i += 1;
        }
        if *i == start {
            return None;
        }
        core::str::from_utf8(&b[start..*i]).ok()?.parse::<f64>().ok()
    }
    let bytes = s.as_bytes();
    let mut idx = 0;
    let val = parse_expr(bytes, &mut idx)?;
    if idx != bytes.len() || !val.is_finite() {
        return None;
    }
    Some(val)
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
            start_anim: 0.0,
            start_vel: 0.0,
            ctx_menu: None,
            ctx_anim: 0.0,
            last_ctx_menu: None,
            cc_open: false,
            cc_anim: 0.0,
            cc_vel: 0.0,
            launcher_open: false,
            launcher_anim: 0.0,
            launcher_vel: 0.0,
            launcher_query: String::new(),
            launcher_sel: 0,
            snap_zone: None,
            last_snap: None,
            snap_anim: 0.0,
            toast: None,
            island_anim: 0.0,
            island_vel: 0.0,
            ripples: Vec::with_capacity(8),
            dock_float_anim: 0.0,
            dock_float_vel: 0.0,
            last_canvas_size: (width.max(320), height.max(240)),
            icon_hover: [0.0; 8],
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
            anim_canvas: RefCell::new(Canvas::new(1, 1)),
        };

        if let Some(sys) = d.windows.iter_mut().find(|w| w.id == AppId::System) {
            sys.minimized = true;
            sys.min_anim = 0.0;
        }
        d.reload_packages();
        d.reload_files();
        d
    }

    pub fn push_toast(&mut self, text: impl Into<String>, color: u32) {
        let mut msg = text.into();
        if msg.chars().count() > 48 {
            msg = msg.chars().take(45).collect::<String>() + "...";
        }
        self.toast = Some(Toast {
            text: msg,
            color,
            ttl: 115,
            max_ttl: 115,
            anim: if self.settings.animations { 0.05 } else { 1.0 },
            vel: 0.0,
        });
    }

    pub fn toggle_control_center(&mut self) {
        self.cc_open = !self.cc_open;
        if self.cc_open {
            self.start_menu_open = false;
            self.ctx_menu = None;
            self.launcher_open = false;
        }
    }

    pub fn toggle_launcher(&mut self) {
        self.launcher_open = !self.launcher_open;
        if self.launcher_open {
            self.start_menu_open = false;
            self.ctx_menu = None;
            self.cc_open = false;
            self.launcher_query.clear();
            self.launcher_sel = 0;
        }
    }

    fn snap_rect(&self, zone: SnapZone, canvas_w: i32, canvas_h: i32) -> (i32, i32, i32, i32) {
        let reserve = self.dock_reserve();
        let top_bar = if self.settings.taskbar_top { reserve } else { 0 };
        let avail_h = (canvas_h - reserve).max(180);
        match zone {
            SnapZone::Maximize => (0, top_bar, canvas_w.max(220), avail_h),
            SnapZone::Left => (6, top_bar + 6, (canvas_w / 2 - 9).max(160), (avail_h - 12).max(160)),
            SnapZone::Right => (
                canvas_w / 2 + 3,
                top_bar + 6,
                (canvas_w / 2 - 9).max(160),
                (avail_h - 12).max(160),
            ),
        }
    }

    fn rescale_viewport(&mut self, old_scale: f32, cur_log_w: i32, cur_log_h: i32) {
        let new_scale = self.settings.ui_scale();
        if (new_scale - old_scale).abs() > 0.001 && cur_log_w > 0 && cur_log_h > 0 {
            let phys_w = (cur_log_w as f32) * old_scale;
            let phys_h = (cur_log_h as f32) * old_scale;
            let nw = (phys_w / new_scale).round().max(320.0) as i32;
            let nh = (phys_h / new_scale).round().max(240.0) as i32;
            self.on_viewport_resize(nw, nh);
        }
    }

    fn launcher_entries(&self) -> Vec<LauncherEntry> {
        let lang = self.settings.lang;
        let accent = self.settings.active_accent();
        let q = self.launcher_query.trim().to_lowercase();
        let mut out = Vec::new();

        if let Some(val) = eval_quick_math(&self.launcher_query) {
            out.push(LauncherEntry {
                label: format!("= {}", fmt_math(val)),
                badge: tr(lang, "ВЫЧИСЛЕНИЕ", "CALC"),
                color: 0x38BDF8,
                kind: LauncherKind::MathCopy(val),
            });
        }

        let app_aliases = [
            (AppId::Terminal, "terminal консоль bash term"),
            (AppId::Paint, "paint draw кисть рисование pizpaint"),
            (AppId::Snake, "snake game игра змейка piznake pizмейка"),
            (AppId::Notepad, "notepad notes блокнот заметки pizpad"),
            (AppId::Calc, "calc math калькулятор счет"),
            (AppId::Matrix, "matrix rain матрица"),
            (AppId::Files, "files explorer файлы проводник"),
            (AppId::PizGet, "pizget packages пакеты apt"),
            (AppId::System, "system cpu ram система монитор"),
            (AppId::Settings, "settings theme scale настройки тема масштаб"),
        ];
        for (id, alias) in app_aliases {
            let title = id.title(lang);
            if q.is_empty()
                || title.to_lowercase().contains(&q)
                || alias.contains(&q)
            {
                out.push(LauncherEntry {
                    label: String::from(title),
                    badge: tr(lang, "ПРИЛОЖЕНИЕ", "APP"),
                    color: id.color(),
                    kind: LauncherKind::App(id),
                });
            }
        }

        let actions = [
            (
                tr(lang, "Следующие обои", "Next Wallpaper"),
                tr(lang, "ДЕЙСТВИЕ", "ACTION"),
                accent,
                LauncherKind::NextTheme,
                "wallpaper обои theme тема color",
            ),
            (
                tr(lang, "Переключить плавучий док", "Toggle Floating Dock"),
                tr(lang, "ДЕЙСТВИЕ", "ACTION"),
                0xA855F7,
                LauncherKind::ToggleDock,
                "dock док panel панель float",
            ),
            (
                tr(lang, "Обновить метрики системы", "Refresh System Metrics"),
                tr(lang, "ДЕЙСТВИЕ", "ACTION"),
                0x3FB950,
                LauncherKind::RefreshMetrics,
                "refresh обновить cpu ram metrics",
            ),
        ];
        for (lbl, badge, col, kind, keys) in actions {
            if q.is_empty() || lbl.to_lowercase().contains(&q) || keys.contains(&q) {
                out.push(LauncherEntry {
                    label: String::from(lbl),
                    badge,
                    color: col,
                    kind,
                });
            }
        }
        for pct in UI_SCALES {
            let lbl = format!("{} {}%", tr(lang, "Масштаб UI", "UI Scale"), pct);
            if q.is_empty()
                || lbl.to_lowercase().contains(&q)
                || "scale dpi zoom".contains(&q)
            {
                out.push(LauncherEntry {
                    label: lbl,
                    badge: "DPI",
                    color: accent,
                    kind: LauncherKind::SetScale(pct),
                });
            }
        }
        out
    }

    fn exec_launcher_item(&mut self, item: &LauncherEntry) {
        let lang = self.settings.lang;
        match item.kind {
            LauncherKind::App(id) => {
                self.launcher_open = false;
                self.focus_or_open(id);
            }
            LauncherKind::NextTheme => {
                self.launcher_open = false;
                self.settings.theme = match self.settings.theme {
                    ThemePreset::AuroraPeaks => ThemePreset::CrimsonSunset,
                    ThemePreset::CrimsonSunset => ThemePreset::MatrixGlow,
                    ThemePreset::MatrixGlow => ThemePreset::OceanGlass,
                    ThemePreset::OceanGlass => ThemePreset::AuroraPeaks,
                };
                self.push_toast(self.settings.theme.name(lang), self.settings.active_accent());
            }
            LauncherKind::ToggleDock => {
                self.launcher_open = false;
                self.settings.floating_dock = !self.settings.floating_dock;
            }
            LauncherKind::RefreshMetrics => {
                self.launcher_open = false;
                self.refresh_metrics();
                self.push_toast(tr(lang, "Метрики обновлены", "Metrics refreshed"), 0x3FB950);
            }
            LauncherKind::SetScale(pct) => {
                self.launcher_open = false;
                self.settings.ui_scale_pct = pct;
                self.push_toast(
                    format!("{}: {}%", tr(lang, "Масштаб", "Scale"), pct),
                    self.settings.active_accent(),
                );
            }
            LauncherKind::MathCopy(val) => {
                let s = fmt_math(val);
                self.calc.display = s.clone();
                self.calc.acc = None;
                self.calc.op = None;
                self.calc.clear_next = true;
                self.calc.never_settle = false;
                self.launcher_open = false;
                self.push_toast(format!("= {s}"), 0x38BDF8);
            }
        }
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

    #[inline]
    fn dock_reserve(&self) -> i32 {
        if self.settings.floating_dock { 46 } else { 36 }
    }

    pub fn on_viewport_resize(&mut self, canvas_w: i32, canvas_h: i32) {
        self.last_canvas_size = (canvas_w.max(320), canvas_h.max(240));
        let reserve = self.dock_reserve();
        let top_bar = if self.settings.taskbar_top { reserve } else { 0 };
        for win in &mut self.windows {
            if win.maximized {
                win.x = 0;
                win.y = top_bar;
                win.w = canvas_w.max(200);
                win.h = (canvas_h - reserve).max(160);
            } else {
                win.x = win.x.clamp(0, (canvas_w - 80).max(0));
                win.y = win.y.clamp(top_bar, (canvas_h - 60).max(top_bar));
            }
            win.sync_vis_rect();
        }
    }

    pub fn on_escape(&mut self) -> bool {
        if self.ctx_menu.take().is_some() {
            return true;
        }
        if self.launcher_open {
            self.launcher_open = false;
            return true;
        }
        if self.cc_open {
            self.cc_open = false;
            return true;
        }
        if self.start_menu_open {
            self.start_menu_open = false;
            return true;
        }
        if self.settings.display_picker.take().is_some() {
            return true;
        }
        if self.settings.editing_device {
            self.settings.on_key(KeyEvent::Escape);
            return true;
        }
        false
    }

    #[inline]
    fn taskbar_rect(&self, canvas_w: i32, canvas_h: i32) -> (i32, i32, i32, i32, bool) {
        let t = Canvas::ease_out_cubic(self.dock_float_anim.clamp(0.0, 1.0));
        let target_w = (canvas_w - 28).max(260);
        let dw = canvas_w - ((canvas_w - target_w) as f32 * t).round() as i32;
        let dx = (canvas_w - dw) / 2;
        let base_y = if self.settings.taskbar_top { 0 } else { canvas_h - 36 };
        let dy = if self.settings.taskbar_top {
            base_y + (6.0 * t).round() as i32
        } else {
            base_y - (6.0 * t).round() as i32
        };
        (dx, dy, dw, 36, t > 0.01)
    }

    fn taskbar_pill_width(&self, canvas_w: i32, open_count: usize) -> i32 {
        if open_count == 0 {
            return 100;
        }
        let avail = (canvas_w - 310).max(120);
        ((avail / open_count as i32) - 4).clamp(64, 130)
    }

    fn taskbar_pill_rect(&self, canvas_w: i32, canvas_h: i32, id: AppId) -> (i32, i32, i32) {
        let (dock_x, task_y, dock_w, _, _) = self.taskbar_rect(canvas_w, canvas_h);
        let open_count = self.windows.iter().filter(|w| w.open).count();
        let pill_w = self.taskbar_pill_width(dock_w, open_count);
        let pos = self
            .windows
            .iter()
            .filter(|w| w.open)
            .position(|w| w.id == id)
            .unwrap_or(0) as i32;
        (dock_x + 102 + pos * (pill_w + 4), task_y + 6, pill_w)
    }

    pub fn on_tick(&mut self) {
        self.tick = self.tick.wrapping_add(1);
        self.settings.step_anim();
        self.calc.step_anim();

        let start_tgt = if self.start_menu_open { 1.0 } else { 0.0 };
        let ctx_tgt = if self.ctx_menu.is_some() { 1.0 } else { 0.0 };
        let cc_tgt = if self.cc_open { 1.0 } else { 0.0 };
        let launch_tgt = if self.launcher_open { 1.0 } else { 0.0 };
        let dock_tgt = if self.settings.floating_dock { 1.0 } else { 0.0 };
        let snap_tgt = if self.snap_zone.is_some() { 1.0 } else { 0.0 };
        let island_tgt = if self.settings.persistent_island { 1.0 } else { 0.0 };
        let genie_step = if self.settings.genie_minimize { 0.070 } else { 0.095 };

        if !self.settings.animations {
            self.start_anim = start_tgt;
            self.start_vel = 0.0;
            self.ctx_anim = ctx_tgt;
            self.cc_anim = cc_tgt;
            self.cc_vel = 0.0;
            self.launcher_anim = launch_tgt;
            self.launcher_vel = 0.0;
            self.dock_float_anim = dock_tgt;
            self.dock_float_vel = 0.0;
            self.island_anim = island_tgt;
            self.island_vel = 0.0;
            self.snap_anim = snap_tgt;
            if self.ctx_menu.is_none() {
                self.last_ctx_menu = None;
            }
            if self.snap_zone.is_none() {
                self.last_snap = None;
            }
            self.ripples.clear();
            if let Some(t) = self.toast.as_mut() {
                if t.ttl > 0 {
                    t.ttl -= 1;
                }
                t.anim = if t.ttl > 8 { 1.0 } else { 0.0 };
                t.vel = 0.0;
            }
            if self.toast.as_ref().is_some_and(|t| t.ttl == 0) {
                self.toast = None;
            }
            for (i, &(_, ix, iy, _)) in DESKTOP_ICONS.iter().enumerate() {
                let hov = self.ctx_menu.is_none()
                    && !self.start_menu_open
                    && !self.cc_open
                    && !self.launcher_open
                    && self.mouse_x >= ix - 4
                    && self.mouse_x <= ix + 74
                    && self.mouse_y >= iy - 2
                    && self.mouse_y <= iy + 58;
                self.icon_hover[i] = if hov { 1.0 } else { 0.0 };
            }
            for w in &mut self.windows {
                w.open_anim = 1.0;
                w.min_anim = if w.open && !w.minimized { 1.0 } else { 0.0 };
                w.close_anim = if w.open { 1.0 } else { 0.0 };
                w.sync_vis_rect();
            }
        } else {
            Canvas::step_spring(&mut self.start_anim, &mut self.start_vel, start_tgt, 0.36, 0.52, 0.008);
            Canvas::step_spring(&mut self.cc_anim, &mut self.cc_vel, cc_tgt, 0.36, 0.52, 0.008);
            Canvas::step_spring(
                &mut self.launcher_anim,
                &mut self.launcher_vel,
                launch_tgt,
                0.38,
                0.52,
                0.008,
            );
            Canvas::step_spring(
                &mut self.dock_float_anim,
                &mut self.dock_float_vel,
                dock_tgt,
                0.34,
                0.52,
                0.008,
            );
            Canvas::step_spring(
                &mut self.island_anim,
                &mut self.island_vel,
                island_tgt,
                0.36,
                0.54,
                0.008,
            );

            let cd = ctx_tgt - self.ctx_anim;
            if cd.abs() < 0.015 {
                self.ctx_anim = ctx_tgt;
                if self.ctx_menu.is_none() {
                    self.last_ctx_menu = None;
                }
            } else {
                self.ctx_anim += cd * 0.36;
            }

            let snd = snap_tgt - self.snap_anim;
            if snd.abs() < 0.015 {
                self.snap_anim = snap_tgt;
                if self.snap_zone.is_none() {
                    self.last_snap = None;
                }
            } else {
                self.snap_anim += snd * 0.32;
            }

            if let Some(t) = self.toast.as_mut() {
                if t.ttl > 0 {
                    t.ttl -= 1;
                }
                let tgt = if t.ttl > 14 { 1.0 } else { 0.0 };
                Canvas::step_spring(&mut t.anim, &mut t.vel, tgt, 0.36, 0.52, 0.008);
            }
            if self
                .toast
                .as_ref()
                .is_some_and(|t| t.ttl == 0 && t.anim <= 0.02)
            {
                self.toast = None;
            }

            for r in &mut self.ripples {
                r.t += 0.11;
            }
            self.ripples.retain(|r| r.t < 1.0);

            for (i, &(_, ix, iy, _)) in DESKTOP_ICONS.iter().enumerate() {
                let hov = self.ctx_menu.is_none()
                    && !self.start_menu_open
                    && !self.cc_open
                    && !self.launcher_open
                    && self.mouse_x >= ix - 4
                    && self.mouse_x <= ix + 74
                    && self.mouse_y >= iy - 2
                    && self.mouse_y <= iy + 58;
                let tgt = if hov { 1.0 } else { 0.0 };
                let d = tgt - self.icon_hover[i];
                if d.abs() < 0.02 {
                    self.icon_hover[i] = tgt;
                } else {
                    self.icon_hover[i] += d * 0.30;
                }
            }

            for w in &mut self.windows {
                if !w.open {
                    w.close_anim = (w.close_anim - 0.13).max(0.0);
                    w.min_anim = 0.0;
                } else {
                    w.close_anim = 1.0;
                    if w.minimized {
                        w.min_anim = (w.min_anim - genie_step).max(0.0);
                    } else {
                        w.min_anim = (w.min_anim + genie_step * 1.12).min(1.0);
                    }
                    if w.open_anim < 1.0 {
                        w.open_anim = (w.open_anim + 0.115).min(1.0);
                    }
                }
                let targets = [w.x as f32, w.y as f32, w.w as f32, w.h as f32];
                for (k, &tgt) in targets.iter().enumerate() {
                    Canvas::step_spring(
                        &mut w.vis_rect[k],
                        &mut w.vis_vel[k],
                        tgt,
                        0.40,
                        0.50,
                        0.45,
                    );
                }
            }
        }

        if self.tick.is_multiple_of(30) {
            self.refresh_metrics();
        }
        let div = self.settings.snake_tick_div.max(1);
        if self
            .windows
            .iter()
            .any(|w| w.id == AppId::Snake && w.open && !w.minimized)
        {
            if self.tick.is_multiple_of(div) {
                match self.snake.step() {
                    SnakeEvent::NewBest(score) => {
                        let msg = format!(
                            "{}: {score}",
                            tr(self.settings.lang, "Новый рекорд", "New Best")
                        );
                        self.push_toast(msg, 0xFBBF24);
                    }
                    SnakeEvent::AteBonus => {
                        self.push_toast(
                            tr(self.settings.lang, "Золотой бонус +3!", "Golden Bonus +3!"),
                            0xF59E0B,
                        );
                    }
                    SnakeEvent::GameOver | SnakeEvent::None => {}
                }
            } else {
                self.snake.step_interp(div, self.settings.animations);
            }
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
            let was_open = win.open;
            let was_min = win.minimized;
            win.open = true;
            win.minimized = false;
            win.close_anim = 1.0;
            if !was_open {
                win.min_anim = 1.0;
                win.open_anim = if self.settings.animations { 0.0 } else { 1.0 };
                win.sync_vis_rect();
            } else if was_min {
                if !self.settings.animations {
                    win.min_anim = 1.0;
                }
                win.open_anim = 1.0;
                win.sync_vis_rect();
            }
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
        let reserve = self.dock_reserve();
        let top_bar = if self.settings.taskbar_top { reserve } else { 0 };
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
            win.y = top_bar;
            win.w = canvas_w;
            win.h = (canvas_h - reserve).max(160);
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

    fn launcher_rect(&self, canvas_w: i32, canvas_h: i32) -> (i32, i32, i32, i32) {
        let lw = (canvas_w - 48).clamp(280, 420);
        let lh = 286;
        let lx = (canvas_w - lw) / 2;
        let ly = ((canvas_h - lh) / 3).max(20);
        (lx, ly, lw, lh)
    }

    fn cc_rect(&self, canvas_w: i32, canvas_h: i32) -> (i32, i32, i32, i32) {
        let (dock_x, task_y, dock_w, dock_h, _) = self.taskbar_rect(canvas_w, canvas_h);
        let pw = 276;
        let ph = 266;
        let px = (dock_x + dock_w - pw - 8).max(8);
        let py = if self.settings.taskbar_top {
            task_y + dock_h + 6
        } else {
            task_y - ph - 6
        };
        (px, py, pw, ph)
    }

    fn idle_island_text(&self) -> String {
        let clock_or_up = if self.settings.show_clock {
            &self.clock_str
        } else {
            &self.uptime
        };
        format!("pizdOS  ·  CPU {}%  ·  {}", self.cpu_pct, clock_or_up)
    }

    fn idle_island_rect(&self, canvas_w: i32, canvas_h: i32, txt: &str) -> (i32, i32, i32, i32) {
        let text_w = crate::font::measure_text_scaled(txt, 1, 1.0);
        let tw = (text_w + 48).clamp(164, (canvas_w - 32).max(164));
        let th = 26;
        let tx = (canvas_w - tw) / 2;
        let ty = if self.settings.taskbar_top { canvas_h - 40 } else { 10 };
        (tx, ty, tw, th)
    }

    pub fn on_key(&mut self, key: KeyEvent) {
        if key == KeyEvent::Escape && self.on_escape() {
            return;
        }
        match key {
            KeyEvent::ToggleLauncher => {
                self.toggle_launcher();
                return;
            }
            KeyEvent::ToggleControlCenter => {
                self.toggle_control_center();
                return;
            }
            KeyEvent::ScaleDelta(delta) => {
                let old_scale = self.settings.ui_scale();
                if delta == 0 {
                    self.settings.ui_scale_pct = 100;
                } else {
                    self.settings.step_ui_scale(delta);
                }
                let (cw, ch) = self.last_canvas_size;
                self.rescale_viewport(old_scale, cw, ch);
                let msg = format!(
                    "{}: {}%",
                    tr(self.settings.lang, "Масштаб UI", "UI Scale"),
                    self.settings.ui_scale_pct
                );
                self.push_toast(msg, self.settings.active_accent());
                return;
            }
            _ => {}
        }

        if self.launcher_open {
            let entries = self.launcher_entries();
            match key {
                KeyEvent::Up => {
                    self.launcher_sel = self.launcher_sel.saturating_sub(1);
                }
                KeyEvent::Down => {
                    if !entries.is_empty() && self.launcher_sel + 1 < entries.len().min(7) {
                        self.launcher_sel += 1;
                    }
                }
                KeyEvent::Enter => {
                    if let Some(entry) = entries.get(self.launcher_sel) {
                        self.exec_launcher_item(entry);
                    }
                }
                KeyEvent::Backspace => {
                    self.launcher_query.pop();
                    self.launcher_sel = 0;
                }
                KeyEvent::Char(ch) if !ch.is_control() && self.launcher_query.chars().count() < 36 => {
                    self.launcher_query.push(ch);
                    self.launcher_sel = 0;
                }
                _ => {}
            }
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
        self.last_canvas_size = (canvas_w.max(320), canvas_h.max(240));
        self.mouse_x = x.clamp(0, canvas_w.saturating_sub(1));
        self.mouse_y = y.clamp(0, canvas_h.saturating_sub(1));

        if let Some(rz) = self.resizing {
            let reserve = self.dock_reserve();
            if let Some(win) = self.windows.iter_mut().find(|w| w.id == rz.id) {
                if !win.maximized {
                    let (min_w, min_h) = win.id.min_size();
                    let (sx, sy, sw, sh) = rz.start_rect;
                    let dx = self.mouse_x - rz.start_mx;
                    let dy = self.mouse_y - rz.start_my;
                    let top_min = if self.settings.taskbar_top { reserve } else { 0 };
                    let bot_max = if self.settings.taskbar_top {
                        canvas_h
                    } else {
                        (canvas_h - reserve).max(top_min + min_h)
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
                    win.sync_vis_rect();
                }
                return;
            }
        }

        if let Some((id, off_x, off_y)) = self.dragging
            && let Some(win) = self.windows.iter_mut().find(|w| w.id == id)
        {
            if !win.maximized {
                win.x = (self.mouse_x - off_x).clamp(0, (canvas_w - 60).max(0));
                win.y = (self.mouse_y - off_y).clamp(0, (canvas_h - 60).max(0));
                win.sync_vis_rect();

                let reserve = self.dock_reserve();
                let top_min = if self.settings.taskbar_top { reserve } else { 0 };
                let zone = if self.mouse_y <= top_min + 10 {
                    Some(SnapZone::Maximize)
                } else if self.mouse_x <= 14 {
                    Some(SnapZone::Left)
                } else if self.mouse_x >= canvas_w - 15 {
                    Some(SnapZone::Right)
                } else {
                    None
                };
                self.snap_zone = zone;
                if let Some(z) = zone {
                    self.last_snap = Some(z);
                }
            }
            return;
        }

        if self.mouse_down
            && !self.launcher_open
            && !self.cc_open
            && !self.start_menu_open
            && let Some(top) = self.windows.iter().rev().find(|w| w.open && !w.minimized)
            && top.id == AppId::Paint
        {
            self.paint
                .paint_at(self.mouse_x - top.x, self.mouse_y - top.y);
        }
    }

    pub fn on_right_click(&mut self, x: i32, y: i32, canvas_w: i32, canvas_h: i32) {
        self.last_canvas_size = (canvas_w.max(320), canvas_h.max(240));
        self.mouse_x = x.clamp(0, canvas_w.saturating_sub(1));
        self.mouse_y = y.clamp(0, canvas_h.saturating_sub(1));
        self.start_menu_open = false;
        self.cc_open = false;
        self.launcher_open = false;
        let mx = x.min((canvas_w - 190).max(0));
        let my = y.min((canvas_h - 170).max(0));
        self.ctx_menu = Some((mx, my));
        self.last_ctx_menu = Some((mx, my));
    }

    pub fn on_mouse_down(&mut self, x: i32, y: i32, canvas_w: i32, canvas_h: i32) {
        self.last_canvas_size = (canvas_w.max(320), canvas_h.max(240));
        self.mouse_x = x.clamp(0, canvas_w.saturating_sub(1));
        self.mouse_y = y.clamp(0, canvas_h.saturating_sub(1));
        self.mouse_down = true;
        if self.settings.animations && self.ripples.len() < 6 {
            self.ripples.push(ClickRipple {
                x: self.mouse_x,
                y: self.mouse_y,
                t: 0.0,
                color: self.settings.active_accent(),
            });
        }

        // 0. Right-click context menu hit test
        if let Some((cx, cy)) = self.ctx_menu {
            self.ctx_menu = None;
            if x >= cx && x <= cx + 184 && y >= cy && y <= cy + 134 {
                let row = (y - cy - 4) / 25;
                let lang = self.settings.lang;
                match row {
                    0 => self.focus_or_open(AppId::Terminal),
                    1 => {
                        self.settings.theme = match self.settings.theme {
                            ThemePreset::AuroraPeaks => ThemePreset::CrimsonSunset,
                            ThemePreset::CrimsonSunset => ThemePreset::MatrixGlow,
                            ThemePreset::MatrixGlow => ThemePreset::OceanGlass,
                            ThemePreset::OceanGlass => ThemePreset::AuroraPeaks,
                        };
                        self.push_toast(
                            self.settings.theme.name(lang),
                            self.settings.active_accent(),
                        );
                    }
                    2 => self.focus_or_open(AppId::Settings),
                    3 => {
                        self.refresh_metrics();
                        self.push_toast(tr(lang, "Метрики обновлены", "Metrics refreshed"), 0x3FB950);
                    }
                    4 => self.running = false,
                    _ => {}
                }
                return;
            }
        }

        let (dock_x, task_y, dock_w, dock_h, _floating) = self.taskbar_rect(canvas_w, canvas_h);
        let lang_x = (dock_x + dock_w - 144).max(dock_x + 108);
        let search_hit = x >= dock_x
            && x < dock_x + dock_w
            && y >= task_y
            && y < task_y + dock_h
            && x >= lang_x - 60
            && x <= lang_x - 35;
        let cc_hit = x >= dock_x
            && x < dock_x + dock_w
            && y >= task_y
            && y < task_y + dock_h
            && x >= lang_x - 30
            && x <= lang_x - 5;

        // 0b. Spotlight Launcher overlay
        if self.launcher_open {
            let (lx, ly, lw, lh) = self.launcher_rect(canvas_w, canvas_h);
            if x >= lx && x <= lx + lw && y >= ly && y <= ly + lh {
                let entries = self.launcher_entries();
                if y >= ly + 52 {
                    let row = ((y - (ly + 52)) / 28) as usize;
                    if row < 7 && let Some(item) = entries.get(row) {
                        self.exec_launcher_item(item);
                    }
                }
                return;
            }
            if !search_hit {
                self.launcher_open = false;
                return;
            }
        }

        // 0c. ColorOS Control Center overlay
        if self.cc_open {
            let (px, py, pw, ph) = self.cc_rect(canvas_w, canvas_h);
            if x >= px && x <= px + pw && y >= py && y <= py + ph {
                let lang = self.settings.lang;
                let tiles_y = py + 36;
                for row in 0..2 {
                    for col in 0..2 {
                        let tx = px + 10 + col * 132;
                        let ty = tiles_y + row * 48;
                        if x >= tx && x <= tx + 124 && y >= ty && y <= ty + 42 {
                            match (col, row) {
                                (0, 0) => {
                                    self.settings.floating_dock = !self.settings.floating_dock;
                                    self.on_viewport_resize(canvas_w, canvas_h);
                                }
                                (1, 0) => self.settings.animations = !self.settings.animations,
                                (0, 1) => {
                                    self.settings.genie_minimize = !self.settings.genie_minimize
                                }
                                _ => self.settings.show_grid = !self.settings.show_grid,
                            }
                            return;
                        }
                    }
                }
                let glass_y = py + 136;
                if y >= glass_y && y <= glass_y + 26 {
                    for idx in 0..3 {
                        let gx = px + 10 + idx * 87;
                        if x >= gx && x <= gx + 82 {
                            self.settings.glass_level = idx as u8;
                            return;
                        }
                    }
                }
                let scale_y = py + 170;
                if y >= scale_y && y <= scale_y + 26 {
                    for (idx, &pct) in UI_SCALES.iter().enumerate() {
                        let sx = px + 10 + (idx as i32) * 65;
                        if x >= sx && x <= sx + 61 {
                            let old_scale = self.settings.ui_scale();
                            self.settings.ui_scale_pct = pct;
                            self.rescale_viewport(old_scale, canvas_w, canvas_h);
                            self.push_toast(
                                format!("{}: {}%", tr(lang, "Масштаб UI", "UI Scale"), pct),
                                self.settings.active_accent(),
                            );
                            return;
                        }
                    }
                }
                let island_y = py + 202;
                if y >= island_y && y <= island_y + 26 && x >= px + 10 && x <= px + pw - 10 {
                    self.settings.persistent_island = !self.settings.persistent_island;
                    return;
                }
                let foot_y = py + 234;
                if y >= foot_y && y <= foot_y + 24 {
                    self.refresh_metrics();
                    self.push_toast(tr(lang, "Метрики обновлены", "Metrics refreshed"), 0x3FB950);
                }
                return;
            }
            if !cc_hit {
                self.cc_open = false;
            }
        }

        // 1. Start Menu popup hit test
        if self.start_menu_open {
            let menu_w = 228;
            let menu_h = (START_ITEMS.len() as i32) * 24 + 42;
            let menu_x = dock_x + 8;
            let menu_y = if self.settings.taskbar_top {
                task_y + dock_h + 2
            } else {
                task_y - menu_h - 4
            };
            if x >= menu_x && x <= menu_x + menu_w && y >= menu_y && y < menu_y + menu_h {
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
        if x >= dock_x && x < dock_x + dock_w && y >= task_y && y < task_y + dock_h {
            if x >= dock_x + 8 && x <= dock_x + 94 {
                self.start_menu_open = !self.start_menu_open;
                if self.start_menu_open {
                    self.cc_open = false;
                    self.launcher_open = false;
                }
                return;
            }
            if search_hit {
                self.toggle_launcher();
                return;
            }
            if cc_hit {
                self.toggle_control_center();
                return;
            }
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

            self.cc_open = false;
            self.launcher_open = false;
            let open_ids: Vec<AppId> = self
                .windows
                .iter()
                .filter(|w| w.open)
                .map(|w| w.id)
                .collect();
            let pill_w = self.taskbar_pill_width(dock_w, open_ids.len());
            let mut bx = dock_x + 102;
            for id in open_ids {
                if x >= bx && x <= bx + pill_w {
                    self.toggle_taskbar_window(id);
                    return;
                }
                bx += pill_w + 4;
            }
            return;
        }

        // 2b. Always-On Dynamic Island pill click (toggles Control Center)
        if self.settings.persistent_island && self.toast.is_none() {
            let txt = self.idle_island_text();
            let (ix, iy, iw, ih) = self.idle_island_rect(canvas_w, canvas_h, &txt);
            if x >= ix && x <= ix + iw && y >= iy && y <= iy + ih {
                self.toggle_control_center();
                return;
            }
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

            self.paint.end_stroke();
            self.handle_window_click(&win, x - win.x, y - win.y, canvas_w, canvas_h);
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
        self.resizing = None;
        self.paint.end_stroke();

        if let Some((id, _, _)) = self.dragging.take()
            && let Some(zone) = self.snap_zone.take()
        {
            let (cw, ch) = self.last_canvas_size;
            let (sx, sy, sw, sh) = self.snap_rect(zone, cw, ch);
            let lang = self.settings.lang;
            let accent = self.settings.active_accent();
            if let Some(win) = self.windows.iter_mut().find(|w| w.id == id) {
                if !win.maximized {
                    win.saved_rect = (win.x, win.y, win.w, win.h);
                }
                win.x = sx;
                win.y = sy;
                win.w = sw;
                win.h = sh;
                win.maximized = zone == SnapZone::Maximize;
                if !self.settings.animations {
                    win.sync_vis_rect();
                }
            }
            let msg = match zone {
                SnapZone::Left => tr(lang, "Приклеено влево", "Snapped Left"),
                SnapZone::Right => tr(lang, "Приклеено вправо", "Snapped Right"),
                SnapZone::Maximize => tr(lang, "Окно развернуто", "Window Maximized"),
            };
            self.push_toast(msg, accent);
        }
        self.snap_zone = None;
    }

    fn handle_window_click(&mut self, win: &Window, lx: i32, ly: i32, canvas_w: i32, canvas_h: i32) {
        let lang = self.settings.lang;
        match win.id {
            AppId::System => {
                if ly >= win.h - 36 && ly <= win.h - 10 && (14..=124).contains(&lx) {
                    self.refresh_metrics();
                    let msg = String::from(tr(lang, "Метрики обновлены", "Metrics refreshed"));
                    self.status = msg.clone();
                    self.push_toast(msg, 0x3FB950);
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
            AppId::Paint => match self.paint.on_click(lx, ly, win.h) {
                PaintAction::None => {}
                PaintAction::Saved(path) => {
                    self.status = format!("BMP: {path}");
                    self.push_toast(
                        format!("{}: {path}", tr(lang, "Сохранено", "Saved")),
                        0xBC8CFF,
                    );
                }
                PaintAction::SaveErr(err) => {
                    self.status = err.clone();
                    self.push_toast(err, 0xF85149);
                }
            },
            AppId::Snake => self.snake.on_click(lx, ly, win.h),
            AppId::Notepad => {
                if let Some(msg) = self.notepad.on_click(lx, ly, win.h, lang) {
                    self.status = msg.clone();
                    self.push_toast(msg, 0x58A6FF);
                }
            }
            AppId::Calc => self.calc.on_click(lx, ly, win.w, win.h),
            AppId::Matrix => {
                if ly >= win.h - 32 && ly <= win.h - 8 && (12..=142).contains(&lx) {
                    self.matrix.cycle_color();
                }
            }
            AppId::Settings => {
                let old_scale = self.settings.ui_scale();
                let prev_dock = self.settings.floating_dock;
                let action = self.settings.on_click(lx, ly, win.w, win.h);
                if self.settings.floating_dock != prev_dock {
                    self.on_viewport_resize(canvas_w, canvas_h);
                }
                if (self.settings.ui_scale() - old_scale).abs() > 0.001 {
                    self.rescale_viewport(old_scale, canvas_w, canvas_h);
                    self.push_toast(
                        format!(
                            "{}: {}%",
                            tr(lang, "Масштаб UI", "UI Scale"),
                            self.settings.ui_scale_pct
                        ),
                        self.settings.active_accent(),
                    );
                }
                match action {
                    SettingsAction::None => {}
                    SettingsAction::RefreshMetrics => {
                        self.refresh_metrics();
                        self.push_toast(
                            tr(lang, "Метрики обновлены", "Metrics refreshed"),
                            0x3FB950,
                        );
                    }
                    SettingsAction::OpenTerminal => self.focus_or_open(AppId::Terminal),
                    SettingsAction::OpenPizGet => self.focus_or_open(AppId::PizGet),
                }
            }
            AppId::Terminal => {}
        }
    }

    pub fn render(&self, canvas: &mut Canvas) {
        canvas.set_ui_scale(self.settings.ui_scale());
        let w = canvas.logical_width();
        let h = canvas.logical_height();
        let lang = self.settings.lang;
        let accent = self.settings.active_accent();
        let (glass_a, border_a) = self.settings.glass_alphas();
        let anim = self.settings.animations;

        // 1. Cached procedural gradient + mountain wallpaper (native physical resolution)
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

        // 2. Desktop Icons (with smooth spring hover lift)
        if self.settings.show_icons {
            for (idx, &(id, ix, iy, col)) in DESKTOP_ICONS.iter().enumerate() {
                self.draw_desktop_icon(canvas, idx, ix, iy, id, id.title(lang), col);
            }
        }

        // 2b. Window Snap Zone Preview (below active windows)
        if let Some(zone) = self.snap_zone.or(if anim && self.snap_anim > 0.02 {
            self.last_snap
        } else {
            None
        }) {
            let t = if anim {
                Canvas::ease_out_cubic(self.snap_anim)
            } else {
                1.0
            };
            self.draw_snap_preview(canvas, w, h, zone, t, accent);
        }

        // 3. Visible & Animating Windows (non-linear Genie / Spring Morph / Back Pop-In)
        let top_vis = self
            .windows
            .iter()
            .enumerate()
            .rev()
            .find(|(_, win)| win.open && !win.minimized)
            .map(|(i, _)| i);

        for (idx, win) in self.windows.iter().enumerate() {
            let focused = Some(idx) == top_vis;
            if !win.open {
                if anim && win.close_anim > 0.01 {
                    let s = Canvas::ease_out_cubic(win.close_anim);
                    let scale = 0.88 + 0.12 * s;
                    let alpha = (s * 255.0).clamp(0.0, 255.0) as u8;
                    let vw = win.vis_rect[2].round().max(80.0) as i32;
                    let vh = win.vis_rect[3].round().max(60.0) as i32;
                    let mut cap = self.anim_canvas.borrow_mut();
                    cap.set_ui_scale(canvas.ui_scale);
                    let cap_pw = ((vw as f32) * canvas.ui_scale).round().max(2.0) as usize;
                    let cap_ph = ((vh as f32) * canvas.ui_scale).round().max(2.0) as usize;
                    cap.resize(cap_pw, cap_ph);
                    cap.pixels.fill(0xFF0B_0E16);
                    let mut cap_win = win.clone();
                    cap_win.x = 0;
                    cap_win.y = 0;
                    cap_win.w = vw;
                    cap_win.h = vh;
                    self.draw_window_frame(&mut cap, &cap_win, focused, false);
                    let cx = win.vis_rect[0] + win.vis_rect[2] * 0.5;
                    let cy = win.vis_rect[1] + win.vis_rect[3] * 0.5;
                    canvas.blit_scaled_alpha_rect(
                        &cap.pixels,
                        (cap_pw as i32, cap_ph as i32),
                        (cx, cy),
                        scale,
                        alpha,
                    );
                }
                continue;
            }

            if win.minimized && (!anim || win.min_anim <= 0.01) {
                continue;
            }

            let is_genie_or_scale = anim && win.min_anim < 0.999;
            let is_fresh_open = anim && !win.minimized && win.open_anim < 0.999;
            let vx = win.vis_rect[0].round() as i32;
            let vy = win.vis_rect[1].round() as i32;
            let vw = win.vis_rect[2].round().max(80.0) as i32;
            let vh = win.vis_rect[3].round().max(60.0) as i32;

            if is_genie_or_scale || is_fresh_open {
                let mut cap = self.anim_canvas.borrow_mut();
                cap.set_ui_scale(canvas.ui_scale);
                let cap_pw = ((vw as f32) * canvas.ui_scale).round().max(2.0) as usize;
                let cap_ph = ((vh as f32) * canvas.ui_scale).round().max(2.0) as usize;
                cap.resize(cap_pw, cap_ph);
                cap.pixels.fill(0xFF0B_0E16);
                let mut cap_win = win.clone();
                cap_win.x = 0;
                cap_win.y = 0;
                cap_win.w = vw;
                cap_win.h = vh;
                self.draw_window_frame(&mut cap, &cap_win, focused, false);

                if is_genie_or_scale {
                    let pill_rect = self.taskbar_pill_rect(w, h, win.id);
                    if self.settings.genie_minimize {
                        canvas.blit_genie_rect(
                            &cap.pixels,
                            (cap_pw as i32, cap_ph as i32),
                            (vx, vy),
                            pill_rect,
                            win.min_anim,
                            self.settings.taskbar_top,
                        );
                    } else {
                        let s = Canvas::ease_in_out_cubic(win.min_anim);
                        let win_cx = vx as f32 + vw as f32 * 0.5;
                        let win_cy = vy as f32 + vh as f32 * 0.5;
                        let pill_cx = pill_rect.0 as f32 + pill_rect.2 as f32 * 0.5;
                        let pill_cy = pill_rect.1 as f32 + 12.0;
                        let cx = pill_cx + (win_cx - pill_cx) * s;
                        let cy = pill_cy + (win_cy - pill_cy) * s;
                        let scale = 0.14 + 0.86 * s;
                        let alpha = (s * 255.0).clamp(0.0, 255.0) as u8;
                        canvas.blit_scaled_alpha_rect(
                            &cap.pixels,
                            (cap_pw as i32, cap_ph as i32),
                            (cx, cy),
                            scale,
                            alpha,
                        );
                    }
                } else {
                    let s_pos = Canvas::ease_out_back(win.open_anim);
                    let s_op = Canvas::ease_out_cubic(win.open_anim);
                    let scale = 0.92 + 0.08 * s_pos;
                    let alpha = (s_op * 255.0).clamp(0.0, 255.0) as u8;
                    let cx = vx as f32 + vw as f32 * 0.5;
                    let cy = vy as f32 + vh as f32 * 0.5 + (1.0 - s_op) * 12.0;
                    canvas.blit_scaled_alpha_rect(
                        &cap.pixels,
                        (cap_pw as i32, cap_ph as i32),
                        (cx, cy),
                        scale,
                        alpha,
                    );
                }
            } else {
                let mut vis_win = win.clone();
                vis_win.x = vx;
                vis_win.y = vy;
                vis_win.w = vw;
                vis_win.h = vh;
                self.draw_window_frame(canvas, &vis_win, focused, true);
            }
        }

        // 4. ColorOS Minimalist Frosted Taskbar or Animated Floating Dock
        let (dock_x, task_y, dock_w, dock_h, floating) = self.taskbar_rect(w, h);
        let dock_t = Canvas::ease_out_cubic(self.dock_float_anim.clamp(0.0, 1.0));
        if floating {
            let r = (14.0 * dock_t).round() as i32;
            if dock_t > 0.15 {
                canvas.draw_soft_shadow(dock_x, task_y, dock_w, dock_h, r);
            }
            canvas.blend_rounded_rect(dock_x, task_y, dock_w, dock_h, r, 0x0B0E16, glass_a);
            canvas.draw_rounded_rect_alpha(
                dock_x,
                task_y,
                dock_w,
                dock_h,
                r,
                0xFFFFFF,
                (border_a as f32 * dock_t).round() as u8,
            );
            canvas.blend_rect(
                dock_x + r + 2,
                task_y + 1,
                (dock_w - (r + 2) * 2).max(0),
                1,
                0xFFFFFF,
                (28.0 * dock_t).round() as u8,
            );
        } else {
            canvas.blend_rect(0, task_y, w, dock_h, 0x0B0E16, glass_a);
            let rim_y = if self.settings.taskbar_top { task_y + 35 } else { task_y };
            canvas.blend_rect(0, rim_y, w, 1, 0xFFFFFF, border_a.min(52));
        }

        let start_x = dock_x + 8;
        let start_hovered = self.mouse_x >= start_x
            && self.mouse_x <= start_x + 86
            && self.mouse_y >= task_y + 6
            && self.mouse_y <= task_y + 30;
        if self.start_menu_open {
            canvas.blend_rounded_rect(start_x, task_y + 6, 86, 24, 8, accent, 58);
            canvas.draw_rounded_rect_alpha(start_x, task_y + 6, 86, 24, 8, accent, 125);
        } else {
            let start_alpha = if start_hovered { 28 } else { 15 };
            canvas.blend_rounded_rect(start_x, task_y + 6, 86, 24, 8, 0xFFFFFF, start_alpha);
            canvas.draw_rounded_rect_alpha(start_x, task_y + 6, 86, 24, 8, 0xFFFFFF, 28);
        }
        canvas.fill_aa_circle((start_x + 11) as f32, (task_y + 15) as f32, 1.7, accent, 255);
        canvas.fill_aa_circle((start_x + 17) as f32, (task_y + 15) as f32, 1.7, 0xE2E8F0, 220);
        canvas.fill_aa_circle((start_x + 11) as f32, (task_y + 21) as f32, 1.7, 0xE2E8F0, 220);
        canvas.fill_aa_circle((start_x + 17) as f32, (task_y + 21) as f32, 1.7, 0x94A3B8, 220);
        canvas.draw_text_clipped(start_x + 26, task_y + 11, 56, tr(lang, "Пуск", "Start"), 0xF1F5F9, 1);

        let open_wins: Vec<(usize, &Window)> = self
            .windows
            .iter()
            .enumerate()
            .filter(|(_, win)| win.open)
            .collect();
        let pill_w = self.taskbar_pill_width(dock_w, open_wins.len());
        let mut bx = dock_x + 102;
        for (orig_idx, win) in open_wins {
            let is_top = Some(orig_idx) == top_vis;
            let pill_hovered = self.mouse_x >= bx
                && self.mouse_x <= bx + pill_w
                && self.mouse_y >= task_y + 6
                && self.mouse_y <= task_y + 30;
            let genie_pulse = if anim && win.min_anim > 0.02 && win.min_anim < 0.98 {
                ((1.0 - (win.min_anim - 0.5).abs() * 2.0) * 70.0) as u8
            } else {
                0
            };
            if is_top {
                canvas.blend_rounded_rect(bx, task_y + 6, pill_w, 24, 8, 0xFFFFFF, 28);
                canvas.draw_rounded_rect_alpha(bx, task_y + 6, pill_w, 24, 8, 0xFFFFFF, 42);
                canvas.blend_rect(bx + 10, task_y + 7, (pill_w - 20).max(0), 1, 0xFFFFFF, 26);
            } else if win.minimized {
                let a = if pill_hovered { 16 } else { 7 };
                canvas.blend_rounded_rect(bx, task_y + 6, pill_w, 24, 8, 0xFFFFFF, a);
            } else {
                let a = if pill_hovered { 20 } else { 12 };
                canvas.blend_rounded_rect(bx, task_y + 6, pill_w, 24, 8, 0xFFFFFF, a);
                canvas.draw_rounded_rect_alpha(bx, task_y + 6, pill_w, 24, 8, 0xFFFFFF, 18);
            }
            if genie_pulse > 0 {
                canvas.blend_rounded_rect(bx, task_y + 6, pill_w, 24, 8, accent, genie_pulse);
                canvas.draw_rounded_rect_alpha(bx, task_y + 6, pill_w, 24, 8, accent, (genie_pulse * 2).min(180));
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

        let lang_x = (dock_x + dock_w - 144).max(bx + 8);
        // Spotlight Search quick pill
        let search_x = lang_x - 60;
        let search_hov = self.mouse_x >= search_x
            && self.mouse_x <= search_x + 26
            && self.mouse_y >= task_y + 6
            && self.mouse_y <= task_y + 30;
        if self.launcher_open {
            canvas.blend_rounded_rect(search_x, task_y + 6, 26, 24, 7, accent, 70);
            canvas.draw_rounded_rect_alpha(search_x, task_y + 6, 26, 24, 7, accent, 150);
        } else {
            canvas.blend_rounded_rect(search_x, task_y + 6, 26, 24, 7, 0xFFFFFF, if search_hov { 26 } else { 13 });
            canvas.draw_rounded_rect_alpha(search_x, task_y + 6, 26, 24, 7, 0xFFFFFF, 24);
        }
        let scx = (search_x + 12) as f32;
        let scy = (task_y + 16) as f32;
        canvas.fill_aa_circle(scx, scy, 4.2, 0xCBD5E1, 210);
        canvas.fill_aa_circle(scx, scy, 2.4, 0x101520, 255);
        canvas.draw_aa_line(scx + 2.4, scy + 2.4, scx + 5.6, scy + 5.6, 1.5, 0xE2E8F0, 235);

        // Control Center quick pill (two ColorOS micro-toggles)
        let cc_x = lang_x - 30;
        let cc_hov = self.mouse_x >= cc_x
            && self.mouse_x <= cc_x + 26
            && self.mouse_y >= task_y + 6
            && self.mouse_y <= task_y + 30;
        if self.cc_open {
            canvas.blend_rounded_rect(cc_x, task_y + 6, 26, 24, 7, accent, 70);
            canvas.draw_rounded_rect_alpha(cc_x, task_y + 6, 26, 24, 7, accent, 150);
        } else {
            canvas.blend_rounded_rect(cc_x, task_y + 6, 26, 24, 7, 0xFFFFFF, if cc_hov { 26 } else { 13 });
            canvas.draw_rounded_rect_alpha(cc_x, task_y + 6, 26, 24, 7, 0xFFFFFF, 24);
        }
        canvas.draw_aa_line((cc_x + 7) as f32, (task_y + 15) as f32, (cc_x + 19) as f32, (task_y + 15) as f32, 1.3, 0xCBD5E1, 210);
        canvas.fill_aa_circle((cc_x + 16) as f32, (task_y + 15) as f32, 2.1, accent, 255);
        canvas.draw_aa_line((cc_x + 7) as f32, (task_y + 21) as f32, (cc_x + 19) as f32, (task_y + 21) as f32, 1.3, 0x94A3B8, 210);
        canvas.fill_aa_circle((cc_x + 10) as f32, (task_y + 21) as f32, 2.1, 0xE2E8F0, 235);

        let lang_lbl = match lang {
            Lang::Ru => "RU",
            Lang::En => "EN",
        };
        canvas.blend_rounded_rect(lang_x, task_y + 6, 36, 24, 7, 0xFFFFFF, 15);
        canvas.draw_rounded_rect_alpha(lang_x, task_y + 6, 36, 24, 7, 0xFFFFFF, 26);
        canvas.draw_text_clipped(lang_x + 10, task_y + 11, 26, lang_lbl, 0xE2E8F0, 1);

        let tray_x = lang_x + 40;
        let tray_w = (dock_x + dock_w - 6 - tray_x).max(40);
        canvas.blend_rounded_rect(tray_x, task_y + 6, tray_w, 24, 7, 0xFFFFFF, 11);
        canvas.draw_rounded_rect_alpha(tray_x, task_y + 6, tray_w, 24, 7, 0xFFFFFF, 20);
        let pulse_a = if anim {
            (205.0 + 50.0 * ((self.tick as f32) * 0.08).sin()).clamp(150.0, 255.0) as u8
        } else {
            255
        };
        canvas.fill_aa_circle((lang_x + 49) as f32, (task_y + 18) as f32, 2.5, 0x3FB950, pulse_a);
        let tray_txt = if self.settings.show_clock {
            &self.clock_str
        } else {
            &self.uptime
        };
        canvas.draw_text_clipped(
            lang_x + 58,
            task_y + 11,
            (tray_w - 22).max(20),
            tray_txt,
            0xCBD5E1,
            1,
        );

        // 5. Overlays: Start Menu, Control Center, Spotlight Launcher, Context Menu, Dynamic Island Toast
        if self.start_menu_open || (anim && self.start_anim > 0.03) {
            self.draw_start_menu(canvas, dock_x, task_y, dock_h, accent);
        }
        if self.cc_open || (anim && self.cc_anim > 0.03) {
            self.draw_control_center(canvas, w, h, accent);
        }
        if self.launcher_open || (anim && self.launcher_anim > 0.03) {
            self.draw_launcher(canvas, w, h, accent);
        }
        let anim_ctx = if anim && self.ctx_anim > 0.04 {
            self.last_ctx_menu
        } else {
            None
        };
        if let Some((cx, cy)) = self.ctx_menu.or(anim_ctx) {
            self.draw_context_menu(canvas, cx, cy, accent);
        }
        if self.settings.persistent_island || (anim && self.island_anim > 0.03) {
            self.draw_idle_island(canvas, w, h, accent);
        }
        if let Some(toast) = self.toast.as_ref() {
            self.draw_toast(canvas, w, h, toast);
        }

        // 6. Expanding AA click ripples
        if anim {
            for r in &self.ripples {
                let s = Canvas::ease_out_cubic(r.t);
                let rad = 3.5 + s * 17.0;
                let glow_a = ((1.0 - s) * 42.0).clamp(0.0, 255.0) as u8;
                let ring_a = ((1.0 - s) * 145.0).clamp(0.0, 255.0) as u8;
                if glow_a > 1 {
                    canvas.fill_aa_circle(r.x as f32, r.y as f32, rad, r.color, glow_a);
                    canvas.fill_aa_circle(r.x as f32, r.y as f32, (rad - 2.0).max(1.0), 0xFFFFFF, ring_a / 3);
                }
            }
        }

        // 7. Mouse cursor (morphs to crisp AA resize arrows over edges/corners)
        if self.settings.crosshair_cursor {
            canvas.fill_rect(self.mouse_x - 6, self.mouse_y, 13, 1, 0xFFFFFF);
            canvas.fill_rect(self.mouse_x, self.mouse_y - 6, 1, 13, 0xFFFFFF);
        } else if let Some(rz) = self.resizing {
            let (ux, uy) = rz.edge.axis();
            canvas.draw_resize_cursor(self.mouse_x, self.mouse_y, ux, uy);
        } else {
            let over_dock = self.mouse_x >= dock_x
                && self.mouse_x < dock_x + dock_w
                && self.mouse_y >= task_y
                && self.mouse_y < task_y + dock_h;
            let mut hover_edge = ResizeEdge::none();
            if self.ctx_menu.is_none()
                && !self.start_menu_open
                && !self.cc_open
                && !self.launcher_open
                && !over_dock
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

    fn draw_snap_preview(
        &self,
        canvas: &mut Canvas,
        w: i32,
        h: i32,
        zone: SnapZone,
        t: f32,
        accent: u32,
    ) {
        if t <= 0.02 {
            return;
        }
        let (sx, sy, sw, sh) = self.snap_rect(zone, w, h);
        canvas.blend_rounded_rect(
            sx + 2,
            sy + 2,
            (sw - 4).max(8),
            (sh - 4).max(8),
            13,
            accent,
            (46.0 * t).clamp(0.0, 255.0) as u8,
        );
        canvas.draw_rounded_rect_alpha(
            sx + 2,
            sy + 2,
            (sw - 4).max(8),
            (sh - 4).max(8),
            13,
            accent,
            (185.0 * t).clamp(0.0, 255.0) as u8,
        );
        canvas.blend_rect(
            sx + 16,
            sy + 4,
            (sw - 32).max(0),
            1,
            0xFFFFFF,
            (65.0 * t).clamp(0.0, 255.0) as u8,
        );
    }

    fn draw_control_center(&self, canvas: &mut Canvas, w: i32, h: i32, accent: u32) {
        let lang = self.settings.lang;
        let (glass_a, border_a) = self.settings.glass_alphas();
        let raw_t = if self.settings.animations {
            self.cc_anim.clamp(0.0, 1.05)
        } else {
            1.0
        };
        let t = raw_t.clamp(0.0, 1.0);
        let (px, base_y, pw, ph) = self.cc_rect(w, h);
        let slide = ((1.0 - raw_t) * 18.0).round() as i32;
        let py = if self.settings.taskbar_top {
            base_y - slide
        } else {
            base_y + slide
        };

        if t > 0.65 {
            canvas.draw_soft_shadow(px, py, pw, ph, 14);
        }
        let panel_a = (((glass_a + 18).min(248) as f32) * t) as u8;
        let stroke_a = ((border_a as f32) * t) as u8;
        canvas.blend_rounded_rect(px, py, pw, ph, 14, 0x0E131E, panel_a);
        canvas.draw_rounded_rect_alpha(px, py, pw, ph, 14, 0xFFFFFF, stroke_a);
        canvas.blend_rect(px + 18, py + 1, pw - 36, 1, 0xFFFFFF, (32.0 * t) as u8);
        if t < 0.25 {
            return;
        }

        canvas.fill_aa_circle((px + 15) as f32, (py + 18) as f32, 3.0, accent, 255);
        canvas.draw_text_clipped(
            px + 24,
            py + 11,
            180,
            tr(lang, "Центр управления", "Control Center"),
            0xF8FAFC,
            1,
        );
        let scale_tag = format!("{}%", self.settings.ui_scale_pct);
        canvas.blend_rounded_rect(px + pw - 54, py + 9, 44, 18, 6, accent, 48);
        canvas.draw_text_clipped(px + pw - 48, py + 12, 36, &scale_tag, 0xE2E8F0, 1);

        let tiles = [
            (0, 0, tr(lang, "Док", "Dock"), self.settings.floating_dock, 0xA855F7),
            (1, 0, tr(lang, "Движение", "Motion"), self.settings.animations, accent),
            (0, 1, tr(lang, "Джин", "Genie"), self.settings.genie_minimize, 0x2DD4BF),
            (1, 1, tr(lang, "Сетка", "Grid"), self.settings.show_grid, 0xF59E0B),
        ];
        for (col, row, title, active, col_dot) in tiles {
            let tx = px + 10 + col * 132;
            let ty = py + 36 + row * 48;
            let hov = self.cc_open
                && self.mouse_x >= tx
                && self.mouse_x <= tx + 124
                && self.mouse_y >= ty
                && self.mouse_y <= ty + 42;
            if active {
                canvas.blend_rounded_rect(tx, ty, 124, 42, 11, col_dot, if hov { 62 } else { 48 });
                canvas.draw_rounded_rect_alpha(tx, ty, 124, 42, 11, col_dot, 130);
                canvas.fill_aa_circle((tx + 18) as f32, (ty + 21) as f32, 10.0, col_dot, 220);
                canvas.fill_aa_circle((tx + 18) as f32, (ty + 21) as f32, 4.0, 0xFFFFFF, 240);
            } else {
                canvas.blend_rounded_rect(tx, ty, 124, 42, 11, 0xFFFFFF, if hov { 20 } else { 12 });
                canvas.draw_rounded_rect_alpha(tx, ty, 124, 42, 11, 0xFFFFFF, 26);
                canvas.fill_aa_circle((tx + 18) as f32, (ty + 21) as f32, 10.0, 0xFFFFFF, 26);
                canvas.fill_aa_circle((tx + 18) as f32, (ty + 21) as f32, 3.8, 0x94A3B8, 200);
            }
            canvas.draw_text_clipped(tx + 34, ty + 8, 84, title, 0xF1F5F9, 1);
            canvas.draw_text_clipped(
                tx + 34,
                ty + 23,
                84,
                if active {
                    tr(lang, "Вкл", "Active")
                } else {
                    tr(lang, "Выкл", "Off")
                },
                if active { 0xE2E8F0 } else { 0x94A3B8 },
                1,
            );
        }

        let glass_y = py + 136;
        let glass_names = [
            tr(lang, "Лёгкое", "Clear"),
            tr(lang, "Мороз", "Frost"),
            tr(lang, "Глубокое", "Blur+"),
        ];
        for (idx, lbl) in glass_names.iter().enumerate() {
            let gx = px + 10 + (idx as i32) * 87;
            let sel = self.settings.glass_level == idx as u8;
            if sel {
                canvas.blend_rounded_rect(gx, glass_y, 82, 26, 8, accent, 72);
                canvas.draw_rounded_rect_alpha(gx, glass_y, 82, 26, 8, accent, 165);
            } else {
                canvas.blend_rounded_rect(gx, glass_y, 82, 26, 8, 0xFFFFFF, 12);
                canvas.draw_rounded_rect_alpha(gx, glass_y, 82, 26, 8, 0xFFFFFF, 22);
            }
            canvas.draw_text_clipped(gx + 8, glass_y + 7, 68, lbl, if sel { 0xFFFFFF } else { 0xCBD5E1 }, 1);
        }

        let scale_y = py + 170;
        for (idx, &pct) in UI_SCALES.iter().enumerate() {
            let sx = px + 10 + (idx as i32) * 65;
            let sel = self.settings.ui_scale_pct == pct;
            if sel {
                canvas.blend_rounded_rect(sx, scale_y, 61, 26, 8, accent, 78);
                canvas.draw_rounded_rect_alpha(sx, scale_y, 61, 26, 8, accent, 170);
            } else {
                canvas.blend_rounded_rect(sx, scale_y, 61, 26, 8, 0xFFFFFF, 12);
                canvas.draw_rounded_rect_alpha(sx, scale_y, 61, 26, 8, 0xFFFFFF, 22);
            }
            let txt = format!("{pct}%");
            canvas.draw_text_clipped(sx + 10, scale_y + 7, 45, &txt, if sel { 0xFFFFFF } else { 0xCBD5E1 }, 1);
        }

        let island_y = py + 202;
        let island_on = self.settings.persistent_island;
        let island_hov = self.cc_open
            && self.mouse_x >= px + 10
            && self.mouse_x <= px + pw - 10
            && self.mouse_y >= island_y
            && self.mouse_y <= island_y + 26;
        if island_on {
            canvas.blend_rounded_rect(px + 10, island_y, pw - 20, 26, 8, 0x10B981, if island_hov { 68 } else { 52 });
            canvas.draw_rounded_rect_alpha(px + 10, island_y, pw - 20, 26, 8, 0x10B981, 155);
        } else {
            canvas.blend_rounded_rect(px + 10, island_y, pw - 20, 26, 8, 0xFFFFFF, if island_hov { 20 } else { 12 });
            canvas.draw_rounded_rect_alpha(px + 10, island_y, pw - 20, 26, 8, 0xFFFFFF, 24);
        }
        canvas.fill_aa_circle((px + 22) as f32, (island_y + 13) as f32, 3.2, if island_on { 0x34D399 } else { 0x64748B }, 255);
        canvas.draw_text_clipped(
            px + 30,
            island_y + 6,
            pw - 96,
            tr(lang, "Dynamic Island всегда", "Always-On Island"),
            0xF1F5F9,
            1,
        );
        let st_col = if island_on { 0x34D399 } else { 0x94A3B8 };
        canvas.draw_text_clipped(
            px + pw - 62,
            island_y + 6,
            46,
            if island_on { tr(lang, "Вкл", "On") } else { tr(lang, "Выкл", "Off") },
            st_col,
            1,
        );

        let foot_y = py + 234;
        canvas.blend_rounded_rect(px + 10, foot_y, pw - 20, 24, 8, 0xFFFFFF, 11);
        canvas.draw_rounded_rect_alpha(px + 10, foot_y, pw - 20, 24, 8, 0xFFFFFF, 22);
        canvas.fill_aa_circle((px + 22) as f32, (foot_y + 12) as f32, 3.0, 0x3FB950, 255);
        canvas.draw_text_clipped(
            px + 30,
            foot_y + 5,
            pw - 48,
            &format!("CPU {}%   RAM {}%", self.cpu_pct, self.mem.percent),
            0xE2E8F0,
            1,
        );
    }

    fn draw_launcher(&self, canvas: &mut Canvas, w: i32, h: i32, accent: u32) {
        let lang = self.settings.lang;
        let (glass_a, border_a) = self.settings.glass_alphas();
        let raw_t = if self.settings.animations {
            self.launcher_anim
        } else {
            1.0
        };
        let t = raw_t.clamp(0.0, 1.0);
        if t <= 0.02 {
            return;
        }
        // Subtle spotlight backdrop dim
        canvas.blend_rect(0, 0, w, h, 0x05080F, (88.0 * t) as u8);

        let (lx, base_y, lw, lh) = self.launcher_rect(w, h);
        let ly = base_y + ((1.0 - raw_t) * 18.0).round() as i32;
        if t > 0.55 {
            canvas.draw_soft_shadow(lx, ly, lw, lh, 16);
        }
        let panel_a = (((glass_a + 24).min(250) as f32) * t) as u8;
        let stroke_a = ((border_a.saturating_add(16) as f32) * t) as u8;
        canvas.blend_rounded_rect(lx, ly, lw, lh, 16, 0x0C111C, panel_a);
        canvas.draw_rounded_rect_alpha(lx, ly, lw, lh, 16, 0xFFFFFF, stroke_a);
        canvas.blend_rect(lx + 20, ly + 1, lw - 40, 1, 0xFFFFFF, (42.0 * t) as u8);
        if t < 0.25 {
            return;
        }

        // Search box
        let sx = lx + 12;
        let sy = ly + 12;
        let sw = lw - 24;
        canvas.blend_rounded_rect(sx, sy, sw, 32, 10, 0xFFFFFF, 16);
        canvas.draw_rounded_rect_alpha(sx, sy, sw, 32, 10, accent, 115);
        canvas.fill_aa_circle((sx + 16) as f32, (sy + 15) as f32, 5.0, accent, 210);
        canvas.fill_aa_circle((sx + 16) as f32, (sy + 15) as f32, 2.9, 0x101624, 255);
        canvas.draw_aa_line((sx + 19) as f32, (sy + 18) as f32, (sx + 23) as f32, (sy + 22) as f32, 1.6, 0xE2E8F0, 235);

        if self.launcher_query.is_empty() {
            canvas.draw_text_clipped(
                sx + 32,
                sy + 9,
                sw - 46,
                tr(lang, "Поиск приложений, команд или 2+2...", "Search apps, actions or 2+2..."),
                0x8B949E,
                1,
            );
        } else {
            canvas.draw_text_clipped(sx + 32, sy + 9, sw - 46, &self.launcher_query, 0xF8FAFC, 1);
        }
        if self.launcher_open && (self.tick / 15).is_multiple_of(2) {
            let qlen = canvas.measure_text(&self.launcher_query, 1).min(sw - 50);
            canvas.fill_rounded_rect(sx + 33 + qlen, sy + 7, 2, 17, 1, accent);
        }

        let entries = self.launcher_entries();
        if entries.is_empty() {
            canvas.draw_text_clipped(
                lx + 24,
                ly + 82,
                lw - 48,
                tr(lang, "Ничего не найдено", "No matches found"),
                0x8B949E,
                1,
            );
        } else {
            for (i, item) in entries.iter().take(7).enumerate() {
                let ry = ly + 52 + (i as i32) * 28;
                let hov = self.launcher_open
                    && self.mouse_x >= lx + 10
                    && self.mouse_x <= lx + lw - 10
                    && self.mouse_y >= ry
                    && self.mouse_y < ry + 26;
                let sel = i == self.launcher_sel || hov;
                if sel {
                    canvas.blend_rounded_rect(lx + 10, ry, lw - 20, 26, 8, accent, 52);
                    canvas.draw_rounded_rect_alpha(lx + 10, ry, lw - 20, 26, 8, accent, 120);
                    canvas.fill_rounded_rect(lx + 10, ry + 5, 3, 16, 1, accent);
                }
                canvas.fill_aa_circle((lx + 26) as f32, (ry + 13) as f32, 3.6, item.color, 255);
                canvas.draw_text_clipped(
                    lx + 38,
                    ry + 6,
                    lw - 134,
                    &item.label,
                    if sel { 0xF8FAFC } else { 0xCBD5E1 },
                    1,
                );
                canvas.blend_rounded_rect(lx + lw - 92, ry + 4, 72, 18, 6, 0xFFFFFF, 15);
                canvas.draw_text_clipped(lx + lw - 86, ry + 6, 62, item.badge, 0x94A3B8, 1);
            }
        }

        canvas.blend_rect(lx + 14, ly + lh - 28, lw - 28, 1, 0xFFFFFF, 22);
        canvas.draw_text_clipped(
            lx + 18,
            ly + lh - 21,
            lw - 36,
            tr(lang, "Alt+Space: поиск  |  Enter: запуск  |  Esc: закрыть", "↑↓ Select   Enter Run   Esc Close"),
            0x7D8590,
            1,
        );
    }

    fn draw_idle_island(&self, canvas: &mut Canvas, w: i32, h: i32, accent: u32) {
        let t = if self.settings.animations {
            self.island_anim.clamp(0.0, 1.0)
        } else if self.settings.persistent_island {
            1.0
        } else {
            0.0
        };
        if t <= 0.02 {
            return;
        }
        let txt = self.idle_island_text();
        let (tx, base_y, tw, th) = self.idle_island_rect(w, h, &txt);
        let slide = ((1.0 - t) * 26.0).round() as i32;
        let ty = if self.settings.taskbar_top {
            base_y + slide
        } else {
            base_y - slide
        };
        let hov = self.settings.persistent_island
            && self.toast.is_none()
            && self.mouse_x >= tx
            && self.mouse_x <= tx + tw
            && self.mouse_y >= ty
            && self.mouse_y <= ty + th;

        if t > 0.5 {
            canvas.draw_soft_shadow(tx, ty, tw, th, 13);
        }
        canvas.blend_rounded_rect(tx, ty, tw, th, 13, 0x090D16, (236.0 * t) as u8);
        let border_col = if hov { accent } else { 0xFFFFFF };
        let border_a = if hov {
            (165.0 * t) as u8
        } else {
            (48.0 * t) as u8
        };
        canvas.draw_rounded_rect_alpha(tx, ty, tw, th, 13, border_col, border_a);
        canvas.blend_rect(tx + 16, ty + 1, (tw - 32).max(0), 1, 0xFFFFFF, (30.0 * t) as u8);

        let pulse = if self.settings.animations {
            (205.0 + 50.0 * ((self.tick as f32) * 0.08).sin()).clamp(150.0, 255.0)
        } else {
            255.0
        };
        canvas.fill_aa_circle((tx + 14) as f32, (ty + 13) as f32, 4.8, 0x3FB950, (55.0 * t) as u8);
        canvas.fill_aa_circle((tx + 14) as f32, (ty + 13) as f32, 2.7, 0x3FB950, (pulse * t) as u8);
        if t > 0.35 {
            canvas.draw_text_clipped(tx + 25, ty + 6, tw - 48, &txt, 0xE2E8F0, 1);
            canvas.fill_aa_circle((tx + tw - 12) as f32, (ty + 13) as f32, 2.4, accent, (220.0 * t) as u8);
        }
    }

    fn draw_toast(&self, canvas: &mut Canvas, w: i32, h: i32, toast: &Toast) {
        let t = toast.anim.clamp(0.0, 1.0);
        if t <= 0.02 {
            return;
        }
        let tw = (canvas.measure_text(&toast.text, 1) + 42).clamp(132, (w - 32).max(132));
        let th = 28;
        let tx = (w - tw) / 2;
        let ty = if self.settings.taskbar_top {
            h - 42 + ((1.0 - t) * 28.0).round() as i32
        } else {
            10 - ((1.0 - t) * 28.0).round() as i32
        };

        if t > 0.5 {
            canvas.draw_soft_shadow(tx, ty, tw, th, 14);
        }
        canvas.blend_rounded_rect(tx, ty, tw, th, 14, 0x090D16, (238.0 * t) as u8);
        canvas.draw_rounded_rect_alpha(tx, ty, tw, th, 14, toast.color, (130.0 * t) as u8);
        canvas.fill_aa_circle((tx + 15) as f32, (ty + 14) as f32, 3.6, toast.color, (255.0 * t) as u8);
        if t > 0.35 {
            canvas.draw_text_clipped(tx + 26, ty + 7, tw - 36, &toast.text, 0xF8FAFC, 1);
        }
        let frac = (toast.ttl as f32 / toast.max_ttl.max(1) as f32).clamp(0.0, 1.0);
        let prog_w = (((tw - 28) as f32) * frac).round() as i32;
        if prog_w > 2 {
            canvas.blend_rect(tx + 14, ty + th - 3, prog_w, 1, toast.color, (185.0 * t) as u8);
        }
    }

    fn draw_context_menu(&self, canvas: &mut Canvas, cx: i32, cy: i32, accent: u32) {
        let lang = self.settings.lang;
        let (glass_a, border_a) = self.settings.glass_alphas();
        let t = if self.settings.animations {
            Canvas::ease_out_cubic(self.ctx_anim)
        } else {
            1.0
        };
        let cy = cy + ((1.0 - t) * 8.0).round() as i32;
        let items = [
            (tr(lang, "Открыть Терминал", "Open Terminal"), 0x3FB950),
            (tr(lang, "Сменить обои", "Next Wallpaper"), accent),
            (tr(lang, "Настройки", "Settings"), 0x58A6FF),
            (tr(lang, "Обновить", "Refresh"), 0xD29922),
            (tr(lang, "Выход", "Exit"), 0xF85149),
        ];
        if t > 0.65 {
            canvas.draw_soft_shadow(cx, cy, 184, 134, 11);
        }
        let panel_a = (((glass_a + 14).min(248) as f32) * t) as u8;
        let stroke_a = ((border_a as f32) * t) as u8;
        canvas.blend_rounded_rect(cx, cy, 184, 134, 11, 0x101520, panel_a);
        canvas.draw_rounded_rect_alpha(cx, cy, 184, 134, 11, 0xFFFFFF, stroke_a);
        if t < 0.30 {
            return;
        }
        for (i, (label, dot_col)) in items.iter().enumerate() {
            let iy = cy + 4 + (i as i32) * 25;
            if i == 4 {
                canvas.blend_rect(cx + 10, iy - 1, 164, 1, 0xFFFFFF, 18);
            }
            if self.ctx_menu.is_some()
                && self.mouse_x >= cx
                && self.mouse_x <= cx + 184
                && self.mouse_y >= iy
                && self.mouse_y < iy + 23
            {
                canvas.blend_rounded_rect(cx + 4, iy, 176, 23, 7, accent, 52);
            }
            canvas.fill_aa_circle((cx + 14) as f32, (iy + 11) as f32, 2.7, *dot_col, 255);
            let col = if i == 4 { 0xFF7B72 } else { 0xE2E8F0 };
            canvas.draw_text_clipped(cx + 24, iy + 5, 150, label, col, 1);
        }
    }

    fn draw_start_menu(&self, canvas: &mut Canvas, dock_x: i32, task_y: i32, dock_h: i32, accent: u32) {
        let lang = self.settings.lang;
        let (glass_a, border_a) = self.settings.glass_alphas();
        let raw_t = if self.settings.animations {
            self.start_anim
        } else {
            1.0
        };
        let t = raw_t.clamp(0.0, 1.0);
        let menu_w = 228;
        let menu_h = (START_ITEMS.len() as i32) * 24 + 42;
        let menu_x = dock_x + 8;
        let slide = ((1.0 - raw_t) * 14.0).round() as i32;
        let menu_y = if self.settings.taskbar_top {
            task_y + dock_h + 2 - slide
        } else {
            task_y - menu_h - 4 + slide
        };

        let panel_a = (((glass_a + 16).min(248) as f32) * t) as u8;
        let stroke_a = ((border_a as f32) * t) as u8;
        if t > 0.7 {
            canvas.draw_soft_shadow(menu_x, menu_y, menu_w, menu_h, 13);
        }
        canvas.blend_rounded_rect(menu_x, menu_y, menu_w, menu_h, 13, 0x101520, panel_a);
        canvas.draw_rounded_rect_alpha(menu_x, menu_y, menu_w, menu_h, 13, 0xFFFFFF, stroke_a);

        if t < 0.28 {
            return;
        }
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
                canvas.blend_rounded_rect(menu_x + 6, iy, menu_w - 12, 22, 7, 0xFFFFFF, 22);
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

    #[allow(clippy::too_many_arguments)]
    fn draw_desktop_icon(
        &self,
        canvas: &mut Canvas,
        idx: usize,
        x: i32,
        y: i32,
        id: AppId,
        label: &str,
        accent: u32,
    ) {
        let (glass_a, border_a) = self.settings.glass_alphas();
        let hover_t = self.icon_hover.get(idx).copied().unwrap_or(0.0);
        let lift = if self.settings.animations {
            (hover_t * 3.0).round() as i32
        } else if hover_t > 0.5 {
            2
        } else {
            0
        };
        if hover_t > 0.03 {
            let bg_a = (hover_t * 24.0) as u8;
            let bd_a = (hover_t * 46.0) as u8;
            canvas.blend_rounded_rect(x - 2, y - 4, 72, 62, 11, 0xFFFFFF, bg_a);
            canvas.draw_rounded_rect_alpha(x - 2, y - 4, 72, 62, 11, 0xFFFFFF, bd_a);
        }

        // Cohesive ColorOS dark-glass squircle container + monoline vector glyphs
        let bx = x + 13;
        let by = y - lift;
        let cx = bx as f32 + 20.0;
        let cy = by as f32 + 20.0;

        canvas.draw_soft_shadow(bx, by, 40, 40, 12);
        canvas.blend_rounded_rect(bx, by, 40, 40, 12, 0x101522, (glass_a as i32 - 4).clamp(165, 248) as u8);
        canvas.fill_aa_circle(cx, cy, 13.5 + hover_t * 4.5, accent, 28 + (hover_t * 36.0) as u8);
        canvas.blend_rect(bx + 10, by + 1, 20, 1, 0xFFFFFF, 46);
        canvas.draw_rounded_rect_alpha(
            bx,
            by,
            40,
            40,
            12,
            0xFFFFFF,
            border_a.saturating_add((hover_t * 54.0) as u8),
        );

        match id {
            AppId::Terminal => {
                canvas.draw_aa_line(cx - 8.5, cy - 5.0, cx - 2.0, cy, 1.8, accent, 255);
                canvas.draw_aa_line(cx - 2.0, cy, cx - 8.5, cy + 5.0, 1.8, accent, 255);
                canvas.draw_aa_line(cx + 1.0, cy + 5.0, cx + 8.5, cy + 5.0, 1.8, 0xE2E8F0, 240);
            }
            AppId::PizGet => {
                canvas.draw_aa_line(cx, cy - 8.0, cx, cy + 3.0, 1.8, 0xFBBF24, 255);
                canvas.draw_aa_line(cx - 4.5, cy - 1.5, cx, cy + 3.0, 1.8, 0xFBBF24, 255);
                canvas.draw_aa_line(cx + 4.5, cy - 1.5, cx, cy + 3.0, 1.8, 0xFBBF24, 255);
                canvas.draw_aa_line(cx - 8.0, cy + 4.5, cx - 8.0, cy + 7.5, 1.7, 0xE2E8F0, 230);
                canvas.draw_aa_line(cx - 8.0, cy + 7.5, cx + 8.0, cy + 7.5, 1.7, 0xE2E8F0, 230);
                canvas.draw_aa_line(cx + 8.0, cy + 7.5, cx + 8.0, cy + 4.5, 1.7, 0xE2E8F0, 230);
            }
            AppId::Paint => {
                canvas.draw_aa_line(cx - 7.5, cy + 6.5, cx + 6.5, cy - 7.5, 2.0, 0xA78BFA, 255);
                canvas.fill_aa_circle(cx + 6.5, cy - 7.5, 2.0, 0xF472B6, 255);
                canvas.fill_aa_circle(cx - 7.0, cy - 5.5, 2.2, 0x38BDF8, 255);
                canvas.fill_aa_circle(cx + 1.0, cy + 7.0, 2.2, 0xFBBF24, 255);
                canvas.fill_aa_circle(cx + 7.5, cy + 3.0, 2.2, 0x4ADE80, 255);
            }
            AppId::Snake => {
                canvas.draw_aa_line(cx - 8.0, cy + 6.0, cx - 1.0, cy + 6.0, 2.6, 0x4ADE80, 255);
                canvas.draw_aa_line(cx - 1.0, cy + 6.0, cx - 1.0, cy - 5.0, 2.6, 0x4ADE80, 255);
                canvas.draw_aa_line(cx - 1.0, cy - 5.0, cx + 7.5, cy - 5.0, 2.6, 0x86EFAC, 255);
                canvas.fill_aa_circle(cx + 7.5, cy - 5.0, 2.2, 0x86EFAC, 255);
                canvas.fill_aa_circle(cx + 7.5, cy + 5.5, 2.6, 0xFB7185, 255);
            }
            AppId::Notepad => {
                canvas.draw_rounded_rect_alpha(bx + 11, by + 10, 18, 20, 4, 0x93C5FD, 215);
                canvas.draw_aa_line(cx - 5.0, cy - 4.0, cx + 5.0, cy - 4.0, 1.5, 0xE2E8F0, 235);
                canvas.draw_aa_line(cx - 5.0, cy, cx + 5.0, cy, 1.5, 0x94A3B8, 215);
                canvas.draw_aa_line(cx - 5.0, cy + 4.0, cx + 2.0, cy + 4.0, 1.5, 0x94A3B8, 215);
            }
            AppId::Settings => {
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

        let tw = canvas.measure_text(label, 1).min(78);
        let lx = x + 33 - tw / 2;
        canvas.draw_text_clipped(lx + 1, y + 44, 80, label, 0x05080D, 1);
        canvas.draw_text_clipped(lx, y + 43, 80, label, 0xE2E8F0, 1);
    }

    fn draw_window_frame(&self, canvas: &mut Canvas, win: &Window, focused: bool, is_onscreen: bool) {
        let lang = self.settings.lang;
        let accent = self.settings.active_accent();

        if is_onscreen {
            canvas.draw_soft_shadow(win.x, win.y, win.w, win.h, 12);
        }
        canvas.fill_rounded_rect(win.x, win.y, win.w, win.h, 12, 0x111622);
        canvas.draw_rounded_rect_alpha(
            win.x,
            win.y,
            win.w,
            win.h,
            12,
            0xFFFFFF,
            if focused { 42 } else { 18 },
        );

        let (tb_top, tb_bot) = if focused {
            (0x1E2638, 0x151B29)
        } else {
            (0x151A25, 0x121621)
        };
        canvas.fill_vgradient_top_rounded_rect(win.x + 1, win.y + 1, win.w - 2, 26, 11, tb_top, tb_bot);
        canvas.blend_rect(win.x + 14, win.y + 1, (win.w - 28).max(0), 1, 0xFFFFFF, if focused { 40 } else { 14 });
        if focused {
            canvas.blend_rect(win.x + 24, win.y, (win.w - 48).max(0), 1, accent, 82);
        }
        canvas.blend_rect(win.x + 1, win.y + 27, win.w - 2, 1, 0xFFFFFF, 16);
        canvas.fill_aa_circle(
            (win.x + 14) as f32,
            (win.y + 14) as f32,
            3.2,
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

        let btn_y = win.y + 5;
        let cy = btn_y + 9;
        let hover_y = is_onscreen && self.mouse_y >= btn_y && self.mouse_y <= btn_y + 18;
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

        let (hlx, hly) = if is_onscreen && focused {
            (self.mouse_x - win.x, self.mouse_y - win.y)
        } else {
            (-1, -1)
        };
        match win.id {
            AppId::System => self.draw_system_contents(canvas, win),
            AppId::PizGet => self.draw_pizget_contents(canvas, win),
            AppId::Files => self.draw_files_contents(canvas, win),
            AppId::Terminal => self.terminal.draw(canvas, win.x, win.y, win.w, win.h, accent),
            AppId::Paint => self.paint.draw(canvas, win.x, win.y, win.w, win.h, lang, accent, hlx, hly),
            AppId::Snake => self.snake.draw(canvas, win.x, win.y, win.h, lang, self.tick),
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
                self.tick,
                hlx,
                hly,
            ),
        }

        if !win.maximized {
            let grip_hov = is_onscreen
                && focused
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

        canvas.fill_rounded_rect(x - 4, y - 3, iw + 8, 34, 8, 0x0C111C);
        canvas.draw_rounded_rect_alpha(x - 4, y - 3, iw + 8, 34, 8, 0xFFFFFF, 20);
        canvas.draw_text_clipped(
            x + 4,
            y,
            iw - 8,
            &format!("{}: {}", tr(lang, "Ядро", "Kernel"), self.kernel),
            0xE6EDF3,
            1,
        );
        canvas.draw_text_clipped(
            x + 4,
            y + 15,
            iw - 8,
            &format!("{}: {}  |  Load: {}", tr(lang, "Аптайм", "Uptime"), self.uptime, self.loadavg),
            0x94A3B8,
            1,
        );

        canvas.draw_text_clipped(x, y + 38, iw, &format!("CPU: {}%", self.cpu_pct), 0x79C0FF, 1);
        canvas.draw_progress_bar(x, y + 52, iw, 14, self.cpu_pct, 0x2EA043, 0x0A0F16, 0x30363D);

        let gx = x;
        let gy = y + 72;
        let gh = 42;
        canvas.fill_rounded_rect(gx, gy, iw, gh, 8, 0x0A0F16);
        for pct_line in [1, 2, 3] {
            let ly = gy + (gh * pct_line) / 4;
            canvas.fill_rect(gx + 2, ly, iw - 4, 1, 0x1A222D);
        }
        canvas.draw_rounded_rect_alpha(gx, gy, iw, gh, 8, 0xFFFFFF, 26);
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
            0x0A0F16,
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

        canvas.fill_rounded_rect(x - 2, y - 2, iw + 4, (win.h - 82).max(40), 8, 0x0C111C);
        canvas.draw_rounded_rect_alpha(x - 2, y - 2, iw + 4, (win.h - 82).max(40), 8, 0xFFFFFF, 20);

        if self.packages.is_empty() {
            canvas.draw_text_clipped(
                x + 6,
                y + 8,
                iw - 12,
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
                    canvas.fill_rounded_rect(x + 1, ry, iw - 2, 16, 6, 0x1F6FEB);
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
        canvas.fill_rounded_rect(x - 2, win.y + 52, iw + 4, (win.h - 68).max(40), 8, 0x0C111C);
        canvas.draw_rounded_rect_alpha(x - 2, win.y + 52, iw + 4, (win.h - 68).max(40), 8, 0xFFFFFF, 20);

        let max_rows = ((win.h - 70) / 18).max(1) as usize;
        for (i, item) in self.files.iter().take(max_rows).enumerate() {
            let ry = win.y + 54 + (i as i32) * 18;
            let selected = i == self.file_sel;
            if selected {
                canvas.fill_rounded_rect(x + 1, ry, iw - 2, 16, 6, 0x21262D);
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
