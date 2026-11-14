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
        if r <= 0 {
            return 255;
        }
        let dist = ((ox as f32 * ox as f32 + oy as f32 * oy as f32).sqrt()) - (r as f32 - 0.5);
        ((0.5 - dist).clamp(0.0, 1.0) * 255.0) as u8
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
    pub fn fill_vgradient_top_rounded_rect(
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
        let r = r.min(w / 2).min(h).max(0);
        let max_t = (h - 1).max(1) as u32;
        for dy in 0..h {
            let col = lerp_rgb(top_col, bot_col, dy as u32, max_t);
            if dy >= r {
                self.fill_rect(x, y + dy, w, 1, col);
            } else {
                let cy = r - 1 - dy;
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

    #[allow(clippy::too_many_arguments)]
    pub fn draw_rounded_rect_alpha(
        &mut self,
        x: i32,
        y: i32,
        w: i32,
        h: i32,
        r: i32,
        color: u32,
        alpha: u8,
    ) {
        if w <= 1 || h <= 1 || alpha == 0 {
            return;
        }
        let r = r.min(w / 2).min(h / 2).max(0);
        if r == 0 {
            self.blend_rect(x, y, w, 1, color, alpha);
            self.blend_rect(x, y + h - 1, w, 1, color, alpha);
            self.blend_rect(x, y + 1, 1, h - 2, color, alpha);
            self.blend_rect(x + w - 1, y + 1, 1, h - 2, color, alpha);
            return;
        }
        self.blend_rect(x + r, y, w - 2 * r, 1, color, alpha);
        self.blend_rect(x + r, y + h - 1, w - 2 * r, 1, color, alpha);
        self.blend_rect(x, y + r, 1, h - 2 * r, color, alpha);
        self.blend_rect(x + w - 1, y + r, 1, h - 2 * r, color, alpha);

        let rf = r as f32 - 0.5;
        for dy in 0..r {
            let oy = rf - dy as f32;
            for dx in 0..r {
                let ox = rf - dx as f32;
                let dist = ((ox * ox + oy * oy).sqrt() - rf).abs();
                let cov = (1.0 - dist).clamp(0.0, 1.0);
                if cov > 0.01 {
                    let a = (alpha as f32 * cov) as u8;
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
        self.draw_rounded_rect_alpha(x, y, w, h, r, color, 255);
    }

    pub fn fill_aa_circle(&mut self, cx: f32, cy: f32, r: f32, color: u32, alpha: u8) {
        if r <= 0.0 || alpha == 0 {
            return;
        }
        let x0 = (cx - r - 1.0).floor() as i32;
        let x1 = (cx + r + 1.0).ceil() as i32;
        let y0 = (cy - r - 1.0).floor() as i32;
        let y1 = (cy + r + 1.0).ceil() as i32;
        for py in y0..=y1 {
            let dy = py as f32 + 0.5 - cy;
            for px in x0..=x1 {
                let dx = px as f32 + 0.5 - cx;
                let dist = (dx * dx + dy * dy).sqrt() - r;
                let cov = (0.5 - dist).clamp(0.0, 1.0);
                if cov > 0.01 {
                    let a = (alpha as f32 * cov) as u8;
                    if a > 0 {
                        self.blend_pixel(px, py, color, a);
                    }
                }
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn draw_aa_line(
        &mut self,
        x0: f32,
        y0: f32,
        x1: f32,
        y1: f32,
        thickness: f32,
        color: u32,
        alpha: u8,
    ) {
        let half_t = (thickness * 0.5).max(0.4);
        let min_x = (x0.min(x1) - half_t - 1.0).floor() as i32;
        let max_x = (x0.max(x1) + half_t + 1.0).ceil() as i32;
        let min_y = (y0.min(y1) - half_t - 1.0).floor() as i32;
        let max_y = (y0.max(y1) + half_t + 1.0).ceil() as i32;
        let vx = x1 - x0;
        let vy = y1 - y0;
        let len2 = (vx * vx + vy * vy).max(0.0001);

        for py in min_y..=max_y {
            let fy = py as f32 + 0.5 - y0;
            for px in min_x..=max_x {
                let fx = px as f32 + 0.5 - x0;
                let t = ((fx * vx + fy * vy) / len2).clamp(0.0, 1.0);
                let dx = fx - vx * t;
                let dy = fy - vy * t;
                let dist = (dx * dx + dy * dy).sqrt() - half_t;
                let cov = (0.5 - dist).clamp(0.0, 1.0);
                if cov > 0.01 {
                    let a = (alpha as f32 * cov) as u8;
                    if a > 0 {
                        self.blend_pixel(px, py, color, a);
                    }
                }
            }
        }
    }

    /// 2001-style Aqua translucent specular sheen on top half of a rounded element.
    pub fn draw_aqua_sheen(
        &mut self,
        x: i32,
        y: i32,
        w: i32,
        h: i32,
        r: i32,
        peak_alpha: u8,
    ) {
        let gloss_h = (h / 2).max(2);
        let r = r.min(w / 2).min(gloss_h).max(0);
        for dy in 1..gloss_h {
            let k = 1.0 - (dy as f32 / gloss_h as f32);
            let a = (peak_alpha as f32 * k * 0.85) as u8;
            if a == 0 {
                continue;
            }
            if dy >= r {
                self.blend_rect(x + 1, y + dy, (w - 2).max(0), 1, 0xFFFFFF, a);
            } else {
                self.blend_rect(x + r, y + dy, (w - 2 * r).max(0), 1, 0xFFFFFF, a);
                for dx in 1..r {
                    let cov = Self::corner_cov(r - 1 - dx, r - 1 - dy, r);
                    if cov > 0 {
                        let ca = ((a as u32 * cov as u32) / 255) as u8;
                        if ca > 0 {
                            self.blend_pixel(x + dx, y + dy, 0xFFFFFF, ca);
                            self.blend_pixel(x + w - 1 - dx, y + dy, 0xFFFFFF, ca);
                        }
                    }
                }
            }
        }
    }

    pub fn fill_glossy_orb(&mut self, cx: i32, cy: i32, r: i32, base_col: u32) {
        let cxf = cx as f32 + 0.5;
        let cyf = cy as f32 + 0.5;
        let rf = r as f32;
        let dark = lerp_rgb(base_col, 0x000000, 120, 255);
        let bright = lerp_rgb(base_col, 0xFFFFFF, 140, 255);
        self.fill_aa_circle(cxf, cyf + 1.0, rf + 0.4, 0x000000, 90);
        self.fill_aa_circle(cxf, cyf, rf, dark, 255);
        self.fill_aa_circle(cxf, cyf + 0.4, (rf - 0.8).max(1.0), base_col, 255);
        self.fill_aa_circle(cxf - rf * 0.18, cyf + rf * 0.25, (rf * 0.65).max(1.0), bright, 70);
        // Aqua top specular drop
        self.fill_aa_circle(cxf - rf * 0.18, cyf - rf * 0.34, (rf * 0.38).max(1.0), 0xFFFFFF, 195);
    }

    pub fn draw_soft_shadow(&mut self, x: i32, y: i32, w: i32, h: i32, r: i32) {
        self.blend_rounded_rect(x - 5, y + 1, w + 10, h + 11, r + 6, 0x000000, 16);
        self.blend_rounded_rect(x - 2, y + 3, w + 5, h + 7, r + 4, 0x000000, 26);
        self.blend_rounded_rect(x, y + 4, w + 2, h + 4, r + 2, 0x000000, 38);
        self.blend_rounded_rect(x + 1, y + 5, w, h + 2, r + 1, 0x000000, 52);
    }

    pub fn draw_char(&mut self, x: i32, y: i32, ch: char, color: u32, scale: i32) -> i32 {
        let g = glyph(ch);
        let s = scale.max(1);
        if s == 1 {
            for (row, &mask) in g.rows.iter().enumerate() {
                for col in 0..(g.width as usize) {
                    if (mask & (0x80 >> col)) != 0 {
                        self.set_pixel(x + col as i32, y + row as i32, color);
                    }
                }
            }
        } else {
            let sample = |ix: i32, iy: i32| -> f32 {
                if ix < 0 || iy < 0 || ix >= g.width as i32 || iy >= GLYPH_H as i32 {
                    return 0.0;
                }
                let m = g.rows[iy as usize];
                if (m & (0x80 >> (ix as usize))) != 0 {
                    1.0
                } else {
                    0.0
                }
            };
            let out_w = (g.width as i32) * s;
            let out_h = (GLYPH_H as i32) * s;
            for oy in 0..out_h {
                let fy = (oy as f32 + 0.5) / (s as f32) - 0.5;
                let y0 = fy.floor() as i32;
                let wy = fy - y0 as f32;
                for ox in 0..out_w {
                    let fx = (ox as f32 + 0.5) / (s as f32) - 0.5;
                    let x0 = fx.floor() as i32;
                    let wx = fx - x0 as f32;
                    let v00 = sample(x0, y0);
                    let v10 = sample(x0 + 1, y0);
                    let v01 = sample(x0, y0 + 1);
                    let v11 = sample(x0 + 1, y0 + 1);
                    let top = v00 * (1.0 - wx) + v10 * wx;
                    let bot = v01 * (1.0 - wx) + v11 * wx;
                    let v = top * (1.0 - wy) + bot * wy;
                    let cov = ((v - 0.30) * 3.15).clamp(0.0, 1.0);
                    if cov > 0.02 {
                        self.blend_pixel(x + ox, y + oy, color, (cov * 255.0) as u8);
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
        let r = (h / 2).min(7);
        self.fill_vgradient_rounded_rect(x, y, w, h, r, lerp_rgb(bg_color, 0x000000, 45, 255), bg_color);
        let pct = percent.min(100) as i32;
        let inner_w = (w - 2).max(0);
        let fill_w = (inner_w * pct) / 100;
        if fill_w > 2 {
            let hi = lerp_rgb(fill_color, 0xFFFFFF, 95, 255);
            let fr = r.min(fill_w / 2);
            self.fill_hgradient_rounded_rect(x + 1, y + 1, fill_w, (h - 2).max(0), fr, fill_color, hi);
            self.draw_aqua_sheen(x + 1, y, fill_w, (h - 2).max(2), fr, 85);
        }
        self.draw_rounded_rect_alpha(x, y, w, h, r, border_color, 130);
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
        let top_col = lerp_rgb(bg_color, 0xFFFFFF, 34, 255);
        let bot_col = lerp_rgb(bg_color, 0x000000, 34, 255);
        self.fill_vgradient_rounded_rect(x, y, w, h, r, top_col, bot_col);
        self.draw_aqua_sheen(x, y, w, h, r, 46);
        self.draw_rounded_rect_alpha(x, y, w, h, r, border_color, 150);
        let tw = measure_text(label, 1);
        let tx = x + ((w - tw) / 2).max(4);
        let ty = y + ((h - (GLYPH_H as i32)) / 2).max(1);
        self.draw_text_clipped(tx, ty + 1, w - 8, label, 0x090C10, 1);
        self.draw_text_clipped(tx, ty, w - 8, label, fg_color, 1);
    }

    pub fn draw_toggle(&mut self, x: i32, y: i32, on: bool, accent: u32) {
        let track = if on { accent } else { 0x262D38 };
        let track_top = lerp_rgb(track, 0x000000, 48, 255);
        self.fill_vgradient_rounded_rect(x, y, 36, 18, 9, track_top, track);
        self.draw_rounded_rect_alpha(x, y, 36, 18, 9, lerp_rgb(track, 0xFFFFFF, 55, 255), 140);
        let knob_cx = if on { x + 26 } else { x + 9 };
        self.fill_glossy_orb(knob_cx, y + 9, 6, 0xE2E8F0);
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
