//! All rendering. Mutations that need `&mut App` while data is borrowed are
//! collected as `Act`s and applied after the frame section is drawn.

use crate::app::*;
use crate::config::*;
use crate::emoticons::{segment, Pack, Seg};
use crate::proto::now_unix;
use eframe::egui::{self, load::SizedTexture, Color32, Key, Modifiers, RichText, Sense};
use egui_extras::{Column, TableBuilder};
use std::net::Ipv4Addr;
use std::sync::atomic::Ordering;
use std::time::Duration;

mod more;
mod private;

const VIEWBAR_BG: Color32 = Color32::from_rgb(0x6e, 0x6e, 0x6e);
const HEADER_BG: Color32 = Color32::from_rgb(0x8c, 0x8c, 0x8c);

pub struct SetupState {
    pub page: usize,
    pub draft: Config,
    pub ifaces: Vec<(String, Ipv4Addr)>,
}

#[derive(Clone)]
enum Act {
    Chat(u64),
    Info(u64, InfoTab),
    Signal(Option<u64>),
    File(u64),
    Appeal(u64),
    Group(u64, Option<String>),
    DelGroup(String),
    Select(u64),
    Click(u64, usize),
    CopyIp(u64),
    ClearChat(u64),
    State(u8),
    Nick(String),
    EditState(usize),
    Accept(u64, bool),
    CancelTransfer(u64),
    Kill(u64, u32),
    SendPrivate(u64, String),
    Typing(u64),
    Global(String),
    Page(Page),
    OpenPath(String),
    OpenGroup(u64),
    GroupAdd(u64, u64),
    GroupRemove(u64, u64),
    GroupSend(u64, String),
    NewConv(Vec<u64>),
    RemoteDlg(u64, bool),
    Cmd(u64),
    Power(u64, u8),
    RenameConv(u64),
    DeleteConv(u64),
    /// open a private chat and jump to a found message
    JumpPrivate(u64, u64),
    /// deselect the private chat (back to the dialog list)
    CloseDm,
}

/// Toolbar button with clear feedback: highlight on hover, "pressed in" look while
/// the mouse button is held, dimmed when disabled, framed when `selected`.
pub fn tool_button(ui: &mut egui::Ui, icon: Option<SizedTexture>, text: Option<&str>, h: f32, enabled: bool, selected: bool) -> egui::Response {
    let pad = egui::vec2(5.0, 3.0);
    let isz = icon.map(|t| egui::vec2(t.size.x * h / t.size.y.max(1.0), h)).unwrap_or(egui::Vec2::ZERO);
    let text_color = if enabled { ui.visuals().text_color() } else { ui.visuals().weak_text_color() };
    let galley = text.map(|t| ui.painter().layout_no_wrap(t.to_string(), egui::FontId::proportional(13.5), text_color));
    let gap = if icon.is_some() && galley.is_some() { 5.0 } else { 0.0 };
    let gw = galley.as_ref().map(|g| g.size()).unwrap_or(egui::Vec2::ZERO);
    let size = egui::vec2(pad.x * 2.0 + isz.x + gap + gw.x, pad.y * 2.0 + isz.y.max(gw.y));
    let (rect, resp) = ui.allocate_exact_size(size, if enabled { Sense::click() } else { Sense::hover() });
    if ui.is_rect_visible(rect) {
        let pressed = enabled && resp.is_pointer_button_down_on();
        let hovered = enabled && resp.hovered();
        let (fill, stroke) = if pressed {
            (Some(Color32::from_rgb(0xb8, 0xd8, 0xf5)), Some(Color32::from_rgb(0x2a, 0x78, 0xc8)))
        } else if hovered {
            (Some(Color32::from_rgb(0xe3, 0xf0, 0xfd)), Some(Color32::from_rgb(0x8c, 0xc0, 0xee)))
        } else if selected {
            (Some(Color32::from_rgb(0xd6, 0xe8, 0xf8)), Some(Color32::from_rgb(0x9c, 0xc4, 0xe8)))
        } else {
            (None, None)
        };
        let p = ui.painter();
        if let Some(f) = fill {
            p.rect_filled(rect, 3.0f32, f);
        }
        if let Some(s) = stroke {
            p.rect_stroke(rect, 3.0f32, egui::Stroke::new(1.0f32, s));
        }
        let off = if pressed { egui::vec2(1.0, 1.0) } else { egui::Vec2::ZERO };
        let mut x = rect.min.x + pad.x + off.x;
        if let Some(t) = icon {
            let r = egui::Rect::from_min_size(egui::pos2(x, rect.center().y - isz.y / 2.0 + off.y), isz);
            let tint = if enabled { Color32::WHITE } else { Color32::from_rgba_unmultiplied(255, 255, 255, 80) };
            p.image(t.id, r, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), tint);
            x += isz.x + gap;
        }
        if let Some(g) = galley {
            let pos = egui::pos2(x, rect.center().y - g.size().y / 2.0 + off.y);
            p.galley(pos, g, text_color);
        }
    }
    resp
}

fn img(t: Option<SizedTexture>) -> egui::Image<'static> {
    match t {
        Some(t) => egui::Image::from_texture(t),
        None => egui::Image::from_texture(SizedTexture::new(egui::TextureId::default(), egui::vec2(1.0, 1.0))),
    }
}

fn c32(c: [u8; 3]) -> Color32 {
    Color32::from_rgb(c[0], c[1], c[2])
}

fn header_bar(ui: &mut egui::Ui, icon: Option<SizedTexture>, title: &str, right: &str) {
    header_bar_ex(ui, icon, title, right, |_| {});
}

/// Header with extra widgets on the right (laid out right to left: `extra` first).
fn header_bar_ex(ui: &mut egui::Ui, icon: Option<SizedTexture>, title: &str, right: &str, extra: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::none().fill(HEADER_BG).inner_margin(egui::Margin::symmetric(6.0, 3.0)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            if let Some(i) = icon {
                ui.add(img(Some(i)));
            }
            ui.label(RichText::new(title).color(Color32::WHITE).strong().size(16.0));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                extra(ui);
                ui.label(RichText::new(right).color(Color32::WHITE).size(12.0));
            });
        });
    });
}

/// Magnifier button (painted, so it does not depend on fonts).
pub fn search_icon_button(ui: &mut egui::Ui, active: bool, color: Color32) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(28.0, 24.0), Sense::click());
    if ui.is_rect_visible(rect) {
        let p = ui.painter();
        if active || resp.hovered() {
            let a = if resp.is_pointer_button_down_on() { 90 } else if active { 70 } else { 45 };
            let bg = if color == Color32::WHITE { Color32::from_rgba_unmultiplied(255, 255, 255, a) } else { Color32::from_rgba_unmultiplied(0, 0, 0, a / 3) };
            p.rect_filled(rect, 4.0f32, bg);
        }
        let c = rect.center() + egui::vec2(-2.0, -2.0);
        let stroke = egui::Stroke::new(2.0f32, color);
        p.circle_stroke(c, 6.0, stroke);
        p.line_segment([c + egui::vec2(4.4, 4.4), c + egui::vec2(9.0, 9.0)], egui::Stroke::new(2.6f32, color));
    }
    resp.on_hover_text("Поиск по сообщениям (Ctrl+F)")
}

// ------------------------------------------------------------------ search

/// Lowercase for search; «ё» is treated as «е».
fn lc(c: char) -> char {
    let c = c.to_lowercase().next().unwrap_or(c);
    if c == 'ё' {
        'е'
    } else {
        c
    }
}

pub fn lower_chars(s: &str) -> Vec<char> {
    s.trim().chars().map(lc).collect()
}

/// Byte ranges of case-insensitive occurrences of `needle` (from `lower_chars`) in `hay`.
pub fn find_ci(hay: &str, needle: &[char]) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    if needle.is_empty() {
        return out;
    }
    let hc: Vec<(usize, char)> = hay.char_indices().map(|(i, c)| (i, lc(c))).collect();
    let mut i = 0;
    while i + needle.len() <= hc.len() {
        if hc[i..i + needle.len()].iter().zip(needle).all(|(a, b)| a.1 == *b) {
            let end = hc.get(i + needle.len()).map(|x| x.0).unwrap_or(hay.len());
            out.push((hc[i].0, end));
            i += needle.len();
        } else {
            i += 1;
        }
    }
    out
}

pub fn contains_ci(hay: &str, needle: &[char]) -> bool {
    if needle.is_empty() {
        return true;
    }
    let hc: Vec<char> = hay.chars().map(lc).collect();
    hc.windows(needle.len()).any(|w| w == needle)
}

/// What `chat_lines` should highlight.
#[derive(Default)]
pub struct Highlight {
    pub needle: Vec<char>,
    pub current: Option<u64>,
    pub scroll: bool,
    pub only: bool,
}

/// Indices of the lines that match the search (oldest first).
pub fn search_matches(lines: &[Line], s: &ChatSearch) -> (Vec<char>, Vec<usize>) {
    if !s.active() {
        return (Vec::new(), Vec::new());
    }
    let needle = lower_chars(&s.query);
    let m = lines.iter().enumerate().filter(|(_, l)| l.from != 0 && contains_ci(&l.text, &needle)).map(|(i, _)| i).collect();
    (needle, m)
}

/// Keep the current match valid: a new query starts from the newest match,
/// `jump_to` selects a given message.
pub fn sync_search(s: &mut ChatSearch, lines: &[Line], matches: &[usize]) {
    if s.query != s.last_query {
        s.last_query = s.query.clone();
        s.cur = matches.len().saturating_sub(1);
        s.scroll = !matches.is_empty();
    }
    if let Some(id) = s.jump_to {
        if let Some(k) = matches.iter().position(|i| lines[*i].id == id) {
            s.cur = k;
            s.scroll = true;
            s.jump_to = None;
        } else if !s.active() {
            s.jump_to = None;
        }
    }
    if s.cur >= matches.len() {
        s.cur = matches.len().saturating_sub(1);
    }
}

/// Telegram-like search strip under a chat header: query, "N из M", ▲/▼, close.
/// Enter / ▲ — earlier message, Shift+Enter / ▼ — later one, Esc — close.
pub fn search_bar(ui: &mut egui::Ui, s: &mut ChatSearch, id_src: impl std::hash::Hash, total: usize) {
    egui::Frame::none().fill(Color32::from_rgb(0xee, 0xf3, 0xf8)).inner_margin(egui::Margin::symmetric(6.0, 4.0)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            let id = egui::Id::new(id_src);
            let (mut up, mut down, mut close) = (false, false, false);
            if ui.memory(|m| m.has_focus(id)) {
                ui.input_mut(|i| {
                    if i.consume_key(Modifiers::SHIFT, Key::Enter) || i.consume_key(Modifiers::NONE, Key::ArrowDown) {
                        down = true;
                    }
                    if i.consume_key(Modifiers::NONE, Key::Enter) || i.consume_key(Modifiers::NONE, Key::ArrowUp) {
                        up = true;
                    }
                    if i.consume_key(Modifiers::NONE, Key::Escape) {
                        close = true;
                    }
                });
            }
            let w = (ui.available_width() - 290.0).clamp(120.0, 420.0);
            let r = ui.add(egui::TextEdit::singleline(&mut s.query).id(id).hint_text("🔍 Поиск по сообщениям…").desired_width(w));
            if s.focus {
                r.request_focus();
                s.focus = false;
            }
            let has_q = !s.query.trim().is_empty();
            let status = if !has_q {
                String::new()
            } else if total == 0 {
                "ничего не найдено".to_string()
            } else {
                format!("{} из {}", total - s.cur.min(total - 1), total)
            };
            if ui.add_enabled(total > 0 && s.cur > 0, egui::Button::new("▲")).on_hover_text("Раньше (Enter)").clicked() {
                up = true;
            }
            if ui.add_enabled(total > 0 && s.cur + 1 < total, egui::Button::new("▼")).on_hover_text("Позже (Shift+Enter)").clicked() {
                down = true;
            }
            ui.label(RichText::new(status).color(if has_q && total == 0 { Color32::from_rgb(190, 60, 40) } else { Color32::from_gray(80) }));
            ui.checkbox(&mut s.only, "только найденные").on_hover_text("Показывать только сообщения, в которых есть искомый текст");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("✖").on_hover_text("Закрыть поиск (Esc)").clicked() {
                    close = true;
                }
            });
            if up && s.cur > 0 {
                s.cur -= 1;
                s.scroll = true;
            }
            if down && s.cur + 1 < total {
                s.cur += 1;
                s.scroll = true;
            }
            if close {
                s.toggle();
            }
        });
    });
}

// ------------------------------------------------------------------ chat rendering

#[allow(clippy::too_many_arguments)]
pub fn chat_lines(
    ui: &mut egui::Ui,
    lines: &[Line],
    emo: Option<&Pack>,
    my_uid: u64,
    font: f32,
    show_delivery: bool,
    show_system: bool,
    hl: &mut Highlight,
) -> bool {
    let time = ui.input(|i| i.time);
    let mut animated = false;
    let max_emo_h = (font * 2.2).max(20.0);
    let searching = !hl.needle.is_empty();
    let mut shown = 0usize;
    for l in lines {
        if l.from == 0 && !show_system {
            continue;
        }
        let is_match = searching && l.from != 0 && contains_ci(&l.text, &hl.needle);
        if hl.only && searching && !is_match {
            continue;
        }
        shown += 1;
        let is_cur = is_match && hl.current == Some(l.id);
        let fill = if is_cur {
            Some(Color32::from_rgb(255, 222, 130))
        } else if is_match {
            Some(Color32::from_rgb(255, 247, 214))
        } else {
            None
        };
        let rect = match fill {
            Some(f) => {
                egui::Frame::none()
                    .fill(f)
                    .rounding(3.0)
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        animated |= chat_line(ui, l, emo, my_uid, font, show_delivery, &hl.needle, time, max_emo_h);
                    })
                    .response
                    .rect
            }
            None => {
                let mut a = false;
                let r = ui.scope(|ui| a = chat_line(ui, l, emo, my_uid, font, show_delivery, &[], time, max_emo_h)).response.rect;
                animated |= a;
                r
            }
        };
        if is_cur && hl.scroll {
            ui.scroll_to_rect(rect, Some(egui::Align::Center));
            hl.scroll = false;
        }
    }
    if searching && hl.only && shown == 0 {
        ui.label(RichText::new("Ничего не найдено").weak());
    }
    animated
}

/// One chat line; `needle` (non-empty while searching) is highlighted inside the text.
#[allow(clippy::too_many_arguments)]
fn chat_line(ui: &mut egui::Ui, l: &Line, emo: Option<&Pack>, my_uid: u64, font: f32, show_delivery: bool, needle: &[char], time: f64, max_emo_h: f32) -> bool {
    let mut animated = false;
    {
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            let t = chrono::DateTime::from_timestamp(l.time, 0)
                .map(|d| d.with_timezone(&chrono::Local))
                .map(|d| {
                    if d.date_naive() == chrono::Local::now().date_naive() {
                        d.format("[%H:%M] ").to_string()
                    } else {
                        d.format("[%d.%m %H:%M] ").to_string()
                    }
                })
                .unwrap_or_default();
            ui.label(RichText::new(t).size(font).color(Color32::GRAY));
            if l.from == 0 {
                ui.label(RichText::new(&l.text).size(font).italics().color(Color32::from_rgb(110, 110, 160)));
                return;
            }
            let color = c32(l.color);
            if l.me {
                ui.label(RichText::new(format!("* {} ", l.nick)).size(font).strong().color(color));
            } else {
                ui.label(RichText::new(format!("{}: ", l.nick)).size(font).strong().color(color));
            }
            for s in segment(&l.text, emo, 10) {
                match s {
                    Seg::Text(t) => {
                        let style = |s: &str| {
                            let mut rt = RichText::new(s).size(font);
                            if l.me {
                                rt = rt.color(color).italics();
                            }
                            rt
                        };
                        let found = find_ci(t, needle);
                        if found.is_empty() {
                            ui.label(style(t));
                        } else {
                            let mut pos = 0;
                            for (a, b) in found {
                                if a > pos {
                                    ui.label(style(&t[pos..a]));
                                }
                                ui.label(style(&t[a..b]).background_color(Color32::from_rgb(255, 196, 0)).color(Color32::BLACK));
                                pos = b;
                            }
                            if pos < t.len() {
                                ui.label(style(&t[pos..]));
                            }
                        }
                    }
                    Seg::Url(u) => {
                        if ui.link(RichText::new(u).size(font)).on_hover_text("Открыть").clicked() {
                            let target = if u.starts_with("www.") { format!("http://{u}") } else { u.to_string() };
                            crate::platform::shell_open(&target);
                        }
                    }
                    Seg::Emo(i) => {
                        if let Some(p) = emo {
                            if let Some((tex, size)) = p.texture(i, time) {
                                let sc = if size.y > max_emo_h { max_emo_h / size.y } else { 1.0 };
                                ui.add(egui::Image::from_texture(SizedTexture::new(tex.id(), size * sc)))
                                    .on_hover_text(&p.items[i].tip);
                                animated |= p.items[i].frames.len() > 1;
                            }
                        }
                    }
                }
            }
            if show_delivery && l.from == my_uid {
                if l.delivered && l.pending.is_empty() {
                    ui.label(RichText::new("  ✔").size(font * 0.8).color(Color32::from_rgb(60, 150, 60)))
                        .on_hover_text("Доставлено");
                } else {
                    ui.label(RichText::new("  ⏳").size(font * 0.8).color(Color32::from_rgb(200, 130, 0)))
                        .on_hover_text("Не доставлено — будет отправлено, когда собеседник появится в сети");
                }
            }
        });
    }
    animated
}

/// Byte offset of the last word (after the last whitespace, which may be multi-byte).
fn word_start(s: &str) -> usize {
    s.char_indices().rev().find(|(_, c)| c.is_whitespace()).map(|(p, c)| p + c.len_utf8()).unwrap_or(0)
}

pub struct InputOut {
    pub submit: Option<String>,
    pub typed: bool,
}

/// Multiline chat input with the original's keyboard features:
/// Enter = send, Ctrl+Enter = new line, Ctrl+Up/Down = history, Tab = complete nick,
/// Alt+R = EN<->RU layout revert, Shift+F1..F12 = macro, Shift+Ctrl+F1..F12 = assign macro.
pub fn chat_input(
    ui: &mut egui::Ui,
    input: &mut ChatInput,
    id_src: impl std::hash::Hash,
    emo: Option<&Pack>,
    emo_icon: Option<SizedTexture>,
    macros: &mut Vec<String>,
    nicks: &[String],
    font: f32,
) -> InputOut {
    let id = egui::Id::new(id_src);
    let focused = ui.memory(|m| m.has_focus(id));
    let mut submit = false;
    let before = input.text.clone();
    if focused {
        ui.input_mut(|i| {
            if i.consume_key(Modifiers::NONE, Key::Enter) || i.consume_key(Modifiers::SHIFT, Key::Enter) {
                submit = true;
            }
            if i.consume_key(Modifiers::CTRL, Key::ArrowUp) && !input.history.is_empty() {
                let p = input.hist_pos.map(|p| p.saturating_sub(1)).unwrap_or(input.history.len() - 1);
                input.hist_pos = Some(p);
                input.text = input.history[p].clone();
            }
            if i.consume_key(Modifiers::CTRL, Key::ArrowDown) {
                if let Some(p) = input.hist_pos {
                    if p + 1 < input.history.len() {
                        input.hist_pos = Some(p + 1);
                        input.text = input.history[p + 1].clone();
                    } else {
                        input.hist_pos = None;
                        input.text.clear();
                    }
                }
            }
            if i.consume_key(Modifiers::NONE, Key::Tab) {
                let start = word_start(&input.text);
                let word = input.text[start..].to_lowercase();
                if !word.is_empty() {
                    if let Some(n) = nicks.iter().find(|n| n.to_lowercase().starts_with(&word)) {
                        let suffix = if start == 0 { ", " } else { " " };
                        input.text.truncate(start);
                        input.text.push_str(n);
                        input.text.push_str(suffix);
                    }
                }
            }
            if i.consume_key(Modifiers::ALT, Key::R) {
                input.text = crate::platform::revert_layout(&input.text);
            }
            let fkeys = [Key::F1, Key::F2, Key::F3, Key::F4, Key::F5, Key::F6, Key::F7, Key::F8, Key::F9, Key::F10, Key::F11, Key::F12];
            for (n, k) in fkeys.iter().enumerate() {
                if i.consume_key(Modifiers::SHIFT | Modifiers::CTRL, *k) {
                    macros[n] = input.text.clone();
                } else if i.consume_key(Modifiers::SHIFT, *k) {
                    input.text.push_str(&macros[n]);
                }
            }
        });
    }
    let mut out = InputOut { submit: None, typed: false };
    // Buttons are laid out FIRST (right to left), then the text box takes exactly the
    // remaining width. Sizing the box as "available - constant" made the row wider than
    // the window, and a resizable egui::Window then grew a bit every frame.
    ui.horizontal(|ui| {
        const COL: f32 = 38.0;
        let w = (ui.available_width() - COL - ui.spacing().item_spacing.x).max(40.0);
        let te = egui::TextEdit::multiline(&mut input.text)
            .id(id)
            .desired_rows(2)
            .desired_width(f32::INFINITY)
            .font(egui::FontId::proportional(font))
            .return_key(egui::KeyboardShortcut::new(Modifiers::CTRL, Key::Enter))
            .hint_text("Enter — отправить, Ctrl+Enter — новая строка, Tab — ник");
        // the box is confined to exactly `w` points
        let r = ui
            .allocate_ui_with_layout(egui::vec2(w, ui.available_height().max(40.0)), egui::Layout::top_down_justified(egui::Align::Min), |ui| {
                ui.set_max_width(w);
                ui.add(te)
            })
            .inner;
        if input.focus {
            r.request_focus();
            input.focus = false;
        }
        ui.allocate_ui_with_layout(egui::vec2(COL, 70.0), egui::Layout::top_down(egui::Align::Center), |ui| {
            ui.set_max_width(COL);
            if let Some(p) = emo {
                let btn = egui::Button::image(img(emo_icon));
                egui::menu::menu_custom_button(ui, btn, |ui| {
                    ui.set_max_width(420.0);
                    let time = ui.input(|i| i.time);
                    egui::ScrollArea::vertical().max_height(300.0).show(ui, |ui| {
                        ui.horizontal_wrapped(|ui| {
                            for (i, e) in p.items.iter().enumerate() {
                                if let Some((tex, size)) = p.texture(i, time) {
                                    let sc = if size.y > 32.0 { 32.0 / size.y } else { 1.0 };
                                    let b = egui::ImageButton::new(egui::Image::from_texture(SizedTexture::new(tex.id(), size * sc)));
                                    if ui.add(b).on_hover_text(format!("{}   {}", e.tip, e.codes.join(" "))).clicked() {
                                        if !input.text.is_empty() && !input.text.ends_with(' ') {
                                            input.text.push(' ');
                                        }
                                        input.text.push_str(&e.codes[0]);
                                        input.text.push(' ');
                                        input.focus = true;
                                        if !ui.input(|i| i.modifiers.ctrl) {
                                            ui.close_menu();
                                        }
                                    }
                                }
                            }
                        });
                    });
                    ui.ctx().request_repaint_after(Duration::from_millis(80));
                })
                .response
                .on_hover_text("Вставить смайл (Ctrl+клик — не закрывать)");
            }
            if ui.button("➤").on_hover_text("Отправить (Enter)").clicked() {
                submit = true;
            }
        });
    });
    if input.text != before {
        out.typed = true;
    }
    if submit {
        let t = input.text.trim_end().to_string();
        if !t.trim().is_empty() {
            if input.history.last() != Some(&t) {
                input.history.push(t.clone());
                if input.history.len() > 100 {
                    input.history.remove(0);
                }
            }
            out.submit = Some(t);
        }
        input.text.clear();
        input.hist_pos = None;
        input.focus = true;
    }
    out
}

// ------------------------------------------------------------------ main update

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.frame(ctx);
    }
}

impl App {
    /// One UI pass of the main window (shared by the OpenGL and the software renderer).
    pub fn frame(&mut self, ctx: &egui::Context) {
        if let Some(t) = &self.tray {
            if t.show_requested.swap(false, Ordering::Relaxed) {
                self.visible = true;
                self.minimized = false;
            }
            if t.quit_requested.swap(false, Ordering::Relaxed) {
                self.quit = true;
            }
        }
        if self.start_hidden && ctx.cumulative_pass_nr() > 2 {
            self.start_hidden = false;
            self.hide_main();
        }
        if !self.visible {
            // keep the tray mode applied (eframe/winit may restore the window style)
            if let Some(h) = self.tray.as_ref().map(|t| t.hwnd.load(Ordering::Relaxed)).filter(|h| *h != 0) {
                if !crate::platform::is_stealth(h) {
                    crate::platform::set_stealth(h, true);
                }
            }
        }
        self.poll_net();
        self.tick();

        let (focused, minimized, close, outer) = ctx.input(|i| {
            let v = i.viewport();
            (v.focused.unwrap_or(true), v.minimized.unwrap_or(false), v.close_requested(), v.outer_rect)
        });
        if focused && !self.focused {
            self.tips.clear();
        }
        self.focused = focused;
        self.minimized = minimized;
        self.update_alert();
        if let Some(r) = outer {
            if self.visible && !minimized && r.width() > 100.0 {
                self.main_pos = Some(r.min);
                let w = [r.min.x, r.min.y, r.width(), r.height()];
                if self.cfg.window.map_or(true, |o| o.iter().zip(w.iter()).any(|(a, b)| (a - b).abs() > 1.0)) {
                    self.cfg.window = Some(w);
                    self.cfg_dirty = true;
                }
            }
        }
        if close && !self.quit {
            if self.cfg.general.close_to_tray && self.tray.is_some() {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                self.hide_main();
            } else {
                self.quit = true;
            }
        }
        if self.quit {
            self.shutdown();
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }
        ctx.set_zoom_factor(self.cfg.general.ui_scale);

        let mut acts: Vec<Act> = Vec::new();
        self.shortcuts(ctx, &mut acts);
        self.menu_and_toolbar(ctx, &mut acts);
        self.status_bar(ctx);
        self.viewbar(ctx, &mut acts);
        if self.cfg.users_panel && self.page != Page::Users {
            egui::SidePanel::right("users_panel").resizable(true).default_width(190.0).show(ctx, |ui| {
                header_bar(ui, self.icons.get("hd_users"), "Пользователи", &format!("{}", self.peers.len() + 1));
                self.users_compact(ui, &mut acts);
            });
        }
        let mut animated = false;
        egui::CentralPanel::default().frame(egui::Frame::central_panel(&ctx.style()).inner_margin(2.0)).show(ctx, |ui| match self.page {
            Page::Users => self.users_page(ui, &mut acts),
            Page::Chat => animated |= self.global_page(ui, &mut acts),
            Page::Private => animated |= self.private_page(ui, &mut acts),
            Page::Groups => animated |= self.groups_page(ui, &mut acts),
            Page::Events => self.events_page(ui),
        });
        self.info_windows(ctx, &mut acts);
        self.transfers_window(ctx, &mut acts);
        self.setup_window(ctx);
        self.dialogs(ctx, &mut acts);
        self.remote_dialogs(ctx, &mut acts);
        self.viewer_windows(ctx, &mut acts);
        self.host_popups(ctx);
        self.tips_viewport(ctx);
        self.apply(acts);

        if let Some(t) = &self.tray {
            let unread: u32 = self.chats.values().map(|c| c.unread).sum::<u32>() + self.groups.values().map(|c| c.unread).sum::<u32>();
            t.set_tooltip(&format!(
                "Nassi RS — {} [{}]{}",
                self.cfg.nick,
                self.cfg.cur_state().title,
                if unread > 0 { format!(" — непрочитанных: {unread}") } else { String::new() }
            ));
        }
        let anim = animated && self.emo.as_ref().is_some_and(|e| e.is_animated());
        // live clocks in the users list tick every second; viewers repaint on frames
        ctx.request_repaint_after(Duration::from_millis(if anim { 70 } else { 250 }));
    }
}

impl App {
    fn apply(&mut self, acts: Vec<Act>) {
        for a in acts {
            match a {
                Act::Chat(u) => self.open_chat(u),
                Act::Info(u, t) => self.open_info(u, t),
                Act::Signal(u) => self.signal(u),
                Act::File(u) => self.offer_file(u),
                Act::Appeal(u) => self.appeal(u),
                Act::Group(u, g) => {
                    match g {
                        Some(g) => self.cfg.user_groups.insert(u, g),
                        None => self.cfg.user_groups.remove(&u),
                    };
                    self.cfg_dirty = true;
                }
                Act::DelGroup(g) => {
                    self.cfg.groups.retain(|x| *x != g);
                    self.cfg.user_groups.retain(|_, x| *x != g);
                    self.cfg_dirty = true;
                }
                Act::Select(u) => self.selected = Some(u),
                Act::Click(u, n) => {
                    self.selected = Some(u);
                    let a = self.cfg.general.click_actions[n];
                    match a {
                        ClickAction::None => {}
                        ClickAction::PrivateChat => self.open_chat(u),
                        ClickAction::Info => self.open_info(u, InfoTab::Details),
                        ClickAction::Appeal => self.appeal(u),
                        ClickAction::Signal => self.signal(Some(u)),
                        ClickAction::SendFile => self.offer_file(u),
                    }
                }
                Act::CopyIp(u) => {
                    if let Some(p) = self.peers.get(&u) {
                        let ip = p.ip.to_string();
                        self.ctx.copy_text(ip.clone());
                        self.flash(format!("IP {ip} скопирован"));
                    }
                }
                Act::ClearChat(u) => self.dlg.clear_chat = Some(u),
                Act::State(s) => {
                    self.set_state(s, false);
                }
                Act::Nick(n) => self.set_nick(&n),
                Act::EditState(i) => self.dlg.state_edit = Some((i, self.cfg.states[i].clone())),
                Act::Accept(id, a) => self.accept_file(id, a),
                Act::CancelTransfer(id) => {
                    if let Some(t) = self.transfers.iter().find(|t| t.id == id) {
                        t.cancel.store(true, Ordering::Relaxed);
                        if !t.incoming {
                            self.shared.lock().unwrap().files.remove(&id);
                        }
                    }
                    if let Some(t) = self.transfers.iter_mut().find(|t| t.id == id && !t.incoming) {
                        t.state = TState::Failed("Отменено".into());
                    }
                }
                Act::Kill(u, pid) => self.kill_remote(u, pid),
                Act::SendPrivate(u, t) => self.send_private(u, t),
                Act::Typing(u) => self.send_typing(u),
                Act::Global(t) => self.global_command(&t),
                Act::Page(p) => {
                    self.page = p;
                    if p == Page::Chat {
                        self.global_unread = false;
                        self.global_input.focus = true;
                    }
                    if p == Page::Events {
                        self.events_unread = false;
                    }
                    if p == Page::Groups {
                        if let Some(g) = self.group_sel {
                            self.open_group(g);
                        }
                    }
                    if p == Page::Private {
                        if let Some(u) = self.private_sel {
                            self.open_chat(u);
                        }
                    }
                }
                Act::CloseDm => self.private_sel = None,
                Act::JumpPrivate(u, id) => {
                    self.open_chat(u);
                    let q = self.dm_filter.trim().to_string();
                    let c = self.chat_mut(u);
                    c.search.open = true;
                    c.search.query = q;
                    c.search.jump_to = Some(id);
                    c.input.focus = false;
                }
                Act::OpenPath(p) => crate::platform::shell_open(&p),
                Act::OpenGroup(g) => self.open_group(g),
                Act::GroupAdd(g, u) => self.group_add(g, u),
                Act::GroupRemove(g, u) => self.group_remove(g, u),
                Act::GroupSend(g, t) => {
                    if let Some(rest) = t.strip_prefix("/me ") {
                        self.send_group(g, rest.to_string(), true);
                    } else {
                        self.send_group(g, t, false);
                    }
                }
                Act::NewConv(members) => {
                    self.dlg.new_conv = Some((String::new(), members));
                }
                Act::RemoteDlg(u, view_only) => {
                    self.dlg.connect = Some(ConnectDlg { uid: u, view_only, password: String::new() });
                }
                Act::Cmd(u) => {
                    self.cmds.entry(u).or_default().open = true;
                }
                Act::Power(u, a) => self.dlg.power = Some((u, a, String::new())),
                Act::RenameConv(g) => {
                    let t = self.groups.get(&g).map(|c| c.info.title.clone()).unwrap_or_default();
                    self.dlg.rename_conv = Some((g, t));
                }
                Act::DeleteConv(g) => {
                    self.delete_group(g);
                    self.flash("Беседа удалена из списка");
                }
            }
        }
    }

    fn shortcuts(&mut self, ctx: &egui::Context, acts: &mut Vec<Act>) {
        let text_focus = ctx.memory(|m| m.focused().is_some());
        // NB: nothing inside `input_mut` may touch the egui context (it is locked) —
        // only record what to do and act afterwards.
        let (mut refresh, mut setup, mut details, mut hide, mut find) = (false, false, false, false, false);
        let states_len = self.cfg.states.len();
        let has_tray = self.tray.is_some();
        ctx.input_mut(|i| {
            if i.consume_key(Modifiers::NONE, Key::F5) {
                refresh = true;
            }
            if i.consume_key(Modifiers::NONE, Key::F4) {
                acts.push(Act::Signal(None));
            }
            if i.consume_key(Modifiers::ALT, Key::F1) {
                acts.push(Act::Page(Page::Users));
            }
            if i.consume_key(Modifiers::ALT, Key::F2) {
                acts.push(Act::Page(Page::Chat));
            }
            if i.consume_key(Modifiers::ALT, Key::F3) {
                acts.push(Act::Page(Page::Groups));
            }
            if i.consume_key(Modifiers::ALT, Key::F5) {
                acts.push(Act::Page(Page::Events));
            }
            if i.consume_key(Modifiers::ALT, Key::F6) {
                acts.push(Act::Page(Page::Private));
            }
            for (k, p) in [(Key::Num1, Page::Users), (Key::Num2, Page::Chat), (Key::Num3, Page::Private), (Key::Num4, Page::Groups), (Key::Num5, Page::Events)] {
                if i.consume_key(Modifiers::ALT, k) {
                    acts.push(Act::Page(p));
                }
            }
            if i.consume_key(Modifiers::CTRL, Key::F) {
                find = true;
            }
            if i.consume_key(Modifiers::ALT, Key::S) {
                setup = true;
            }
            if i.consume_key(Modifiers::ALT, Key::M) {
                details = true;
            }
            if i.consume_key(Modifiers::ALT, Key::D) {
                self.quit = true;
            }
            if !text_focus {
                let keys = [Key::R, Key::W, Key::D, Key::P, Key::A, Key::O];
                for (n, k) in keys.iter().enumerate() {
                    if i.consume_key(Modifiers::CTRL, *k) {
                        acts.push(Act::State(n as u8));
                    }
                }
                let nums = [Key::Num1, Key::Num2, Key::Num3, Key::Num4, Key::Num5, Key::Num6];
                for (n, k) in nums.iter().enumerate() {
                    if i.consume_key(Modifiers::CTRL, *k) && 6 + n < states_len {
                        acts.push(Act::State((6 + n) as u8));
                    }
                }
                if has_tray && i.consume_key(Modifiers::NONE, Key::Escape) {
                    hide = true;
                }
            }
        });
        if refresh {
            self.refresh_users();
        }
        if find {
            match self.page {
                Page::Users => self.users_filter_focus = true,
                Page::Chat => self.global_search.start(),
                Page::Private => match self.private_sel {
                    Some(u) => self.chat_mut(u).search.start(),
                    None => self.dm_filter_focus = true,
                },
                Page::Groups => {
                    if let Some(c) = self.group_sel.and_then(|g| self.groups.get_mut(&g)) {
                        c.search.start();
                    }
                }
                Page::Events => {}
            }
        }
        if setup && self.dlg.setup.is_none() {
            self.open_setup();
        }
        if details {
            let d = self.cfg.details.clone();
            let t = avatar_texture(&self.ctx, self.cfg.uid, &d.picture_png);
            self.dlg.details_edit = Some((d, t));
        }
        if hide {
            self.hide_main();
        }
    }

    fn state_menu(&mut self, ui: &mut egui::Ui, acts: &mut Vec<Act>) {
        let keys = ["Ctrl+R", "Ctrl+W", "Ctrl+D", "Ctrl+P", "Ctrl+A", "Ctrl+O"];
        for (i, s) in self.cfg.states.iter().enumerate() {
            let icon = self.icons.get(self.icons.state(i as u8));
            let text = if i < 6 { format!("{}    {}", s.title, keys[i]) } else { format!("{}    Ctrl+{}", s.title, i - 5) };
            let b = egui::Button::image_and_text(img(icon), text).selected(self.cfg.state as usize == i);
            if ui.add(b).clicked() {
                acts.push(Act::State(i as u8));
                ui.close_menu();
            }
        }
        ui.separator();
        ui.menu_button("Настроить состояние", |ui| {
            for (i, s) in self.cfg.states.iter().enumerate() {
                if ui.button(&s.title).clicked() {
                    acts.push(Act::EditState(i));
                    ui.close_menu();
                }
            }
        });
        if self.cfg.states.len() < 6 + MAX_CUSTOM_STATES && ui.button("Новое состояние…").clicked() {
            self.dlg.state_edit = Some((usize::MAX, StateDef { title: "Новое состояние".into(), ..Default::default() }));
            ui.close_menu();
        }
    }

    fn nick_menu(&mut self, ui: &mut egui::Ui, acts: &mut Vec<Act>) {
        for n in &self.cfg.nicks {
            if ui.selectable_label(*n == self.cfg.nick, n).clicked() {
                acts.push(Act::Nick(n.clone()));
                ui.close_menu();
            }
        }
        ui.separator();
        if ui.button("Новый ник…").clicked() {
            self.dlg.nick_edit = Some(String::new());
            ui.close_menu();
        }
        if ui.button(format!("Ник по умолчанию ({})", self.os_user)).clicked() {
            acts.push(Act::Nick(self.os_user.clone()));
            ui.close_menu();
        }
        ui.separator();
        ui.horizontal(|ui| {
            ui.label("Цвет в чате:");
            if ui.color_edit_button_srgb(&mut self.cfg.color).changed() {
                self.cfg_dirty = true;
                self.last_presence = std::time::Instant::now() - Duration::from_secs(14);
            }
        });
    }

    fn user_menu(&self, ui: &mut egui::Ui, uid: u64, acts: &mut Vec<Act>) {
        let me = uid == self.cfg.uid;
        let ic = |n: &str| img(self.icons.sized(n, 16.0));
        let mut item = |ui: &mut egui::Ui, icon: &str, text: &str, a: Act| {
            if ui.add(egui::Button::image_and_text(ic(icon), text)).clicked() {
                acts.push(a);
                ui.close_menu();
            }
        };
        let online = self.peers.contains_key(&uid);
        if !me {
            item(ui, "tb_chat", "Написать личное сообщение", Act::Chat(uid));
            item(ui, "tb_signal", "Сигнал", Act::Signal(Some(uid)));
            item(ui, "tb_file", "Отправить файл…", Act::File(uid));
            item(ui, "hd_chat", "Обращение в общем чате", Act::Appeal(uid));
            ui.menu_button("👥 Пригласить в беседу", |ui| {
                if ui.button("➕ Новая беседа…").clicked() {
                    acts.push(Act::NewConv(vec![uid]));
                    ui.close_menu();
                }
                let mine: Vec<&Conv> = self.groups.values().filter(|c| c.is_member(self.cfg.uid) && !c.is_member(uid)).collect();
                if !mine.is_empty() {
                    ui.separator();
                }
                for c in mine {
                    if ui.button(&c.info.title).clicked() {
                        acts.push(Act::GroupAdd(c.info.id, uid));
                        ui.close_menu();
                    }
                }
            });
            ui.separator();
            let mode = self.peers.get(&uid).map(|p| p.p.allow.remote).unwrap_or(0);
            ui.add_enabled_ui(online && mode != 0, |ui| {
                ui.menu_button("🖥 Удалённый доступ", |ui| {
                    if ui.button("🖱 Управление рабочим столом…").clicked() {
                        acts.push(Act::RemoteDlg(uid, false));
                        ui.close_menu();
                    }
                    if ui.button("👁 Только просмотр…").clicked() {
                        acts.push(Act::RemoteDlg(uid, true));
                        ui.close_menu();
                    }
                    if ui.button("⌨ Командная строка…").clicked() {
                        acts.push(Act::Cmd(uid));
                        ui.close_menu();
                    }
                    ui.menu_button("⏻ Питание", |ui| {
                        for (a, n) in [(3u8, "Заблокировать"), (2, "Выйти из системы"), (1, "Перезагрузить"), (0, "Выключить")] {
                            if ui.button(n).clicked() {
                                acts.push(Act::Power(uid, a));
                                ui.close_menu();
                            }
                        }
                    });
                    ui.separator();
                    ui.label(
                        RichText::new("🔑 Каждое действие требует пароль администратора")
                        .small()
                        .weak(),
                    );
                });
            })
            .response
            .on_disabled_hover_text(if online { "Удалённый доступ на этом компьютере отключён" } else { "Пользователь не в сети" });
            ui.separator();
        }
        ui.menu_button("Информация", |ui| {
            for (t, name) in [
                (InfoTab::Details, "Данные пользователя"),
                (InfoTab::Processes, "Процессы"),
                (InfoTab::Screenshot, "Снимок экрана"),
                (InfoTab::Clipboard, "Буфер обмена"),
            ] {
                if ui.button(name).clicked() {
                    acts.push(Act::Info(uid, t));
                    ui.close_menu();
                }
            }
        });
        if self.cfg.groups_enabled {
            ui.menu_button("Группа", |ui| {
                let cur = self.cfg.user_groups.get(&uid);
                for g in &self.cfg.groups {
                    if ui.selectable_label(cur == Some(g), g).clicked() {
                        acts.push(Act::Group(uid, Some(g.clone())));
                        ui.close_menu();
                    }
                }
                if ui.selectable_label(cur.is_none(), "Без группы").clicked() {
                    acts.push(Act::Group(uid, None));
                    ui.close_menu();
                }
            });
        }
        if !me {
            if self.chats.get(&uid).is_some_and(|c| !c.lines.is_empty()) && ui.button("Очистить историю личных сообщений").clicked() {
                acts.push(Act::ClearChat(uid));
                ui.close_menu();
            }
            if ui.button("Копировать IP-адрес").clicked() {
                acts.push(Act::CopyIp(uid));
                ui.close_menu();
            }
        }
    }

    fn menu_and_toolbar(&mut self, ctx: &egui::Context, acts: &mut Vec<Act>) {
        egui::TopBottomPanel::top("menu").show(ctx, |ui| {
            egui::menu::bar(ui, |ui| {
                ui.menu_button("Файл", |ui| {
                    if ui.button("Мои данные…    Alt+M").clicked() {
                        let d = self.cfg.details.clone();
                        let t = avatar_texture(&self.ctx, self.cfg.uid, &d.picture_png);
                        self.dlg.details_edit = Some((d, t));
                        ui.close_menu();
                    }
                    ui.menu_button("Состояние", |ui| self.state_menu(ui, acts));
                    ui.menu_button("Ник", |ui| self.nick_menu(ui, acts));
                    ui.separator();
                    if ui.button("Настройка…    Alt+S").clicked() {
                        self.open_setup();
                        ui.close_menu();
                    }
                    ui.separator();
                    if self.tray.is_some() && ui.button("Свернуть в трей    Esc").clicked() {
                        self.hide_main();
                        ui.close_menu();
                    }
                    if ui.button("Выход    Alt+D").clicked() {
                        self.quit = true;
                        ui.close_menu();
                    }
                });
                ui.menu_button("Вид", |ui| {
                    for (p, n) in [
                        (Page::Users, "Пользователи    Alt+F1"),
                        (Page::Chat, "Общий чат    Alt+F2"),
                        (Page::Private, "Личные сообщения    Alt+F6"),
                        (Page::Groups, "Групповой чат    Alt+F3"),
                        (Page::Events, "Журнал событий    Alt+F5"),
                    ] {
                        if ui.selectable_label(self.page == p, n).clicked() {
                            acts.push(Act::Page(p));
                            ui.close_menu();
                        }
                    }
                    ui.separator();
                    if ui.checkbox(&mut self.cfg.users_panel, "Панель пользователей").changed() {
                        self.cfg_dirty = true;
                    }
                    if ui.checkbox(&mut self.cfg.groups_enabled, "Группы пользователей").changed() {
                        self.cfg_dirty = true;
                    }
                    if ui.button("Новая группа…").clicked() {
                        self.dlg.new_group = Some(String::new());
                        ui.close_menu();
                    }
                    ui.separator();
                    ui.horizontal(|ui| {
                        ui.label("Масштаб:");
                        for s in [0.9f32, 1.0, 1.15, 1.3, 1.5] {
                            if ui.selectable_label((self.cfg.general.ui_scale - s).abs() < 0.01, format!("{:.0}%", s * 100.0)).clicked() {
                                self.cfg.general.ui_scale = s;
                                self.cfg_dirty = true;
                            }
                        }
                    });
                });
                let sel = self.selected.filter(|u| *u != self.cfg.uid && self.peers.contains_key(u));
                ui.add_enabled_ui(sel.is_some(), |ui| {
                    ui.menu_button("Пользователь", |ui| {
                        if let Some(u) = sel {
                            self.user_menu(ui, u, acts);
                        }
                    });
                });
                ui.menu_button("Действия", |ui| {
                    if ui.button("Обновить список пользователей    F5").clicked() {
                        self.refresh_users();
                        ui.close_menu();
                    }
                    if ui.button("Сигнал всем    F4").clicked() {
                        acts.push(Act::Signal(None));
                        ui.close_menu();
                    }
                    ui.separator();
                    if ui.button("Передачи файлов…").clicked() {
                        self.dlg.transfers = true;
                        ui.close_menu();
                    }
                    if ui.button("Папка принятых файлов").clicked() {
                        let _ = std::fs::create_dir_all(&self.paths.downloads);
                        acts.push(Act::OpenPath(self.paths.downloads.display().to_string()));
                        ui.close_menu();
                    }
                    if ui.button("Папка данных (история, настройки)").clicked() {
                        acts.push(Act::OpenPath(self.paths.data.display().to_string()));
                        ui.close_menu();
                    }
                });
                ui.menu_button("Справка", |ui| {
                    if ui.button("Клавиши и команды чата").clicked() {
                        self.dlg.help = true;
                        ui.close_menu();
                    }
                    if ui.button("О программе").clicked() {
                        self.dlg.about = true;
                        ui.close_menu();
                    }
                });
            });
        });

        egui::TopBottomPanel::top("toolbar").show(ctx, |ui| {
            ui.add_space(2.0);
            ui.horizontal(|ui| {
                let st_icon = self.icons.sized(self.icons.state(self.cfg.state), 22.0);
                let b = egui::Button::image_and_text(img(st_icon), format!("{} ⏷", self.cfg.cur_state().title));
                egui::menu::menu_custom_button(ui, b, |ui| self.state_menu(ui, acts)).response.on_hover_text("Состояние");
                let b = egui::Button::image_and_text(img(self.icons.sized("tb_nick", 22.0)), format!("{} ⏷", self.cfg.nick));
                egui::menu::menu_custom_button(ui, b, |ui| self.nick_menu(ui, acts)).response.on_hover_text("Ник");
                ui.separator();
                let sel = self.selected.filter(|u| *u != self.cfg.uid && self.peers.contains_key(u));
                let icons = &self.icons;
                let mut tb = |ui: &mut egui::Ui, icon: &str, tip: &str, enabled: bool, a: Act| {
                    let r = tool_button(ui, icons.sized(icon, 24.0), None, 24.0, enabled, false);
                    if r.on_hover_text(tip).on_disabled_hover_text(format!("{tip} — сначала выберите пользователя в списке")).clicked() {
                        acts.push(a);
                    }
                };
                let s = sel.unwrap_or(0);
                tb(ui, "tb_chat", "Написать личное сообщение", sel.is_some(), Act::Chat(s));
                tb(ui, "tb_signal", "Сигнал (F4 — всем)", sel.is_some(), Act::Signal(Some(s)));
                tb(ui, "tb_file", "Отправить файл", sel.is_some(), Act::File(s));
                tb(ui, "tb_info", "Информация о пользователе", self.selected.is_some(), Act::Info(self.selected.unwrap_or(0), InfoTab::Details));
                let remote_ok = sel.and_then(|u| self.peers.get(&u)).is_some_and(|p| p.p.allow.remote != 0);
                let r = tool_button(ui, None, Some("🖥 Подключиться"), 22.0, remote_ok, false)
                    .on_hover_text("Удалённое управление рабочим столом выбранного пользователя")
                    .on_disabled_hover_text("Выберите пользователя, у которого разрешён удалённый доступ");
                if r.clicked() {
                    acts.push(Act::RemoteDlg(s, false));
                }
                ui.separator();
                let refreshing = self.refresh_deadline.is_some();
                let r = tool_button(ui, self.icons.sized("tb_refresh", 22.0), Some(if refreshing { "Обновление…" } else { "Обновить" }), 22.0, !refreshing, refreshing);
                if r.on_hover_text("Обновить список пользователей (F5)").clicked() {
                    self.refresh_users();
                }
                if refreshing {
                    ui.spinner();
                }
                if self.page == Page::Events {
                    ui.separator();
                    if tool_button(ui, self.icons.sized("tb_delete", 22.0), Some("Очистить журнал"), 22.0, !self.events.is_empty(), false).clicked() {
                        self.events.clear();
                        self.flash("Журнал очищен");
                    }
                }
                if self.page == Page::Groups {
                    ui.separator();
                    if tool_button(ui, self.icons.sized("p_add", 22.0), Some("Новая беседа"), 22.0, true, false).clicked() {
                        acts.push(Act::NewConv(Vec::new()));
                    }
                }
            });
            ui.add_space(2.0);
        });
    }

    fn status_bar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::bottom("status").show(ctx, |ui| {
            ui.horizontal(|ui| {
                match &self.net {
                    Some(n) => {
                        let ok = !n.bound_ip.is_unspecified();
                        let txt = format!("● {}:{}", if ok { n.bound_ip.to_string() } else { "0.0.0.0".into() }, n.port);
                        ui.label(RichText::new(txt).color(if ok { Color32::from_rgb(40, 150, 40) } else { Color32::from_rgb(200, 120, 0) }))
                            .on_hover_text(format!(
                                "Интерфейс закреплён в настройках: {}\nШироковещательная рассылка: 255.255.255.255:{}\nTCP-порт: {}",
                                self.cfg.conn.bind_ip, n.port, n.tcp_port
                            ));
                    }
                    None => {
                        ui.label(RichText::new("● сеть не запущена").color(Color32::RED));
                    }
                }
                ui.separator();
                ui.label(format!("В сети: {}", self.peers.len() + 1));
                ui.separator();
                ui.label(format!("Состояние: {}", self.cfg.cur_state().title));
                let active = self.transfers.iter().filter(|t| matches!(t.state, TState::Running | TState::Offered | TState::Waiting)).count();
                if active > 0 {
                    ui.separator();
                    if ui.link(format!("Передачи файлов: {active}")).clicked() {
                        self.dlg.transfers = true;
                    }
                }
                let pending: usize =
                    self.chats.values().map(|c| c.pending_count()).sum::<usize>() + self.groups.values().map(|c| c.pending_count()).sum::<usize>();
                if pending > 0 {
                    ui.separator();
                    ui.label(RichText::new(format!("⏳ недоставленных: {pending}")).color(Color32::from_rgb(200, 130, 0)))
                        .on_hover_text("Будут доставлены автоматически, когда получатель появится в сети");
                }
                if !self.viewers.is_empty() {
                    ui.separator();
                    ui.label(RichText::new(format!("🖥 сеансов: {}", self.viewers.len())).color(Color32::from_rgb(30, 90, 170)));
                }
                if let Some((msg, t)) = &self.flash {
                    let age = t.elapsed().as_secs_f32();
                    if age < 5.0 {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            // fade out during the last second
                            let a = ((5.0 - age).min(1.0) * 255.0) as u8;
                            egui::Frame::none()
                                .fill(Color32::from_rgba_unmultiplied(255, 244, 190, a))
                                .rounding(3.0)
                                .inner_margin(egui::Margin::symmetric(6.0, 1.0))
                                .show(ui, |ui| {
                                    ui.label(RichText::new(msg).color(Color32::from_rgba_unmultiplied(60, 50, 0, a)));
                                });
                        });
                    }
                }
            });
        });
    }

    fn viewbar(&mut self, ctx: &egui::Context, acts: &mut Vec<Act>) {
        egui::SidePanel::left("viewbar")
            .resizable(false)
            .exact_width(92.0)
            .frame(egui::Frame::none().fill(VIEWBAR_BG).inner_margin(egui::Margin::symmetric(4.0, 8.0)))
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden).show(ui, |ui| {
                ui.vertical_centered(|ui| {
                    let blink = (ui.input(|i| i.time) * 2.0) as i64 % 2 == 0;
                    let unread_chat = self.global_unread && self.page != Page::Chat;
                    let unread_ev = self.events_unread && self.page != Page::Events;
                    let unread_groups = self.groups.values().any(|c| c.unread > 0);
                    let unread_private: u32 = self.chats.values().map(|c| c.unread).sum();
                    let uv = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
                    for (page, icon, label, flag, count) in [
                        (Page::Users, "vb_users", "Пользователи", false, 0u32),
                        (Page::Chat, "vb_chat", "Общий чат", unread_chat, 0),
                        (Page::Private, "tb_message", "Личные\nсообщения", unread_private > 0, unread_private),
                        (Page::Groups, "vb_chat", "Групповой\nчат", unread_groups, self.groups.values().map(|c| c.unread).sum()),
                        (Page::Events, "vb_eventlog", "Журнал\nсобытий", unread_ev, 0),
                    ] {
                        let sel = self.page == page;
                        let (rect, resp) = ui.allocate_exact_size(egui::vec2(84.0, 80.0), Sense::click());
                        let pressed = resp.is_pointer_button_down_on();
                        let r2 = rect.shrink(2.0);
                        if pressed {
                            ui.painter().rect_filled(r2, 3.0f32, Color32::from_rgb(0x5a, 0x5a, 0x5a));
                            ui.painter().rect_stroke(r2, 3.0f32, egui::Stroke::new(1.0f32, Color32::from_gray(230)));
                        } else if sel {
                            ui.painter().rect_filled(r2, 3.0f32, Color32::from_rgb(0x88, 0x88, 0x88));
                            ui.painter().rect_stroke(r2, 3.0f32, egui::Stroke::new(1.0f32, Color32::from_gray(200)));
                        } else if resp.hovered() {
                            ui.painter().rect_filled(r2, 3.0f32, Color32::from_rgb(0x7a, 0x7a, 0x7a));
                            ui.painter().rect_stroke(r2, 3.0f32, egui::Stroke::new(1.0f32, Color32::from_gray(160)));
                        }
                        let off = if pressed { egui::vec2(1.0, 1.0) } else { egui::Vec2::ZERO };
                        let tint = if flag && blink { Color32::from_rgb(255, 230, 120) } else { Color32::WHITE };
                        let icon_c = rect.center_top() + egui::vec2(0.0, 25.0) + off;
                        if page == Page::Groups {
                            // two overlapping bubbles + people badge
                            if let Some(t) = self.icons.get("vb_chat") {
                                ui.painter().image(t.id, egui::Rect::from_center_size(icon_c + egui::vec2(-6.0, -4.0), egui::vec2(32.0, 32.0)), uv, tint);
                                ui.painter().image(t.id, egui::Rect::from_center_size(icon_c + egui::vec2(7.0, 5.0), egui::vec2(32.0, 32.0)), uv, tint);
                            }
                            if let Some(t) = self.icons.get("hd_users") {
                                ui.painter().image(t.id, egui::Rect::from_center_size(icon_c + egui::vec2(16.0, 12.0), egui::vec2(18.0, 18.0)), uv, Color32::WHITE);
                            }
                        } else if page == Page::Private {
                            // a speech bubble with an envelope
                            if let Some(t) = self.icons.get("vb_chat") {
                                ui.painter().image(t.id, egui::Rect::from_center_size(icon_c, egui::vec2(40.0, 40.0)), uv, tint);
                            }
                            if let Some(t) = self.icons.get(icon) {
                                ui.painter().image(t.id, egui::Rect::from_center_size(icon_c + egui::vec2(14.0, 11.0), egui::vec2(20.0, 20.0)), uv, Color32::WHITE);
                            }
                        } else if let Some(t) = self.icons.get(icon) {
                            ui.painter().image(t.id, egui::Rect::from_center_size(icon_c, egui::vec2(40.0, 40.0)), uv, tint);
                        }
                        if count > 0 {
                            // unread counter badge
                            let txt = if count > 99 { "99+".to_string() } else { count.to_string() };
                            let g = ui.painter().layout_no_wrap(txt, egui::FontId::proportional(11.0), Color32::WHITE);
                            let w = (g.size().x + 8.0).max(18.0);
                            let br = egui::Rect::from_center_size(rect.right_top() + egui::vec2(-w / 2.0 - 4.0, 12.0) + off, egui::vec2(w, 18.0));
                            ui.painter().rect_filled(br, 9.0f32, Color32::from_rgb(225, 50, 50));
                            ui.painter().galley(br.center() - g.size() / 2.0, g, Color32::WHITE);
                        }
                        let col = if flag && blink { Color32::YELLOW } else { Color32::WHITE };
                        let lines: Vec<&str> = label.split('\n').collect();
                        let y0 = if lines.len() == 1 { rect.top() + 57.0 } else { rect.top() + 50.0 };
                        for (k, s) in lines.iter().enumerate() {
                            let p = egui::pos2(rect.center().x, y0 + k as f32 * 14.0) + off;
                            ui.painter().text(p, egui::Align2::CENTER_TOP, *s, egui::FontId::proportional(11.5), col);
                        }
                        if resp.clicked() {
                            acts.push(Act::Page(page));
                        }
                        if flag {
                            ui.ctx().request_repaint_after(Duration::from_millis(500));
                        }
                    }
                });
                });
            });
    }

    // ------------------------------------------------------------------ users

    fn users_sorted(&self) -> Vec<u64> {
        let mut v: Vec<&Peer> = self.peers.values().collect();
        let (col, asc) = self.sort;
        v.sort_by(|a, b| {
            let o = match col {
                Col::Computer => a.p.computer.to_lowercase().cmp(&b.p.computer.to_lowercase()),
                Col::State => a.p.state.cmp(&b.p.state),
                Col::InState => a.p.in_state_secs.cmp(&b.p.in_state_secs),
                Col::Idle => a.p.idle_secs.cmp(&b.p.idle_secs),
                Col::Ping => a.ping_ms.cmp(&b.ping_ms),
                Col::FgTitle => a.p.fg_title.cmp(&b.p.fg_title),
                Col::Ip => a.ip.cmp(&b.ip),
                Col::Uptime => a.p.uptime_secs.cmp(&b.p.uptime_secs),
                Col::Version => a.p.version.cmp(&b.p.version),
                Col::Os => a.p.os_name.cmp(&b.p.os_name),
                Col::OsUptime => a.p.os_uptime_secs.cmp(&b.p.os_uptime_secs),
                Col::Time => a.p.time.cmp(&b.p.time),
                Col::Nick => a.p.nick.to_lowercase().cmp(&b.p.nick.to_lowercase()),
            };
            if asc {
                o
            } else {
                o.reverse()
            }
        });
        let mut out = vec![self.cfg.uid];
        out.extend(v.iter().map(|p| p.uid));
        let needle = lower_chars(&self.users_filter);
        if !needle.is_empty() {
            out.retain(|u| self.user_matches(*u, &needle));
        }
        out
    }

    /// Users search: nick, computer, IP, Windows user, state, active window, name / department.
    fn user_matches(&self, uid: u64, needle: &[char]) -> bool {
        let mut fields: Vec<String> = Vec::new();
        if uid == self.cfg.uid {
            let p = self.presence();
            fields.extend([p.nick, p.computer, p.os_user, p.state_title, p.state_text, p.fg_title.unwrap_or_default()]);
            fields.push(self.net.as_ref().map(|n| n.local_ip.to_string()).unwrap_or_default());
            fields.extend([self.cfg.details.full_name.clone(), self.cfg.details.department.clone()]);
        } else if let Some(p) = self.peers.get(&uid) {
            let q = &p.p;
            fields.extend([q.nick.clone(), q.computer.clone(), q.os_user.clone(), q.state_title.clone(), q.state_text.clone()]);
            fields.push(q.fg_title.clone().unwrap_or_default());
            fields.push(p.ip.to_string());
            if let Some(d) = &p.details {
                fields.extend([d.full_name.clone(), d.department.clone(), d.position.clone()]);
            }
        }
        fields.iter().any(|f| contains_ci(f, needle))
    }

    /// Search box for the users list (shared by the Users page and the side panel).
    fn users_search_box(&mut self, ui: &mut egui::Ui, width: f32) {
        let id = egui::Id::new("users_search");
        if ui.memory(|m| m.has_focus(id)) && ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape)) {
            self.users_filter.clear();
        }
        let r = ui.add(egui::TextEdit::singleline(&mut self.users_filter).id(id).hint_text("🔍 Поиск пользователей").desired_width(width));
        if self.users_filter_focus {
            r.request_focus();
            self.users_filter_focus = false;
        }
        r.on_hover_text("Ник, компьютер, IP-адрес, имя, отдел, состояние… (Ctrl+F, Esc — очистить)");
    }

    /// Groups -> uids (first entry: users without a group, title "").
    fn grouped(&self) -> Vec<(String, Vec<u64>)> {
        let all = self.users_sorted();
        if !self.cfg.groups_enabled {
            return vec![(String::new(), all)];
        }
        let mut out: Vec<(String, Vec<u64>)> = self.cfg.groups.iter().map(|g| (g.clone(), Vec::new())).collect();
        let mut none = Vec::new();
        for u in all {
            match self.cfg.user_groups.get(&u).and_then(|g| out.iter_mut().find(|x| &x.0 == g)) {
                Some(g) => g.1.push(u),
                None => none.push(u),
            }
        }
        out.insert(0, (String::new(), none));
        out
    }

    fn cell(&self, ui: &mut egui::Ui, uid: u64, col: Col) {
        let me = uid == self.cfg.uid;
        let mine;
        let p: &crate::proto::Presence = if me {
            mine = self.presence();
            &mine
        } else {
            &self.peers[&uid].p
        };
        let peer = self.peers.get(&uid);
        // seconds since the last presence packet: clocks keep ticking between updates
        let el = peer.map(|x| x.last_seen.elapsed().as_secs()).unwrap_or(0);
        match col {
            Col::Nick => {
                ui.add(img(self.icons.sized(self.icons.state(p.state), 16.0)));
                let mut t = RichText::new(format!(" {}", p.nick));
                if me {
                    t = t.strong();
                }
                let unread = self.chats.get(&uid).map(|c| c.unread).unwrap_or(0);
                if unread > 0 {
                    t = RichText::new(format!(" {} ✉{}", p.nick, unread)).strong().color(Color32::from_rgb(200, 60, 0));
                }
                let r = ui.add(egui::Label::new(t).truncate().selectable(false));
                if peer.is_some_and(|pp| pp.typing_until.is_some_and(|t| t > std::time::Instant::now())) {
                    ui.label(RichText::new(" ✎").color(Color32::GRAY)).on_hover_text("печатает…");
                }
                if !p.state_text.is_empty() {
                    r.on_hover_text(&p.state_text);
                }
            }
            Col::Computer => {
                ui.add(egui::Label::new(&p.computer).truncate().selectable(false));
            }
            Col::State => {
                let t = if p.state_text.is_empty() { p.state_title.clone() } else { format!("{} ({})", p.state_title, p.state_text) };
                ui.add(egui::Label::new(t).truncate().selectable(false));
            }
            Col::InState => {
                ui.label(fmt_dur(p.in_state_secs as u64 + el));
            }
            Col::Idle => {
                ui.label(fmt_dur(if p.idle_secs >= 3 { p.idle_secs as u64 + el } else { p.idle_secs as u64 }));
            }
            Col::Ping => {
                ui.label(if me { "0".into() } else { peer.and_then(|x| x.ping_ms).map(|m| m.to_string()).unwrap_or("—".into()) });
            }
            Col::FgTitle => match p.fg_title.as_deref() {
                Some(t) if !t.trim().is_empty() => {
                    ui.add(egui::Label::new(t).truncate().selectable(false));
                }
                Some(_) => {
                    ui.label(RichText::new("—").color(Color32::GRAY));
                }
                None => {
                    ui.add(egui::Label::new(RichText::new("—").color(Color32::GRAY)).selectable(false))
                        .on_hover_text("Скрыто: у пользователя включена приватность");
                }
            },
            Col::Ip => {
                ui.label(if me { self.net.as_ref().map(|n| n.local_ip.to_string()).unwrap_or_default() } else { peer.map(|x| x.ip.to_string()).unwrap_or_default() });
            }
            Col::Uptime => {
                ui.label(fmt_dur(p.uptime_secs as u64 + el));
            }
            Col::Version => {
                ui.label(&p.version);
            }
            Col::Os => {
                ui.add(egui::Label::new(&p.os_name).truncate().selectable(false));
            }
            Col::OsUptime => {
                ui.label(fmt_dur(p.os_uptime_secs as u64 + el));
            }
            Col::Time => {
                let t = chrono::DateTime::from_timestamp(p.time + el as i64, 0).map(|d| d.with_timezone(&chrono::Local).format("%H:%M:%S").to_string());
                ui.label(t.unwrap_or_default());
            }
        }
    }

    fn handle_row(&self, resp: &egui::Response, uid: u64, acts: &mut Vec<Act>) {
        let m = resp.ctx.input(|i| i.modifiers);
        if resp.double_clicked() {
            acts.push(Act::Click(uid, 0));
        } else if resp.clicked() {
            if m.ctrl {
                acts.push(Act::Click(uid, 1));
            } else if m.alt {
                acts.push(Act::Click(uid, 2));
            } else if m.shift {
                acts.push(Act::Click(uid, 3));
            } else {
                acts.push(Act::Select(uid));
            }
        }
        if resp.secondary_clicked() {
            acts.push(Act::Select(uid));
        }
        resp.context_menu(|ui| self.user_menu(ui, uid, acts));
    }

    fn users_page(&mut self, ui: &mut egui::Ui, acts: &mut Vec<Act>) {
        let groups = self.grouped();
        let found: usize = groups.iter().map(|g| g.1.len()).sum();
        let filtering = !self.users_filter.trim().is_empty();
        let right = if filtering { format!("найдено: {found} из {}", self.peers.len() + 1) } else { format!("в сети: {}", self.peers.len() + 1) };
        let icon = self.icons.get("hd_users");
        header_bar_ex(ui, icon, "Пользователи", &right, |ui| {
            if filtering && ui.small_button("✖").on_hover_text("Сбросить поиск").clicked() {
                self.users_filter.clear();
            }
            let w = (ui.available_width() * 0.45).clamp(140.0, 280.0);
            self.users_search_box(ui, w);
        });
        let cols: Vec<Col> = self.cfg.columns.iter().filter(|c| c.1).map(|c| c.0).collect();
        let mut toggle_group: Option<String> = None;
        let collapsed: std::collections::HashSet<String> =
            ui.data(|d| d.get_temp::<std::collections::HashSet<String>>(egui::Id::new("collapsed_groups")).unwrap_or_default());
        let mut sort_click: Option<Col> = None;
        let mut table = TableBuilder::new(ui).striped(true).resizable(true).sense(Sense::click()).cell_layout(egui::Layout::left_to_right(egui::Align::Center));
        for (i, c) in cols.iter().enumerate() {
            let w = match c {
                Col::Nick => 150.0,
                Col::Computer => 120.0,
                Col::FgTitle => 220.0,
                Col::State => 120.0,
                Col::Os => 180.0,
                _ => 80.0,
            };
            table = table.column(if i + 1 == cols.len() { Column::remainder().at_least(60.0) } else { Column::initial(w).at_least(30.0).clip(true) });
        }
        table
            .header(20.0, |mut h| {
                for c in &cols {
                    h.col(|ui| {
                        let arrow = if self.sort.0 == *c { if self.sort.1 { " ▲" } else { " ▼" } } else { "" };
                        if ui.add(egui::Label::new(RichText::new(format!("{}{arrow}", c.title())).strong()).sense(Sense::click()).selectable(false)).clicked() {
                            sort_click = Some(*c);
                        }
                    });
                }
            })
            .body(|mut body| {
                for (g, uids) in &groups {
                    if !g.is_empty() && uids.is_empty() {
                        continue;
                    }
                    if !g.is_empty() {
                        let is_collapsed = collapsed.contains(g) && !filtering;
                        body.row(20.0, |mut row| {
                            for (i, _) in cols.iter().enumerate() {
                                row.col(|ui| {
                                    if i == 0 {
                                        ui.label(RichText::new(format!("{} {} ({})", if is_collapsed { "⊞" } else { "⊟" }, g, uids.len())).strong().color(Color32::from_gray(90)));
                                    }
                                });
                            }
                            let r = row.response();
                            if r.clicked() {
                                toggle_group = Some(g.clone());
                            }
                            r.context_menu(|ui| {
                                if ui.button("Удалить группу").clicked() {
                                    acts.push(Act::DelGroup(g.clone()));
                                    ui.close_menu();
                                }
                                if ui.button("Новая группа…").clicked() {
                                    self.dlg_new_group_request();
                                    ui.close_menu();
                                }
                            });
                        });
                        if is_collapsed {
                            continue;
                        }
                    }
                    for &uid in uids {
                        body.row(20.0, |mut row| {
                            row.set_selected(self.selected == Some(uid));
                            for c in &cols {
                                row.col(|ui| self.cell(ui, uid, *c));
                            }
                            let r = row.response();
                            self.handle_row(&r, uid, acts);
                        });
                    }
                }
            });
        if let Some(c) = sort_click {
            self.sort = if self.sort.0 == c { (c, !self.sort.1) } else { (c, true) };
        }
        if let Some(g) = toggle_group {
            ui.data_mut(|d| {
                let set = d.get_temp_mut_or_default::<std::collections::HashSet<String>>(egui::Id::new("collapsed_groups"));
                if !set.remove(&g) {
                    set.insert(g);
                }
            });
        }
    }

    fn dlg_new_group_request(&self) {
        self.ctx.data_mut(|d| d.insert_temp(egui::Id::new("want_new_group"), true));
    }

    fn users_compact(&mut self, ui: &mut egui::Ui, acts: &mut Vec<Act>) {
        self.users_search_box(ui, ui.available_width());
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            for (g, uids) in self.grouped() {
                if !g.is_empty() {
                    if uids.is_empty() {
                        continue;
                    }
                    ui.label(RichText::new(&g).strong().color(Color32::from_gray(90)));
                }
                for uid in uids {
                    let me = uid == self.cfg.uid;
                    let (st, nick, tip) = if me {
                        (self.cfg.state, self.cfg.nick.clone(), "Это вы".to_string())
                    } else {
                        let p = &self.peers[&uid];
                        (p.p.state, p.p.nick.clone(), format!("{} — {}\n{}", p.p.computer, p.ip, p.p.state_title))
                    };
                    let unread = self.chats.get(&uid).map(|c| c.unread).unwrap_or(0);
                    let text = if unread > 0 {
                        format!("{nick}  ✉{unread}")
                    } else if me {
                        format!("{nick} (вы)")
                    } else {
                        nick
                    };
                    let r = tool_button(ui, self.icons.sized(self.icons.state(st), 16.0), Some(&text), 16.0, true, self.selected == Some(uid))
                        .on_hover_text(format!("{tip}\nДвойной щелчок — личное сообщение, правая кнопка — меню"));
                    self.handle_row(&r, uid, acts);
                }
            }
        });
    }

    // ------------------------------------------------------------------ global chat

    fn global_page(&mut self, ui: &mut egui::Ui, acts: &mut Vec<Act>) -> bool {
        self.global_unread = false;
        let icon = self.icons.get("hd_chat");
        let search_open = self.global_search.open;
        let mut toggle = false;
        header_bar_ex(ui, icon, "Общий чат", "сообщения для всех", |ui| {
            toggle = search_icon_button(ui, search_open, Color32::WHITE).clicked();
        });
        if toggle {
            self.global_search.toggle();
        }
        let (needle, matches) = search_matches(&self.global, &self.global_search);
        sync_search(&mut self.global_search, &self.global, &matches);
        if self.global_search.open {
            search_bar(ui, &mut self.global_search, "global_search", matches.len());
        }
        let nicks: Vec<String> = self.peers.values().map(|p| p.p.nick.clone()).collect();
        let font = self.cfg.chat.font_size;
        egui::TopBottomPanel::bottom("global_input").show_inside(ui, |ui| {
            ui.add_space(3.0);
            ui.horizontal(|ui| {
                if tool_button(ui, self.icons.sized("tb_signal", 20.0), Some("Сигнал всем"), 20.0, true, false).on_hover_text("Сигнал всем (F4)").clicked() {
                    acts.push(Act::Signal(None));
                }
                let auto = self.cfg.chat.autoscroll;
                let r = tool_button(ui, self.icons.sized("tb_autoscroll", 20.0), Some(if auto { "Автопрокрутка: вкл" } else { "Автопрокрутка: выкл" }), 20.0, true, auto);
                if r.on_hover_text("Автоматически прокручивать к новым сообщениям").clicked() {
                    self.cfg.chat.autoscroll = !auto;
                    self.cfg_dirty = true;
                    self.flash(if auto { "Автопрокрутка выключена" } else { "Автопрокрутка включена" });
                }
                if tool_button(ui, self.icons.sized("tb_leave", 20.0), Some("Очистить окно"), 20.0, !self.global.is_empty(), false)
                    .on_hover_text("Очистить окно общего чата (история на диске сохраняется)")
                    .clicked()
                {
                    self.global.clear();
                    self.flash("Окно общего чата очищено");
                }
                ui.label(RichText::new("/help — команды").weak().small());
            });
            let emo_icon = self.icons.sized("tb_emoticon", 22.0);
            let emo = if self.cfg.chat.no_emoticons { None } else { self.emo.as_ref() };
            let out = chat_input(ui, &mut self.global_input, "global_input_box", emo, emo_icon, &mut self.cfg.chat.macros, &nicks, font);
            if let Some(t) = out.submit {
                acts.push(Act::Global(t));
            }
        });
        let mut animated = false;
        let searching = !needle.is_empty();
        let mut hl = Highlight {
            current: matches.get(self.global_search.cur).map(|i| self.global[*i].id),
            needle,
            scroll: self.global_search.scroll,
            only: self.global_search.only,
        };
        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(Color32::WHITE).inner_margin(6.0))
            .show_inside(ui, |ui| {
                egui::ScrollArea::vertical().auto_shrink([false, false]).stick_to_bottom(self.cfg.chat.autoscroll && !searching).show(ui, |ui| {
                    let emo = if self.cfg.chat.no_emoticons { None } else { self.emo.as_ref() };
                    animated = chat_lines(ui, &self.global, emo, self.cfg.uid, font, false, self.cfg.chat.show_system, &mut hl);
                    if self.global.is_empty() {
                        ui.label(RichText::new("Здесь появляются сообщения для всех. Личное сообщение — правой кнопкой по пользователю → «Написать личное сообщение».").weak());
                    }
                });
            });
        self.global_search.scroll = hl.scroll;
        animated
    }

    // ------------------------------------------------------------------ event log

    fn events_page(&mut self, ui: &mut egui::Ui) {
        self.events_unread = false;
        header_bar(ui, self.icons.get("hd_eventlog"), "Журнал событий", &format!("{} записей", self.events.len()));
        let info = self.icons.sized("ev_info", 16.0);
        TableBuilder::new(ui)
            .striped(true)
            .resizable(true)
            .stick_to_bottom(true)
            .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
            .column(Column::initial(50.0))
            .column(Column::initial(80.0))
            .column(Column::initial(65.0))
            .column(Column::initial(130.0).clip(true))
            .column(Column::remainder())
            .header(20.0, |mut h| {
                for t in ["№", "Дата", "Время", "Источник", "Информация"] {
                    h.col(|ui| {
                        ui.strong(t);
                    });
                }
            })
            .body(|body| {
                body.rows(20.0, self.events.len(), |mut row| {
                    let e = &self.events[row.index()];
                    let color = match e.level {
                        Level::Info => None,
                        Level::Warn => Some(Color32::from_rgb(170, 110, 0)),
                        Level::Error => Some(Color32::from_rgb(200, 30, 30)),
                    };
                    row.col(|ui| {
                        match e.level {
                            Level::Info => {
                                ui.add(img(info));
                            }
                            Level::Warn => {
                                ui.label(RichText::new("⚠").color(color.unwrap()));
                            }
                            Level::Error => {
                                ui.label(RichText::new("⛔").color(color.unwrap()));
                            }
                        }
                        ui.label(format!(" {}", e.n));
                    });
                    row.col(|ui| {
                        ui.label(e.time.format("%d.%m.%Y").to_string());
                    });
                    row.col(|ui| {
                        ui.label(e.time.format("%H:%M:%S").to_string());
                    });
                    row.col(|ui| {
                        ui.label(&e.source);
                    });
                    row.col(|ui| {
                        let mut t = RichText::new(&e.text);
                        if let Some(c) = color {
                            t = t.color(c);
                        }
                        ui.add(egui::Label::new(t).truncate()).on_hover_text(&e.text);
                    });
                });
            });
    }

    // ------------------------------------------------------------------ info windows

    fn info_windows(&mut self, ctx: &egui::Context, acts: &mut Vec<Act>) {
        let uids: Vec<u64> = self.infos.keys().copied().collect();
        let mut refresh = Vec::new();
        let mut close = Vec::new();
        for uid in uids {
            let nick = self.peer_name(uid);
            let me = uid == self.cfg.uid;
            let allow = if me { Some(self.cfg.effective_allow()) } else { self.peers.get(&uid).map(|p| p.p.allow.clone()) };
            let w = self.infos.get_mut(&uid).unwrap();
            let mut open = true;
            egui::Window::new(format!("Информация: {nick}"))
                .id(egui::Id::new(("info", uid)))
                .open(&mut open)
                .default_size([640.0, 480.0])
                .resizable(true)
                .show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        for (t, n) in [
                            (InfoTab::Details, "Данные"),
                            (InfoTab::Processes, "Процессы"),
                            (InfoTab::Screenshot, "Снимок экрана"),
                            (InfoTab::Clipboard, "Буфер обмена"),
                        ] {
                            if ui.selectable_label(w.tab == t, n).clicked() && w.tab != t {
                                w.tab = t;
                                if !ui.input(|i| i.modifiers.ctrl) {
                                    refresh.push(uid);
                                }
                            }
                        }
                        ui.separator();
                        if ui.add_enabled(!w.busy, egui::Button::new("⟳ Обновить")).clicked() {
                            refresh.push(uid);
                        }
                        egui::ComboBox::from_id_salt(("auto", uid))
                            .width(90.0)
                            .selected_text(if w.auto_secs == 0 { "авто: нет".to_string() } else { format!("авто: {} c", w.auto_secs) })
                            .show_ui(ui, |ui| {
                                for s in [0u32, 2, 5, 10, 30, 60] {
                                    ui.selectable_value(&mut w.auto_secs, s, if s == 0 { "нет".to_string() } else { format!("{s} с") });
                                }
                            });
                        if w.busy {
                            ui.spinner();
                        }
                    });
                    if let Some(a) = &allow {
                        let mut denied = Vec::new();
                        if !a.processes {
                            denied.push("процессы");
                        }
                        if !a.screenshot {
                            denied.push("экран");
                        }
                        if !a.clipboard {
                            denied.push("буфер");
                        }
                        if !denied.is_empty() && !me {
                            ui.label(RichText::new(format!("🔒 Пользователь закрыл доступ: {}", denied.join(", "))).small().weak());
                        }
                    }
                    ui.label(RichText::new(&w.status).small());
                    ui.separator();
                    match w.tab {
                        InfoTab::Details => {
                            let Some(d) = &w.details else {
                                ui.label("Нет данных");
                                return;
                            };
                            ui.horizontal_top(|ui| {
                                if let Some(t) = &w.avatar {
                                    ui.add(egui::Image::from_texture(SizedTexture::new(t.id(), egui::vec2(128.0, 128.0))));
                                }
                                egui::Grid::new(("details", uid)).num_columns(2).striped(true).show(ui, |ui| {
                                    let g = ["—", "мужской", "женский"];
                                    let rows = [
                                        ("Полное имя", d.full_name.clone()),
                                        ("Пол", g.get(d.gender as usize).unwrap_or(&"—").to_string()),
                                        ("Возраст", if d.age == 0 { String::new() } else { d.age.to_string() }),
                                        ("Компания", d.company.clone()),
                                        ("Отдел", d.department.clone()),
                                        ("Должность", d.position.clone()),
                                        ("Рабочий тел.", d.phone_work.clone()),
                                        ("Мобильный", d.phone_mobile.clone()),
                                        ("Эл. почта", d.email.clone()),
                                        ("Адрес", d.address.clone()),
                                        ("Заметки", d.notes.clone()),
                                    ];
                                    for (k, v) in rows {
                                        ui.label(RichText::new(k).weak());
                                        ui.label(v);
                                        ui.end_row();
                                    }
                                });
                            });
                            if d.modified > 0 {
                                let t = chrono::DateTime::from_timestamp(d.modified, 0).map(|d| d.with_timezone(&chrono::Local).format("%d.%m.%Y %H:%M").to_string());
                                ui.label(RichText::new(format!("Изменено: {}", t.unwrap_or_default())).small().weak());
                            }
                        }
                        InfoTab::Processes => {
                            ui.horizontal(|ui| {
                                let can = w.sel_pid.is_some() && allow.as_ref().is_some_and(|a| a.remote_kill || me);
                                if ui.add_enabled(can, egui::Button::new("⛔ Завершить процесс")).on_hover_text("Двойной щелчок по кнопке для подтверждения").double_clicked() {
                                    if let Some(pid) = w.sel_pid {
                                        if me {
                                            let _ = crate::platform::kill(pid);
                                            refresh.push(uid);
                                        } else {
                                            acts.push(Act::Kill(uid, pid));
                                        }
                                    }
                                }
                                ui.label(RichText::new("(двойной щелчок)").small().weak());
                            });
                            TableBuilder::new(ui)
                                .striped(true)
                                .resizable(true)
                                .sense(Sense::click())
                                .column(Column::initial(70.0))
                                .column(Column::initial(180.0).clip(true))
                                .column(Column::remainder())
                                .header(18.0, |mut h| {
                                    for t in ["PID", "Модуль", "Заголовок окна"] {
                                        h.col(|ui| {
                                            ui.strong(t);
                                        });
                                    }
                                })
                                .body(|body| {
                                    body.rows(18.0, w.procs.len(), |mut row| {
                                        let p = &w.procs[row.index()];
                                        row.set_selected(w.sel_pid == Some(p.pid));
                                        row.col(|ui| {
                                            ui.label(format!("{}{}", if p.active { "▶ " } else { "" }, p.pid));
                                        });
                                        row.col(|ui| {
                                            ui.label(&p.name);
                                        });
                                        row.col(|ui| {
                                            ui.add(egui::Label::new(&p.title).truncate());
                                        });
                                        let pid = p.pid;
                                        if row.response().clicked() {
                                            w.sel_pid = Some(pid);
                                        }
                                    });
                                });
                        }
                        InfoTab::Screenshot => {
                            ui.horizontal(|ui| {
                                egui::ComboBox::from_id_salt(("mon", uid))
                                    .selected_text(if w.monitor == 0 { "Все мониторы".to_string() } else { format!("Монитор {}", w.monitor) })
                                    .show_ui(ui, |ui| {
                                        ui.selectable_value(&mut w.monitor, 0, "Все мониторы");
                                        for m in 1..=w.monitors.max(1) {
                                            ui.selectable_value(&mut w.monitor, m, format!("Монитор {m}"));
                                        }
                                    });
                                ui.add(egui::Slider::new(&mut w.quality, 20..=95).text("качество"));
                                ui.checkbox(&mut w.fit, "Вписать");
                                if ui.add_enabled(!w.shot_jpeg.is_empty(), egui::Button::new("💾 Сохранить")).clicked() {
                                    if let Some(p) = rfd::FileDialog::new().add_filter("JPEG", &["jpg"]).set_file_name(format!("screenshot_{nick}.jpg")).save_file() {
                                        let _ = std::fs::write(p, &w.shot_jpeg);
                                    }
                                }
                            });
                            if let Some((t, iw, ih)) = &w.shot {
                                let size = egui::vec2(*iw as f32, *ih as f32);
                                if w.fit {
                                    let avail = ui.available_size();
                                    let s = (avail.x / size.x).min(avail.y / size.y).min(1.0);
                                    ui.add(egui::Image::from_texture(SizedTexture::new(t.id(), size * s)));
                                } else {
                                    egui::ScrollArea::both().show(ui, |ui| {
                                        ui.add(egui::Image::from_texture(SizedTexture::new(t.id(), size)));
                                    });
                                }
                            }
                        }
                        InfoTab::Clipboard => {
                            egui::ScrollArea::vertical().show(ui, |ui| {
                                let mut s = w.clipboard.as_str();
                                ui.add(egui::TextEdit::multiline(&mut s).desired_width(f32::INFINITY).desired_rows(12));
                            });
                        }
                    }
                });
            if !open {
                close.push(uid);
            }
        }
        for u in close {
            self.infos.remove(&u);
        }
        for u in refresh {
            self.info_refresh(u);
        }
    }

    // ------------------------------------------------------------------ transfers

    fn transfers_window(&mut self, ctx: &egui::Context, acts: &mut Vec<Act>) {
        if !self.dlg.transfers {
            return;
        }
        let mut open = true;
        let mut clear_done = false;
        egui::Window::new("Передачи файлов").open(&mut open).default_size([520.0, 300.0]).show(ctx, |ui| {
            if self.transfers.is_empty() {
                ui.label("Нет передач. Отправить файл: правой кнопкой по пользователю → «Отправить файл…»");
            }
            egui::ScrollArea::vertical().show(ui, |ui| {
                for t in self.transfers.iter().rev() {
                    ui.group(|ui| {
                        ui.set_width(ui.available_width());
                        ui.horizontal(|ui| {
                            ui.label(if t.incoming { "⬇" } else { "⬆" });
                            ui.label(RichText::new(&t.name).strong());
                            ui.label(format!("{} {} · {}", if t.incoming { "от" } else { "для" }, t.nick, fmt_size(t.size)));
                        });
                        let frac = if t.size > 0 { t.done as f32 / t.size as f32 } else { 0.0 };
                        match &t.state {
                            TState::Offered => {
                                ui.horizontal(|ui| {
                                    if ui.button("✔ Принять").clicked() {
                                        acts.push(Act::Accept(t.id, true));
                                    }
                                    if ui.button("✖ Отклонить").clicked() {
                                        acts.push(Act::Accept(t.id, false));
                                    }
                                });
                            }
                            TState::Waiting => {
                                ui.horizontal(|ui| {
                                    ui.label("Ожидание ответа получателя…");
                                    if ui.button("Отменить").clicked() {
                                        acts.push(Act::CancelTransfer(t.id));
                                    }
                                });
                            }
                            TState::Running => {
                                let secs = t.started.elapsed().as_secs_f32().max(0.1);
                                ui.add(egui::ProgressBar::new(frac).show_percentage().text(format!("{} / {} — {}/с", fmt_size(t.done), fmt_size(t.size), fmt_size((t.done as f32 / secs) as u64))));
                                if ui.button("Отменить").clicked() {
                                    acts.push(Act::CancelTransfer(t.id));
                                }
                            }
                            TState::Done => {
                                ui.horizontal(|ui| {
                                    ui.label(RichText::new("✔ Готово").color(Color32::from_rgb(40, 140, 40)));
                                    if let Some(p) = &t.path {
                                        if t.incoming && ui.button("Открыть").clicked() {
                                            acts.push(Act::OpenPath(p.display().to_string()));
                                        }
                                        if ui.button("Папка").clicked() {
                                            acts.push(Act::OpenPath(p.parent().map(|x| x.display().to_string()).unwrap_or_default()));
                                        }
                                    }
                                });
                            }
                            TState::Failed(e) => {
                                ui.horizontal(|ui| {
                                    ui.label(RichText::new(format!("✖ {e}")).color(Color32::from_rgb(190, 40, 40)));
                                    if t.incoming && ui.button("Докачать").clicked() {
                                        acts.push(Act::Accept(t.id, true));
                                    }
                                });
                            }
                            TState::Declined => {
                                ui.label("Отклонено");
                            }
                        }
                    });
                }
            });
            if ui.button("Убрать завершённые").clicked() {
                clear_done = true;
            }
        });
        if clear_done {
            self.transfers.retain(|t| matches!(t.state, TState::Offered | TState::Waiting | TState::Running));
        }
        if !open {
            self.dlg.transfers = false;
        }
    }

    // ------------------------------------------------------------------ dialogs

    pub fn open_setup(&mut self) {
        self.dlg.setup = Some(SetupState { page: 0, draft: self.cfg.clone(), ifaces: crate::net::interfaces() });
    }

    fn dialogs(&mut self, ctx: &egui::Context, acts: &mut Vec<Act>) {
        if ctx.data_mut(|d| d.remove_temp::<bool>(egui::Id::new("want_new_group"))).is_some() {
            self.dlg.new_group = Some(String::new());
        }
        // new group
        if let Some(name) = &mut self.dlg.new_group {
            let mut done = None;
            egui::Window::new("Новая группа").collapsible(false).resizable(false).show(ctx, |ui| {
                let r = ui.text_edit_singleline(name);
                r.request_focus();
                ui.horizontal(|ui| {
                    if ui.button("ОК").clicked() || ui.input(|i| i.key_pressed(Key::Enter)) {
                        done = Some(true);
                    }
                    if ui.button("Отмена").clicked() {
                        done = Some(false);
                    }
                });
            });
            if let Some(ok) = done {
                let n = name.trim().to_string();
                if ok && !n.is_empty() && !self.cfg.groups.contains(&n) {
                    self.cfg.groups.push(n);
                    self.cfg.groups_enabled = true;
                    self.cfg_dirty = true;
                }
                self.dlg.new_group = None;
            }
        }
        // new nick
        if let Some(name) = &mut self.dlg.nick_edit {
            let mut done = None;
            egui::Window::new("Новый ник").collapsible(false).resizable(false).show(ctx, |ui| {
                ui.text_edit_singleline(name).request_focus();
                ui.horizontal(|ui| {
                    if ui.button("ОК").clicked() || ui.input(|i| i.key_pressed(Key::Enter)) {
                        done = Some(true);
                    }
                    if ui.button("Отмена").clicked() {
                        done = Some(false);
                    }
                });
            });
            if let Some(ok) = done {
                if ok {
                    acts.push(Act::Nick(name.clone()));
                }
                self.dlg.nick_edit = None;
            }
        }
        // state editor
        if let Some((idx, st)) = &mut self.dlg.state_edit {
            let mut done: Option<u8> = None; // 1 save, 2 save+switch, 3 delete, 0 cancel
            let is_std = *idx < 6;
            egui::Window::new("Настройка состояния").collapsible(false).resizable(false).show(ctx, |ui| {
                egui::Grid::new("st").num_columns(2).show(ui, |ui| {
                    ui.label("Название:");
                    ui.add_enabled(!is_std, egui::TextEdit::singleline(&mut st.title));
                    ui.end_row();
                    ui.label("Пояснение:");
                    ui.text_edit_singleline(&mut st.text);
                    ui.end_row();
                });
                ui.checkbox(&mut st.auto_answer, "Автоответ пояснением на личные сообщения");
                ui.checkbox(&mut st.no_sounds, "Отключить все звуки");
                ui.checkbox(&mut st.no_popups, "Не показывать всплывающие уведомления");
                ui.checkbox(&mut st.no_auto_away, "Не переключаться в «Отошёл» автоматически");
                ui.horizontal(|ui| {
                    if ui.button("Переключиться").clicked() {
                        done = Some(2);
                    }
                    if ui.button("Сохранить").clicked() {
                        done = Some(1);
                    }
                    if !is_std && *idx != usize::MAX && ui.button("Удалить").clicked() {
                        done = Some(3);
                    }
                    if ui.button("Отмена").clicked() {
                        done = Some(0);
                    }
                });
            });
            if let Some(d) = done {
                let (idx, st) = (*idx, st.clone());
                self.dlg.state_edit = None;
                match d {
                    1 | 2 => {
                        let i = if idx == usize::MAX {
                            self.cfg.states.push(st);
                            self.cfg.states.len() - 1
                        } else {
                            self.cfg.states[idx] = st;
                            idx
                        };
                        self.cfg_dirty = true;
                        if d == 2 {
                            acts.push(Act::State(i as u8));
                        } else {
                            self.send_presence();
                        }
                    }
                    3 => {
                        if self.cfg.state as usize == idx {
                            self.set_state(0, false);
                        } else if (self.cfg.state as usize) > idx {
                            self.cfg.state -= 1;
                        }
                        self.cfg.states.remove(idx);
                        self.cfg_dirty = true;
                    }
                    _ => {}
                }
            }
        }
        // my details
        if self.dlg.details_edit.is_some() {
            let mut done = None;
            let ctx2 = self.ctx.clone();
            let uid = self.cfg.uid;
            let (d, tex) = self.dlg.details_edit.as_mut().unwrap();
            egui::Window::new("Мои данные").collapsible(false).default_width(460.0).show(ctx, |ui| {
                ui.horizontal_top(|ui| {
                    ui.vertical(|ui| {
                        match tex {
                            Some(t) => {
                                ui.add(egui::Image::from_texture(SizedTexture::new(t.id(), egui::vec2(128.0, 128.0))));
                            }
                            None => {
                                ui.add_sized([128.0, 128.0], egui::Label::new(RichText::new("нет фото").weak()));
                            }
                        }
                        if ui.button("Загрузить фото…").clicked() {
                            if let Some(p) = rfd::FileDialog::new().add_filter("Изображения", &["png", "jpg", "jpeg", "bmp", "gif"]).pick_file() {
                                if let Ok(im) = image::open(p) {
                                    let th = im.resize_to_fill(128, 128, image::imageops::FilterType::Lanczos3);
                                    let mut png = Vec::new();
                                    if th.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).is_ok() {
                                        *tex = avatar_texture(&ctx2, uid, &png);
                                        d.picture_png = png;
                                    }
                                }
                            }
                        }
                        if !d.picture_png.is_empty() && ui.button("Убрать фото").clicked() {
                            d.picture_png.clear();
                            *tex = None;
                        }
                    });
                    egui::Grid::new("mydetails").num_columns(2).show(ui, |ui| {
                        let row = |ui: &mut egui::Ui, k: &str, v: &mut String| {
                            ui.label(k);
                            ui.text_edit_singleline(v);
                            ui.end_row();
                        };
                        row(ui, "Полное имя", &mut d.full_name);
                        ui.label("Пол");
                        ui.horizontal(|ui| {
                            ui.radio_value(&mut d.gender, 0, "—");
                            ui.radio_value(&mut d.gender, 1, "муж.");
                            ui.radio_value(&mut d.gender, 2, "жен.");
                        });
                        ui.end_row();
                        ui.label("Возраст");
                        ui.add(egui::DragValue::new(&mut d.age).range(0..=120));
                        ui.end_row();
                        row(ui, "Компания", &mut d.company);
                        row(ui, "Отдел", &mut d.department);
                        row(ui, "Должность", &mut d.position);
                        row(ui, "Рабочий тел.", &mut d.phone_work);
                        row(ui, "Мобильный", &mut d.phone_mobile);
                        row(ui, "Эл. почта", &mut d.email);
                        row(ui, "Адрес", &mut d.address);
                        ui.label("Заметки");
                        ui.text_edit_multiline(&mut d.notes);
                        ui.end_row();
                    });
                });
                ui.horizontal(|ui| {
                    if ui.button("Сохранить").clicked() {
                        done = Some(true);
                    }
                    if ui.button("Отмена").clicked() {
                        done = Some(false);
                    }
                });
            });
            if let Some(ok) = done {
                let (mut d, _) = self.dlg.details_edit.take().unwrap();
                if ok {
                    d.modified = now_unix();
                    self.cfg.details = d;
                    self.cfg_dirty = true;
                    self.update_shared();
                    self.send_presence();
                }
            }
        }
        if let Some(u) = self.dlg.clear_chat {
            let nick = self.peer_name(u);
            let mut done = None;
            egui::Window::new("Очистить историю").collapsible(false).resizable(false).show(ctx, |ui| {
                ui.label(format!("Удалить всю переписку с {nick}?"));
                ui.label(RichText::new("Это действие нельзя отменить. Недоставленные сообщения останутся.").small().weak());
                ui.horizontal(|ui| {
                    if ui.button("🗑 Удалить").clicked() {
                        done = Some(true);
                    }
                    if ui.button("Отмена").clicked() || ui.input(|i| i.key_pressed(Key::Escape)) {
                        done = Some(false);
                    }
                });
            });
            if let Some(ok) = done {
                self.dlg.clear_chat = None;
                if ok {
                    self.clear_chat(u);
                    self.flash(format!("История переписки с {nick} очищена"));
                }
            }
        }
        if self.dlg.about {
            egui::Window::new("О программе").collapsible(false).resizable(false).open(&mut self.dlg.about).show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.add(img(self.icons.get("app")));
                    ui.heading(format!("Nassi RS {}", crate::proto::VERSION));
                });
                ui.label("Мессенджер для локальной сети без сервера, написан на Rust.");
                ui.label("Вдохновлён Network Assistant 4.5 (Gracebyte Software); собственный протокол, со старым Nassi по сети несовместим.");
                ui.label(format!("Данные: {}", self.paths.data.display()));
            });
        }
        if self.dlg.help {
            egui::Window::new("Клавиши и команды").open(&mut self.dlg.help).default_width(480.0).show(ctx, |ui| {
                ui.label(RichText::new("Общий чат").strong());
                ui.label("/me текст — действие от своего имени\n/nick ник — сменить ник\n/here — кто в сети\n/m ник текст — личное сообщение\n/s [ник] — сигнал\n/i ник — информация о пользователе\n/clear — очистить окно");
                ui.label(RichText::new("Ввод текста").strong());
                ui.label("Enter — отправить, Ctrl+Enter — новая строка\nCtrl+↑/↓ — история фраз, Tab — дописать ник\nAlt+R — исправить раскладку (EN↔RU)\nShift+F1..F12 — макрос, Shift+Ctrl+F1..F12 — запомнить макрос");
                ui.label(RichText::new("Поиск").strong());
                ui.label("Ctrl+F — поиск на текущей вкладке (пользователи, общий чат, личные сообщения, беседа)\nEnter / ▲ — более раннее совпадение, Shift+Enter / ▼ — более позднее, Esc — закрыть");
                ui.label(RichText::new("Окно").strong());
                ui.label("Ctrl+R/W/D/P/A/O — состояние, Ctrl+1..6 — свои состояния\nF5 — обновить список, F4 — сигнал всем\nAlt+1…5 — вкладки по порядку\nAlt+F1 — Пользователи, Alt+F2 — Общий чат, Alt+F6 — Личные сообщения,\nAlt+F3 — Групповой чат, Alt+F5 — Журнал событий\nAlt+S — настройка, Alt+M — мои данные, Alt+D — выход, Esc — в трей");
            });
        }
    }

    // ------------------------------------------------------------------ setup

    fn setup_window(&mut self, ctx: &egui::Context) {
        let Some(st) = &mut self.dlg.setup else { return };
        let mut result: Option<u8> = None; // 0 cancel, 1 ok, 2 apply, 3 restart
        let pages = ["Общие", "Соединение", "Приватность", "Удалённый доступ", "Чат", "Звуки", "Отсутствие", "Ресурсы", "Колонки"];
        let emo_packs = self.emo_packs.clone();
        let assets = self.assets.clone();
        let cur_conn = self.cfg.conn.clone();
        let cur_render = self.cfg.general.render;
        egui::Window::new("Настройка").collapsible(false).default_size([640.0, 440.0]).resizable(true).show(ctx, |ui| {
            egui::SidePanel::left("setup_pages").resizable(false).exact_width(130.0).show_inside(ui, |ui| {
                for (i, p) in pages.iter().enumerate() {
                    if ui.selectable_label(st.page == i, *p).clicked() {
                        st.page = i;
                    }
                }
            });
            egui::TopBottomPanel::bottom("setup_buttons").show_inside(ui, |ui| {
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    if ui.button("ОК").clicked() {
                        result = Some(1);
                    }
                    if ui.button("Отмена").clicked() {
                        result = Some(0);
                    }
                    if ui.button("Применить").clicked() {
                        result = Some(2);
                    }
                    if st.draft.conn != cur_conn || st.draft.general.render != cur_render {
                        ui.label(RichText::new("Эти настройки применятся после перезапуска").color(Color32::from_rgb(190, 110, 0)));
                        if ui.button("Сохранить и перезапустить").clicked() {
                            result = Some(3);
                        }
                    }
                });
            });
            egui::CentralPanel::default().show_inside(ui, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    let d = &mut st.draft;
                    match st.page {
                        0 => {
                            ui.checkbox(&mut d.general.close_to_tray, "Кнопка «Закрыть» сворачивает в трей");
                            ui.checkbox(&mut d.general.start_minimized, "Запускаться свёрнутым в трей");
                            ui.horizontal(|ui| {
                                ui.label("Отрисовка окна:");
                                egui::ComboBox::from_id_salt("render_mode").selected_text(d.general.render.title()).show_ui(ui, |ui| {
                                    for m in [crate::config::RenderMode::Auto, crate::config::RenderMode::OpenGl, crate::config::RenderMode::Software] {
                                        ui.selectable_value(&mut d.general.render, m, m.title());
                                    }
                                });
                            })
                            .response
                            .on_hover_text(
                                "Программной отрисовке не нужны видеокарта и драйвер OpenGL.\nВключите её, если окно программы не открывается или рисуется с артефактами\n(старые видеокарты, стандартный VGA-драйвер Windows 7, виртуальные машины).\nАвто: OpenGL, если драйвер поддерживает OpenGL 3.0+, иначе программная.",
                            );
                            if let Some(r) = crate::RENDER_INFO.get() {
                                ui.label(RichText::new(format!("    Сейчас: {r}")).weak());
                            }
                            ui.checkbox(&mut d.general.popup_on_private, "Разворачивать окно и открывать «Личные сообщения» при новом личном сообщении");
                            ui.checkbox(&mut d.general.popup_on_global, "Разворачивать окно при сообщении в общем чате");
                            ui.checkbox(&mut d.general.flash_on_message, "Мигать значком программы, если пришло сообщение, а меня нет на месте")
                                .on_hover_text("Общий чат, личные сообщения, беседы, сигналы и файлы.\nМигает кнопка в панели задач (а если программа свёрнута в трей — значок в трее),\nпока вы не вернётесь к окну программы.");
                            ui.add_enabled_ui(d.general.flash_on_message, |ui| {
                                ui.horizontal(|ui| {
                                    ui.label("    Считать, что меня нет на месте, если окно не активно или нет действий с мышью/клавиатурой, сек:");
                                    ui.add(egui::DragValue::new(&mut d.general.away_secs).range(5..=3600));
                                });
                            });
                            ui.checkbox(&mut d.general.tip_enabled, "Всплывающие уведомления о сообщениях");
                            ui.horizontal(|ui| {
                                ui.label("Показывать, сек:");
                                ui.add(egui::DragValue::new(&mut d.general.tip_secs).range(1..=60));
                                ui.label("Угол:");
                                egui::ComboBox::from_id_salt("corner")
                                    .selected_text(["Слева вверху", "Справа вверху", "Слева внизу", "Справа внизу"][d.general.tip_corner as usize % 4])
                                    .show_ui(ui, |ui| {
                                        for (i, n) in ["Слева вверху", "Справа вверху", "Слева внизу", "Справа внизу"].iter().enumerate() {
                                            ui.selectable_value(&mut d.general.tip_corner, i as u8, *n);
                                        }
                                    });
                            });
                            ui.horizontal(|ui| {
                                ui.label("Таймаут обновления списка (мс):");
                                ui.add(egui::DragValue::new(&mut d.general.refresh_timeout_ms).range(500..=30000));
                            });
                            ui.separator();
                            ui.label(RichText::new("Действия по щелчку на пользователе").strong());
                            for (i, n) in ["Двойной щелчок", "Ctrl + щелчок", "Alt + щелчок", "Shift + щелчок"].iter().enumerate() {
                                ui.horizontal(|ui| {
                                    ui.label(*n);
                                    egui::ComboBox::from_id_salt(("click", i)).selected_text(d.general.click_actions[i].title()).show_ui(ui, |ui| {
                                        for a in ClickAction::ALL {
                                            ui.selectable_value(&mut d.general.click_actions[i], a, a.title());
                                        }
                                    });
                                });
                            }
                        }
                        1 => {
                            ui.label("Сообщения для всех рассылаются широковещательно на 255.255.255.255. Один порт — для UDP и TCP.");
                            ui.add_space(6.0);
                            ui.label(RichText::new("Сетевой интерфейс (закрепляется жёстко)").strong());
                            let cur = d.conn.bind_ip.clone();
                            let label = st.ifaces.iter().find(|(_, ip)| ip.to_string() == cur).map(|(n, ip)| format!("{ip} — {n}")).unwrap_or_else(|| {
                                if cur.is_empty() {
                                    "не выбран".into()
                                } else {
                                    format!("{cur} (сейчас отсутствует)")
                                }
                            });
                            egui::ComboBox::from_id_salt("iface").width(360.0).selected_text(label).show_ui(ui, |ui| {
                                for (n, ip) in &st.ifaces {
                                    ui.selectable_value(&mut d.conn.bind_ip, ip.to_string(), format!("{ip} — {n}"));
                                }
                            });
                            if ui.button("Определить автоматически (без VPN)").clicked() {
                                if let Some(ip) = crate::net::detect_local_ip() {
                                    d.conn.bind_ip = ip.to_string();
                                }
                            }
                            ui.label(RichText::new("Выбранный адрес хранится в настройках и сам не меняется, даже если включится VPN.").small().weak());
                            ui.add_space(6.0);
                            ui.horizontal(|ui| {
                                ui.label("Порт:");
                                ui.add(egui::DragValue::new(&mut d.conn.port).range(1024..=65535));
                                if ui.small_button("по умолчанию").clicked() {
                                    d.conn.port = crate::net::DEFAULT_PORT;
                                }
                            });
                            ui.horizontal(|ui| {
                                ui.label("Сетевой пароль:");
                                ui.add(egui::TextEdit::singleline(&mut d.conn.network_password).password(true).desired_width(160.0));
                            });
                            ui.label(RichText::new("Пароль шифрует трафик; общаться могут только копии с одинаковым паролем (пусто — общий ключ).").small().weak());
                        }
                        2 => {
                            ui.label(RichText::new("Когда действуют ограничения").strong());
                            ui.radio_value(&mut d.privacy.global, true, "Всегда, во всех состояниях (рекомендуется)");
                            ui.radio_value(&mut d.privacy.global, false, "Только в состоянии «Приватный»");
                            ui.add_space(4.0);
                            ui.label("Запретить другим:");
                            ui.checkbox(&mut d.privacy.no_fg_title, "Видеть заголовок моего активного окна (вместо него — «—»)");
                            ui.checkbox(&mut d.privacy.no_processes, "Просматривать мои процессы");
                            ui.checkbox(&mut d.privacy.no_screenshot, "Снимать мой экран");
                            ui.checkbox(&mut d.privacy.no_clipboard, "Читать мой буфер обмена");
                            ui.separator();
                            ui.checkbox(&mut d.privacy.no_remote_kill, "Запретить удалённое завершение моих процессов (всегда)");
                            ui.add_space(6.0);
                            ui.label(
                                RichText::new(
                                    "В состоянии «Приватный» (Ctrl+P) скрывается всё: активное окно (у других в списке — «—»), \
                                     процессы, экран и буфер обмена — независимо от галочек выше.",
                                )
                                .small(),
                            );
                            ui.add_space(6.0);
                            // what the others see right now with these settings
                            let mut preview = d.clone();
                            preview.state = self.cfg.state;
                            let a = preview.effective_allow();
                            let yes_no = |b: bool| if b { "видно" } else { "скрыто" };
                            egui::Frame::group(ui.style()).show(ui, |ui| {
                                ui.label(RichText::new(format!("Сейчас (состояние «{}») другие видят:", self.cfg.cur_state().title)).strong());
                                egui::Grid::new("privacy_now").num_columns(2).show(ui, |ui| {
                                    for (k, v) in [
                                        ("Активное окно", a.fg_title),
                                        ("Процессы", a.processes),
                                        ("Снимок экрана", a.screenshot),
                                        ("Буфер обмена", a.clipboard),
                                    ] {
                                        ui.label(k);
                                        ui.label(RichText::new(yes_no(v)).color(if v { Color32::from_rgb(40, 130, 40) } else { Color32::from_rgb(190, 60, 40) }));
                                        ui.end_row();
                                    }
                                });
                            });
                            ui.label(RichText::new("Каждый удалённый запрос записывается в журнал событий.").small().weak());
                        }
                        3 => {
                            ui.label(RichText::new("Удалённый доступ к этому компьютеру").strong());
                            ui.add_space(4.0);
                            ui.label("Удалённый доступ всегда включён и защищён встроенным паролем администратора.");
                            ui.label("Пароль задан в программе и не меняется из настроек.");
                            ui.add_space(4.0);
                            ui.label("Пароль спрашивается при каждом действии: подключение (управление или просмотр), каждая команда командной строки, каждая команда питания.");
                            ui.add_space(6.0);
                            ui.label(
                                RichText::new(
                                    "Во время сеанса на экране всегда виден индикатор с кнопкой «Отключить», всё пишется в журнал событий. \
                                     Пароль не передаётся по сети (проверка challenge-response), неверный пароль — задержка 2 с.",
                                )
                                .small()
                                .weak(),
                            );
                        }                        4 => {
                            ui.horizontal(|ui| {
                                ui.label("Пакет смайлов:");
                                let cur = if d.emoticon_pack.is_empty() { "по умолчанию (Kolobok)".to_string() } else { d.emoticon_pack.clone() };
                                egui::ComboBox::from_id_salt("emo").width(300.0).selected_text(cur).show_ui(ui, |ui| {
                                    ui.selectable_value(&mut d.emoticon_pack, String::new(), "по умолчанию (Kolobok)");
                                    for p in &emo_packs {
                                        let rel = assets.as_ref().and_then(|a| p.strip_prefix(a).ok()).map(|r| r.display().to_string()).unwrap_or(p.display().to_string());
                                        ui.selectable_value(&mut d.emoticon_pack, rel.clone(), rel);
                                    }
                                });
                            });
                            ui.checkbox(&mut d.chat.no_emoticons, "Отключить графические смайлы");
                            ui.checkbox(&mut d.chat.show_system, "Показывать системные сообщения");
                            ui.horizontal(|ui| {
                                ui.label("Размер шрифта чата:");
                                ui.add(egui::Slider::new(&mut d.chat.font_size, 10.0..=24.0));
                            });
                            ui.horizontal(|ui| {
                                ui.label("Хранить строк общего чата в окне:");
                                ui.add(egui::DragValue::new(&mut d.chat.history_lines).range(100..=20000));
                            });
                            ui.separator();
                            ui.label(RichText::new("Макросы (Shift+F1..F12)").strong());
                            egui::Grid::new("macros").num_columns(2).show(ui, |ui| {
                                for (i, m) in d.chat.macros.iter_mut().enumerate() {
                                    ui.label(format!("F{}", i + 1));
                                    ui.add(egui::TextEdit::singleline(m).desired_width(320.0));
                                    ui.end_row();
                                }
                            });
                        }
                        5 => {
                            ui.checkbox(&mut d.sounds.enabled, "Включить звуки");
                            egui::Grid::new("sounds").num_columns(3).striped(true).show(ui, |ui| {
                                for (k, title, _) in SOUND_EVENTS {
                                    if let Some((on, file)) = d.sounds.events.get_mut(k) {
                                        ui.checkbox(on, title);
                                        ui.add(egui::TextEdit::singleline(file).desired_width(200.0));
                                        ui.horizontal(|ui| {
                                            if ui.small_button("▶").on_hover_text("Прослушать").clicked() {
                                                let p = std::path::PathBuf::from(&*file);
                                                let p = if p.is_absolute() { p } else { assets.clone().unwrap_or_default().join("Sounds").join(p) };
                                                crate::platform::play_wav(&p);
                                            }
                                            if ui.small_button("…").clicked() {
                                                if let Some(p) = rfd::FileDialog::new().add_filter("WAV", &["wav"]).pick_file() {
                                                    *file = p.display().to_string();
                                                }
                                            }
                                        });
                                        ui.end_row();
                                    }
                                }
                            });
                        }
                        6 => {
                            ui.checkbox(&mut d.away.on_idle, "Переключаться в «Отошёл» при бездействии");
                            ui.horizontal(|ui| {
                                ui.label("через (мин):");
                                ui.add(egui::DragValue::new(&mut d.away.idle_min).range(1..=600));
                            });
                            ui.label(RichText::new("При возвращении к компьютеру прежнее состояние восстанавливается.").small().weak());
                        }
                        7 => {
                            ui.label("Папка, в которой лежат Emoticons\\ и Sounds\\ (по умолчанию — папка программы):");
                            ui.horizontal(|ui| {
                                ui.add(egui::TextEdit::singleline(&mut d.assets_dir).desired_width(340.0).hint_text("авто"));
                                if ui.button("Обзор…").clicked() {
                                    if let Some(p) = rfd::FileDialog::new().pick_folder() {
                                        d.assets_dir = p.display().to_string();
                                    }
                                }
                            });
                            ui.label(format!("Сейчас используется: {}", assets.as_ref().map(|a| a.display().to_string()).unwrap_or("не найдено".into())));
                        }
                        8 => {
                            ui.label("Колонки списка пользователей (порядок — кнопками):");
                            let n = d.columns.len();
                            let mut swap = None;
                            for i in 0..n {
                                ui.horizontal(|ui| {
                                    let (c, on) = &mut d.columns[i];
                                    ui.add_enabled(*c != Col::Nick, egui::Checkbox::new(on, c.title()));
                                    if i > 0 && ui.small_button("▲").clicked() {
                                        swap = Some((i, i - 1));
                                    }
                                    if i + 1 < n && ui.small_button("▼").clicked() {
                                        swap = Some((i, i + 1));
                                    }
                                });
                            }
                            if let Some((a, b)) = swap {
                                d.columns.swap(a, b);
                            }
                            if ui.button("Сбросить").clicked() {
                                d.columns = default_columns();
                            }
                        }
                        _ => {}
                    }
                });
            });
        });
        if let Some(r) = result {
            if r == 0 {
                self.dlg.setup = None;
                return;
            }
            let mut draft = self.dlg.setup.as_ref().unwrap().draft.clone();
            // keep live values that may have changed while the dialog was open
            draft.state = self.cfg.state;
            draft.states = self.cfg.states.clone();
            draft.nick = self.cfg.nick.clone();
            draft.nicks = self.cfg.nicks.clone();
            draft.known = self.cfg.known.clone();
            draft.window = self.cfg.window;
            draft.details = self.cfg.details.clone();
            draft.color = self.cfg.color;
            draft.groups = self.cfg.groups.clone();
            draft.user_groups = self.cfg.user_groups.clone();
            draft.groups_enabled = self.cfg.groups_enabled;
            draft.users_panel = self.cfg.users_panel;
            draft.general.ui_scale = self.cfg.general.ui_scale;
            draft.chat.autoscroll = self.cfg.chat.autoscroll;
            let emo_changed = draft.emoticon_pack != self.cfg.emoticon_pack;
            let assets_changed = draft.assets_dir != self.cfg.assets_dir;
            self.cfg = draft;
            self.save_cfg();
            self.update_shared();
            self.send_presence();
            if assets_changed {
                self.assets = Paths::find_assets(&self.cfg);
                self.emo_packs = crate::emoticons::find_packs(self.assets.as_deref());
            }
            if emo_changed || assets_changed {
                self.load_emoticons();
            }
            self.log(Level::Info, "Настройки", "Настройки сохранены");
            if r == 1 {
                self.dlg.setup = None;
            }
            if r == 3 {
                self.restart();
            }
        }
    }

    pub fn restart(&mut self) {
        if let Ok(exe) = std::env::current_exe() {
            let mut args: Vec<String> = std::env::args().skip(1).filter(|a| !a.starts_with("--delay") && a != "--software" && a != "--opengl").collect();
            args.push("--delay=1500".into());
            let _ = std::process::Command::new(exe).args(args).spawn();
        }
        self.quit = true;
    }

    // ------------------------------------------------------------------ tips

    fn tips_viewport(&mut self, ctx: &egui::Context) {
        if self.tips.is_empty() || (self.focused && self.visible && !self.minimized) {
            if self.focused {
                self.tips.clear();
            }
            return;
        }
        let (w, h) = (320.0f32, 30.0 + 52.0 * self.tips.len() as f32);
        let anchor = self.main_pos.unwrap_or(egui::pos2(100.0, 100.0));
        let (ax, ay, aw, ah) = crate::platform::work_area_at(anchor.x as i32 + 50, anchor.y as i32 + 50);
        let ppp = ctx.pixels_per_point();
        let (ax, ay, aw, ah) = (ax as f32 / ppp, ay as f32 / ppp, aw as f32 / ppp, ah as f32 / ppp);
        let pos = match self.cfg.general.tip_corner {
            0 => egui::pos2(ax + 8.0, ay + 8.0),
            1 => egui::pos2(ax + aw - w - 8.0, ay + 8.0),
            2 => egui::pos2(ax + 8.0, ay + ah - h - 8.0),
            _ => egui::pos2(ax + aw - w - 8.0, ay + ah - h - 8.0),
        };
        let tips: Vec<(String, String, (Option<u64>, Option<u64>))> = self.tips.iter().map(|t| (t.title.clone(), t.text.clone(), (t.chat_uid, t.group))).collect();
        let mut clicked: Option<(Option<u64>, Option<u64>)> = None;
        let mut dismiss = false;
        ctx.show_viewport_immediate(
            egui::ViewportId::from_hash_of("chat_in_tip"),
            egui::ViewportBuilder::default()
                .with_title("Nassi RS")
                .with_decorations(false)
                .with_always_on_top()
                .with_taskbar(false)
                .with_resizable(false)
                .with_inner_size([w, h])
                .with_position(pos),
            |ctx, _| {
                egui::CentralPanel::default()
                    .frame(egui::Frame::none().fill(Color32::from_rgb(255, 252, 225)).stroke(egui::Stroke::new(1.0f32, Color32::from_gray(120))).inner_margin(6.0))
                    .show(ctx, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new("Nassi RS").strong().color(Color32::from_rgb(60, 60, 60)));
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if ui.small_button("✖").clicked() {
                                    dismiss = true;
                                }
                            });
                        });
                        for (title, text, uid) in &tips {
                            let r = ui
                                .vertical(|ui| {
                                    ui.label(RichText::new(title).strong().color(Color32::from_rgb(40, 40, 120)));
                                    let short: String = text.chars().take(90).collect();
                                    ui.add(egui::Label::new(RichText::new(short).color(Color32::BLACK)).truncate());
                                })
                                .response
                                .interact(Sense::click());
                            if r.clicked() {
                                clicked = Some(*uid);
                            }
                            ui.separator();
                        }
                    });
            },
        );
        if dismiss {
            self.tips.clear();
        }
        if let Some(uid) = clicked {
            self.tips.clear();
            self.show_main();
            match uid {
                (Some(u), _) => self.open_chat(u),
                (None, Some(g)) => self.open_group(g),
                _ => self.page = Page::Chat,
            }
        }
        ctx.request_repaint_after(Duration::from_millis(250));
    }
}

#[cfg(test)]
mod tests {
    use super::{contains_ci, find_ci, lower_chars, word_start};

    #[test]
    fn search_is_case_insensitive_and_yo_agnostic() {
        let n = lower_chars("ЁЛКА");
        assert!(contains_ci("новая елка во дворе", &n));
        assert!(contains_ci("Ёлка", &n));
        assert!(!contains_ci("ель", &n));
        let n = lower_chars("  при ");
        assert_eq!(find_ci("Привет, ПРИятель", &n), vec![(0, 6), (14, 20)]);
        assert!(find_ci("abc", &[]).is_empty());
        assert!(contains_ci("abc", &[]));
    }

    #[test]
    fn word_start_handles_multibyte_whitespace() {
        let s = "привет\u{a0}ва"; // NBSP is 2 bytes in UTF-8
        assert_eq!(&s[word_start(s)..], "ва");
        let s = "a\u{3000}bc"; // ideographic space, 3 bytes
        assert_eq!(&s[word_start(s)..], "bc");
        assert_eq!(word_start("hello wo"), 6);
        assert_eq!(word_start("ник"), 0);
        assert_eq!(word_start("ник "), "ник ".len());
    }
}
