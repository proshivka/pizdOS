use std::io;
use std::sync::{Mutex, OnceLock};

use crate::apps::KeyEvent;
use crate::desktop::Desktop;
use crate::gfx::Canvas;

static MEM_CLIPBOARD: OnceLock<Mutex<String>> = OnceLock::new();

fn mem_clip_set(text: &str) {
    let lock = MEM_CLIPBOARD.get_or_init(|| Mutex::new(String::new()));
    if let Ok(mut guard) = lock.lock() {
        guard.clear();
        guard.push_str(text);
    }
}

fn mem_clip_get() -> Option<String> {
    let lock = MEM_CLIPBOARD.get_or_init(|| Mutex::new(String::new()));
    lock.lock().ok().filter(|s| !s.is_empty()).map(|s| s.clone())
}

#[cfg(windows)]
pub fn copy_to_clipboard(text: &str) {
    mem_clip_set(text);
    if !cfg!(test) {
        win32::set_clipboard_text(text);
    }
}

#[cfg(not(windows))]
pub fn copy_to_clipboard(text: &str) {
    mem_clip_set(text);
}

#[cfg(windows)]
pub fn paste_from_clipboard() -> Option<String> {
    if cfg!(test) {
        return mem_clip_get();
    }
    win32::get_clipboard_text().or_else(mem_clip_get)
}

#[cfg(not(windows))]
pub fn paste_from_clipboard() -> Option<String> {
    mem_clip_get()
}

#[cfg(target_os = "linux")]
pub fn trigger_power_action(action: &str) -> bool {
    let candidates: &[&str] = match action {
        "poweroff" => &["/sbin/poweroff", "/usr/sbin/poweroff", "poweroff"],
        _ => &["/sbin/reboot", "/usr/sbin/reboot", "reboot"],
    };
    for bin in candidates {
        if std::process::Command::new(bin).spawn().is_ok() {
            return true;
        }
    }
    false
}

#[cfg(not(target_os = "linux"))]
pub fn trigger_power_action(_action: &str) -> bool {
    false
}

#[cfg(windows)]
pub fn run_gui() -> io::Result<()> {
    win32::run_win32_gui()
}

#[cfg(target_os = "linux")]
pub fn run_gui() -> io::Result<()> {
    linux_fb::run_linux_fb()
}

#[cfg(all(not(windows), not(target_os = "linux")))]
pub fn run_gui() -> io::Result<()> {
    Err(io::Error::other("GUI backend unsupported on this OS"))
}

#[cfg(target_os = "linux")]
mod linux_fb {
    use super::*;
    use std::fs::{File, OpenOptions};
    use std::io::{Read, Seek, SeekFrom, Write};
    use std::mem;
    use std::os::fd::AsRawFd;
    use std::thread;
    use std::time::Duration;

    const FBIOGET_VSCREENINFO: usize = 0x4600;
    const FBIOPUT_VSCREENINFO: usize = 0x4601;
    const FBIOGET_FSCREENINFO: usize = 0x4602;
    const EVIOCGABS_X: usize = 0x8018_4540;
    const EVIOCGABS_Y: usize = 0x8018_4541;
    const TCGETS: usize = 0x5401;
    const TCSETS: usize = 0x5402;
    const O_NONBLOCK: i32 = 0x800;
    const F_GETFL: i32 = 3;
    const F_SETFL: i32 = 4;

    const EV_SYN: u16 = 0x00;
    const EV_KEY: u16 = 0x01;
    const EV_REL: u16 = 0x02;
    const EV_ABS: u16 = 0x03;
    const REL_X: u16 = 0x00;
    const REL_Y: u16 = 0x01;
    const ABS_X: u16 = 0x00;
    const ABS_Y: u16 = 0x01;
    const BTN_LEFT: u16 = 0x110;
    const BTN_RIGHT: u16 = 0x111;

    unsafe extern "C" {
        fn ioctl(fd: i32, request: usize, ...) -> i32;
        fn fcntl(fd: i32, cmd: i32, ...) -> i32;
    }

    #[repr(C)]
    #[derive(Default, Clone, Copy)]
    struct FbBitfield {
        offset: u32,
        length: u32,
        msb_right: u32,
    }

    #[repr(C)]
    #[derive(Default)]
    struct FbVarScreenInfo {
        xres: u32,
        yres: u32,
        xres_virtual: u32,
        yres_virtual: u32,
        xoffset: u32,
        yoffset: u32,
        bits_per_pixel: u32,
        grayscale: u32,
        red: FbBitfield,
        green: FbBitfield,
        blue: FbBitfield,
        transp: FbBitfield,
        _rest: [u32; 20],
    }

    #[repr(C)]
    struct FbFixScreenInfo {
        id: [u8; 16],
        smem_start: usize,
        smem_len: u32,
        fb_type: u32,
        type_aux: u32,
        visual: u32,
        xpanstep: u16,
        ypanstep: u16,
        ywrapstep: u16,
        line_length: u32,
        mmio_start: usize,
        mmio_len: u32,
        accel: u32,
        capabilities: u16,
        reserved: [u16; 2],
    }

    impl Default for FbFixScreenInfo {
        fn default() -> Self {
            unsafe { mem::zeroed() }
        }
    }

    #[repr(C)]
    #[derive(Default, Clone, Copy)]
    struct InputAbsInfo {
        value: i32,
        minimum: i32,
        maximum: i32,
        fuzz: i32,
        flat: i32,
        resolution: i32,
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct InputEvent {
        tv_sec: isize,
        tv_usec: isize,
        kind: u16,
        code: u16,
        value: i32,
    }

    struct EvdevDevice {
        file: File,
        abs_x: Option<(i32, i32)>,
        abs_y: Option<(i32, i32)>,
    }

    struct Modifiers {
        shift: bool,
        ctrl: bool,
        alt: bool,
        caps: bool,
    }

    impl Modifiers {
        fn new() -> Self {
            Self {
                shift: false,
                ctrl: false,
                alt: false,
                caps: false,
            }
        }
    }

    struct RawTtyGuard {
        fd: i32,
        orig: [u32; 16],
        active: bool,
        orig_flags: i32,
    }

    impl RawTtyGuard {
        fn enter() -> Self {
            let fd = 0;
            let mut termios = [0u32; 16];
            let ok = unsafe { ioctl(fd, TCGETS, termios.as_mut_ptr()) } == 0;
            let orig_flags = unsafe { fcntl(fd, F_GETFL) };
            if orig_flags >= 0 {
                unsafe { fcntl(fd, F_SETFL, orig_flags | O_NONBLOCK) };
            }
            if ok {
                let mut raw = termios;
                raw[3] &= !(0x0002 | 0x0008); // clear ICANON | ECHO
                unsafe { ioctl(fd, TCSETS, raw.as_ptr()) };
            }
            let _ = io::stdout().write_all(b"\x1b[?25l");
            let _ = io::stdout().flush();
            Self {
                fd,
                orig: termios,
                active: ok,
                orig_flags,
            }
        }
    }

    impl Drop for RawTtyGuard {
        fn drop(&mut self) {
            if self.orig_flags >= 0 {
                unsafe { fcntl(self.fd, F_SETFL, self.orig_flags) };
            }
            if self.active {
                unsafe { ioctl(self.fd, TCSETS, self.orig.as_ptr()) };
            }
            let _ = io::stdout().write_all(b"\x1b[?25h\x1b[2J\x1b[H");
            let _ = io::stdout().flush();
        }
    }

    fn set_nonblocking(file: &File) {
        unsafe {
            let fd = file.as_raw_fd();
            let flags = fcntl(fd, F_GETFL);
            if flags >= 0 {
                fcntl(fd, F_SETFL, flags | O_NONBLOCK);
            }
        }
    }

    fn open_evdev_devices() -> Vec<EvdevDevice> {
        let mut out = Vec::new();
        for idx in 0..16 {
            let path = format!("/dev/input/event{idx}");
            if let Ok(file) = File::open(&path) {
                set_nonblocking(&file);
                let mut ax = InputAbsInfo::default();
                let mut ay = InputAbsInfo::default();
                let abs_x = if unsafe { ioctl(file.as_raw_fd(), EVIOCGABS_X, &mut ax) } >= 0
                    && ax.maximum > ax.minimum
                {
                    Some((ax.minimum, ax.maximum))
                } else {
                    None
                };
                let abs_y = if unsafe { ioctl(file.as_raw_fd(), EVIOCGABS_Y, &mut ay) } >= 0
                    && ay.maximum > ay.minimum
                {
                    Some((ay.minimum, ay.maximum))
                } else {
                    None
                };
                out.push(EvdevDevice { file, abs_x, abs_y });
            }
        }
        out
    }

    fn map_evdev_key(code: u16, m: &Modifiers) -> Option<KeyEvent> {
        if (m.alt && code == 57) || code == 60 || (m.ctrl && code == 37) {
            return Some(KeyEvent::ToggleLauncher);
        }
        if code == 59 || (m.ctrl && code == 34) {
            return Some(KeyEvent::ToggleControlCenter);
        }
        if (m.alt || m.ctrl) && code == 15 {
            return Some(KeyEvent::SwitchWindows);
        }
        if m.ctrl {
            return match code {
                38 => Some(KeyEvent::LockScreen),  // L
                32 => Some(KeyEvent::ShowDesktop), // D
                46 => Some(KeyEvent::Copy),        // C
                47 => paste_from_clipboard().map(KeyEvent::Paste), // V
                31 => Some(KeyEvent::Save),        // S
                33 => Some(KeyEvent::Find),        // F
                44 => Some(KeyEvent::Undo),        // Z
                2 => Some(KeyEvent::Workspace(1)), // 1
                3 => Some(KeyEvent::Workspace(2)), // 2
                4 => Some(KeyEvent::Workspace(3)), // 3
                13 | 78 => Some(KeyEvent::ScaleDelta(1)),
                12 | 74 => Some(KeyEvent::ScaleDelta(-1)),
                11 => Some(KeyEvent::ScaleDelta(0)),
                _ => None,
            };
        }

        let upper = m.shift ^ m.caps;
        let letter = match code {
            30 => Some(('a', 'A')),
            48 => Some(('b', 'B')),
            46 => Some(('c', 'C')),
            32 => Some(('d', 'D')),
            18 => Some(('e', 'E')),
            33 => Some(('f', 'F')),
            34 => Some(('g', 'G')),
            35 => Some(('h', 'H')),
            23 => Some(('i', 'I')),
            36 => Some(('j', 'J')),
            37 => Some(('k', 'K')),
            38 => Some(('l', 'L')),
            50 => Some(('m', 'M')),
            49 => Some(('n', 'N')),
            24 => Some(('o', 'O')),
            25 => Some(('p', 'P')),
            16 => Some(('q', 'Q')),
            19 => Some(('r', 'R')),
            31 => Some(('s', 'S')),
            20 => Some(('t', 'T')),
            22 => Some(('u', 'U')),
            47 => Some(('v', 'V')),
            17 => Some(('w', 'W')),
            45 => Some(('x', 'X')),
            21 => Some(('y', 'Y')),
            44 => Some(('z', 'Z')),
            _ => None,
        };
        if let Some((lo, hi)) = letter {
            return Some(KeyEvent::Char(if upper { hi } else { lo }));
        }

        let sym = match code {
            2 => Some(('1', '!')),
            3 => Some(('2', '@')),
            4 => Some(('3', '#')),
            5 => Some(('4', '$')),
            6 => Some(('5', '%')),
            7 => Some(('6', '^')),
            8 => Some(('7', '&')),
            9 => Some(('8', '*')),
            10 => Some(('9', '(')),
            11 => Some(('0', ')')),
            12 => Some(('-', '_')),
            13 => Some(('=', '+')),
            26 => Some(('[', '{')),
            27 => Some((']', '}')),
            39 => Some((';', ':')),
            40 => Some(('\'', '"')),
            41 => Some(('`', '~')),
            43 => Some(('\\', '|')),
            51 => Some((',', '<')),
            52 => Some(('.', '>')),
            53 => Some(('/', '?')),
            57 => Some((' ', ' ')),
            _ => None,
        };
        if let Some((lo, hi)) = sym {
            return Some(KeyEvent::Char(if m.shift { hi } else { lo }));
        }

        match code {
            1 => Some(KeyEvent::Escape),
            14 => Some(KeyEvent::Backspace),
            15 => Some(KeyEvent::Tab),
            28 | 96 => Some(KeyEvent::Enter),
            103 => Some(KeyEvent::Up),
            108 => Some(KeyEvent::Down),
            105 => Some(KeyEvent::Left),
            106 => Some(KeyEvent::Right),
            _ => None,
        }
    }

    fn query_fb_mode(
        fb: &File,
        req: Option<(i32, i32)>,
    ) -> io::Result<(FbVarScreenInfo, usize, usize, usize, usize)> {
        let mut vinfo = FbVarScreenInfo::default();
        if let Some((rw, rh)) = req
            && unsafe { ioctl(fb.as_raw_fd(), FBIOGET_VSCREENINFO, &mut vinfo) } >= 0
        {
            vinfo.xres = rw.max(320) as u32;
            vinfo.yres = rh.max(240) as u32;
            vinfo.xres_virtual = vinfo.xres;
            vinfo.yres_virtual = vinfo.yres;
            let _ = unsafe { ioctl(fb.as_raw_fd(), FBIOPUT_VSCREENINFO, &vinfo) };
        }
        vinfo = FbVarScreenInfo::default();
        let rc = unsafe { ioctl(fb.as_raw_fd(), FBIOGET_VSCREENINFO, &mut vinfo) };
        if rc < 0 || vinfo.xres == 0 || vinfo.yres == 0 {
            return Err(io::Error::other("FBIOGET_VSCREENINFO failed"));
        }
        let mut finfo = FbFixScreenInfo::default();
        let frc = unsafe { ioctl(fb.as_raw_fd(), FBIOGET_FSCREENINFO, &mut finfo) };
        let phys_w = vinfo.xres as usize;
        let phys_h = vinfo.yres as usize;
        let bpp = vinfo.bits_per_pixel.max(32) as usize;
        let bytes_pp = (bpp / 8).clamp(2, 4);
        let stride = if frc == 0 && finfo.line_length >= (phys_w * bytes_pp) as u32 {
            finfo.line_length as usize
        } else {
            phys_w * bytes_pp
        };
        Ok((vinfo, phys_w, phys_h, bytes_pp, stride))
    }

    fn scale_nearest(
        src: &[u32],
        sw: usize,
        sh: usize,
        dst: &mut [u32],
        dw: usize,
        dh: usize,
    ) {
        if sw == 0 || sh == 0 || dw == 0 || dh == 0 {
            return;
        }
        for y in 0..dh {
            let sy = (y * sh) / dh;
            let srow = sy * sw;
            let drow = y * dw;
            for x in 0..dw {
                let sx = (x * sw) / dw;
                dst[drow + x] = src[srow + sx];
            }
        }
    }

    pub fn run_linux_fb() -> io::Result<()> {
        let fb_path = std::env::var("PIZSOFT_FB").unwrap_or_else(|_| String::from("/dev/fb0"));
        let mut fb = OpenOptions::new().read(true).write(true).open(&fb_path)?;
        let (mut vinfo, mut phys_w, mut phys_h, mut bytes_pp, mut stride) =
            query_fb_mode(&fb, Some((800, 600)))?;

        let _tty = RawTtyGuard::enter();
        let mut desktop = Desktop::new(800, 600);
        let (init_rw, init_rh) = desktop.settings.resolution();
        if (init_rw, init_rh) != (800, 600)
            && let Ok((vi, pw, ph, bpp, st)) = query_fb_mode(&fb, Some((init_rw, init_rh)))
        {
            vinfo = vi;
            phys_w = pw;
            phys_h = ph;
            bytes_pp = bpp;
            stride = st;
        }
        let mut canvas = Canvas::new(init_rw.max(320) as usize, init_rh.max(240) as usize);
        let mut prev_scale = desktop.settings.ui_scale();
        let mut prev_res_idx = desktop.settings.res_idx;
        canvas.set_ui_scale(prev_scale);
        desktop.on_viewport_resize(canvas.logical_width(), canvas.logical_height());

        let mut evdevs = open_evdev_devices();
        let mut mice = File::open("/dev/input/mice").ok();
        if let Some(ref m) = mice {
            set_nonblocking(m);
        }

        let mut stdin = io::stdin();
        let mut frame_buf = vec![0u8; stride * phys_h];
        let mut scaled_buf = vec![0u32; phys_w * phys_h];
        let mut prev_pixels: Vec<u32> = Vec::new();
        let mut prev_right = false;
        let mut mods = Modifiers::new();
        let mut abs_norm_x: i32 = desktop.mouse_x;
        let mut abs_norm_y: i32 = desktop.mouse_y;
        let ev_size = mem::size_of::<InputEvent>();

        while desktop.running {
            if desktop.settings.res_idx != prev_res_idx {
                prev_res_idx = desktop.settings.res_idx;
                let (rw, rh) = desktop.settings.resolution();
                if let Ok((vi, pw, ph, bpp, st)) = query_fb_mode(&fb, Some((rw, rh))) {
                    vinfo = vi;
                    phys_w = pw;
                    phys_h = ph;
                    bytes_pp = bpp;
                    stride = st;
                    frame_buf.resize(stride * phys_h, 0);
                    scaled_buf.resize(phys_w * phys_h, 0);
                    prev_pixels.clear();
                }
                canvas.resize(rw.max(320) as usize, rh.max(240) as usize);
                desktop.on_viewport_resize(canvas.logical_width(), canvas.logical_height());
            }
            let cur_scale = desktop.settings.ui_scale();
            if (cur_scale - prev_scale).abs() > 0.001 {
                canvas.set_ui_scale(cur_scale);
                desktop.on_viewport_resize(canvas.logical_width(), canvas.logical_height());
                prev_scale = cur_scale;
            }
            let log_w = canvas.logical_width();
            let log_h = canvas.logical_height();

            // 1. Read Linux /dev/input/event* (keyboard + PS/2/USB mouse + QEMU tablet)
            let mut buf = [0u8; 64 * 24];
            let max_bytes = (buf.len() / ev_size) * ev_size;
            for dev in &mut evdevs {
                while let Ok(n) = dev.file.read(&mut buf[..max_bytes]) {
                    if n < ev_size {
                        break;
                    }
                    let mut moved_rel = (0i32, 0i32);
                    let mut moved_abs = false;
                    for chunk in buf[..n].chunks_exact(ev_size) {
                        let ev: InputEvent =
                            unsafe { mem::transmute_copy::<u8, InputEvent>(&chunk[0]) };
                        match ev.kind {
                            EV_KEY => {
                                let down = ev.value != 0;
                                match ev.code {
                                    42 | 54 => mods.shift = down,
                                    29 | 97 => mods.ctrl = down,
                                    56 | 100 => mods.alt = down,
                                    58 if ev.value == 1 => mods.caps = !mods.caps,
                                    BTN_LEFT => {
                                        if down && !desktop.mouse_down {
                                            desktop.on_mouse_down(abs_norm_x, abs_norm_y, log_w, log_h);
                                        } else if !down && desktop.mouse_down {
                                            desktop.on_mouse_up();
                                        }
                                    }
                                    BTN_RIGHT if ev.value == 1 => {
                                        desktop.on_right_click(abs_norm_x, abs_norm_y, log_w, log_h);
                                    }
                                    _ if down => {
                                        if mods.ctrl && mods.alt && ev.code == 14 {
                                            desktop.running = false;
                                        } else if let Some(kev) = map_evdev_key(ev.code, &mods) {
                                            if kev == KeyEvent::Escape {
                                                let _ = desktop.on_escape();
                                            } else {
                                                desktop.on_key(kev);
                                            }
                                        }
                                    }
                                    _ => {}
                                }
                            }
                            EV_REL => match ev.code {
                                REL_X => moved_rel.0 += ev.value,
                                REL_Y => moved_rel.1 += ev.value,
                                _ => {}
                            },
                            EV_ABS => match ev.code {
                                ABS_X => {
                                    if let Some((min, max)) = dev.abs_x {
                                        let span = (max - min).max(1) as i64;
                                        abs_norm_x = ((((ev.value - min) as i64) * log_w as i64)
                                            / span)
                                            .clamp(0, (log_w - 1) as i64) as i32;
                                        moved_abs = true;
                                    }
                                }
                                ABS_Y => {
                                    if let Some((min, max)) = dev.abs_y {
                                        let span = (max - min).max(1) as i64;
                                        abs_norm_y = ((((ev.value - min) as i64) * log_h as i64)
                                            / span)
                                            .clamp(0, (log_h - 1) as i64) as i32;
                                        moved_abs = true;
                                    }
                                }
                                _ => {}
                            },
                            EV_SYN if moved_abs || moved_rel != (0, 0) => {
                                if !moved_abs {
                                    abs_norm_x = (desktop.mouse_x + moved_rel.0).clamp(0, log_w - 1);
                                    abs_norm_y = (desktop.mouse_y + moved_rel.1).clamp(0, log_h - 1);
                                }
                                desktop.on_mouse_move(abs_norm_x, abs_norm_y, log_w, log_h);
                                moved_rel = (0, 0);
                                moved_abs = false;
                            }
                            _ => {}
                        }
                    }
                    if moved_abs || moved_rel != (0, 0) {
                        if !moved_abs {
                            abs_norm_x = (desktop.mouse_x + moved_rel.0).clamp(0, log_w - 1);
                            abs_norm_y = (desktop.mouse_y + moved_rel.1).clamp(0, log_h - 1);
                        }
                        desktop.on_mouse_move(abs_norm_x, abs_norm_y, log_w, log_h);
                    }
                }
            }

            // 2. Legacy /dev/input/mice fallback
            if let Some(ref mut m) = mice {
                let mut pkt = [0u8; 3];
                while m.read_exact(&mut pkt).is_ok() {
                    let buttons = pkt[0];
                    let dx = pkt[1] as i8 as i32;
                    let dy = -(pkt[2] as i8 as i32);
                    abs_norm_x = (desktop.mouse_x + dx).clamp(0, log_w - 1);
                    abs_norm_y = (desktop.mouse_y + dy).clamp(0, log_h - 1);
                    let left = (buttons & 1) != 0;
                    let right = (buttons & 2) != 0;
                    if left && !desktop.mouse_down {
                        desktop.on_mouse_down(abs_norm_x, abs_norm_y, log_w, log_h);
                    } else if !left && desktop.mouse_down {
                        desktop.on_mouse_up();
                    } else {
                        desktop.on_mouse_move(abs_norm_x, abs_norm_y, log_w, log_h);
                    }
                    if right && !prev_right {
                        desktop.on_right_click(abs_norm_x, abs_norm_y, log_w, log_h);
                    }
                    prev_right = right;
                }
            }

            // 3. Stdin non-blocking ANSI keyboard fallback (when no evdev devices are open)
            let mut kbuf = [0u8; 32];
            if evdevs.is_empty()
                && let Ok(kn) = stdin.read(&mut kbuf)
            {
                let mut i = 0;
                while i < kn {
                    let b = kbuf[i];
                    if b == 0x1B {
                        if i + 2 < kn && kbuf[i + 1] == b'[' {
                            let ev = match kbuf[i + 2] {
                                b'A' => Some(KeyEvent::Up),
                                b'B' => Some(KeyEvent::Down),
                                b'C' => Some(KeyEvent::Right),
                                b'D' => Some(KeyEvent::Left),
                                _ => None,
                            };
                            if let Some(k) = ev {
                                desktop.on_key(k);
                                i += 3;
                                continue;
                            }
                        }
                        let _ = desktop.on_escape();
                    } else if b == b'\r' || b == b'\n' {
                        desktop.on_key(KeyEvent::Enter);
                    } else if b == 0x7F || b == 0x08 {
                        desktop.on_key(KeyEvent::Backspace);
                    } else if b == b'\t' {
                        desktop.on_key(KeyEvent::Tab);
                    } else if (0x20..0x7F).contains(&b) {
                        desktop.on_key(KeyEvent::Char(b as char));
                    }
                    i += 1;
                }
            }

            desktop.on_tick();
            desktop.render(&mut canvas);

            // Pack and flush only dirty horizontal spans to /dev/fb0 for high FPS in QEMU/fbdev
            let active_px: &[u32] = if canvas.width == phys_w && canvas.height == phys_h {
                &canvas.pixels
            } else {
                scale_nearest(
                    &canvas.pixels,
                    canvas.width,
                    canvas.height,
                    &mut scaled_buf,
                    phys_w,
                    phys_h,
                );
                &scaled_buf
            };
            let first_frame = prev_pixels.len() != active_px.len();
            let rgb_order = vinfo.red.offset == 0;
            let mut dirty_start: Option<usize> = None;
            let mut any_dirty = false;

            for y in 0..phys_h {
                let src_row = &active_px[y * phys_w..(y + 1) * phys_w];
                let row_dirty = first_frame || src_row != &prev_pixels[y * phys_w..(y + 1) * phys_w];
                if !row_dirty {
                    if let Some(ys) = dirty_start.take() {
                        fb.seek(SeekFrom::Start((ys * stride) as u64))?;
                        fb.write_all(&frame_buf[ys * stride..y * stride])?;
                    }
                    continue;
                }
                if dirty_start.is_none() {
                    dirty_start = Some(y);
                }
                any_dirty = true;
                let row_off = y * stride;
                match bytes_pp {
                    4 => {
                        let dst_cells = frame_buf[row_off..row_off + phys_w * 4].as_chunks_mut::<4>().0;
                        if rgb_order {
                            for (dst, &px) in dst_cells.iter_mut().zip(src_row.iter()) {
                                *dst = [(px >> 16) as u8, (px >> 8) as u8, px as u8, 0xFF];
                            }
                        } else {
                            for (dst, &px) in dst_cells.iter_mut().zip(src_row.iter()) {
                                *dst = px.to_le_bytes();
                            }
                        }
                    }
                    3 => {
                        for (x, &px) in src_row.iter().enumerate() {
                            let o = row_off + x * 3;
                            frame_buf[o] = px as u8;
                            frame_buf[o + 1] = (px >> 8) as u8;
                            frame_buf[o + 2] = (px >> 16) as u8;
                        }
                    }
                    _ => {
                        for (x, &px) in src_row.iter().enumerate() {
                            let o = row_off + x * 2;
                            let r = (px >> 19) & 0x1F;
                            let g = (px >> 10) & 0x3F;
                            let b = (px >> 3) & 0x1F;
                            let v = ((r << 11) | (g << 5) | b) as u16;
                            frame_buf[o..o + 2].copy_from_slice(&v.to_le_bytes());
                        }
                    }
                }
            }
            if let Some(ys) = dirty_start {
                fb.seek(SeekFrom::Start((ys * stride) as u64))?;
                fb.write_all(&frame_buf[ys * stride..phys_h * stride])?;
            }
            if any_dirty {
                fb.flush()?;
                if first_frame {
                    prev_pixels = active_px.to_vec();
                } else {
                    prev_pixels.copy_from_slice(active_px);
                }
            }

            thread::sleep(Duration::from_millis(16));
        }
        Ok(())
    }
}

#[cfg(windows)]
mod win32 {
    use super::*;
    use std::mem;
    use std::ptr::{null, null_mut};

    type Hwnd = isize;
    type Hdc = isize;
    type Hinstance = isize;

    const WS_OVERLAPPEDWINDOW: u32 = 0x00CF_0000;
    const WS_VISIBLE: u32 = 0x1000_0000;
    const CW_USEDEFAULT: i32 = 0x8000_0000u32 as i32;
    const WM_DESTROY: u32 = 0x0002;
    const WM_PAINT: u32 = 0x000F;
    const WM_KEYDOWN: u32 = 0x0100;
    const WM_CHAR: u32 = 0x0102;
    const WM_SYSKEYDOWN: u32 = 0x0104;
    const WM_SYSCHAR: u32 = 0x0106;
    const WM_MOUSEMOVE: u32 = 0x0200;
    const WM_LBUTTONDOWN: u32 = 0x0201;
    const WM_LBUTTONUP: u32 = 0x0202;
    const WM_RBUTTONDOWN: u32 = 0x0204;
    const WM_CAPTURECHANGED: u32 = 0x0215;
    const VK_BACK: usize = 0x08;
    const VK_TAB: usize = 0x09;
    const VK_RETURN: usize = 0x0D;
    const VK_CONTROL: i32 = 0x11;
    const VK_MENU: i32 = 0x12;
    const VK_ESCAPE: usize = 0x1B;
    const VK_SPACE: usize = 0x20;
    const VK_LEFT: usize = 0x25;
    const VK_UP: usize = 0x26;
    const VK_RIGHT: usize = 0x27;
    const VK_DOWN: usize = 0x28;
    const VK_0: usize = 0x30;
    const VK_1: usize = 0x31;
    const VK_2: usize = 0x32;
    const VK_3: usize = 0x33;
    const VK_C: usize = 0x43;
    const VK_D: usize = 0x44;
    const VK_F: usize = 0x46;
    const VK_G: usize = 0x47;
    const VK_K: usize = 0x4B;
    const VK_L: usize = 0x4C;
    const VK_S: usize = 0x53;
    const VK_V: usize = 0x56;
    const VK_Z: usize = 0x5A;
    const CF_UNICODETEXT: u32 = 13;
    const GMEM_MOVEABLE: u32 = 0x0002;
    const VK_ADD: usize = 0x6B;
    const VK_SUBTRACT: usize = 0x6D;
    const VK_F1: usize = 0x70;
    const VK_F2: usize = 0x71;
    const VK_OEM_PLUS: usize = 0xBB;
    const VK_OEM_MINUS: usize = 0xBD;
    const SM_CXSCREEN: i32 = 0;
    const SM_CYSCREEN: i32 = 1;
    const LOGPIXELSX: i32 = 88;
    const PM_REMOVE: u32 = 0x0001;
    const SRCCOPY: u32 = 0x00CC_0020;

    #[repr(C)]
    struct WndClassW {
        style: u32,
        lpfn_wnd_proc: unsafe extern "system" fn(Hwnd, u32, usize, isize) -> isize,
        cb_cls_extra: i32,
        cb_wnd_extra: i32,
        h_instance: Hinstance,
        h_icon: isize,
        h_cursor: isize,
        h_br_background: isize,
        lpsz_menu_name: *const u16,
        lpsz_class_name: *const u16,
    }

    #[repr(C)]
    #[derive(Default)]
    struct Point {
        x: i32,
        y: i32,
    }

    #[repr(C)]
    #[derive(Default)]
    struct Msg {
        hwnd: Hwnd,
        message: u32,
        w_param: usize,
        l_param: isize,
        time: u32,
        pt: Point,
    }

    #[repr(C)]
    #[derive(Default)]
    struct Rect {
        left: i32,
        top: i32,
        right: i32,
        bottom: i32,
    }

    #[repr(C)]
    struct BitmapInfoHeader {
        bi_size: u32,
        bi_width: i32,
        bi_height: i32,
        bi_planes: u16,
        bi_bit_count: u16,
        bi_compression: u32,
        bi_size_image: u32,
        bi_x_pels_per_meter: i32,
        bi_y_pels_per_meter: i32,
        bi_clr_used: u32,
        bi_clr_important: u32,
    }

    #[repr(C)]
    struct BitmapInfo {
        bmi_header: BitmapInfoHeader,
        bmi_colors: [u32; 1],
    }

    #[link(name = "user32")]
    unsafe extern "system" {
        fn RegisterClassW(lp_wnd_class: *const WndClassW) -> u16;
        fn CreateWindowExW(
            dw_ex_style: u32,
            lp_class_name: *const u16,
            lp_window_name: *const u16,
            dw_style: u32,
            x: i32,
            y: i32,
            n_width: i32,
            n_height: i32,
            h_wnd_parent: Hwnd,
            h_menu: isize,
            h_instance: Hinstance,
            lp_param: *const u8,
        ) -> Hwnd;
        fn DefWindowProcW(hwnd: Hwnd, msg: u32, w_param: usize, l_param: isize) -> isize;
        fn PeekMessageW(
            lp_msg: *mut Msg,
            hwnd: Hwnd,
            w_msg_filter_min: u32,
            w_msg_filter_max: u32,
            w_remove_msg: u32,
        ) -> i32;
        fn TranslateMessage(lp_msg: *const Msg) -> i32;
        fn DispatchMessageW(lp_msg: *const Msg) -> isize;
        fn PostQuitMessage(n_exit_code: i32);
        fn GetDC(hwnd: Hwnd) -> Hdc;
        fn ReleaseDC(hwnd: Hwnd, hdc: Hdc) -> i32;
        fn GetClientRect(hwnd: Hwnd, lp_rect: *mut Rect) -> i32;
        fn AdjustWindowRectEx(lp_rect: *mut Rect, dw_style: u32, b_menu: i32, dw_ex_style: u32) -> i32;
        fn SetWindowPos(
            hwnd: Hwnd,
            h_wnd_insert_after: isize,
            x: i32,
            y: i32,
            cx: i32,
            cy: i32,
            u_flags: u32,
        ) -> i32;
        fn SetCapture(hwnd: Hwnd) -> Hwnd;
        fn ReleaseCapture() -> i32;
        fn ValidateRect(hwnd: Hwnd, lp_rect: *const Rect) -> i32;
        fn LoadCursorW(h_instance: Hinstance, lp_cursor_name: *const u16) -> isize;
        fn SetProcessDPIAware() -> i32;
        fn GetSystemMetrics(n_index: i32) -> i32;
        fn GetKeyState(n_virt_key: i32) -> i16;
        fn OpenClipboard(h_wnd_new_owner: Hwnd) -> i32;
        fn EmptyClipboard() -> i32;
        fn SetClipboardData(u_format: u32, h_mem: isize) -> isize;
        fn GetClipboardData(u_format: u32) -> isize;
        fn CloseClipboard() -> i32;
    }

    #[link(name = "gdi32")]
    unsafe extern "system" {
        fn GetDeviceCaps(hdc: Hdc, index: i32) -> i32;
        fn StretchDIBits(
            hdc: Hdc,
            x_dest: i32,
            y_dest: i32,
            dest_width: i32,
            dest_height: i32,
            x_src: i32,
            y_src: i32,
            src_width: i32,
            src_height: i32,
            lp_bits: *const u32,
            lpbmi: *const BitmapInfo,
            i_usage: u32,
            rop: u32,
        ) -> i32;
    }

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetModuleHandleW(lp_module_name: *const u16) -> Hinstance;
        fn GlobalAlloc(u_flags: u32, dw_bytes: usize) -> isize;
        fn GlobalLock(h_mem: isize) -> *mut u16;
        fn GlobalUnlock(h_mem: isize) -> i32;
        fn GlobalFree(h_mem: isize) -> isize;
    }

    pub fn set_clipboard_text(text: &str) {
        let mut wide: Vec<u16> = text.encode_utf16().collect();
        wide.push(0);
        let bytes = wide.len() * 2;
        unsafe {
            if OpenClipboard(0) == 0 {
                return;
            }
            EmptyClipboard();
            let h = GlobalAlloc(GMEM_MOVEABLE, bytes);
            if h != 0 {
                let ptr = GlobalLock(h);
                if !ptr.is_null() {
                    std::ptr::copy_nonoverlapping(wide.as_ptr(), ptr, wide.len());
                    GlobalUnlock(h);
                    if SetClipboardData(CF_UNICODETEXT, h) == 0 {
                        GlobalFree(h);
                    }
                } else {
                    GlobalFree(h);
                }
            }
            CloseClipboard();
        }
    }

    pub fn get_clipboard_text() -> Option<String> {
        unsafe {
            if OpenClipboard(0) == 0 {
                return None;
            }
            let h = GetClipboardData(CF_UNICODETEXT);
            let mut result = None;
            if h != 0 {
                let ptr = GlobalLock(h);
                if !ptr.is_null() {
                    let mut len = 0usize;
                    while *ptr.add(len) != 0 && len < 8192 {
                        len += 1;
                    }
                    let slice = std::slice::from_raw_parts(ptr, len);
                    result = Some(String::from_utf16_lossy(slice));
                    GlobalUnlock(h);
                }
            }
            CloseClipboard();
            result
        }
    }

    unsafe extern "system" fn wnd_proc(
        hwnd: Hwnd,
        msg: u32,
        w_param: usize,
        l_param: isize,
    ) -> isize {
        match msg {
            WM_DESTROY => {
                unsafe { PostQuitMessage(0) };
                0
            }
            WM_PAINT => {
                unsafe { ValidateRect(hwnd, null()) };
                0
            }
            0x0014 /* WM_ERASEBKGND */ => 1,
            _ => unsafe { DefWindowProcW(hwnd, msg, w_param, l_param) },
        }
    }

    fn utf16(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(Some(0)).collect()
    }

    pub fn run_win32_gui() -> io::Result<()> {
        let prev_hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            let log_path = std::env::temp_dir().join("pizsoft_panic.log");
            let _ = std::fs::write(&log_path, format!("{info}\n"));
            prev_hook(info);
        }));
        // Disable automatic Windows DPI bitmap stretching so every pixel matches native screen DPI 1:1.
        unsafe { SetProcessDPIAware() };
        let scr_w = unsafe { GetSystemMetrics(SM_CXSCREEN) };
        let scr_h = unsafe { GetSystemMetrics(SM_CYSCREEN) };
        let screen_dc = unsafe { GetDC(0) };
        let dpi_x = if screen_dc != 0 {
            let d = unsafe { GetDeviceCaps(screen_dc, LOGPIXELSX) };
            unsafe { ReleaseDC(0, screen_dc) };
            d
        } else {
            96
        };
        let mut desktop = Desktop::new(800, 600);
        let init_scale_pct: u16 = if desktop.settings.ui_scale_pct == 100 && dpi_x >= 144 {
            150
        } else {
            desktop.settings.ui_scale_pct
        };
        desktop.settings.ui_scale_pct = init_scale_pct;
        let (base_w, base_h) = desktop.settings.resolution();
        let scale_f = desktop.settings.ui_scale();
        let target_cw = ((base_w as f32 * scale_f).round() as i32)
            .min(if scr_w > 0 { scr_w - 40 } else { 1600 })
            .max(640);
        let target_ch = ((base_h as f32 * scale_f).round() as i32)
            .min(if scr_h > 0 { scr_h - 64 } else { 1000 })
            .max(480);
        let mut adj = Rect {
            left: 0,
            top: 0,
            right: target_cw,
            bottom: target_ch,
        };
        unsafe { AdjustWindowRectEx(&mut adj, WS_OVERLAPPEDWINDOW, 0, 0) };
        let outer_w = adj.right - adj.left;
        let outer_h = adj.bottom - adj.top;

        let class_name = utf16("PizsoftUIClass");
        let title = utf16("Pizsoft UI — pizdOS 2.0");

        let h_inst = unsafe { GetModuleHandleW(null()) };
        let wc = WndClassW {
            style: 0,
            lpfn_wnd_proc: wnd_proc,
            cb_cls_extra: 0,
            cb_wnd_extra: 0,
            h_instance: h_inst,
            h_icon: 0,
            h_cursor: unsafe { LoadCursorW(0, 32512 as *const u16) },
            h_br_background: 0,
            lpsz_menu_name: null(),
            lpsz_class_name: class_name.as_ptr(),
        };

        unsafe { RegisterClassW(&wc) };

        let hwnd = unsafe {
            CreateWindowExW(
                0,
                class_name.as_ptr(),
                title.as_ptr(),
                WS_OVERLAPPEDWINDOW | WS_VISIBLE,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                outer_w,
                outer_h,
                0,
                0,
                h_inst,
                null(),
            )
        };
        if hwnd == 0 {
            return Err(io::Error::other("CreateWindowExW failed"));
        }

        let mut canvas = Canvas::new(target_cw as usize, target_ch as usize);
        let mut prev_scale = desktop.settings.ui_scale();
        let mut prev_res_idx = desktop.settings.res_idx;
        canvas.set_ui_scale(prev_scale);
        desktop.on_viewport_resize(canvas.logical_width(), canvas.logical_height());

        let mut msg = Msg::default();
        let mut bmi = BitmapInfo {
            bmi_header: BitmapInfoHeader {
                bi_size: mem::size_of::<BitmapInfoHeader>() as u32,
                bi_width: target_cw,
                bi_height: -target_ch, // top-down
                bi_planes: 1,
                bi_bit_count: 32,
                bi_compression: 0,
                bi_size_image: 0,
                bi_x_pels_per_meter: 0,
                bi_y_pels_per_meter: 0,
                bi_clr_used: 0,
                bi_clr_important: 0,
            },
            bmi_colors: [0],
        };

        while desktop.running {
            while unsafe { PeekMessageW(&mut msg, 0, 0, 0, PM_REMOVE) } != 0 {
                if msg.message == 0x0012 {
                    // WM_QUIT
                    desktop.running = false;
                    break;
                }
                let x = (msg.l_param & 0xFFFF) as i16 as i32;
                let y = ((msg.l_param >> 16) & 0xFFFF) as i16 as i32;
                let cur_scale = desktop.settings.ui_scale();
                canvas.set_ui_scale(cur_scale);
                let log_w = canvas.logical_width();
                let log_h = canvas.logical_height();
                let lx = (x as f32 / cur_scale).round() as i32;
                let ly = (y as f32 / cur_scale).round() as i32;

                let mut swallow = false;
                match msg.message {
                    WM_MOUSEMOVE => desktop.on_mouse_move(lx, ly, log_w, log_h),
                    WM_LBUTTONDOWN => {
                        unsafe { SetCapture(hwnd) };
                        desktop.on_mouse_down(lx, ly, log_w, log_h);
                    }
                    WM_LBUTTONUP => {
                        unsafe { ReleaseCapture() };
                        desktop.on_mouse_up();
                    }
                    WM_CAPTURECHANGED => {
                        desktop.on_mouse_up();
                    }
                    WM_RBUTTONDOWN => desktop.on_right_click(lx, ly, log_w, log_h),
                    WM_SYSKEYDOWN => {
                        let alt_down =
                            (msg.l_param & (1 << 29)) != 0 || unsafe { GetKeyState(VK_MENU) } < 0;
                        if alt_down && msg.w_param == VK_SPACE {
                            desktop.on_key(KeyEvent::ToggleLauncher);
                            swallow = true;
                        } else if alt_down && msg.w_param == VK_TAB {
                            desktop.on_key(KeyEvent::SwitchWindows);
                            swallow = true;
                        }
                    }
                    WM_SYSCHAR => {
                        swallow = true;
                    }
                    WM_KEYDOWN => {
                        let ctrl_down = unsafe { GetKeyState(VK_CONTROL) } < 0;
                        let alt_down =
                            (msg.l_param & (1 << 29)) != 0 || unsafe { GetKeyState(VK_MENU) } < 0;
                        if (alt_down && msg.w_param == VK_SPACE)
                            || msg.w_param == VK_F2
                            || (ctrl_down && msg.w_param == VK_K)
                        {
                            desktop.on_key(KeyEvent::ToggleLauncher);
                            swallow = true;
                        } else if msg.w_param == VK_F1 || (ctrl_down && msg.w_param == VK_G) {
                            desktop.on_key(KeyEvent::ToggleControlCenter);
                            swallow = true;
                        } else if (ctrl_down || alt_down) && msg.w_param == VK_TAB {
                            desktop.on_key(KeyEvent::SwitchWindows);
                            swallow = true;
                        } else if ctrl_down && msg.w_param == VK_L {
                            desktop.on_key(KeyEvent::LockScreen);
                            swallow = true;
                        } else if ctrl_down && msg.w_param == VK_D {
                            desktop.on_key(KeyEvent::ShowDesktop);
                            swallow = true;
                        } else if ctrl_down && msg.w_param == VK_C {
                            desktop.on_key(KeyEvent::Copy);
                            swallow = true;
                        } else if ctrl_down && msg.w_param == VK_V {
                            if let Some(txt) = paste_from_clipboard() {
                                desktop.on_key(KeyEvent::Paste(txt));
                            }
                            swallow = true;
                        } else if ctrl_down && msg.w_param == VK_S {
                            desktop.on_key(KeyEvent::Save);
                            swallow = true;
                        } else if ctrl_down && msg.w_param == VK_F {
                            desktop.on_key(KeyEvent::Find);
                            swallow = true;
                        } else if ctrl_down && msg.w_param == VK_Z {
                            desktop.on_key(KeyEvent::Undo);
                            swallow = true;
                        } else if ctrl_down && msg.w_param == VK_1 {
                            desktop.on_key(KeyEvent::Workspace(1));
                            swallow = true;
                        } else if ctrl_down && msg.w_param == VK_2 {
                            desktop.on_key(KeyEvent::Workspace(2));
                            swallow = true;
                        } else if ctrl_down && msg.w_param == VK_3 {
                            desktop.on_key(KeyEvent::Workspace(3));
                            swallow = true;
                        } else if ctrl_down && (msg.w_param == VK_OEM_PLUS || msg.w_param == VK_ADD)
                        {
                            desktop.on_key(KeyEvent::ScaleDelta(1));
                            swallow = true;
                        } else if ctrl_down
                            && (msg.w_param == VK_OEM_MINUS || msg.w_param == VK_SUBTRACT)
                        {
                            desktop.on_key(KeyEvent::ScaleDelta(-1));
                            swallow = true;
                        } else if ctrl_down && msg.w_param == VK_0 {
                            desktop.on_key(KeyEvent::ScaleDelta(0));
                            swallow = true;
                        } else {
                            match msg.w_param {
                                VK_ESCAPE => {
                                    if !desktop.on_escape() {
                                        desktop.running = false;
                                    }
                                }
                                VK_LEFT => desktop.on_key(KeyEvent::Left),
                                VK_UP => desktop.on_key(KeyEvent::Up),
                                VK_RIGHT => desktop.on_key(KeyEvent::Right),
                                VK_DOWN => desktop.on_key(KeyEvent::Down),
                                VK_BACK => desktop.on_key(KeyEvent::Backspace),
                                VK_TAB => desktop.on_key(KeyEvent::Tab),
                                VK_RETURN => desktop.on_key(KeyEvent::Enter),
                                _ => {}
                            }
                        }
                    }
                    WM_CHAR => {
                        let alt_down = (msg.l_param & (1 << 29)) != 0;
                        let ctrl_down = unsafe { GetKeyState(VK_CONTROL) } < 0;
                        if !alt_down
                            && !ctrl_down
                            && let Some(ch) = char::from_u32(msg.w_param as u32)
                            && !ch.is_control()
                        {
                            desktop.on_key(KeyEvent::Char(ch));
                        }
                    }
                    _ => {}
                }
                if !swallow {
                    unsafe {
                        TranslateMessage(&msg);
                        DispatchMessageW(&msg);
                    }
                }
            }

            if desktop.settings.res_idx != prev_res_idx {
                prev_res_idx = desktop.settings.res_idx;
                let (rw, rh) = desktop.settings.resolution();
                let sc = desktop.settings.ui_scale();
                let nw = ((rw as f32 * sc).round() as i32)
                    .min(if scr_w > 0 { scr_w - 40 } else { 1600 })
                    .max(400);
                let nh = ((rh as f32 * sc).round() as i32)
                    .min(if scr_h > 0 { scr_h - 64 } else { 1000 })
                    .max(300);
                let mut adj = Rect {
                    left: 0,
                    top: 0,
                    right: nw,
                    bottom: nh,
                };
                unsafe {
                    AdjustWindowRectEx(&mut adj, WS_OVERLAPPEDWINDOW, 0, 0);
                    SetWindowPos(
                        hwnd,
                        0,
                        0,
                        0,
                        adj.right - adj.left,
                        adj.bottom - adj.top,
                        0x0002 | 0x0004 | 0x0010, // SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE
                    );
                }
            }

            let mut rect = Rect::default();
            if unsafe { GetClientRect(hwnd, &mut rect) } == 0 {
                break;
            }
            let cw_raw = rect.right - rect.left;
            let ch_raw = rect.bottom - rect.top;
            if cw_raw <= 0 || ch_raw <= 0 {
                std::thread::sleep(std::time::Duration::from_millis(24));
                continue;
            }
            let cw = cw_raw.max(320) as usize;
            let ch = ch_raw.max(240) as usize;
            let cur_scale = desktop.settings.ui_scale();
            if cw != canvas.width || ch != canvas.height || (cur_scale - prev_scale).abs() > 0.001
            {
                canvas.resize(cw, ch);
                canvas.set_ui_scale(cur_scale);
                desktop.on_viewport_resize(canvas.logical_width(), canvas.logical_height());
                bmi.bmi_header.bi_width = cw as i32;
                bmi.bmi_header.bi_height = -(ch as i32);
                prev_scale = cur_scale;
            }

            desktop.on_tick();
            desktop.render(&mut canvas);

            let hdc = unsafe { GetDC(hwnd) };
            if hdc != 0 {
                unsafe {
                    StretchDIBits(
                        hdc,
                        0,
                        0,
                        cw as i32,
                        ch as i32,
                        0,
                        0,
                        cw as i32,
                        ch as i32,
                        canvas.pixels.as_ptr(),
                        &bmi,
                        0,
                        SRCCOPY,
                    );
                    ReleaseDC(hwnd, hdc);
                }
            }

            std::thread::sleep(std::time::Duration::from_millis(16));
        }

        let _ = null_mut::<u8>();
        Ok(())
    }
}
