//! Icons taken from the original program's UI, embedded into the executable.

use eframe::egui::{self, load::SizedTexture, TextureHandle, TextureOptions};
use std::collections::HashMap;

macro_rules! icons {
    ($($name:literal),* $(,)?) => {
        const ICONS: &[(&str, &[u8])] = &[$(($name, include_bytes!(concat!("../assets/icons/", $name, ".png")))),*];
    };
}

icons!(
    "app", "ev_info", "hd_chat", "hd_eventlog", "hd_users", "ic_bell", "ic_lock", "p_add", "p_block", "p_refresh",
    "p_remove", "st_away", "st_dnd", "st_offline", "st_private", "st_ready", "st_working", "tb_autoscroll", "tb_chat",
    "tb_delete", "tb_emoticon", "tb_file", "tb_info", "tb_join", "tb_leave", "tb_message", "tb_nick", "tb_refresh",
    "tb_signal", "tb_state", "vb_chat", "vb_eventlog", "vb_users",
);

pub struct Icons {
    map: HashMap<&'static str, TextureHandle>,
}

pub fn decode(bytes: &[u8]) -> Option<(Vec<u8>, u32, u32)> {
    let img = image::load_from_memory(bytes).ok()?.to_rgba8();
    let (w, h) = (img.width(), img.height());
    Some((img.into_raw(), w, h))
}

impl Icons {
    pub fn load(ctx: &egui::Context) -> Icons {
        let mut map = HashMap::new();
        for (name, bytes) in ICONS {
            if let Some((rgba, w, h)) = decode(bytes) {
                let ci = egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &rgba);
                map.insert(*name, ctx.load_texture(format!("icon:{name}"), ci, TextureOptions::LINEAR));
            }
        }
        Icons { map }
    }

    pub fn get(&self, name: &str) -> Option<SizedTexture> {
        self.map.get(name).map(|t| SizedTexture::new(t.id(), t.size_vec2()))
    }

    /// Texture scaled to a given height.
    pub fn sized(&self, name: &str, h: f32) -> Option<SizedTexture> {
        self.get(name).map(|t| SizedTexture::new(t.id, egui::vec2(t.size.x * h / t.size.y, h)))
    }

    pub fn state(&self, st: u8) -> &'static str {
        match st {
            0 => "st_ready",
            1 => "st_working",
            2 => "st_dnd",
            3 => "st_private",
            4 => "st_away",
            5 => "st_offline",
            _ => "st_working",
        }
    }

    pub fn app_icon() -> Option<egui::IconData> {
        let bytes = ICONS.iter().find(|(n, _)| *n == "app")?.1;
        let (rgba, width, height) = decode(bytes)?;
        Some(egui::IconData { rgba, width, height })
    }
}
