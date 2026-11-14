use std::fs::File;
use std::io::{self, BufWriter, Write};
use std::path::Path;

use crate::font::{GLYPH_H, RasterGlyph, measure_text, rasterize_cached};

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
pub fn blend_rgb(dst: u32, src: u32, alpha: u8) -> u32 {
    if alpha == 0 {
        return dst;
    }
    if alpha == 255 {
        return src;
    }
    let a = alpha as u32;
    let inv = 255 - a;
    let d_rb = dst & 0x00FF_00FF;
    let s_rb = src & 0x00FF_00FF;
    let rb = d_rb * inv + s_rb * a + 0x0080_0080;
    let rb = ((rb + ((rb >> 8) & 0x00FF_00FF)) >> 8) & 0x00FF_00FF;

    let d_g = (dst >> 8) & 0xFF;
    let s_g = (src >> 8) & 0xFF;
    let g = d_g * inv + s_g * a + 128;
    let g = (g + (g >> 8)) >> 8;
    rb | (g << 8)
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

    /// Copy an opaque packed RGB pixel buffer (`w x h`) onto the canvas with clipping.
    pub fn blit_rgb_rect(&mut self, dx: i32, dy: i32, w: i32, h: i32, src: &[u32]) {
        if w <= 0 || h <= 0 || src.len() < (w as usize) * (h as usize) {
            return;
        }
        let x0 = dx.max(0);
        let y0 = dy.max(0);
        let x1 = (dx + w).min(self.width as i32);
        let y1 = (dy + h).min(self.height as i32);
        if x0 >= x1 || y0 >= y1 {
            return;
        }
        let copy_w = (x1 - x0) as usize;
        let sx0 = (x0 - dx) as usize;
        for py in y0..y1 {
            let sy = (py - dy) as usize;
            let dst_off = (py as usize) * self.width + (x0 as usize);
            let src_off = sy * (w as usize) + sx0;
            self.pixels[dst_off..dst_off + copy_w]
                .copy_from_slice(&src[src_off..src_off + copy_w]);
        }
    }

    pub fn blend_rect(&mut self, x: i32, y: i32, w: i32, h: i32, color: u32, alpha: u8) {
        if w <= 0 || h <= 0 || alpha == 0 {
            return;
        }
        if alpha == 255 {
            self.fill_rect(x, y, w, h, color);
            return;
        }
        let x0 = x.max(0) as usize;
        let y0 = y.max(0) as usize;
        let x1 = (x + w).min(self.width as i32).max(0) as usize;
        let y1 = (y + h).min(self.height as i32).max(0) as usize;
        let a = alpha as u32;
        let inv = 255 - a;
        let s_rb_a = (color & 0x00FF_00FF) * a + 0x0080_0080;
        let s_g_a = ((color >> 8) & 0xFF) * a + 128;
        for py in y0..y1 {
            let row = py * self.width;
            for px in &mut self.pixels[row + x0..row + x1] {
                let dst = *px;
                let rb = (dst & 0x00FF_00FF) * inv + s_rb_a;
                let rb = ((rb + ((rb >> 8) & 0x00FF_00FF)) >> 8) & 0x00FF_00FF;
                let g = ((dst >> 8) & 0xFF) * inv + s_g_a;
                let g = (g + (g >> 8)) >> 8;
                *px = rb | (g << 8);
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
    pub fn fill_luminous_card(
        &mut self,
        x: i32,
        y: i32,
        w: i32,
        h: i32,
        r: i32,
        tl_col: u32,
        br_col: u32,
        glow_a: u32,
        glow_b: u32,
    ) {
        if w <= 0 || h <= 0 {
            return;
        }
        let r = r.min(w / 2).min(h / 2).max(0);
        let split = |c: u32| -> (f32, f32, f32) {
            (((c >> 16) & 0xFF) as f32, ((c >> 8) & 0xFF) as f32, (c & 0xFF) as f32)
        };
        let (tl_r, tl_g, tl_b) = split(tl_col);
        let (br_r, br_g, br_b) = split(br_col);
        let (ar, ag, ab) = split(glow_a);
        let (gr2, gg2, gb2) = split(glow_b);

        let wf = (w - 1).max(1) as f32;
        let hf = (h - 1).max(1) as f32;

        for dy in 0..h {
            let ny = dy as f32 / hf;
            for dx in 0..w {
                let cov = if dx < r && dy < r {
                    Self::corner_cov(r - 1 - dx, r - 1 - dy, r)
                } else if dx >= w - r && dy < r {
                    Self::corner_cov(dx - (w - r), r - 1 - dy, r)
                } else if dx < r && dy >= h - r {
                    Self::corner_cov(r - 1 - dx, dy - (h - r), r)
                } else if dx >= w - r && dy >= h - r {
                    Self::corner_cov(dx - (w - r), dy - (h - r), r)
                } else {
                    255
                };
                if cov == 0 {
                    continue;
                }

                let nx = dx as f32 / wf;
                let diag = (nx * 0.65 + ny * 0.55).clamp(0.0, 1.0);
                let mut cr = tl_r + (br_r - tl_r) * diag;
                let mut cg = tl_g + (br_g - tl_g) * diag;
                let mut cb = tl_b + (br_b - tl_b) * diag;

                let d1 = ((nx - 0.78) * 1.4).powi(2) + ((ny - 0.25) * 1.5).powi(2);
                if d1 < 1.0 {
                    let k = 1.0 - d1;
                    let w1 = k * k * k * 0.34;
                    cr += (ar - cr) * w1;
                    cg += (ag - cg) * w1;
                    cb += (ab - cb) * w1;
                }

                let d2 = ((nx - 0.28) * 1.35).powi(2) + ((ny - 0.85) * 1.5).powi(2);
                if d2 < 1.0 {
                    let k = 1.0 - d2;
                    let w2 = k * k * k * 0.26;
                    cr += (gr2 - cr) * w2;
                    cg += (gg2 - cg) * w2;
                    cb += (gb2 - cb) * w2;
                }

                // Subtle Liquid Glass top-edge specular sheen
                if dy == 1 {
                    cr += (255.0 - cr) * 0.14;
                    cg += (255.0 - cg) * 0.14;
                    cb += (255.0 - cb) * 0.14;
                }

                let col = ((cr.clamp(0.0, 255.0) as u32) << 16)
                    | ((cg.clamp(0.0, 255.0) as u32) << 8)
                    | (cb.clamp(0.0, 255.0) as u32);
                if cov == 255 {
                    self.set_pixel(x + dx, y + dy, col);
                } else {
                    self.blend_pixel(x + dx, y + dy, col, cov);
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
        let x0 = ((cx - r - 1.0).floor() as i32).max(0);
        let x1 = ((cx + r + 1.0).ceil() as i32).min(self.width as i32 - 1);
        let y0 = ((cy - r - 1.0).floor() as i32).max(0);
        let y1 = ((cy + r + 1.0).ceil() as i32).min(self.height as i32 - 1);
        if x0 > x1 || y0 > y1 {
            return;
        }
        let r_in = (r - 0.7).max(0.0);
        let r_in2 = r_in * r_in;
        let r_out = r + 0.7;
        let r_out2 = r_out * r_out;

        for py in y0..=y1 {
            let dy = py as f32 + 0.5 - cy;
            let dy2 = dy * dy;
            let row = (py as usize) * self.width;
            for px in x0..=x1 {
                let dx = px as f32 + 0.5 - cx;
                let d2 = dx * dx + dy2;
                if d2 >= r_out2 {
                    continue;
                }
                if d2 <= r_in2 {
                    let idx = row + px as usize;
                    self.pixels[idx] = blend_rgb(self.pixels[idx], color, alpha);
                    continue;
                }
                let dist = d2.sqrt() - r;
                let cov = (0.5 - dist).clamp(0.0, 1.0);
                let a = (alpha as f32 * cov) as u8;
                if a > 0 {
                    let idx = row + px as usize;
                    self.pixels[idx] = blend_rgb(self.pixels[idx], color, a);
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

    #[allow(clippy::too_many_arguments)]
    pub fn fill_aa_triangle(
        &mut self,
        x0: f32,
        y0: f32,
        x1: f32,
        y1: f32,
        x2: f32,
        y2: f32,
        color: u32,
        alpha: u8,
    ) {
        if alpha == 0 {
            return;
        }
        let min_x = x0.min(x1).min(x2).floor() as i32 - 1;
        let max_x = x0.max(x1).max(x2).ceil() as i32 + 1;
        let min_y = y0.min(y1).min(y2).floor() as i32 - 1;
        let max_y = y0.max(y1).max(y2).ceil() as i32 + 1;
        let area = (x1 - x0) * (y2 - y0) - (y1 - y0) * (x2 - x0);
        if area.abs() < 0.001 {
            return;
        }
        let sgn = if area > 0.0 { 1.0 } else { -1.0 };
        let l0 = ((x1 - x0) * (x1 - x0) + (y1 - y0) * (y1 - y0)).sqrt().max(1.0);
        let l1 = ((x2 - x1) * (x2 - x1) + (y2 - y1) * (y2 - y1)).sqrt().max(1.0);
        let l2 = ((x0 - x2) * (x0 - x2) + (y0 - y2) * (y0 - y2)).sqrt().max(1.0);

        for py in min_y..=max_y {
            let fy = py as f32 + 0.5;
            for px in min_x..=max_x {
                let fx = px as f32 + 0.5;
                let e0 = ((x1 - x0) * (fy - y0) - (y1 - y0) * (fx - x0)) * sgn / l0;
                let e1 = ((x2 - x1) * (fy - y1) - (y2 - y1) * (fx - x1)) * sgn / l1;
                let e2 = ((x0 - x2) * (fy - y2) - (y0 - y2) * (fx - x2)) * sgn / l2;
                let d = e0.min(e1).min(e2);
                let cov = (d + 0.5).clamp(0.0, 1.0);
                if cov > 0.01 {
                    let a = (alpha as f32 * cov) as u8;
                    if a > 0 {
                        self.blend_pixel(px, py, color, a);
                    }
                }
            }
        }
    }

    fn blend_rounded_outer_ring(
        &mut self,
        outer: (i32, i32, i32, i32, i32),
        inner: (i32, i32, i32, i32),
        alpha: u8,
    ) {
        let (sx, sy, sw, sh, sr) = outer;
        let (inner_x, inner_y, inner_w, inner_h) = inner;
        if sw <= 0 || sh <= 0 || alpha == 0 {
            return;
        }
        let sr = sr.min(sw / 2).min(sh / 2).max(0);
        let top_h = (inner_y - sy).clamp(0, sh);
        let bot_y = (inner_y + inner_h).clamp(sy, sy + sh);
        let bot_h = (sy + sh - bot_y).max(0);
        let mid_y = sy + top_h;
        let mid_h = (bot_y - mid_y).max(0);
        let left_w = (inner_x - sx).clamp(0, sw);
        let right_x = (inner_x + inner_w).clamp(sx, sx + sw);
        let right_w = (sx + sw - right_x).max(0);

        // Straight outer bands outside the inner window rect (zero wasted fills behind opaque UI!)
        if top_h > sr {
            self.blend_rect(sx, sy + sr, sw, top_h - sr, 0x000000, alpha);
        }
        let top_bar_h = top_h.min(sr);
        if top_bar_h > 0 {
            self.blend_rect(sx + sr, sy, (sw - 2 * sr).max(0), top_bar_h, 0x000000, alpha);
        }
        if bot_h > sr {
            self.blend_rect(sx, bot_y, sw, bot_h - sr, 0x000000, alpha);
        }
        let bot_bar_h = bot_h.min(sr);
        if bot_bar_h > 0 {
            self.blend_rect(
                sx + sr,
                sy + sh - bot_bar_h,
                (sw - 2 * sr).max(0),
                bot_bar_h,
                0x000000,
                alpha,
            );
        }
        if mid_h > 0 {
            if left_w > 0 {
                self.blend_rect(sx, mid_y, left_w, mid_h, 0x000000, alpha);
            }
            if right_w > 0 {
                self.blend_rect(right_x, mid_y, right_w, mid_h, 0x000000, alpha);
            }
        }
        for dy in 0..sr {
            for dx in 0..sr {
                let cov = Self::corner_cov(sr - 1 - dx, sr - 1 - dy, sr);
                if cov > 0 {
                    let a = ((alpha as u32 * cov as u32) / 255) as u8;
                    if a > 0 {
                        for &(px, py) in &[
                            (sx + dx, sy + dy),
                            (sx + sw - 1 - dx, sy + dy),
                            (sx + dx, sy + sh - 1 - dy),
                            (sx + sw - 1 - dx, sy + sh - 1 - dy),
                        ] {
                            if px < inner_x || px >= inner_x + inner_w || py < inner_y || py >= inner_y + inner_h {
                                self.blend_pixel(px, py, 0x000000, a);
                            }
                        }
                    }
                }
            }
        }
    }

    pub fn draw_soft_shadow(&mut self, x: i32, y: i32, w: i32, h: i32, r: i32) {
        let ir = r.min(w / 2).min(h / 2).max(0);
        let inner = (x + ir, y + ir, (w - 2 * ir).max(0), (h - 2 * ir).max(0));
        self.blend_rounded_outer_ring((x - 6, y + 2, w + 12, h + 14, r + 7), inner, 14);
        self.blend_rounded_outer_ring((x - 3, y + 4, w + 6, h + 8, r + 4), inner, 24);
        self.blend_rounded_outer_ring((x, y + 5, w + 2, h + 5, r + 2), inner, 36);
        self.blend_rounded_outer_ring((x + 1, y + 6, w, h + 2, r + 1), inner, 48);
    }

    fn draw_glyph(&mut self, x: i32, y: i32, g: &RasterGlyph, color: u32) -> i32 {
        let x0 = (-x).max(0) as usize;
        let y0 = (-y).max(0) as usize;
        let x1 = ((self.width as i32 - x).min(g.width as i32)).max(0) as usize;
        let y1 = ((self.height as i32 - y).min(g.height as i32)).max(0) as usize;
        for row in y0..y1 {
            let dst_row = (y + row as i32) as usize * self.width;
            let src_row = row * g.width;
            for col in x0..x1 {
                let a = g.alpha[src_row + col];
                if a > 0 {
                    let dst_idx = dst_row + (x + col as i32) as usize;
                    self.pixels[dst_idx] = blend_rgb(self.pixels[dst_idx], color, a);
                }
            }
        }
        g.advance
    }

    pub fn draw_char(&mut self, x: i32, y: i32, ch: char, color: u32, scale: i32) -> i32 {
        let g = rasterize_cached(ch, scale);
        self.draw_glyph(x, y, g, color)
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
            let g = rasterize_cached(ch, s);
            if cx + g.advance > limit {
                break;
            }
            cx += self.draw_glyph(cx, y, g, color);
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
            let hi = lerp_rgb(fill_color, 0xFFFFFF, 45, 255);
            let fr = r.min(fill_w / 2);
            self.fill_hgradient_rounded_rect(x + 1, y + 1, fill_w, (h - 2).max(0), fr, fill_color, hi);
        }
        self.draw_rounded_rect_alpha(x, y, w, h, r, border_color, 55);
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
        let r = 7.min(w / 2).min(h / 2);
        let top_col = lerp_rgb(bg_color, 0xFFFFFF, 10, 255);
        let bot_col = lerp_rgb(bg_color, 0x000000, 10, 255);
        self.fill_vgradient_rounded_rect(x, y, w, h, r, top_col, bot_col);
        self.draw_rounded_rect_alpha(x, y, w, h, r, border_color, 85);
        let tw = measure_text(label, 1);
        let tx = x + ((w - tw) / 2).max(4);
        let ty = y + ((h - (GLYPH_H as i32)) / 2).max(1);
        self.draw_text_clipped(tx, ty, w - 8, label, fg_color, 1);
    }

    pub fn draw_toggle_anim(&mut self, x: i32, y: i32, t: f32, accent: u32) {
        let t = t.clamp(0.0, 1.0);
        let ti = (t * 255.0) as u32;
        let track = lerp_rgb(0x262B36, accent, ti, 255);
        self.fill_rounded_rect(x, y, 36, 18, 9, track);
        self.draw_rounded_rect_alpha(x, y, 36, 18, 9, 0xFFFFFF, 28 + (ti * 27 / 255) as u8);
        let knob_cx = x as f32 + 9.0 + t * 17.0;
        self.fill_aa_circle(knob_cx + 0.5, y as f32 + 10.2, 6.2, 0x000000, 65);
        self.fill_aa_circle(knob_cx + 0.5, y as f32 + 9.0, 6.0, 0xF8FAFC, 255);
    }

    pub fn draw_cursor(&mut self, mx: i32, my: i32) {
        let x = mx as f32;
        let y = my as f32;
        // Soft shadow
        self.fill_aa_triangle(x + 1.5, y + 2.0, x + 1.5, y + 17.5, x + 13.0, y + 13.0, 0x000000, 75);
        // Clean modern crisp black interior with white outline
        self.fill_aa_triangle(x, y, x, y + 15.5, x + 11.2, y + 11.0, 0x090C12, 255);
        self.fill_aa_triangle(x + 4.8, y + 10.5, x + 8.5, y + 18.0, x + 10.5, y + 16.8, 0x090C12, 255);
        self.draw_aa_line(x + 0.5, y + 0.5, x + 0.5, y + 15.5, 1.3, 0xFFFFFF, 240);
        self.draw_aa_line(x + 0.5, y + 0.5, x + 11.2, y + 11.0, 1.3, 0xFFFFFF, 240);
        self.draw_aa_line(x + 0.5, y + 15.5, x + 5.2, y + 11.8, 1.2, 0xFFFFFF, 230);
        self.draw_aa_line(x + 5.2, y + 11.8, x + 11.2, y + 11.0, 1.2, 0xFFFFFF, 230);
    }

    pub fn draw_resize_cursor(&mut self, mx: i32, my: i32, ux: f32, uy: f32) {
        let len = (ux * ux + uy * uy).sqrt().max(0.001);
        let vx = ux / len;
        let vy = uy / len;
        let px = -vy;
        let py = vx;
        let cx = mx as f32;
        let cy = my as f32;
        let half = 7.5;
        let x0 = cx - vx * half;
        let y0 = cy - vy * half;
        let x1 = cx + vx * half;
        let y1 = cy + vy * half;

        // Crisp dark halo under bright white vector arrows for legibility on dark/light backgrounds
        for &(col, w, a) in &[(0x090C12, 3.0, 220u8), (0xF8FAFC, 1.4, 245u8)] {
            self.draw_aa_line(x0, y0, x1, y1, w, col, a);
            let h_len = 3.5;
            self.draw_aa_line(x0, y0, x0 + vx * h_len + px * h_len, y0 + vy * h_len + py * h_len, w, col, a);
            self.draw_aa_line(x0, y0, x0 + vx * h_len - px * h_len, y0 + vy * h_len - py * h_len, w, col, a);
            self.draw_aa_line(x1, y1, x1 - vx * h_len + px * h_len, y1 - vy * h_len + py * h_len, w, col, a);
            self.draw_aa_line(x1, y1, x1 - vx * h_len - px * h_len, y1 - vy * h_len - py * h_len, w, col, a);
        }
    }

    #[inline]
    pub fn smoothstep(x: f32) -> f32 {
        let x = x.clamp(0.0, 1.0);
        x * x * (3.0 - 2.0 * x)
    }

    #[inline]
    fn blit_warped_row(
        &mut self,
        src_row: &[u32],
        py: i32,
        row_l: f32,
        row_r: f32,
        alpha: u8,
    ) {
        if py < 0
            || py >= self.height as i32
            || alpha == 0
            || row_r <= row_l + 0.5
            || src_row.is_empty()
        {
            return;
        }
        let x0 = (row_l.floor() as i32).max(0);
        let x1 = (row_r.ceil() as i32).min(self.width as i32 - 1);
        if x0 > x1 {
            return;
        }
        let sw = src_row.len();
        let sw_f = sw as f32;
        let span = (row_r - row_l).max(1.0);
        let step_fp = ((sw_f / span) * 65536.0) as i64;
        let mut sx_fp = (((x0 as f32 + 0.5 - row_l) / span) * sw_f * 65536.0) as i64;
        let max_sx = (sw - 1) as i64;
        let dst_off = (py as usize) * self.width;
        let edge_fade = span > 8.0;

        for px in x0..=x1 {
            let sx = (sx_fp >> 16).clamp(0, max_sx) as usize;
            sx_fp += step_fp;
            let src_px = src_row[sx];
            if (src_px >> 24) != 0 {
                continue;
            }
            let pix_a = if edge_fade {
                let cov = (px as f32 + 1.0 - row_l)
                    .clamp(0.0, 1.0)
                    .min((row_r - px as f32).clamp(0.0, 1.0));
                ((alpha as f32) * cov) as u8
            } else {
                alpha
            };
            let idx = dst_off + px as usize;
            if pix_a == 255 {
                self.pixels[idx] = src_px;
            } else if pix_a > 0 {
                self.pixels[idx] = blend_rgb(self.pixels[idx], src_px, pix_a);
            }
        }
    }

    /// Warp an offscreen window capture (`0xFF______` = transparent) along a macOS-style
    /// S-curve funnel into `target_pill (px, py, pw)`. `t = 1.0` open -> `0.0` minimized.
    pub fn blit_genie_rect(
        &mut self,
        src: &[u32],
        src_size: (i32, i32),
        win_pos: (i32, i32),
        target_pill: (i32, i32, i32),
        t: f32,
        top_dock: bool,
    ) {
        let (sw, sh) = src_size;
        if sw <= 1 || sh <= 1 || src.len() < (sw as usize) * (sh as usize) {
            return;
        }
        let t = t.clamp(0.0, 1.0);
        if t <= 0.005 {
            return;
        }
        let p = 1.0 - t;
        let funnel = Self::smoothstep(p / 0.48);
        let slide = Self::smoothstep((p - 0.14) / 0.86);
        let pull_near = Self::smoothstep(p * 1.32);

        let wx = win_pos.0 as f32;
        let wy = win_pos.1 as f32;
        let sw_f = sw as f32;
        let sh_f = sh as f32;
        let wr = wx + sw_f;

        let px_f = target_pill.0 as f32;
        let pr_f = px_f + (target_pill.2.max(12)) as f32;
        let dock_anchor = target_pill.1 as f32 + if top_dock { 18.0 } else { 14.0 };

        let near_l = wx + (px_f - wx) * funnel;
        let near_r = wr + (pr_f - wr) * funnel;
        let far_k = slide * slide;
        let far_l = wx + (px_f - wx) * far_k;
        let far_r = wr + (pr_f - wr) * far_k;

        let (top_y, bot_y) = if top_dock {
            (
                wy + (dock_anchor - wy) * pull_near,
                (wy + sh_f) + (dock_anchor + 4.0 - (wy + sh_f)) * slide,
            )
        } else {
            (
                wy + (dock_anchor - 4.0 - wy) * slide,
                (wy + sh_f) + (dock_anchor - (wy + sh_f)) * pull_near,
            )
        };
        if bot_y <= top_y + 1.0 {
            return;
        }
        let y0 = (top_y.round() as i32).max(0);
        let y1 = (bot_y.round() as i32).min(self.height as i32 - 1);
        if y0 > y1 {
            return;
        }
        let alpha = if t < 0.28 {
            ((t / 0.28) * 255.0).clamp(0.0, 255.0) as u8
        } else {
            255
        };
        let span_y = (bot_y - top_y).max(1.0);
        let sw_u = sw as usize;

        for py in y0..=y1 {
            let v = ((py as f32 + 0.5 - top_y) / span_y).clamp(0.0, 1.0);
            let curve_t = if top_dock { 1.0 - v } else { v };
            let s_curve = 0.5 - 0.5 * (curve_t * core::f32::consts::PI).cos();
            let row_l = far_l + (near_l - far_l) * s_curve;
            let row_r = far_r + (near_r - far_r) * s_curve;
            let sy = ((v * (sh - 1) as f32).round() as i32).clamp(0, sh - 1) as usize;
            let src_row = &src[sy * sw_u..(sy + 1) * sw_u];
            self.blit_warped_row(src_row, py, row_l, row_r, alpha);
        }
    }

    /// Blit a captured offscreen window scaled around `dst_center` with uniform opacity `alpha`.
    pub fn blit_scaled_alpha_rect(
        &mut self,
        src: &[u32],
        src_size: (i32, i32),
        dst_center: (f32, f32),
        scale: f32,
        alpha: u8,
    ) {
        let (sw, sh) = src_size;
        if sw <= 1 || sh <= 1 || scale <= 0.02 || alpha == 0 || src.len() < (sw as usize) * (sh as usize) {
            return;
        }
        let dw = (sw as f32) * scale;
        let dh = (sh as f32) * scale;
        let row_l = dst_center.0 - dw * 0.5;
        let row_r = row_l + dw;
        let top_y = dst_center.1 - dh * 0.5;
        let bot_y = top_y + dh;
        let y0 = (top_y.round() as i32).max(0);
        let y1 = (bot_y.round() as i32).min(self.height as i32 - 1);
        let span_y = (bot_y - top_y).max(1.0);
        let sw_u = sw as usize;

        for py in y0..=y1 {
            let v = ((py as f32 + 0.5 - top_y) / span_y).clamp(0.0, 1.0);
            let sy = ((v * (sh - 1) as f32).round() as i32).clamp(0, sh - 1) as usize;
            let src_row = &src[sy * sw_u..(sy + 1) * sw_u];
            self.blit_warped_row(src_row, py, row_l, row_r, alpha);
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
