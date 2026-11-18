//! OpenGL self-test: before the app starts with OpenGL, draw a typical frame of the UI
//! (side panel with icons and captions, toolbar, list with text) with egui_glow into an
//! offscreen framebuffer, read it back and compare it with the CPU rasterizer.
//!
//! Some old drivers report OpenGL 3.x but mix up vertex/index buffers between draw calls:
//! triangles get stretched across the whole window and only stripes of text are visible.
//! The version check cannot catch that, so we look at what the driver actually draws.

use crate::soft::raster::{Canvas, Painter as SoftPainter};
use eframe::egui::{self, Color32};
use eframe::{egui_glow, glow, glow::HasContext};
use std::sync::Arc;

const W: u32 = 480;
const H: u32 = 320;

/// The test frame: many small meshes with different clip rects and textures, the same kind
/// of shapes the real window has.
fn test_frame() -> (egui::TexturesDelta, Vec<egui::ClippedPrimitive>, Color32) {
    let ctx = egui::Context::default();
    ctx.set_visuals(egui::Visuals::light());
    let mut input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(W as f32, H as f32))),
        ..Default::default()
    };
    input.viewports.entry(egui::ViewportId::ROOT).or_default().native_pixels_per_point = Some(1.0);
    let mut textures = egui::TexturesDelta::default();
    let mut out = None;
    // a few passes: fonts get loaded during the first one
    for _ in 0..3 {
        let o = ctx.run(input.clone(), |ctx| {
            egui::TopBottomPanel::top("menu").show(ctx, |ui| {
                ui.horizontal(|ui| {
                    for s in ["Файл", "Вид", "Пользователь", "Действия", "Справка"] {
                        let _ = ui.button(s);
                    }
                });
            });
            egui::SidePanel::left("bar")
                .exact_width(92.0)
                .resizable(false)
                .frame(egui::Frame::none().fill(Color32::from_rgb(0x6e, 0x6e, 0x6e)))
                .show(ctx, |ui| {
                    for (k, s) in ["Пользователи", "Общий чат", "Личные", "Групповой", "Журнал событий"].iter().enumerate() {
                        let r = egui::Rect::from_min_size(ui.min_rect().min + egui::vec2(4.0, 8.0 + k as f32 * 56.0), egui::vec2(84.0, 50.0));
                        ui.painter().rect_filled(r.shrink(2.0), 3.0, Color32::from_rgb(0x88, 0x88, 0x88));
                        ui.painter().circle_filled(r.center_top() + egui::vec2(0.0, 16.0), 10.0, Color32::from_rgb(70, 130, 220));
                        ui.painter().text(r.center_bottom() - egui::vec2(0.0, 6.0), egui::Align2::CENTER_BOTTOM, *s, egui::FontId::proportional(11.5), Color32::WHITE);
                    }
                });
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.heading("Пользователи");
                egui::ScrollArea::vertical().show(ui, |ui| {
                    for i in 0..14 {
                        ui.horizontal(|ui| {
                            ui.label(egui::RichText::new(format!("user-{i:02}")).strong());
                            ui.label("Готов");
                            ui.label(format!("192.168.0.{}", 10 + i));
                            ui.label("Windows 7 SP1");
                        });
                    }
                });
            });
        });
        textures.append(o.textures_delta.clone());
        out = Some(o);
    }
    let o = out.expect("ran");
    let prims = ctx.tessellate(o.shapes, 1.0);
    (textures, prims, ctx.style().visuals.panel_fill)
}

/// Share of pixels that differ noticeably between the two images (0.0 ..= 1.0).
pub fn diff_share(gl_rgba: &[u8], soft: &Canvas) -> f32 {
    let mut bad = 0usize;
    for (i, &c) in soft.px.iter().enumerate() {
        let s = [(c >> 16) as u8, (c >> 8) as u8, c as u8];
        let g = &gl_rgba[i * 4..i * 4 + 3];
        if (0..3).any(|k| (s[k] as i32 - g[k] as i32).abs() > 48) {
            bad += 1;
        }
    }
    bad as f32 / soft.px.len().max(1) as f32
}

/// Ok(()) when OpenGL draws the test frame like the CPU rasterizer, Err(reason) when it
/// draws garbage. Errors of the test itself (no framebuffer objects, …) are not held
/// against the driver.
pub fn run(gl: &Arc<glow::Context>) -> Result<(), String> {
    let (textures, prims, bg) = test_frame();

    // reference image
    let mut soft = SoftPainter::default();
    for (id, d) in &textures.set {
        soft.set_texture(*id, d);
    }
    let mut canvas = Canvas::default();
    canvas.resize(W as usize, H as usize);
    canvas.clear([bg.r(), bg.g(), bg.b()]);
    soft.paint(&mut canvas, 1.0, &prims);

    let Ok(mut painter) = egui_glow::Painter::new(gl.clone(), "", None, true) else {
        return Ok(());
    };
    let rgba = unsafe {
        let Ok(fbo) = gl.create_framebuffer() else {
            painter.destroy();
            return Ok(());
        };
        let Ok(tex) = gl.create_texture() else {
            gl.delete_framebuffer(fbo);
            painter.destroy();
            return Ok(());
        };
        gl.bind_texture(glow::TEXTURE_2D, Some(tex));
        gl.tex_image_2d(glow::TEXTURE_2D, 0, glow::RGBA8 as i32, W as i32, H as i32, 0, glow::RGBA, glow::UNSIGNED_BYTE, None);
        gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MIN_FILTER, glow::NEAREST as i32);
        gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MAG_FILTER, glow::NEAREST as i32);
        gl.bind_texture(glow::TEXTURE_2D, None);
        gl.bind_framebuffer(glow::FRAMEBUFFER, Some(fbo));
        gl.framebuffer_texture_2d(glow::FRAMEBUFFER, glow::COLOR_ATTACHMENT0, glow::TEXTURE_2D, Some(tex), 0);
        let complete = gl.check_framebuffer_status(glow::FRAMEBUFFER) == glow::FRAMEBUFFER_COMPLETE;
        let mut px = Vec::new();
        if complete {
            let clear = [bg.r() as f32 / 255.0, bg.g() as f32 / 255.0, bg.b() as f32 / 255.0, 1.0];
            for (id, d) in &textures.set {
                painter.set_texture(*id, d);
            }
            // several frames in a row: broken drivers often get the buffers wrong only when
            // they are reused
            for _ in 0..3 {
                painter.clear([W, H], clear);
                painter.paint_primitives([W, H], 1.0, &prims);
            }
            gl.finish();
            px = painter.read_screen_rgba([W, H]).pixels.iter().flat_map(|c| c.to_array()).collect();
        }
        gl.bind_framebuffer(glow::FRAMEBUFFER, None);
        gl.delete_framebuffer(fbo);
        gl.delete_texture(tex);
        painter.destroy();
        if !complete {
            return Ok(());
        }
        px
    };
    let share = diff_share(&rgba, &canvas);
    if std::env::var_os("NASSI_GLTEST_LOG").is_some() {
        eprintln!("gltest: {:.3}% pixels differ", share * 100.0);
    }
    if share > 0.04 {
        return Err(format!("драйвер OpenGL рисует с искажениями (тест: {:.0}% точек не совпало)", share * 100.0));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identical_images_match_and_garbage_does_not() {
        let (textures, prims, bg) = test_frame();
        assert!(prims.len() > 5, "the test frame should have many meshes");
        let mut soft = SoftPainter::default();
        for (id, d) in &textures.set {
            soft.set_texture(*id, d);
        }
        let mut canvas = Canvas::default();
        canvas.resize(W as usize, H as usize);
        canvas.clear([bg.r(), bg.g(), bg.b()]);
        soft.paint(&mut canvas, 1.0, &prims);
        let same: Vec<u8> = canvas.px.iter().flat_map(|&c| [(c >> 16) as u8, (c >> 8) as u8, c as u8, 255]).collect();
        assert_eq!(diff_share(&same, &canvas), 0.0);
        // what the broken driver shows: mostly a dark fill
        let dark: Vec<u8> = canvas.px.iter().flat_map(|_| [30u8, 30, 40, 255]).collect();
        assert!(diff_share(&dark, &canvas) > 0.5);

        // what the broken driver does: each draw call uses the indices with the vertex
        // buffer of the previous one (stale buffer) -> stretched triangles
        let mut broken = prims.clone();
        let verts: Vec<_> = prims.iter().map(|p| match &p.primitive { egui::epaint::Primitive::Mesh(m) => m.vertices.clone(), _ => vec![] }).collect();
        for (k, p) in broken.iter_mut().enumerate().skip(1) {
            if let egui::epaint::Primitive::Mesh(m) = &mut p.primitive {
                let prev = &verts[k - 1];
                for (i, v) in m.vertices.iter_mut().enumerate() {
                    if let Some(pv) = prev.get(i) {
                        v.pos = pv.pos;
                    }
                }
            }
        }
        let mut bad = Canvas::default();
        bad.resize(W as usize, H as usize);
        bad.clear([bg.r(), bg.g(), bg.b()]);
        soft.paint(&mut bad, 1.0, &broken);
        let bad_rgba: Vec<u8> = bad.px.iter().flat_map(|&c| [(c >> 16) as u8, (c >> 8) as u8, c as u8, 255]).collect();
        let share = diff_share(&bad_rgba, &canvas);
        assert!(share > 0.04, "stale-buffer corruption must be detected, got {share}");
    }
}
