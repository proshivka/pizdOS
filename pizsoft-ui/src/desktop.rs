use std::cell::RefCell;
use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

use crate::apps::{
    AiChatState, AiMsgKind, BrowserState, CalcState, CuAction, Game2048State, KeyEvent, Lang,
    MatrixState, MinesweeperState, NotepadState, PaintAction, PaintState, PizViewState,
    SettingsAction, SettingsState, SnakeEvent, SnakeState, TermAction, TerminalState, ThemePreset,
    UI_SCALES, parse_cu_response, tr,
};
use crate::display_srv::{DisplayServer, GuestProto};
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
    Game2048,
    PizView,
    Minesweeper,
    Ai,
    Browser,
    Guest(u32),
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
            (AppId::Ai, _) => "PizAI",
            (AppId::Browser, Lang::Ru) => "Браузер",
            (AppId::Browser, Lang::En) => "Browser",
            (AppId::Game2048, _) => "2048",
            (AppId::PizView, Lang::Ru) => "Просмотр",
            (AppId::PizView, Lang::En) => "PizView",
            (AppId::Minesweeper, Lang::Ru) => "Сапёр",
            (AppId::Minesweeper, Lang::En) => "Sweeper",
            (AppId::Guest(_), _) => "X11 / Wayland",
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
            AppId::Ai => 0xA855F7,
            AppId::Browser => 0x0EA5E9,
            AppId::Game2048 => 0xF59E0B,
            AppId::PizView => 0x06B6D4,
            AppId::Minesweeper => 0xF43F5E,
            AppId::Guest(_) => 0x0EA5E9,
        }
    }

    pub fn min_size(self) -> (i32, i32) {
        match self {
            AppId::Settings => (440, 310),
            AppId::Paint => (368, 286),
            AppId::Snake => (316, 274),
            AppId::Calc | AppId::Game2048 | AppId::Minesweeper => (220, 260),
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
    pub workspace: u8,
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
            workspace: 1,
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

const START_ITEMS: [Option<AppId>; 16] = [
    Some(AppId::Browser),
    Some(AppId::Ai),
    Some(AppId::Terminal),
    Some(AppId::Calc),
    Some(AppId::Paint),
    Some(AppId::Snake),
    Some(AppId::Game2048),
    Some(AppId::Minesweeper),
    Some(AppId::Notepad),
    Some(AppId::PizView),
    Some(AppId::Matrix),
    Some(AppId::Files),
    Some(AppId::PizGet),
    Some(AppId::System),
    Some(AppId::Settings),
    None,
];

const DESKTOP_ICONS: [(AppId, i32, i32, u32); 14] = [
    (AppId::Terminal, 18, 42, 0x3FB950),
    (AppId::PizGet, 18, 114, 0xD29922),
    (AppId::Paint, 18, 186, 0xBC8CFF),
    (AppId::Snake, 18, 258, 0x2EA043),
    (AppId::Notepad, 18, 330, 0x58A6FF),
    (AppId::Settings, 18, 402, 0xF85149),
    (AppId::Calc, 102, 42, 0x38BDF8),
    (AppId::Matrix, 102, 114, 0x22C55E),
    (AppId::Game2048, 102, 186, 0xF59E0B),
    (AppId::PizView, 102, 258, 0x06B6D4),
    (AppId::Minesweeper, 102, 330, 0xF43F5E),
    (AppId::Files, 102, 402, 0x58A6FF),
    (AppId::Ai, 186, 42, 0xA855F7),
    (AppId::Browser, 186, 114, 0x0EA5E9),
];

struct WallpaperCache {
    key: (usize, usize, ThemePreset, bool),
    pixels: Vec<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IslandMode {
    Status,
    Music,
    Pomodoro,
}

#[derive(Clone, Copy)]
struct AeroStar {
    x: f32,
    y: f32,
    vx: f32,
    vy: f32,
    life: f32,
    size: f32,
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
    pub cu_mini_anim: f32,
    pub cu_mini_vel: f32,
    pub ripples: Vec<ClickRipple>,
    stars: Vec<AeroStar>,
    pub sticky_note: String,
    pub sticky_focused: bool,
    pub screenshot_req: std::cell::Cell<bool>,
    pub locked: bool,
    pub switcher_open: bool,
    pub switcher_idx: usize,
    pub dnd: bool,
    pub island_mode: IslandMode,
    pub pomo_ticks: u32,
    pub active_workspace: u8,
    pub cal_open: bool,
    pub toast_history: Vec<String>,
    pub last_title_click: Option<(AppId, u64)>,
    pub file_sort_desc: bool,
    pub dock_float_anim: f32,
    pub dock_float_vel: f32,
    pub last_canvas_size: (i32, i32),
    pub icon_hover: [f32; 14],
    pub ai: AiChatState,
    pub browser: BrowserState,
    pub display_srv: DisplayServer,
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
    pub ram_history: [u8; 32],
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
    pub game2048: Game2048State,
    pub pizview: PizViewState,
    pub minesweeper: MinesweeperState,

    wp_cache: RefCell<WallpaperCache>,
    anim_canvas: RefCell<Canvas>,
}

#[derive(Clone, Copy)]
enum LauncherKind {
    App(AppId),
    NextTheme,
    ToggleDock,
    RefreshMetrics,
    LockScreen,
    ToggleDnd,
    CycleIslandMode,
    TakeScreenshot,
    ShowDesktop,
    SwitchWorkspace(u8),
    ToggleCalendar,
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
        let mut settings = SettingsState::new();
        let persisted = crate::config::load_config(&mut settings);
        let lang = settings.lang;
        let mut stars = Vec::with_capacity(28);
        for i in 0..28 {
            let ang = (i as f32) * 2.399_963;
            let spd = 0.45 + ((i % 5) as f32) * 0.14;
            let fx = ((i * 37 + 13) % 89) as f32 / 89.0;
            let fy = ((i * 61 + 29) % 83) as f32 / 83.0;
            stars.push(AeroStar {
                x: 0.04 + fx * 0.92,
                y: 0.04 + fy * 0.92,
                vx: ang.cos() * spd * 0.00065,
                vy: ang.sin() * spd * 0.00065,
                life: (i as f32) / 28.0,
                size: 1.5 + ((i % 3) as f32) * 0.75,
            });
        }

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
            island_anim: if settings.persistent_island { 1.0 } else { 0.0 },
            island_vel: 0.0,
            cu_mini_anim: 0.0,
            cu_mini_vel: 0.0,
            ripples: Vec::with_capacity(8),
            stars,
            sticky_note: persisted.sticky_note,
            sticky_focused: false,
            screenshot_req: std::cell::Cell::new(false),
            locked: false,
            switcher_open: false,
            switcher_idx: 0,
            dnd: false,
            island_mode: IslandMode::Status,
            pomo_ticks: 25 * 60 * 30,
            active_workspace: 1,
            cal_open: false,
            toast_history: Vec::new(),
            last_title_click: None,
            file_sort_desc: false,
            dock_float_anim: 0.0,
            dock_float_vel: 0.0,
            last_canvas_size: (width.max(320), height.max(240)),
            icon_hover: [0.0; 14],
            ai: AiChatState::new(&settings),
            browser: BrowserState::new(),
            display_srv: if cfg!(test) {
                DisplayServer::disabled()
            } else {
                DisplayServer::start_live()
            },
            mouse_x: width / 2,
            mouse_y: height / 2,
            mouse_down: false,
            dragging: None,
            resizing: None,
            windows: vec![
                Window::new(AppId::Files, 150, 70, 420, 310, false),
                Window::new(AppId::PizGet, 190, 90, 440, 310, false),
                Window::new(AppId::Terminal, 120, 60, 450, 290, false),
                Window::new(AppId::Paint, 240, 96, 368, 294, false),
                Window::new(AppId::Snake, 280, 100, 320, 278, false),
                Window::new(AppId::Notepad, 210, 118, 456, 286, false),
                Window::new(AppId::Calc, 300, 82, 276, 342, false),
                Window::new(AppId::Matrix, 240, 80, 380, 280, false),
                Window::new(AppId::Game2048, 340, 80, 276, 336, false),
                Window::new(AppId::PizView, 220, 86, 360, 300, false),
                Window::new(AppId::Minesweeper, 310, 76, 284, 332, false),
                Window::new(AppId::Ai, 272, 42, 450, 350, false),
                Window::new(AppId::Browser, 170, 52, 480, 356, false),
                Window::new(AppId::System, (width - 610).clamp(16, 190), 26, 330, 268, true),
                Window::new(AppId::Settings, (width - 540).clamp(16, 370), 90, 520, 344, true),
            ],
            tick: 0,
            status: String::from("0% GNU | 0 Deps"),

            cpu_pct: 12,
            cpu_history: [8, 12, 10, 14, 18, 15, 11, 9, 13, 16, 20, 14, 10, 12, 15, 18, 14, 11, 9, 13, 15, 12, 10, 14, 17, 13, 11, 10, 12, 15, 14, 12],
            ram_history: [28; 32],
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
            game2048: Game2048State::new(),
            pizview: PizViewState::new(lang),
            minesweeper: MinesweeperState::new(),

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
        d.snake.best_score = persisted.best_snake;
        d.display_srv.set_screen_size(width, height);
        d.terminal.display_env = d.display_srv.runtime_env();
        d.reload_packages();
        d.reload_files();
        d
    }

    pub fn win_title(&self, id: AppId) -> String {
        if let AppId::Guest(gid) = id
            && let Some(surf) = self.display_srv.window(gid)
        {
            return surf.title.clone();
        }
        if id == AppId::Browser && !self.browser.page.title.is_empty() {
            return self.browser.page.title.clone();
        }
        String::from(id.title(self.settings.lang))
    }

    pub fn poll_display_server(&mut self) {
        let (mapped, unmapped) = self.display_srv.poll();
        for gid in unmapped {
            self.windows.retain(|w| w.id != AppId::Guest(gid));
        }
        let (cw, ch) = self.last_canvas_size;
        for gid in mapped {
            if self.windows.iter().any(|w| w.id == AppId::Guest(gid)) {
                continue;
            }
            if let Some(surf) = self.display_srv.window(gid) {
                let ww = (surf.w + 8).clamp(240, (cw - 32).max(240));
                let wh = (surf.h + 36).clamp(180, (ch - 56).max(180));
                let wx = 190 + ((gid as i32 * 28) % 120);
                let wy = 56 + ((gid as i32 * 22) % 90);
                let mut win = Window::new(AppId::Guest(gid), wx, wy, ww, wh, true);
                win.workspace = self.active_workspace;
                win.open_anim = if self.settings.animations { 0.0 } else { 1.0 };
                win.sync_vis_rect();
                let proto = match surf.proto {
                    GuestProto::X11 => "X11",
                    GuestProto::Wayland => "Wayland",
                };
                let title = surf.title.clone();
                self.display_srv.set_surface_origin(gid, wx + 4, wy + 32);
                self.windows.push(win);
                self.push_toast(format!("{proto}: {title}"), AppId::Guest(gid).color());
            }
        }
    }

    pub fn push_toast(&mut self, text: impl Into<String>, color: u32) {
        if self.dnd {
            return;
        }
        self.force_toast(text, color);
    }

    pub fn force_toast(&mut self, text: impl Into<String>, color: u32) {
        let mut msg = text.into();
        if msg.chars().count() > 48 {
            msg = msg.chars().take(45).collect::<String>() + "...";
        }
        self.toast_history.push(msg.clone());
        if self.toast_history.len() > 8 {
            self.toast_history.remove(0);
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
            self.cal_open = false;
        }
    }

    pub fn toggle_calendar(&mut self) {
        self.cal_open = !self.cal_open;
        if self.cal_open {
            self.start_menu_open = false;
            self.cc_open = false;
            self.ctx_menu = None;
            self.launcher_open = false;
        }
    }

    pub fn switch_workspace(&mut self, ws: u8) {
        let target = ws.clamp(1, 3);
        if self.active_workspace != target {
            self.active_workspace = target;
            self.dragging = None;
            self.resizing = None;
            self.switcher_open = false;
            let lang = self.settings.lang;
            self.push_toast(
                format!("{} {target}", tr(lang, "Рабочий стол", "Workspace")),
                self.settings.active_accent(),
            );
        }
    }

    pub fn toggle_show_desktop(&mut self) {
        let ws = self.active_workspace;
        let any_vis = self
            .windows
            .iter()
            .any(|w| w.open && !w.minimized && w.workspace == ws);
        for w in &mut self.windows {
            if w.open && w.workspace == ws {
                w.minimized = any_vis;
                if !self.settings.animations {
                    w.min_anim = if any_vis { 0.0 } else { 1.0 };
                }
            }
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
            (AppId::Ai, "ai gemini chat нейросеть ии"),
            (AppId::Browser, "browser web internet pizarf браузер интернет"),
            (AppId::Game2048, "2048 game puzzle игра плитки"),
            (AppId::PizView, "bmp image photo viewer картинка просмотр фото"),
            (AppId::Minesweeper, "minesweeper сапер игра мины"),
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
            (
                tr(lang, "Экран блокировки (Ctrl+L)", "Lock Screen (Ctrl+L)"),
                tr(lang, "СИСТЕМА", "SYSTEM"),
                0x818CF8,
                LauncherKind::LockScreen,
                "lock block блок пароль экран",
            ),
            (
                tr(lang, "Не беспокоить (DND)", "Do Not Disturb (DND)"),
                tr(lang, "РЕЖИМ", "MODE"),
                0xC084FC,
                LauncherKind::ToggleDnd,
                "dnd quiet не беспокоить тишина",
            ),
            (
                tr(lang, "Островок: Эквалайзер / Pomodoro", "Island: Equalizer / Pomodoro"),
                tr(lang, "ОСТРОВ", "ISLAND"),
                0x10B981,
                LauncherKind::CycleIslandMode,
                "music player equalizer pomodoro timer музыка таймер",
            ),
            (
                tr(lang, "Скриншот рабочего стола", "Desktop Screenshot"),
                tr(lang, "СНИМОК", "SHOT"),
                0x38BDF8,
                LauncherKind::TakeScreenshot,
                "shot screenshot скриншот снимок bmp",
            ),
            (
                tr(lang, "Показать рабочий стол (Ctrl+D)", "Show Desktop (Ctrl+D)"),
                tr(lang, "ОКНА", "WIN"),
                0x34D399,
                LauncherKind::ShowDesktop,
                "desktop minimize свернуть рабочий стол",
            ),
            (
                tr(lang, "Календарь и уведомления", "Calendar & Notifications"),
                tr(lang, "ТРЕЙ", "TRAY"),
                0xF59E0B,
                LauncherKind::ToggleCalendar,
                "calendar notifications toast history календарь уведомления",
            ),
            (
                tr(lang, "Рабочий стол 2 (Ctrl+2)", "Workspace 2 (Ctrl+2)"),
                "WS 2",
                accent,
                LauncherKind::SwitchWorkspace(2),
                "workspace 2 стол 2",
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
            LauncherKind::LockScreen => {
                self.launcher_open = false;
                self.locked = true;
            }
            LauncherKind::ToggleDnd => {
                self.launcher_open = false;
                self.dnd = !self.dnd;
                self.force_toast(
                    if self.dnd {
                        tr(lang, "DND включён", "Do Not Disturb ON")
                    } else {
                        tr(lang, "DND выключен", "Do Not Disturb OFF")
                    },
                    0xC084FC,
                );
            }
            LauncherKind::CycleIslandMode => {
                self.launcher_open = false;
                self.settings.persistent_island = true;
                self.cycle_island_mode();
            }
            LauncherKind::TakeScreenshot => {
                self.launcher_open = false;
                self.screenshot_req.set(true);
                self.force_toast(tr(lang, "Скриншот сохранён", "Screenshot saved"), 0x38BDF8);
            }
            LauncherKind::ShowDesktop => {
                self.launcher_open = false;
                self.toggle_show_desktop();
            }
            LauncherKind::SwitchWorkspace(ws) => {
                self.launcher_open = false;
                self.switch_workspace(ws);
            }
            LauncherKind::ToggleCalendar => {
                self.launcher_open = false;
                self.toggle_calendar();
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
        dirs.sort_by(|a, b| if self.file_sort_desc { b.name.cmp(&a.name) } else { a.name.cmp(&b.name) });
        reg.sort_by(|a, b| if self.file_sort_desc { b.name.cmp(&a.name) } else { a.name.cmp(&b.name) });
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
        self.ram_history.rotate_left(1);
        self.ram_history[31] = self.mem.percent;
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
        self.display_srv.set_screen_size(canvas_w, canvas_h);
        let reserve = self.dock_reserve();
        let top_bar = if self.settings.taskbar_top { reserve } else { 0 };
        for win in &mut self.windows {
            if win.maximized {
                win.x = 0;
                win.y = top_bar;
                win.w = canvas_w.max(200);
                win.h = (canvas_h - reserve).max(160);
            } else {
                win.w = win.w.min((canvas_w - 12).max(200));
                win.x = win.x.clamp(0, (canvas_w - win.w).max(0));
                win.y = win.y.clamp(top_bar, (canvas_h - 60).max(top_bar));
            }
            win.sync_vis_rect();
        }
    }

    pub fn on_escape(&mut self) -> bool {
        if self.switcher_open {
            self.switcher_open = false;
            return true;
        }
        if self.ctx_menu.take().is_some() {
            return true;
        }
        if self.sticky_focused {
            self.sticky_focused = false;
            crate::config::save_config(&self.settings, self.snake.best_score, &self.sticky_note);
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
        if self.cal_open {
            self.cal_open = false;
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
        let avail = (canvas_w - 372).max(100);
        ((avail / open_count as i32) - 4).clamp(56, 130)
    }

    fn taskbar_pill_rect(&self, canvas_w: i32, canvas_h: i32, id: AppId) -> (i32, i32, i32) {
        let (dock_x, task_y, dock_w, _, _) = self.taskbar_rect(canvas_w, canvas_h);
        let ws = self.active_workspace;
        let open_count = self.windows.iter().filter(|w| w.open && w.workspace == ws).count();
        let pill_w = self.taskbar_pill_width(dock_w, open_count);
        let pos = self
            .windows
            .iter()
            .filter(|w| w.open && w.workspace == ws)
            .position(|w| w.id == id)
            .unwrap_or(0) as i32;
        (dock_x + 102 + pos * (pill_w + 4), task_y + 6, pill_w)
    }

    fn sync_ai_settings(&mut self) {
        self.settings.ai_provider = self.ai.provider.as_u8();
        self.settings.ai_api_key = self.ai.api_key.clone();
        self.settings.ai_base_url = self.ai.base_url.clone();
        self.settings.ai_custom_model = self.ai.custom_model.clone();
        self.settings.ai_model_idx = self.ai.model_idx;
        self.settings.ai_thinking_idx = self.ai.thinking.idx();
        self.settings.ai_thinking_mask = self.ai.custom_thinking_mask;
        self.settings.ai_show_full_thinking = self.ai.show_full_thinking;
        self.settings.ai_temp_idx = self.ai.temp_idx;
        self.settings.ai_system_prompt = self.ai.system_prompt.clone();
        crate::config::save_config(&self.settings, self.snake.best_score, &self.sticky_note);
    }

    pub fn capture_screen_png_base64(&self) -> (String, i32, i32) {
        let (w, h) = self.last_canvas_size;
        let cw = w.max(320);
        let ch = h.max(240);
        let mut cap = Canvas::new(cw as usize, ch as usize);
        self.render(&mut cap);
        // Overlay crisp 100px labeled grid + last click reticle ONLY on the screenshot sent to VLM
        let mut gx = 100;
        while gx < cw {
            cap.blend_rect(gx, 0, 1, ch, 0x38BDF8, 55);
            cap.draw_text_micro_clipped(gx + 2, 2, 32, &format!("{gx}"), 0xF8FAFC);
            gx += 100;
        }
        let mut gy = 100;
        while gy < ch {
            cap.blend_rect(0, gy, cw, 1, 0x38BDF8, 55);
            cap.draw_text_micro_clipped(2, gy + 2, 32, &format!("{gy}"), 0xF8FAFC);
            gy += 100;
        }
        if let Some((lx, ly)) = self.ai.cu_last_click {
            cap.fill_aa_circle(lx as f32, ly as f32, 6.0, 0xEF4444, 170);
            cap.fill_rect(lx - 8, ly, 17, 1, 0xFFFFFF);
            cap.fill_rect(lx, ly - 8, 1, 17, 0xFFFFFF);
        }
        let b64 = crate::gfx::encode_png_base64(&cap.pixels, cap.width, cap.height);
        (b64, cap.width as i32, cap.height as i32)
    }

    fn build_cu_ui_hint(&self) -> String {
        let (cw, ch) = self.last_canvas_size;
        let ws = self.active_workspace;
        let icons: Vec<String> = DESKTOP_ICONS
            .iter()
            .map(|&(id, ix, iy, _)| format!("{}({},{})", id.title(crate::apps::Lang::En), ix + 32, iy + 28))
            .collect();
        let mut wins = Vec::new();
        for win in &self.windows {
            if win.open && !win.minimized && win.workspace == ws {
                let extra = match win.id {
                    AppId::Browser => format!(
                        ",url_bar=({},{}),go=({},{})",
                        win.x + 200,
                        win.y + 44,
                        win.x + win.w - 26,
                        win.y + 44
                    ),
                    AppId::Terminal => format!(",input=({},{})", win.x + 120, win.y + win.h - 24),
                    AppId::Ai => format!(",self_input=({},{})", win.x + 80, win.y + win.h - 22),
                    _ => String::new(),
                };
                wins.push(format!(
                    "{}[x={}..{},y={}..{},close=({},{}){}]",
                    self.win_title(win.id),
                    win.x,
                    win.x + win.w,
                    win.y,
                    win.y + win.h,
                    win.x + win.w - 16,
                    win.y + 14,
                    extra
                ));
            }
        }
        let task_y = if self.settings.taskbar_top { 22 } else { ch - 22 };
        format!(
            "Screen={cw}x{ch}. StartBtn=(45,{task_y}). DesktopIcons(center x,y): {}. OpenWindows: {}",
            icons.join(", "),
            wins.join(", ")
        )
    }

    fn match_app_by_name(raw: &str) -> Option<AppId> {
        let q = raw.trim().to_ascii_lowercase();
        if q.contains("browser") || q.contains("брауз") || q.contains("surf") {
            Some(AppId::Browser)
        } else if q.contains("calc") || q.contains("кальк") {
            Some(AppId::Calc)
        } else if q.contains("term") || q.contains("терм") || q.contains("shell") {
            Some(AppId::Terminal)
        } else if q.contains("file") || q.contains("файл") {
            Some(AppId::Files)
        } else if q.contains("sett") || q.contains("настр") {
            Some(AppId::Settings)
        } else if q.contains("paint") || q.contains("рис") {
            Some(AppId::Paint)
        } else if q.contains("note") || q.contains("блокн") {
            Some(AppId::Notepad)
        } else if q.contains("snake") || q.contains("змей") {
            Some(AppId::Snake)
        } else if q.contains("2048") {
            Some(AppId::Game2048)
        } else if q.contains("mine") || q.contains("сап") {
            Some(AppId::Minesweeper)
        } else if q.contains("matrix") || q.contains("матр") {
            Some(AppId::Matrix)
        } else if q.contains("get") || q.contains("пак") {
            Some(AppId::PizGet)
        } else if q.contains("sys") || q.contains("сист") {
            Some(AppId::System)
        } else {
            None
        }
    }

    fn execute_cu_action(&mut self, act: CuAction, say: Option<String>) -> bool {
        if let Some(ref msg) = say {
            self.ai.cu_action_text = msg.clone();
            self.ai.push_msg(AiMsgKind::Assistant, msg.clone());
        }
        let (cw, ch) = self.last_canvas_size;
        let log_line = match &act {
            CuAction::Screenshot => String::from("screenshot"),
            CuAction::OpenApp(name) => format!("open_app({name})"),
            CuAction::Move(x, y) => format!("move({x},{y})"),
            CuAction::Click(x, y) => format!("click({x},{y})"),
            CuAction::DoubleClick(x, y) => format!("double_click({x},{y})"),
            CuAction::Type(t) => format!("type({t})"),
            CuAction::Key(k) => format!("key({k})"),
            CuAction::Wait => String::from("wait"),
            CuAction::Done(_) => String::from("done"),
        };
        if say.is_none() && !matches!(act, CuAction::Done(_)) {
            self.ai.cu_action_text = log_line.clone();
        }
        if self.ai.cu_history.len() >= 8 {
            self.ai.cu_history.remove(0);
        }
        self.ai.cu_history.push(log_line);
        match act {
            CuAction::Screenshot => {
                self.ai.cu_needs_capture = true;
                false
            }
            CuAction::OpenApp(name) => {
                if let Some(id) = Self::match_app_by_name(&name) {
                    self.focus_or_open(id);
                }
                self.ai.push_msg(AiMsgKind::CuStep, format!("open_app({name})"));
                false
            }
            CuAction::Move(x, y) => {
                self.ai.cu_last_click = Some((x, y));
                self.on_mouse_move(x, y, cw, ch);
                self.ai.push_msg(AiMsgKind::CuStep, format!("move({x}, {y})"));
                false
            }
            CuAction::Click(x, y) => {
                self.ai.cu_last_click = Some((x, y));
                self.on_mouse_move(x, y, cw, ch);
                self.on_mouse_down(x, y, cw, ch);
                self.on_mouse_up();
                self.ai.push_msg(AiMsgKind::CuStep, format!("click({x}, {y})"));
                false
            }
            CuAction::DoubleClick(x, y) => {
                self.ai.cu_last_click = Some((x, y));
                self.on_mouse_move(x, y, cw, ch);
                self.on_mouse_down(x, y, cw, ch);
                self.on_mouse_up();
                self.on_mouse_down(x, y, cw, ch);
                self.on_mouse_up();
                self.ai.push_msg(AiMsgKind::CuStep, format!("double_click({x}, {y})"));
                false
            }
            CuAction::Type(txt) => {
                for ch_in in txt.chars() {
                    self.on_key(KeyEvent::Char(ch_in));
                }
                self.ai.push_msg(AiMsgKind::CuStep, format!("type({txt})"));
                false
            }
            CuAction::Key(name) => {
                let ev = match name.to_ascii_lowercase().as_str() {
                    "enter" | "return" => Some(KeyEvent::Enter),
                    "backspace" => Some(KeyEvent::Backspace),
                    "escape" | "esc" => Some(KeyEvent::Escape),
                    "tab" => Some(KeyEvent::Tab),
                    "up" => Some(KeyEvent::Up),
                    "down" => Some(KeyEvent::Down),
                    "left" => Some(KeyEvent::Left),
                    "right" => Some(KeyEvent::Right),
                    _ => None,
                };
                if let Some(k) = ev {
                    self.on_key(k);
                }
                self.ai.push_msg(AiMsgKind::CuStep, format!("key({name})"));
                false
            }
            CuAction::Wait => {
                self.ai.push_msg(AiMsgKind::CuStep, "wait");
                false
            }
            CuAction::Done(summary) => {
                self.ai.cu_action_text = summary.clone();
                self.ai.push_msg(AiMsgKind::Assistant, summary);
                true
            }
        }
    }

    pub fn on_tick(&mut self) {
        self.tick = self.tick.wrapping_add(1);
        self.poll_display_server();
        self.ai.screen_size = self.last_canvas_size;
        if self.ai.cu_needs_capture && self.ai.cu_active {
            let (b64, w, h) = self.capture_screen_png_base64();
            let ui_hint = self.build_cu_ui_hint();
            self.ai.dispatch_cu_screen(&b64, w, h, &ui_hint);
        }
        if let Some(reply) = self.ai.poll() {
            if let Some(resp) = parse_cu_response(&reply) {
                if !self.ai.cu_active {
                    // Replace raw JSON tool call just appended by poll() with live CU execution
                    if matches!(self.ai.messages.last(), Some(m) if m.kind == AiMsgKind::Assistant && m.text.trim_start().starts_with('{')) {
                        self.ai.messages.pop();
                    }
                    self.ai.cu_active = true;
                }
                let is_shot = matches!(resp.action, CuAction::Screenshot);
                let done = self.execute_cu_action(resp.action, resp.say);
                if done || self.ai.cu_steps >= 12 {
                    self.ai.cu_active = false;
                } else if !is_shot {
                    self.ai.cu_wait_ticks = 10;
                }
            } else if self.ai.cu_active {
                self.ai.push_msg(AiMsgKind::Assistant, reply);
                self.ai.cu_active = false;
            }
        }
        if self.ai.cu_wait_ticks > 0 {
            self.ai.cu_wait_ticks -= 1;
            if self.ai.cu_wait_ticks == 0 && self.ai.cu_active && !self.ai.busy {
                self.ai.cu_needs_capture = true;
            }
        }
        self.browser.poll();
        self.settings.step_anim();
        self.calc.step_anim();

        let start_tgt = if self.start_menu_open { 1.0 } else { 0.0 };
        let ctx_tgt = if self.ctx_menu.is_some() { 1.0 } else { 0.0 };
        let cc_tgt = if self.cc_open { 1.0 } else { 0.0 };
        let launch_tgt = if self.launcher_open { 1.0 } else { 0.0 };
        let dock_tgt = if self.settings.floating_dock { 1.0 } else { 0.0 };
        let snap_tgt = if self.snap_zone.is_some() { 1.0 } else { 0.0 };
        let island_tgt = if self.settings.persistent_island || self.ai.cu_active {
            1.0
        } else {
            0.0
        };
        let mini_tgt = if self.ai.cu_active { 1.0 } else { 0.0 };
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
            self.cu_mini_anim = mini_tgt;
            self.cu_mini_vel = 0.0;
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
                0.28,
                0.44,
                0.005,
            );
            Canvas::step_spring(
                &mut self.cu_mini_anim,
                &mut self.cu_mini_vel,
                mini_tgt,
                0.26,
                0.42,
                0.005,
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

            if self.settings.aero_stars {
                for s in &mut self.stars {
                    s.x += s.vx;
                    s.y += s.vy;
                    s.life = (s.life + 0.0045) % 1.0;
                    if s.x < -0.04 {
                        s.x = 1.04;
                    } else if s.x > 1.04 {
                        s.x = -0.04;
                    }
                    if s.y < -0.04 {
                        s.y = 1.04;
                    } else if s.y > 1.04 {
                        s.y = -0.04;
                    }
                }
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
        if self.island_mode == IslandMode::Pomodoro {
            if self.pomo_ticks > 0 {
                self.pomo_ticks -= 1;
            }
            if self.pomo_ticks == 0 {
                self.island_mode = IslandMode::Status;
                self.pomo_ticks = 25 * 60 * 30;
                self.force_toast(tr(self.settings.lang, "Pomodoro завершён!", "Pomodoro Done!"), 0x10B981);
            }
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
                        crate::config::save_config(&self.settings, score, &self.sticky_note);
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
            win.workspace = self.active_workspace;
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
        let ws = self.active_workspace;
        let is_top_visible = self
            .windows
            .iter()
            .rev()
            .find(|w| w.open && !w.minimized && w.workspace == ws)
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
        if let AppId::Guest(gid) = self.windows[idx].id {
            let (gw, gh) = (self.windows[idx].w - 8, self.windows[idx].h - 36);
            self.display_srv.resize_guest(gid, gw, gh);
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
        let ph = 298;
        let px = (dock_x + dock_w - pw - 8).max(8);
        let py = if self.settings.taskbar_top {
            task_y + dock_h + 6
        } else {
            task_y - ph - 6
        };
        (px, py, pw, ph)
    }

    fn idle_island_text(&self) -> String {
        match self.island_mode {
            IslandMode::Status => {
                let clock_or_up = if self.settings.show_clock {
                    &self.clock_str
                } else {
                    &self.uptime
                };
                if self.dnd {
                    format!("DND  ·  pizdOS  ·  CPU {}%  ·  {}", self.cpu_pct, clock_or_up)
                } else {
                    format!("pizdOS  ·  CPU {}%  ·  {}", self.cpu_pct, clock_or_up)
                }
            }
            IslandMode::Music => {
                if self.dnd {
                    String::from("DND  ·  pizdOS Lo-Fi  ·  Synth")
                } else {
                    String::from("pizdOS Lo-Fi  ·  Synthwave")
                }
            }
            IslandMode::Pomodoro => {
                let secs = self.pomo_ticks / 30;
                format!("Pomodoro  ·  {:02}:{:02}", secs / 60, secs % 60)
            }
        }
    }

    fn idle_island_rect(&self, canvas_w: i32, canvas_h: i32, txt: &str) -> (i32, i32, i32, i32) {
        let text_w = crate::font::measure_text_scaled(txt, 1, 1.0);
        let tw = (text_w + 76).clamp(184, (canvas_w - 32).max(184));
        let th = 26;
        let tx = (canvas_w - tw) / 2;
        let ty = if self.settings.taskbar_top { canvas_h - 40 } else { 10 };
        (tx, ty, tw, th)
    }

    fn switcher_rect(&self, canvas_w: i32, canvas_h: i32, count: usize) -> (i32, i32, i32, i32, i32, i32) {
        let n = count.clamp(1, 7) as i32;
        let gap = 8;
        let bw = (n * 92 + (n - 1) * gap + 28).clamp(240, (canvas_w - 28).max(240));
        let bh = 114;
        let cw = ((bw - 28 - (n - 1) * gap) / n).max(36);
        let bx = (canvas_w - bw) / 2;
        let by = (canvas_h - bh) / 2;
        (bx, by, bw, bh, cw, gap)
    }

    pub fn cycle_island_mode(&mut self) {
        self.island_mode = match self.island_mode {
            IslandMode::Status => IslandMode::Music,
            IslandMode::Music => {
                self.pomo_ticks = 25 * 60 * 30;
                IslandMode::Pomodoro
            }
            IslandMode::Pomodoro => IslandMode::Status,
        };
    }

    pub fn on_key(&mut self, key: KeyEvent) {
        if self.locked {
            if matches!(key, KeyEvent::Enter | KeyEvent::Char(' ') | KeyEvent::Escape) {
                self.locked = false;
            }
            return;
        }
        if key == KeyEvent::Escape && self.on_escape() {
            return;
        }
        match key {
            KeyEvent::LockScreen => {
                self.locked = true;
                self.switcher_open = false;
                self.start_menu_open = false;
                self.cc_open = false;
                self.launcher_open = false;
                self.cal_open = false;
                return;
            }
            KeyEvent::ShowDesktop => {
                self.toggle_show_desktop();
                return;
            }
            KeyEvent::Workspace(ws) => {
                self.switch_workspace(ws);
                return;
            }
            KeyEvent::SwitchWindows => {
                let ws = self.active_workspace;
                let count = self.windows.iter().filter(|w| w.open && w.workspace == ws).count();
                if count > 0 {
                    if !self.switcher_open {
                        self.switcher_open = true;
                        self.switcher_idx = if count > 1 { 1 } else { 0 };
                    } else {
                        self.switcher_idx = (self.switcher_idx + 1) % count;
                    }
                }
                return;
            }
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

        if self.switcher_open {
            let ws = self.active_workspace;
            let ids: Vec<AppId> = self.windows.iter().rev().filter(|w| w.open && w.workspace == ws).map(|w| w.id).collect();
            match key {
                KeyEvent::Right | KeyEvent::Down | KeyEvent::Tab => {
                    if !ids.is_empty() {
                        self.switcher_idx = (self.switcher_idx + 1) % ids.len();
                    }
                }
                KeyEvent::Left | KeyEvent::Up => {
                    if !ids.is_empty() {
                        self.switcher_idx = (self.switcher_idx + ids.len() - 1) % ids.len();
                    }
                }
                KeyEvent::Enter => {
                    if let Some(&id) = ids.get(self.switcher_idx) {
                        self.focus_or_open(id);
                    }
                    self.switcher_open = false;
                }
                _ => {}
            }
            return;
        }

        if self.sticky_focused {
            match key {
                KeyEvent::Escape => {
                    self.sticky_focused = false;
                    crate::config::save_config(&self.settings, self.snake.best_score, &self.sticky_note);
                }
                KeyEvent::Backspace => {
                    self.sticky_note.pop();
                    crate::config::save_config(&self.settings, self.snake.best_score, &self.sticky_note);
                }
                KeyEvent::Enter => {
                    if self.sticky_note.len() < 160 {
                        self.sticky_note.push('\n');
                        crate::config::save_config(&self.settings, self.snake.best_score, &self.sticky_note);
                    }
                }
                KeyEvent::Paste(txt) => {
                    for c in txt.chars() {
                        if !c.is_control() && self.sticky_note.len() < 160 {
                            self.sticky_note.push(c);
                        }
                    }
                    crate::config::save_config(&self.settings, self.snake.best_score, &self.sticky_note);
                }
                KeyEvent::Copy => {
                    crate::platform::copy_to_clipboard(&self.sticky_note);
                    self.push_toast(tr(self.settings.lang, "Скопировано", "Copied"), 0xFCD34D);
                }
                KeyEvent::Char(c) if self.sticky_note.len() < 160 => {
                    self.sticky_note.push(c);
                    crate::config::save_config(&self.settings, self.snake.best_score, &self.sticky_note);
                }
                _ => {}
            }
            return;
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
                KeyEvent::Paste(txt) => {
                    for ch in txt.chars() {
                        if !ch.is_control() && self.launcher_query.chars().count() < 36 {
                            self.launcher_query.push(ch);
                        }
                    }
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

        let ws = self.active_workspace;
        let Some(top) = self.windows.iter().rev().find(|w| w.open && !w.minimized && w.workspace == ws) else {
            return;
        };
        if key == KeyEvent::Copy {
            let clip = match top.id {
                AppId::Notepad => self.notepad.lines.join("\n"),
                AppId::Terminal => {
                    if !self.terminal.input.is_empty() {
                        self.terminal.input.clone()
                    } else {
                        self.terminal.lines.last().cloned().unwrap_or_default()
                    }
                }
                AppId::Calc => self.calc.display.clone(),
                _ => String::new(),
            };
            if !clip.is_empty() {
                crate::platform::copy_to_clipboard(&clip);
                self.push_toast(tr(self.settings.lang, "Скопировано", "Copied"), self.settings.active_accent());
            }
            return;
        }
        match top.id {
            AppId::Terminal => match self.terminal.on_key(key, &self.kernel, &self.mem) {
                TermAction::None => {}
                TermAction::SetTheme(idx) => {
                    self.settings.theme = match idx {
                        1 => ThemePreset::CrimsonSunset,
                        2 => ThemePreset::MatrixGlow,
                        3 => ThemePreset::OceanGlass,
                        _ => ThemePreset::AuroraPeaks,
                    };
                    crate::config::save_config(&self.settings, self.snake.best_score, &self.sticky_note);
                    self.push_toast(
                        self.settings.theme.name(self.settings.lang),
                        self.settings.active_accent(),
                    );
                }
                TermAction::SetScale(pct) => {
                    let old_scale = self.settings.ui_scale();
                    self.settings.ui_scale_pct = pct;
                    let (cw, ch) = self.last_canvas_size;
                    self.rescale_viewport(old_scale, cw, ch);
                    crate::config::save_config(&self.settings, self.snake.best_score, &self.sticky_note);
                }
                TermAction::SetResolution(idx) => {
                    self.settings.res_idx = idx.min(crate::apps::SCREEN_RES_PRESETS.len() - 1);
                    let (rw, rh) = self.settings.resolution();
                    self.display_srv.set_screen_size(rw, rh);
                    crate::config::save_config(&self.settings, self.snake.best_score, &self.sticky_note);
                    self.push_toast(
                        format!("{rw}x{rh}"),
                        self.settings.active_accent(),
                    );
                }
                TermAction::LaunchX11Demo => {
                    if !self.display_srv.spawn_x11_demo() {
                        self.terminal.push_line(String::from("X11 TCP socket unavailable (disabled in test/occupied port)"));
                    } else {
                        self.push_toast("X11 Demo launching...", 0x0EA5E9);
                    }
                }
            },
            AppId::Snake => self.snake.on_key(key),
            AppId::Game2048 => self.game2048.on_key(key),
            AppId::Notepad => {
                let lang = self.settings.lang;
                if let Some(msg) = self.notepad.on_key(key, lang) {
                    self.status = msg.clone();
                    self.push_toast(msg, 0x58A6FF);
                }
            }
            AppId::Calc => self.calc.on_key(key),
            AppId::Paint => {
                if key == KeyEvent::Undo && self.paint.undo() {
                    self.push_toast(tr(self.settings.lang, "Отменено в Paint", "Paint Undo"), 0xBC8CFF);
                }
            }
            AppId::Settings => self.settings.on_key(key),
            AppId::Ai => {
                match key {
                    KeyEvent::Char(c) => self.ai.on_char(c),
                    KeyEvent::Backspace => self.ai.on_backspace(),
                    KeyEvent::Enter => {
                        if self.ai.on_enter() {
                            self.sync_ai_settings();
                        }
                    }
                    KeyEvent::Paste(s) => self.ai.paste(&s),
                    _ => {}
                }
            }
            AppId::Browser => self.browser.on_key(&key),
            AppId::Guest(gid) => {
                let ch = match key {
                    KeyEvent::Char(c) => Some(c),
                    _ => None,
                };
                self.display_srv.send_key(gid, ch, true);
                self.display_srv.send_key(gid, ch, false);
            }
            _ => {}
        }
    }

    pub fn on_mouse_move(&mut self, x: i32, y: i32, canvas_w: i32, canvas_h: i32) {
        self.last_canvas_size = (canvas_w.max(320), canvas_h.max(240));
        self.mouse_x = x.clamp(0, canvas_w.saturating_sub(1));
        self.mouse_y = y.clamp(0, canvas_h.saturating_sub(1));
        for w in &self.windows {
            if let AppId::Guest(gid) = w.id {
                self.display_srv.set_surface_origin(gid, w.x + 4, w.y + 32);
            }
        }
        self.display_srv.set_global_pointer(self.mouse_x, self.mouse_y);

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

        if !self.launcher_open
            && !self.cc_open
            && !self.start_menu_open
            && !self.cal_open
            && let Some(top) = self
                .windows
                .iter()
                .rev()
                .find(|w| w.open && !w.minimized && w.workspace == self.active_workspace)
                .cloned()
        {
            if self.mouse_down && top.id == AppId::Paint {
                self.paint
                    .paint_at(self.mouse_x - top.x, self.mouse_y - top.y);
            } else if let AppId::Guest(gid) = top.id
                && self.mouse_x >= top.x + 4
                && self.mouse_x < top.x + top.w - 4
                && self.mouse_y >= top.y + 32
                && self.mouse_y < top.y + top.h - 4
            {
                self.display_srv
                    .send_pointer(gid, self.mouse_x - top.x - 4, self.mouse_y - top.y - 32, None);
            }
        }
    }

    pub fn on_right_click(&mut self, x: i32, y: i32, canvas_w: i32, canvas_h: i32) {
        self.last_canvas_size = (canvas_w.max(320), canvas_h.max(240));
        self.mouse_x = x.clamp(0, canvas_w.saturating_sub(1));
        self.mouse_y = y.clamp(0, canvas_h.saturating_sub(1));
        for w in &self.windows {
            if let AppId::Guest(gid) = w.id {
                self.display_srv.set_surface_origin(gid, w.x + 4, w.y + 32);
            }
        }
        self.display_srv.set_global_pointer(self.mouse_x, self.mouse_y);
        let ws = self.active_workspace;
        if let Some(win) = self
            .windows
            .iter()
            .rev()
            .find(|w| w.open && !w.minimized && w.workspace == ws && x >= w.x && x < w.x + w.w && y >= w.y && y < w.y + w.h)
            .cloned()
            && win.id == AppId::Minesweeper
        {
            self.minesweeper.on_click(x - win.x, y - win.y, win.w, win.h, true);
            return;
        }
        self.start_menu_open = false;
        self.cc_open = false;
        self.launcher_open = false;
        self.cal_open = false;
        let mx = x.min((canvas_w - 190).max(0));
        let my = y.min((canvas_h - 170).max(0));
        self.ctx_menu = Some((mx, my));
        self.last_ctx_menu = Some((mx, my));
    }

    pub fn on_mouse_down(&mut self, x: i32, y: i32, canvas_w: i32, canvas_h: i32) {
        self.last_canvas_size = (canvas_w.max(320), canvas_h.max(240));
        self.mouse_x = x.clamp(0, canvas_w.saturating_sub(1));
        self.mouse_y = y.clamp(0, canvas_h.saturating_sub(1));
        for w in &self.windows {
            if let AppId::Guest(gid) = w.id {
                self.display_srv.set_surface_origin(gid, w.x + 4, w.y + 32);
            }
        }
        self.display_srv.set_global_pointer(self.mouse_x, self.mouse_y);
        self.mouse_down = true;
        if self.settings.animations && self.ripples.len() < 6 {
            self.ripples.push(ClickRipple {
                x: self.mouse_x,
                y: self.mouse_y,
                t: 0.0,
                color: self.settings.active_accent(),
            });
        }

        if self.locked {
            self.locked = false;
            return;
        }

        if self.switcher_open {
            let ws = self.active_workspace;
            let ids: Vec<AppId> = self.windows.iter().rev().filter(|w| w.open && w.workspace == ws).map(|w| w.id).collect();
            let shown = ids.len().min(7);
            if shown > 0 {
                let (bx, by, bw, bh, cw, gap) = self.switcher_rect(canvas_w, canvas_h, shown);
                if x >= bx && x <= bx + bw && y >= by + 32 && y <= by + bh - 12 {
                    for (i, &id) in ids.iter().take(7).enumerate() {
                        let cx = bx + 14 + (i as i32) * (cw + gap);
                        if x >= cx && x <= cx + cw {
                            self.focus_or_open(id);
                            self.switcher_open = false;
                            return;
                        }
                    }
                }
            }
            self.switcher_open = false;
            return;
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
        let ws_hit = x >= dock_x
            && x < dock_x + dock_w
            && y >= task_y
            && y < task_y + dock_h
            && x >= lang_x - 122
            && x <= lang_x - 94;
        let cal_hit = x >= dock_x
            && x < dock_x + dock_w
            && y >= task_y
            && y < task_y + dock_h
            && x >= lang_x - 90
            && x <= lang_x - 64;
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
                    crate::config::save_config(&self.settings, self.snake.best_score, &self.sticky_note);
                    return;
                }
                let fx_y = py + 234;
                if y >= fx_y && y <= fx_y + 26 {
                    for idx in 0..4 {
                        let fx = px + 10 + idx * 65;
                        if x >= fx && x <= fx + 61 {
                            match idx {
                                0 => self.settings.vista_sidebar = !self.settings.vista_sidebar,
                                1 => self.settings.aero_stars = !self.settings.aero_stars,
                                2 => self.settings.night_shift = !self.settings.night_shift,
                                _ => self.settings.crt_scanlines = !self.settings.crt_scanlines,
                            }
                            crate::config::save_config(&self.settings, self.snake.best_score, &self.sticky_note);
                            return;
                        }
                    }
                }
                let foot_y = py + 266;
                if y >= foot_y && y <= foot_y + 24 {
                    if x >= px + pw - 72 {
                        self.screenshot_req.set(true);
                        self.cc_open = false;
                        self.push_toast(
                            tr(lang, "Скриншот сохранён", "Screenshot saved"),
                            0x38BDF8,
                        );
                    } else {
                        self.refresh_metrics();
                        self.push_toast(tr(lang, "Метрики обновлены", "Metrics refreshed"), 0x3FB950);
                    }
                }
                return;
            }
            if !cc_hit {
                self.cc_open = false;
            }
        }

        // 0d. Calendar + Notifications + Linux DE Power Popup
        if self.cal_open {
            let pw = 290;
            let ph = 310;
            let px = (dock_x + dock_w - pw - 8).max(8);
            let py = if self.settings.taskbar_top {
                task_y + dock_h + 6
            } else {
                (task_y - ph - 8).max(8)
            };
            if x >= px && x <= px + pw && y >= py && y <= py + ph {
                let lang = self.settings.lang;
                if y >= py + 154 && y <= py + 174 && x >= px + pw - 70 && x <= px + pw - 10 {
                    self.toast_history.clear();
                    return;
                }
                if y >= py + 274 && y <= py + 300 {
                    if x >= px + 10 && x <= px + 96 {
                        self.running = false;
                    } else if x >= px + 102 && x <= px + 188 {
                        if !crate::platform::trigger_power_action("reboot") {
                            self.force_toast(
                                tr(
                                    lang,
                                    "Доступно только в pizdOS Linux DE",
                                    "Only available in real pizdOS Linux DE",
                                ),
                                0xF59E0B,
                            );
                        }
                    } else if x >= px + 194
                        && x <= px + 280
                        && !crate::platform::trigger_power_action("poweroff")
                    {
                        self.force_toast(
                            tr(
                                lang,
                                "Доступно только в pizdOS Linux DE",
                                "Only available in real pizdOS Linux DE",
                            ),
                            0xF85149,
                        );
                    }
                }
                return;
            }
            if !cal_hit {
                self.cal_open = false;
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
                    self.cal_open = false;
                }
                return;
            }
            if ws_hit {
                self.switch_workspace((self.active_workspace % 3) + 1);
                return;
            }
            if cal_hit {
                self.toggle_calendar();
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
            if x >= dock_x + dock_w - 10 {
                self.toggle_show_desktop();
                return;
            }
            if x > lang_x + 38 {
                self.settings.show_clock = !self.settings.show_clock;
                return;
            }

            self.cc_open = false;
            self.launcher_open = false;
            self.cal_open = false;
            let ws = self.active_workspace;
            let open_ids: Vec<AppId> = self
                .windows
                .iter()
                .filter(|w| w.open && w.workspace == ws)
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

        // 2b. Always-On Dynamic Island pill click (left -> Control Center, right -> cycle mode)
        if self.settings.persistent_island && self.toast.is_none() {
            let txt = self.idle_island_text();
            let (ix, iy, iw, ih) = self.idle_island_rect(canvas_w, canvas_h, &txt);
            if x >= ix && x <= ix + iw && y >= iy && y <= iy + ih {
                if x >= ix + iw - 38 {
                    self.cycle_island_mode();
                } else {
                    self.toggle_control_center();
                }
                return;
            }
        }

        // 3. Topmost visible window hit test (expanded 4px border zone for effortless edge-resize grabs)
        let ws = self.active_workspace;
        let mut hit_idx = None;
        for (idx, win) in self.windows.iter().enumerate().rev() {
            let pad = if win.maximized { 0 } else { 4 };
            if win.open
                && !win.minimized
                && win.workspace == ws
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
                if self.settings.macos_buttons {
                    if x >= win.x + 6 && x <= win.x + 20 {
                        if let AppId::Guest(gid) = win.id {
                            self.display_srv.close_guest(gid);
                            self.windows.remove(top_idx);
                        } else {
                            self.windows[top_idx].open = false;
                        }
                        self.dragging = None;
                        self.resizing = None;
                        return;
                    }
                    if x >= win.x + 24 && x <= win.x + 38 {
                        self.windows[top_idx].minimized = true;
                        self.dragging = None;
                        self.resizing = None;
                        return;
                    }
                    if x >= win.x + 42 && x <= win.x + 56 {
                        self.toggle_maximize(top_idx, canvas_w, canvas_h);
                        self.dragging = None;
                        self.resizing = None;
                        return;
                    }
                } else {
                    // Close [×]
                    if x >= win.x + win.w - 26 && x <= win.x + win.w - 6 {
                        if let AppId::Guest(gid) = win.id {
                            self.display_srv.close_guest(gid);
                            self.windows.remove(top_idx);
                        } else {
                            self.windows[top_idx].open = false;
                        }
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
                if let Some((prev_id, prev_t)) = self.last_title_click
                    && prev_id == win.id
                    && self.tick.saturating_sub(prev_t) <= 18
                {
                    self.last_title_click = None;
                    self.toggle_maximize(top_idx, canvas_w, canvas_h);
                    self.dragging = None;
                    self.resizing = None;
                    return;
                }
                self.last_title_click = Some((win.id, self.tick));
                self.dragging = Some((win.id, x - win.x, y - win.y));
                return;
            }

            self.paint.end_stroke();
            self.handle_window_click(&win, x - win.x, y - win.y, canvas_w, canvas_h);
            return;
        }

        // 4. Desktop icons & Vista Sidebar Sticky Note
        if self.sticky_focused {
            self.sticky_focused = false;
            crate::config::save_config(&self.settings, self.snake.best_score, &self.sticky_note);
        }
        if self.settings.vista_sidebar && canvas_w >= 540 {
            let top_bar = if self.settings.taskbar_top { self.dock_reserve() } else { 0 };
            let sx = canvas_w - 176;
            let sy = top_bar + 272;
            if x >= sx && x <= sx + 164 && y >= sy && y <= sy + 104 {
                self.sticky_focused = true;
                return;
            }
        }
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
        let ended_rz = self.resizing.take();
        if let Some(rz) = ended_rz
            && let AppId::Guest(gid) = rz.id
            && let Some(win) = self.windows.iter().find(|w| w.id == AppId::Guest(gid))
        {
            self.display_srv.resize_guest(gid, win.w - 8, win.h - 36);
        }
        self.paint.end_stroke();

        let was_dragging = self.dragging.take();
        if let Some((id, _, _)) = was_dragging
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
            if let AppId::Guest(gid) = id {
                self.display_srv.resize_guest(gid, sw - 8, sh - 36);
            }
            let msg = match zone {
                SnapZone::Left => tr(lang, "Приклеено влево", "Snapped Left"),
                SnapZone::Right => tr(lang, "Приклеено вправо", "Snapped Right"),
                SnapZone::Maximize => tr(lang, "Окно развернуто", "Window Maximized"),
            };
            self.push_toast(msg, accent);
        } else if was_dragging.is_none()
            && ended_rz.is_none()
            && let Some(top) = self.windows.iter().rfind(|w| w.open && !w.minimized && w.workspace == self.active_workspace)
            && let AppId::Guest(gid) = top.id
            && self.mouse_x >= top.x + 4
            && self.mouse_x < top.x + top.w - 4
            && self.mouse_y >= top.y + 32
            && self.mouse_y < top.y + top.h - 4
        {
            let gx = self.mouse_x - (top.x + 4);
            let gy = self.mouse_y - (top.y + 32);
            self.display_srv.send_pointer(gid, gx, gy, Some((1, false)));
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
                if (50..=70).contains(&ly) {
                    if (14..=48).contains(&lx) {
                        if let Some(p) = self.cwd.parent().map(|p| p.to_path_buf()) {
                            self.cwd = p;
                            self.reload_files();
                        }
                    } else if (52..=102).contains(&lx) {
                        self.file_sort_desc = !self.file_sort_desc;
                        self.reload_files();
                    } else if (106..=164).contains(&lx) {
                        let mut n = 1;
                        let target = loop {
                            let cand = self.cwd.join(format!("note_{n}.txt"));
                            if !cand.exists() {
                                break cand;
                            }
                            n += 1;
                        };
                        match fs::write(&target, "pizdOS 2.1(00) note\n") {
                            Ok(()) => {
                                let name = target.file_name().unwrap_or_default().to_string_lossy().to_string();
                                self.reload_files();
                                self.push_toast(format!("{}: {name}", tr(lang, "Создан", "Created")), 0x3FB950);
                            }
                            Err(e) => self.push_toast(format!("Err: {e}"), 0xF85149),
                        }
                    } else if (168..=224).contains(&lx) {
                        let mut n = 1;
                        let target = loop {
                            let cand = self.cwd.join(format!("dir_{n}"));
                            if !cand.exists() {
                                break cand;
                            }
                            n += 1;
                        };
                        match fs::create_dir(&target) {
                            Ok(()) => {
                                let name = target.file_name().unwrap_or_default().to_string_lossy().to_string();
                                self.reload_files();
                                self.push_toast(format!("{}: {name}", tr(lang, "Папка", "Folder")), 0x3FB950);
                            }
                            Err(e) => self.push_toast(format!("Err: {e}"), 0xF85149),
                        }
                    } else if (228..=278).contains(&lx)
                        && let Some(entry) = self.files.get(self.file_sel).cloned()
                        && entry.name != ".."
                    {
                        let res = if entry.is_dir {
                            fs::remove_dir(&entry.path)
                        } else {
                            fs::remove_file(&entry.path)
                        };
                        match res {
                            Ok(()) => {
                                self.reload_files();
                                self.push_toast(
                                    format!("{}: {}", tr(lang, "Удалён", "Deleted"), entry.name),
                                    0xF59E0B,
                                );
                            }
                            Err(e) => self.push_toast(format!("Err: {e}"), 0xF85149),
                        }
                    }
                    return;
                }
                if ly >= 76 && ly < win.h - 16 {
                    let row = ((ly - 76) / 18) as usize;
                    if let Some(entry) = self.files.get(row).cloned() {
                        self.file_sel = row;
                        if entry.is_dir {
                            self.cwd = entry.path;
                            self.reload_files();
                        } else if entry
                            .path
                            .extension()
                            .and_then(|e| e.to_str())
                            .is_some_and(|e| e.eq_ignore_ascii_case("bmp"))
                        {
                            match self.pizview.open_bmp(&entry.path, lang) {
                                Ok(msg) => {
                                    self.status = msg.clone();
                                    self.push_toast(msg, 0x38BDF8);
                                    self.focus_or_open(AppId::PizView);
                                }
                                Err(err) => {
                                    self.status = err.clone();
                                    self.push_toast(err, 0xF85149);
                                }
                            }
                        } else {
                            match self.notepad.open_file(&entry.path, lang) {
                                Ok(msg) => {
                                    self.status = msg.clone();
                                    self.push_toast(msg, 0x58A6FF);
                                    self.focus_or_open(AppId::Notepad);
                                }
                                Err(err) => {
                                    self.status = err.clone();
                                    self.push_toast(err, 0xF85149);
                                }
                            }
                        }
                    }
                }
            }
            AppId::PizView => self.pizview.on_click(lx, ly, win.h),
            AppId::Minesweeper => self.minesweeper.on_click(lx, ly, win.w, win.h, false),
            AppId::Guest(gid) => self.display_srv.send_pointer(gid, lx - 4, ly - 32, Some((1, true))),
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
            AppId::Game2048 => self.game2048.on_click(lx, ly, win.w, win.h),
            AppId::Matrix => {
                if ly >= win.h - 32 && ly <= win.h - 8 && (12..=142).contains(&lx) {
                    self.matrix.cycle_color();
                }
            }
            AppId::Ai => {
                if self.ai.on_click(lx, ly, win.w, win.h) {
                    self.sync_ai_settings();
                }
            }
            AppId::Browser => self.browser.on_click(lx, ly, win.w, win.h),
            AppId::Settings => {
                let old_scale = self.settings.ui_scale();
                let prev_dock = self.settings.floating_dock;
                let prev_taps = self.settings.update_taps;
                let prev_res = self.settings.res_idx;
                let action = self.settings.on_click(lx, ly, win.w, win.h);
                if self.settings.update_taps >= 5 && prev_taps < 5 {
                    self.force_toast(
                        tr(lang, "Режим разработчика 2.1(00) активирован!", "Developer Mode 2.1(00) unlocked!"),
                        0xA855F7,
                    );
                }
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
                if self.settings.res_idx != prev_res {
                    let (rw, rh) = self.settings.resolution();
                    self.display_srv.set_screen_size(rw, rh);
                    self.push_toast(format!("{rw}x{rh}"), self.settings.active_accent());
                }
                crate::config::save_config(&self.settings, self.snake.best_score, &self.sticky_note);
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
                    SettingsAction::ResetDefaults => {
                        let old_scale = self.settings.ui_scale();
                        self.settings = SettingsState::new();
                        self.sticky_note = crate::config::PersistedState::default().sticky_note;
                        self.rescale_viewport(old_scale, canvas_w, canvas_h);
                        self.on_viewport_resize(canvas_w, canvas_h);
                        crate::config::save_config(
                            &self.settings,
                            self.snake.best_score,
                            &self.sticky_note,
                        );
                        self.force_toast(
                            tr(lang, "Настройки сброшены", "Settings reset to defaults"),
                            0xF59E0B,
                        );
                    }
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

        // 1b. Longhorn 4074 Aero Stars
        if self.settings.aero_stars {
            self.draw_aero_stars(canvas, w, h, accent);
        }

        // 2. Desktop Icons (with smooth spring hover lift)
        if self.settings.show_icons {
            for (idx, &(id, ix, iy, col)) in DESKTOP_ICONS.iter().enumerate() {
                self.draw_desktop_icon(canvas, idx, ix, iy, id, id.title(lang), col);
            }
        }

        // 2a. Windows Vista Sidebar Gadgets (Analog Clock, CPU/RAM Gauges, Sticky Note)
        if self.settings.vista_sidebar && w >= 540 {
            self.draw_vista_sidebar(canvas, w, h, accent);
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
        let ws = self.active_workspace;
        let top_vis = self
            .windows
            .iter()
            .enumerate()
            .rev()
            .find(|(_, win)| win.open && !win.minimized && win.workspace == ws)
            .map(|(i, _)| i);

        for (idx, win) in self.windows.iter().enumerate() {
            if win.workspace != ws {
                continue;
            }
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
            .filter(|(_, win)| win.open && win.workspace == ws)
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
            let title = self.win_title(win.id);
            if pill_w > 34 {
                canvas.fill_aa_circle((bx + 11) as f32, (task_y + 18) as f32, 2.5, win.id.color(), 255);
                canvas.draw_text_clipped(bx + 19, task_y + 11, pill_w - 24, &title, fg, 1);
            } else {
                canvas.draw_text_clipped(bx + 5, task_y + 11, pill_w - 8, &title, fg, 1);
            }
            if is_top {
                canvas.fill_rounded_rect(bx + 14, task_y + 27, (pill_w - 28).max(8), 2, 1, accent);
            }
            bx += pill_w + 4;
        }

        let lang_x = (dock_x + dock_w - 144).max(bx + 8);
        // Workspace pill [W1..W3]
        let ws_x = lang_x - 122;
        let ws_hov = self.mouse_x >= ws_x
            && self.mouse_x <= ws_x + 28
            && self.mouse_y >= task_y + 6
            && self.mouse_y <= task_y + 30;
        if ws > 1 {
            canvas.blend_rounded_rect(ws_x, task_y + 6, 28, 24, 7, accent, 55);
            canvas.draw_rounded_rect_alpha(ws_x, task_y + 6, 28, 24, 7, accent, 140);
        } else {
            canvas.blend_rounded_rect(ws_x, task_y + 6, 28, 24, 7, 0xFFFFFF, if ws_hov { 26 } else { 13 });
            canvas.draw_rounded_rect_alpha(ws_x, task_y + 6, 28, 24, 7, 0xFFFFFF, 24);
        }
        canvas.draw_text_clipped(ws_x + 7, task_y + 11, 20, &format!("W{ws}"), 0xE2E8F0, 1);

        // Calendar / Notifications popup pill
        let cal_x = lang_x - 90;
        let cal_hov = self.mouse_x >= cal_x
            && self.mouse_x <= cal_x + 26
            && self.mouse_y >= task_y + 6
            && self.mouse_y <= task_y + 30;
        if self.cal_open {
            canvas.blend_rounded_rect(cal_x, task_y + 6, 26, 24, 7, accent, 70);
            canvas.draw_rounded_rect_alpha(cal_x, task_y + 6, 26, 24, 7, accent, 150);
        } else {
            canvas.blend_rounded_rect(cal_x, task_y + 6, 26, 24, 7, 0xFFFFFF, if cal_hov { 26 } else { 13 });
            canvas.draw_rounded_rect_alpha(cal_x, task_y + 6, 26, 24, 7, 0xFFFFFF, 24);
        }
        canvas.draw_rounded_rect_alpha(cal_x + 7, task_y + 11, 12, 13, 2, 0xCBD5E1, 220);
        canvas.blend_rect(cal_x + 8, task_y + 13, 10, 2, accent, 200);
        if !self.toast_history.is_empty() {
            canvas.fill_aa_circle((cal_x + 21) as f32, (task_y + 11) as f32, 2.2, accent, 245);
        }

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
        let tray_w = (dock_x + dock_w - 12 - tray_x).max(40);
        canvas.blend_rounded_rect(tray_x, task_y + 6, tray_w, 24, 7, 0xFFFFFF, 11);
        canvas.draw_rounded_rect_alpha(tray_x, task_y + 6, tray_w, 24, 7, 0xFFFFFF, 20);
        canvas.blend_rect(dock_x + dock_w - 9, task_y + 8, 2, 20, 0xFFFFFF, 46);
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

        // 5. Overlays: Start Menu, Control Center, Spotlight Launcher, Calendar, Context Menu, Toast
        if self.start_menu_open || (anim && self.start_anim > 0.03) {
            self.draw_start_menu(canvas, dock_x, task_y, dock_h, accent);
        }
        if self.cc_open || (anim && self.cc_anim > 0.03) {
            self.draw_control_center(canvas, w, h, accent);
        }
        if self.launcher_open || (anim && self.launcher_anim > 0.03) {
            self.draw_launcher(canvas, w, h, accent);
        }
        if self.cal_open {
            self.draw_calendar_popup(canvas, dock_x, task_y, dock_w, dock_h, accent);
        }
        let anim_ctx = if anim && self.ctx_anim > 0.04 {
            self.last_ctx_menu
        } else {
            None
        };
        if let Some((cx, cy)) = self.ctx_menu.or(anim_ctx) {
            self.draw_context_menu(canvas, cx, cy, accent);
        }
        if self.settings.persistent_island
            || self.ai.cu_active
            || (anim && (self.island_anim > 0.02 || self.cu_mini_anim > 0.02))
        {
            self.draw_idle_island(canvas, w, h, accent);
        }
        if let Some(toast) = self.toast.as_ref() {
            self.draw_toast(canvas, w, h, toast);
        }
        if self.switcher_open {
            self.draw_window_switcher(canvas, w, h, accent);
        }
        if self.locked {
            self.draw_lock_screen(canvas, w, h, accent);
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
                && !self.cal_open
                && !over_dock
            {
                for win in self.windows.iter().rev() {
                    if !win.open || win.minimized || win.workspace != ws {
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

        if self.settings.night_shift {
            canvas.apply_night_shift();
        }
        if self.settings.crt_scanlines {
            canvas.apply_crt_scanlines();
        }
        if self.screenshot_req.replace(false) {
            let p = std::env::temp_dir().join(format!("pizsoft_shot_{}.bmp", self.tick));
            let _ = canvas.write_bmp(&p);
        }
    }

    fn draw_aero_stars(&self, canvas: &mut Canvas, w: i32, h: i32, accent: u32) {
        let wf = w as f32;
        let hf = h as f32;
        let t = (self.tick as f32) * 0.018;

        // Longhorn Aurora gentle translucent ribbons
        let steps = 18i32;
        let step_w = (w / steps).max(16);
        for band in 0..2 {
            let base_y = hf * if band == 0 { 0.72 } else { 0.78 };
            let col = if band == 0 { 0x38BDF8 } else { accent };
            for i in 0..steps {
                let x0 = i * step_w;
                let phase = (i as f32) * 0.36 + t * (1.0 + band as f32 * 0.35);
                let y0 = base_y + phase.sin() * 14.0;
                let y1 = base_y + (phase + 0.36).sin() * 14.0;
                let a = (11.0 + 7.0 * (phase * 0.5).cos().abs()) as u8;
                canvas.draw_aa_line(x0 as f32, y0, (x0 + step_w) as f32, y1, 18.0, col, a);
            }
        }

        // Windows Longhorn 4074 (MILDesktop) Aero Stars:
        // Calm, softly glowing stars drifting gently across the desktop.
        for (i, s) in self.stars.iter().enumerate() {
            let pulse = 0.65 + 0.35 * (s.life * core::f32::consts::TAU).sin();
            let a = (pulse * 185.0) as u8;
            if a < 4 {
                continue;
            }
            let sx = s.x * wf;
            let sy = s.y * hf;
            let col = if i % 4 == 0 { 0xBAE6FD } else { 0xFFFFFF };
            let r = s.size * (0.88 + pulse * 0.22);

            // Soft Aero halo + crisp star core + gentle 4-point flare
            canvas.fill_aa_circle(sx, sy, r * 2.8, 0xBAE6FD, a / 5);
            canvas.fill_aa_circle(sx, sy, r, col, a);
            if r > 1.65 {
                let arm = r * 2.5;
                canvas.draw_aa_line(sx - arm, sy, sx + arm, sy, 0.85, 0xFFFFFF, a / 2);
                canvas.draw_aa_line(sx, sy - arm, sx, sy + arm, 0.85, 0xFFFFFF, a / 2);
            }
        }
    }

    fn draw_vista_sidebar(&self, canvas: &mut Canvas, w: i32, h: i32, accent: u32) {
        let lang = self.settings.lang;
        let (glass_a, border_a) = self.settings.glass_alphas();
        let top_bar = if self.settings.taskbar_top {
            self.dock_reserve()
        } else {
            0
        };
        let gx = w - 176;
        let gy = top_bar + 14;
        let gw = 164;

        // Subtle Vista translucent sidebar strip
        let strip_h = (h - top_bar - 44).max(120);
        canvas.blend_rounded_rect(gx - 6, top_bar + 6, gw + 12, strip_h, 14, 0x090D16, (glass_a / 4).max(32));
        canvas.draw_rounded_rect_alpha(gx - 6, top_bar + 6, gw + 12, strip_h, 14, 0xFFFFFF, 16);

        // 1. Gadget 1: Vista Analog Clock (164 x 146)
        canvas.draw_soft_shadow(gx, gy, gw, 146, 14);
        canvas.blend_rounded_rect(gx, gy, gw, 146, 14, 0x0C111C, glass_a.saturating_sub(18));
        canvas.draw_rounded_rect_alpha(gx, gy, gw, 146, 14, 0xFFFFFF, border_a);
        let cx = (gx + gw / 2) as f32;
        let cy = (gy + 66) as f32;
        let rad = 48.0;
        canvas.fill_aa_circle(cx, cy, rad + 2.0, accent, 50);
        canvas.fill_aa_circle(cx, cy, rad, 0x080C15, 225);
        canvas.fill_aa_circle(cx, cy, rad - 2.0, 0x161E31, 95);
        for m in 0..12 {
            let a = (m as f32) * (core::f32::consts::TAU / 12.0) - core::f32::consts::FRAC_PI_2;
            let r0 = if m % 3 == 0 { rad - 9.0 } else { rad - 5.5 };
            let r1 = rad - 2.5;
            canvas.draw_aa_line(
                cx + a.cos() * r0,
                cy + a.sin() * r0,
                cx + a.cos() * r1,
                cy + a.sin() * r1,
                if m % 3 == 0 { 1.8 } else { 1.1 },
                0xCBD5E1,
                215,
            );
        }
        // Parse HH:MM:SS or HH:MM from clock_str
        let parts: Vec<f32> = self
            .clock_str
            .split(':')
            .filter_map(|p| p.trim().parse::<f32>().ok())
            .collect();
        let hr = parts.first().copied().unwrap_or(12.0) % 12.0;
        let mn = parts.get(1).copied().unwrap_or(0.0);
        let sc = parts.get(2).copied().unwrap_or((self.tick % 60) as f32);
        let hr_a = ((hr + mn / 60.0) / 12.0) * core::f32::consts::TAU - core::f32::consts::FRAC_PI_2;
        let mn_a = ((mn + sc / 60.0) / 60.0) * core::f32::consts::TAU - core::f32::consts::FRAC_PI_2;
        let sc_a = (sc / 60.0) * core::f32::consts::TAU - core::f32::consts::FRAC_PI_2;
        canvas.draw_aa_line(cx, cy, cx + hr_a.cos() * 24.0, cy + hr_a.sin() * 24.0, 2.6, 0xF8FAFC, 245);
        canvas.draw_aa_line(cx, cy, cx + mn_a.cos() * 35.0, cy + mn_a.sin() * 35.0, 1.8, 0xBAE6FD, 245);
        canvas.draw_aa_line(cx, cy, cx + sc_a.cos() * 40.0, cy + sc_a.sin() * 40.0, 1.1, accent, 245);
        canvas.fill_aa_circle(cx, cy, 3.2, accent, 255);
        let cw_txt = canvas.measure_text(&self.clock_str, 1);
        canvas.draw_text_clipped(gx + (gw - cw_txt) / 2, gy + 123, gw - 12, &self.clock_str, 0xE2E8F0, 1);

        // 2. Gadget 2: Vista CPU / RAM Dual Meters (164 x 96)
        let my = gy + 154;
        canvas.draw_soft_shadow(gx, my, gw, 96, 14);
        canvas.blend_rounded_rect(gx, my, gw, 96, 14, 0x0C111C, glass_a.saturating_sub(18));
        canvas.draw_rounded_rect_alpha(gx, my, gw, 96, 14, 0xFFFFFF, border_a);
        canvas.draw_text_clipped(gx + 12, my + 8, gw - 24, tr(lang, "Монитор Vista", "Vista Meters"), 0x94A3B8, 1);
        for (idx, (lbl, pct, col)) in [
            ("CPU", self.cpu_pct as f32, accent),
            ("RAM", self.mem.percent as f32, 0xF59E0B),
        ]
        .iter()
        .enumerate()
        {
            let mcx = (gx + 43 + (idx as i32) * 78) as f32;
            let mcy = (my + 58) as f32;
            canvas.fill_aa_circle(mcx, mcy, 24.0, 0x1E293B, 220);
            canvas.fill_aa_circle(mcx, mcy, 20.0, 0x0B101B, 245);
            let frac = (pct / 100.0).clamp(0.0, 1.0);
            let steps = ((frac * 18.0).round() as usize).max(1);
            for s in 0..steps {
                let ang = -core::f32::consts::PI * 1.15 + (s as f32 / 18.0) * core::f32::consts::PI * 1.3;
                canvas.fill_aa_circle(mcx + ang.cos() * 21.8, mcy + ang.sin() * 21.8, 2.2, *col, 240);
            }
            let ptxt = format!("{}%", *pct as u8);
            let pw_t = canvas.measure_text(&ptxt, 1);
            canvas.draw_text_clipped(mcx as i32 - pw_t / 2, (mcy as i32) - 5, 42, &ptxt, 0xF8FAFC, 1);
            let lw_t = canvas.measure_text(lbl, 1);
            canvas.draw_text_clipped(mcx as i32 - lw_t / 2, (mcy as i32) + 23, 40, lbl, 0x94A3B8, 1);
        }

        // 3. Gadget 3: Sticky Note (164 x 104)
        let sy = gy + 258;
        if sy + 104 < h - 36 {
            canvas.draw_soft_shadow(gx, sy, gw, 104, 14);
            canvas.blend_rounded_rect(gx, sy, gw, 104, 14, 0x261F0B, glass_a.saturating_sub(12));
            let s_border = if self.sticky_focused { 0xFBBF24 } else { 0xD97706 };
            canvas.draw_rounded_rect_alpha(gx, sy, gw, 104, 14, s_border, if self.sticky_focused { 185 } else { 95 });
            canvas.draw_text_clipped(
                gx + 10,
                sy + 7,
                gw - 20,
                if self.sticky_focused {
                    tr(lang, "Заметка (ввод...)", "Note (typing...)")
                } else {
                    tr(lang, "Стикер (клик)", "Sticky Note")
                },
                0xFCD34D,
                1,
            );
            for (row, line) in self.sticky_note.split('\n').take(4).enumerate() {
                canvas.draw_text_clipped(gx + 10, sy + 26 + (row as i32) * 18, gw - 20, line, 0xFEF3C7, 1);
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

        let fx_y = py + 234;
        let fx_items = [
            ("Vista", self.settings.vista_sidebar, 0x38BDF8),
            ("Stars", self.settings.aero_stars, 0xA855F7),
            ("Night", self.settings.night_shift, 0xF59E0B),
            ("CRT", self.settings.crt_scanlines, 0x22C55E),
        ];
        for (idx, (lbl, on, col)) in fx_items.iter().enumerate() {
            let fx = px + 10 + (idx as i32) * 65;
            if *on {
                canvas.blend_rounded_rect(fx, fx_y, 61, 26, 8, *col, 68);
                canvas.draw_rounded_rect_alpha(fx, fx_y, 61, 26, 8, *col, 165);
            } else {
                canvas.blend_rounded_rect(fx, fx_y, 61, 26, 8, 0xFFFFFF, 12);
                canvas.draw_rounded_rect_alpha(fx, fx_y, 61, 26, 8, 0xFFFFFF, 22);
            }
            canvas.draw_text_clipped(fx + 8, fx_y + 7, 46, lbl, if *on { 0xFFFFFF } else { 0xCBD5E1 }, 1);
        }

        let foot_y = py + 266;
        canvas.blend_rounded_rect(px + 10, foot_y, pw - 86, 24, 8, 0xFFFFFF, 11);
        canvas.draw_rounded_rect_alpha(px + 10, foot_y, pw - 86, 24, 8, 0xFFFFFF, 22);
        canvas.fill_aa_circle((px + 22) as f32, (foot_y + 12) as f32, 3.0, 0x3FB950, 255);
        canvas.draw_text_clipped(
            px + 30,
            foot_y + 5,
            pw - 110,
            &format!("CPU {}%  RAM {}%", self.cpu_pct, self.mem.percent),
            0xE2E8F0,
            1,
        );
        canvas.blend_rounded_rect(px + pw - 70, foot_y, 60, 24, 8, accent, 58);
        canvas.draw_rounded_rect_alpha(px + pw - 70, foot_y, 60, 24, 8, accent, 150);
        canvas.draw_text_clipped(px + pw - 58, foot_y + 5, 44, "Shot", 0xFFFFFF, 1);
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
        let cu_on = self.ai.cu_active || self.cu_mini_anim > 0.04;
        let t_raw = if self.settings.animations {
            self.island_anim
        } else if self.settings.persistent_island || cu_on {
            1.0
        } else {
            0.0
        };
        let t = t_raw.clamp(0.0, 1.0);
        if t <= 0.02 {
            return;
        }
        let txt = if cu_on {
            String::from("PizAI is using your computer")
        } else {
            self.idle_island_text()
        };
        let (_, base_y, full_tw, th) = self.idle_island_rect(w, h, &txt);

        // Liquid jelly morph: emerges stretched out of y=0 (or bottom edge if taskbar_top)
        let stretch = t_raw.clamp(0.0, 1.12);
        let eff_w = (full_tw as f32 * (0.36 + 0.64 * (stretch * stretch).min(1.08)))
            .round()
            .clamp(56.0, (w - 20) as f32) as i32;
        let eff_h = (th as f32 * (0.58 + 0.42 * stretch)).round().clamp(14.0, 34.0) as i32;
        let tx = (w - eff_w) / 2;
        let ty = if self.settings.taskbar_top {
            let from_edge = (h as f32 - base_y as f32) * (1.0 - stretch);
            (base_y as f32 + from_edge).round() as i32
        } else {
            ((base_y as f32 + eff_h as f32 * 0.45) * stretch - eff_h as f32 * 0.45)
                .round()
                .max(0.0) as i32
        };

        // Liquid neck bridge connecting the top edge to the emerging jelly pill
        if !self.settings.taskbar_top && t < 0.80 && ty > 0 {
            let neck_f = 1.0 - (t / 0.80);
            let neck_w = (eff_w as f32 * (0.45 + 0.35 * neck_f)).round().max(26.0) as i32;
            let neck_x = (w - neck_w) / 2;
            canvas.blend_rounded_rect(neck_x, -4, neck_w, ty + eff_h / 2 + 6, 10, 0x090D16, (230.0 * t) as u8);
            let flare_w = (neck_w + (24.0 * neck_f) as i32).min(eff_w + 16);
            canvas.blend_rounded_rect((w - flare_w) / 2, -6, flare_w, 12, 6, 0x090D16, (220.0 * t) as u8);
        }

        let hov = self.settings.persistent_island
            && self.toast.is_none()
            && self.mouse_x >= tx
            && self.mouse_x <= tx + eff_w
            && self.mouse_y >= ty
            && self.mouse_y <= ty + eff_h;

        if t > 0.45 {
            canvas.draw_soft_shadow(tx, ty, eff_w, eff_h, 13);
        }
        canvas.blend_rounded_rect(tx, ty, eff_w, eff_h, 13, 0x090D16, (238.0 * t) as u8);
        let border_col = if cu_on {
            0xA855F7
        } else if hov {
            accent
        } else {
            0xFFFFFF
        };
        let border_a = if cu_on {
            (180.0 * t) as u8
        } else if hov {
            (165.0 * t) as u8
        } else {
            (48.0 * t) as u8
        };
        canvas.draw_rounded_rect_alpha(tx, ty, eff_w, eff_h, 13, border_col, border_a);
        canvas.blend_rect(tx + 16, ty + 1, (eff_w - 32).max(0), 1, 0xFFFFFF, (30.0 * t) as u8);

        let pulse = if self.settings.animations {
            (205.0 + 50.0 * ((self.tick as f32) * 0.08).sin()).clamp(150.0, 255.0)
        } else {
            255.0
        };
        if cu_on {
            canvas.fill_aa_circle((tx + 14) as f32, (ty + eff_h / 2) as f32, 5.2, 0xA855F7, (65.0 * t) as u8);
            canvas.fill_aa_circle((tx + 14) as f32, (ty + eff_h / 2) as f32, 2.8, 0xC084FC, (pulse * t) as u8);
        } else {
            match self.island_mode {
                IslandMode::Music => {
                    let cols = [0x22C55E, 0x38BDF8, 0xA855F7, 0xEC4899, 0xF59E0B];
                    for (b, &col) in cols.iter().enumerate() {
                        let phase = ((self.tick as f32) * 0.24 + (b as f32) * 1.15).sin();
                        let bh = if self.settings.animations {
                            ((5.0 + 6.0 * (0.5 + 0.5 * phase)) * t).round() as i32
                        } else {
                            7
                        }
                        .max(2);
                        canvas.fill_rect(tx + 11 + (b as i32) * 3, ty + 18 - bh, 2, bh, col);
                    }
                }
                IslandMode::Pomodoro => {
                    let col = 0xF97316;
                    canvas.fill_aa_circle((tx + 14) as f32, (ty + 13) as f32, 4.8, col, (55.0 * t) as u8);
                    canvas.fill_aa_circle((tx + 14) as f32, (ty + 13) as f32, 2.7, col, (pulse * t) as u8);
                    let total = (25 * 60 * 30) as f32;
                    let frac = 1.0 - (self.pomo_ticks as f32 / total).clamp(0.0, 1.0);
                    let pw = (((eff_w - 30) as f32) * frac.max(0.04)).round() as i32;
                    canvas.blend_rect(tx + 15, ty + eff_h - 3, pw, 1, col, (190.0 * t) as u8);
                }
                IslandMode::Status => {
                    let dot_col = if self.dnd { 0xA855F7 } else { 0x3FB950 };
                    canvas.fill_aa_circle((tx + 14) as f32, (ty + 13) as f32, 4.8, dot_col, (55.0 * t) as u8);
                    canvas.fill_aa_circle((tx + 14) as f32, (ty + 13) as f32, 2.7, dot_col, (pulse * t) as u8);
                }
            }
        }
        if t > 0.35 {
            let text_x = if !cu_on && self.island_mode == IslandMode::Music {
                tx + 31
            } else {
                tx + 24
            };
            let right_pad = if cu_on { 32 } else { 66 };
            canvas.draw_text_clipped(text_x, ty + 6, (eff_w - right_pad).max(10), &txt, 0xE2E8F0, 1);
            if !cu_on {
                let mode_idx = match self.island_mode {
                    IslandMode::Status => 0,
                    IslandMode::Music => 1,
                    IslandMode::Pomodoro => 2,
                };
                canvas.blend_rounded_rect(tx + eff_w - 34, ty + 5, 26, 16, 8, 0xFFFFFF, (22.0 * t) as u8);
                for d in 0..3 {
                    let col = if d == mode_idx { accent } else { 0x64748B };
                    let rad = if d == mode_idx { 2.3 } else { 1.5 };
                    canvas.fill_aa_circle(
                        (tx + eff_w - 28 + d * 7) as f32,
                        (ty + 13) as f32,
                        rad,
                        col,
                        (220.0 * t) as u8,
                    );
                }
            }
        }

        // Liquid Mini Dynamic Island dripping right below main island during AI Computer Use
        let mt_raw = if self.settings.animations {
            self.cu_mini_anim
        } else if self.ai.cu_active {
            1.0
        } else {
            0.0
        };
        let mt = mt_raw.clamp(0.0, 1.0);
        if mt > 0.03 {
            let mtxt = if self.ai.cu_action_text.is_empty() {
                "Thinking..."
            } else {
                self.ai.cu_action_text.as_str()
            };
            let tw_micro = canvas.measure_text_micro(mtxt);
            let full_mw = (tw_micro + 28).clamp(104, (w - 36).max(104));
            let mh = 18;
            let mw = (full_mw as f32 * (0.35 + 0.65 * mt_raw.clamp(0.0, 1.08)))
                .round()
                .max(36.0) as i32;
            let mx = (w - mw) / 2;
            let my = if self.settings.taskbar_top {
                (ty as f32 - (mh + 5) as f32 * mt_raw.clamp(0.0, 1.08)).round() as i32
            } else {
                ty + eff_h - mh / 2 + ((mh / 2 + 5) as f32 * mt_raw.clamp(0.0, 1.08)).round() as i32
            };

            // Gooey droplet neck bridging main island <-> mini island while emerging
            if !self.settings.taskbar_top && mt < 0.82 {
                let neck_w = (mw as f32 * 0.42).round().max(16.0) as i32;
                canvas.blend_rounded_rect(
                    (w - neck_w) / 2,
                    ty + eff_h - 4,
                    neck_w,
                    (my - (ty + eff_h) + mh / 2 + 4).max(6),
                    6,
                    0x090D16,
                    (225.0 * mt) as u8,
                );
            }
            if mt > 0.45 {
                canvas.draw_soft_shadow(mx, my, mw, mh, 9);
            }
            canvas.blend_rounded_rect(mx, my, mw, mh, 9, 0x090D16, (234.0 * mt) as u8);
            canvas.draw_rounded_rect_alpha(mx, my, mw, mh, 9, 0x34D399, (150.0 * mt) as u8);
            if mt > 0.35 {
                canvas.fill_aa_circle((mx + 10) as f32, (my + 9) as f32, 2.3, 0x34D399, (pulse * mt) as u8);
                canvas.draw_text_micro_clipped(mx + 17, my + 4, (mw - 24).max(10), mtxt, 0xCBD5E1);
            }
        }
    }

    fn draw_window_switcher(&self, canvas: &mut Canvas, w: i32, h: i32, accent: u32) {
        let lang = self.settings.lang;
        let ids: Vec<AppId> = self.windows.iter().rev().filter(|win| win.open).map(|win| win.id).collect();
        if ids.is_empty() {
            return;
        }
        let shown = ids.len().min(7);
        let active = self.switcher_idx % ids.len();
        let (bx, by, bw, bh, cw, gap) = self.switcher_rect(w, h, shown);

        canvas.blend_rect(0, 0, w, h, 0x050811, 115);
        canvas.draw_soft_shadow(bx, by, bw, bh, 16);
        canvas.blend_rounded_rect(bx, by, bw, bh, 16, 0x101624, 236);
        canvas.draw_rounded_rect_alpha(bx, by, bw, bh, 16, 0xFFFFFF, 52);
        canvas.draw_text_clipped(
            bx + 14,
            by + 10,
            bw - 28,
            tr(lang, "Переключение окон  (Alt/Ctrl+Tab)", "Window Switcher  (Alt/Ctrl+Tab)"),
            0x94A3B8,
            1,
        );

        for (i, &id) in ids.iter().take(7).enumerate() {
            let cx = bx + 14 + (i as i32) * (cw + gap);
            let cy = by + 32;
            let sel = i == active;
            if sel {
                canvas.blend_rounded_rect(cx, cy, cw, 68, 11, accent, 58);
                canvas.draw_rounded_rect_alpha(cx, cy, cw, 68, 11, accent, 215);
            } else {
                canvas.blend_rounded_rect(cx, cy, cw, 68, 11, 0xFFFFFF, 12);
                canvas.draw_rounded_rect_alpha(cx, cy, cw, 68, 11, 0xFFFFFF, 26);
            }
            canvas.fill_aa_circle((cx + cw / 2) as f32, (cy + 24) as f32, 11.0, id.color(), 215);
            let title = self.win_title(id);
            canvas.draw_text_clipped(
                cx + 6,
                cy + 46,
                cw - 12,
                &title,
                if sel { 0xFFFFFF } else { 0xCBD5E1 },
                1,
            );
        }
    }

    fn draw_lock_screen(&self, canvas: &mut Canvas, w: i32, h: i32, accent: u32) {
        let lang = self.settings.lang;
        canvas.blend_rect(0, 0, w, h, 0x050912, 216);
        canvas.fill_aa_circle((w / 2) as f32, (h / 2 - 20) as f32, 160.0, accent, 26);
        canvas.fill_aa_circle((w / 2) as f32, (h / 2 - 20) as f32, 85.0, accent, 22);

        let top_txt = if self.dnd {
            format!("pizdOS 2.1(00)  ·  DND  ·  CPU {}%", self.cpu_pct)
        } else {
            format!("pizdOS 2.1(00)  ·  CPU {}%  ·  RAM {}%", self.cpu_pct, self.mem.percent)
        };
        let top_w = (canvas.measure_text(&top_txt, 1) + 28).min(w - 24);
        canvas.blend_rounded_rect((w - top_w) / 2, 16, top_w, 24, 12, 0x0F172A, 210);
        canvas.draw_rounded_rect_alpha((w - top_w) / 2, 16, top_w, 24, 12, 0xFFFFFF, 44);
        canvas.draw_text_clipped((w - top_w) / 2 + 14, 21, top_w - 28, &top_txt, 0xCBD5E1, 1);

        let clk_w = canvas.measure_text(&self.clock_str, 3);
        canvas.draw_text_clipped((w - clk_w) / 2, h / 2 - 64, w - 32, &self.clock_str, 0xF8FAFC, 3);

        let sub = format!("{}  ·  pizdOS 2.1(00)", self.settings.device_name);
        let sub_w = canvas.measure_text(&sub, 1);
        canvas.draw_text_clipped((w - sub_w) / 2, h / 2 - 4, w - 32, &sub, 0x94A3B8, 1);

        let hint = tr(lang, "Клик или Enter / Пробел для входа", "Click or press Enter / Space to unlock");
        let hw = (canvas.measure_text(hint, 1) + 34).clamp(180, w - 24);
        let hx = (w - hw) / 2;
        let hy = (h / 2 + 34).min(h - 46);
        canvas.blend_rounded_rect(hx, hy, hw, 28, 14, accent, 52);
        canvas.draw_rounded_rect_alpha(hx, hy, hw, 28, 14, accent, 165);
        canvas.fill_aa_circle((hx + 14) as f32, (hy + 14) as f32, 3.0, 0x3FB950, 255);
        canvas.draw_text_clipped(hx + 24, hy + 7, hw - 32, hint, 0xF8FAFC, 1);
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
            tr(lang, "pizdOS 2.1(00)", "pizdOS 2.1(00)"),
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
            AppId::Ai => {
                canvas.draw_aa_line(cx, cy - 9.0, cx, cy + 9.0, 2.0, 0xC084FC, 255);
                canvas.draw_aa_line(cx - 9.0, cy, cx + 9.0, cy, 2.0, 0xC084FC, 255);
                canvas.fill_aa_circle(cx, cy, 3.2, 0xF8FAFC, 255);
            }
            AppId::Browser => {
                canvas.fill_aa_circle(cx, cy, 8.5, 0x0EA5E9, 55);
                canvas.draw_aa_line(cx - 8.5, cy, cx + 8.5, cy, 1.4, 0x38BDF8, 240);
                canvas.draw_aa_line(cx, cy - 8.5, cx, cy + 8.5, 1.4, 0x38BDF8, 240);
                canvas.draw_aa_line(cx - 6.0, cy - 5.5, cx + 6.0, cy - 5.5, 1.1, 0xBAE6FD, 190);
                canvas.draw_aa_line(cx - 6.0, cy + 5.5, cx + 6.0, cy + 5.5, 1.1, 0xBAE6FD, 190);
            }
            AppId::Game2048 => {
                let qx = cx as i32 - 8;
                let qy = cy as i32 - 8;
                canvas.fill_rounded_rect(qx, qy, 7, 7, 2, 0xFDE68A);
                canvas.fill_rounded_rect(qx + 9, qy, 7, 7, 2, 0xF59E0B);
                canvas.fill_rounded_rect(qx, qy + 9, 7, 7, 2, 0xFB923C);
                canvas.fill_rounded_rect(qx + 9, qy + 9, 7, 7, 2, 0xEF4444);
            }
            AppId::Files => {
                canvas.fill_rounded_rect(bx + 10, by + 11, 9, 4, 1, 0x60A5FA);
                canvas.fill_rounded_rect(bx + 10, by + 14, 20, 14, 3, 0x3B82F6);
                canvas.blend_rect(bx + 12, by + 16, 16, 2, 0xBFDBFE, 120);
            }
            AppId::PizView => {
                canvas.draw_rounded_rect_alpha(bx + 10, by + 11, 20, 18, 3, 0x7DD3FC, 230);
                canvas.fill_aa_circle(cx + 4.5, cy - 4.0, 2.2, 0xFBBF24, 255);
                canvas.draw_aa_line(cx - 7.0, cy + 5.0, cx - 1.5, cy - 1.5, 1.8, 0x38BDF8, 255);
                canvas.draw_aa_line(cx - 1.5, cy - 1.5, cx + 3.0, cy + 3.0, 1.8, 0x38BDF8, 255);
                canvas.draw_aa_line(cx + 1.5, cy + 2.0, cx + 6.5, cy + 5.0, 1.8, 0x7DD3FC, 240);
            }
            AppId::Minesweeper => {
                canvas.draw_aa_line(cx - 8.5, cy, cx + 8.5, cy, 1.6, 0x94A3B8, 220);
                canvas.draw_aa_line(cx, cy - 8.5, cx, cy + 8.5, 1.6, 0x94A3B8, 220);
                canvas.draw_aa_line(cx - 6.0, cy - 6.0, cx + 6.0, cy + 6.0, 1.4, 0x94A3B8, 200);
                canvas.draw_aa_line(cx + 6.0, cy - 6.0, cx - 6.0, cy + 6.0, 1.4, 0x94A3B8, 200);
                canvas.fill_aa_circle(cx, cy, 5.2, 0x1E293B, 255);
                canvas.fill_aa_circle(cx - 1.5, cy - 1.5, 1.6, 0xF87171, 240);
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
        if focused {
            // Longhorn Aero Glass upper-half sheen + diagonal specular streak
            canvas.blend_rect(win.x + 10, win.y + 2, (win.w - 20).max(0), 11, 0xFFFFFF, 10);
            let sx = (win.x + (win.w / 3)).min(win.x + win.w - 40);
            canvas.draw_aa_line(sx as f32, (win.y + 2) as f32, (sx + 26) as f32, (win.y + 25) as f32, 8.0, 0xFFFFFF, 12);
        }
        canvas.blend_rect(win.x + 14, win.y + 1, (win.w - 28).max(0), 1, 0xFFFFFF, if focused { 40 } else { 14 });
        if focused {
            canvas.blend_rect(win.x + 24, win.y, (win.w - 48).max(0), 1, accent, 82);
        }
        canvas.blend_rect(win.x + 1, win.y + 27, win.w - 2, 1, 0xFFFFFF, 16);
        let btn_y = win.y + 5;
        let cy = btn_y + 9;
        let hover_y = is_onscreen && self.mouse_y >= btn_y && self.mouse_y <= btn_y + 18;
        if self.settings.macos_buttons {
            canvas.fill_aa_circle((win.x + 13) as f32, cy as f32, 5.2, 0xFF5F57, 255);
            canvas.fill_aa_circle((win.x + 31) as f32, cy as f32, 5.2, 0xFEBC2E, 255);
            canvas.fill_aa_circle((win.x + 49) as f32, cy as f32, 5.2, 0x28C840, 255);
            let title = self.win_title(win.id);
            canvas.draw_text_clipped(
                win.x + 66,
                win.y + 7,
                (win.w - 80).max(40),
                &title,
                if focused { 0xF1F5F9 } else { 0x7D8590 },
                1,
            );
        } else {
            canvas.fill_aa_circle(
                (win.x + 14) as f32,
                (win.y + 14) as f32,
                3.2,
                win.id.color(),
                if focused { 245 } else { 125 },
            );
            let title = self.win_title(win.id);
            canvas.draw_text_clipped(
                win.x + 24,
                win.y + 7,
                win.w - 106,
                &title,
                if focused { 0xF1F5F9 } else { 0x7D8590 },
                1,
            );

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
        }

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
            AppId::Game2048 => self.game2048.draw(canvas, win.x, win.y, win.w, win.h, accent, lang),
            AppId::PizView => self.pizview.draw(canvas, win.x, win.y, win.w, win.h, accent, lang),
            AppId::Minesweeper => self.minesweeper.draw(canvas, win.x, win.y, win.w, win.h, accent, lang),
            AppId::Matrix => self.matrix.draw(canvas, win.x, win.y, win.w, win.h, self.tick, lang),
            AppId::Ai => self.ai.draw(canvas, win.x, win.y, win.w, win.h, accent, lang),
            AppId::Browser => self.browser.draw(canvas, win.x, win.y, win.w, win.h, accent, lang),
            AppId::Guest(gid) => self.draw_guest_contents(canvas, win, gid, accent),
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

    fn draw_guest_contents(&self, canvas: &mut Canvas, win: &Window, gid: u32, accent: u32) {
        let lang = self.settings.lang;
        let cx = win.x + 4;
        let cy = win.y + 32;
        let cw = (win.w - 8).max(1);
        let ch = (win.h - 36).max(1);
        canvas.fill_rect(cx, cy, cw, ch, 0x090D16);
        let Some(surf) = self.display_srv.window(gid) else {
            canvas.draw_text_clipped(
                cx + 12,
                cy + 12,
                cw - 24,
                tr(lang, "Ожидание кадра...", "Waiting for frame..."),
                0x94A3B8,
                1,
            );
            return;
        };
        let bw = cw.min(surf.w).max(0);
        let bh = ch.min(surf.h).max(0);
        if bw > 0 && bh > 0 && !surf.pixels.is_empty() {
            if bw == surf.w {
                canvas.blit_rgb_rect(cx, cy, bw, bh, &surf.pixels);
            } else {
                let sw = surf.w.max(1) as usize;
                for row in 0..bh as usize {
                    let off = row * sw;
                    if off + bw as usize <= surf.pixels.len() {
                        canvas.blit_rgb_rect(
                            cx,
                            cy + row as i32,
                            bw,
                            1,
                            &surf.pixels[off..off + bw as usize],
                        );
                    }
                }
            }
        }
        let tag = match surf.proto {
            GuestProto::X11 => "X11",
            GuestProto::Wayland => "Wayland",
        };
        let badge_w = canvas.measure_text(tag, 1) + 12;
        let bx = cx + cw - badge_w - 6;
        let by = cy + 6;
        if cw > badge_w + 20 && ch > 28 {
            canvas.blend_rounded_rect(bx, by, badge_w, 16, 6, 0x0F172A, 190);
            canvas.draw_rounded_rect_alpha(bx, by, badge_w, 16, 6, accent, 150);
            canvas.draw_text_clipped(bx + 6, by + 2, badge_w - 8, tag, accent, 1);
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
        let rgy = y + 154;
        let rgh = 26;
        canvas.fill_rounded_rect(x, rgy, iw, rgh, 6, 0x0A0F16);
        canvas.draw_rounded_rect_alpha(x, rgy, iw, rgh, 6, 0xFFFFFF, 20);
        for (i, &val) in self.ram_history.iter().enumerate() {
            let bh = (((val as i32) * (rgh - 4)) / 100).max(2);
            let bx = x + 2 + (i as i32) * bar_w;
            if bx + bar_w < x + iw {
                canvas.fill_vgradient_rounded_rect(
                    bx,
                    rgy + rgh - 2 - bh,
                    (bar_w - 1).max(1),
                    bh,
                    1,
                    0xFBBF24,
                    0x78350F,
                );
            }
        }

        canvas.draw_text_clipped(
            x,
            y + 186,
            iw,
            &format!("{}: {}", tr(lang, "Процессы", "Processes"), self.procs.len()),
            0x8B949E,
            1,
        );
        for (i, p) in self.procs.iter().take(2).enumerate() {
            canvas.draw_text_clipped(
                x,
                y + 202 + (i as i32) * 14,
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
        let lang = self.settings.lang;
        let x = win.x + 14;
        let y = win.y + 34;
        let iw = win.w - 28;

        canvas.draw_text_clipped(x, y, iw, &format!("CWD: {}", self.cwd.display()), 0x79C0FF, 1);

        let tb_y = win.y + 50;
        let sort_lbl = if self.file_sort_desc { "Z-A" } else { "A-Z" };
        canvas.draw_button(win.x + 14, tb_y, 34, 20, "..", 0x1E293B, 0xE2E8F0, 0x475569);
        canvas.draw_button(win.x + 52, tb_y, 50, 20, sort_lbl, 0x1E293B, 0xE2E8F0, 0x475569);
        canvas.draw_button(
            win.x + 106,
            tb_y,
            58,
            20,
            tr(lang, "+Файл", "+File"),
            0x14532D,
            0x86EFAC,
            0x22C55E,
        );
        canvas.draw_button(
            win.x + 168,
            tb_y,
            56,
            20,
            tr(lang, "+Папк", "+Dir"),
            0x1E3A8A,
            0x93C5FD,
            0x3B82F6,
        );
        canvas.draw_button(
            win.x + 228,
            tb_y,
            50,
            20,
            tr(lang, "Удал.", "Del"),
            0x7F1D1D,
            0xFCA5A5,
            0xEF4444,
        );

        canvas.fill_rounded_rect(x - 2, win.y + 74, iw + 4, (win.h - 90).max(40), 8, 0x0C111C);
        canvas.draw_rounded_rect_alpha(x - 2, win.y + 74, iw + 4, (win.h - 90).max(40), 8, 0xFFFFFF, 20);

        let max_rows = ((win.h - 92) / 18).max(1) as usize;
        for (i, item) in self.files.iter().take(max_rows).enumerate() {
            let ry = win.y + 76 + (i as i32) * 18;
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

    fn draw_calendar_popup(
        &self,
        canvas: &mut Canvas,
        dock_x: i32,
        task_y: i32,
        dock_w: i32,
        dock_h: i32,
        accent: u32,
    ) {
        let lang = self.settings.lang;
        let (glass_a, border_a) = self.settings.glass_alphas();
        let pw = 290;
        let ph = 310;
        let px = (dock_x + dock_w - pw - 8).max(8);
        let py = if self.settings.taskbar_top {
            task_y + dock_h + 6
        } else {
            (task_y - ph - 8).max(8)
        };

        canvas.draw_soft_shadow(px, py, pw, ph, 14);
        canvas.blend_rounded_rect(px, py, pw, ph, 14, 0x0B0F1A, glass_a);
        canvas.draw_rounded_rect_alpha(px, py, pw, ph, 14, 0xFFFFFF, border_a);

        let secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;
        let days = secs / 86_400;
        let z = days + 719_468;
        let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
        let doe = (z - era * 146_097) as u32;
        let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
        let y = yoe as i64 + era * 400;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let d = doy - (153 * mp + 2) / 5 + 1;
        let m = if mp < 10 { mp + 3 } else { mp - 9 };
        let year = y + if m <= 2 { 1 } else { 0 };
        let first_wd = ((days - (d as i64 - 1) + 3).rem_euclid(7)) as u32;
        let leap = (year % 4 == 0 && year % 100 != 0) || year % 400 == 0;
        let dim: u32 = match m {
            1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
            4 | 6 | 9 | 11 => 30,
            2 if leap => 29,
            2 => 28,
            _ => 30,
        };

        canvas.draw_text_clipped(
            px + 12,
            py + 10,
            180,
            &format!("{year}-{m:02}-{d:02} UTC  |  W{}", self.active_workspace),
            0xF8FAFC,
            1,
        );
        canvas.draw_text_clipped(px + pw - 68, py + 10, 56, &self.clock_str, accent, 1);

        let wdays = ["Mo", "Tu", "We", "Th", "Fr", "Sa", "Su"];
        for (c, lbl) in wdays.iter().enumerate() {
            canvas.draw_text_clipped(px + 16 + (c as i32) * 38, py + 28, 30, lbl, 0x94A3B8, 1);
        }
        for day in 1..=dim {
            let slot = first_wd + day - 1;
            let row = (slot / 7) as i32;
            let col = (slot % 7) as i32;
            let cx = px + 12 + col * 38;
            let cy = py + 44 + row * 18;
            if day == d {
                canvas.fill_rounded_rect(cx, cy - 1, 26, 16, 6, accent);
            }
            canvas.draw_text_clipped(
                cx + 4,
                cy + 2,
                22,
                &format!("{day:2}"),
                if day == d { 0x0B0F1A } else { 0xCBD5E1 },
                1,
            );
        }

        canvas.blend_rect(px + 10, py + 150, pw - 20, 1, 0xFFFFFF, 22);
        canvas.draw_text_clipped(
            px + 12,
            py + 157,
            180,
            tr(lang, "Уведомления", "Notifications"),
            0xE2E8F0,
            1,
        );
        canvas.draw_button(
            px + pw - 70,
            py + 154,
            60,
            18,
            tr(lang, "Очист.", "Clear"),
            0x1E293B,
            0xCBD5E1,
            0x475569,
        );
        if self.toast_history.is_empty() {
            canvas.draw_text_clipped(
                px + 14,
                py + 186,
                pw - 28,
                tr(lang, "История пуста", "No recent notifications"),
                0x64748B,
                1,
            );
        } else {
            for (i, msg) in self.toast_history.iter().rev().take(4).enumerate() {
                let ny = py + 178 + (i as i32) * 21;
                canvas.blend_rounded_rect(px + 10, ny, pw - 20, 18, 6, 0xFFFFFF, 10);
                canvas.draw_text_clipped(px + 16, ny + 3, pw - 32, msg, 0xCBD5E1, 1);
            }
        }

        canvas.blend_rect(px + 10, py + 266, pw - 20, 1, 0xFFFFFF, 20);
        canvas.draw_button(
            px + 10,
            py + 274,
            86,
            26,
            tr(lang, "Выход", "Exit"),
            0x1E293B,
            0xE2E8F0,
            0x475569,
        );
        canvas.draw_button(
            px + 102,
            py + 274,
            86,
            26,
            tr(lang, "Ребут", "Reboot"),
            0x78350F,
            0xFDE68A,
            0xF59E0B,
        );
        canvas.draw_button(
            px + 194,
            py + 274,
            86,
            26,
            tr(lang, "Выкл", "Power"),
            0x7F1D1D,
            0xFCA5A5,
            0xEF4444,
        );
    }
}
