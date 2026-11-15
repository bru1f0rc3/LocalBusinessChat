#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod config;
mod emoticons;
mod icons;
mod net;
mod platform;
mod proto;
mod remote;
mod tray;
mod ui;

use eframe::egui;
use std::sync::Arc;

fn arg_value(args: &[String], name: &str) -> Option<String> {
    for (i, a) in args.iter().enumerate() {
        if let Some(v) = a.strip_prefix(&format!("{name}=")) {
            return Some(v.to_string());
        }
        if a == name {
            return args.get(i + 1).cloned();
        }
    }
    None
}

fn setup_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    let candidates = [r"C:\Windows\Fonts\segoeui.ttf", r"C:\Windows\Fonts\tahoma.ttf", r"C:\Windows\Fonts\arial.ttf"];
    if let Some(bytes) = candidates.iter().find_map(|p| std::fs::read(p).ok()) {
        fonts.font_data.insert("system".into(), egui::FontData::from_owned(bytes));
        fonts.families.entry(egui::FontFamily::Proportional).or_default().insert(0, "system".into());
    }
    if let Ok(bytes) = std::fs::read(r"C:\Windows\Fonts\seguisym.ttf") {
        fonts.font_data.insert("symbols".into(), egui::FontData::from_owned(bytes));
        fonts.families.entry(egui::FontFamily::Proportional).or_default().push("symbols".into());
    }
    ctx.set_fonts(fonts);
    ctx.style_mut(|s| {
        s.visuals = egui::Visuals::light();
        s.spacing.item_spacing = egui::vec2(6.0, 4.0);
    });
}

fn main() -> eframe::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if let Some(ms) = arg_value(&args, "--delay").and_then(|v| v.parse::<u64>().ok()) {
        std::thread::sleep(std::time::Duration::from_millis(ms.min(30_000)));
    }
    let profile = arg_value(&args, "--profile");
    let paths = config::Paths::new(profile.as_deref());
    let (cfg, _, _) = config::Config::load(&paths);

    let title = match &profile {
        Some(p) => format!("Nassi RS — {} [{p}]", cfg.nick),
        None => format!("Nassi RS — {}", cfg.nick),
    };
    let mut vp = egui::ViewportBuilder::default()
        .with_title(title)
        .with_inner_size([960.0, 640.0])
        .with_min_inner_size([560.0, 380.0]);
    if let Some(icon) = icons::Icons::app_icon() {
        vp = vp.with_icon(Arc::new(icon));
    }
    let pos = arg_value(&args, "--pos").and_then(|v| {
        let (x, y) = v.split_once(',')?;
        Some(egui::pos2(x.trim().parse().ok()?, y.trim().parse().ok()?))
    });
    if let Some(p) = pos {
        vp = vp.with_position(p);
    } else if let Some([x, y, w, h]) = cfg.window {
        vp = vp.with_position([x, y]).with_inner_size([w.max(560.0), (h - 30.0).max(380.0)]);
    }

    let options = eframe::NativeOptions { viewport: vp, ..Default::default() };
    eframe::run_native(
        "nassi-rs",
        options,
        Box::new(move |cc| {
            setup_fonts(&cc.egui_ctx);
            let mut app = app::App::new(cc, profile);
            if app.cfg.general.start_minimized && app.tray.is_some() && pos.is_none() {
                app.start_hidden = true;
            }
            Ok(Box::new(app))
        }),
    )
}
