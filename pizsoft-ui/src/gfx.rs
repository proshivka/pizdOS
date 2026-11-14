use std::fs::File;
use std::io::{self, BufWriter, Write};
use std::path::Path;

use crate::font::{GLYPH_H, GLYPH_W, glyph};

/// 32-bit XRGB pixel canvas (0x00RRGGBB).
pub struct Canvas {
    pub width: usize,
    pub height: usize,
    pub pixels: Vec<u32>,
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

    pub fn draw_rect(&mut self, x: i32, y: i32, w: i32, h: i32, color: u32) {
        if w <= 0 || h <= 0 {
            return;
        }
        self.fill_rect(x, y, w, 1, color);
        self.fill_rect(x, y + h - 1, w, 1, color);
        self.fill_rect(x, y, 1, h, color);
        self.fill_rect(x + w - 1, y, 1, h, color);
    }

    pub fn draw_char(&mut self, x: i32, y: i32, ch: char, color: u32, scale: i32) {
        let bits = glyph(ch);
        let s = scale.max(1);
        for (row, &mask) in bits.iter().enumerate() {
            for col in 0..GLYPH_W {
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
    }

    pub fn draw_text(&mut self, x: i32, y: i32, text: &str, color: u32, scale: i32) {
        let step = (GLYPH_W as i32) * scale.max(1);
        for (i, ch) in text.chars().enumerate() {
            let cx = x + (i as i32) * step;
            if cx >= self.width as i32 {
                break;
            }
            self.draw_char(cx, y, ch, color, scale);
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
        let step = (GLYPH_W as i32) * scale.max(1);
        let max_chars = (max_w / step).max(0) as usize;
        for (i, ch) in text.chars().take(max_chars).enumerate() {
            self.draw_char(x + (i as i32) * step, y, ch, color, scale);
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
        self.fill_rect(x, y, w, h, bg_color);
        let pct = percent.min(100) as i32;
        let inner_w = (w - 2).max(0);
        let fill_w = (inner_w * pct) / 100;
        self.fill_rect(x + 1, y + 1, fill_w, (h - 2).max(0), fill_color);
        self.draw_rect(x, y, w, h, border_color);
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
        self.fill_rect(x, y, w, h, bg_color);
        self.draw_rect(x, y, w, h, border_color);
        let text_w = (label.chars().count() as i32) * (GLYPH_W as i32);
        let tx = x + ((w - text_w) / 2).max(4);
        let ty = y + ((h - (GLYPH_H as i32)) / 2).max(2);
        self.draw_text_clipped(tx, ty, w - 4, label, fg_color, 1);
    }

    pub fn draw_cursor(&mut self, mx: i32, my: i32) {
        const CURSOR: [&str; 12] = [
            "X.........",
            "XX........",
            "XWX.......",
            "XWWX......",
            "XWWWX.....",
            "XWWWWX....",
            "XWWWWWX...",
            "XWWWWWWX..",
            "XWWWXXXX..",
            "XWX.X.....",
            "XX...X....",
            "X....X....",
        ];
        for (dy, row) in CURSOR.iter().enumerate() {
            for (dx, ch) in row.bytes().enumerate() {
                match ch {
                    b'X' => self.set_pixel(mx + dx as i32, my + dy as i32, 0x000000),
                    b'W' => self.set_pixel(mx + dx as i32, my + dy as i32, 0xFFFFFF),
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
