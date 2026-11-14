use std::fs::File;
use std::io::{self, BufWriter, Write};
use std::path::Path;

use crate::font::{GLYPH_H, glyph, measure_text};

/// 32-bit XRGB pixel canvas (0x00RRGGBB).
pub struct Canvas {
    pub width: usize,
    pub height: usize,
    pub pixels: Vec<u32>,
}

#[inline]
pub fn lerp_rgb(c1: u32, c2: u32, t: u32, max_t: u32) -> u32 {
    if max_t == 0 {
        return c1;
    }
    let t = t.min(max_t);
    let inv = max_t - t;
    let r = (((c1 >> 16) & 0xFF) * inv + ((c2 >> 16) & 0xFF) * t) / max_t;
    let g = (((c1 >> 8) & 0xFF) * inv + ((c2 >> 8) & 0xFF) * t) / max_t;
    let b = ((c1 & 0xFF) * inv + (c2 & 0xFF) * t) / max_t;
    (r << 16) | (g << 8) | b
}

#[inline]
fn blend_rgb(dst: u32, src: u32, alpha: u8) -> u32 {
    let a = alpha as u32;
    let inv = 255 - a;
    let r = (((dst >> 16) & 0xFF) * inv + ((src >> 16) & 0xFF) * a) / 255;
    let g = (((dst >> 8) & 0xFF) * inv + ((src >> 8) & 0xFF) * a) / 255;
    let b = ((dst & 0xFF) * inv + (src & 0xFF) * a) / 255;
    (r << 16) | (g << 8) | b
}

impl Canvas {
    pub fn new(width: usize, height: usize) -> Self {
        Self {
            width,
            height,
            pixels: vec![0; width.saturating_mul(height)],
        }
    }

    pub fn resize(&mut self, width: usize, height: usize) {
        if self.width == width && self.height == height {
            return;
        }
        self.width = width;
        self.height = height;
        self.pixels.resize(width.saturating_mul(height), 0);
    }

    #[inline]
    pub fn set_pixel(&mut self, x: i32, y: i32, color: u32) {
        if x >= 0 && y >= 0 && (x as usize) < self.width && (y as usize) < self.height {
            self.pixels[(y as usize) * self.width + (x as usize)] = color;
        }
    }

    #[inline]
    pub fn blend_pixel(&mut self, x: i32, y: i32, color: u32, alpha: u8) {
        if x >= 0 && y >= 0 && (x as usize) < self.width && (y as usize) < self.height {
            let idx = (y as usize) * self.width + (x as usize);
            self.pixels[idx] = blend_rgb(self.pixels[idx], color, alpha);
        }
    }

    pub fn fill_rect(&mut self, x: i32, y: i32, w: i32, h: i32, color: u32) {
        if w <= 0 || h <= 0 {
            return;
        }
        let x0 = x.max(0) as usize;
        let y0 = y.max(0) as usize;
        let x1 = (x + w).min(self.width as i32).max(0) as usize;
        let y1 = (y + h).min(self.height as i32).max(0) as usize;
        for py in y0..y1 {
            let row = py * self.width;
            self.pixels[row + x0..row + x1].fill(color);
        }
    }

    pub fn blend_rect(&mut self, x: i32, y: i32, w: i32, h: i32, color: u32, alpha: u8) {
        if w <= 0 || h <= 0 {
            return;
        }
        let x0 = x.max(0) as usize;
        let y0 = y.max(0) as usize;
        let x1 = (x + w).min(self.width as i32).max(0) as usize;
        let y1 = (y + h).min(self.height as i32).max(0) as usize;
        for py in y0..y1 {
            let row = py * self.width;
            for px in x0..x1 {
                self.pixels[row + px] = blend_rgb(self.pixels[row + px], color, alpha);
            }
        }
    }

    pub fn draw_rect(&mut self, x: i32, y: i32, w: i32, h: i32, color: u32) {
        if w <= 0 || h <= 0 {
            return;
        }
        self.fill_rect(x, y, w, 1, color);
        self.fill_rect(x, y + h - 1, w, 1, color);
        self.fill_rect(x, y, 1, h, color);
        self.fill_rect(x + w - 1, y, 1, h, color);
    }

    #[inline]
    fn corner_cov(ox: i32, oy: i32, r: i32) -> u8 {
        // 2x-subpixel distance approximation for smooth anti-aliased rounded corners.
        let fx = 2 * ox + 1;
        let fy = 2 * oy + 1;
        let d2 = fx * fx + fy * fy;
        let outer = 4 * r * r;
        if d2 >= outer {
            return 0;
        }
        let inner = (2 * r - 2).max(0);
        let inner2 = inner * inner;
        if d2 <= inner2 {
            255
        } else {
            (((outer - d2) * 255) / (outer - inner2).max(1)) as u8
        }
    }

    pub fn fill_rounded_rect(&mut self, x: i32, y: i32, w: i32, h: i32, r: i32, color: u32) {
        if w <= 0 || h <= 0 {
            return;
        }
        let r = r.min(w / 2).min(h / 2).max(0);
        if r == 0 {
            self.fill_rect(x, y, w, h, color);
            return;
        }
        self.fill_rect(x + r, y, w - 2 * r, h, color);
        self.fill_rect(x, y + r, r, h - 2 * r, color);
        self.fill_rect(x + w - r, y + r, r, h - 2 * r, color);

        for dy in 0..r {
            for dx in 0..r {
                let cov = Self::corner_cov(r - 1 - dx, r - 1 - dy, r);
                if cov == 255 {
                    self.set_pixel(x + dx, y + dy, color);
                    self.set_pixel(x + w - 1 - dx, y + dy, color);
                    self.set_pixel(x + dx, y + h - 1 - dy, color);
                    self.set_pixel(x + w - 1 - dx, y + h - 1 - dy, color);
                } else if cov > 0 {
                    self.blend_pixel(x + dx, y + dy, color, cov);
                    self.blend_pixel(x + w - 1 - dx, y + dy, color, cov);
                    self.blend_pixel(x + dx, y + h - 1 - dy, color, cov);
                    self.blend_pixel(x + w - 1 - dx, y + h - 1 - dy, color, cov);
                }
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn fill_vgradient_rounded_rect(
        &mut self,
        x: i32,
        y: i32,
        w: i32,
        h: i32,
        r: i32,
        top_col: u32,
        bot_col: u32,
    ) {
        if w <= 0 || h <= 0 {
            return;
        }
        let r = r.min(w / 2).min(h / 2).max(0);
        let max_t = (h - 1).max(1) as u32;
        for dy in 0..h {
            let col = lerp_rgb(top_col, bot_col, dy as u32, max_t);
            if dy >= r && dy < h - r {
                self.fill_rect(x, y + dy, w, 1, col);
            } else {
                let cy = if dy < r { r - 1 - dy } else { dy - (h - r) };
                self.fill_rect(x + r, y + dy, w - 2 * r, 1, col);
                for dx in 0..r {
                    let cov = Self::corner_cov(r - 1 - dx, cy, r);
                    if cov > 0 {
                        self.blend_pixel(x + dx, y + dy, col, cov);
                        self.blend_pixel(x + w - 1 - dx, y + dy, col, cov);
                    }
                }
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn fill_hgradient_rounded_rect(
        &mut self,
        x: i32,
        y: i32,
        w: i32,
        h: i32,
        r: i32,
        left_col: u32,
        right_col: u32,
    ) {
        if w <= 0 || h <= 0 {
            return;
        }
        let r = r.min(w / 2).min(h / 2).max(0);
        let max_t = (w - 1).max(1) as u32;
        for dx in 0..w {
            let col = lerp_rgb(left_col, right_col, dx as u32, max_t);
            if dx >= r && dx < w - r {
                self.fill_rect(x + dx, y, 1, h, col);
            } else {
                let cx = if dx < r { r - 1 - dx } else { dx - (w - r) };
                self.fill_rect(x + dx, y + r, 1, h - 2 * r, col);
                for dy in 0..r {
                    let cov = Self::corner_cov(cx, r - 1 - dy, r);
                    if cov > 0 {
                        self.blend_pixel(x + dx, y + dy, col, cov);
                        self.blend_pixel(x + dx, y + h - 1 - dy, col, cov);
                    }
                }
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn blend_rounded_rect(
        &mut self,
        x: i32,
        y: i32,
        w: i32,
        h: i32,
        r: i32,
        color: u32,
        alpha: u8,
    ) {
        if w <= 0 || h <= 0 {
            return;
        }
        let r = r.min(w / 2).min(h / 2).max(0);
        self.blend_rect(x + r, y, w - 2 * r, h, color, alpha);
        self.blend_rect(x, y + r, r, h - 2 * r, color, alpha);
        self.blend_rect(x + w - r, y + r, r, h - 2 * r, color, alpha);

        for dy in 0..r {
            for dx in 0..r {
                let cov = Self::corner_cov(r - 1 - dx, r - 1 - dy, r);
                if cov > 0 {
                    let a = ((alpha as u32 * cov as u32) / 255) as u8;
                    if a > 0 {
                        self.blend_pixel(x + dx, y + dy, color, a);
                        self.blend_pixel(x + w - 1 - dx, y + dy, color, a);
                        self.blend_pixel(x + dx, y + h - 1 - dy, color, a);
                        self.blend_pixel(x + w - 1 - dx, y + h - 1 - dy, color, a);
                    }
                }
            }
        }
    }

    pub fn draw_rounded_rect(&mut self, x: i32, y: i32, w: i32, h: i32, r: i32, color: u32) {
        if w <= 1 || h <= 1 {
            return;
        }
        let r = r.min(w / 2).min(h / 2).max(0);
        if r == 0 {
            self.draw_rect(x, y, w, h, color);
            return;
        }
        self.fill_rect(x + r, y, w - 2 * r, 1, color);
        self.fill_rect(x + r, y + h - 1, w - 2 * r, 1, color);
        self.fill_rect(x, y + r, 1, h - 2 * r, color);
        self.fill_rect(x + w - 1, y + r, 1, h - 2 * r, color);

        let r2 = r * r;
        let inner_r2 = (r - 1) * (r - 1);
        for dy in 0..r {
            for dx in 0..r {
                let ox = r - 1 - dx;
                let oy = r - 1 - dy;
                let d2 = ox * ox + oy * oy;
                if d2 <= r2 && d2 >= inner_r2 {
                    self.set_pixel(x + dx, y + dy, color);
                    self.set_pixel(x + w - 1 - dx, y + dy, color);
                    self.set_pixel(x + dx, y + h - 1 - dy, color);
                    self.set_pixel(x + w - 1 - dx, y + h - 1 - dy, color);
                }
            }
        }
    }

    pub fn draw_soft_shadow(&mut self, x: i32, y: i32, w: i32, h: i32, r: i32) {
        self.blend_rounded_rect(x - 2, y + 2, w + 4, h + 6, r + 4, 0x000000, 32);
        self.blend_rounded_rect(x + 1, y + 4, w + 2, h + 3, r + 2, 0x000000, 56);
        self.blend_rounded_rect(x + 3, y + 6, w - 2, h, r, 0x000000, 82);
    }

    pub fn draw_char(&mut self, x: i32, y: i32, ch: char, color: u32, scale: i32) -> i32 {
        let g = glyph(ch);
        let s = scale.max(1);
        for (row, &mask) in g.rows.iter().enumerate() {
            for col in 0..(g.width as usize) {
                if (mask & (0x80 >> col)) != 0 {
                    let px = x + (col as i32) * s;
                    let py = y + (row as i32) * s;
                    if s == 1 {
                        self.set_pixel(px, py, color);
                    } else {
                        self.fill_rect(px, py, s, s, color);
                    }
                }
            }
        }
        (g.width as i32) * s
    }

    pub fn draw_text(&mut self, x: i32, y: i32, text: &str, color: u32, scale: i32) {
        let mut cx = x;
        for ch in text.chars() {
            if cx >= self.width as i32 {
                break;
            }
            cx += self.draw_char(cx, y, ch, color, scale);
        }
    }

    pub fn draw_text_clipped(
        &mut self,
        x: i32,
        y: i32,
        max_w: i32,
        text: &str,
        color: u32,
        scale: i32,
    ) {
        if max_w <= 0 {
            return;
        }
        let s = scale.max(1);
        let limit = x + max_w;
        let mut cx = x;
        for ch in text.chars() {
            let adv = (glyph(ch).width as i32) * s;
            if cx + adv > limit {
                break;
            }
            cx += self.draw_char(cx, y, ch, color, s);
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn draw_progress_bar(
        &mut self,
        x: i32,
        y: i32,
        w: i32,
        h: i32,
        percent: u8,
        fill_color: u32,
        bg_color: u32,
        border_color: u32,
    ) {
        let r = (h / 2).min(6);
        self.fill_rounded_rect(x, y, w, h, r, bg_color);
        let pct = percent.min(100) as i32;
        let inner_w = (w - 2).max(0);
        let fill_w = (inner_w * pct) / 100;
        if fill_w > 2 {
            let hi = lerp_rgb(fill_color, 0xFFFFFF, 75, 255);
            let fr = r.min(fill_w / 2);
            self.fill_hgradient_rounded_rect(x + 1, y + 1, fill_w, (h - 2).max(0), fr, fill_color, hi);
            self.blend_rect(x + 2, y + 2, (fill_w - 2).max(0), ((h - 4) / 2).max(1), 0xFFFFFF, 42);
        }
        self.draw_rounded_rect(x, y, w, h, r, border_color);
    }

    #[allow(clippy::too_many_arguments)]
    pub fn draw_button(
        &mut self,
        x: i32,
        y: i32,
        w: i32,
        h: i32,
        label: &str,
        bg_color: u32,
        fg_color: u32,
        border_color: u32,
    ) {
        let r = 6.min(w / 2).min(h / 2);
        let top_col = lerp_rgb(bg_color, 0xFFFFFF, 26, 255);
        let bot_col = lerp_rgb(bg_color, 0x000000, 18, 255);
        self.fill_vgradient_rounded_rect(x, y, w, h, r, top_col, bot_col);
        if w > 2 * r + 2 {
            self.blend_rect(x + r, y + 1, w - 2 * r, 1, 0xFFFFFF, 38);
        }
        self.draw_rounded_rect(x, y, w, h, r, border_color);
        let tw = measure_text(label, 1);
        let tx = x + ((w - tw) / 2).max(4);
        let ty = y + ((h - (GLYPH_H as i32)) / 2).max(1);
        self.draw_text_clipped(tx, ty, w - 8, label, fg_color, 1);
    }

    pub fn draw_toggle(&mut self, x: i32, y: i32, on: bool, accent: u32) {
        let track = if on { accent } else { 0x30363D };
        let track_bot = lerp_rgb(track, 0x000000, 32, 255);
        self.fill_vgradient_rounded_rect(x, y, 36, 18, 9, track_bot, track);
        self.draw_rounded_rect(x, y, 36, 18, 9, lerp_rgb(track, 0xFFFFFF, 45, 255));
        let knob_x = if on { x + 20 } else { x + 2 };
        self.blend_rounded_rect(knob_x, y + 3, 14, 14, 7, 0x000000, 75);
        self.fill_vgradient_rounded_rect(knob_x, y + 2, 14, 14, 7, 0xFFFFFF, 0xD8DEE9);
    }

    pub fn draw_cursor(&mut self, mx: i32, my: i32) {
        const CURSOR: [&str; 14] = [
            "X...........",
            "XX..........",
            "XWX.........",
            "XWWX........",
            "XWWWX.......",
            "XWWWWX......",
            "XWWWWWX.....",
            "XWWWWWWX....",
            "XWWWWWWWX...",
            "XWWWWWXXXX..",
            "XWWXWWX.....",
            "XWX.XWWX....",
            "XX...XWX....",
            "X.....X.....",
        ];
        // Soft drop shadow beneath pointer
        for (dy, row) in CURSOR.iter().enumerate() {
            for (dx, ch) in row.bytes().enumerate() {
                if ch != b'.' {
                    self.blend_pixel(mx + dx as i32 + 2, my + dy as i32 + 2, 0x000000, 85);
                }
            }
        }
        for (dy, row) in CURSOR.iter().enumerate() {
            for (dx, ch) in row.bytes().enumerate() {
                match ch {
                    b'X' => self.set_pixel(mx + dx as i32, my + dy as i32, 0x0B0F17),
                    b'W' => {
                        let col = if dx < 4 { 0xFFFFFF } else { 0xE2E8F0 };
                        self.set_pixel(mx + dx as i32, my + dy as i32, col);
                    }
                    _ => {}
                }
            }
        }
    }

    /// Writes a standard uncompressed 24-bit BMP image for offline inspection.
    pub fn write_bmp(&self, path: &Path) -> io::Result<()> {
        let w = self.width as u32;
        let h = self.height as u32;
        let row_bytes = (w * 3) as usize;
        let pad = (4 - (row_bytes % 4)) % 4;
        let data_size = (row_bytes + pad) as u32 * h;
        let file_size = 54 + data_size;

        let mut out = BufWriter::new(File::create(path)?);
        out.write_all(b"BM")?;
        out.write_all(&file_size.to_le_bytes())?;
        out.write_all(&0u32.to_le_bytes())?;
        out.write_all(&54u32.to_le_bytes())?;

        out.write_all(&40u32.to_le_bytes())?;
        out.write_all(&w.to_le_bytes())?;
        out.write_all(&h.to_le_bytes())?;
        out.write_all(&1u16.to_le_bytes())?;
        out.write_all(&24u16.to_le_bytes())?;
        out.write_all(&0u32.to_le_bytes())?;
        out.write_all(&data_size.to_le_bytes())?;
        out.write_all(&2835u32.to_le_bytes())?;
        out.write_all(&2835u32.to_le_bytes())?;
        out.write_all(&0u32.to_le_bytes())?;
        out.write_all(&0u32.to_le_bytes())?;

        let pad_bytes = [0u8; 3];
        for y in (0..self.height).rev() {
            for x in 0..self.width {
                let px = self.pixels[y * self.width + x];
                let b = (px & 0xFF) as u8;
                let g = ((px >> 8) & 0xFF) as u8;
                let r = ((px >> 16) & 0xFF) as u8;
                out.write_all(&[b, g, r])?;
            }
            if pad > 0 {
                out.write_all(&pad_bytes[..pad])?;
            }
        }
        out.flush()?;
        Ok(())
    }
}
