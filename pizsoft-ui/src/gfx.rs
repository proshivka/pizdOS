use std::fs::File;
use std::io::{self, BufWriter, Write};
use std::path::Path;

use crate::font::{GLYPH_H, measure_text, rasterize_cached};

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

    pub fn draw_soft_shadow(&mut self, x: i32, y: i32, w: i32, h: i32, r: i32) {
        self.blend_rounded_rect(x - 6, y + 2, w + 12, h + 14, r + 7, 0x000000, 14);
        self.blend_rounded_rect(x - 3, y + 4, w + 6, h + 8, r + 4, 0x000000, 24);
        self.blend_rounded_rect(x, y + 5, w + 2, h + 5, r + 2, 0x000000, 36);
        self.blend_rounded_rect(x + 1, y + 6, w, h + 2, r + 1, 0x000000, 48);
    }

    pub fn draw_char(&mut self, x: i32, y: i32, ch: char, color: u32, scale: i32) -> i32 {
        let g = rasterize_cached(ch, scale);
        for row in 0..g.height {
            let py = y + row as i32;
            let row_off = row * g.width;
            for col in 0..g.width {
                let a = g.alpha[row_off + col];
                if a > 0 {
                    self.blend_pixel(x + col as i32, py, color, a);
                }
            }
        }
        g.advance
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
            let adv = rasterize_cached(ch, s).advance;
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

    pub fn draw_toggle(&mut self, x: i32, y: i32, on: bool, accent: u32) {
        let track = if on { accent } else { 0x262B36 };
        self.fill_rounded_rect(x, y, 36, 18, 9, track);
        self.draw_rounded_rect_alpha(x, y, 36, 18, 9, 0xFFFFFF, if on { 55 } else { 28 });
        let knob_cx = if on { x + 26 } else { x + 9 };
        self.fill_aa_circle(knob_cx as f32 + 0.5, y as f32 + 10.2, 6.2, 0x000000, 65);
        self.fill_aa_circle(knob_cx as f32 + 0.5, y as f32 + 9.0, 6.0, 0xF8FAFC, 255);
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
