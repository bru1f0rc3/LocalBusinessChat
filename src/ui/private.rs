//! "Личные сообщения": Telegram-like list of dialogs with a search box on the left
//! and the selected conversation on the right.

use super::{
    chat_input, chat_lines, contains_ci, find_ci, img, lower_chars, search_bar, search_icon_button, search_matches, sync_search, tool_button, Act,
    Highlight,
};
use crate::app::*;
use crate::config::*;
use eframe::egui::{self, load::SizedTexture, Color32, FontId, Key, Modifiers, RichText, Sense};
use std::sync::Arc;

const ACCENT: Color32 = Color32::from_rgb(0x41, 0x9f, 0xd9);
const BADGE: Color32 = Color32::from_rgb(0x4f, 0xa7, 0xe8);
const ONLINE: Color32 = Color32::from_rgb(0x4c, 0xc7, 0x5a);
const HOVER: Color32 = Color32::from_rgb(0xf1, 0xf3, 0xf5);
const AVATAR_COLORS: [Color32; 7] = [
    Color32::from_rgb(0xe1, 0x70, 0x76),
    Color32::from_rgb(0xfa, 0xa7, 0x74),
    Color32::from_rgb(0xa6, 0x95, 0xe7),
    Color32::from_rgb(0x7b, 0xc8, 0x62),
    Color32::from_rgb(0x6e, 0xc9, 0xcb),
    Color32::from_rgb(0x65, 0xaa, 0xdd),
    Color32::from_rgb(0xee, 0x7a, 0xae),
];

/// One entry of the dialog list.
struct Dlg {
    uid: u64,
    name: String,
    sub: String,
    /// (time, preview, mine, delivered)
    last: Option<(i64, String, bool, bool)>,
    unread: u32,
    online: bool,
    typing: bool,
}

/// A message found by the search box above the dialog list.
struct Found {
    uid: u64,
    id: u64,
    time: i64,
    name: String,
    text: String,
    mine: bool,
}

fn avatar_color(uid: u64) -> Color32 {
    AVATAR_COLORS[((uid.wrapping_mul(0x9E37_79B9_7F4A_7C15) >> 32) % AVATAR_COLORS.len() as u64) as usize]
}

/// Round photo, or a colored circle with the first letter; small green dot when online.
fn paint_avatar(ui: &egui::Ui, center: egui::Pos2, r: f32, tex: Option<&egui::TextureHandle>, name: &str, uid: u64, online: bool, ring: Color32) {
    let rect = egui::Rect::from_center_size(center, egui::vec2(r * 2.0, r * 2.0));
    let p = ui.painter();
    match tex {
        Some(t) => egui::Image::from_texture(SizedTexture::new(t.id(), rect.size())).rounding(r).paint_at(ui, rect),
        None => {
            p.circle_filled(center, r, avatar_color(uid));
            let letter: String = name.chars().find(|c| c.is_alphanumeric()).map(|c| c.to_uppercase().collect()).unwrap_or_else(|| "?".into());
            p.text(center, egui::Align2::CENTER_CENTER, letter, FontId::proportional(r * 0.95), Color32::WHITE);
        }
    }
    if online {
        let c = center + egui::vec2(r * 0.70, r * 0.70);
        let dr = (r * 0.22).max(4.0);
        p.circle_filled(c, dr + 2.0, ring);
        p.circle_filled(c, dr, ONLINE);
    }
}

/// Single line of text cut with "…" at `max_w`.
fn one_line(ui: &egui::Ui, text: &str, size: f32, color: Color32, max_w: f32) -> Arc<egui::Galley> {
    let mut job = egui::text::LayoutJob::single_section(
        text.replace(['\n', '\r', '\t'], " "),
        egui::TextFormat { font_id: FontId::proportional(size), color, ..Default::default() },
    );
    job.wrap = egui::text::TextWrapping { max_width: max_w.max(8.0), max_rows: 1, break_anywhere: true, overflow_character: Some('…') };
    ui.fonts(|f| f.layout_job(job))
}

/// Like `one_line`, with the search matches highlighted.
fn one_line_hl(ui: &egui::Ui, text: &str, needle: &[char], size: f32, color: Color32, max_w: f32) -> Arc<egui::Galley> {
    let text = text.replace(['\n', '\r', '\t'], " ");
    let normal = egui::TextFormat { font_id: FontId::proportional(size), color, ..Default::default() };
    let hit = egui::TextFormat { font_id: FontId::proportional(size), color: Color32::BLACK, background: Color32::from_rgb(255, 214, 90), ..Default::default() };
    let mut job = egui::text::LayoutJob::default();
    let mut pos = 0;
    for (a, b) in find_ci(&text, needle) {
        if a > pos {
            job.append(&text[pos..a], 0.0, normal.clone());
        }
        job.append(&text[a..b], 0.0, hit.clone());
        pos = b;
    }
    if pos < text.len() {
        job.append(&text[pos..], 0.0, normal);
    }
    job.wrap = egui::text::TextWrapping { max_width: max_w.max(8.0), max_rows: 1, break_anywhere: true, overflow_character: Some('…') };
    ui.fonts(|f| f.layout_job(job))
}

/// Telegram-style short time: today "14:05", this week "Пн", older "03.11.26".
pub fn short_time(t: i64) -> String {
    use chrono::Datelike;
    let Some(d) = chrono::DateTime::from_timestamp(t, 0).map(|d| d.with_timezone(&chrono::Local)) else { return String::new() };
    let now = chrono::Local::now();
    let days = (now.date_naive() - d.date_naive()).num_days();
    if days == 0 {
        d.format("%H:%M").to_string()
    } else if (1..7).contains(&days) {
        ["Пн", "Вт", "Ср", "Чт", "Пт", "Сб", "Вс"][d.weekday().num_days_from_monday() as usize].to_string()
    } else {
        d.format("%d.%m.%y").to_string()
    }
}

/// Text around the first match, so the match is visible in a one-line preview.
fn snippet(text: &str, needle: &[char]) -> String {
    let flat = text.replace(['\n', '\r', '\t'], " ");
    let Some(&(a, _)) = find_ci(&flat, needle).first() else { return flat };
    let before: Vec<char> = flat[..a].chars().collect();
    if before.len() <= 18 {
        flat
    } else {
        let start: String = before[before.len() - 15..].iter().collect();
        format!("…{start}{}", &flat[a..])
    }
}

impl App {
    fn dm_dialogs(&self) -> Vec<Dlg> {
        let now = std::time::Instant::now();
        let mut v: Vec<Dlg> = self
            .chats
            .values()
            .filter(|c| !c.lines.is_empty() || self.private_sel == Some(c.uid))
            .map(|c| {
                let peer = self.peers.get(&c.uid);
                let last = c.lines.iter().rev().find(|l| l.from != 0).map(|l| {
                    let mine = l.from == self.cfg.uid;
                    (l.time, l.text.clone(), mine, l.delivered)
                });
                Dlg {
                    uid: c.uid,
                    name: self.peer_name(c.uid),
                    sub: peer.map(|p| p.p.computer.clone()).or_else(|| self.cfg.known.get(&c.uid).map(|k| k.1.clone())).unwrap_or_default(),
                    last,
                    unread: c.unread,
                    online: peer.is_some(),
                    typing: peer.is_some_and(|p| p.typing_until.is_some_and(|t| t > now)),
                }
            })
            .collect();
        v.sort_by_key(|d| std::cmp::Reverse(d.last.as_ref().map(|l| l.0).unwrap_or(i64::MAX)));
        v
    }

    /// Paint one dialog row; returns its response.
    fn dialog_row(&self, ui: &mut egui::Ui, d: &Dlg, sel: bool, needle: &[char]) -> egui::Response {
        let h = 62.0;
        let (rect, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), h), Sense::click());
        if !ui.is_rect_visible(rect) {
            return resp;
        }
        let bg = if sel {
            ACCENT
        } else if resp.hovered() {
            HOVER
        } else {
            Color32::WHITE
        };
        ui.painter().rect_filled(rect, 0.0f32, bg);
        let (c_name, c_text, c_time) = if sel {
            (Color32::WHITE, Color32::from_rgb(0xe6, 0xf2, 0xfb), Color32::from_rgb(0xe6, 0xf2, 0xfb))
        } else {
            (Color32::from_rgb(0x22, 0x22, 0x22), Color32::from_rgb(0x70, 0x79, 0x81), Color32::from_rgb(0x95, 0x99, 0x9d))
        };
        let r = 24.0;
        let tex = self.peers.get(&d.uid).and_then(|p| p.avatar.as_ref());
        paint_avatar(ui, egui::pos2(rect.left() + 10.0 + r, rect.center().y), r, tex, &d.name, d.uid, d.online, bg);
        let x0 = rect.left() + 10.0 + 2.0 * r + 10.0;
        let x1 = rect.right() - 10.0;
        let y_top = rect.top() + 11.0;
        let y_bot = rect.top() + 34.0;
        let p = ui.painter();

        // top line: name ........ ✔ time
        let mut right_w = 0.0;
        if let Some((t, _, mine, delivered)) = &d.last {
            let g = one_line(ui, &short_time(*t), 12.0, c_time, 80.0);
            right_w = g.size().x;
            p.galley(egui::pos2(x1 - g.size().x, y_top + 2.0), g, c_time);
            if *mine {
                let (s, c) = if *delivered {
                    ("✔", if sel { Color32::WHITE } else { Color32::from_rgb(0x4f, 0xae, 0x4e) })
                } else {
                    ("⏳", if sel { Color32::WHITE } else { Color32::from_rgb(0xc8, 0x82, 0x00) })
                };
                let g = one_line(ui, s, 12.0, c, 30.0);
                right_w += g.size().x + 4.0;
                p.galley(egui::pos2(x1 - right_w, y_top + 2.0), g, c);
            }
        }
        let name_w = x1 - x0 - right_w - 8.0;
        let g = if needle.is_empty() { one_line(ui, &d.name, 14.5, c_name, name_w) } else { one_line_hl(ui, &d.name, needle, 14.5, c_name, name_w) };
        p.galley(egui::pos2(x0, y_top), g, c_name);

        // bottom line: preview ........ (unread)
        let mut badge_w = 0.0;
        if d.unread > 0 {
            let txt = if d.unread > 999 { "999+".to_string() } else { d.unread.to_string() };
            let (fill, tc) = if sel { (Color32::WHITE, ACCENT) } else { (BADGE, Color32::WHITE) };
            let g = one_line(ui, &txt, 12.0, tc, 60.0);
            badge_w = (g.size().x + 12.0).max(22.0);
            let br = egui::Rect::from_min_size(egui::pos2(x1 - badge_w, y_bot - 1.0), egui::vec2(badge_w, 21.0));
            p.rect_filled(br, 10.5f32, fill);
            p.galley(br.center() - g.size() / 2.0, g, tc);
        }
        let prev_w = x1 - x0 - badge_w - 8.0;
        if d.typing {
            let c = if sel { Color32::WHITE } else { ACCENT };
            p.galley(egui::pos2(x0, y_bot), one_line(ui, "печатает…", 13.0, c, prev_w), c);
        } else {
            let text = match &d.last {
                Some((_, t, true, _)) => format!("Вы: {t}"),
                Some((_, t, false, _)) => t.clone(),
                None if d.online => "в сети — напишите первое сообщение".to_string(),
                None => "нет сообщений".to_string(),
            };
            p.galley(egui::pos2(x0, y_bot), one_line(ui, &text, 13.0, c_text, prev_w), c_text);
        }
        resp
    }

    /// Smaller row for "Пользователи" and "Сообщения" search results.
    #[allow(clippy::too_many_arguments)]
    fn result_row(&self, ui: &mut egui::Ui, uid: u64, title: &str, right: &str, text: &str, needle: &[char], title_hl: bool) -> egui::Response {
        let (rect, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 54.0), Sense::click());
        if !ui.is_rect_visible(rect) {
            return resp;
        }
        let bg = if resp.hovered() { HOVER } else { Color32::WHITE };
        ui.painter().rect_filled(rect, 0.0f32, bg);
        let r = 20.0;
        let online = self.peers.contains_key(&uid);
        let tex = self.peers.get(&uid).and_then(|p| p.avatar.as_ref());
        paint_avatar(ui, egui::pos2(rect.left() + 10.0 + r, rect.center().y), r, tex, title, uid, online, bg);
        let x0 = rect.left() + 10.0 + 2.0 * r + 10.0;
        let x1 = rect.right() - 10.0;
        let p = ui.painter();
        let c_name = Color32::from_rgb(0x22, 0x22, 0x22);
        let c_text = Color32::from_rgb(0x70, 0x79, 0x81);
        let g = one_line(ui, right, 12.0, Color32::from_rgb(0x95, 0x99, 0x9d), 90.0);
        let rw = g.size().x;
        p.galley(egui::pos2(x1 - rw, rect.top() + 10.0), g, c_text);
        let g = if title_hl { one_line_hl(ui, title, needle, 14.0, c_name, x1 - x0 - rw - 8.0) } else { one_line(ui, title, 14.0, c_name, x1 - x0 - rw - 8.0) };
        p.galley(egui::pos2(x0, rect.top() + 8.0), g, c_name);
        let g = if title_hl { one_line(ui, text, 12.5, c_text, x1 - x0) } else { one_line_hl(ui, text, needle, 12.5, c_text, x1 - x0) };
        p.galley(egui::pos2(x0, rect.top() + 29.0), g, c_text);
        resp
    }

    fn section_title(ui: &mut egui::Ui, s: &str) {
        egui::Frame::none().fill(Color32::from_rgb(0xf4, 0xf4, 0xf5)).inner_margin(egui::Margin::symmetric(10.0, 4.0)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new(s).color(Color32::from_rgb(0x70, 0x79, 0x81)).size(12.5));
        });
    }

    pub(super) fn private_page(&mut self, ui: &mut egui::Ui, acts: &mut Vec<Act>) -> bool {
        let my = self.cfg.uid;
        // ---------------------------------------------------------------- dialog list
        egui::SidePanel::left("dm_list")
            .resizable(true)
            .default_width(300.0)
            .width_range(220.0..=520.0)
            .frame(egui::Frame::none().fill(Color32::WHITE).stroke(egui::Stroke::new(1.0f32, Color32::from_gray(215))))
            .show_inside(ui, |ui| {
                // search box (rounded, like Telegram)
                egui::Frame::none().inner_margin(egui::Margin::symmetric(8.0, 8.0)).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    egui::Frame::none().fill(Color32::from_rgb(0xf1, 0xf1, 0xf1)).rounding(16.0).inner_margin(egui::Margin::symmetric(10.0, 5.0)).show(ui, |ui| {
                        ui.horizontal(|ui| {
                            let id = egui::Id::new("dm_filter");
                            if ui.memory(|m| m.has_focus(id)) && ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape)) {
                                self.dm_filter.clear();
                            }
                            let clear_w = if self.dm_filter.is_empty() { 0.0 } else { 22.0 };
                            let r = ui.add(
                                egui::TextEdit::singleline(&mut self.dm_filter)
                                    .id(id)
                                    .frame(false)
                                    .hint_text("🔍 Поиск")
                                    .desired_width(ui.available_width() - clear_w),
                            );
                            if self.dm_filter_focus {
                                r.request_focus();
                                self.dm_filter_focus = false;
                            }
                            r.on_hover_text("Поиск по собеседникам и по тексту всех личных сообщений (Esc — очистить)");
                            if !self.dm_filter.is_empty() && ui.add(egui::Button::new("✖").frame(false)).on_hover_text("Очистить").clicked() {
                                self.dm_filter.clear();
                            }
                        });
                    });
                });
                let needle = lower_chars(&self.dm_filter);
                let dialogs = self.dm_dialogs();
                egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    if needle.is_empty() {
                        for d in &dialogs {
                            let r = self.dialog_row(ui, d, self.private_sel == Some(d.uid), &[]);
                            if r.clicked() {
                                acts.push(Act::Chat(d.uid));
                            }
                            r.context_menu(|ui| self.user_menu(ui, d.uid, acts));
                        }
                        if dialogs.is_empty() {
                            ui.add_space(20.0);
                            ui.vertical_centered(|ui| {
                                ui.label(
                                    RichText::new("Личных сообщений пока нет.\n\nНайдите собеседника через поиск выше\nили щёлкните правой кнопкой по пользователю\n→ «Написать личное сообщение».")
                                        .color(Color32::GRAY),
                                );
                            });
                        }
                        return;
                    }
                    // ---- search results: chats, users, messages
                    let chats: Vec<&Dlg> = dialogs.iter().filter(|d| contains_ci(&d.name, &needle) || contains_ci(&d.sub, &needle)).collect();
                    let mut users: Vec<(u64, String, String, bool)> = Vec::new();
                    for p in self.peers.values() {
                        if !dialogs.iter().any(|d| d.uid == p.uid) && (contains_ci(&p.p.nick, &needle) || contains_ci(&p.p.computer, &needle)) {
                            users.push((p.uid, p.p.nick.clone(), p.p.computer.clone(), true));
                        }
                    }
                    for (u, (nick, comp)) in &self.cfg.known {
                        if *u != my
                            && !self.peers.contains_key(u)
                            && !dialogs.iter().any(|d| d.uid == *u)
                            && (contains_ci(nick, &needle) || contains_ci(comp, &needle))
                        {
                            users.push((*u, nick.clone(), comp.clone(), false));
                        }
                    }
                    users.sort_by(|a, b| b.3.cmp(&a.3).then(a.1.to_lowercase().cmp(&b.1.to_lowercase())));
                    let mut found: Vec<Found> = Vec::new();
                    for c in self.chats.values() {
                        let name = self.peer_name(c.uid);
                        for l in c.lines.iter().filter(|l| l.from != 0 && contains_ci(&l.text, &needle)) {
                            found.push(Found { uid: c.uid, id: l.id, time: l.time, name: name.clone(), text: l.text.clone(), mine: l.from == my });
                        }
                    }
                    found.sort_by_key(|f| std::cmp::Reverse(f.time));
                    let total_found = found.len();
                    found.truncate(200);

                    if !chats.is_empty() {
                        Self::section_title(ui, "Чаты");
                        for d in chats {
                            let r = self.dialog_row(ui, d, self.private_sel == Some(d.uid), &needle);
                            if r.clicked() {
                                acts.push(Act::Chat(d.uid));
                            }
                            r.context_menu(|ui| self.user_menu(ui, d.uid, acts));
                        }
                    }
                    if !users.is_empty() {
                        Self::section_title(ui, "Пользователи");
                        for (u, nick, comp, online) in &users {
                            let sub = if *online { format!("в сети · {comp}") } else { format!("не в сети · {comp}") };
                            let r = self.result_row(ui, *u, nick, "", &sub, &needle, true).on_hover_text("Написать личное сообщение");
                            if r.clicked() {
                                acts.push(Act::Chat(*u));
                            }
                            r.context_menu(|ui| self.user_menu(ui, *u, acts));
                        }
                    }
                    Self::section_title(
                        ui,
                        &if total_found > found.len() {
                            format!("Найдено сообщений: {total_found} (показаны последние {})", found.len())
                        } else {
                            format!("Найдено сообщений: {total_found}")
                        },
                    );
                    for f in &found {
                        let text = snippet(&f.text, &needle);
                        let text = if f.mine { format!("Вы: {text}") } else { text };
                        let r = self.result_row(ui, f.uid, &f.name, &short_time(f.time), &text, &needle, false);
                        if r.clicked() {
                            acts.push(Act::JumpPrivate(f.uid, f.id));
                        }
                    }
                    if total_found == 0 {
                        ui.add_space(8.0);
                        ui.vertical_centered(|ui| {
                            ui.label(RichText::new("Сообщений не найдено").color(Color32::GRAY));
                        });
                    }
                });
            });

        // ---------------------------------------------------------------- conversation
        let Some(uid) = self.private_sel.filter(|u| *u != my) else {
            egui::CentralPanel::default().frame(egui::Frame::none().fill(Color32::from_rgb(0xe6, 0xeb, 0xee))).show_inside(ui, |ui| {
                ui.centered_and_justified(|ui| {
                    egui::Frame::none().fill(Color32::from_rgba_unmultiplied(0, 0, 0, 40)).rounding(12.0).inner_margin(egui::Margin::symmetric(12.0, 4.0)).show(ui, |ui| {
                        ui.label(RichText::new("Выберите, кому хотели бы написать").color(Color32::WHITE).size(14.0));
                    });
                });
            });
            return false;
        };
        let nick = self.peer_name(uid);
        let peer = self.peers.get(&uid);
        let online = peer.is_some();
        let typing = peer.is_some_and(|p| p.typing_until.is_some_and(|t| t > std::time::Instant::now()));
        let status = match peer {
            Some(p) => {
                let mut s = format!("в сети · {} · {}", p.p.computer, p.p.state_title);
                if !p.p.state_text.is_empty() {
                    s.push_str(&format!(" — {}", p.p.state_text));
                }
                s
            }
            None => "не в сети — сообщения будут доставлены, когда появится".into(),
        };
        let avatar = peer.and_then(|p| p.avatar.clone());
        let st_icon = self.icons.sized(self.icons.state(peer.map(|p| p.p.state).unwrap_or(ST_OFFLINE)), 16.0);
        let font = self.cfg.chat.font_size;
        let nicks: Vec<String> = self.peers.values().map(|p| p.p.nick.clone()).collect();
        let emo_icon = self.icons.sized("tb_emoticon", 22.0);
        let away = self.user_away();
        let icons = &self.icons;
        let emo = if self.cfg.chat.no_emoticons { None } else { self.emo.as_ref() };
        let macros = &mut self.cfg.chat.macros;
        let chat = self.chats.entry(uid).or_insert_with(|| PrivateChat::empty(uid));
        if !away {
            chat.unread = 0;
        }

        // header: avatar, name, status ............ tools, 🔍
        egui::TopBottomPanel::top("dm_header")
            .frame(egui::Frame::none().fill(Color32::WHITE).inner_margin(egui::Margin::symmetric(10.0, 6.0)).stroke(egui::Stroke::new(1.0f32, Color32::from_gray(220))))
            .show_inside(ui, |ui| {
                // tools are laid out first (right to left), the name/status get the rest
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if search_icon_button(ui, chat.search.open, Color32::from_gray(80)).clicked() {
                        chat.search.toggle();
                    }
                    let has_lines = !chat.lines.is_empty();
                    ui.menu_button(RichText::new("⋮").size(18.0), |ui| {
                        if ui.button("Информация о пользователе").clicked() {
                            acts.push(Act::Info(uid, InfoTab::Details));
                            ui.close_menu();
                        }
                        if ui.add_enabled(has_lines, egui::Button::new("🗑 Очистить историю…")).clicked() {
                            acts.push(Act::ClearChat(uid));
                            ui.close_menu();
                        }
                        ui.separator();
                        if ui.button("Закрыть чат    Esc").clicked() {
                            acts.push(Act::CloseDm);
                            ui.close_menu();
                        }
                    })
                    .response
                    .on_hover_text("Ещё");
                    let mut tb = |ui: &mut egui::Ui, icon: &str, tip: &str, en: bool, a: Act| {
                        if tool_button(ui, icons.sized(icon, 20.0), None, 20.0, en, false).on_hover_text(tip).clicked() {
                            acts.push(a);
                        }
                    };
                    tb(ui, "tb_info", "Информация о пользователе", online, Act::Info(uid, InfoTab::Details));
                    tb(ui, "tb_file", "Отправить файл", online, Act::File(uid));
                    tb(ui, "tb_signal", "Сигнал", online, Act::Signal(Some(uid)));
                    ui.add_space(8.0);
                    ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                        let (rect, _) = ui.allocate_exact_size(egui::vec2(40.0, 40.0), Sense::hover());
                        paint_avatar(ui, rect.center(), 20.0, avatar.as_ref(), &nick, uid, online, Color32::WHITE);
                        ui.vertical(|ui| {
                            ui.horizontal(|ui| {
                                ui.add(img(st_icon));
                                ui.add(egui::Label::new(RichText::new(&nick).strong().size(15.0).color(Color32::from_rgb(0x22, 0x22, 0x22))).truncate());
                            });
                            if typing {
                                ui.label(RichText::new("печатает…").color(ACCENT));
                            } else {
                                ui.add(egui::Label::new(RichText::new(&status).color(if online { ACCENT } else { Color32::GRAY }).size(12.5)).truncate())
                                    .on_hover_text(&status);
                            }
                        });
                    });
                });
            });

        let (needle, matches) = search_matches(&chat.lines, &chat.search);
        sync_search(&mut chat.search, &chat.lines, &matches);
        if chat.search.open {
            egui::TopBottomPanel::top("dm_search").frame(egui::Frame::none()).show_inside(ui, |ui| {
                search_bar(ui, &mut chat.search, ("dm_search", uid), matches.len());
            });
        }

        let input_focused = ui.memory(|m| m.has_focus(egui::Id::new(("pchat_box", uid))));
        if input_focused && chat.input.text.is_empty() && !chat.search.open && ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape)) {
            acts.push(Act::CloseDm);
        }
        egui::TopBottomPanel::bottom("dm_input").show_inside(ui, |ui| {
            ui.add_space(3.0);
            let out = chat_input(ui, &mut chat.input, ("pchat_box", uid), emo, emo_icon, macros, &nicks, font);
            if let Some(t) = out.submit {
                acts.push(Act::SendPrivate(uid, t));
            } else if out.typed && !chat.input.text.is_empty() && online {
                acts.push(Act::Typing(uid));
            }
        });

        let mut animated = false;
        let searching = !needle.is_empty();
        let mut hl = Highlight { current: matches.get(chat.search.cur).map(|i| chat.lines[*i].id), needle, scroll: chat.search.scroll, only: chat.search.only };
        egui::CentralPanel::default().frame(egui::Frame::none().fill(Color32::WHITE).inner_margin(6.0)).show_inside(ui, |ui| {
            egui::ScrollArea::vertical().auto_shrink([false, false]).stick_to_bottom(!searching).show(ui, |ui| {
                if chat.lines.is_empty() {
                    ui.label(RichText::new("Сообщений пока нет. Переписка сохраняется на диске.").weak());
                }
                animated = chat_lines(ui, &chat.lines, emo, my, font, true, true, &mut hl);
            });
        });
        chat.search.scroll = hl.scroll;
        animated
    }
}
