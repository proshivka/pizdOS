use std::collections::HashMap;
#[cfg(target_os = "linux")]
use std::collections::VecDeque;
use std::io::{self, ErrorKind, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::thread;

use crate::font;

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
        let mut tmp = [0u8; 16384];
        let res = match self {
            X11Io::Tcp(s) => s.read(&mut tmp),
            #[cfg(target_os = "linux")]
            X11Io::Unix(s) => s.read(&mut tmp),
        };
        match res {
            Ok(0) => Err(io::Error::new(ErrorKind::UnexpectedEof, "eof")),
            Ok(n) => {
                dst.extend_from_slice(&tmp[..n]);
                Ok(n)
            }
            Err(e) if e.kind() == ErrorKind::WouldBlock => Ok(0),
            Err(e) => Err(e),
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
    bg_pixel: u32,
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
            fg: 0x00FF_FFFF,
            bg: 0x0000_0000,
            line_width: 1,
        }
    }
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
    atoms: HashMap<u32, String>,
    atom_names: HashMap<String, u32>,
    next_atom: u32,
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
            atoms,
            atom_names,
            next_atom: 100,
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

    fn send_setup_reply(&mut self, rid_base: u32) {
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
        w16(&mut b[80..82], 1280, self.msb);
        w16(&mut b[82..84], 800, self.msb);
        w16(&mut b[84..86], 338, self.msb);
        w16(&mut b[86..88], 211, self.msb);
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

    fn send_pointer_ev(&mut self, wid: u32, x: i32, y: i32, btn: Option<(u8, bool)>) {
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
        w32(&mut ev[12..16], wid, self.msb);
        w16(&mut ev[20..22], x as i16 as u16, self.msb);
        w16(&mut ev[22..24], y as i16 as u16, self.msb);
        w16(&mut ev[24..26], x as i16 as u16, self.msb);
        w16(&mut ev[26..28], y as i16 as u16, self.msb);
        ev[30] = 1; // same-screen
        self.push_event(ev);
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
        for _ in 0..8 {
            let win = self.windows.get(&wid)?;
            if let Some(gid) = win.gid {
                return Some((gid, ox, oy));
            }
            ox += win.x;
            oy += win.y;
            if win.parent == ROOT_WIN {
                return None;
            }
            wid = win.parent;
        }
        None
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
            next_gid: 1,
            next_cid: 1,
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
            c.send_setup_reply(base);
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
        match op {
            1 if pkt.len() >= 32 => {
                // CreateWindow
                let wid = r32(&pkt[4..8], msb);
                let parent = r32(&pkt[8..12], msb);
                let x = r16(&pkt[12..14], msb) as i16 as i32;
                let y = r16(&pkt[14..16], msb) as i16 as i32;
                let w = (r16(&pkt[16..18], msb) as i32).clamp(64, 1600);
                let h = (r16(&pkt[18..20], msb) as i32).clamp(48, 1000);
                let mask = r32(&pkt[28..32], msb);
                let mut bg = 0x000D_1117u32;
                if (mask & 0x02) != 0 && (mask & 0x01) == 0 && pkt.len() >= 36 {
                    bg = r32(&pkt[32..36], msb);
                }
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
                            pixels: vec![bg & 0x00FF_FFFF; (w * h) as usize],
                            mapped: false,
                            client_id: c.id,
                            proto_obj_id: wid,
                        },
                    );
                    Some(g)
                } else {
                    None
                };
                c.windows.insert(
                    wid,
                    XWin {
                        parent,
                        gid,
                        x,
                        y,
                        w,
                        h,
                        bg_pixel: bg,
                        mapped: false,
                    },
                );
            }
            3 if pkt.len() >= 8 => {
                // GetWindowAttributes
                let wid = r32(&pkt[4..8], msb);
                let mapped = c.windows.get(&wid).is_some_and(|w| w.mapped);
                let mut rep = [0u8; 44];
                rep[0] = 1;
                rep[1] = 2; // BackingStore NotUseful
                w16(&mut rep[2..4], c.seq, msb);
                w32(&mut rep[4..8], 3, msb); // 12 extra bytes
                w32(&mut rep[8..12], ROOT_VISUAL, msb);
                w16(&mut rep[12..14], 1, msb); // InputOutput
                rep[26] = if mapped { 2 } else { 0 };
                w32(&mut rep[28..32], DEF_COLORMAP, msb);
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
            8 if pkt.len() >= 8 => {
                // MapWindow
                let wid = r32(&pkt[4..8], msb);
                let mut dims = None;
                if let Some(win) = c.windows.get_mut(&wid) {
                    win.mapped = true;
                    dims = Some((win.gid, win.w, win.h));
                }
                if let Some((gid_opt, w, h)) = dims {
                    if let Some(gid) = gid_opt
                        && let Some(surf) = self.surfaces.get_mut(&gid)
                        && !surf.mapped
                    {
                        surf.mapped = true;
                        newly_mapped.push(gid);
                    }
                    c.send_expose(wid, w, h);
                }
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
            14 if pkt.len() >= 8 => {
                // GetGeometry
                let did = r32(&pkt[4..8], msb);
                let (x, y, w, h) = if did == ROOT_WIN {
                    (0, 0, 1280, 800)
                } else if let Some(win) = c.windows.get(&did) {
                    (win.x, win.y, win.w, win.h)
                } else if let Some(pm) = c.pixmaps.get(&did) {
                    (0, 0, pm.w, pm.h)
                } else {
                    (0, 0, 320, 240)
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
                c.tx.extend_from_slice(&rep);
            }
            15 if pkt.len() >= 8 => {
                // QueryTree
                let mut rep = [0u8; 32];
                rep[0] = 1;
                w16(&mut rep[2..4], c.seq, msb);
                w32(&mut rep[8..12], ROOT_WIN, msb);
                w32(&mut rep[12..16], ROOT_WIN, msb);
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
                // ChangeProperty (WM_NAME / _NET_WM_NAME updates title)
                let wid = r32(&pkt[4..8], msb);
                let prop = r32(&pkt[8..12], msb);
                let format = pkt[16];
                let dlen = r32(&pkt[20..24], msb) as usize;
                let is_title = matches!(c.atoms.get(&prop).map(|s| s.as_str()), Some("WM_NAME" | "_NET_WM_NAME"));
                if is_title && format == 8 && 24 + dlen <= pkt.len() {
                    let title = String::from_utf8_lossy(&pkt[24..24 + dlen])
                        .trim_matches('\0')
                        .to_string();
                    if !title.is_empty()
                        && let Some((gid, _, _)) = c.find_root_gid(wid)
                        && let Some(surf) = self.surfaces.get_mut(&gid)
                    {
                        surf.title = title;
                    }
                }
            }
            20 => {
                // GetProperty -> None
                let mut rep = [0u8; 32];
                rep[0] = 1;
                w16(&mut rep[2..4], c.seq, msb);
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
                let bg = c.windows.get(&wid).map(|win| win.bg_pixel).unwrap_or(0x000D_1117);
                if let Some((gid, ox, oy)) = c.find_root_gid(wid)
                    && let Some(surf) = self.surfaces.get_mut(&gid)
                {
                    let rw = if w == 0 { surf.w } else { w };
                    let rh = if h == 0 { surf.h } else { h };
                    fill_rect_buf(&mut surf.pixels, (surf.w, surf.h), ox + x, oy + y, rw, rh, bg);
                }
            }
            62 if pkt.len() >= 28 => {
                // CopyArea
                let src_id = r32(&pkt[4..8], msb);
                let dst_id = r32(&pkt[8..12], msb);
                let sx = r16(&pkt[16..18], msb) as i16 as i32;
                let sy = r16(&pkt[18..20], msb) as i16 as i32;
                let dx = r16(&pkt[20..22], msb) as i16 as i32;
                let dy = r16(&pkt[22..24], msb) as i16 as i32;
                let cw = r16(&pkt[24..26], msb) as i32;
                let ch = r16(&pkt[26..28], msb) as i32;
                let copied = if let Some(pm) = c.pixmaps.get(&src_id) {
                    Some((pm.w, pm.h, pm.pixels.clone()))
                } else if let Some((gid, ox, oy)) = c.find_root_gid(src_id)
                    && let Some(surf) = self.surfaces.get(&gid)
                {
                    let _ = (ox, oy);
                    Some((surf.w, surf.h, surf.pixels.clone()))
                } else {
                    None
                };
                if let Some((sw, sh, spx)) = copied
                    && let Some((dgid, dox, doy)) = c.find_root_gid(dst_id)
                    && let Some(dsurf) = self.surfaces.get_mut(&dgid)
                {
                    for row in 0..ch {
                        let syy = sy + row;
                        let dyy = doy + dy + row;
                        if syy < 0 || syy >= sh || dyy < 0 || dyy >= dsurf.h {
                            continue;
                        }
                        for col in 0..cw {
                            let sxx = sx + col;
                            let dxx = dox + dx + col;
                            if sxx >= 0 && sxx < sw && dxx >= 0 && dxx < dsurf.w {
                                dsurf.pixels[(dyy * dsurf.w + dxx) as usize] =
                                    spx[(syy * sw + sxx) as usize];
                            }
                        }
                    }
                }
            }
            65 | 66 | 70 if pkt.len() >= 12 => {
                // PolyLine(65), PolySegment(66), PolyFillRectangle(70)
                let did = r32(&pkt[4..8], msb);
                let gcid = r32(&pkt[8..12], msb);
                let fg = c.gcs.get(&gcid).map(|g| g.fg).unwrap_or(0x00FF_FFFF);
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
                        70 => {
                            for r in data.as_chunks::<8>().0 {
                                let rx = r16(&r[0..2], msb) as i16 as i32;
                                let ry = r16(&r[2..4], msb) as i16 as i32;
                                let rw = r16(&r[4..6], msb) as i32;
                                let rh = r16(&r[6..8], msb) as i32;
                                fill_rect_buf(buf, (tw, th), ox + rx, oy + ry, rw, rh, fg);
                            }
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

                if let Some(buf) = target && iw > 0 && ih > 0 {
                    if detail == 2 && depth >= 24 {
                        // ZPixmap 32bpp
                        let row_bytes = (iw as usize) * 4;
                        for row in 0..ih {
                            let roff = (row as usize) * row_bytes;
                            if roff + row_bytes > img.len() {
                                break;
                            }
                            for col in 0..iw {
                                let po = roff + (col as usize) * 4;
                                let val = r32(&img[po..po + 4], msb) & 0x00FF_FFFF;
                                put_px(buf, tw, th, ox + dx + col, oy + dy + row, val);
                            }
                        }
                    } else if detail == 0 {
                        // XYBitmap 1bpp
                        let stride = (((iw + 31) / 32) * 4) as usize;
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
                                    if set { gc.fg } else { gc.bg },
                                );
                            }
                        }
                    }
                }
            }
            76 if pkt.len() >= 16 => {
                // ImageText8
                let slen = detail as usize;
                let did = r32(&pkt[4..8], msb);
                let gcid = r32(&pkt[8..12], msb);
                let tx = r16(&pkt[12..14], msb) as i16 as i32;
                let ty = r16(&pkt[14..16], msb) as i16 as i32;
                if 16 + slen <= pkt.len() {
                    let s = String::from_utf8_lossy(&pkt[16..16 + slen]).into_owned();
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
            98 => {
                // QueryExtension -> not present
                let mut rep = [0u8; 32];
                rep[0] = 1;
                w16(&mut rep[2..4], c.seq, msb);
                c.tx.extend_from_slice(&rep);
            }
            101 if pkt.len() >= 8 => {
                // GetKeyboardMapping
                let count = pkt[5] as usize;
                let syms_per = 2usize;
                let extra_words = count * syms_per;
                let mut rep = vec![0u8; 32 + extra_words * 4];
                rep[0] = 1;
                rep[1] = syms_per as u8;
                w16(&mut rep[2..4], c.seq, msb);
                w32(&mut rep[4..8], extra_words as u32, msb);
                c.tx.extend_from_slice(&rep);
            }
            119 => {
                // GetModifierMapping
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
                    c.send_pointer_ev(obj, x, y, btn);
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
                let code = ch.map(|c| (c as u32 & 0x7F).max(8) as u8).unwrap_or(36);
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
