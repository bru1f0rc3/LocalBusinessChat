//! CPU rasterizer for egui meshes: the same output as egui_glow (premultiplied alpha,
//! blending in gamma space), but needs no GPU or OpenGL driver at all.

use eframe::egui;
use egui::epaint::{textures::TextureFilter, ClippedPrimitive, ImageData, ImageDelta, Primitive, TextureId, Vertex};
use std::collections::HashMap;

/// Largest texture side we accept (egui splits bigger images itself).
pub const MAX_TEXTURE_SIDE: usize = 8192;

struct Tex {
    w: usize,
    h: usize,
    /// premultiplied RGBA (sRGB gamma space), row-major
    px: Vec<[u8; 4]>,
    linear_mag: bool,
    linear_min: bool,
}

impl Tex {
    #[inline]
    fn texel(&self, x: i32, y: i32) -> [u8; 4] {
        let x = x.clamp(0, self.w as i32 - 1) as usize;
        let y = y.clamp(0, self.h as i32 - 1) as usize;
        self.px[y * self.w + x]
    }

    /// Sample at texel coordinates (u in 0..w, v in 0..h). Returns premultiplied RGBA 0..255.
    #[inline]
    fn sample(&self, u: f32, v: f32, linear: bool) -> [f32; 4] {
        if !linear {
            let t = self.texel(u.floor() as i32, v.floor() as i32);
            return [t[0] as f32, t[1] as f32, t[2] as f32, t[3] as f32];
        }
        let (fx, fy) = (u - 0.5, v - 0.5);
        let (x0, y0) = (fx.floor(), fy.floor());
        let (tx, ty) = (fx - x0, fy - y0);
        let (x0, y0) = (x0 as i32, y0 as i32);
        let a = self.texel(x0, y0);
        let b = self.texel(x0 + 1, y0);
        let c = self.texel(x0, y0 + 1);
        let d = self.texel(x0 + 1, y0 + 1);
        let mut out = [0.0f32; 4];
        for i in 0..4 {
            let top = a[i] as f32 + (b[i] as f32 - a[i] as f32) * tx;
            let bot = c[i] as f32 + (d[i] as f32 - c[i] as f32) * tx;
            out[i] = top + (bot - top) * ty;
        }
        out
    }
}

/// An RGB frame buffer in the layout GDI wants (0x00RRGGBB, top-down rows).
#[derive(Default)]
pub struct Canvas {
    pub w: usize,
    pub h: usize,
    pub px: Vec<u32>,
}

impl Canvas {
    pub fn resize(&mut self, w: usize, h: usize) {
        if self.w != w || self.h != h {
            self.w = w;
            self.h = h;
            self.px = vec![0; w * h];
        }
    }

    pub fn clear(&mut self, rgb: [u8; 3]) {
        let c = (rgb[0] as u32) << 16 | (rgb[1] as u32) << 8 | rgb[2] as u32;
        self.px.fill(c);
    }
}

#[derive(Default)]
pub struct Painter {
    tex: HashMap<TextureId, Tex>,
}

impl Painter {
    pub fn set_texture(&mut self, id: TextureId, delta: &ImageDelta) {
        let (w, h, px): (usize, usize, Vec<[u8; 4]>) = match &delta.image {
            ImageData::Color(img) => (img.size[0], img.size[1], img.pixels.iter().map(|c| c.to_array()).collect()),
            ImageData::Font(f) => (f.size[0], f.size[1], f.srgba_pixels(None).map(|c| c.to_array()).collect()),
        };
        let linear_mag = delta.options.magnification == TextureFilter::Linear;
        let linear_min = delta.options.minification == TextureFilter::Linear;
        match delta.pos {
            None => {
                self.tex.insert(id, Tex { w, h, px, linear_mag, linear_min });
            }
            Some([x0, y0]) => {
                let Some(t) = self.tex.get_mut(&id) else { return };
                t.linear_mag = linear_mag;
                t.linear_min = linear_min;
                for row in 0..h {
                    let y = y0 + row;
                    if y >= t.h || x0 >= t.w {
                        continue;
                    }
                    let n = w.min(t.w - x0);
                    t.px[y * t.w + x0..y * t.w + x0 + n].copy_from_slice(&px[row * w..row * w + n]);
                }
            }
        }
    }

    pub fn free(&mut self, id: TextureId) {
        self.tex.remove(&id);
    }

    pub fn paint(&self, canvas: &mut Canvas, ppp: f32, prims: &[ClippedPrimitive]) {
        let (cw, ch) = (canvas.w as f32, canvas.h as f32);
        for ClippedPrimitive { clip_rect, primitive } in prims {
            let Primitive::Mesh(mesh) = primitive else { continue };
            let Some(tex) = self.tex.get(&mesh.texture_id) else { continue };
            // the same rounding as the glScissor of egui_glow
            let clip = Clip {
                x0: (clip_rect.min.x * ppp).round().clamp(0.0, cw) as i32,
                y0: (clip_rect.min.y * ppp).round().clamp(0.0, ch) as i32,
                x1: (clip_rect.max.x * ppp).round().clamp(0.0, cw) as i32 - 1,
                y1: (clip_rect.max.y * ppp).round().clamp(0.0, ch) as i32 - 1,
            };
            if clip.x0 > clip.x1 || clip.y0 > clip.y1 {
                continue;
            }
            for t in mesh.indices.chunks_exact(3) {
                let (Some(a), Some(b), Some(c)) =
                    (mesh.vertices.get(t[0] as usize), mesh.vertices.get(t[1] as usize), mesh.vertices.get(t[2] as usize))
                else {
                    continue;
                };
                triangle(canvas, &clip, tex, ppp, a, b, c);
            }
        }
    }
}

struct Clip {
    x0: i32,
    y0: i32,
    x1: i32,
    y1: i32,
}

#[inline]
fn blend(dst: &mut u32, s: [u32; 4]) {
    if s[3] >= 255 {
        *dst = s[0].min(255) << 16 | s[1].min(255) << 8 | s[2].min(255);
        return;
    }
    let inv = 255 - s[3];
    let d = *dst;
    let r = (s[0] + (((d >> 16) & 0xff) * inv + 127) / 255).min(255);
    let g = (s[1] + (((d >> 8) & 0xff) * inv + 127) / 255).min(255);
    let b = (s[2] + ((d & 0xff) * inv + 127) / 255).min(255);
    *dst = r << 16 | g << 8 | b;
}

/// Linear attribute over the triangle: value at vertex 0 plus screen-space gradients.
#[derive(Clone, Copy)]
struct Grad {
    f0: f32,
    dx: f32,
    dy: f32,
}

fn triangle(canvas: &mut Canvas, clip: &Clip, tex: &Tex, ppp: f32, a: &Vertex, b: &Vertex, c: &Vertex) {
    let (mut v1, mut v2) = (b, c);
    let (x0, y0) = (a.pos.x * ppp, a.pos.y * ppp);
    let (mut x1, mut y1) = (v1.pos.x * ppp, v1.pos.y * ppp);
    let (mut x2, mut y2) = (v2.pos.x * ppp, v2.pos.y * ppp);
    let mut area = (x1 - x0) * (y2 - y0) - (x2 - x0) * (y1 - y0);
    if !(area.abs() > 1e-6) {
        return; // degenerate or NaN
    }
    if area < 0.0 {
        std::mem::swap(&mut v1, &mut v2);
        std::mem::swap(&mut x1, &mut x2);
        std::mem::swap(&mut y1, &mut y2);
        area = -area;
    }
    let v0 = a;

    // pixel (x, y) is covered when its centre (x + 0.5, y + 0.5) is inside
    let min_x = ((x0.min(x1).min(x2) - 0.5).ceil() as i32).max(clip.x0);
    let max_x = ((x0.max(x1).max(x2) - 0.5).floor() as i32).min(clip.x1);
    let min_y = ((y0.min(y1).min(y2) - 0.5).ceil() as i32).max(clip.y0);
    let max_y = ((y0.max(y1).max(y2) - 0.5).floor() as i32).min(clip.y1);
    if min_x > max_x || min_y > max_y {
        return;
    }

    // edge functions E(p) = A*px + B*py + C, positive inside
    let edge = |px: f32, py: f32, qx: f32, qy: f32| -> (f32, f32, f32) {
        let (a, b) = (-(qy - py), qx - px);
        (a, b, -(a * px + b * py))
    };
    let edges = [edge(x1, y1, x2, y2), edge(x2, y2, x0, y0), edge(x0, y0, x1, y1)];

    let grad = |f0: f32, f1: f32, f2: f32| -> Grad {
        let (d1, d2) = (f1 - f0, f2 - f0);
        Grad { f0, dx: (d1 * (y2 - y0) - d2 * (y1 - y0)) / area, dy: (d2 * (x1 - x0) - d1 * (x2 - x0)) / area }
    };

    let (tw, th) = (tex.w as f32, tex.h as f32);
    let uv_const = v0.uv == v1.uv && v0.uv == v2.uv;
    let col_const = v0.color == v1.color && v0.color == v2.color;
    let gu = grad(v0.uv.x * tw, v1.uv.x * tw, v2.uv.x * tw);
    let gv = grad(v0.uv.y * th, v1.uv.y * th, v2.uv.y * th);
    let linear = if gu.dx.abs() + gu.dy.abs() + gv.dx.abs() + gv.dy.abs() > 2.01 { tex.linear_min } else { tex.linear_mag };
    let col = |v: &Vertex| v.color.to_array().map(|x| x as f32);
    let (c0, c1, c2) = (col(v0), col(v1), col(v2));
    let gc: [Grad; 4] = std::array::from_fn(|i| grad(c0[i], c1[i], c2[i]));

    let const_texel = uv_const.then(|| tex.sample(v0.uv.x * tw, v0.uv.y * th, linear));
    // both constant: one colour for the whole triangle
    let const_src: Option<[u32; 4]> = match (const_texel, col_const) {
        (Some(t), true) => Some(std::array::from_fn(|i| (c0[i] * t[i] / 255.0 + 0.5).clamp(0.0, 255.0) as u32)),
        _ => None,
    };
    if const_src == Some([0, 0, 0, 0]) {
        return;
    }

    let w = canvas.w;
    for y in min_y..=max_y {
        let py = y as f32 + 0.5;
        let (mut xs, mut xe) = (min_x, max_x);
        let mut empty = false;
        for &(ea, eb, ec) in &edges {
            let r = eb * py + ec;
            if ea > 0.0 {
                // inside when ea*px + r >= 0 (left edge: ties belong to it)
                let t = (-r / ea - 0.5).clamp(min_x as f32 - 1.0, max_x as f32 + 1.0);
                xs = xs.max(t.ceil() as i32);
            } else if ea < 0.0 {
                // inside when ea*px + r > 0 (right edge: ties belong to the neighbour)
                let t = (-r / ea - 0.5).clamp(min_x as f32 - 1.0, max_x as f32 + 1.0);
                xe = xe.min(t.ceil() as i32 - 1);
            } else if !(r > 0.0 || (r == 0.0 && eb > 0.0)) {
                empty = true;
            }
        }
        if empty || xs > xe {
            continue;
        }
        let row = &mut canvas.px[y as usize * w..y as usize * w + w];
        if let Some(s) = const_src {
            for d in &mut row[xs as usize..=xe as usize] {
                blend(d, s);
            }
            continue;
        }
        let px0 = xs as f32 + 0.5;
        let (dxp, dyp) = (px0 - x0, py - y0);
        let at = |g: &Grad| g.f0 + g.dx * dxp + g.dy * dyp;
        let (mut u, mut v) = (at(&gu), at(&gv));
        let mut cc: [f32; 4] = std::array::from_fn(|i| at(&gc[i]));
        for d in &mut row[xs as usize..=xe as usize] {
            let t = match const_texel {
                Some(t) => t,
                None => tex.sample(u, v, linear),
            };
            let s: [u32; 4] = std::array::from_fn(|i| (cc[i].clamp(0.0, 255.0) * t[i] / 255.0 + 0.5) as u32);
            if s != [0, 0, 0, 0] {
                blend(d, s);
            }
            u += gu.dx;
            v += gv.dx;
            for i in 0..4 {
                cc[i] += gc[i].dx;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::{epaint::Mesh, pos2, Color32, Rect};

    fn white_tex() -> Painter {
        let mut p = Painter::default();
        let img = egui::ColorImage::new([1, 1], Color32::WHITE);
        p.set_texture(TextureId::Managed(0), &ImageDelta::full(img, egui::TextureOptions::LINEAR));
        p
    }

    #[test]
    fn rect_covers_exact_pixels_without_seams() {
        let p = white_tex();
        let mut c = Canvas::default();
        c.resize(10, 10);
        c.clear([0, 0, 0]);
        let mut m = Mesh::default();
        // half-transparent red: a double-blended diagonal would show up as a brighter pixel
        m.add_colored_rect(Rect::from_min_max(pos2(2.0, 3.0), pos2(7.0, 8.0)), Color32::from_rgba_premultiplied(128, 0, 0, 128));
        let prims = vec![ClippedPrimitive { clip_rect: Rect::EVERYTHING, primitive: Primitive::Mesh(m) }];
        p.paint(&mut c, 1.0, &prims);
        for y in 0..10 {
            for x in 0..10 {
                let inside = (2..7).contains(&x) && (3..8).contains(&y);
                let v = c.px[y * 10 + x];
                assert_eq!(v, if inside { 128 << 16 } else { 0 }, "pixel {x},{y}");
            }
        }
    }

    #[test]
    fn clip_rect_is_respected() {
        let p = white_tex();
        let mut c = Canvas::default();
        c.resize(8, 8);
        c.clear([0, 0, 0]);
        let mut m = Mesh::default();
        m.add_colored_rect(Rect::from_min_max(pos2(0.0, 0.0), pos2(8.0, 8.0)), Color32::WHITE);
        let prims = vec![ClippedPrimitive { clip_rect: Rect::from_min_max(pos2(2.0, 2.0), pos2(4.0, 4.0)), primitive: Primitive::Mesh(m) }];
        p.paint(&mut c, 1.0, &prims);
        let lit = c.px.iter().filter(|&&v| v == 0xffffff).count();
        assert_eq!(lit, 4);
    }
}
