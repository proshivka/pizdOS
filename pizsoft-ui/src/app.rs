use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

use crate::sys::{
    CpuSample, MemInfo, PkgSummary, ProcInfo, kill_process, list_processes, load_packages,
    read_cpu_sample, read_kernel_version, read_loadavg, read_meminfo, read_uptime_str,
};
use crate::term::{Color, Key, Screen};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Dashboard,
    Files,
    Packages,
    Matrix,
}

impl Tab {
    pub fn next(self) -> Self {
        match self {
            Tab::Dashboard => Tab::Files,
            Tab::Files => Tab::Packages,
            Tab::Packages => Tab::Matrix,
            Tab::Matrix => Tab::Dashboard,
        }
    }
}

#[derive(Debug, Clone)]
pub struct FileEntry {
    pub name: String,
    pub is_dir: bool,
    pub size: u64,
    pub path: PathBuf,
}

#[derive(Debug, Clone)]
pub struct MatrixCol {
    pub y: usize,
    pub speed: usize,
    pub len: usize,
}

pub struct App {
    pub tab: Tab,
    pub running: bool,
    pub ascii_borders: bool,
    pub status_msg: String,
    pub tick: u64,

    // Dashboard
    pub cpu_percent: u8,
    pub cpu_history: [u8; 32],
    pub last_cpu: Option<CpuSample>,
    pub mem: MemInfo,
    pub uptime: String,
    pub loadavg: String,
    pub kernel: String,
    pub procs: Vec<ProcInfo>,
    pub proc_sel: usize,

    // File manager
    pub cwd: PathBuf,
    pub entries: Vec<FileEntry>,
    pub file_sel: usize,
    pub preview_lines: Vec<String>,

    // Packages
    pub piz_root: PathBuf,
    pub packages: Vec<PkgSummary>,
    pub pkg_sel: usize,
    pub pkg_log: String,

    // Matrix
    pub cols: Vec<MatrixCol>,
    pub rng_state: u64,
    pub matrix_color: Color,
}

impl App {
    pub fn new(ascii_borders: bool) -> Self {
        let cwd = env::current_dir().unwrap_or_else(|_| PathBuf::from("/"));
        let piz_root = env::var("PIZ_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("/"));

        let mut app = Self {
            tab: Tab::Dashboard,
            running: true,
            ascii_borders,
            status_msg: String::from("Gotov k rabote | 0% GNU | pizdOS 2.0"),
            tick: 0,

            cpu_percent: 0,
            cpu_history: [0; 32],
            last_cpu: read_cpu_sample(),
            mem: read_meminfo(),
            uptime: read_uptime_str(),
            loadavg: read_loadavg(),
            kernel: read_kernel_version(),
            procs: list_processes(),
            proc_sel: 0,

            cwd,
            entries: Vec::new(),
            file_sel: 0,
            preview_lines: Vec::new(),

            piz_root,
            packages: Vec::new(),
            pkg_sel: 0,
            pkg_log: String::from("u: obnovi | i/Enter: ebash | d: snosi"),

            cols: Vec::new(),
            rng_state: 0xDEAD_BEEF_CAFE_BABEu64,
            matrix_color: Color::Green,
        };

        app.reload_files();
        app.reload_packages();
        app.init_matrix(80);
        app
    }

    fn next_rand(&mut self) -> u64 {
        let mut x = self.rng_state;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.rng_state = x;
        x
    }

    fn init_matrix(&mut self, width: usize) {
        self.cols.clear();
        for _ in 0..width {
            let r = self.next_rand();
            self.cols.push(MatrixCol {
                y: (r % 24) as usize,
                speed: 1 + ((r >> 8) % 2) as usize,
                len: 4 + ((r >> 16) % 8) as usize,
            });
        }
    }

    pub fn on_tick(&mut self, screen_w: usize, screen_h: usize) {
        self.tick = self.tick.wrapping_add(1);

        if self.tick.is_multiple_of(5) {
            self.refresh_metrics();
        }

        if self.tab == Tab::Matrix {
            if self.cols.len() != screen_w {
                self.init_matrix(screen_w);
            }
            let h = screen_h.saturating_sub(4).max(1);
            for i in 0..self.cols.len() {
                if self.tick.is_multiple_of(self.cols[i].speed as u64) {
                    self.cols[i].y += 1;
                    if self.cols[i].y > h + self.cols[i].len {
                        let r = self.next_rand();
                        self.cols[i].y = 0;
                        self.cols[i].len = 4 + (r % 9) as usize;
                    }
                }
            }
        }
    }

    pub fn refresh_metrics(&mut self) {
        if let Some(now) = read_cpu_sample() {
            if let Some(prev) = self.last_cpu {
                self.cpu_percent = now.percent_since(prev);
            }
            self.last_cpu = Some(now);
        } else {
            // Gentle deterministic wave when /proc/stat is unavailable
            self.cpu_percent = 5 + ((self.tick * 7) % 19) as u8;
        }
        self.cpu_history.rotate_left(1);
        self.cpu_history[31] = self.cpu_percent;

        self.mem = read_meminfo();
        self.uptime = read_uptime_str();
        self.loadavg = read_loadavg();
        self.procs = list_processes();
        if !self.procs.is_empty() && self.proc_sel >= self.procs.len() {
            self.proc_sel = self.procs.len() - 1;
        }
    }

    pub fn reload_files(&mut self) {
        self.entries.clear();
        if let Some(parent) = self.cwd.parent() {
            self.entries.push(FileEntry {
                name: String::from(".."),
                is_dir: true,
                size: 0,
                path: parent.to_path_buf(),
            });
        }

        let mut dirs = Vec::new();
        let mut files = Vec::new();
        if let Ok(rd) = fs::read_dir(&self.cwd) {
            for item in rd.flatten() {
                let path = item.path();
                let name = item.file_name().to_string_lossy().to_string();
                let meta = item.metadata().ok();
                let is_dir = meta.as_ref().is_some_and(|m| m.is_dir());
                let size = meta.as_ref().map_or(0, |m| m.len());
                let entry = FileEntry {
                    name,
                    is_dir,
                    size,
                    path,
                };
                if is_dir {
                    dirs.push(entry);
                } else {
                    files.push(entry);
                }
            }
        }

        dirs.sort_by(|a, b| a.name.cmp(&b.name));
        files.sort_by(|a, b| a.name.cmp(&b.name));
        self.entries.extend(dirs);
        self.entries.extend(files);

        if self.entries.is_empty() {
            self.file_sel = 0;
        } else if self.file_sel >= self.entries.len() {
            self.file_sel = self.entries.len() - 1;
        }
        self.load_file_preview();
    }

    fn load_file_preview(&mut self) {
        self.preview_lines.clear();
        let Some(sel) = self.entries.get(self.file_sel) else {
            return;
        };
        if sel.is_dir {
            self.preview_lines
                .push(format!("[Direktoriya] {}", sel.path.display()));
            self.preview_lines
                .push(String::from("Enter — zayti vnutr"));
            return;
        }
        match fs::read(&sel.path) {
            Ok(bytes) => {
                if bytes.iter().take(512).any(|&b| b == 0) {
                    self.preview_lines
                        .push(format!("[Binarny fayl, {} bayt]", bytes.len()));
                } else {
                    let text = String::from_utf8_lossy(&bytes);
                    for line in text.lines().take(64) {
                        self.preview_lines.push(line.to_string());
                    }
                    if self.preview_lines.is_empty() {
                        self.preview_lines.push(String::from("<pustoy fayl>"));
                    }
                }
            }
            Err(e) => {
                self.preview_lines.push(format!("Oshibka chteniya: {e}"));
            }
        }
    }

    pub fn reload_packages(&mut self) {
        self.packages = load_packages(&self.piz_root);
        if !self.packages.is_empty() && self.pkg_sel >= self.packages.len() {
            self.pkg_sel = self.packages.len() - 1;
        }
    }

    fn run_piz_get(&mut self, args: &[&str]) {
        let out = Command::new("piz-get")
            .args(args)
            .env("PIZ_ROOT", &self.piz_root)
            .output();
        match out {
            Ok(res) => {
                let stdout = String::from_utf8_lossy(&res.stdout);
                let stderr = String::from_utf8_lossy(&res.stderr);
                let msg = if res.status.success() {
                    stdout.lines().last().unwrap_or("OK").to_string()
                } else {
                    stderr.lines().last().unwrap_or("Oshibka piz-get").to_string()
                };
                self.pkg_log = msg.clone();
                self.status_msg = format!("piz-get {}: {}", args.join(" "), msg);
            }
            Err(e) => {
                self.pkg_log = format!("Ne udalos zapustit piz-get: {e}");
                self.status_msg = self.pkg_log.clone();
            }
        }
        self.reload_packages();
    }

    pub fn handle_key(&mut self, key: Key) {
        match key {
            Key::Char('q') | Key::Char('Q') | Key::F(10) => {
                self.running = false;
                return;
            }
            Key::Tab => {
                self.tab = self.tab.next();
                return;
            }
            Key::Char('1') | Key::F(1) => {
                self.tab = Tab::Dashboard;
                return;
            }
            Key::Char('2') | Key::F(2) => {
                self.tab = Tab::Files;
                return;
            }
            Key::Char('3') | Key::F(3) => {
                self.tab = Tab::Packages;
                return;
            }
            Key::Char('4') | Key::F(4) => {
                self.tab = Tab::Matrix;
                return;
            }
            Key::Char('b') => {
                self.ascii_borders = !self.ascii_borders;
                self.status_msg = if self.ascii_borders {
                    String::from("Ramki: ASCII (+-+|)")
                } else {
                    String::from("Ramki: Unicode")
                };
                return;
            }
            _ => {}
        }

        match self.tab {
            Tab::Dashboard => self.handle_dashboard_key(key),
            Tab::Files => self.handle_files_key(key),
            Tab::Packages => self.handle_packages_key(key),
            Tab::Matrix => self.handle_matrix_key(key),
        }
    }

    fn handle_dashboard_key(&mut self, key: Key) {
        match key {
            Key::Up => {
                self.proc_sel = self.proc_sel.saturating_sub(1);
            }
            Key::Down => {
                if self.proc_sel + 1 < self.procs.len() {
                    self.proc_sel += 1;
                }
            }
            Key::Char('r') => {
                self.refresh_metrics();
                self.status_msg = String::from("Metriki obnovleny");
            }
            Key::Char('k') => {
                if let Some(p) = self.procs.get(self.proc_sel).cloned() {
                    match kill_process(p.pid) {
                        Ok(()) => {
                            self.status_msg = format!("Ubit process {} ({})", p.name, p.pid);
                            self.refresh_metrics();
                        }
                        Err(e) => {
                            self.status_msg = format!("Ne ubit {} ({}): {e}", p.name, p.pid);
                        }
                    }
                }
            }
            _ => {}
        }
    }

    fn handle_files_key(&mut self, key: Key) {
        match key {
            Key::Up => {
                self.file_sel = self.file_sel.saturating_sub(1);
                self.load_file_preview();
            }
            Key::Down => {
                if self.file_sel + 1 < self.entries.len() {
                    self.file_sel += 1;
                    self.load_file_preview();
                }
            }
            Key::Enter => {
                if let Some(entry) = self.entries.get(self.file_sel).cloned() {
                    if entry.is_dir {
                        self.cwd = entry.path;
                        self.file_sel = 0;
                        self.reload_files();
                        self.status_msg = format!("CWD: {}", self.cwd.display());
                    } else {
                        self.load_file_preview();
                    }
                }
            }
            Key::Backspace => {
                if let Some(parent) = self.cwd.parent().map(PathBuf::from) {
                    self.cwd = parent;
                    self.file_sel = 0;
                    self.reload_files();
                    self.status_msg = format!("CWD: {}", self.cwd.display());
                }
            }
            Key::Char('x') => {
                if let Some(entry) = self.entries.get(self.file_sel).cloned()
                    && !entry.is_dir
                {
                    match fs::remove_file(&entry.path) {
                        Ok(()) => {
                            self.status_msg = format!("Udalen fayl {}", entry.name);
                            self.reload_files();
                        }
                        Err(e) => {
                            self.status_msg = format!("Oshibka udaleniya: {e}");
                        }
                    }
                }
            }
            _ => {}
        }
    }

    fn handle_packages_key(&mut self, key: Key) {
        match key {
            Key::Up => {
                self.pkg_sel = self.pkg_sel.saturating_sub(1);
            }
            Key::Down => {
                if self.pkg_sel + 1 < self.packages.len() {
                    self.pkg_sel += 1;
                }
            }
            Key::Char('u') => {
                self.run_piz_get(&["obnovi"]);
            }
            Key::Char('i') | Key::Enter => {
                if let Some(pkg) = self.packages.get(self.pkg_sel).cloned() {
                    self.run_piz_get(&["ebash", &pkg.name]);
                }
            }
            Key::Char('d') => {
                if let Some(pkg) = self.packages.get(self.pkg_sel).cloned() {
                    self.run_piz_get(&["snosi", &pkg.name]);
                }
            }
            _ => {}
        }
    }

    fn handle_matrix_key(&mut self, key: Key) {
        match key {
            Key::Char('c') => {
                self.matrix_color = match self.matrix_color {
                    Color::Green => Color::Red,
                    Color::Red => Color::Cyan,
                    Color::Cyan => Color::Yellow,
                    Color::Yellow => Color::Magenta,
                    _ => Color::Green,
                };
                self.status_msg = String::from("Cvet matricy izmenen");
            }
            Key::Char(' ') => {
                let w = self.cols.len();
                self.init_matrix(w);
            }
            _ => {}
        }
    }

    pub fn render(&mut self, screen: &mut Screen) {
        screen.clear(Color::Black);
        let w = screen.width;
        let h = screen.height;
        if w < 20 || h < 10 {
            screen.put_str(0, 0, w, "Terminal slishkom mal!", Color::Red, Color::Black, true);
            return;
        }

        self.render_header(screen);

        let body_y = 2;
        let body_h = h.saturating_sub(4);
        match self.tab {
            Tab::Dashboard => self.render_dashboard(screen, body_y, body_h),
            Tab::Files => self.render_files(screen, body_y, body_h),
            Tab::Packages => self.render_packages(screen, body_y, body_h),
            Tab::Matrix => self.render_matrix(screen, body_y, body_h),
        }

        self.render_footer(screen);
    }

    fn render_header(&self, screen: &mut Screen) {
        let w = screen.width;
        screen.fill_rect(0, 0, w, 1, ' ', Color::White, Color::Blue);
        screen.put_str(
            1,
            0,
            w.saturating_sub(2),
            "PIZSOFT UI v0.1.0 - pizdOS 2.0 (GNU-Free)",
            Color::Yellow,
            Color::Blue,
            true,
        );

        screen.fill_rect(0, 1, w, 1, ' ', Color::White, Color::Black);
        let tabs = [
            (Tab::Dashboard, "[1:Sistema]"),
            (Tab::Files, "[2:Fayly]"),
            (Tab::Packages, "[3:piz-get]"),
            (Tab::Matrix, "[4:Matrica]"),
        ];
        let mut x = 1usize;
        for (t, label) in tabs {
            let active = self.tab == t;
            let (fg, bg) = if active {
                (Color::Black, Color::Cyan)
            } else {
                (Color::White, Color::Black)
            };
            screen.put_str(x, 1, w.saturating_sub(x), label, fg, bg, active);
            x += label.chars().count() + 1;
        }
    }

    fn render_footer(&self, screen: &mut Screen) {
        let w = screen.width;
        let h = screen.height;
        screen.fill_rect(0, h - 2, w, 1, ' ', Color::White, Color::Black);
        screen.put_str(
            1,
            h - 2,
            w.saturating_sub(2),
            &format!("Status: {}", self.status_msg),
            Color::Green,
            Color::Black,
            false,
        );

        screen.fill_rect(0, h - 1, w, 1, ' ', Color::Black, Color::Cyan);
        screen.put_str(
            1,
            h - 1,
            w.saturating_sub(2),
            "1-4/Tab:Vkladki | Strl:Vybor | b:Ramki | q/F10:Vpizdu (Vyhod)",
            Color::Black,
            Color::Cyan,
            true,
        );
    }

    fn render_dashboard(&self, screen: &mut Screen, y: usize, h: usize) {
        let w = screen.width;
        let left_w = (w / 2).max(24).min(w);
        let right_x = left_w;
        let right_w = w.saturating_sub(left_w);

        screen.draw_box(
            0,
            y,
            left_w,
            h,
            "Sistema",
            Color::Cyan,
            Color::Black,
            self.ascii_borders,
        );

        let inner_w = left_w.saturating_sub(4);
        if h > 10 && inner_w > 10 {
            screen.put_str(
                2,
                y + 2,
                inner_w,
                &format!("Kernel: {}", self.kernel),
                Color::White,
                Color::Black,
                false,
            );
            screen.put_str(
                2,
                y + 3,
                inner_w,
                &format!("Uptime: {}", self.uptime),
                Color::White,
                Color::Black,
                false,
            );
            screen.put_str(
                2,
                y + 4,
                inner_w,
                &format!("Load  : {}", self.loadavg),
                Color::White,
                Color::Black,
                false,
            );

            let cpu_lbl = format!("CPU {:>3}%", self.cpu_percent);
            screen.draw_gauge(
                2,
                y + 6,
                inner_w,
                self.cpu_percent,
                &cpu_lbl,
                Color::Green,
                Color::Black,
            );

            let mem_lbl = format!(
                "RAM {}/{} MB ({}%)",
                self.mem.used_mb, self.mem.total_mb, self.mem.percent
            );
            screen.draw_gauge(
                2,
                y + 8,
                inner_w,
                self.mem.percent,
                &mem_lbl,
                Color::Yellow,
                Color::Black,
            );

            if h > 13 {
                let blocks = [' ', '_', '.', '-', '~', '=', '#', '@'];
                let mut spark = String::with_capacity(32);
                for &v in &self.cpu_history {
                    let idx = ((v as usize) * (blocks.len() - 1)) / 100;
                    spark.push(blocks[idx]);
                }
                screen.put_str(
                    2,
                    y + 10,
                    inner_w,
                    "CPU istoriya:",
                    Color::Cyan,
                    Color::Black,
                    true,
                );
                screen.put_str(2, y + 11, inner_w, &spark, Color::Green, Color::Black, false);
            }

            if h > 15 {
                screen.put_str(
                    2,
                    y + h - 2,
                    inner_w,
                    "r:obnovit | k:ubit process",
                    Color::Magenta,
                    Color::Black,
                    false,
                );
            }
        }

        if right_w >= 16 {
            screen.draw_box(
                right_x,
                y,
                right_w,
                h,
                "Processy",
                Color::Cyan,
                Color::Black,
                self.ascii_borders,
            );
            let rw = right_w.saturating_sub(4);
            screen.put_str(
                right_x + 2,
                y + 1,
                rw,
                "PID   S   MEM_KB  NAME",
                Color::Yellow,
                Color::Black,
                true,
            );

            let list_rows = h.saturating_sub(3);
            let start = self.proc_sel.saturating_sub(list_rows.saturating_sub(1));
            for (row_idx, p) in self.procs.iter().skip(start).take(list_rows).enumerate() {
                let abs_idx = start + row_idx;
                let selected = abs_idx == self.proc_sel;
                let (fg, bg) = if selected {
                    (Color::Black, Color::White)
                } else {
                    (Color::White, Color::Black)
                };
                let line = format!("{:<5} {} {:>7}  {}", p.pid, p.state, p.mem_kb, p.name);
                screen.fill_rect(right_x + 2, y + 2 + row_idx, rw, 1, ' ', fg, bg);
                screen.put_str(right_x + 2, y + 2 + row_idx, rw, &line, fg, bg, selected);
            }
        }
    }

    fn render_files(&self, screen: &mut Screen, y: usize, h: usize) {
        let w = screen.width;
        let left_w = (w / 2).max(24).min(w);
        let right_x = left_w;
        let right_w = w.saturating_sub(left_w);

        let title = format!("Fayly: {}", self.cwd.display());
        screen.draw_box(
            0,
            y,
            left_w,
            h,
            &title,
            Color::Green,
            Color::Black,
            self.ascii_borders,
        );

        let lw = left_w.saturating_sub(4);
        let list_rows = h.saturating_sub(3);
        let start = self.file_sel.saturating_sub(list_rows.saturating_sub(1));
        for (row_idx, entry) in self.entries.iter().skip(start).take(list_rows).enumerate() {
            let abs_idx = start + row_idx;
            let selected = abs_idx == self.file_sel;
            let (fg, bg) = if selected {
                (Color::Black, Color::Green)
            } else if entry.is_dir {
                (Color::Cyan, Color::Black)
            } else {
                (Color::White, Color::Black)
            };
            let tag = if entry.is_dir { "[D]" } else { "[F]" };
            let line = if entry.is_dir {
                format!("{tag} {}", entry.name)
            } else {
                format!("{tag} {} ({}B)", entry.name, entry.size)
            };
            screen.fill_rect(2, y + 1 + row_idx, lw, 1, ' ', fg, bg);
            screen.put_str(2, y + 1 + row_idx, lw, &line, fg, bg, selected);
        }
        screen.put_str(
            2,
            y + h - 2,
            lw,
            "Enter:otkryt | Backspace:vverh | x:udalit",
            Color::Yellow,
            Color::Black,
            false,
        );

        if right_w >= 16 {
            screen.draw_box(
                right_x,
                y,
                right_w,
                h,
                "Prosmotr",
                Color::Green,
                Color::Black,
                self.ascii_borders,
            );
            let rw = right_w.saturating_sub(4);
            let preview_rows = h.saturating_sub(2);
            for (idx, line) in self.preview_lines.iter().take(preview_rows).enumerate() {
                screen.put_str(
                    right_x + 2,
                    y + 1 + idx,
                    rw,
                    line,
                    Color::White,
                    Color::Black,
                    false,
                );
            }
        }
    }

    fn render_packages(&self, screen: &mut Screen, y: usize, h: usize) {
        let w = screen.width;
        screen.draw_box(
            0,
            y,
            w,
            h,
            "piz-get Center",
            Color::Yellow,
            Color::Black,
            self.ascii_borders,
        );

        let iw = w.saturating_sub(4);
        if self.packages.is_empty() {
            screen.put_str(
                2,
                y + 2,
                iw,
                "Net paketov v index.db / installed.db (nazhmi 'u' dlya obnovi)",
                Color::White,
                Color::Black,
                false,
            );
        } else {
            let list_rows = h.saturating_sub(5);
            let start = self.pkg_sel.saturating_sub(list_rows.saturating_sub(1));
            for (row_idx, pkg) in self.packages.iter().skip(start).take(list_rows).enumerate() {
                let abs_idx = start + row_idx;
                let selected = abs_idx == self.pkg_sel;
                let state = if pkg.installed { "[INST]" } else { "[AVAIL]" };
                let (fg, bg) = if selected {
                    (Color::Black, Color::Yellow)
                } else if pkg.installed {
                    (Color::Green, Color::Black)
                } else {
                    (Color::White, Color::Black)
                };
                let line = format!(
                    "{:<7} {:<16} {:<9} {}",
                    state, pkg.name, pkg.version, pkg.desc
                );
                screen.fill_rect(2, y + 1 + row_idx, iw, 1, ' ', fg, bg);
                screen.put_str(2, y + 1 + row_idx, iw, &line, fg, bg, selected);
            }
        }

        if h >= 5 {
            screen.put_str(
                2,
                y + h - 3,
                iw,
                &format!("Log: {}", self.pkg_log),
                Color::Cyan,
                Color::Black,
                false,
            );
            screen.put_str(
                2,
                y + h - 2,
                iw,
                "u:obnovi | i/Enter:ebash (stavit) | d:snosi (udalit)",
                Color::Magenta,
                Color::Black,
                false,
            );
        }
    }

    fn render_matrix(&mut self, screen: &mut Screen, y: usize, h: usize) {
        let w = screen.width;
        screen.draw_box(
            0,
            y,
            w,
            h,
            "Matrica (c:cvet | Space:sbros)",
            self.matrix_color,
            Color::Black,
            self.ascii_borders,
        );

        let glyphs = b"01PIZDOS20MUSL@#%&*+=?";
        let inner_h = h.saturating_sub(2);
        for x in 1..w.saturating_sub(1) {
            let Some(col) = self.cols.get(x) else {
                continue;
            };
            let head_y = col.y;
            let len = col.len;
            for dy in 0..inner_h {
                if dy <= head_y && head_y - dy < len {
                    let dist = head_y - dy;
                    let idx = ((self.tick as usize) + x * 13 + dy * 7) % glyphs.len();
                    let ch = glyphs[idx] as char;
                    let (fg, bold) = if dist == 0 {
                        (Color::White, true)
                    } else {
                        (self.matrix_color, dist < 3)
                    };
                    screen.put(x, y + 1 + dy, ch, fg, Color::Black, bold);
                }
            }
        }
    }
}
