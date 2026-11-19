use std::collections::HashMap;
#[cfg(target_os = "linux")]
use std::collections::VecDeque;
use std::io::{self, ErrorKind, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::OnceLock;
use std::thread;

use crate::font;

fn x11_log_enabled() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("PIZSOFT_X11_LOG").is_some())
}

fn x11_log(msg: &str) {
    if !x11_log_enabled() {
        return;
    }
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open("/tmp/pizsoft_x11.log")
    {
        let _ = writeln!(f, "{msg}");
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GuestProto {
    X11,
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    Wayland,
}

pub struct GuestSurface {
    pub gid: u32,
    pub proto: GuestProto,
    pub title: String,
    pub w: i32,
    pub h: i32,
    pub pixels: Vec<u32>,
    pub mapped: bool,
    client_id: u32,
    proto_obj_id: u32,
}

const ROOT_WIN: u32 = 0x0000_0100;
const DEF_COLORMAP: u32 = 0x0000_0101;
const ROOT_VISUAL: u32 = 0x0000_0020;
const XGE_EXT_OPCODE: u8 = 130;
const XINPUT_EXT_OPCODE: u8 = 131;
const XINPUT_FIRST_EVENT: u8 = 66;
const XINPUT_FIRST_ERROR: u8 = 129;
const RENDER_EXT_OPCODE: u8 = 132;
const RENDER_FIRST_ERROR: u8 = 142;
const SHAPE_EXT_OPCODE: u8 = 133;
const SHAPE_FIRST_EVENT: u8 = 70;

#[inline]
fn pad4(n: usize) -> usize {
    (n + 3) & !3
}

#[inline]
fn r16(b: &[u8], msb: bool) -> u16 {
    if msb {
        u16::from_be_bytes([b[0], b[1]])
    } else {
        u16::from_le_bytes([b[0], b[1]])
    }
}

#[inline]
fn r32(b: &[u8], msb: bool) -> u32 {
    if msb {
        u32::from_be_bytes([b[0], b[1], b[2], b[3]])
    } else {
        u32::from_le_bytes([b[0], b[1], b[2], b[3]])
    }
}

#[inline]
fn w16(b: &mut [u8], v: u16, msb: bool) {
    let bytes = if msb { v.to_be_bytes() } else { v.to_le_bytes() };
    b[0..2].copy_from_slice(&bytes);
}

#[inline]
fn w32(b: &mut [u8], v: u32, msb: bool) {
    let bytes = if msb { v.to_be_bytes() } else { v.to_le_bytes() };
    b[0..4].copy_from_slice(&bytes);
}

#[inline]
fn put_px(buf: &mut [u32], bw: i32, bh: i32, x: i32, y: i32, col: u32) {
    if x >= 0 && y >= 0 && x < bw && y < bh {
        buf[(y * bw + x) as usize] = col & 0x00FF_FFFF;
    }
}

#[inline]
fn put_px_raw(buf: &mut [u32], bw: i32, bh: i32, x: i32, y: i32, col: u32) {
    if x >= 0 && y >= 0 && x < bw && y < bh {
        buf[(y * bw + x) as usize] = col;
    }
}

#[inline]
fn blend_px_rgb(buf: &mut [u32], bw: i32, bh: i32, x: i32, y: i32, rgb: u32, a: u32) {
    if x < 0 || y < 0 || x >= bw || y >= bh || a == 0 {
        return;
    }
    let idx = (y * bw + x) as usize;
    if a >= 255 {
        buf[idx] = rgb & 0x00FF_FFFF;
        return;
    }
    let dst = buf[idx];
    let inv = 255 - a;
    let r = (((rgb >> 16) & 0xFF) * a + ((dst >> 16) & 0xFF) * inv + 127) / 255;
    let g = (((rgb >> 8) & 0xFF) * a + ((dst >> 8) & 0xFF) * inv + 127) / 255;
    let b = ((rgb & 0xFF) * a + (dst & 0xFF) * inv + 127) / 255;
    buf[idx] = (r << 16) | (g << 8) | b;
}

fn fill_rect_buf(buf: &mut [u32], (bw, bh): (i32, i32), x: i32, y: i32, w: i32, h: i32, col: u32) {
    let x0 = x.max(0);
    let y0 = y.max(0);
    let x1 = (x + w).min(bw);
    let y1 = (y + h).min(bh);
    let c = col & 0x00FF_FFFF;
    for py in y0..y1 {
        let row = (py * bw) as usize;
        for px in x0..x1 {
            buf[row + px as usize] = c;
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn blend_rect_buf(
    buf: &mut [u32],
    (bw, bh): (i32, i32),
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    col: u32,
    alpha: u32,
) {
    if alpha == 0 {
        return;
    }
    if alpha >= 254 {
        fill_rect_buf(buf, (bw, bh), x, y, w, h, col);
        return;
    }
    let x0 = x.max(0);
    let y0 = y.max(0);
    let x1 = (x + w).min(bw);
    let y1 = (y + h).min(bh);
    let sr = (col >> 16) & 0xFF;
    let sg = (col >> 8) & 0xFF;
    let sb = col & 0xFF;
    let inv = 255 - alpha;
    for py in y0..y1 {
        let row = (py * bw) as usize;
        for px in x0..x1 {
            let d = buf[row + px as usize];
            let r = (sr * alpha + ((d >> 16) & 0xFF) * inv) / 255;
            let g = (sg * alpha + ((d >> 8) & 0xFF) * inv) / 255;
            let b = (sb * alpha + (d & 0xFF) * inv) / 255;
            buf[row + px as usize] = (r << 16) | (g << 8) | b;
        }
    }
}

fn resolve_x11_color(raw: &str) -> (u16, u16, u16) {
    let s = raw.trim().to_ascii_lowercase();
    if let Some(hex) = s.strip_prefix('#') {
        if hex.len() == 3 {
            let r = u16::from_str_radix(&hex[0..1], 16).unwrap_or(15) * 4369;
            let g = u16::from_str_radix(&hex[1..2], 16).unwrap_or(15) * 4369;
            let b = u16::from_str_radix(&hex[2..3], 16).unwrap_or(15) * 4369;
            return (r, g, b);
        }
        if hex.len() == 6 {
            let r = u16::from_str_radix(&hex[0..2], 16).unwrap_or(255) * 257;
            let g = u16::from_str_radix(&hex[2..4], 16).unwrap_or(255) * 257;
            let b = u16::from_str_radix(&hex[4..6], 16).unwrap_or(255) * 257;
            return (r, g, b);
        }
        if hex.len() == 12 {
            let r = u16::from_str_radix(&hex[0..4], 16).unwrap_or(0xFFFF);
            let g = u16::from_str_radix(&hex[4..8], 16).unwrap_or(0xFFFF);
            let b = u16::from_str_radix(&hex[8..12], 16).unwrap_or(0xFFFF);
            return (r, g, b);
        }
    }
    if let Some(rest) = s.strip_prefix("rgb:") {
        let parts: Vec<&str> = rest.split('/').collect();
        if parts.len() == 3 {
            let parse_comp = |p: &str| -> u16 {
                let v = u16::from_str_radix(p, 16).unwrap_or(0xFF);
                match p.len() {
                    1 => v * 4369,
                    2 => v * 257,
                    3 => (v << 4) | (v >> 8),
                    _ => v,
                }
            };
            return (parse_comp(parts[0]), parse_comp(parts[1]), parse_comp(parts[2]));
        }
    }
    let clean: String = s.chars().filter(|c| !c.is_whitespace()).collect();
    if let Some(pct_s) = clean.strip_prefix("gray").or_else(|| clean.strip_prefix("grey"))
        && let Ok(pct) = pct_s.parse::<u32>()
    {
        let v = ((pct.min(100) * 65535) / 100) as u16;
        return (v, v, v);
    }
    match clean.as_str() {
        "black" => (0, 0, 0),
        "white" => (0xFFFF, 0xFFFF, 0xFFFF),
        "red" => (0xFFFF, 0, 0),
        "green" => (0, 0xFFFF, 0),
        "blue" => (0, 0, 0xFFFF),
        "yellow" => (0xFFFF, 0xFFFF, 0),
        "cyan" => (0, 0xFFFF, 0xFFFF),
        "magenta" => (0xFFFF, 0, 0xFFFF),
        "gray" | "grey" => (0xBEBE, 0xBEBE, 0xBEBE),
        "lightgray" | "lightgrey" => (0xD3D3, 0xD3D3, 0xD3D3),
        "darkgray" | "darkgrey" => (0xA9A9, 0xA9A9, 0xA9A9),
        "dimgray" | "dimgrey" => (0x6969, 0x6969, 0x6969),
        "gainsboro" => (0xDCDC, 0xDCDC, 0xDCDC),
        "silver" => (0xC0C0, 0xC0C0, 0xC0C0),
        "slategray" | "slategrey" => (0x7070, 0x8080, 0x9090),
        "lightslategray" | "lightslategrey" => (0x7777, 0x8888, 0x9999),
        "darkslategray" | "darkslategrey" => (0x2F2F, 0x4F4F, 0x4F4F),
        "steelblue" => (0x4646, 0x8282, 0xB4B4),
        "lightsteelblue" => (0xB0B0, 0xC4C4, 0xDEDE),
        "royalblue" => (0x4141, 0x6969, 0xE1E1),
        "navy" | "navyblue" => (0, 0, 0x8080),
        "midnightblue" => (0x1919, 0x1919, 0x7070),
        "lightblue" => (0xADAD, 0xD8D8, 0xE6E6),
        "skyblue" => (0x8787, 0xCECE, 0xEBEB),
        "darkblue" => (0, 0, 0x8B8B),
        "darkgreen" => (0, 0x6464, 0),
        "darkred" => (0x8B8B, 0, 0),
        "orange" => (0xFFFF, 0xA5A5, 0),
        "darkorange" => (0xFFFF, 0x8C8C, 0),
        "gold" => (0xFFFF, 0xD7D7, 0),
        "pink" => (0xFFFF, 0xC0C0, 0xCBCB),
        "purple" => (0xA0A0, 0x2020, 0xF0F0),
        "violet" => (0xEEEE, 0x8282, 0xEEEE),
        "brown" => (0xA5A5, 0x2A2A, 0x2A2A),
        "tan" => (0xD2D2, 0xB4B4, 0x8C8C),
        "beige" => (0xF5F5, 0xF5F5, 0xDCDC),
        "wheat" => (0xF5F5, 0xDEDE, 0xB3B3),
        "ivory" => (0xFFFF, 0xFFFF, 0xF0F0),
        "linen" => (0xFAFA, 0xF0F0, 0xE6E6),
        "maroon" => (0xB0B0, 0x3030, 0x6060),
        "teal" => (0, 0x8080, 0x8080),
        "olive" => (0x8080, 0x8080, 0),
        _ => (0xE0E0, 0xE0E0, 0xE0E0),
    }
}

fn draw_line_buf(
    buf: &mut [u32],
    (bw, bh): (i32, i32),
    mut x0: i32,
    mut y0: i32,
    x1: i32,
    y1: i32,
    col: u32,
) {
    let dx = (x1 - x0).abs();
    let sx = if x0 < x1 { 1 } else { -1 };
    let dy = -(y1 - y0).abs();
    let sy = if y0 < y1 { 1 } else { -1 };
    let mut err = dx + dy;
    loop {
        put_px(buf, bw, bh, x0, y0, col);
        if x0 == x1 && y0 == y1 {
            break;
        }
        let e2 = 2 * err;
        if e2 >= dy {
            err += dy;
            x0 += sx;
        }
        if e2 <= dx {
            err += dx;
            y0 += sy;
        }
    }
}

fn draw_text_buf(buf: &mut [u32], (bw, bh): (i32, i32), x: i32, y: i32, text: &str, fg: u32, bg: Option<u32>) {
    let mut cx = x;
    let fr = (fg >> 16) & 0xFF;
    let fg_g = (fg >> 8) & 0xFF;
    let fb = fg & 0xFF;
    for ch in text.chars() {
        let g = font::rasterize_scaled_cached(ch, 1, 1.0);
        if let Some(bgc) = bg {
            fill_rect_buf(buf, (bw, bh), cx, y - 11, g.advance.max(6), font::GLYPH_H as i32, bgc);
        }
        for ry in 0..g.height {
            let py = y - 11 + ry as i32;
            if py < 0 || py >= bh {
                continue;
            }
            let row_off = ry * g.width;
            for rx in 0..g.width {
                let px = cx + rx as i32;
                if px < 0 || px >= bw {
                    continue;
                }
                let a = g.alpha[row_off + rx] as u32;
                if a == 0 {
                    continue;
                }
                let dst = &mut buf[(py * bw + px) as usize];
                if a >= 254 {
                    *dst = fg & 0x00FF_FFFF;
                } else {
                    let dr = (*dst >> 16) & 0xFF;
                    let dg = (*dst >> 8) & 0xFF;
                    let db = *dst & 0xFF;
                    let inv = 255 - a;
                    let r = (fr * a + dr * inv) / 255;
                    let gr = (fg_g * a + dg * inv) / 255;
                    let b = (fb * a + db * inv) / 255;
                    *dst = (r << 16) | (gr << 8) | b;
                }
            }
        }
        cx += g.advance;
    }
}

fn measure_text_advance(text: &str) -> i32 {
    text.chars()
        .map(|ch| font::rasterize_scaled_cached(ch, 1, 1.0).advance)
        .sum()
}

fn stroke_rect_buf(
    buf: &mut [u32],
    (bw, bh): (i32, i32),
    (x, y, w, h): (i32, i32, i32, i32),
    lw: i32,
    col: u32,
) {
    if w < 0 || h < 0 {
        return;
    }
    let lw = lw.clamp(1, 8);
    let rw = (w + 1).max(1);
    let rh = (h + 1).max(1);
    fill_rect_buf(buf, (bw, bh), x, y, rw, lw.min(rh), col);
    fill_rect_buf(buf, (bw, bh), x, y + rh - lw.min(rh), rw, lw.min(rh), col);
    fill_rect_buf(buf, (bw, bh), x, y, lw.min(rw), rh, col);
    fill_rect_buf(buf, (bw, bh), x + rw - lw.min(rw), y, lw.min(rw), rh, col);
}

fn fill_poly_buf(buf: &mut [u32], (bw, bh): (i32, i32), pts: &[(i32, i32)], col: u32) {
    if pts.len() < 3 || bw <= 0 || bh <= 0 {
        return;
    }
    let mut min_y = i32::MAX;
    let mut max_y = i32::MIN;
    for &(_, py) in pts {
        min_y = min_y.min(py);
        max_y = max_y.max(py);
    }
    let y0 = min_y.max(0);
    let y1 = max_y.min(bh - 1);
    let c = col & 0x00FF_FFFF;
    let mut xs = Vec::with_capacity(pts.len());
    for sy in y0..=y1 {
        xs.clear();
        for i in 0..pts.len() {
            let (x0, y_a) = pts[i];
            let (x1, y_b) = pts[(i + 1) % pts.len()];
            if (y_a <= sy && y_b > sy) || (y_b <= sy && y_a > sy) {
                let t_num = (sy - y_a) as f32 / (y_b - y_a) as f32;
                xs.push((x0 as f32 + t_num * (x1 - x0) as f32).round() as i32);
            }
        }
        xs.sort_unstable();
        for pair in xs.as_chunks::<2>().0 {
            let sx0 = pair[0].max(0);
            let sx1 = pair[1].min(bw - 1);
            let row = (sy * bw) as usize;
            for sx in sx0..=sx1 {
                buf[row + sx as usize] = c;
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_arc_buf(
    buf: &mut [u32],
    (bw, bh): (i32, i32),
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    a1_64: i16,
    a2_64: i16,
    lw: i32,
    col: u32,
    filled: bool,
) {
    if w <= 0 || h <= 0 || a2_64 == 0 {
        return;
    }
    let rx = (w as f32) * 0.5;
    let ry = (h as f32) * 0.5;
    let cx = x as f32 + rx;
    let cy = y as f32 + ry;
    if filled && a2_64.unsigned_abs() >= 23000 {
        let y0 = y.max(0);
        let y1 = (y + h).min(bh - 1);
        let c = col & 0x00FF_FFFF;
        for py in y0..=y1 {
            let dy = (py as f32 + 0.5) - cy;
            let norm = dy / ry.max(0.5);
            if norm.abs() <= 1.0 {
                let dx = rx * (1.0 - norm * norm).max(0.0).sqrt();
                let sx0 = (cx - dx).ceil().max(0.0) as i32;
                let sx1 = (cx + dx).floor().min((bw - 1) as f32) as i32;
                if sx0 <= sx1 {
                    let row = (py * bw) as usize;
                    for px in sx0..=sx1 {
                        buf[row + px as usize] = c;
                    }
                }
            }
        }
        return;
    }
    let steps = ((w + h) / 2).clamp(20, 96) as usize;
    let start_rad = (a1_64 as f32 / 64.0).to_radians();
    let sweep_rad = (a2_64 as f32 / 64.0).to_radians();
    let mut pts = Vec::with_capacity(steps + 3);
    if filled {
        pts.push((cx.round() as i32, cy.round() as i32));
    }
    for i in 0..=steps {
        let ang = start_rad + sweep_rad * (i as f32 / steps as f32);
        let px = (cx + rx * ang.cos()).round() as i32;
        let py = (cy - ry * ang.sin()).round() as i32;
        pts.push((px, py));
    }
    if filled {
        fill_poly_buf(buf, (bw, bh), &pts, col);
    } else {
        for i in 0..steps {
            draw_line_buf(buf, (bw, bh), pts[i].0, pts[i].1, pts[i + 1].0, pts[i + 1].1, col);
            if lw > 1 {
                draw_line_buf(buf, (bw, bh), pts[i].0 + 1, pts[i].1, pts[i + 1].0 + 1, pts[i + 1].1, col);
                draw_line_buf(buf, (bw, bh), pts[i].0, pts[i].1 + 1, pts[i + 1].0, pts[i + 1].1 + 1, col);
            }
        }
    }
}

fn x11_keysym(kc: u8, shifted: bool) -> u32 {
    match kc {
        9 => 0xFF1B,  // Escape
        22 => 0xFF08, // BackSpace
        23 => 0xFF09, // Tab
        36 => 0xFF0D, // Return
        37 => 0xFFE3, // Control_L
        50 => 0xFFE1, // Shift_L
        64 => 0xFFE9, // Alt_L
        65 => 0x0020, // space
        66 => 0xFFE5, // Caps_Lock
        67..=160 => {
            let ch = (kc - 67) + 0x21;
            if shifted && ch.is_ascii_lowercase() {
                (ch - 32) as u32
            } else {
                ch as u32
            }
        }
        161 => 0xFF51, // Left
        162 => 0xFF52, // Up
        163 => 0xFF53, // Right
        164 => 0xFF54, // Down
        165 => 0xFFFF, // Delete
        166 => 0xFF50, // Home
        167 => 0xFF57, // End
        _ => 0,
    }
}

fn char_to_x11_keycode(ch: Option<char>) -> u8 {
    match ch {
        None => 36,
        Some('\n' | '\r') => 36,
        Some('\x08') => 22,
        Some('\t') => 23,
        Some('\x1b') => 9,
        Some(' ') => 65,
        Some(c) if (0x21..=0x7E).contains(&(c as u32)) => ((c as u32 - 0x21) + 67) as u8,
        Some(_) => 65,
    }
}

#[cfg(target_os = "linux")]
mod unix_ipc {
    use std::io::{self, ErrorKind, Write};
    use std::mem;
    use std::os::fd::{AsRawFd, FromRawFd, RawFd};
    use std::os::unix::net::{UnixListener, UnixStream};

    const PROT_READ: i32 = 1;
    const MAP_SHARED: i32 = 1;
    const MAP_FAILED: *mut u8 = usize::MAX as *mut u8;
    const SOL_SOCKET: i32 = 1;
    const SCM_RIGHTS: i32 = 1;
    const MSG_DONTWAIT: i32 = 0x40;
    const F_GETFL: i32 = 3;
    const F_SETFL: i32 = 4;
    const O_NONBLOCK: i32 = 0x800;

    #[repr(C)]
    struct IoVec {
        iov_base: *mut u8,
        iov_len: usize,
    }

    #[repr(C)]
    struct MsgHdr {
        msg_name: *mut u8,
        msg_namelen: u32,
        _pad1: u32,
        msg_iov: *mut IoVec,
        msg_iovlen: i32,
        _pad2: u32,
        msg_control: *mut u8,
        msg_controllen: u32,
        _pad3: u32,
        msg_flags: i32,
        _pad4: u32,
    }

    #[repr(C)]
    struct CmsgHdr {
        cmsg_len: usize,
        cmsg_level: i32,
        cmsg_type: i32,
    }

    unsafe extern "C" {
        fn fcntl(fd: i32, cmd: i32, ...) -> i32;
        fn recvmsg(sockfd: i32, msg: *mut MsgHdr, flags: i32) -> isize;
        fn sendmsg(sockfd: i32, msg: *const MsgHdr, flags: i32) -> isize;
        fn mmap(addr: *mut u8, len: usize, prot: i32, flags: i32, fd: i32, offset: i64) -> *mut u8;
        fn munmap(addr: *mut u8, len: usize) -> i32;
        fn close(fd: i32) -> i32;
    }

    pub struct ShmMap {
        ptr: *mut u8,
        len: usize,
        fd: RawFd,
    }

    impl ShmMap {
        pub fn map(fd: RawFd, size: usize) -> io::Result<Self> {
            if size == 0 || fd < 0 {
                return Err(io::Error::other("invalid shm map size/fd"));
            }
            let ptr = unsafe { mmap(std::ptr::null_mut(), size, PROT_READ, MAP_SHARED, fd, 0) };
            if ptr == MAP_FAILED || ptr.is_null() {
                unsafe { close(fd) };
                return Err(io::Error::last_os_error());
            }
            Ok(Self { ptr, len: size, fd })
        }

        pub fn resize(&mut self, new_size: usize) -> io::Result<()> {
            if new_size == 0 {
                return Ok(());
            }
            let new_ptr = unsafe { mmap(std::ptr::null_mut(), new_size, PROT_READ, MAP_SHARED, self.fd, 0) };
            if new_ptr == MAP_FAILED || new_ptr.is_null() {
                return Err(io::Error::last_os_error());
            }
            unsafe {
                munmap(self.ptr, self.len);
            }
            self.ptr = new_ptr;
            self.len = new_size;
            Ok(())
        }

        pub fn slice(&self) -> &[u8] {
            unsafe { std::slice::from_raw_parts(self.ptr, self.len) }
        }
    }

    impl Drop for ShmMap {
        fn drop(&mut self) {
            unsafe {
                if !self.ptr.is_null() && self.ptr != MAP_FAILED {
                    munmap(self.ptr, self.len);
                }
                if self.fd >= 0 {
                    close(self.fd);
                }
            }
        }
    }

    pub fn set_nonblocking_unix(s: &UnixStream) {
        let fd = s.as_raw_fd();
        unsafe {
            let f = fcntl(fd, F_GETFL);
            if f >= 0 {
                fcntl(fd, F_SETFL, f | O_NONBLOCK);
            }
        }
    }

    pub fn recv_with_fds(stream: &UnixStream, out: &mut Vec<u8>, fds: &mut std::collections::VecDeque<RawFd>) -> io::Result<usize> {
        let mut buf = [0u8; 4096];
        let mut cbuf = [0u64; 16];
        let mut iov = IoVec {
            iov_base: buf.as_mut_ptr(),
            iov_len: buf.len(),
        };
        let mut msg = MsgHdr {
            msg_name: std::ptr::null_mut(),
            msg_namelen: 0,
            _pad1: 0,
            msg_iov: &mut iov,
            msg_iovlen: 1,
            _pad2: 0,
            msg_control: cbuf.as_mut_ptr() as *mut u8,
            msg_controllen: mem::size_of_val(&cbuf) as u32,
            _pad3: 0,
            msg_flags: 0,
            _pad4: 0,
        };
        let n = unsafe { recvmsg(stream.as_raw_fd(), &mut msg, MSG_DONTWAIT) };
        if n < 0 {
            let e = io::Error::last_os_error();
            return Err(e);
        }
        let nu = n as usize;
        if nu > 0 {
            out.extend_from_slice(&buf[..nu]);
        }
        let ctrl_ptr = cbuf.as_ptr() as *const u8;
        let ctrl_len = msg.msg_controllen as usize;
        let hdr_sz = mem::size_of::<CmsgHdr>();
        let mut off = 0usize;
        while off + hdr_sz <= ctrl_len {
            let hdr = unsafe { &*(ctrl_ptr.add(off) as *const CmsgHdr) };
            let cmsg_len = hdr.cmsg_len;
            if cmsg_len < hdr_sz || off + cmsg_len > ctrl_len {
                break;
            }
            if hdr.cmsg_level == SOL_SOCKET && hdr.cmsg_type == SCM_RIGHTS {
                let data_bytes = cmsg_len - hdr_sz;
                let count = data_bytes / mem::size_of::<i32>();
                let fd_ptr = unsafe { ctrl_ptr.add(off + hdr_sz) as *const i32 };
                for idx in 0..count {
                    let rfd = unsafe { *fd_ptr.add(idx) };
                    if rfd >= 0 {
                        fds.push_back(rfd);
                    }
                }
            }
            off += (cmsg_len + mem::size_of::<usize>() - 1) & !(mem::size_of::<usize>() - 1);
        }
        Ok(nu)
    }

    pub fn send_with_fd(stream: &mut UnixStream, bytes: &[u8], fd_to_send: Option<RawFd>) -> io::Result<()> {
        let Some(rfd) = fd_to_send else {
            return match stream.write_all(bytes) {
                Ok(()) => Ok(()),
                Err(e) if e.kind() == ErrorKind::WouldBlock => Ok(()),
                Err(e) => Err(e),
            };
        };
        let mut cbuf = [0u64; 4];
        let hdr_sz = mem::size_of::<CmsgHdr>();
        let cmsg_len = hdr_sz + mem::size_of::<i32>();
        let cptr = cbuf.as_mut_ptr() as *mut u8;
        unsafe {
            let hdr = &mut *(cptr as *mut CmsgHdr);
            hdr.cmsg_len = cmsg_len;
            hdr.cmsg_level = SOL_SOCKET;
            hdr.cmsg_type = SCM_RIGHTS;
            let data = cptr.add(hdr_sz) as *mut i32;
            *data = rfd;
        }
        let mut iov = IoVec {
            iov_base: bytes.as_ptr() as *mut u8,
            iov_len: bytes.len(),
        };
        let msg = MsgHdr {
            msg_name: std::ptr::null_mut(),
            msg_namelen: 0,
            _pad1: 0,
            msg_iov: &mut iov,
            msg_iovlen: 1,
            _pad2: 0,
            msg_control: cptr,
            msg_controllen: ((cmsg_len + 7) & !7) as u32,
            _pad3: 0,
            msg_flags: 0,
            _pad4: 0,
        };
        let n = unsafe { sendmsg(stream.as_raw_fd(), &msg, 0) };
        if n < 0 {
            let e = io::Error::last_os_error();
            if e.kind() == ErrorKind::WouldBlock {
                return Ok(());
            }
            return Err(e);
        }
        Ok(())
    }

    pub fn open_dev_null_fd() -> Option<std::fs::File> {
        std::fs::File::open("/dev/null").ok()
    }

    pub fn try_bind_x11_unix() -> Option<UnixListener> {
        let _ = std::fs::create_dir_all("/tmp/.X11-unix");
        let path = "/tmp/.X11-unix/X0";
        let _ = std::fs::remove_file(path);
        let l = UnixListener::bind(path).ok()?;
        let _ = l.set_nonblocking(true);
        Some(l)
    }

    pub fn try_bind_wayland_unix(runtime_dir: &std::path::Path) -> Option<UnixListener> {
        let _ = std::fs::create_dir_all(runtime_dir);
        let sock = runtime_dir.join("piz-0");
        let _ = std::fs::remove_file(&sock);
        let l = UnixListener::bind(&sock).ok()?;
        let _ = l.set_nonblocking(true);
        Some(l)
    }

    pub fn accept_unix(l: &UnixListener) -> Option<UnixStream> {
        match l.accept() {
            Ok((s, _)) => {
                set_nonblocking_unix(&s);
                Some(s)
            }
            Err(_) => None,
        }
    }

    pub fn duplicate_fd(f: &std::fs::File) -> RawFd {
        f.as_raw_fd()
    }

    pub fn close_fds(fds: &mut std::collections::VecDeque<RawFd>) {
        while let Some(fd) = fds.pop_front() {
            unsafe {
                let _ = std::fs::File::from_raw_fd(fd);
            }
        }
    }
}

enum X11Io {
    Tcp(TcpStream),
    #[cfg(target_os = "linux")]
    Unix(std::os::unix::net::UnixStream),
}

impl X11Io {
    fn try_read(&mut self, dst: &mut Vec<u8>) -> io::Result<usize> {
        let mut total = 0usize;
        let mut tmp = [0u8; 16384];
        loop {
            let res = match self {
                X11Io::Tcp(s) => s.read(&mut tmp),
                #[cfg(target_os = "linux")]
                X11Io::Unix(s) => s.read(&mut tmp),
            };
            match res {
                Ok(0) => {
                    return if total > 0 {
                        Ok(total)
                    } else {
                        Err(io::Error::new(ErrorKind::UnexpectedEof, "eof"))
                    };
                }
                Ok(n) => {
                    dst.extend_from_slice(&tmp[..n]);
                    total += n;
                    if total >= 262_144 {
                        return Ok(total);
                    }
                }
                Err(e) if e.kind() == ErrorKind::WouldBlock => return Ok(total),
                Err(e) => return if total > 0 { Ok(total) } else { Err(e) },
            }
        }
    }

    fn write_bytes(&mut self, data: &[u8]) -> io::Result<()> {
        match self {
            X11Io::Tcp(s) => s.write_all(data),
            #[cfg(target_os = "linux")]
            X11Io::Unix(s) => s.write_all(data),
        }
    }
}

#[derive(Clone)]
struct XWin {
    parent: u32,
    gid: Option<u32>,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    border_width: i32,
    bg_pixel: u32,
    border_pixel: u32,
    has_bg: bool,
    has_border: bool,
    event_mask: u32,
    z: u32,
    mapped: bool,
}

#[derive(Clone)]
struct XPixmap {
    w: i32,
    h: i32,
    pixels: Vec<u32>,
}

#[derive(Clone, Copy)]
struct XGc {
    fg: u32,
    bg: u32,
    line_width: i32,
}

impl Default for XGc {
    fn default() -> Self {
        Self {
            fg: 0x0000_0000,
            bg: 0x00FF_FFFF,
            line_width: 1,
        }
    }
}

#[derive(Clone, Copy)]
enum XRenderPic {
    Drawable { did: u32, fmt: u32, repeat: bool },
    Solid(u32),
}

#[derive(Clone)]
struct XRenderGlyph {
    w: u16,
    h: u16,
    x_off: i16,
    y_off: i16,
    alpha: Vec<u8>,
}

struct X11Client {
    id: u32,
    io: X11Io,
    rx: Vec<u8>,
    tx: Vec<u8>,
    msb: bool,
    authed: bool,
    seq: u16,
    windows: HashMap<u32, XWin>,
    pixmaps: HashMap<u32, XPixmap>,
    gcs: HashMap<u32, XGc>,
    pictures: HashMap<u32, XRenderPic>,
    glyphsets: HashMap<u32, HashMap<u32, XRenderGlyph>>,
    atoms: HashMap<u32, String>,
    atom_names: HashMap<String, u32>,
    props: HashMap<(u32, u32), (u32, u8, Vec<u8>)>,
    next_atom: u32,
    next_z: u32,
    xi_selected: bool,
    xi_raw_motion: bool,
    dead: bool,
}

impl X11Client {
    fn new(id: u32, io: X11Io) -> Self {
        let mut atoms = HashMap::new();
        let mut atom_names = HashMap::new();
        let preset = [
            (1, "PRIMARY"),
            (2, "SECONDARY"),
            (4, "ATOM"),
            (6, "CARDINAL"),
            (31, "STRING"),
            (39, "WM_NAME"),
            (40, "WM_ICON_NAME"),
            (68, "WM_PROTOCOLS"),
            (69, "WM_DELETE_WINDOW"),
            (70, "UTF8_STRING"),
            (71, "_NET_WM_NAME"),
            (72, "Abs X"),
            (73, "Abs Y"),
        ];
        for (aid, name) in preset {
            atoms.insert(aid, String::from(name));
            atom_names.insert(String::from(name), aid);
        }
        Self {
            id,
            io,
            rx: Vec::with_capacity(4096),
            tx: Vec::with_capacity(1024),
            msb: false,
            authed: false,
            seq: 0,
            windows: HashMap::new(),
            pixmaps: HashMap::new(),
            gcs: HashMap::new(),
            pictures: HashMap::new(),
            glyphsets: HashMap::new(),
            atoms,
            atom_names,
            props: HashMap::new(),
            next_atom: 100,
            next_z: 1,
            xi_selected: false,
            xi_raw_motion: false,
            dead: false,
        }
    }

    fn flush_tx(&mut self) {
        if !self.tx.is_empty() {
            if self.io.write_bytes(&self.tx).is_err() {
                self.dead = true;
            }
            self.tx.clear();
        }
    }

    fn send_setup_reply(&mut self, rid_base: u32, sw: u16, sh: u16) {
        let mut reply = [0u8; 8 + 132];
        reply[0] = 1; // Success
        w16(&mut reply[2..4], 11, self.msb);
        w16(&mut reply[4..6], 0, self.msb);
        w16(&mut reply[6..8], 33, self.msb); // 132 / 4

        let b = &mut reply[8..];
        w32(&mut b[0..4], 2100, self.msb);
        w32(&mut b[4..8], rid_base, self.msb);
        w32(&mut b[8..12], 0x001F_FFFF, self.msb);
        w32(&mut b[12..16], 256, self.msb);
        w16(&mut b[16..18], 9, self.msb); // vendor len
        w16(&mut b[18..20], 65535, self.msb);
        b[20] = 1; // num screens
        b[21] = 2; // num formats
        b[22] = 0; // LSBFirst image order
        b[23] = 0; // LSBFirst bitmap order
        b[24] = 32;
        b[25] = 32;
        b[26] = 8; // min keycode
        b[27] = 255; // max keycode
        b[32..41].copy_from_slice(b"pizdOS-X1");

        // Formats (16 bytes at offset 44)
        b[44] = 24;
        b[45] = 32;
        b[46] = 32;
        b[52] = 1;
        b[53] = 1;
        b[54] = 32;

        // Screen (40 bytes at offset 60)
        w32(&mut b[60..64], ROOT_WIN, self.msb);
        w32(&mut b[64..68], DEF_COLORMAP, self.msb);
        w32(&mut b[68..72], 0x00FF_FFFF, self.msb);
        w32(&mut b[72..76], 0x0000_0000, self.msb);
        w32(&mut b[76..80], 0, self.msb);
        w16(&mut b[80..82], sw.max(320), self.msb);
        w16(&mut b[82..84], sh.max(240), self.msb);
        w16(&mut b[84..86], (sw.max(320) as u32 * 254 / 960) as u16, self.msb);
        w16(&mut b[86..88], (sh.max(240) as u32 * 254 / 960) as u16, self.msb);
        w16(&mut b[88..90], 1, self.msb);
        w16(&mut b[90..92], 1, self.msb);
        w32(&mut b[92..96], ROOT_VISUAL, self.msb);
        b[96] = 0;
        b[97] = 0;
        b[98] = 24;
        b[99] = 1; // 1 depth

        // Depth 24 header (8 bytes at offset 100)
        b[100] = 24;
        w16(&mut b[102..104], 1, self.msb);

        // VisualType (24 bytes at offset 108)
        w32(&mut b[108..112], ROOT_VISUAL, self.msb);
        b[112] = 4; // TrueColor
        b[113] = 8; // bits per rgb
        w16(&mut b[114..116], 256, self.msb);
        w32(&mut b[116..120], 0x00FF_0000, self.msb);
        w32(&mut b[120..124], 0x0000_FF00, self.msb);
        w32(&mut b[124..128], 0x0000_00FF, self.msb);

        self.tx.extend_from_slice(&reply);
    }

    fn push_event(&mut self, mut ev: [u8; 32]) {
        w16(&mut ev[2..4], self.seq, self.msb);
        self.tx.extend_from_slice(&ev);
    }

    fn send_expose(&mut self, wid: u32, w: i32, h: i32) {
        let mut ev = [0u8; 32];
        ev[0] = 12; // Expose
        w32(&mut ev[4..8], wid, self.msb);
        w16(&mut ev[12..14], w.clamp(1, 32767) as u16, self.msb);
        w16(&mut ev[14..16], h.clamp(1, 32767) as u16, self.msb);
        self.push_event(ev);
    }

    fn send_configure_notify(&mut self, wid: u32, x: i32, y: i32, w: i32, h: i32) {
        let mut ev = [0u8; 32];
        ev[0] = 22; // ConfigureNotify
        w32(&mut ev[4..8], wid, self.msb);
        w32(&mut ev[8..12], wid, self.msb);
        w16(&mut ev[16..18], x as i16 as u16, self.msb);
        w16(&mut ev[18..20], y as i16 as u16, self.msb);
        w16(&mut ev[20..22], w.clamp(1, 32767) as u16, self.msb);
        w16(&mut ev[22..24], h.clamp(1, 32767) as u16, self.msb);
        self.push_event(ev);
    }

    fn send_map_notify(&mut self, event_wid: u32, wid: u32) {
        let mut ev = [0u8; 32];
        ev[0] = 19; // MapNotify
        w32(&mut ev[4..8], event_wid, self.msb);
        w32(&mut ev[8..12], wid, self.msb);
        self.push_event(ev);
    }

    #[allow(clippy::too_many_arguments)]
    fn send_pointer_ev(
        &mut self,
        event_wid: u32,
        child_wid: u32,
        rx: i32,
        ry: i32,
        ex: i32,
        ey: i32,
        btn: Option<(u8, bool)>,
    ) {
        let mut ev = [0u8; 32];
        match btn {
            Some((b, true)) => {
                ev[0] = 4; // ButtonPress
                ev[1] = b;
            }
            Some((b, false)) => {
                ev[0] = 5; // ButtonRelease
                ev[1] = b;
            }
            None => {
                ev[0] = 6; // MotionNotify
                ev[1] = 0;
            }
        }
        w32(&mut ev[4..8], 1, self.msb); // time
        w32(&mut ev[8..12], ROOT_WIN, self.msb);
        w32(&mut ev[12..16], event_wid, self.msb);
        w32(&mut ev[16..20], child_wid, self.msb);
        w16(&mut ev[20..22], rx as i16 as u16, self.msb);
        w16(&mut ev[22..24], ry as i16 as u16, self.msb);
        w16(&mut ev[24..26], ex as i16 as u16, self.msb);
        w16(&mut ev[26..28], ey as i16 as u16, self.msb);
        ev[30] = 1; // same-screen
        self.push_event(ev);
    }

    fn send_xi_raw_motion(&mut self, rx: i32, ry: i32) {
        // GenericEvent (35): XI_RawMotion (evtype 17), 32-byte header + 4-byte mask + 32-byte FP3232 axes = 68 bytes (len = 9)
        let mut ev = [0u8; 68];
        ev[0] = 35;
        ev[1] = XINPUT_EXT_OPCODE;
        w16(&mut ev[2..4], self.seq, self.msb);
        w32(&mut ev[4..8], 9, self.msb);
        w16(&mut ev[8..10], 17, self.msb); // XI_RawMotion
        w16(&mut ev[10..12], 2, self.msb); // deviceid = 2
        w32(&mut ev[12..16], 1, self.msb); // time
        w16(&mut ev[18..20], 2, self.msb); // sourceid = 2
        w16(&mut ev[20..22], 1, self.msb); // valuators_len = 1 (4 bytes)
        w32(&mut ev[32..36], 0x03, self.msb); // axis 0 & 1 mask
        // FP3232: integral (i32) + frac (u32) -> axes[0]=rx, axes[1]=ry, unaccel[0]=rx, unaccel[1]=ry
        w32(&mut ev[36..40], rx as u32, self.msb);
        w32(&mut ev[44..48], ry as u32, self.msb);
        w32(&mut ev[52..56], rx as u32, self.msb);
        w32(&mut ev[60..64], ry as u32, self.msb);
        self.tx.extend_from_slice(&ev);
    }

    fn send_xi_device_motion(&mut self, event_wid: u32, child_wid: u32, rx: i32, ry: i32, ex: i32, ey: i32) {
        // GenericEvent (35): XI_Motion (evtype 6), 80 bytes total -> length = 12
        let mut ev = [0u8; 80];
        ev[0] = 35;
        ev[1] = XINPUT_EXT_OPCODE;
        w16(&mut ev[2..4], self.seq, self.msb);
        w32(&mut ev[4..8], 12, self.msb);
        w16(&mut ev[8..10], 6, self.msb); // XI_Motion
        w16(&mut ev[10..12], 2, self.msb); // deviceid = 2
        w32(&mut ev[12..16], 1, self.msb); // time
        w16(&mut ev[18..20], 2, self.msb); // sourceid = 2
        w32(&mut ev[20..24], ROOT_WIN, self.msb);
        w32(&mut ev[24..28], event_wid, self.msb);
        w32(&mut ev[28..32], child_wid, self.msb);
        w32(&mut ev[32..36], (rx << 16) as u32, self.msb);
        w32(&mut ev[36..40], (ry << 16) as u32, self.msb);
        w32(&mut ev[40..44], (ex << 16) as u32, self.msb);
        w32(&mut ev[44..48], (ey << 16) as u32, self.msb);
        self.tx.extend_from_slice(&ev);
    }

    fn send_key_ev(&mut self, wid: u32, keycode: u8, pressed: bool) {
        let mut ev = [0u8; 32];
        ev[0] = if pressed { 2 } else { 3 };
        ev[1] = keycode;
        w32(&mut ev[4..8], 1, self.msb);
        w32(&mut ev[8..12], ROOT_WIN, self.msb);
        w32(&mut ev[12..16], wid, self.msb);
        ev[30] = 1;
        self.push_event(ev);
    }

    fn send_delete_msg(&mut self, wid: u32) {
        let mut ev = [0u8; 32];
        ev[0] = 33; // ClientMessage
        ev[1] = 32; // format
        w32(&mut ev[4..8], wid, self.msb);
        w32(&mut ev[8..12], 68, self.msb); // WM_PROTOCOLS
        w32(&mut ev[12..16], 69, self.msb); // WM_DELETE_WINDOW
        w32(&mut ev[16..20], 1, self.msb);
        self.push_event(ev);
    }

    fn find_root_gid(&self, mut wid: u32) -> Option<(u32, i32, i32)> {
        let mut ox = 0i32;
        let mut oy = 0i32;
        for _ in 0..16 {
            let win = self.windows.get(&wid)?;
            if let Some(gid) = win.gid {
                return Some((gid, ox, oy));
            }
            ox += win.x + win.border_width;
            oy += win.y + win.border_width;
            if win.parent == ROOT_WIN {
                return None;
            }
            wid = win.parent;
        }
        None
    }

    fn sorted_children(&self, parent: u32) -> Vec<u32> {
        let mut children: Vec<(u32, u32)> = self
            .windows
            .iter()
            .filter(|(_, w)| w.parent == parent)
            .map(|(&id, w)| (w.z, id))
            .collect();
        children.sort_unstable_by_key(|&(z, id)| (z, id));
        children.into_iter().map(|(_, id)| id).collect()
    }

    fn find_hit_window(&self, top_wid: u32, x: i32, y: i32) -> (u32, i32, i32) {
        let mut cur = top_wid;
        let mut lx = x;
        let mut ly = y;
        for _ in 0..12 {
            let mut next_hit = None;
            for cid in self.sorted_children(cur).into_iter().rev() {
                if let Some(cw) = self.windows.get(&cid)
                    && cw.mapped
                    && lx >= cw.x
                    && lx < cw.x + cw.w + cw.border_width * 2
                    && ly >= cw.y
                    && ly < cw.y + cw.h + cw.border_width * 2
                {
                    next_hit = Some((
                        cid,
                        lx - (cw.x + cw.border_width),
                        ly - (cw.y + cw.border_width),
                    ));
                    break;
                }
            }
            if let Some((nid, nx, ny)) = next_hit {
                cur = nid;
                lx = nx;
                ly = ny;
            } else {
                break;
            }
        }
        (cur, lx, ly)
    }

    fn paint_win_on_surface(&self, surfaces: &mut HashMap<u32, GuestSurface>, wid: u32) {
        let Some(win) = self.windows.get(&wid) else {
            return;
        };
        let Some((gid, ox, oy)) = self.find_root_gid(wid) else {
            return;
        };
        let Some(surf) = surfaces.get_mut(&gid) else {
            return;
        };
        let bw = win.border_width;
        if win.has_border && bw > 0 && win.gid.is_none() {
            stroke_rect_buf(
                &mut surf.pixels,
                (surf.w, surf.h),
                (ox - bw, oy - bw, win.w + bw * 2 - 1, win.h + bw * 2 - 1),
                bw,
                win.border_pixel,
            );
        }
        if win.has_bg {
            fill_rect_buf(
                &mut surf.pixels,
                (surf.w, surf.h),
                ox,
                oy,
                win.w,
                win.h,
                win.bg_pixel,
            );
        }
    }

    fn map_and_expose_tree(
        &mut self,
        surfaces: &mut HashMap<u32, GuestSurface>,
        wid: u32,
        include_children: bool,
    ) {
        let mut queue = Vec::new();
        if include_children {
            for cid in self.sorted_children(wid) {
                queue.push(cid);
            }
        } else {
            queue.push(wid);
        }
        let mut idx = 0;
        while idx < queue.len() {
            let cur = queue[idx];
            idx += 1;
            let (parent, w, h) = if let Some(cw) = self.windows.get_mut(&cur) {
                cw.mapped = true;
                (cw.parent, cw.w, cw.h)
            } else {
                continue;
            };
            self.paint_win_on_surface(surfaces, cur);
            self.send_map_notify(parent, cur);
            self.send_expose(cur, w, h);
            for gc in self.sorted_children(cur) {
                if self.windows.get(&gc).is_some_and(|w| w.mapped) {
                    queue.push(gc);
                }
            }
        }
    }
}

#[cfg(target_os = "linux")]
enum WlObj {
    Display,
    Registry,
    Compositor,
    Shm,
    ShmPool(unix_ipc::ShmMap),
    Buffer {
        pool_id: u32,
        offset: usize,
        w: i32,
        h: i32,
        stride: usize,
        format: u32,
    },
    Surface {
        gid: Option<u32>,
        pending_buf: Option<u32>,
        pending_frame_cbs: Vec<u32>,
    },
    XdgWmBase,
    XdgSurface {
        surface_id: u32,
    },
    XdgToplevel {
        surface_id: u32,
    },
    Seat,
    Pointer,
    Keyboard,
}

#[cfg(target_os = "linux")]
struct WaylandClient {
    id: u32,
    stream: std::os::unix::net::UnixStream,
    rx: Vec<u8>,
    fds: VecDeque<std::os::fd::RawFd>,
    objs: HashMap<u32, WlObj>,
    pointer_obj: Option<u32>,
    keyboard_obj: Option<u32>,
    serial: u32,
    dead: bool,
}

#[cfg(target_os = "linux")]
impl WaylandClient {
    fn new(id: u32, stream: std::os::unix::net::UnixStream) -> Self {
        let mut objs = HashMap::new();
        objs.insert(1, WlObj::Display);
        Self {
            id,
            stream,
            rx: Vec::with_capacity(4096),
            fds: VecDeque::new(),
            objs,
            pointer_obj: None,
            keyboard_obj: None,
            serial: 1,
            dead: false,
        }
    }

    fn send_msg(&mut self, obj_id: u32, opcode: u16, payload: &[u8], fd: Option<std::os::fd::RawFd>) {
        let total = 8 + payload.len();
        let mut pkt = Vec::with_capacity(total);
        pkt.extend_from_slice(&obj_id.to_le_bytes());
        let word = ((total as u32) << 16) | (opcode as u32);
        pkt.extend_from_slice(&word.to_le_bytes());
        pkt.extend_from_slice(payload);
        if unix_ipc::send_with_fd(&mut self.stream, &pkt, fd).is_err() {
            self.dead = true;
        }
    }

    fn encode_wl_str(buf: &mut Vec<u8>, s: &str) {
        let len = s.len() + 1;
        buf.extend_from_slice(&(len as u32).to_le_bytes());
        buf.extend_from_slice(s.as_bytes());
        buf.push(0);
        while !buf.len().is_multiple_of(4) {
            buf.push(0);
        }
    }

    fn next_serial(&mut self) -> u32 {
        self.serial = self.serial.wrapping_add(1);
        self.serial
    }
}

#[cfg(target_os = "linux")]
impl Drop for WaylandClient {
    fn drop(&mut self) {
        unix_ipc::close_fds(&mut self.fds);
    }
}

pub struct DisplayServer {
    tcp_x11: Option<TcpListener>,
    pub tcp_port: Option<u16>,
    #[cfg(target_os = "linux")]
    unix_x11: Option<std::os::unix::net::UnixListener>,
    #[cfg(target_os = "linux")]
    unix_wl: Option<std::os::unix::net::UnixListener>,
    runtime_dir: Option<PathBuf>,
    pub wayland_name: Option<String>,
    x11_clients: Vec<X11Client>,
    #[cfg(target_os = "linux")]
    wl_clients: Vec<WaylandClient>,
    pub surfaces: HashMap<u32, GuestSurface>,
    surf_origins: HashMap<u32, (i32, i32)>,
    selections: HashMap<u32, u32>,
    screen_w: u16,
    screen_h: u16,
    ptr_x: i32,
    ptr_y: i32,
    next_gid: u32,
    next_cid: u32,
}

impl DisplayServer {
    pub fn disabled() -> Self {
        Self {
            tcp_x11: None,
            tcp_port: None,
            #[cfg(target_os = "linux")]
            unix_x11: None,
            #[cfg(target_os = "linux")]
            unix_wl: None,
            runtime_dir: None,
            wayland_name: None,
            x11_clients: Vec::new(),
            #[cfg(target_os = "linux")]
            wl_clients: Vec::new(),
            surfaces: HashMap::new(),
            surf_origins: HashMap::new(),
            selections: HashMap::new(),
            screen_w: 800,
            screen_h: 600,
            ptr_x: 400,
            ptr_y: 300,
            next_gid: 1,
            next_cid: 1,
        }
    }

    #[cfg(test)]
    pub fn new_tcp_test(port: u16) -> io::Result<Self> {
        let l = TcpListener::bind(("127.0.0.1", port))?;
        l.set_nonblocking(true)?;
        let actual = l.local_addr()?.port();
        Ok(Self {
            tcp_x11: Some(l),
            tcp_port: Some(actual),
            #[cfg(target_os = "linux")]
            unix_x11: None,
            #[cfg(target_os = "linux")]
            unix_wl: None,
            runtime_dir: None,
            wayland_name: None,
            x11_clients: Vec::new(),
            #[cfg(target_os = "linux")]
            wl_clients: Vec::new(),
            surfaces: HashMap::new(),
            surf_origins: HashMap::new(),
            selections: HashMap::new(),
            screen_w: 800,
            screen_h: 600,
            ptr_x: 400,
            ptr_y: 300,
            next_gid: 1,
            next_cid: 1,
        })
    }

    pub fn start_live() -> Self {
        let tcp_x11 = TcpListener::bind("127.0.0.1:6000").ok().inspect(|l| {
            let _ = l.set_nonblocking(true);
        });
        let tcp_port = tcp_x11.as_ref().and_then(|l| l.local_addr().ok()).map(|a| a.port());

        #[cfg(target_os = "linux")]
        let (unix_x11, unix_wl, runtime_dir, wayland_name) = {
            let rdir = std::env::var("XDG_RUNTIME_DIR")
                .map(PathBuf::from)
                .unwrap_or_else(|_| PathBuf::from("/tmp"));
            let ux = unix_ipc::try_bind_x11_unix();
            let uw = unix_ipc::try_bind_wayland_unix(&rdir);
            let wl_name = if uw.is_some() {
                Some(String::from("piz-0"))
            } else {
                None
            };
            (ux, uw, Some(rdir), wl_name)
        };

        #[cfg(not(target_os = "linux"))]
        let (runtime_dir, wayland_name) = (None, None);

        Self {
            tcp_x11,
            tcp_port,
            #[cfg(target_os = "linux")]
            unix_x11,
            #[cfg(target_os = "linux")]
            unix_wl,
            runtime_dir,
            wayland_name,
            x11_clients: Vec::new(),
            #[cfg(target_os = "linux")]
            wl_clients: Vec::new(),
            surfaces: HashMap::new(),
            surf_origins: HashMap::new(),
            selections: HashMap::new(),
            screen_w: 800,
            screen_h: 600,
            ptr_x: 400,
            ptr_y: 300,
            next_gid: 1,
            next_cid: 1,
        }
    }

    pub fn set_screen_size(&mut self, w: i32, h: i32) {
        self.screen_w = w.clamp(320, 4096) as u16;
        self.screen_h = h.clamp(240, 4096) as u16;
    }

    pub fn set_surface_origin(&mut self, gid: u32, sx: i32, sy: i32) {
        self.surf_origins.insert(gid, (sx, sy));
    }

    pub fn set_global_pointer(&mut self, x: i32, y: i32) {
        if self.ptr_x == x && self.ptr_y == y {
            return;
        }
        self.ptr_x = x;
        self.ptr_y = y;
        for c in &mut self.x11_clients {
            if c.xi_selected {
                if c.xi_raw_motion {
                    c.send_xi_raw_motion(x, y);
                }
                if let Some((&top_wid, tw)) = c.windows.iter().find(|(_, w)| w.gid.is_some() && w.mapped) {
                    let (sox, soy) = tw
                        .gid
                        .and_then(|g| self.surf_origins.get(&g).copied())
                        .unwrap_or((0, 0));
                    c.send_xi_device_motion(top_wid, 0, x, y, x - sox, y - soy);
                }
                c.flush_tx();
            }
        }
    }

    pub fn runtime_env(&self) -> Vec<(&'static str, String)> {
        let mut vars = Vec::new();
        if self.tcp_port.is_some() {
            #[cfg(target_os = "linux")]
            vars.push(("DISPLAY", String::from(":0")));
            #[cfg(not(target_os = "linux"))]
            if let Some(p) = self.tcp_port {
                let disp = p.saturating_sub(6000);
                vars.push(("DISPLAY", format!("127.0.0.1:{disp}")));
            }
        }
        if let Some(ref wl) = self.wayland_name {
            vars.push(("WAYLAND_DISPLAY", wl.clone()));
            if let Some(ref rd) = self.runtime_dir {
                vars.push(("XDG_RUNTIME_DIR", rd.display().to_string()));
            }
        }
        vars
    }

    pub fn window(&self, gid: u32) -> Option<&GuestSurface> {
        self.surfaces.get(&gid)
    }

    pub fn poll(&mut self) -> (Vec<u32>, Vec<u32>) {
        let mut newly_mapped = Vec::new();
        let mut unmapped = Vec::new();

        if let Some(ref listener) = self.tcp_x11 {
            while let Ok((stream, _)) = listener.accept() {
                let _ = stream.set_nonblocking(true);
                let _ = stream.set_nodelay(true);
                let cid = self.next_cid;
                self.next_cid += 1;
                self.x11_clients.push(X11Client::new(cid, X11Io::Tcp(stream)));
            }
        }

        #[cfg(target_os = "linux")]
        if let Some(ref listener) = self.unix_x11 {
            while let Some(stream) = unix_ipc::accept_unix(listener) {
                let cid = self.next_cid;
                self.next_cid += 1;
                self.x11_clients.push(X11Client::new(cid, X11Io::Unix(stream)));
            }
        }

        #[cfg(target_os = "linux")]
        if let Some(ref listener) = self.unix_wl {
            while let Some(stream) = unix_ipc::accept_unix(listener) {
                let cid = self.next_cid;
                self.next_cid += 1;
                self.wl_clients.push(WaylandClient::new(cid, stream));
            }
        }

        let mut i = 0;
        while i < self.x11_clients.len() {
            let mut client = self.x11_clients.remove(i);
            match client.io.try_read(&mut client.rx) {
                Ok(_) => self.process_x11_client(&mut client, &mut newly_mapped, &mut unmapped),
                Err(_) => client.dead = true,
            }
            client.flush_tx();
            if client.dead {
                let gids: Vec<u32> = self
                    .surfaces
                    .values()
                    .filter(|s| s.client_id == client.id)
                    .map(|s| s.gid)
                    .collect();
                for gid in gids {
                    self.surfaces.remove(&gid);
                    unmapped.push(gid);
                }
            } else {
                self.x11_clients.insert(i, client);
                i += 1;
            }
        }

        #[cfg(target_os = "linux")]
        {
            let mut wi = 0;
            while wi < self.wl_clients.len() {
                let mut client = self.wl_clients.remove(wi);
                match unix_ipc::recv_with_fds(&client.stream, &mut client.rx, &mut client.fds) {
                    Ok(0) => {}
                    Ok(_) => self.process_wl_client(&mut client, &mut newly_mapped, &mut unmapped),
                    Err(e) if e.kind() == ErrorKind::WouldBlock => {}
                    Err(_) => client.dead = true,
                }
                if client.dead {
                    let gids: Vec<u32> = self
                        .surfaces
                        .values()
                        .filter(|s| s.client_id == client.id)
                        .map(|s| s.gid)
                        .collect();
                    for gid in gids {
                        self.surfaces.remove(&gid);
                        unmapped.push(gid);
                    }
                } else {
                    self.wl_clients.insert(wi, client);
                    wi += 1;
                }
            }
        }

        (newly_mapped, unmapped)
    }

    fn process_x11_client(
        &mut self,
        c: &mut X11Client,
        newly_mapped: &mut Vec<u32>,
        unmapped: &mut Vec<u32>,
    ) {
        if !c.authed {
            if c.rx.len() < 12 {
                return;
            }
            c.msb = c.rx[0] == b'B';
            let auth_name_len = r16(&c.rx[6..8], c.msb) as usize;
            let auth_data_len = r16(&c.rx[8..10], c.msb) as usize;
            let total = 12 + pad4(auth_name_len) + pad4(auth_data_len);
            if c.rx.len() < total {
                return;
            }
            c.rx.drain(0..total);
            c.authed = true;
            let base = (c.id & 0xFF) << 21 | 0x0040_0000;
            c.send_setup_reply(base, self.screen_w, self.screen_h);
        }

        while c.rx.len() >= 4 {
            let op = c.rx[0];
            let detail = c.rx[1];
            let req_words = r16(&c.rx[2..4], c.msb) as usize;
            if req_words == 0 {
                c.dead = true;
                return;
            }
            let req_bytes = req_words * 4;
            if c.rx.len() < req_bytes {
                return;
            }
            let pkt: Vec<u8> = c.rx.drain(0..req_bytes).collect();
            c.seq = c.seq.wrapping_add(1);
            self.handle_x11_req(c, op, detail, &pkt, newly_mapped, unmapped);
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn handle_x11_req(
        &mut self,
        c: &mut X11Client,
        op: u8,
        detail: u8,
        pkt: &[u8],
        newly_mapped: &mut Vec<u32>,
        unmapped: &mut Vec<u32>,
    ) {
        let msb = c.msb;
        if x11_log_enabled() {
            x11_log(&format!(
                "c{} seq={} op={} det={} len={}",
                c.id,
                c.seq,
                op,
                detail,
                pkt.len()
            ));
        }
        match op {
            1 if pkt.len() >= 32 => {
                // CreateWindow
                let wid = r32(&pkt[4..8], msb);
                let parent = r32(&pkt[8..12], msb);
                let x = r16(&pkt[12..14], msb) as i16 as i32;
                let y = r16(&pkt[14..16], msb) as i16 as i32;
                let raw_w = (r16(&pkt[16..18], msb) as i32).max(1);
                let raw_h = (r16(&pkt[18..20], msb) as i32).max(1);
                let (w, h) = if parent == ROOT_WIN {
                    (raw_w.clamp(64, 1600), raw_h.clamp(48, 1200))
                } else {
                    (raw_w.min(1600), raw_h.min(1200))
                };
                let border_width = (r16(&pkt[20..22], msb) as i32).clamp(0, 16);
                let mask = r32(&pkt[28..32], msb);
                let z = c.next_z;
                c.next_z += 1;
                let mut xwin = XWin {
                    parent,
                    gid: None,
                    x,
                    y,
                    w,
                    h,
                    border_width,
                    bg_pixel: if parent == ROOT_WIN { 0x000D_1117 } else { 0x00F0_F0F0 },
                    border_pixel: 0x0000_0000,
                    has_bg: parent == ROOT_WIN,
                    has_border: false,
                    event_mask: 0,
                    z,
                    mapped: false,
                };
                parse_win_attrs(&mut xwin, mask, &pkt[32..], msb);
                let gid = if parent == ROOT_WIN {
                    let g = self.next_gid;
                    self.next_gid += 1;
                    self.surfaces.insert(
                        g,
                        GuestSurface {
                            gid: g,
                            proto: GuestProto::X11,
                            title: format!("X11 App #{g}"),
                            w,
                            h,
                            pixels: vec![xwin.bg_pixel & 0x00FF_FFFF; (w * h) as usize],
                            mapped: false,
                            client_id: c.id,
                            proto_obj_id: wid,
                        },
                    );
                    Some(g)
                } else {
                    None
                };
                xwin.gid = gid;
                c.windows.insert(wid, xwin);
            }
            2 if pkt.len() >= 12 => {
                // ChangeWindowAttributes
                let wid = r32(&pkt[4..8], msb);
                let mask = r32(&pkt[8..12], msb);
                let mut repaint = false;
                if let Some(win) = c.windows.get_mut(&wid) {
                    let old_bg = win.bg_pixel;
                    let old_has = win.has_bg;
                    parse_win_attrs(win, mask, &pkt[12..], msb);
                    repaint = (win.bg_pixel != old_bg || win.has_bg != old_has) && win.has_bg;
                }
                if repaint {
                    c.paint_win_on_surface(&mut self.surfaces, wid);
                }
            }
            3 if pkt.len() >= 8 => {
                // GetWindowAttributes
                let wid = r32(&pkt[4..8], msb);
                let (mapped, ev_mask) = c
                    .windows
                    .get(&wid)
                    .map(|w| (w.mapped, w.event_mask))
                    .unwrap_or((wid == ROOT_WIN, 0));
                let mut rep = [0u8; 44];
                rep[0] = 1;
                rep[1] = 2; // BackingStore NotUseful
                w16(&mut rep[2..4], c.seq, msb);
                w32(&mut rep[4..8], 3, msb); // 12 extra bytes
                w32(&mut rep[8..12], ROOT_VISUAL, msb);
                w16(&mut rep[12..14], 1, msb); // InputOutput
                rep[26] = if mapped { 2 } else { 0 };
                w32(&mut rep[28..32], DEF_COLORMAP, msb);
                w32(&mut rep[36..40], ev_mask, msb);
                w32(&mut rep[40..44], ev_mask, msb);
                c.tx.extend_from_slice(&rep);
            }
            4 if pkt.len() >= 8 => {
                // DestroyWindow
                let wid = r32(&pkt[4..8], msb);
                if let Some(w) = c.windows.remove(&wid)
                    && let Some(gid) = w.gid
                {
                    self.surfaces.remove(&gid);
                    unmapped.push(gid);
                }
            }
            7 if pkt.len() >= 16 => {
                // ReparentWindow
                let wid = r32(&pkt[4..8], msb);
                let new_parent = r32(&pkt[8..12], msb);
                let nx = r16(&pkt[12..14], msb) as i16 as i32;
                let ny = r16(&pkt[14..16], msb) as i16 as i32;
                if let Some(w) = c.windows.get_mut(&wid) {
                    w.parent = new_parent;
                    w.x = nx;
                    w.y = ny;
                }
            }
            8 if pkt.len() >= 8 => {
                // MapWindow
                let wid = r32(&pkt[4..8], msb);
                let info = c.windows.get_mut(&wid).map(|win| {
                    win.mapped = true;
                    (win.gid, win.parent, win.w, win.h)
                });
                if let Some((gid_opt, parent, w, h)) = info {
                    if let Some(gid) = gid_opt
                        && let Some(surf) = self.surfaces.get_mut(&gid)
                        && !surf.mapped
                    {
                        surf.mapped = true;
                        newly_mapped.push(gid);
                    }
                    c.paint_win_on_surface(&mut self.surfaces, wid);
                    c.send_map_notify(parent, wid);
                    if wid != parent {
                        c.send_map_notify(wid, wid);
                    }
                    c.send_expose(wid, w, h);
                    // Expose any already-mapped children (e.g., created & mapped before parent)
                    let mut child_queue = c.sorted_children(wid);
                    let mut ci = 0;
                    while ci < child_queue.len() {
                        let cid = child_queue[ci];
                        ci += 1;
                        if let Some(cw) = c.windows.get(&cid)
                            && cw.mapped
                        {
                            let (cw_w, cw_h) = (cw.w, cw.h);
                            c.paint_win_on_surface(&mut self.surfaces, cid);
                            c.send_expose(cid, cw_w, cw_h);
                            child_queue.extend(c.sorted_children(cid));
                        }
                    }
                }
            }
            9 if pkt.len() >= 8 => {
                // MapSubwindows
                let parent = r32(&pkt[4..8], msb);
                c.map_and_expose_tree(&mut self.surfaces, parent, true);
            }
            10 if pkt.len() >= 8 => {
                // UnmapWindow
                let wid = r32(&pkt[4..8], msb);
                if let Some(win) = c.windows.get_mut(&wid) {
                    win.mapped = false;
                    if let Some(gid) = win.gid
                        && let Some(surf) = self.surfaces.get_mut(&gid)
                        && surf.mapped
                    {
                        surf.mapped = false;
                        unmapped.push(gid);
                    }
                }
            }
            11 if pkt.len() >= 8 => {
                // UnmapSubwindows
                let parent = r32(&pkt[4..8], msb);
                for cid in c.sorted_children(parent) {
                    if let Some(cw) = c.windows.get_mut(&cid) {
                        cw.mapped = false;
                    }
                }
            }
            12 if pkt.len() >= 12 => {
                // ConfigureWindow
                let wid = r32(&pkt[4..8], msb);
                let mask = r16(&pkt[8..10], msb);
                let vals = &pkt[12..];
                let mut off = 0usize;
                let mut cfg = c.windows.get(&wid).map(|w| (w.x, w.y, w.w, w.h, w.border_width, w.gid, w.parent, w.bg_pixel));
                if let Some((mut nx, mut ny, mut nw, mut nh, mut nbw, gid_opt, parent, bg)) = cfg {
                    let mut stack_mode = None;
                    for bit in 0..7u16 {
                        if (mask & (1 << bit)) != 0 {
                            if off + 4 > vals.len() {
                                break;
                            }
                            let v = r32(&vals[off..off + 4], msb);
                            match bit {
                                0 => nx = v as i16 as i32,
                                1 => ny = v as i16 as i32,
                                2 => {
                                    nw = (v as i32).max(1);
                                    if parent == ROOT_WIN {
                                        nw = nw.clamp(64, 1600);
                                    } else {
                                        nw = nw.min(1600);
                                    }
                                }
                                3 => {
                                    nh = (v as i32).max(1);
                                    if parent == ROOT_WIN {
                                        nh = nh.clamp(48, 1200);
                                    } else {
                                        nh = nh.min(1200);
                                    }
                                }
                                4 => nbw = (v as i32).clamp(0, 16),
                                6 => stack_mode = Some(v),
                                _ => {}
                            }
                            off += 4;
                        }
                    }
                    let new_z = if let Some(sm) = stack_mode {
                        if sm == 0 {
                            let z = c.next_z;
                            c.next_z += 1;
                            Some(z)
                        } else {
                            None
                        }
                    } else {
                        None
                    };
                    if let Some(win) = c.windows.get_mut(&wid) {
                        let size_changed = win.w != nw || win.h != nh;
                        win.x = nx;
                        win.y = ny;
                        win.w = nw;
                        win.h = nh;
                        win.border_width = nbw;
                        if let Some(z) = new_z {
                            win.z = z;
                        }
                        if size_changed
                            && let Some(gid) = gid_opt
                            && let Some(surf) = self.surfaces.get_mut(&gid)
                        {
                            surf.w = nw;
                            surf.h = nh;
                            surf.pixels.resize((nw * nh) as usize, bg & 0x00FF_FFFF);
                        }
                        cfg = Some((nx, ny, nw, nh, nbw, gid_opt, parent, bg));
                    }
                    if let Some((nx, ny, nw, nh, _, _, _, _)) = cfg {
                        if c.windows.get(&wid).is_some_and(|w| w.mapped) {
                            c.paint_win_on_surface(&mut self.surfaces, wid);
                        }
                        c.send_configure_notify(wid, nx, ny, nw, nh);
                        c.send_expose(wid, nw, nh);
                    }
                }
            }
            14 if pkt.len() >= 8 => {
                // GetGeometry
                let did = r32(&pkt[4..8], msb);
                let (x, y, w, h, bw) = if did == ROOT_WIN {
                    (0, 0, self.screen_w as i32, self.screen_h as i32, 0)
                } else if let Some(win) = c.windows.get(&did) {
                    (win.x, win.y, win.w, win.h, win.border_width)
                } else if let Some(pm) = c.pixmaps.get(&did) {
                    (0, 0, pm.w, pm.h, 0)
                } else {
                    (0, 0, 320, 240, 0)
                };
                let mut rep = [0u8; 32];
                rep[0] = 1;
                rep[1] = 24;
                w16(&mut rep[2..4], c.seq, msb);
                w32(&mut rep[8..12], ROOT_WIN, msb);
                w16(&mut rep[12..14], x as i16 as u16, msb);
                w16(&mut rep[14..16], y as i16 as u16, msb);
                w16(&mut rep[16..18], w as u16, msb);
                w16(&mut rep[18..20], h as u16, msb);
                w16(&mut rep[20..22], bw as u16, msb);
                c.tx.extend_from_slice(&rep);
            }
            15 if pkt.len() >= 8 => {
                // QueryTree
                let wid = r32(&pkt[4..8], msb);
                let parent = if wid == ROOT_WIN {
                    0
                } else {
                    c.windows.get(&wid).map(|w| w.parent).unwrap_or(ROOT_WIN)
                };
                let children = c.sorted_children(wid);
                let mut rep = vec![0u8; 32 + children.len() * 4];
                rep[0] = 1;
                w16(&mut rep[2..4], c.seq, msb);
                w32(&mut rep[4..8], children.len() as u32, msb);
                w32(&mut rep[8..12], ROOT_WIN, msb);
                w32(&mut rep[12..16], parent, msb);
                w16(&mut rep[16..18], children.len() as u16, msb);
                for (i, cid) in children.iter().enumerate() {
                    w32(&mut rep[32 + i * 4..36 + i * 4], *cid, msb);
                }
                c.tx.extend_from_slice(&rep);
            }
            16 if pkt.len() >= 8 => {
                // InternAtom
                let only_if = detail != 0;
                let nlen = r16(&pkt[4..6], msb) as usize;
                let name = if 8 + nlen <= pkt.len() {
                    String::from_utf8_lossy(&pkt[8..8 + nlen]).into_owned()
                } else {
                    String::new()
                };
                let aid = if let Some(&id) = c.atom_names.get(&name) {
                    id
                } else if only_if {
                    0
                } else {
                    let id = c.next_atom;
                    c.next_atom += 1;
                    c.atoms.insert(id, name.clone());
                    c.atom_names.insert(name, id);
                    id
                };
                let mut rep = [0u8; 32];
                rep[0] = 1;
                w16(&mut rep[2..4], c.seq, msb);
                w32(&mut rep[8..12], aid, msb);
                c.tx.extend_from_slice(&rep);
            }
            17 if pkt.len() >= 8 => {
                // GetAtomName
                let aid = r32(&pkt[4..8], msb);
                let name = c.atoms.get(&aid).cloned().unwrap_or_default();
                let nb = name.as_bytes();
                let words = pad4(nb.len()) / 4;
                let mut rep = vec![0u8; 32 + words * 4];
                rep[0] = 1;
                w16(&mut rep[2..4], c.seq, msb);
                w32(&mut rep[4..8], words as u32, msb);
                w16(&mut rep[8..10], nb.len() as u16, msb);
                rep[32..32 + nb.len()].copy_from_slice(nb);
                c.tx.extend_from_slice(&rep);
            }
            18 if pkt.len() >= 24 => {
                // ChangeProperty
                let mode = detail; // 0=Replace, 1=Prepend, 2=Append
                let wid = r32(&pkt[4..8], msb);
                let prop = r32(&pkt[8..12], msb);
                let typ = r32(&pkt[12..16], msb);
                let format = pkt[16];
                let dlen = r32(&pkt[20..24], msb) as usize;
                let unit_bytes = match format {
                    16 => 2usize,
                    32 => 4usize,
                    _ => 1usize,
                };
                let byte_len = dlen.saturating_mul(unit_bytes);
                if 24 + byte_len <= pkt.len() {
                    let data = pkt[24..24 + byte_len].to_vec();
                    let entry = c.props.entry((wid, prop)).or_insert((typ, format, Vec::new()));
                    match mode {
                        1 => {
                            let mut merged = data;
                            merged.extend_from_slice(&entry.2);
                            *entry = (typ, format, merged);
                        }
                        2 => {
                            entry.0 = typ;
                            entry.1 = format;
                            entry.2.extend_from_slice(&data);
                        }
                        _ => {
                            *entry = (typ, format, data);
                        }
                    }
                    let is_title = matches!(c.atoms.get(&prop).map(|s| s.as_str()), Some("WM_NAME" | "_NET_WM_NAME"));
                    if is_title
                        && format == 8
                        && let Some((_, _, raw)) = c.props.get(&(wid, prop))
                    {
                        let title = String::from_utf8_lossy(raw).trim_matches('\0').to_string();
                        if !title.is_empty()
                            && let Some((gid, _, _)) = c.find_root_gid(wid)
                            && let Some(surf) = self.surfaces.get_mut(&gid)
                        {
                            surf.title = title;
                        }
                    }
                }
            }
            19 if pkt.len() >= 12 => {
                // DeleteProperty
                let wid = r32(&pkt[4..8], msb);
                let prop = r32(&pkt[8..12], msb);
                c.props.remove(&(wid, prop));
            }
            20 if pkt.len() >= 24 => {
                // GetProperty
                let del = detail != 0;
                let wid = r32(&pkt[4..8], msb);
                let prop = r32(&pkt[8..12], msb);
                let off_bytes = (r32(&pkt[16..20], msb) as usize).saturating_mul(4);
                let max_bytes = (r32(&pkt[20..24], msb) as usize).saturating_mul(4);
                if let Some((typ, format, data)) = c.props.get(&(wid, prop)).cloned() {
                    let start = off_bytes.min(data.len());
                    let take = (data.len() - start).min(max_bytes);
                    let slice = &data[start..start + take];
                    let bytes_after = (data.len() - start - take) as u32;
                    let words = pad4(slice.len()) / 4;
                    let unit_div = match format {
                        16 => 2,
                        32 => 4,
                        _ => 1,
                    };
                    let mut rep = vec![0u8; 32 + words * 4];
                    rep[0] = 1;
                    rep[1] = format;
                    w16(&mut rep[2..4], c.seq, msb);
                    w32(&mut rep[4..8], words as u32, msb);
                    w32(&mut rep[8..12], typ, msb);
                    w32(&mut rep[12..16], bytes_after, msb);
                    w32(&mut rep[16..20], (slice.len() / unit_div) as u32, msb);
                    rep[32..32 + slice.len()].copy_from_slice(slice);
                    c.tx.extend_from_slice(&rep);
                    if del && bytes_after == 0 {
                        c.props.remove(&(wid, prop));
                    }
                } else {
                    let mut rep = [0u8; 32];
                    rep[0] = 1;
                    w16(&mut rep[2..4], c.seq, msb);
                    c.tx.extend_from_slice(&rep);
                }
            }
            21 if pkt.len() >= 8 => {
                // ListProperties
                let wid = r32(&pkt[4..8], msb);
                let atoms: Vec<u32> = c
                    .props
                    .keys()
                    .filter(|(w, _)| *w == wid)
                    .map(|(_, a)| *a)
                    .collect();
                let mut rep = vec![0u8; 32 + atoms.len() * 4];
                rep[0] = 1;
                w16(&mut rep[2..4], c.seq, msb);
                w32(&mut rep[4..8], atoms.len() as u32, msb);
                w16(&mut rep[8..10], atoms.len() as u16, msb);
                for (i, a) in atoms.iter().enumerate() {
                    w32(&mut rep[32 + i * 4..36 + i * 4], *a, msb);
                }
                c.tx.extend_from_slice(&rep);
            }
            22 if pkt.len() >= 16 => {
                // SetSelectionOwner
                let owner = r32(&pkt[4..8], msb);
                let selection = r32(&pkt[8..12], msb);
                if owner == 0 {
                    self.selections.remove(&selection);
                } else {
                    self.selections.insert(selection, owner);
                }
            }
            24 if pkt.len() >= 24 => {
                // ConvertSelection -> SelectionNotify
                let requestor = r32(&pkt[4..8], msb);
                let selection = r32(&pkt[8..12], msb);
                let target = r32(&pkt[12..16], msb);
                let prop = r32(&pkt[16..20], msb);
                let time = r32(&pkt[20..24], msb);
                let mut ev = [0u8; 32];
                ev[0] = 31; // SelectionNotify
                w32(&mut ev[4..8], time, msb);
                w32(&mut ev[8..12], requestor, msb);
                w32(&mut ev[12..16], selection, msb);
                w32(&mut ev[16..20], target, msb);
                w32(&mut ev[20..24], prop, msb);
                c.push_event(ev);
            }
            26 | 31 => {
                // GrabPointer (26) / GrabKeyboard (31) -> Success (0)
                let mut rep = [0u8; 32];
                rep[0] = 1;
                rep[1] = 0; // Success
                w16(&mut rep[2..4], c.seq, msb);
                c.tx.extend_from_slice(&rep);
            }
            38 if pkt.len() >= 8 => {
                // QueryPointer
                let wid = r32(&pkt[4..8], msb);
                let (wx, wy) = if let Some((gid, ox, oy)) = c.find_root_gid(wid) {
                    let (sox, soy) = self.surf_origins.get(&gid).copied().unwrap_or((0, 0));
                    (self.ptr_x - sox - ox, self.ptr_y - soy - oy)
                } else {
                    (self.ptr_x, self.ptr_y)
                };
                let (child, _, _) = c.find_hit_window(wid, wx, wy);
                let child = if child == wid { 0 } else { child };
                let mut rep = [0u8; 32];
                rep[0] = 1;
                rep[1] = 1; // same_screen = true
                w16(&mut rep[2..4], c.seq, msb);
                w32(&mut rep[8..12], ROOT_WIN, msb);
                w32(&mut rep[12..16], child, msb);
                w16(&mut rep[16..18], self.ptr_x as i16 as u16, msb);
                w16(&mut rep[18..20], self.ptr_y as i16 as u16, msb);
                w16(&mut rep[20..22], wx as i16 as u16, msb);
                w16(&mut rep[22..24], wy as i16 as u16, msb);
                c.tx.extend_from_slice(&rep);
            }
            40 if pkt.len() >= 16 => {
                // TranslateCoordinates
                let src_wid = r32(&pkt[4..8], msb);
                let dst_wid = r32(&pkt[8..12], msb);
                let sx = r16(&pkt[12..14], msb) as i16 as i32;
                let sy = r16(&pkt[14..16], msb) as i16 as i32;
                let (sox, soy) = c.find_root_gid(src_wid).map(|(_, ox, oy)| (ox, oy)).unwrap_or((0, 0));
                let (dox, doy) = c.find_root_gid(dst_wid).map(|(_, ox, oy)| (ox, oy)).unwrap_or((0, 0));
                let dx = sox + sx - dox;
                let dy = soy + sy - doy;
                let mut rep = [0u8; 32];
                rep[0] = 1;
                rep[1] = 1; // same_screen
                w16(&mut rep[2..4], c.seq, msb);
                w16(&mut rep[12..14], dx as i16 as u16, msb);
                w16(&mut rep[14..16], dy as i16 as u16, msb);
                c.tx.extend_from_slice(&rep);
            }
            43 => {
                // GetInputFocus
                let mut rep = [0u8; 32];
                rep[0] = 1;
                rep[1] = 1;
                w16(&mut rep[2..4], c.seq, msb);
                w32(&mut rep[8..12], 1, msb); // PointerRoot
                c.tx.extend_from_slice(&rep);
            }
            53 if pkt.len() >= 16 => {
                // CreatePixmap
                let pid = r32(&pkt[4..8], msb);
                let pw = (r16(&pkt[12..14], msb) as i32).clamp(1, 1600);
                let ph = (r16(&pkt[14..16], msb) as i32).clamp(1, 1200);
                c.pixmaps.insert(
                    pid,
                    XPixmap {
                        w: pw,
                        h: ph,
                        pixels: vec![0; (pw * ph) as usize],
                    },
                );
            }
            54 if pkt.len() >= 8 => {
                let pid = r32(&pkt[4..8], msb);
                c.pixmaps.remove(&pid);
            }
            55 if pkt.len() >= 16 => {
                // CreateGC
                let gid = r32(&pkt[4..8], msb);
                let mask = r32(&pkt[12..16], msb);
                let gc = parse_gc_values(XGc::default(), mask, &pkt[16..], msb);
                c.gcs.insert(gid, gc);
            }
            56 if pkt.len() >= 12 => {
                // ChangeGC
                let gid = r32(&pkt[4..8], msb);
                let mask = r32(&pkt[8..12], msb);
                let prev = c.gcs.get(&gid).copied().unwrap_or_default();
                let next = parse_gc_values(prev, mask, &pkt[12..], msb);
                c.gcs.insert(gid, next);
            }
            57 if pkt.len() >= 16 => {
                // CopyGC (src -> dst with value-mask)
                let src_gid = r32(&pkt[4..8], msb);
                let dst_gid = r32(&pkt[8..12], msb);
                let mask = r32(&pkt[12..16], msb);
                let src = c.gcs.get(&src_gid).copied().unwrap_or_default();
                let mut dst = c.gcs.get(&dst_gid).copied().unwrap_or_default();
                if (mask & (1 << 2)) != 0 {
                    dst.fg = src.fg;
                }
                if (mask & (1 << 3)) != 0 {
                    dst.bg = src.bg;
                }
                if (mask & (1 << 4)) != 0 {
                    dst.line_width = src.line_width;
                }
                c.gcs.insert(dst_gid, dst);
            }
            60 if pkt.len() >= 8 => {
                let gid = r32(&pkt[4..8], msb);
                c.gcs.remove(&gid);
            }
            61 if pkt.len() >= 16 => {
                // ClearArea
                let wid = r32(&pkt[4..8], msb);
                let x = r16(&pkt[8..10], msb) as i16 as i32;
                let y = r16(&pkt[10..12], msb) as i16 as i32;
                let w = r16(&pkt[12..14], msb) as i32;
                let h = r16(&pkt[14..16], msb) as i32;
                let (bg, ww, wh) = c
                    .windows
                    .get(&wid)
                    .map(|win| (win.bg_pixel, win.w, win.h))
                    .unwrap_or((0x000D_1117, 320, 240));
                let rw = if w == 0 { (ww - x).max(0) } else { w };
                let rh = if h == 0 { (wh - y).max(0) } else { h };
                if let Some((gid, ox, oy)) = c.find_root_gid(wid)
                    && let Some(surf) = self.surfaces.get_mut(&gid)
                {
                    fill_rect_buf(&mut surf.pixels, (surf.w, surf.h), ox + x, oy + y, rw, rh, bg);
                }
                if detail != 0 && rw > 0 && rh > 0 {
                    c.send_expose(wid, rw, rh);
                }
            }
            62 if pkt.len() >= 28 => {
                // CopyArea (Window/Pixmap -> Window/Pixmap)
                let src_id = r32(&pkt[4..8], msb);
                let dst_id = r32(&pkt[8..12], msb);
                let sx = r16(&pkt[16..18], msb) as i16 as i32;
                let sy = r16(&pkt[18..20], msb) as i16 as i32;
                let dx = r16(&pkt[20..22], msb) as i16 as i32;
                let dy = r16(&pkt[22..24], msb) as i16 as i32;
                let cw = r16(&pkt[24..26], msb) as i32;
                let ch = r16(&pkt[26..28], msb) as i32;
                let copied = if let Some(pm) = c.pixmaps.get(&src_id) {
                    Some((pm.w, pm.h, 0, 0, pm.pixels.clone()))
                } else if let Some((gid, sox, soy)) = c.find_root_gid(src_id)
                    && let Some(surf) = self.surfaces.get(&gid)
                {
                    Some((surf.w, surf.h, sox, soy, surf.pixels.clone()))
                } else {
                    None
                };
                if let Some((sw, sh, sox, soy, spx)) = copied {
                    let mut tw = 0;
                    let mut th = 0;
                    let mut dox = 0;
                    let mut doy = 0;
                    let mut target: Option<&mut [u32]> = None;
                    if let Some((dgid, gx, gy)) = c.find_root_gid(dst_id)
                        && let Some(dsurf) = self.surfaces.get_mut(&dgid)
                    {
                        tw = dsurf.w;
                        th = dsurf.h;
                        dox = gx;
                        doy = gy;
                        target = Some(&mut dsurf.pixels);
                    } else if let Some(dpm) = c.pixmaps.get_mut(&dst_id) {
                        tw = dpm.w;
                        th = dpm.h;
                        target = Some(&mut dpm.pixels);
                    }
                    if let Some(dst_px) = target {
                        for row in 0..ch {
                            let syy = soy + sy + row;
                            let dyy = doy + dy + row;
                            if syy < 0 || syy >= sh || dyy < 0 || dyy >= th {
                                continue;
                            }
                            for col in 0..cw {
                                let sxx = sox + sx + col;
                                let dxx = dox + dx + col;
                                if sxx >= 0 && sxx < sw && dxx >= 0 && dxx < tw {
                                    dst_px[(dyy * tw + dxx) as usize] =
                                        spx[(syy * sw + sxx) as usize];
                                }
                            }
                        }
                    }
                }
            }
            63 if pkt.len() >= 32 => {
                // CopyPlane (1-bit plane -> Window/Pixmap using gc.fg / gc.bg)
                let src_id = r32(&pkt[4..8], msb);
                let dst_id = r32(&pkt[8..12], msb);
                let gcid = r32(&pkt[12..16], msb);
                let sx = r16(&pkt[16..18], msb) as i16 as i32;
                let sy = r16(&pkt[18..20], msb) as i16 as i32;
                let dx = r16(&pkt[20..22], msb) as i16 as i32;
                let dy = r16(&pkt[22..24], msb) as i16 as i32;
                let cw = r16(&pkt[24..26], msb) as i32;
                let ch = r16(&pkt[26..28], msb) as i32;
                let bit_plane = r32(&pkt[28..32], msb);
                let gc = c.gcs.get(&gcid).copied().unwrap_or_default();
                let copied = if let Some(pm) = c.pixmaps.get(&src_id) {
                    Some((pm.w, pm.h, 0, 0, pm.pixels.clone()))
                } else if let Some((gid, sox, soy)) = c.find_root_gid(src_id)
                    && let Some(surf) = self.surfaces.get(&gid)
                {
                    Some((surf.w, surf.h, sox, soy, surf.pixels.clone()))
                } else {
                    None
                };
                if let Some((sw, sh, sox, soy, spx)) = copied {
                    let mut tw = 0;
                    let mut th = 0;
                    let mut dox = 0;
                    let mut doy = 0;
                    let mut target: Option<&mut [u32]> = None;
                    if let Some((dgid, gx, gy)) = c.find_root_gid(dst_id)
                        && let Some(dsurf) = self.surfaces.get_mut(&dgid)
                    {
                        tw = dsurf.w;
                        th = dsurf.h;
                        dox = gx;
                        doy = gy;
                        target = Some(&mut dsurf.pixels);
                    } else if let Some(dpm) = c.pixmaps.get_mut(&dst_id) {
                        tw = dpm.w;
                        th = dpm.h;
                        target = Some(&mut dpm.pixels);
                    }
                    if let Some(dst_px) = target {
                        let fg = gc.fg & 0x00FF_FFFF;
                        let bg = gc.bg & 0x00FF_FFFF;
                        for row in 0..ch {
                            let syy = soy + sy + row;
                            let dyy = doy + dy + row;
                            if syy < 0 || syy >= sh || dyy < 0 || dyy >= th {
                                continue;
                            }
                            for col in 0..cw {
                                let sxx = sox + sx + col;
                                let dxx = dox + dx + col;
                                if sxx >= 0 && sxx < sw && dxx >= 0 && dxx < tw {
                                    let v = spx[(syy * sw + sxx) as usize];
                                    let set = (v & bit_plane) != 0;
                                    dst_px[(dyy * tw + dxx) as usize] = if set { fg } else { bg };
                                }
                            }
                        }
                    }
                }
            }
            64..=71 if pkt.len() >= 12 => {
                // PolyPoint(64), PolyLine(65), PolySegment(66), PolyRectangle(67),
                // PolyArc(68), FillPoly(69), PolyFillRectangle(70), PolyFillArc(71)
                let did = r32(&pkt[4..8], msb);
                let gcid = r32(&pkt[8..12], msb);
                let gc = c.gcs.get(&gcid).copied().unwrap_or_default();
                let fg = gc.fg;
                let data = &pkt[12..];
                let mut target: Option<&mut [u32]> = None;
                let mut tw = 0;
                let mut th = 0;
                let mut ox = 0;
                let mut oy = 0;
                if let Some((gid, gx, gy)) = c.find_root_gid(did)
                    && let Some(surf) = self.surfaces.get_mut(&gid)
                {
                    tw = surf.w;
                    th = surf.h;
                    ox = gx;
                    oy = gy;
                    target = Some(&mut surf.pixels);
                } else if let Some(pm) = c.pixmaps.get_mut(&did) {
                    tw = pm.w;
                    th = pm.h;
                    target = Some(&mut pm.pixels);
                }
                if let Some(buf) = target {
                    match op {
                        64 => {
                            let mut prev: Option<(i32, i32)> = None;
                            for pt in data.as_chunks::<4>().0 {
                                let px = r16(&pt[0..2], msb) as i16 as i32;
                                let py = r16(&pt[2..4], msb) as i16 as i32;
                                let cur = if detail == 1 {
                                    if let Some((lx, ly)) = prev {
                                        (lx + px, ly + py)
                                    } else {
                                        (ox + px, oy + py)
                                    }
                                } else {
                                    (ox + px, oy + py)
                                };
                                put_px(buf, tw, th, cur.0, cur.1, fg);
                                prev = Some(cur);
                            }
                        }
                        70 => {
                            for r in data.as_chunks::<8>().0 {
                                let rx = r16(&r[0..2], msb) as i16 as i32;
                                let ry = r16(&r[2..4], msb) as i16 as i32;
                                let rw = r16(&r[4..6], msb) as i32;
                                let rh = r16(&r[6..8], msb) as i32;
                                fill_rect_buf(buf, (tw, th), ox + rx, oy + ry, rw, rh, fg);
                            }
                        }
                        67 => {
                            for r in data.as_chunks::<8>().0 {
                                let rx = r16(&r[0..2], msb) as i16 as i32;
                                let ry = r16(&r[2..4], msb) as i16 as i32;
                                let rw = r16(&r[4..6], msb) as i32;
                                let rh = r16(&r[6..8], msb) as i32;
                                stroke_rect_buf(
                                    buf,
                                    (tw, th),
                                    (ox + rx, oy + ry, rw, rh),
                                    gc.line_width,
                                    fg,
                                );
                            }
                        }
                        68 | 71 => {
                            let filled = op == 71;
                            for a in data.as_chunks::<12>().0 {
                                let ax = r16(&a[0..2], msb) as i16 as i32;
                                let ay = r16(&a[2..4], msb) as i16 as i32;
                                let aw = r16(&a[4..6], msb) as i32;
                                let ah = r16(&a[6..8], msb) as i32;
                                let a1 = r16(&a[8..10], msb) as i16;
                                let a2 = r16(&a[10..12], msb) as i16;
                                draw_arc_buf(
                                    buf,
                                    (tw, th),
                                    ox + ax,
                                    oy + ay,
                                    aw,
                                    ah,
                                    a1,
                                    a2,
                                    gc.line_width,
                                    fg,
                                    filled,
                                );
                            }
                        }
                        69 if data.len() >= 4 => {
                            let coord_mode = data[1];
                            let mut pts = Vec::new();
                            let mut prev: Option<(i32, i32)> = None;
                            for pt in data[4..].as_chunks::<4>().0 {
                                let px = r16(&pt[0..2], msb) as i16 as i32;
                                let py = r16(&pt[2..4], msb) as i16 as i32;
                                let cur = if coord_mode == 1 {
                                    if let Some((lx, ly)) = prev {
                                        (lx + px, ly + py)
                                    } else {
                                        (ox + px, oy + py)
                                    }
                                } else {
                                    (ox + px, oy + py)
                                };
                                pts.push(cur);
                                prev = Some(cur);
                            }
                            fill_poly_buf(buf, (tw, th), &pts, fg);
                        }
                        66 => {
                            for s in data.as_chunks::<8>().0 {
                                let x1 = r16(&s[0..2], msb) as i16 as i32;
                                let y1 = r16(&s[2..4], msb) as i16 as i32;
                                let x2 = r16(&s[4..6], msb) as i16 as i32;
                                let y2 = r16(&s[6..8], msb) as i16 as i32;
                                draw_line_buf(buf, (tw, th), ox + x1, oy + y1, ox + x2, oy + y2, fg);
                            }
                        }
                        _ => {
                            // PolyLine
                            let mut prev: Option<(i32, i32)> = None;
                            for pt in data.as_chunks::<4>().0 {
                                let px = r16(&pt[0..2], msb) as i16 as i32;
                                let py = r16(&pt[2..4], msb) as i16 as i32;
                                let cur = if detail == 1 {
                                    if let Some((lx, ly)) = prev {
                                        (lx + px, ly + py)
                                    } else {
                                        (ox + px, oy + py)
                                    }
                                } else {
                                    (ox + px, oy + py)
                                };
                                if let Some((lx, ly)) = prev {
                                    draw_line_buf(buf, (tw, th), lx, ly, cur.0, cur.1, fg);
                                } else {
                                    put_px(buf, tw, th, cur.0, cur.1, fg);
                                }
                                prev = Some(cur);
                            }
                        }
                    }
                }
            }
            72 if pkt.len() >= 24 => {
                // PutImage
                let did = r32(&pkt[4..8], msb);
                let gcid = r32(&pkt[8..12], msb);
                let iw = r16(&pkt[12..14], msb) as i32;
                let ih = r16(&pkt[14..16], msb) as i32;
                let dx = r16(&pkt[16..18], msb) as i16 as i32;
                let dy = r16(&pkt[18..20], msb) as i16 as i32;
                let depth = pkt[21];
                let gc = c.gcs.get(&gcid).copied().unwrap_or_default();
                let img = &pkt[24..];

                let mut tw = 0;
                let mut th = 0;
                let mut ox = 0;
                let mut oy = 0;
                let mut is_pixmap = false;
                let mut target: Option<&mut [u32]> = None;
                if let Some((gid, gx, gy)) = c.find_root_gid(did)
                    && let Some(surf) = self.surfaces.get_mut(&gid)
                {
                    tw = surf.w;
                    th = surf.h;
                    ox = gx;
                    oy = gy;
                    target = Some(&mut surf.pixels);
                } else if let Some(pm) = c.pixmaps.get_mut(&did) {
                    tw = pm.w;
                    th = pm.h;
                    is_pixmap = true;
                    target = Some(&mut pm.pixels);
                }

                if let Some(buf) = target && iw > 0 && ih > 0 {
                    if detail == 2 && depth >= 24 {
                        // ZPixmap 32bpp (preserve alpha on Pixmaps for ARGB32 XRender Pictures)
                        let row_bytes = (iw as usize) * 4;
                        for row in 0..ih {
                            let roff = (row as usize) * row_bytes;
                            if roff + row_bytes > img.len() {
                                break;
                            }
                            for col in 0..iw {
                                let po = roff + (col as usize) * 4;
                                let val = r32(&img[po..po + 4], msb);
                                if is_pixmap {
                                    put_px_raw(buf, tw, th, ox + dx + col, oy + dy + row, val);
                                } else {
                                    put_px(buf, tw, th, ox + dx + col, oy + dy + row, val);
                                }
                            }
                        }
                    } else if detail == 0 || (detail == 1 && depth == 1) {
                        // XYBitmap 1bpp / XYPixmap 1bpp
                        let stride = (((iw + 31) / 32) * 4) as usize;
                        let on_col = if detail == 1 { 0x00FF_FFFF } else { gc.fg };
                        let off_col = if detail == 1 { 0x0000_0000 } else { gc.bg };
                        for row in 0..ih {
                            let roff = (row as usize) * stride;
                            if roff + stride > img.len() {
                                break;
                            }
                            for col in 0..iw {
                                let b = img[roff + (col / 8) as usize];
                                let set = (b & (1 << (col % 8))) != 0;
                                put_px(
                                    buf,
                                    tw,
                                    th,
                                    ox + dx + col,
                                    oy + dy + row,
                                    if set { on_col } else { off_col },
                                );
                            }
                        }
                    } else if detail == 2 && depth == 8 {
                        // ZPixmap 8bpp (alpha/grayscale mask)
                        let stride = pad4(iw as usize);
                        for row in 0..ih {
                            let roff = (row as usize) * stride;
                            if roff + (iw as usize) > img.len() {
                                break;
                            }
                            for col in 0..iw {
                                let v = img[roff + col as usize] as u32;
                                put_px(
                                    buf,
                                    tw,
                                    th,
                                    ox + dx + col,
                                    oy + dy + row,
                                    (v << 16) | (v << 8) | v,
                                );
                            }
                        }
                    }
                }
            }
            73 if pkt.len() >= 20 => {
                // GetImage -> ZPixmap 32bpp
                let did = r32(&pkt[4..8], msb);
                let x = r16(&pkt[8..10], msb) as i16 as i32;
                let y = r16(&pkt[10..12], msb) as i16 as i32;
                let gw = (r16(&pkt[12..14], msb) as i32).clamp(1, 1600);
                let gh = (r16(&pkt[14..16], msb) as i32).clamp(1, 1200);
                let nbytes = (gw * gh * 4) as usize;
                let mut rep = vec![0u8; 32 + nbytes];
                rep[0] = 1;
                rep[1] = 24;
                w16(&mut rep[2..4], c.seq, msb);
                w32(&mut rep[4..8], (nbytes / 4) as u32, msb);
                w32(&mut rep[8..12], ROOT_VISUAL, msb);
                let (src_w, src_h, sox, soy, src_px) = if let Some((gid, ox, oy)) = c.find_root_gid(did)
                    && let Some(surf) = self.surfaces.get(&gid)
                {
                    (surf.w, surf.h, ox, oy, Some(&surf.pixels))
                } else if let Some(pm) = c.pixmaps.get(&did) {
                    (pm.w, pm.h, 0, 0, Some(&pm.pixels))
                } else {
                    (0, 0, 0, 0, None)
                };
                if let Some(pxs) = src_px {
                    for row in 0..gh {
                        let syy = soy + y + row;
                        if syy < 0 || syy >= src_h {
                            continue;
                        }
                        for col in 0..gw {
                            let sxx = sox + x + col;
                            if sxx >= 0 && sxx < src_w {
                                let v = pxs[(syy * src_w + sxx) as usize];
                                let off = 32 + ((row * gw + col) as usize) * 4;
                                w32(&mut rep[off..off + 4], v, msb);
                            }
                        }
                    }
                }
                c.tx.extend_from_slice(&rep);
            }
            74 | 75 if pkt.len() >= 16 => {
                // PolyText8 (74) / PolyText16 (75)
                let is16 = op == 75;
                let did = r32(&pkt[4..8], msb);
                let gcid = r32(&pkt[8..12], msb);
                let mut tx = r16(&pkt[12..14], msb) as i16 as i32;
                let ty = r16(&pkt[14..16], msb) as i16 as i32;
                let gc = c.gcs.get(&gcid).copied().unwrap_or_default();
                let mut chunks: Vec<(i32, String)> = Vec::new();
                let mut i = 16usize;
                while i < pkt.len() {
                    let len = pkt[i] as usize;
                    if len == 255 {
                        i += 5;
                        continue;
                    }
                    if len == 0 {
                        break;
                    }
                    if i + 2 > pkt.len() {
                        break;
                    }
                    let delta = pkt[i + 1] as i8 as i32;
                    let byte_len = if is16 { len * 2 } else { len };
                    if i + 2 + byte_len > pkt.len() {
                        break;
                    }
                    let s = if is16 {
                        pkt[i + 2..i + 2 + byte_len]
                            .as_chunks::<2>()
                            .0
                            .iter()
                            .map(|ch| char::from_u32(u16::from_be_bytes([ch[0], ch[1]]) as u32).unwrap_or('?'))
                            .collect()
                    } else {
                        String::from_utf8_lossy(&pkt[i + 2..i + 2 + byte_len]).into_owned()
                    };
                    chunks.push((delta, s));
                    i += 2 + byte_len;
                }
                if !chunks.is_empty() {
                    if let Some((gid, gx, gy)) = c.find_root_gid(did)
                        && let Some(surf) = self.surfaces.get_mut(&gid)
                    {
                        for (delta, s) in &chunks {
                            tx += *delta;
                            draw_text_buf(&mut surf.pixels, (surf.w, surf.h), gx + tx, gy + ty, s, gc.fg, None);
                            tx += measure_text_advance(s);
                        }
                    } else if let Some(pm) = c.pixmaps.get_mut(&did) {
                        for (delta, s) in &chunks {
                            tx += *delta;
                            draw_text_buf(&mut pm.pixels, (pm.w, pm.h), tx, ty, s, gc.fg, None);
                            tx += measure_text_advance(s);
                        }
                    }
                }
            }
            76 | 77 if pkt.len() >= 16 => {
                // ImageText8 (76) / ImageText16 (77)
                let slen = detail as usize;
                let did = r32(&pkt[4..8], msb);
                let gcid = r32(&pkt[8..12], msb);
                let tx = r16(&pkt[12..14], msb) as i16 as i32;
                let ty = r16(&pkt[14..16], msb) as i16 as i32;
                let need = if op == 77 { slen * 2 } else { slen };
                if 16 + need <= pkt.len() {
                    let s = if op == 77 {
                        pkt[16..16 + need]
                            .as_chunks::<2>()
                            .0
                            .iter()
                            .map(|ch| char::from_u32(u16::from_be_bytes([ch[0], ch[1]]) as u32).unwrap_or('?'))
                            .collect()
                    } else {
                        String::from_utf8_lossy(&pkt[16..16 + need]).into_owned()
                    };
                    let gc = c.gcs.get(&gcid).copied().unwrap_or_default();
                    if let Some((gid, gx, gy)) = c.find_root_gid(did)
                        && let Some(surf) = self.surfaces.get_mut(&gid)
                    {
                        draw_text_buf(
                            &mut surf.pixels,
                            (surf.w, surf.h),
                            gx + tx,
                            gy + ty,
                            &s,
                            gc.fg,
                            Some(gc.bg),
                        );
                    } else if let Some(pm) = c.pixmaps.get_mut(&did) {
                        draw_text_buf(&mut pm.pixels, (pm.w, pm.h), tx, ty, &s, gc.fg, Some(gc.bg));
                    }
                }
            }
            23 if pkt.len() >= 8 => {
                // GetSelectionOwner
                let sel = r32(&pkt[4..8], msb);
                let owner = self.selections.get(&sel).copied().unwrap_or(0);
                let mut rep = [0u8; 32];
                rep[0] = 1;
                w16(&mut rep[2..4], c.seq, msb);
                w32(&mut rep[8..12], owner, msb);
                c.tx.extend_from_slice(&rep);
            }
            47 => {
                // QueryFont -> minimal fixed 8x14 font metrics
                let mut rep = [0u8; 60];
                rep[0] = 1;
                w16(&mut rep[2..4], c.seq, msb);
                w32(&mut rep[4..8], 7, msb); // 7 extra 4-byte words (28 bytes)
                // min_bounds (8..20) & max_bounds (24..36): rightSideBearing=8, character_width=8, ascent=11, descent=3
                w16(&mut rep[10..12], 8, msb);
                w16(&mut rep[12..14], 8, msb);
                w16(&mut rep[14..16], 11, msb);
                w16(&mut rep[16..18], 3, msb);
                w16(&mut rep[26..28], 8, msb);
                w16(&mut rep[28..30], 8, msb);
                w16(&mut rep[30..32], 11, msb);
                w16(&mut rep[32..34], 3, msb);
                w16(&mut rep[40..42], 32, msb); // min_char_or_byte2
                w16(&mut rep[42..44], 126, msb); // max_char_or_byte2
                w16(&mut rep[44..46], 32, msb); // default_char
                w16(&mut rep[52..54], 11, msb); // font_ascent
                w16(&mut rep[54..56], 3, msb); // font_descent
                c.tx.extend_from_slice(&rep);
            }
            49 => {
                // ListFonts -> ["fixed", "-misc-fixed-medium-r-normal--14-130-75-75-c-70-iso8859-1"]
                let max_names = if pkt.len() >= 6 { r16(&pkt[4..6], msb) as usize } else { 2 };
                let fonts: [&[u8]; 2] = [
                    b"fixed",
                    b"-misc-fixed-medium-r-normal--14-130-75-75-c-70-iso8859-1",
                ];
                let count = fonts.len().min(max_names);
                let mut list = Vec::new();
                for f in &fonts[..count] {
                    list.push(f.len() as u8);
                    list.extend_from_slice(f);
                }
                let words = pad4(list.len()) / 4;
                let mut rep = vec![0u8; 32 + words * 4];
                rep[0] = 1;
                w16(&mut rep[2..4], c.seq, msb);
                w32(&mut rep[4..8], words as u32, msb);
                w16(&mut rep[8..10], count as u16, msb);
                rep[32..32 + list.len()].copy_from_slice(&list);
                c.tx.extend_from_slice(&rep);
            }
            84 => {
                // AllocColor -> pass RGB directly
                let r = if pkt.len() >= 10 { r16(&pkt[8..10], msb) >> 8 } else { 255 };
                let g = if pkt.len() >= 12 { r16(&pkt[10..12], msb) >> 8 } else { 255 };
                let b = if pkt.len() >= 14 { r16(&pkt[12..14], msb) >> 8 } else { 255 };
                let pixel = ((r as u32) << 16) | ((g as u32) << 8) | (b as u32);
                let mut rep = [0u8; 32];
                rep[0] = 1;
                w16(&mut rep[2..4], c.seq, msb);
                w16(&mut rep[8..10], (r << 8) | r, msb);
                w16(&mut rep[10..12], (g << 8) | g, msb);
                w16(&mut rep[12..14], (b << 8) | b, msb);
                w32(&mut rep[16..20], pixel, msb);
                c.tx.extend_from_slice(&rep);
            }
            85 | 92 => {
                // AllocNamedColor (85) / LookupColor (92)
                let nlen = if pkt.len() >= 10 { r16(&pkt[8..10], msb) as usize } else { 0 };
                let name = if 12 + nlen <= pkt.len() {
                    String::from_utf8_lossy(&pkt[12..12 + nlen]).to_ascii_lowercase()
                } else {
                    String::new()
                };
                let (r, g, b) = resolve_x11_color(&name);
                let pixel = (((r >> 8) as u32) << 16) | (((g >> 8) as u32) << 8) | ((b >> 8) as u32);
                let mut rep = [0u8; 32];
                rep[0] = 1;
                w16(&mut rep[2..4], c.seq, msb);
                if op == 85 {
                    w32(&mut rep[8..12], pixel, msb);
                    w16(&mut rep[12..14], r, msb);
                    w16(&mut rep[14..16], g, msb);
                    w16(&mut rep[16..18], b, msb);
                    w16(&mut rep[18..20], r, msb);
                    w16(&mut rep[20..22], g, msb);
                    w16(&mut rep[22..24], b, msb);
                } else {
                    w16(&mut rep[8..10], r, msb);
                    w16(&mut rep[10..12], g, msb);
                    w16(&mut rep[12..14], b, msb);
                    w16(&mut rep[14..16], r, msb);
                    w16(&mut rep[16..18], g, msb);
                    w16(&mut rep[18..20], b, msb);
                }
                c.tx.extend_from_slice(&rep);
            }
            91 if pkt.len() >= 8 => {
                // QueryColors
                let pixels = &pkt[8..];
                let n = pixels.len() / 4;
                let mut rep = vec![0u8; 32 + n * 8];
                rep[0] = 1;
                w16(&mut rep[2..4], c.seq, msb);
                w32(&mut rep[4..8], (n * 2) as u32, msb);
                w16(&mut rep[8..10], n as u16, msb);
                for (i, ch) in pixels.as_chunks::<4>().0.iter().enumerate() {
                    let p = r32(ch, msb);
                    let r = (((p >> 16) & 0xFF) as u16) * 257;
                    let g = (((p >> 8) & 0xFF) as u16) * 257;
                    let b = ((p & 0xFF) as u16) * 257;
                    let o = 32 + i * 8;
                    w16(&mut rep[o..o + 2], r, msb);
                    w16(&mut rep[o + 2..o + 4], g, msb);
                    w16(&mut rep[o + 4..o + 6], b, msb);
                }
                c.tx.extend_from_slice(&rep);
            }
            97 if pkt.len() >= 12 => {
                // QueryBestSize
                let rw = r16(&pkt[8..10], msb).max(16);
                let rh = r16(&pkt[10..12], msb).max(16);
                let mut rep = [0u8; 32];
                rep[0] = 1;
                w16(&mut rep[2..4], c.seq, msb);
                w16(&mut rep[8..10], rw, msb);
                w16(&mut rep[10..12], rh, msb);
                c.tx.extend_from_slice(&rep);
            }
            98 => {
                // QueryExtension
                let nlen = if pkt.len() >= 8 {
                    r16(&pkt[4..6], msb) as usize
                } else {
                    0
                };
                let name = if nlen > 0 && 8 + nlen <= pkt.len() {
                    String::from_utf8_lossy(&pkt[8..8 + nlen]).into_owned()
                } else {
                    String::new()
                };
                if x11_log_enabled() {
                    x11_log(&format!("  QueryExtension: '{name}'"));
                }
                let mut rep = [0u8; 32];
                rep[0] = 1;
                w16(&mut rep[2..4], c.seq, msb);
                match name.as_str() {
                    "XInputExtension" => {
                        rep[8] = 1;
                        rep[9] = XINPUT_EXT_OPCODE;
                        rep[10] = XINPUT_FIRST_EVENT;
                        rep[11] = XINPUT_FIRST_ERROR;
                    }
                    "Generic Event Extension" => {
                        rep[8] = 1;
                        rep[9] = XGE_EXT_OPCODE;
                    }
                    "RENDER" => {
                        rep[8] = 1;
                        rep[9] = RENDER_EXT_OPCODE;
                        rep[11] = RENDER_FIRST_ERROR;
                    }
                    "SHAPE" => {
                        rep[8] = 1;
                        rep[9] = SHAPE_EXT_OPCODE;
                        rep[10] = SHAPE_FIRST_EVENT;
                    }
                    _ => {}
                }
                c.tx.extend_from_slice(&rep);
            }
            101 if pkt.len() >= 8 => {
                // GetKeyboardMapping
                let first = pkt[4];
                let count = pkt[5] as usize;
                let syms_per = 2usize;
                let extra_words = count * syms_per;
                let mut rep = vec![0u8; 32 + extra_words * 4];
                rep[0] = 1;
                rep[1] = syms_per as u8;
                w16(&mut rep[2..4], c.seq, msb);
                w32(&mut rep[4..8], extra_words as u32, msb);
                for k in 0..count {
                    let kc = first.saturating_add(k as u8);
                    let o = 32 + k * syms_per * 4;
                    w32(&mut rep[o..o + 4], x11_keysym(kc, false), msb);
                    w32(&mut rep[o + 4..o + 8], x11_keysym(kc, true), msb);
                }
                c.tx.extend_from_slice(&rep);
            }
            119 => {
                // GetModifierMapping
                let kpm = 1usize;
                let mut rep = [0u8; 32 + 8];
                rep[0] = 1;
                rep[1] = kpm as u8;
                w16(&mut rep[2..4], c.seq, msb);
                w32(&mut rep[4..8], (kpm * 8 / 4) as u32, msb);
                // Shift=50, Lock=66, Control=37, Mod1=64
                rep[32] = 50;
                rep[33] = 66;
                rep[34] = 37;
                rep[35] = 64;
                c.tx.extend_from_slice(&rep);
            }
            39 => {
                // GetMotionEvents -> 0 events
                let mut rep = [0u8; 32];
                rep[0] = 1;
                w16(&mut rep[2..4], c.seq, msb);
                c.tx.extend_from_slice(&rep);
            }
            44 => {
                // QueryKeymap -> 32-byte key_vector (40 bytes total)
                let mut rep = [0u8; 40];
                rep[0] = 1;
                w16(&mut rep[2..4], c.seq, msb);
                w32(&mut rep[4..8], 2, msb);
                c.tx.extend_from_slice(&rep);
            }
            48 => {
                // QueryTextExtents -> fixed 8x14 metrics
                let nchars = if pkt.len() >= 8 {
                    ((pkt.len() - 8) / 2).saturating_sub(usize::from(detail != 0))
                } else {
                    0
                };
                let overall_w = (nchars as i32) * 8;
                let mut rep = [0u8; 32];
                rep[0] = 1;
                w16(&mut rep[2..4], c.seq, msb);
                w16(&mut rep[8..10], 11, msb); // font_ascent
                w16(&mut rep[10..12], 3, msb); // font_descent
                w16(&mut rep[12..14], 11, msb); // overall_ascent
                w16(&mut rep[14..16], 3, msb); // overall_descent
                w32(&mut rep[16..20], overall_w as u32, msb); // overall_width
                w32(&mut rep[24..28], overall_w as u32, msb); // overall_right
                c.tx.extend_from_slice(&rep);
            }
            50 | 52 | 110 => {
                // ListFontsWithInfo (50), GetFontPath (52), ListHosts (110) -> empty
                let mut rep = [0u8; 32];
                rep[0] = 1;
                w16(&mut rep[2..4], c.seq, msb);
                c.tx.extend_from_slice(&rep);
            }
            83 => {
                // ListInstalledColormaps -> [DEF_COLORMAP]
                let mut rep = [0u8; 36];
                rep[0] = 1;
                w16(&mut rep[2..4], c.seq, msb);
                w32(&mut rep[4..8], 1, msb);
                w16(&mut rep[8..10], 1, msb);
                w32(&mut rep[32..36], DEF_COLORMAP, msb);
                c.tx.extend_from_slice(&rep);
            }
            99 => {
                // ListExtensions -> ["XInputExtension", "Generic Event Extension", "RENDER", "SHAPE"]
                let exts: [&[u8]; 4] = [
                    b"XInputExtension",
                    b"Generic Event Extension",
                    b"RENDER",
                    b"SHAPE",
                ];
                let mut list = Vec::new();
                for e in exts {
                    list.push(e.len() as u8);
                    list.extend_from_slice(e);
                }
                let words = pad4(list.len()) / 4;
                let mut rep = vec![0u8; 32 + words * 4];
                rep[0] = 1;
                rep[1] = exts.len() as u8;
                w16(&mut rep[2..4], c.seq, msb);
                w32(&mut rep[4..8], words as u32, msb);
                rep[32..32 + list.len()].copy_from_slice(&list);
                c.tx.extend_from_slice(&rep);
            }
            103 => {
                // GetKeyboardControl
                let mut rep = [0u8; 52];
                rep[0] = 1;
                rep[1] = 1; // global_auto_repeat = On
                w16(&mut rep[2..4], c.seq, msb);
                w32(&mut rep[4..8], 5, msb);
                c.tx.extend_from_slice(&rep);
            }
            106 => {
                // GetPointerControl
                let mut rep = [0u8; 32];
                rep[0] = 1;
                w16(&mut rep[2..4], c.seq, msb);
                w16(&mut rep[8..10], 1, msb);
                w16(&mut rep[10..12], 1, msb);
                w16(&mut rep[12..14], 1, msb);
                c.tx.extend_from_slice(&rep);
            }
            108 => {
                // GetScreenSaver
                let mut rep = [0u8; 32];
                rep[0] = 1;
                w16(&mut rep[2..4], c.seq, msb);
                c.tx.extend_from_slice(&rep);
            }
            117 => {
                // GetPointerMapping -> [1, 2, 3]
                let mut rep = [0u8; 36];
                rep[0] = 1;
                rep[1] = 3; // map_len
                w16(&mut rep[2..4], c.seq, msb);
                w32(&mut rep[4..8], 1, msb);
                rep[32] = 1;
                rep[33] = 2;
                rep[34] = 3;
                c.tx.extend_from_slice(&rep);
            }
            XGE_EXT_OPCODE => {
                // Generic Event Extension: GEQueryVersion (minor 0)
                let mut rep = [0u8; 32];
                rep[0] = 1;
                w16(&mut rep[2..4], c.seq, msb);
                w16(&mut rep[8..10], 1, msb); // major
                w16(&mut rep[10..12], 0, msb); // minor
                c.tx.extend_from_slice(&rep);
            }
            XINPUT_EXT_OPCODE => {
                self.handle_xinput_req(c, detail, pkt);
            }
            RENDER_EXT_OPCODE => {
                self.handle_render_req(c, detail, pkt);
            }
            SHAPE_EXT_OPCODE => {
                self.handle_shape_req(c, detail);
            }
            _ => {}
        }
    }

    fn handle_shape_req(&self, c: &mut X11Client, minor: u8) {
        let msb = c.msb;
        if minor == 0 {
            // ShapeQueryVersion -> 1.1
            let mut rep = [0u8; 32];
            rep[0] = 1;
            w16(&mut rep[2..4], c.seq, msb);
            w16(&mut rep[8..10], 1, msb);
            w16(&mut rep[10..12], 1, msb);
            c.tx.extend_from_slice(&rep);
        }
    }

    fn handle_render_req(&mut self, c: &mut X11Client, minor: u8, pkt: &[u8]) {
        let msb = c.msb;
        const FMT_ARGB32: u32 = 0x20;
        const FMT_RGB24: u32 = 0x21;
        const FMT_A8: u32 = 0x22;
        match minor {
            0 => {
                // RenderQueryVersion -> 0.11
                let mut rep = [0u8; 32];
                rep[0] = 1;
                w16(&mut rep[2..4], c.seq, msb);
                w32(&mut rep[8..12], 0, msb);
                w32(&mut rep[12..16], 11, msb);
                c.tx.extend_from_slice(&rep);
            }
            1 => {
                // RenderQueryPictFormats: 3 PictFormInfo (28 bytes each = 84) + 1 PictScreen (8 + 8 + 8 + 8 = 32)
                // Total payload = 84 + 32 = 116 bytes = 29 words
                let mut rep = vec![0u8; 32 + 116];
                rep[0] = 1;
                w16(&mut rep[2..4], c.seq, msb);
                w32(&mut rep[4..8], 29, msb);
                w32(&mut rep[8..12], 3, msb); // numFormats
                w32(&mut rep[12..16], 1, msb); // numScreens
                w32(&mut rep[16..20], 1, msb); // numDepths
                w32(&mut rep[20..24], 1, msb); // numVisuals

                // PictFormInfo 0: ARGB32
                let f0 = 32usize;
                w32(&mut rep[f0..f0 + 4], FMT_ARGB32, msb);
                rep[f0 + 4] = 1; // Direct
                rep[f0 + 5] = 32; // depth
                w16(&mut rep[f0 + 8..f0 + 10], 16, msb);
                w16(&mut rep[f0 + 10..f0 + 12], 255, msb);
                w16(&mut rep[f0 + 12..f0 + 14], 8, msb);
                w16(&mut rep[f0 + 14..f0 + 16], 255, msb);
                w16(&mut rep[f0 + 16..f0 + 18], 0, msb);
                w16(&mut rep[f0 + 18..f0 + 20], 255, msb);
                w16(&mut rep[f0 + 20..f0 + 22], 24, msb);
                w16(&mut rep[f0 + 22..f0 + 24], 255, msb);

                // PictFormInfo 1: RGB24 (x8r8g8b8)
                let f1 = 32 + 28;
                w32(&mut rep[f1..f1 + 4], FMT_RGB24, msb);
                rep[f1 + 4] = 1; // Direct
                rep[f1 + 5] = 24;
                w16(&mut rep[f1 + 8..f1 + 10], 16, msb);
                w16(&mut rep[f1 + 10..f1 + 12], 255, msb);
                w16(&mut rep[f1 + 12..f1 + 14], 8, msb);
                w16(&mut rep[f1 + 14..f1 + 16], 255, msb);
                w16(&mut rep[f1 + 16..f1 + 18], 0, msb);
                w16(&mut rep[f1 + 18..f1 + 20], 255, msb);

                // PictFormInfo 2: A8
                let f2 = 32 + 56;
                w32(&mut rep[f2..f2 + 4], FMT_A8, msb);
                rep[f2 + 4] = 1; // Direct
                rep[f2 + 5] = 8;
                w16(&mut rep[f2 + 20..f2 + 22], 0, msb);
                w16(&mut rep[f2 + 22..f2 + 24], 255, msb);

                // PictScreen (8) -> PictDepth (8) -> PictVisual (8) -> root visual matches FMT_RGB24
                let s0 = 32 + 84;
                w32(&mut rep[s0..s0 + 4], 1, msb); // numDepths
                w32(&mut rep[s0 + 4..s0 + 8], FMT_RGB24, msb); // fallback
                rep[s0 + 8] = 24; // depth
                w16(&mut rep[s0 + 10..s0 + 12], 1, msb); // numVisuals
                w32(&mut rep[s0 + 16..s0 + 20], ROOT_VISUAL, msb);
                w32(&mut rep[s0 + 20..s0 + 24], FMT_RGB24, msb);
                c.tx.extend_from_slice(&rep);
            }
            4 if pkt.len() >= 16 => {
                // RenderCreatePicture: pid(4), drawable(8), format(12), mask(16)
                let pid = r32(&pkt[4..8], msb);
                let did = r32(&pkt[8..12], msb);
                let fmt = r32(&pkt[12..16], msb);
                c.pictures.insert(pid, XRenderPic::Drawable { did, fmt, repeat: false });
            }
            5 if pkt.len() >= 12 => {
                // RenderChangePicture: pid(4), mask(8), values(12..)
                let pid = r32(&pkt[4..8], msb);
                let mask = r32(&pkt[8..12], msb);
                if (mask & 1) != 0 && pkt.len() >= 16
                    && let Some(XRenderPic::Drawable { repeat, .. }) = c.pictures.get_mut(&pid)
                {
                    *repeat = r32(&pkt[12..16], msb) != 0;
                }
            }
            7 if pkt.len() >= 8 => {
                let pid = r32(&pkt[4..8], msb);
                c.pictures.remove(&pid);
            }
            33 if pkt.len() >= 16 => {
                // RenderCreateSolidFill: pid(4), color(r16,g16,b16,a16 at 8..16)
                let pid = r32(&pkt[4..8], msb);
                let r = (r16(&pkt[8..10], msb) >> 8) as u32;
                let g = (r16(&pkt[10..12], msb) >> 8) as u32;
                let b = (r16(&pkt[12..14], msb) >> 8) as u32;
                let a = (r16(&pkt[14..16], msb) >> 8) as u32;
                c.pictures.insert(pid, XRenderPic::Solid((a << 24) | (r << 16) | (g << 8) | b));
            }
            26 if pkt.len() >= 20 => {
                // RenderFillRectangles: op(4), pad, dst(8..12), color(12..20), rects(20..)
                let rop = pkt[4];
                let dst_pid = r32(&pkt[8..12], msb);
                let r = (r16(&pkt[12..14], msb) >> 8) as u32;
                let g = (r16(&pkt[14..16], msb) >> 8) as u32;
                let b = (r16(&pkt[16..18], msb) >> 8) as u32;
                let a = (r16(&pkt[18..20], msb) >> 8) as u32;
                let col = (r << 16) | (g << 8) | b;
                if let Some(XRenderPic::Drawable { did, .. }) = c.pictures.get(&dst_pid).copied() {
                    let mut tw = 0;
                    let mut th = 0;
                    let mut ox = 0;
                    let mut oy = 0;
                    let mut target: Option<&mut [u32]> = None;
                    if let Some((gid, gx, gy)) = c.find_root_gid(did)
                        && let Some(surf) = self.surfaces.get_mut(&gid)
                    {
                        tw = surf.w;
                        th = surf.h;
                        ox = gx;
                        oy = gy;
                        target = Some(&mut surf.pixels);
                    } else if let Some(pm) = c.pixmaps.get_mut(&did) {
                        tw = pm.w;
                        th = pm.h;
                        target = Some(&mut pm.pixels);
                    }
                    if let Some(buf) = target {
                        for rect in pkt[20..].as_chunks::<8>().0 {
                            let rx = ox + (r16(&rect[0..2], msb) as i16 as i32);
                            let ry = oy + (r16(&rect[2..4], msb) as i16 as i32);
                            let rw = r16(&rect[4..6], msb) as i32;
                            let rh = r16(&rect[6..8], msb) as i32;
                            if rop == 0 {
                                fill_rect_buf(buf, (tw, th), rx, ry, rw, rh, 0);
                            } else if rop == 1 || a >= 250 {
                                fill_rect_buf(buf, (tw, th), rx, ry, rw, rh, col);
                            } else if a > 0 {
                                blend_rect_buf(buf, (tw, th), rx, ry, rw, rh, col, a);
                            }
                        }
                    }
                }
            }
            8 if pkt.len() >= 36 => {
                // RenderComposite: op(4), src(8), mask(12), dst(16), sx(20), sy(22), mx(24), my(26), dx(28), dy(30), w(32), h(34)
                let rop = pkt[4];
                let src_pid = r32(&pkt[8..12], msb);
                let mask_pid = r32(&pkt[12..16], msb);
                let dst_pid = r32(&pkt[16..20], msb);
                let sx0 = r16(&pkt[20..22], msb) as i16 as i32;
                let sy0 = r16(&pkt[22..24], msb) as i16 as i32;
                let mx0 = r16(&pkt[24..26], msb) as i16 as i32;
                let my0 = r16(&pkt[26..28], msb) as i16 as i32;
                let dx = r16(&pkt[28..30], msb) as i16 as i32;
                let dy = r16(&pkt[30..32], msb) as i16 as i32;
                let w = r16(&pkt[32..34], msb) as i32;
                let h = r16(&pkt[34..36], msb) as i32;
                let src_pic = c.pictures.get(&src_pid).copied();
                let mask_info = if mask_pid != 0 {
                    c.pictures.get(&mask_pid).copied().and_then(|mp| match mp {
                        XRenderPic::Drawable { did, fmt, repeat } => {
                            c.pixmaps.get(&did).map(|pm| (pm.w, pm.h, fmt, repeat, pm.pixels.clone()))
                        }
                        XRenderPic::Solid(argb) => Some((1, 1, FMT_ARGB32, true, vec![argb])),
                    })
                } else {
                    None
                };
                let src_pm = match src_pic {
                    Some(XRenderPic::Drawable { did: sdid, fmt, repeat }) => {
                        c.pixmaps.get(&sdid).map(|pm| (pm.w, pm.h, fmt, repeat, pm.pixels.clone()))
                    }
                    _ => None,
                };
                if let (Some(src), Some(XRenderPic::Drawable { did, .. })) =
                    (src_pic, c.pictures.get(&dst_pid).copied())
                {
                    let mut tw = 0;
                    let mut th = 0;
                    let mut ox = 0;
                    let mut oy = 0;
                    let mut target: Option<&mut [u32]> = None;
                    if let Some((gid, gx, gy)) = c.find_root_gid(did)
                        && let Some(surf) = self.surfaces.get_mut(&gid)
                    {
                        tw = surf.w;
                        th = surf.h;
                        ox = gx;
                        oy = gy;
                        target = Some(&mut surf.pixels);
                    } else if let Some(pm) = c.pixmaps.get_mut(&did) {
                        tw = pm.w;
                        th = pm.h;
                        target = Some(&mut pm.pixels);
                    }
                    if let Some(buf) = target {
                        let sample_mask = |col: i32, row: i32| -> u32 {
                            let Some((mw, mh, mfmt, mrep, ref mpx)) = mask_info else {
                                return 255;
                            };
                            if mw <= 0 || mh <= 0 || mpx.is_empty() {
                                return 0;
                            }
                            let mxx = if mrep {
                                (mx0 + col).rem_euclid(mw)
                            } else {
                                mx0 + col
                            };
                            let myy = if mrep {
                                (my0 + row).rem_euclid(mh)
                            } else {
                                my0 + row
                            };
                            if mxx < 0 || mxx >= mw || myy < 0 || myy >= mh {
                                return 0;
                            }
                            let v = mpx[(myy * mw + mxx) as usize];
                            if mfmt == FMT_A8 {
                                v & 0xFF
                            } else if mfmt == FMT_ARGB32 {
                                (v >> 24) & 0xFF
                            } else {
                                255
                            }
                        };
                        match src {
                            XRenderPic::Solid(argb) => {
                                let sa = (argb >> 24) & 0xFF;
                                let rgb = argb & 0x00FF_FFFF;
                                if mask_info.is_none() && rop == 1 {
                                    fill_rect_buf(buf, (tw, th), ox + dx, oy + dy, w, h, rgb);
                                } else if mask_info.is_none() {
                                    blend_rect_buf(buf, (tw, th), ox + dx, oy + dy, w, h, rgb, sa);
                                } else {
                                    for row in 0..h {
                                        for col in 0..w {
                                            let ma = sample_mask(col, row);
                                            let eff_a = (sa * ma + 127) / 255;
                                            blend_px_rgb(buf, tw, th, ox + dx + col, oy + dy + row, rgb, eff_a);
                                        }
                                    }
                                }
                            }
                            XRenderPic::Drawable { fmt, repeat, .. } if fmt != FMT_A8 => {
                                if let Some((sw, sh, _, _, ref spx)) = src_pm
                                    && sw > 0
                                    && sh > 0
                                {
                                    if mask_info.is_none() && repeat && sw == 1 && sh == 1 && !spx.is_empty() {
                                        let v = spx[0];
                                        let sa = if fmt == FMT_ARGB32 { (v >> 24) & 0xFF } else { 255 };
                                        if rop == 1 || sa >= 250 {
                                            fill_rect_buf(buf, (tw, th), ox + dx, oy + dy, w, h, v);
                                        } else {
                                            blend_rect_buf(buf, (tw, th), ox + dx, oy + dy, w, h, v, sa);
                                        }
                                    } else {
                                        for row in 0..h {
                                            let syy = if repeat {
                                                (sy0 + row).rem_euclid(sh)
                                            } else {
                                                sy0 + row
                                            };
                                            if syy < 0 || syy >= sh {
                                                continue;
                                            }
                                            for col in 0..w {
                                                let sxx = if repeat {
                                                    (sx0 + col).rem_euclid(sw)
                                                } else {
                                                    sx0 + col
                                                };
                                                if sxx < 0 || sxx >= sw {
                                                    continue;
                                                }
                                                let v = spx[(syy * sw + sxx) as usize];
                                                let sa = if fmt == FMT_ARGB32 { (v >> 24) & 0xFF } else { 255 };
                                                let ma = sample_mask(col, row);
                                                let eff_a = (sa * ma + 127) / 255;
                                                if rop == 1 && mask_info.is_none() {
                                                    put_px(buf, tw, th, ox + dx + col, oy + dy + row, v);
                                                } else {
                                                    blend_px_rgb(
                                                        buf,
                                                        tw,
                                                        th,
                                                        ox + dx + col,
                                                        oy + dy + row,
                                                        v & 0x00FF_FFFF,
                                                        eff_a,
                                                    );
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }
            17 if pkt.len() >= 12 => {
                // RenderCreateGlyphSet: gsid(4), format(8)
                let gsid = r32(&pkt[4..8], msb);
                c.glyphsets.insert(gsid, HashMap::new());
            }
            19 if pkt.len() >= 8 => {
                // RenderFreeGlyphSet
                let gsid = r32(&pkt[4..8], msb);
                c.glyphsets.remove(&gsid);
            }
            20 if pkt.len() >= 12 => {
                // RenderAddGlyphs: gsid(4), num_glyphs(8), glyphids(4*n), glyphinfos(12*n), data(...)
                let gsid = r32(&pkt[4..8], msb);
                let n = r32(&pkt[8..12], msb) as usize;
                let ids_off = 12usize;
                let infos_off = ids_off + n * 4;
                let data_off = infos_off + n * 12;
                if pkt.len() >= data_off {
                    let gset = c.glyphsets.entry(gsid).or_default();
                    let mut cur = data_off;
                    for i in 0..n {
                        let gid = r32(&pkt[ids_off + i * 4..ids_off + i * 4 + 4], msb);
                        let io = infos_off + i * 12;
                        let gw = r16(&pkt[io..io + 2], msb);
                        let gh = r16(&pkt[io + 2..io + 4], msb);
                        let gx = r16(&pkt[io + 4..io + 6], msb) as i16;
                        let gy = r16(&pkt[io + 6..io + 8], msb) as i16;
                        let stride = pad4(gw as usize);
                        let bytes = stride * (gh as usize);
                        let mut alpha = vec![0u8; (gw as usize) * (gh as usize)];
                        if cur + bytes <= pkt.len() {
                            for row in 0..(gh as usize) {
                                let src_row = &pkt[cur + row * stride..cur + row * stride + (gw as usize)];
                                alpha[row * (gw as usize)..(row + 1) * (gw as usize)]
                                    .copy_from_slice(src_row);
                            }
                            cur += bytes;
                        }
                        gset.insert(
                            gid,
                            XRenderGlyph {
                                w: gw,
                                h: gh,
                                x_off: gx,
                                y_off: gy,
                                alpha,
                            },
                        );
                    }
                }
            }
            23..=25 if pkt.len() >= 28 => {
                // RenderCompositeGlyphs8 (23), 16 (24), 32 (25)
                let src_pid = r32(&pkt[8..12], msb);
                let dst_pid = r32(&pkt[12..16], msb);
                let mut gsid = r32(&pkt[20..24], msb);
                let (rgb, sa) = match c.pictures.get(&src_pid).copied() {
                    Some(XRenderPic::Solid(argb)) => (argb & 0x00FF_FFFF, (argb >> 24) & 0xFF),
                    _ => (0x0000_0000, 255),
                };
                if let Some(XRenderPic::Drawable { did, .. }) = c.pictures.get(&dst_pid).copied() {
                    let mut tw = 0;
                    let mut th = 0;
                    let mut ox = 0;
                    let mut oy = 0;
                    let mut target: Option<&mut [u32]> = None;
                    if let Some((gid, gx, gy)) = c.find_root_gid(did)
                        && let Some(surf) = self.surfaces.get_mut(&gid)
                    {
                        tw = surf.w;
                        th = surf.h;
                        ox = gx;
                        oy = gy;
                        target = Some(&mut surf.pixels);
                    } else if let Some(pm) = c.pixmaps.get_mut(&did) {
                        tw = pm.w;
                        th = pm.h;
                        target = Some(&mut pm.pixels);
                    }
                    if let Some(buf) = target {
                        let elem_sz = match minor {
                            23 => 1usize,
                            24 => 2usize,
                            _ => 4usize,
                        };
                        let mut p = 28usize;
                        let mut pen_x = 0i32;
                        let mut pen_y = 0i32;
                        while p + 8 <= pkt.len() {
                            let count = pkt[p] as usize;
                            let dx = r16(&pkt[p + 4..p + 6], msb) as i16 as i32;
                            let dy = r16(&pkt[p + 6..p + 8], msb) as i16 as i32;
                            p += 8;
                            if count == 255 {
                                if p + 4 <= pkt.len() {
                                    gsid = r32(&pkt[p..p + 4], msb);
                                    p += 4;
                                }
                                continue;
                            }
                            pen_x += dx;
                            pen_y += dy;
                            let bytes = pad4(count * elem_sz);
                            if p + bytes > pkt.len() {
                                break;
                            }
                            if let Some(gset) = c.glyphsets.get(&gsid) {
                                for i in 0..count {
                                    let gid = match elem_sz {
                                        1 => pkt[p + i] as u32,
                                        2 => r16(&pkt[p + i * 2..p + i * 2 + 2], msb) as u32,
                                        _ => r32(&pkt[p + i * 4..p + i * 4 + 4], msb),
                                    };
                                    if let Some(gl) = gset.get(&gid) {
                                        let gx0 = ox + pen_x - gl.x_off as i32;
                                        let gy0 = oy + pen_y - gl.y_off as i32;
                                        for row in 0..(gl.h as i32) {
                                            for col in 0..(gl.w as i32) {
                                                let ga = gl.alpha[(row as usize) * (gl.w as usize) + (col as usize)] as u32;
                                                if ga > 0 {
                                                    let eff_a = (sa * ga + 127) / 255;
                                                    blend_px_rgb(buf, tw, th, gx0 + col, gy0 + row, rgb, eff_a);
                                                }
                                            }
                                        }
                                        pen_x += gl.w as i32;
                                    }
                                }
                            }
                            p += bytes;
                        }
                    }
                }
            }
            _ => {}
        }
    }

    fn handle_xinput_req(&self, c: &mut X11Client, minor: u8, pkt: &[u8]) {
        let msb = c.msb;
        match minor {
            1 => {
                // XI1 GetExtensionVersion
                let mut rep = [0u8; 32];
                rep[0] = 1;
                rep[1] = 1; // Xi_GetExtensionVersion
                w16(&mut rep[2..4], c.seq, msb);
                w16(&mut rep[8..10], 2, msb); // major
                w16(&mut rep[10..12], 3, msb); // minor
                rep[12] = 1; // present
                c.tx.extend_from_slice(&rep);
            }
            2 => {
                // XI1 ListInputDevices -> 0 ext devices (VCP/VCK are core)
                let mut rep = [0u8; 32];
                rep[0] = 1;
                rep[1] = 2;
                w16(&mut rep[2..4], c.seq, msb);
                rep[8] = 0;
                c.tx.extend_from_slice(&rep);
            }
            46 if pkt.len() >= 12 => {
                // XISelectEvents: hdr(8) + win(4) + num_masks(2) + pad(2) -> check pkt[8..10]
                c.xi_selected = true;
                let num_masks = r16(&pkt[8..10], msb) as usize;
                let mut off = 12usize;
                for _ in 0..num_masks {
                    if off + 4 > pkt.len() {
                        break;
                    }
                    let mlen = r16(&pkt[off + 2..off + 4], msb) as usize;
                    let bytes = mlen * 4;
                    if off + 4 + bytes <= pkt.len() && bytes >= 4 {
                        let mask0 = r32(&pkt[off + 4..off + 8], msb);
                        if (mask0 & (1 << 17)) != 0 {
                            c.xi_raw_motion = true;
                        }
                    }
                    off += 4 + bytes;
                }
            }
            47 => {
                // XIQueryVersion
                let cmaj = if pkt.len() >= 6 { r16(&pkt[4..6], msb) } else { 2 };
                let cmin = if pkt.len() >= 8 { r16(&pkt[6..8], msb) } else { 3 };
                let maj = cmaj.min(2);
                let min = if maj >= 2 { cmin.min(3) } else { 0 };
                let mut rep = [0u8; 32];
                rep[0] = 1;
                w16(&mut rep[2..4], c.seq, msb);
                w16(&mut rep[8..10], maj, msb);
                w16(&mut rep[10..12], min, msb);
                c.tx.extend_from_slice(&rep);
            }
            48 => {
                // XIQueryDevice
                let dev_id = if pkt.len() >= 6 { r16(&pkt[4..6], msb) } else { 0 };
                let mut payload: Vec<u8> = Vec::with_capacity(256);
                let mut n_devs = 0u16;

                let push_pointer = |buf: &mut Vec<u8>| {
                    let name = b"Virtual core pointer";
                    let nlen = pad4(name.len());
                    let start = buf.len();
                    buf.resize(start + 12 + nlen, 0);
                    w16(&mut buf[start..start + 2], 2, msb); // id = 2
                    w16(&mut buf[start + 2..start + 4], 1, msb); // XIMasterPointer
                    w16(&mut buf[start + 4..start + 6], 3, msb); // attached = 3
                    w16(&mut buf[start + 6..start + 8], 3, msb); // 3 classes: Button + ValX + ValY
                    w16(&mut buf[start + 8..start + 10], name.len() as u16, msb);
                    buf[start + 10] = 1; // enabled
                    buf[start + 12..start + 12 + name.len()].copy_from_slice(name);

                    // ButtonClass (24 bytes = 6 words)
                    let b0 = buf.len();
                    buf.resize(b0 + 24, 0);
                    w16(&mut buf[b0..b0 + 2], 1, msb); // XIButtonClass
                    w16(&mut buf[b0 + 2..b0 + 4], 6, msb);
                    w16(&mut buf[b0 + 4..b0 + 6], 2, msb);
                    w16(&mut buf[b0 + 6..b0 + 8], 3, msb); // 3 buttons

                    // 2 ValuatorClass (44 bytes each = 11 words)
                    for &(axis_num, label_atom, max_val, cur_val) in &[
                        (0u16, 72u32, self.screen_w as i32, self.ptr_x),
                        (1u16, 73u32, self.screen_h as i32, self.ptr_y),
                    ] {
                        let v0 = buf.len();
                        buf.resize(v0 + 44, 0);
                        w16(&mut buf[v0..v0 + 2], 2, msb); // XIValuatorClass
                        w16(&mut buf[v0 + 2..v0 + 4], 11, msb);
                        w16(&mut buf[v0 + 4..v0 + 6], 2, msb);
                        w16(&mut buf[v0 + 6..v0 + 8], axis_num, msb);
                        w32(&mut buf[v0 + 8..v0 + 12], label_atom, msb);
                        w32(&mut buf[v0 + 20..v0 + 24], max_val as u32, msb);
                        w32(&mut buf[v0 + 28..v0 + 32], cur_val as u32, msb);
                        w32(&mut buf[v0 + 36..v0 + 40], 1000, msb);
                        buf[v0 + 40] = 1; // Absolute
                    }
                };

                let push_keyboard = |buf: &mut Vec<u8>| {
                    let name = b"Virtual core keyboard";
                    let nlen = pad4(name.len());
                    let start = buf.len();
                    buf.resize(start + 12 + nlen, 0);
                    w16(&mut buf[start..start + 2], 3, msb); // id = 3
                    w16(&mut buf[start + 2..start + 4], 2, msb); // XIMasterKeyboard
                    w16(&mut buf[start + 4..start + 6], 2, msb); // attached = 2
                    w16(&mut buf[start + 6..start + 8], 1, msb); // 1 KeyClass
                    w16(&mut buf[start + 8..start + 10], name.len() as u16, msb);
                    buf[start + 10] = 1; // enabled
                    buf[start + 12..start + 12 + name.len()].copy_from_slice(name);

                    // KeyClass (16 bytes = 4 words)
                    let k0 = buf.len();
                    buf.resize(k0 + 16, 0);
                    w16(&mut buf[k0..k0 + 2], 0, msb); // XIKeyClass
                    w16(&mut buf[k0 + 2..k0 + 4], 4, msb);
                    w16(&mut buf[k0 + 4..k0 + 6], 3, msb);
                    w16(&mut buf[k0 + 6..k0 + 8], 2, msb);
                    w32(&mut buf[k0 + 8..k0 + 12], 36, msb);
                    w32(&mut buf[k0 + 12..k0 + 16], 65, msb);
                };

                if matches!(dev_id, 0..=2) {
                    push_pointer(&mut payload);
                    n_devs += 1;
                }
                if matches!(dev_id, 0 | 1 | 3) {
                    push_keyboard(&mut payload);
                    n_devs += 1;
                }
                let mut rep = vec![0u8; 32 + payload.len()];
                rep[0] = 1;
                w16(&mut rep[2..4], c.seq, msb);
                w32(&mut rep[4..8], (payload.len() / 4) as u32, msb);
                w16(&mut rep[8..10], n_devs, msb);
                rep[32..].copy_from_slice(&payload);
                c.tx.extend_from_slice(&rep);
            }
            51 if pkt.len() >= 8 => {
                // XIQueryPointer
                let wid = r32(&pkt[4..8], msb);
                let (wx, wy) = if let Some((gid, ox, oy)) = c.find_root_gid(wid) {
                    let (sox, soy) = self.surf_origins.get(&gid).copied().unwrap_or((0, 0));
                    (self.ptr_x - sox - ox, self.ptr_y - soy - oy)
                } else {
                    (self.ptr_x, self.ptr_y)
                };
                let (child, _, _) = c.find_hit_window(wid, wx, wy);
                let child = if child == wid { 0 } else { child };
                let mut rep = [0u8; 56];
                rep[0] = 1;
                w16(&mut rep[2..4], c.seq, msb);
                w32(&mut rep[4..8], 6, msb);
                w32(&mut rep[8..12], ROOT_WIN, msb);
                w32(&mut rep[12..16], child, msb);
                w32(&mut rep[16..20], (self.ptr_x << 16) as u32, msb);
                w32(&mut rep[20..24], (self.ptr_y << 16) as u32, msb);
                w32(&mut rep[24..28], (wx << 16) as u32, msb);
                w32(&mut rep[28..32], (wy << 16) as u32, msb);
                rep[32] = 1; // same_screen
                w16(&mut rep[34..36], 0, msb); // buttons_len = 0
                c.tx.extend_from_slice(&rep);
            }
            52 => {
                // XIGrabDevice -> GrabSuccess
                let mut rep = [0u8; 32];
                rep[0] = 1;
                w16(&mut rep[2..4], c.seq, msb);
                rep[8] = 0; // XIGrabSuccess
                c.tx.extend_from_slice(&rep);
            }
            55 => {
                // XIGetSelectedEvents
                let mut rep = [0u8; 32];
                rep[0] = 1;
                w16(&mut rep[2..4], c.seq, msb);
                c.tx.extend_from_slice(&rep);
            }
            _ => {}
        }
    }

    #[cfg(target_os = "linux")]
    fn process_wl_client(
        &mut self,
        c: &mut WaylandClient,
        newly_mapped: &mut Vec<u32>,
        unmapped: &mut Vec<u32>,
    ) {
        while c.rx.len() >= 8 {
            let obj_id = u32::from_le_bytes([c.rx[0], c.rx[1], c.rx[2], c.rx[3]]);
            let word = u32::from_le_bytes([c.rx[4], c.rx[5], c.rx[6], c.rx[7]]);
            let size = (word >> 16) as usize;
            let op = (word & 0xFFFF) as u16;
            if size < 8 {
                c.dead = true;
                return;
            }
            if c.rx.len() < size {
                return;
            }
            let pkt: Vec<u8> = c.rx.drain(0..size).collect();
            let args = &pkt[8..];
            self.handle_wl_req(c, obj_id, op, args, newly_mapped, unmapped);
        }
    }

    #[cfg(target_os = "linux")]
    fn handle_wl_req(
        &mut self,
        c: &mut WaylandClient,
        obj_id: u32,
        op: u16,
        args: &[u8],
        newly_mapped: &mut Vec<u32>,
        unmapped: &mut Vec<u32>,
    ) {
        let kind = match c.objs.get(&obj_id) {
            Some(WlObj::Display) => 0,
            Some(WlObj::Registry) => 1,
            Some(WlObj::Compositor) => 2,
            Some(WlObj::Shm) => 3,
            Some(WlObj::ShmPool(_)) => 4,
            Some(WlObj::Buffer { .. }) => 5,
            Some(WlObj::Surface { .. }) => 6,
            Some(WlObj::XdgWmBase) => 7,
            Some(WlObj::XdgSurface { .. }) => 8,
            Some(WlObj::XdgToplevel { .. }) => 9,
            Some(WlObj::Seat) => 10,
            _ => return,
        };

        match (kind, op) {
            (0, 0) if args.len() >= 4 => {
                // wl_display::sync(cb)
                let cb = r32(&args[0..4], false);
                let ser = c.next_serial();
                c.send_msg(cb, 0, &ser.to_le_bytes(), None); // wl_callback::done
                c.send_msg(1, 1, &cb.to_le_bytes(), None); // wl_display::delete_id
            }
            (0, 1) if args.len() >= 4 => {
                // wl_display::get_registry(id)
                let reg = r32(&args[0..4], false);
                c.objs.insert(reg, WlObj::Registry);
                for &(name, iface, ver) in &[
                    (1u32, "wl_compositor", 4u32),
                    (2, "wl_shm", 1),
                    (3, "xdg_wm_base", 2),
                    (4, "wl_seat", 5),
                ] {
                    let mut p = Vec::with_capacity(32);
                    p.extend_from_slice(&name.to_le_bytes());
                    WaylandClient::encode_wl_str(&mut p, iface);
                    p.extend_from_slice(&ver.to_le_bytes());
                    c.send_msg(reg, 0, &p, None); // wl_registry::global
                }
            }
            (1, 0) if args.len() >= 12 => {
                // wl_registry::bind(name, iface_str, version, new_id)
                let name = r32(&args[0..4], false);
                let slen = r32(&args[4..8], false) as usize;
                let off = 8 + pad4(slen);
                if off + 8 <= args.len() {
                    let new_id = r32(&args[off + 4..off + 8], false);
                    match name {
                        1 => {
                            c.objs.insert(new_id, WlObj::Compositor);
                        }
                        2 => {
                            c.objs.insert(new_id, WlObj::Shm);
                            c.send_msg(new_id, 0, &0u32.to_le_bytes(), None); // ARGB8888
                            c.send_msg(new_id, 0, &1u32.to_le_bytes(), None); // XRGB8888
                        }
                        3 => {
                            c.objs.insert(new_id, WlObj::XdgWmBase);
                        }
                        4 => {
                            c.objs.insert(new_id, WlObj::Seat);
                            c.send_msg(new_id, 0, &3u32.to_le_bytes(), None); // POINTER | KEYBOARD
                            let mut np = Vec::new();
                            WaylandClient::encode_wl_str(&mut np, "seat0");
                            c.send_msg(new_id, 1, &np, None);
                        }
                        _ => {}
                    }
                }
            }
            (2, 0) if args.len() >= 4 => {
                // wl_compositor::create_surface(id)
                let sid = r32(&args[0..4], false);
                c.objs.insert(
                    sid,
                    WlObj::Surface {
                        gid: None,
                        pending_buf: None,
                        pending_frame_cbs: Vec::new(),
                    },
                );
            }
            (3, 0) if args.len() >= 8 => {
                // wl_shm::create_pool(id, size) + fd from fds
                let pid = r32(&args[0..4], false);
                let sz = r32(&args[4..8], false) as i32;
                if let Some(fd) = c.fds.pop_front()
                    && sz > 0
                    && let Ok(map) = unix_ipc::ShmMap::map(fd, sz as usize)
                {
                    c.objs.insert(pid, WlObj::ShmPool(map));
                }
            }
            (4, 0) if args.len() >= 24 => {
                // wl_shm_pool::create_buffer(id, off, w, h, stride, fmt)
                let bid = r32(&args[0..4], false);
                let offset = r32(&args[4..8], false) as i32 as usize;
                let bw = r32(&args[8..12], false) as i32;
                let bh = r32(&args[12..16], false) as i32;
                let stride = r32(&args[16..20], false) as i32 as usize;
                let format = r32(&args[20..24], false);
                c.objs.insert(
                    bid,
                    WlObj::Buffer {
                        pool_id: obj_id,
                        offset,
                        w: bw,
                        h: bh,
                        stride,
                        format,
                    },
                );
            }
            (4, 1) => {
                // wl_shm_pool::destroy
                c.objs.remove(&obj_id);
            }
            (4, 2) if args.len() >= 4 => {
                // wl_shm_pool::resize(size)
                let nsz = r32(&args[0..4], false) as i32;
                if nsz > 0
                    && let Some(WlObj::ShmPool(map)) = c.objs.get_mut(&obj_id)
                {
                    let _ = map.resize(nsz as usize);
                }
            }
            (5, 0) => {
                // wl_buffer::destroy
                c.objs.remove(&obj_id);
            }
            (6, 0) => {
                // wl_surface::destroy
                if let Some(WlObj::Surface { gid: Some(g), .. }) = c.objs.remove(&obj_id) {
                    self.surfaces.remove(&g);
                    unmapped.push(g);
                }
            }
            (6, 1) if args.len() >= 4 => {
                // wl_surface::attach(buf_id, x, y)
                let bid = r32(&args[0..4], false);
                if let Some(WlObj::Surface { pending_buf, .. }) = c.objs.get_mut(&obj_id) {
                    *pending_buf = if bid == 0 { None } else { Some(bid) };
                }
            }
            (6, 3) if args.len() >= 4 => {
                // wl_surface::frame(cb_id)
                let cb = r32(&args[0..4], false);
                if let Some(WlObj::Surface { pending_frame_cbs, .. }) = c.objs.get_mut(&obj_id) {
                    pending_frame_cbs.push(cb);
                }
            }
            (6, 6) => {
                // wl_surface::commit()
                let (gid_opt, buf_id_opt, cbs) = match c.objs.get_mut(&obj_id) {
                    Some(WlObj::Surface {
                        gid,
                        pending_buf,
                        pending_frame_cbs,
                    }) => (*gid, *pending_buf, std::mem::take(pending_frame_cbs)),
                    _ => return,
                };
                if let Some(bid) = buf_id_opt
                    && let Some(&WlObj::Buffer {
                        pool_id,
                        offset,
                        w: bw,
                        h: bh,
                        stride,
                        format,
                    }) = c.objs.get(&bid)
                    && let Some(WlObj::ShmPool(map)) = c.objs.get(&pool_id)
                    && let Some(gid) = gid_opt
                    && let Some(surf) = self.surfaces.get_mut(&gid)
                {
                    let cw = bw.clamp(1, 1600);
                    let ch = bh.clamp(1, 1200);
                    if surf.w != cw || surf.h != ch {
                        surf.w = cw;
                        surf.h = ch;
                        surf.pixels.resize((cw * ch) as usize, 0);
                    }
                    let src = map.slice();
                    let _ = format;
                    for row in 0..ch {
                        let roff = offset + (row as usize) * stride;
                        let need = (cw as usize) * 4;
                        if roff + need > src.len() {
                            break;
                        }
                        for col in 0..cw {
                            let po = roff + (col as usize) * 4;
                            let px = u32::from_le_bytes([src[po], src[po + 1], src[po + 2], src[po + 3]]);
                            surf.pixels[(row * cw + col) as usize] = px & 0x00FF_FFFF;
                        }
                    }
                    if !surf.mapped {
                        surf.mapped = true;
                        newly_mapped.push(gid);
                    }
                    c.send_msg(bid, 0, &[], None); // wl_buffer::release
                }
                let ser = c.next_serial();
                for cb in cbs {
                    c.send_msg(cb, 0, &ser.to_le_bytes(), None);
                    c.send_msg(1, 1, &cb.to_le_bytes(), None);
                }
            }
            (7, 2) if args.len() >= 8 => {
                // xdg_wm_base::get_xdg_surface(id, surface)
                let xsid = r32(&args[0..4], false);
                let sid = r32(&args[4..8], false);
                c.objs.insert(xsid, WlObj::XdgSurface { surface_id: sid });
            }
            (8, 1) if args.len() >= 4 => {
                // xdg_surface::get_toplevel(id)
                let tl_id = r32(&args[0..4], false);
                let sid = match c.objs.get(&obj_id) {
                    Some(WlObj::XdgSurface { surface_id }) => *surface_id,
                    _ => return,
                };
                let g = self.next_gid;
                self.next_gid += 1;
                self.surfaces.insert(
                    g,
                    GuestSurface {
                        gid: g,
                        proto: GuestProto::Wayland,
                        title: format!("Wayland App #{g}"),
                        w: 480,
                        h: 340,
                        pixels: vec![0x000D_1117; 480 * 340],
                        mapped: false,
                        client_id: c.id,
                        proto_obj_id: tl_id,
                    },
                );
                if let Some(WlObj::Surface { gid, .. }) = c.objs.get_mut(&sid) {
                    *gid = Some(g);
                }
                c.objs.insert(tl_id, WlObj::XdgToplevel { surface_id: sid });
                // Send initial configure sequence
                let mut tl_cfg = Vec::with_capacity(12);
                tl_cfg.extend_from_slice(&480i32.to_le_bytes());
                tl_cfg.extend_from_slice(&340i32.to_le_bytes());
                tl_cfg.extend_from_slice(&0u32.to_le_bytes()); // empty wl_array states
                c.send_msg(tl_id, 0, &tl_cfg, None);
                let ser = c.next_serial();
                c.send_msg(obj_id, 0, &ser.to_le_bytes(), None); // xdg_surface::configure
            }
            (9, 2) => {
                // xdg_toplevel::set_title(title)
                if args.len() >= 4 {
                    let slen = r32(&args[0..4], false) as usize;
                    if slen > 0 && 4 + slen <= args.len() {
                        let t = String::from_utf8_lossy(&args[4..4 + slen])
                            .trim_matches('\0')
                            .to_string();
                        if !t.is_empty()
                            && let Some(WlObj::XdgToplevel { surface_id }) = c.objs.get(&obj_id)
                            && let Some(WlObj::Surface { gid: Some(g), .. }) = c.objs.get(surface_id)
                            && let Some(surf) = self.surfaces.get_mut(g)
                        {
                            surf.title = t;
                        }
                    }
                }
            }
            (10, 0) if args.len() >= 4 => {
                // wl_seat::get_pointer(id)
                let pid = r32(&args[0..4], false);
                c.objs.insert(pid, WlObj::Pointer);
                c.pointer_obj = Some(pid);
            }
            (10, 1) if args.len() >= 4 => {
                // wl_seat::get_keyboard(id)
                let kid = r32(&args[0..4], false);
                c.objs.insert(kid, WlObj::Keyboard);
                c.keyboard_obj = Some(kid);
                if let Some(dev_null) = unix_ipc::open_dev_null_fd() {
                    let mut p = [0u8; 8];
                    p[0..4].copy_from_slice(&0u32.to_le_bytes()); // no_keymap
                    p[4..8].copy_from_slice(&0u32.to_le_bytes()); // size 0
                    c.send_msg(kid, 0, &p, Some(unix_ipc::duplicate_fd(&dev_null)));
                }
            }
            _ => {}
        }
    }

    pub fn resize_guest(&mut self, gid: u32, w: i32, h: i32) {
        let nw = w.clamp(64, 1600);
        let nh = h.clamp(48, 1200);
        let Some(surf) = self.surfaces.get_mut(&gid) else {
            return;
        };
        if surf.w == nw && surf.h == nh {
            return;
        }
        let mut next = vec![0x000D_1117; (nw * nh) as usize];
        let cw = surf.w.min(nw);
        let ch = surf.h.min(nh);
        for row in 0..ch {
            let so = (row * surf.w) as usize;
            let doff = (row * nw) as usize;
            next[doff..doff + cw as usize].copy_from_slice(&surf.pixels[so..so + cw as usize]);
        }
        surf.w = nw;
        surf.h = nh;
        surf.pixels = next;
        let cid = surf.client_id;
        let obj = surf.proto_obj_id;
        let proto = surf.proto;

        match proto {
            GuestProto::X11 => {
                if let Some(c) = self.x11_clients.iter_mut().find(|c| c.id == cid) {
                    c.send_configure_notify(obj, 0, 0, nw, nh);
                    c.send_expose(obj, nw, nh);
                    c.flush_tx();
                }
            }
            GuestProto::Wayland => {
                #[cfg(target_os = "linux")]
                if let Some(c) = self.wl_clients.iter_mut().find(|c| c.id == cid) {
                    let mut tl_cfg = Vec::with_capacity(12);
                    tl_cfg.extend_from_slice(&nw.to_le_bytes());
                    tl_cfg.extend_from_slice(&nh.to_le_bytes());
                    tl_cfg.extend_from_slice(&0u32.to_le_bytes());
                    c.send_msg(obj, 0, &tl_cfg, None);
                    let xs_id = c
                        .objs
                        .iter()
                        .find_map(|(&id, o)| match o {
                            WlObj::XdgSurface { surface_id } => {
                                if let Some(WlObj::XdgToplevel { surface_id: s2 }) = c.objs.get(&obj)
                                    && surface_id == s2
                                {
                                    Some(id)
                                } else {
                                    None
                                }
                            }
                            _ => None,
                        });
                    if let Some(xs) = xs_id {
                        let ser = c.next_serial();
                        c.send_msg(xs, 0, &ser.to_le_bytes(), None);
                    }
                }
            }
        }
    }

    pub fn send_pointer(&mut self, gid: u32, x: i32, y: i32, btn: Option<(u8, bool)>) {
        let Some(surf) = self.surfaces.get(&gid) else {
            return;
        };
        let cid = surf.client_id;
        let obj = surf.proto_obj_id;
        match surf.proto {
            GuestProto::X11 => {
                if let Some(c) = self.x11_clients.iter_mut().find(|c| c.id == cid) {
                    let (target_wid, tx, ty) = c.find_hit_window(obj, x, y);
                    let child = if target_wid == obj { 0 } else { target_wid };
                    c.send_pointer_ev(target_wid, child, self.ptr_x, self.ptr_y, tx, ty, btn);
                    if btn.is_none() && c.xi_selected {
                        c.send_xi_device_motion(target_wid, child, self.ptr_x, self.ptr_y, tx, ty);
                    }
                    c.flush_tx();
                }
            }
            GuestProto::Wayland => {
                #[cfg(target_os = "linux")]
                if let Some(c) = self.wl_clients.iter_mut().find(|c| c.id == cid)
                    && let Some(ptr) = c.pointer_obj
                {
                    let fx = (x << 8).to_le_bytes();
                    let fy = (y << 8).to_le_bytes();
                    let mut mp = [0u8; 12];
                    mp[0..4].copy_from_slice(&1u32.to_le_bytes());
                    mp[4..8].copy_from_slice(&fx);
                    mp[8..12].copy_from_slice(&fy);
                    c.send_msg(ptr, 2, &mp, None); // motion
                    if let Some((b, pressed)) = btn {
                        let ser = c.next_serial();
                        let mut bp = [0u8; 16];
                        bp[0..4].copy_from_slice(&ser.to_le_bytes());
                        bp[4..8].copy_from_slice(&1u32.to_le_bytes());
                        bp[8..12].copy_from_slice(&(0x110u32 + b.saturating_sub(1) as u32).to_le_bytes());
                        bp[12..16].copy_from_slice(&(u32::from(pressed)).to_le_bytes());
                        c.send_msg(ptr, 3, &bp, None); // button
                    }
                    c.send_msg(ptr, 5, &[], None); // frame
                }
            }
        }
    }

    pub fn send_key(&mut self, gid: u32, ch: Option<char>, pressed: bool) {
        let Some(surf) = self.surfaces.get(&gid) else {
            return;
        };
        let cid = surf.client_id;
        let obj = surf.proto_obj_id;
        match surf.proto {
            GuestProto::X11 => {
                let code = char_to_x11_keycode(ch);
                if let Some(c) = self.x11_clients.iter_mut().find(|c| c.id == cid) {
                    c.send_key_ev(obj, code, pressed);
                    c.flush_tx();
                }
            }
            GuestProto::Wayland => {
                #[cfg(target_os = "linux")]
                if let Some(c) = self.wl_clients.iter_mut().find(|c| c.id == cid)
                    && let Some(kbd) = c.keyboard_obj
                {
                    let ser = c.next_serial();
                    let code = ch.map(|c| c as u32).unwrap_or(28);
                    let mut kp = [0u8; 16];
                    kp[0..4].copy_from_slice(&ser.to_le_bytes());
                    kp[4..8].copy_from_slice(&1u32.to_le_bytes());
                    kp[8..12].copy_from_slice(&code.to_le_bytes());
                    kp[12..16].copy_from_slice(&(u32::from(pressed)).to_le_bytes());
                    c.send_msg(kbd, 3, &kp, None);
                }
            }
        }
    }

    pub fn close_guest(&mut self, gid: u32) {
        let Some(surf) = self.surfaces.remove(&gid) else {
            return;
        };
        match surf.proto {
            GuestProto::X11 => {
                if let Some(c) = self.x11_clients.iter_mut().find(|c| c.id == surf.client_id) {
                    c.send_delete_msg(surf.proto_obj_id);
                    c.flush_tx();
                    c.windows.remove(&surf.proto_obj_id);
                }
            }
            GuestProto::Wayland => {
                #[cfg(target_os = "linux")]
                if let Some(c) = self.wl_clients.iter_mut().find(|c| c.id == surf.client_id) {
                    c.send_msg(surf.proto_obj_id, 1, &[], None); // xdg_toplevel::close
                }
            }
        }
    }

    /// Launches a genuine X11 wire-protocol demo client against the local X11 TCP port
    /// so external X11 rendering can be seen live on both Windows and Linux.
    pub fn spawn_x11_demo(&self) -> bool {
        let Some(port) = self.tcp_port else {
            return false;
        };
        thread::spawn(move || {
            let Ok(mut s) = TcpStream::connect(("127.0.0.1", port)) else {
                return;
            };
            let _ = s.set_nonblocking(true);
            // X11 setup request (12 bytes)
            let mut setup = [0u8; 12];
            setup[0] = b'l';
            setup[2..4].copy_from_slice(&11u16.to_le_bytes());
            if s.write_all(&setup).is_err() {
                return;
            }
            // Wait briefly for server setup reply
            thread::sleep(std::time::Duration::from_millis(40));
            let mut reply = [0u8; 256];
            let _ = s.read(&mut reply);

            let wid: u32 = 0x0040_0001;
            let gc: u32 = 0x0040_0002;
            let w = 280u16;
            let h = 210u16;

            // CreateWindow (32 bytes = 8 words)
            let mut cw = [0u8; 32];
            cw[0] = 1;
            cw[1] = 24;
            cw[2..4].copy_from_slice(&8u16.to_le_bytes());
            cw[4..8].copy_from_slice(&wid.to_le_bytes());
            cw[8..12].copy_from_slice(&ROOT_WIN.to_le_bytes());
            cw[16..18].copy_from_slice(&w.to_le_bytes());
            cw[18..20].copy_from_slice(&h.to_le_bytes());
            cw[24..26].copy_from_slice(&1u16.to_le_bytes()); // InputOutput
            let _ = s.write_all(&cw);

            // ChangeProperty WM_NAME = "x11demo (MIT X11)"
            let title = b"x11demo (MIT X11)";
            let words = 6 + pad4(title.len()) / 4;
            let mut cp = vec![0u8; words * 4];
            cp[0] = 18;
            cp[1] = 0; // Replace
            cp[2..4].copy_from_slice(&(words as u16).to_le_bytes());
            cp[4..8].copy_from_slice(&wid.to_le_bytes());
            cp[8..12].copy_from_slice(&39u32.to_le_bytes()); // WM_NAME
            cp[12..16].copy_from_slice(&31u32.to_le_bytes()); // STRING
            cp[16] = 8;
            cp[20..24].copy_from_slice(&(title.len() as u32).to_le_bytes());
            cp[24..24 + title.len()].copy_from_slice(title);
            let _ = s.write_all(&cp);

            // CreateGC (16 bytes = 4 words)
            let mut cg = [0u8; 16];
            cg[0] = 55;
            cg[2..4].copy_from_slice(&4u16.to_le_bytes());
            cg[4..8].copy_from_slice(&gc.to_le_bytes());
            cg[8..12].copy_from_slice(&wid.to_le_bytes());
            let _ = s.write_all(&cg);

            // MapWindow (8 bytes = 2 words)
            let mut mw = [0u8; 8];
            mw[0] = 8;
            mw[2..4].copy_from_slice(&2u16.to_le_bytes());
            mw[4..8].copy_from_slice(&wid.to_le_bytes());
            let _ = s.write_all(&mw);

            // Render a live plasma/oscilloscope strip via X11 PolyFillRectangle + ImageText8
            let mut ev_buf = [0u8; 512];
            for frame in 0..360u32 {
                if let Ok(n) = s.read(&mut ev_buf)
                    && ev_buf[..n].as_chunks::<32>().0.iter().any(|e| e[0] == 33)
                {
                    break;
                }
                // Background clear via PolyFillRectangle
                let mut bg = [0u8; 20];
                bg[0] = 56; // ChangeGC fg=0x090D16
                bg[2..4].copy_from_slice(&4u16.to_le_bytes());
                bg[4..8].copy_from_slice(&gc.to_le_bytes());
                bg[8..12].copy_from_slice(&4u32.to_le_bytes()); // Foreground
                bg[12..16].copy_from_slice(&0x0009_0D16u32.to_le_bytes());
                let _ = s.write_all(&bg[..16]);

                let mut rect = [0u8; 20];
                rect[0] = 70; // PolyFillRectangle
                rect[2..4].copy_from_slice(&5u16.to_le_bytes());
                rect[4..8].copy_from_slice(&wid.to_le_bytes());
                rect[8..12].copy_from_slice(&gc.to_le_bytes());
                rect[16..18].copy_from_slice(&w.to_le_bytes());
                rect[18..20].copy_from_slice(&h.to_le_bytes());
                if s.write_all(&rect).is_err() {
                    break;
                }

                // Draw X11 wave bars via ChangeGC + PolyFillRectangle
                for bar in 0..16i32 {
                    let phase = ((frame as f32) * 0.14 + (bar as f32) * 0.45).sin();
                    let bh = (40.0 + phase * 36.0).round() as u16;
                    let col = if bar % 3 == 0 {
                        0x0038_BDF8u32
                    } else if bar % 3 == 1 {
                        0x00A8_55F7
                    } else {
                        0x003F_B950
                    };
                    bg[12..16].copy_from_slice(&col.to_le_bytes());
                    let _ = s.write_all(&bg[..16]);
                    let bx = (16 + bar * 15) as u16;
                    let by = 148u16.saturating_sub(bh);
                    rect[12..14].copy_from_slice(&bx.to_le_bytes());
                    rect[14..16].copy_from_slice(&by.to_le_bytes());
                    rect[16..18].copy_from_slice(&10u16.to_le_bytes());
                    rect[18..20].copy_from_slice(&bh.to_le_bytes());
                    let _ = s.write_all(&rect);
                }

                // Label via ImageText8
                let txt = format!("X11 Wire Frame #{frame:03}");
                let tb = txt.as_bytes();
                let words = 4 + pad4(tb.len()) / 4;
                let mut it = vec![0u8; words * 4];
                it[0] = 76;
                it[1] = tb.len() as u8;
                it[2..4].copy_from_slice(&(words as u16).to_le_bytes());
                it[4..8].copy_from_slice(&wid.to_le_bytes());
                it[8..12].copy_from_slice(&gc.to_le_bytes());
                it[12..14].copy_from_slice(&16u16.to_le_bytes());
                it[14..16].copy_from_slice(&36u16.to_le_bytes());
                it[16..16 + tb.len()].copy_from_slice(tb);
                let _ = s.write_all(&it);

                thread::sleep(std::time::Duration::from_millis(33));
            }
        });
        true
    }
}

fn parse_win_attrs(win: &mut XWin, mask: u32, vals: &[u8], msb: bool) {
    let mut off = 0usize;
    for bit in 0..15u32 {
        if (mask & (1 << bit)) != 0 {
            if off + 4 > vals.len() {
                break;
            }
            let v = r32(&vals[off..off + 4], msb);
            match bit {
                1 => {
                    win.bg_pixel = v & 0x00FF_FFFF;
                    win.has_bg = true;
                }
                3 => {
                    win.border_pixel = v & 0x00FF_FFFF;
                    win.has_border = true;
                }
                11 => {
                    win.event_mask = v;
                }
                _ => {}
            }
            off += 4;
        }
    }
}

fn parse_gc_values(mut gc: XGc, mask: u32, vals: &[u8], msb: bool) -> XGc {
    let mut off = 0usize;
    for bit in 0..23u32 {
        if (mask & (1 << bit)) != 0 {
            if off + 4 > vals.len() {
                break;
            }
            let v = r32(&vals[off..off + 4], msb);
            match bit {
                2 => gc.fg = v & 0x00FF_FFFF,
                3 => gc.bg = v & 0x00FF_FFFF,
                4 => gc.line_width = (v as i32).max(1),
                _ => {}
            }
            off += 4;
        }
    }
    gc
}
