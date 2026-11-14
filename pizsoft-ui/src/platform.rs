use std::io;

use crate::apps::KeyEvent;
use crate::desktop::Desktop;
use crate::gfx::Canvas;

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
    use std::os::fd::AsRawFd;
    use std::thread;
    use std::time::Duration;

    const FBIOGET_VSCREENINFO: usize = 0x4600;
    const O_NONBLOCK: i32 = 0x800;
    const F_GETFL: i32 = 3;
    const F_SETFL: i32 = 4;

    unsafe extern "C" {
        fn ioctl(fd: i32, request: usize, ...) -> i32;
        fn fcntl(fd: i32, cmd: i32, ...) -> i32;
    }

    #[repr(C)]
    struct FbVarScreenInfo {
        xres: u32,
        yres: u32,
        xres_virtual: u32,
        yres_virtual: u32,
        xoffset: u32,
        yoffset: u32,
        bits_per_pixel: u32,
        _rest: [u32; 32],
    }

    impl Default for FbVarScreenInfo {
        fn default() -> Self {
            Self {
                xres: 0,
                yres: 0,
                xres_virtual: 0,
                yres_virtual: 0,
                xoffset: 0,
                yoffset: 0,
                bits_per_pixel: 0,
                _rest: [0; 32],
            }
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

    pub fn run_linux_fb() -> io::Result<()> {
        let mut fb = OpenOptions::new().read(true).write(true).open("/dev/fb0")?;
        let mut vinfo = FbVarScreenInfo::default();
        let rc = unsafe { ioctl(fb.as_raw_fd(), FBIOGET_VSCREENINFO, &mut vinfo) };
        if rc < 0 || vinfo.xres == 0 || vinfo.yres == 0 {
            return Err(io::Error::other("FBIOGET_VSCREENINFO failed on /dev/fb0"));
        }

        let w = vinfo.xres as usize;
        let h = vinfo.yres as usize;
        let mut canvas = Canvas::new(w, h);
        let mut desktop = Desktop::new(w as i32, h as i32);
        let mut prev_scale = desktop.settings.ui_scale();
        canvas.set_ui_scale(prev_scale);

        let mut mice = File::open("/dev/input/mice").ok();
        if let Some(ref m) = mice {
            set_nonblocking(m);
        }

        let stdin = io::stdin();
        let mut raw_buf = Vec::with_capacity(w * h * 4);
        let mut prev_right = false;

        while desktop.running {
            let cur_scale = desktop.settings.ui_scale();
            if (cur_scale - prev_scale).abs() > 0.001 {
                canvas.set_ui_scale(cur_scale);
                desktop.on_viewport_resize(canvas.logical_width(), canvas.logical_height());
                prev_scale = cur_scale;
            }
            let log_w = canvas.logical_width();
            let log_h = canvas.logical_height();

            if let Some(ref mut m) = mice {
                let mut pkt = [0u8; 3];
                while m.read_exact(&mut pkt).is_ok() {
                    let buttons = pkt[0];
                    let dx = pkt[1] as i8 as i32;
                    let dy = -(pkt[2] as i8 as i32);
                    let nx = desktop.mouse_x + dx;
                    let ny = desktop.mouse_y + dy;
                    let left = (buttons & 1) != 0;
                    let right = (buttons & 2) != 0;
                    if left && !desktop.mouse_down {
                        desktop.on_mouse_down(nx, ny, log_w, log_h);
                    } else if !left && desktop.mouse_down {
                        desktop.on_mouse_up();
                    } else {
                        desktop.on_mouse_move(nx, ny, log_w, log_h);
                    }
                    if right && !prev_right {
                        desktop.on_right_click(nx, ny, log_w, log_h);
                    }
                    prev_right = right;
                }
            }

            let _ = &stdin;
            desktop.on_tick();
            desktop.render(&mut canvas);

            raw_buf.clear();
            for &px in &canvas.pixels {
                raw_buf.extend_from_slice(&px.to_le_bytes());
            }
            fb.seek(SeekFrom::Start(0))?;
            fb.write_all(&raw_buf)?;
            fb.flush()?;

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
    const VK_G: usize = 0x47;
    const VK_K: usize = 0x4B;
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
        fn SetCapture(hwnd: Hwnd) -> Hwnd;
        fn ReleaseCapture() -> i32;
        fn ValidateRect(hwnd: Hwnd, lp_rect: *const Rect) -> i32;
        fn LoadCursorW(h_instance: Hinstance, lp_cursor_name: *const u16) -> isize;
        fn SetProcessDPIAware() -> i32;
        fn GetSystemMetrics(n_index: i32) -> i32;
        fn GetKeyState(n_virt_key: i32) -> i16;
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
        let init_scale_pct: u16 = if dpi_x >= 144 {
            150
        } else if dpi_x >= 120 || scr_w >= 1900 {
            125
        } else {
            100
        };
        let init_w = if scr_w > 0 {
            (scr_w * 78 / 100).clamp(1160, 1480)
        } else {
            1240
        };
        let init_h = if scr_h > 0 {
            (scr_h * 82 / 100).clamp(720, 920)
        } else {
            800
        };

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
                init_w,
                init_h,
                0,
                0,
                h_inst,
                null(),
            )
        };
        if hwnd == 0 {
            return Err(io::Error::other("CreateWindowExW failed"));
        }

        let mut canvas = Canvas::new(init_w as usize, init_h as usize);
        let mut desktop = Desktop::new(init_w, init_h);
        desktop.settings.ui_scale_pct = init_scale_pct;
        let mut prev_scale = desktop.settings.ui_scale();
        canvas.set_ui_scale(prev_scale);
        desktop.on_viewport_resize(canvas.logical_width(), canvas.logical_height());

        let mut msg = Msg::default();
        let mut bmi = BitmapInfo {
            bmi_header: BitmapInfoHeader {
                bi_size: mem::size_of::<BitmapInfoHeader>() as u32,
                bi_width: init_w,
                bi_height: -init_h, // top-down
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
