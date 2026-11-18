//! Действия → «История принятых файлов»: who sent which file and when, with filters by
//! sender, period and file name (data: src/received.rs).

use super::Act;
use crate::app::*;
use crate::received::{Filter, Period};
use eframe::egui::{self, Color32, RichText};
use egui_extras::{Column, TableBuilder};
use std::collections::BTreeMap;

impl App {
    pub fn open_received(&mut self) {
        if self.dlg.received.is_none() {
            self.dlg.received = Some(Filter::default());
        }
    }

    pub(super) fn received_window(&mut self, ctx: &egui::Context, acts: &mut Vec<Act>) {
        let Some(f) = &mut self.dlg.received else { return };
        let mut open = true;
        // senders for the filter: key -> the latest nick (and computer) seen for it
        let mut senders: BTreeMap<String, String> = BTreeMap::new();
        for r in &self.received {
            let label = if r.computer.is_empty() { r.nick.clone() } else { format!("{} ({})", r.nick, r.computer) };
            senders.insert(r.sender_key(), label);
        }
        let today = chrono::Local::now().date_naive();
        egui::Window::new("История принятых файлов").open(&mut open).default_size([760.0, 420.0]).resizable(true).show(ctx, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.label("От кого:");
                let cur = if f.sender.is_empty() { "Все".to_string() } else { senders.get(&f.sender).cloned().unwrap_or_else(|| "?".into()) };
                egui::ComboBox::from_id_salt("recv_sender").width(170.0).selected_text(cur).show_ui(ui, |ui| {
                    ui.selectable_value(&mut f.sender, String::new(), "Все");
                    let mut list: Vec<(&String, &String)> = senders.iter().collect();
                    list.sort_by_key(|(_, n)| n.to_lowercase());
                    for (k, n) in list {
                        ui.selectable_value(&mut f.sender, k.clone(), n);
                    }
                });
                ui.label("Когда:");
                egui::ComboBox::from_id_salt("recv_period").selected_text(f.period.title()).show_ui(ui, |ui| {
                    for p in Period::ALL {
                        ui.selectable_value(&mut f.period, p, p.title());
                    }
                });
                if f.period == Period::Custom {
                    ui.label("с");
                    ui.add(egui::TextEdit::singleline(&mut f.from_date).desired_width(80.0).hint_text("ДД.ММ.ГГГГ"));
                    ui.label("по");
                    ui.add(egui::TextEdit::singleline(&mut f.to_date).desired_width(80.0).hint_text("ДД.ММ.ГГГГ"));
                }
                ui.label("Файл:");
                ui.add(egui::TextEdit::singleline(&mut f.name).desired_width(160.0).hint_text("часть имени"));
                if ui.button("Сбросить").clicked() {
                    *f = Filter::default();
                }
            });
            let range = f.date_range(today);
            let rows: Vec<&crate::received::Received> = match range {
                Ok(r) => self.received.iter().rev().filter(|x| f.matches(x, r)).collect(),
                Err(_) => Vec::new(),
            };
            ui.horizontal(|ui| {
                match range {
                    Err(e) => ui.label(RichText::new(format!("⚠ {e}")).color(Color32::from_rgb(190, 110, 0))),
                    Ok(_) => ui.label(RichText::new(format!("Найдено: {} из {}", rows.len(), self.received.len())).weak()),
                };
                if ui.button("Папка принятых файлов").clicked() {
                    let _ = std::fs::create_dir_all(&self.paths.downloads);
                    acts.push(Act::OpenPath(self.paths.downloads.display().to_string()));
                }
            });
            ui.separator();
            if self.received.is_empty() {
                ui.label("Принятых файлов пока нет.");
                return;
            }
            TableBuilder::new(ui)
                .striped(true)
                .resizable(true)
                .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
                .column(Column::initial(120.0))
                .column(Column::initial(150.0).clip(true))
                .column(Column::initial(220.0).clip(true))
                .column(Column::initial(70.0))
                .column(Column::remainder().at_least(130.0))
                .header(20.0, |mut h| {
                    for t in ["Дата и время", "От кого", "Файл", "Размер", ""] {
                        h.col(|ui| {
                            ui.strong(t);
                        });
                    }
                })
                .body(|body| {
                    body.rows(22.0, rows.len(), |mut row| {
                        let r = rows[row.index()];
                        let exists = std::path::Path::new(&r.path).exists();
                        row.col(|ui| {
                            ui.label(r.local_time().format("%d.%m.%Y %H:%M").to_string());
                        });
                        row.col(|ui| {
                            let l = ui.label(&r.nick);
                            if !r.computer.is_empty() {
                                l.on_hover_text(format!("Компьютер: {}", r.computer));
                            }
                        });
                        row.col(|ui| {
                            let t = if exists { RichText::new(&r.name) } else { RichText::new(&r.name).weak().strikethrough() };
                            ui.label(t).on_hover_text(if exists { r.path.clone() } else { format!("Файла больше нет: {}", r.path) });
                        });
                        row.col(|ui| {
                            ui.label(if r.size > 0 { fmt_size(r.size) } else { "—".into() });
                        });
                        row.col(|ui| {
                            if exists {
                                if ui.small_button("Открыть").clicked() {
                                    acts.push(Act::OpenPath(r.path.clone()));
                                }
                                if ui.small_button("Папка").clicked() {
                                    let dir = std::path::Path::new(&r.path).parent().map(|p| p.display().to_string()).unwrap_or_default();
                                    acts.push(Act::OpenPath(dir));
                                }
                            } else {
                                ui.label(RichText::new("файл удалён").weak());
                            }
                        });
                    });
                });
        });
        if !open {
            self.dlg.received = None;
        }
    }
}
