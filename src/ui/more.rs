//! Group chat page, remote desktop viewer windows, host-side prompts/indicator,
//! command line windows and related dialogs.

use super::{chat_input, chat_lines, header_bar, search_bar, search_icon_button, search_matches, sync_search, tool_button, Act, Highlight};
use crate::app::*;
use crate::config::*;
use eframe::egui::{self, load::SizedTexture, Color32, Key, RichText, Sense};
use std::sync::atomic::Ordering;
use std::time::Duration;

impl App {
    pub(super) fn groups_page(&mut self, ui: &mut egui::Ui, acts: &mut Vec<Act>) -> bool {
        let my = self.cfg.uid;
        header_bar(ui, self.icons.get("hd_chat"), "Групповой чат", &format!("бесед: {}", self.groups.len()));
        let font = self.cfg.chat.font_size;
        egui::SidePanel::left("conv_list").resizable(true).default_width(210.0).show_inside(ui, |ui| {
            ui.add_space(4.0);
            if tool_button(ui, self.icons.sized("p_add", 20.0), Some("Новая беседа"), 20.0, true, false)
                .on_hover_text("Создать беседу и выбрать участников")
                .clicked()
            {
                acts.push(Act::NewConv(Vec::new()));
            }
            ui.separator();
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                let mut list: Vec<&Conv> = self.groups.values().collect();
                list.sort_by_key(|c| std::cmp::Reverse(c.lines.last().map(|l| l.time).unwrap_or(0)));
                for c in list {
                    let id = c.info.id;
                    let mut t = format!("{} ({})", c.info.title, c.info.members.len());
                    if c.unread > 0 {
                        t.push_str(&format!("  ✉{}", c.unread));
                    }
                    if !c.is_member(my) {
                        t.push_str("  [вы не участник]");
                    }
                    let r = tool_button(ui, self.icons.sized("hd_chat", 16.0), Some(&t), 16.0, true, self.group_sel == Some(id));
                    let names: Vec<String> =
                        c.info.members.iter().map(|u| c.info.name_of(*u).map(String::from).unwrap_or_else(|| self.peer_name(*u))).collect();
                    let r = r.on_hover_text(format!("Участники: {}", names.join(", ")));
                    if r.clicked() {
                        acts.push(Act::OpenGroup(id));
                    }
                    let member = c.is_member(my);
                    r.context_menu(|ui| {
                        if member {
                            if ui.button("✏ Переименовать…").clicked() {
                                acts.push(Act::RenameConv(id));
                                ui.close_menu();
                            }
                            if ui.button("🚪 Выйти из беседы").clicked() {
                                acts.push(Act::GroupRemove(id, my));
                                ui.close_menu();
                            }
                        }
                        if ui.button("🗑 Удалить из списка").clicked() {
                            acts.push(Act::DeleteConv(id));
                            ui.close_menu();
                        }
                    });
                }
                if self.groups.is_empty() {
                    ui.label(
                        RichText::new("Бесед пока нет.\nНажмите «Новая беседа» или правой кнопкой по пользователю → «Пригласить в беседу».").weak(),
                    );
                }
            });
        });

        let Some(id) = self.group_sel.filter(|g| self.groups.contains_key(g)) else {
            egui::CentralPanel::default().show_inside(ui, |ui| {
                ui.centered_and_justified(|ui| {
                    ui.label(RichText::new("Выберите беседу слева или создайте новую").weak().size(15.0));
                });
            });
            return false;
        };

        egui::SidePanel::right("conv_members").resizable(true).default_width(180.0).show_inside(ui, |ui| {
            let c = &self.groups[&id];
            let member = c.is_member(my);
            let owner = c.info.owner == my;
            ui.add_space(4.0);
            ui.strong(format!("Участники ({})", c.info.members.len()));
            ui.separator();
            for &u in &c.info.members {
                let online = u == my || self.peers.contains_key(&u);
                let st = if u == my { self.cfg.state } else { self.peers.get(&u).map(|p| p.p.state).unwrap_or(ST_OFFLINE) };
                let name = if u == my {
                    format!("{} (вы)", self.cfg.nick)
                } else {
                    self.peers
                        .get(&u)
                        .map(|p| p.p.nick.clone())
                        .or_else(|| c.info.name_of(u).map(String::from))
                        .unwrap_or_else(|| self.peer_name(u))
                };
                let label = if u == c.info.owner { format!("{name} ★") } else { name };
                let r = tool_button(ui, self.icons.sized(self.icons.state(st), 16.0), Some(&label), 16.0, true, false)
                    .on_hover_text(if online { "в сети" } else { "не в сети — сообщения будут доставлены, когда появится" });
                if u != my {
                    if r.double_clicked() {
                        acts.push(Act::Chat(u));
                    }
                    r.context_menu(|ui| {
                        if ui.button("Написать личное сообщение").clicked() {
                            acts.push(Act::Chat(u));
                            ui.close_menu();
                        }
                        if member && owner && ui.button("Исключить из беседы").clicked() {
                            acts.push(Act::GroupRemove(id, u));
                            ui.close_menu();
                        }
                    });
                }
            }
            ui.separator();
            if member {
                ui.menu_button("➕ Добавить участника", |ui| {
                    let mut cand: Vec<(u64, String, bool)> =
                        self.peers.iter().filter(|(u, _)| !c.is_member(**u)).map(|(u, p)| (*u, p.p.nick.clone(), true)).collect();
                    for (u, (nick, _)) in &self.cfg.known {
                        if !c.is_member(*u) && !self.peers.contains_key(u) && *u != my {
                            cand.push((*u, nick.clone(), false));
                        }
                    }
                    if cand.is_empty() {
                        ui.label("Все известные пользователи уже в беседе");
                    }
                    for (u, nick, online) in cand {
                        let t = if online { nick } else { format!("{nick} (не в сети)") };
                        if ui.button(t).clicked() {
                            acts.push(Act::GroupAdd(id, u));
                            ui.close_menu();
                        }
                    }
                });
                if ui.button("✏ Переименовать…").clicked() {
                    acts.push(Act::RenameConv(id));
                }
                if ui.button("🚪 Выйти из беседы").clicked() {
                    acts.push(Act::GroupRemove(id, my));
                }
            } else {
                ui.label(RichText::new("Вы не участник этой беседы — история только для чтения").weak());
                if ui.button("🗑 Удалить из списка").clicked() {
                    acts.push(Act::DeleteConv(id));
                }
            }
        });

        let nicks: Vec<String> = self.peers.values().map(|p| p.p.nick.clone()).collect();
        let emo_icon = self.icons.sized("tb_emoticon", 22.0);
        let emo = if self.cfg.chat.no_emoticons { None } else { self.emo.as_ref() };
        let focused = !self.user_away();
        let show_system = self.cfg.chat.show_system;
        let macros = &mut self.cfg.chat.macros;
        let c = self.groups.get_mut(&id).unwrap();
        if focused {
            c.unread = 0;
        }
        let member = c.is_member(my);
        egui::TopBottomPanel::bottom("conv_input").show_inside(ui, |ui| {
            ui.add_space(3.0);
            if member {
                let out = chat_input(ui, &mut c.input, ("conv_input", id), emo, emo_icon, macros, &nicks, font);
                if let Some(t) = out.submit {
                    acts.push(Act::GroupSend(id, t));
                }
            } else {
                ui.label(RichText::new("Вы не участник этой беседы").weak());
            }
        });
        let mut animated = false;
        egui::CentralPanel::default().frame(egui::Frame::none().fill(Color32::WHITE).inner_margin(6.0)).show_inside(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new(&c.info.title).strong().size(16.0));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if search_icon_button(ui, c.search.open, Color32::from_gray(70)).clicked() {
                        c.search.toggle();
                    }
                });
            });
            let (needle, matches) = search_matches(&c.lines, &c.search);
            sync_search(&mut c.search, &c.lines, &matches);
            if c.search.open {
                search_bar(ui, &mut c.search, ("conv_search", id), matches.len());
            }
            ui.separator();
            let searching = !needle.is_empty();
            let mut hl = Highlight { current: matches.get(c.search.cur).map(|i| c.lines[*i].id), needle, scroll: c.search.scroll, only: c.search.only };
            egui::ScrollArea::vertical().auto_shrink([false, false]).stick_to_bottom(!searching).show(ui, |ui| {
                animated = chat_lines(ui, &c.lines, emo, my, font, true, show_system, &mut hl);
                if c.lines.is_empty() {
                    ui.label(RichText::new("Сообщения видят только участники беседы.").weak());
                }
            });
            c.search.scroll = hl.scroll;
        });
        animated
    }

    pub(super) fn remote_dialogs(&mut self, ctx: &egui::Context, _acts: &mut Vec<Act>) {
        // ---- new conversation
        if self.dlg.new_conv.is_some() {
            let mut done: Option<bool> = None;
            let peers: Vec<(u64, String, bool)> = {
                let mut v: Vec<(u64, String, bool)> = self.peers.values().map(|p| (p.uid, p.p.nick.clone(), true)).collect();
                v.sort_by(|a, b| a.1.to_lowercase().cmp(&b.1.to_lowercase()));
                let mut off: Vec<(u64, String, bool)> = self
                    .cfg
                    .known
                    .iter()
                    .filter(|(u, _)| !self.peers.contains_key(u) && **u != self.cfg.uid)
                    .map(|(u, (n, _))| (*u, n.clone(), false))
                    .collect();
                off.sort_by(|a, b| a.1.to_lowercase().cmp(&b.1.to_lowercase()));
                v.extend(off);
                v
            };
            let (title, sel) = self.dlg.new_conv.as_mut().unwrap();
            egui::Window::new("Новая беседа").collapsible(false).resizable(false).default_width(340.0).show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label("Название:");
                    ui.add(egui::TextEdit::singleline(title).hint_text("например, «Бухгалтерия»").desired_width(220.0));
                });
                ui.label("Участники:");
                egui::ScrollArea::vertical().max_height(260.0).show(ui, |ui| {
                    if peers.is_empty() {
                        ui.label(RichText::new("Пока никого нет в сети").weak());
                    }
                    for (u, nick, online) in &peers {
                        let mut on = sel.contains(u);
                        let t = if *online { nick.clone() } else { format!("{nick} (не в сети)") };
                        if ui.checkbox(&mut on, t).changed() {
                            if on {
                                sel.push(*u);
                            } else {
                                sel.retain(|x| x != u);
                            }
                        }
                    }
                });
                ui.separator();
                ui.horizontal(|ui| {
                    if ui.add_enabled(!sel.is_empty(), egui::Button::new("Создать")).on_disabled_hover_text("Отметьте хотя бы одного участника").clicked() {
                        done = Some(true);
                    }
                    if ui.button("Отмена").clicked() {
                        done = Some(false);
                    }
                    ui.label(RichText::new(format!("выбрано: {}", sel.len())).weak());
                });
            });
            if let Some(ok) = done {
                let (t, s) = self.dlg.new_conv.take().unwrap();
                if ok {
                    self.create_group(t, s);
                }
            }
        }
        // ---- rename
        if let Some((id, title)) = &mut self.dlg.rename_conv {
            let mut done = None;
            egui::Window::new("Переименовать беседу").collapsible(false).resizable(false).show(ctx, |ui| {
                ui.text_edit_singleline(title).request_focus();
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
                let (id, t) = (*id, title.clone());
                self.dlg.rename_conv = None;
                if ok {
                    self.group_rename(id, t);
                }
            }
        }
        // ---- connect
        if self.dlg.connect.is_some() {
            let mut done = None;
            let uid = self.dlg.connect.as_ref().unwrap().uid;
            let (nick, comp, mode) =
                self.peers.get(&uid).map(|p| (p.p.nick.clone(), p.p.computer.clone(), p.p.allow.remote)).unwrap_or_default();
            let c = self.dlg.connect.as_mut().unwrap();
            egui::Window::new("Удалённый доступ").collapsible(false).resizable(false).default_width(360.0).show(ctx, |ui| {
                ui.label(RichText::new(format!("{nick} — {comp}")).strong().size(15.0));
                ui.add_space(4.0);
                ui.radio_value(&mut c.view_only, false, "🖱 Управление (мышь и клавиатура)");
                ui.radio_value(&mut c.view_only, true, "👁 Только просмотр");
                ui.add_space(4.0);
                if mode == 0 {
                    ui.label(RichText::new("Пользователь не в сети или у него старая версия программы").color(Color32::RED));
                }
                ui.horizontal(|ui| {
                    ui.label("🔑 Пароль администратора:");
                    let r = ui.add(egui::TextEdit::singleline(&mut c.password).password(true).desired_width(160.0));
                    if r.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) && !c.password.is_empty() {
                        done = Some(true);
                    }
                });
                ui.separator();
                ui.horizontal(|ui| {
                    if ui.add_enabled(mode != 0 && !c.password.is_empty(), egui::Button::new("Подключиться")).clicked() {
                        done = Some(true);
                    }
                    if ui.button("Отмена").clicked() {
                        done = Some(false);
                    }
                });
            });
            if let Some(ok) = done {
                let c = self.dlg.connect.take().unwrap();
                if ok {
                    self.remote_connect(c.uid, c.view_only, c.password);
                }
            }
        }
        // ---- power confirmation
        if self.dlg.power.is_some() {
            let (uid, a) = {
                let p = self.dlg.power.as_ref().unwrap();
                (p.0, p.1)
            };
            let what = ["выключить", "перезагрузить", "завершить сеанс пользователя на", "заблокировать"][a.min(3) as usize];
            let nick = self.peer_name(uid);
            let mut done = None;
            let pw = &mut self.dlg.power.as_mut().unwrap().2;
            egui::Window::new("Подтверждение").collapsible(false).resizable(false).show(ctx, |ui| {
                ui.label(format!("Действительно {what} компьютер пользователя {nick}?"));
                ui.label(RichText::new("Несохранённые данные пользователя могут быть потеряны.").small().weak());
                ui.horizontal(|ui| {
                    ui.label("🔑 Пароль администратора:");
                    ui.add(egui::TextEdit::singleline(pw).password(true).desired_width(150.0));
                });
                ui.horizontal(|ui| {
                    if ui.add_enabled(!pw.is_empty(), egui::Button::new("Выполнить")).clicked() {
                        done = Some(true);
                    }
                    if ui.button("Отмена").clicked() {
                        done = Some(false);
                    }
                });
            });
            if let Some(ok) = done {
                let (uid, a, pw) = self.dlg.power.take().unwrap();
                if ok {
                    self.remote_power(uid, a, pw);
                }
            }
        }
        // ---- command line windows
        let uids: Vec<u64> = self.cmds.iter().filter(|(_, c)| c.open).map(|(u, _)| *u).collect();
        for uid in uids {
            let nick = self.peer_name(uid);
            let mut run: Option<String> = None;
            let mut open = true;
            let c = self.cmds.get_mut(&uid).unwrap();
            egui::Window::new(format!("Командная строка — {nick}"))
                .id(egui::Id::new(("cmdwin", uid)))
                .open(&mut open)
                .default_size([620.0, 420.0])
                .show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        ui.label("🔑 Пароль администратора:");
                        ui.add(egui::TextEdit::singleline(&mut c.password).password(true).desired_width(150.0));
                        ui.label(RichText::new("проверяется для каждой команды").small().weak());
                    });
                    ui.label(RichText::new("Команды выполняются через cmd /C от имени пользователя удалённого ПК.").small().weak());
                    ui.horizontal_wrapped(|ui| {
                        for p in [
                            "ipconfig /all",
                            "systeminfo",
                            "tasklist",
                            "whoami",
                            "gpupdate /force",
                            "ipconfig /flushdns",
                            "netstat -an",
                            "wmic logicaldisk get caption,freespace,size",
                        ] {
                            if ui.add_enabled(!c.busy, egui::Button::new(p).small()).clicked() {
                                run = Some(p.to_string());
                            }
                        }
                    });
                    egui::TopBottomPanel::bottom(egui::Id::new(("cmdin", uid))).show_inside(ui, |ui| {
                        ui.horizontal(|ui| {
                            let r = ui.add(
                                egui::TextEdit::singleline(&mut c.input)
                                    .desired_width(ui.available_width() - 110.0)
                                    .font(egui::TextStyle::Monospace)
                                    .hint_text("команда…"),
                            );
                            let enter = r.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
                            if (ui.add_enabled(!c.busy, egui::Button::new("▶ Выполнить")).clicked() || enter) && !c.input.trim().is_empty() && !c.busy {
                                run = Some(c.input.trim().to_string());
                                c.input.clear();
                                r.request_focus();
                            }
                            if c.busy {
                                ui.spinner();
                            }
                        });
                    });
                    egui::CentralPanel::default().frame(egui::Frame::none().fill(Color32::from_rgb(12, 12, 12)).inner_margin(6.0)).show_inside(ui, |ui| {
                        egui::ScrollArea::both().auto_shrink([false, false]).stick_to_bottom(true).show(ui, |ui| {
                            ui.label(RichText::new(&c.output).monospace().color(Color32::from_rgb(210, 210, 210)));
                        });
                    });
                });
            if !open {
                c.open = false;
            }
            if let Some(cmd) = run {
                self.remote_exec(uid, cmd);
            }
        }
    }

    pub(super) fn viewer_windows(&mut self, ctx: &egui::Context, acts: &mut Vec<Act>) {
        use crate::proto::RemoteInput as RI;
        use crate::remote::*;
        for v in &mut self.viewers {
            let (connected, status, closed, monitors, cursor, frames, bytes) = {
                let mut st = v.state.lock().unwrap();
                if st.dirty {
                    if let Some(c) = &st.canvas {
                        let ci = egui::ColorImage::from_rgba_unmultiplied([c.width() as usize, c.height() as usize], c.as_raw());
                        match &mut v.tex {
                            Some(t) => t.set(ci, egui::TextureOptions::LINEAR),
                            None => v.tex = Some(ctx.load_texture(format!("rv:{}", v.id), ci, egui::TextureOptions::LINEAR)),
                        }
                    }
                    st.dirty = false;
                }
                (st.connected, st.status.clone(), st.closed, st.monitors, st.cursor, st.frames, st.bytes)
            };
            let dt = v.stats_t.elapsed().as_secs_f32();
            if dt >= 1.0 {
                v.rate = ((frames - v.stats_frames) as f32 / dt, (bytes - v.stats_bytes) as f32 / dt / 1024.0);
                v.stats_frames = frames;
                v.stats_bytes = bytes;
                v.stats_t = std::time::Instant::now();
            }
            let title = format!("{} {} — Nassi RS", if v.view_only { "Просмотр:" } else { "Управление:" }, v.nick);
            let vid = egui::ViewportId::from_hash_of(("rv", v.id));
            let mut close = false;
            ctx.show_viewport_immediate(
                vid,
                egui::ViewportBuilder::default().with_title(title).with_inner_size([1100.0, 720.0]).with_min_inner_size([520.0, 340.0]),
                |ctx, _| {
                    if ctx.input(|i| i.viewport().close_requested()) {
                        close = true;
                    }
                    egui::TopBottomPanel::top("rv_tools").show(ctx, |ui| {
                        ui.add_space(2.0);
                        ui.horizontal_wrapped(|ui| {
                            let col = if connected {
                                Color32::from_rgb(40, 160, 40)
                            } else if closed {
                                Color32::RED
                            } else {
                                Color32::from_rgb(220, 150, 0)
                            };
                            ui.label(RichText::new("●").color(col));
                            ui.label(&status);
                            ui.separator();
                            if tool_button(ui, None, Some("Вписать"), 18.0, true, v.fit).on_hover_text("Масштабировать под окно / 1:1").clicked() {
                                v.fit = !v.fit;
                            }
                            let full = v.fullscreen;
                            if tool_button(ui, None, Some("⛶ Во весь экран"), 18.0, true, full).clicked() {
                                v.fullscreen = !full;
                                ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(v.fullscreen));
                            }
                            let mut changed = false;
                            egui::ComboBox::from_id_salt(("rvmon", v.id))
                                .width(110.0)
                                .selected_text(if v.monitor == 0 { "Все мониторы".to_string() } else { format!("Монитор {}", v.monitor) })
                                .show_ui(ui, |ui| {
                                    for m in 1..=monitors.max(1) {
                                        changed |= ui.selectable_value(&mut v.monitor, m, format!("Монитор {m}")).changed();
                                    }
                                    changed |= ui.selectable_value(&mut v.monitor, 0, "Все мониторы").changed();
                                });
                            egui::ComboBox::from_id_salt(("rvfps", v.id)).width(70.0).selected_text(format!("{} к/с", v.fps)).show_ui(ui, |ui| {
                                for f in [2u8, 5, 10, 15, 20, 30] {
                                    changed |= ui.selectable_value(&mut v.fps, f, format!("{f} к/с")).changed();
                                }
                            });
                            let r = ui.add(egui::Slider::new(&mut v.quality, 10..=95).text("качество"));
                            if r.drag_stopped() || (r.changed() && !r.dragged()) {
                                changed = true;
                            }
                            if changed {
                                v.send_settings();
                                v.send(RI::Full);
                            }
                            ui.separator();
                            let ctl = connected && !v.view_only;
                            if tool_button(ui, None, Some("Win"), 18.0, ctl, false).on_hover_text("Клавиша Windows").clicked() {
                                v.combo(&[VK_LWIN]);
                            }
                            if tool_button(ui, None, Some("Alt+Tab"), 18.0, ctl, false).clicked() {
                                v.combo(&[VK_MENU, VK_TAB]);
                            }
                            if tool_button(ui, None, Some("Ctrl+Shift+Esc"), 18.0, ctl, false).on_hover_text("Диспетчер задач").clicked() {
                                v.combo(&[VK_CONTROL, VK_SHIFT, VK_ESCAPE]);
                            }
                            if tool_button(ui, None, Some("Win+R"), 18.0, ctl, false).on_hover_text("Окно «Выполнить»").clicked() {
                                v.combo(&[VK_LWIN, 0x52]);
                            }
                            if tool_button(ui, None, Some("📋 Буфер →"), 18.0, ctl, false)
                                .on_hover_text("Передать текст из моего буфера обмена в буфер удалённого ПК")
                                .clicked()
                            {
                                if let Ok(t) = arboard::Clipboard::new().and_then(|mut c| c.get_text()) {
                                    v.send(RI::Clipboard(t));
                                }
                            }
                            ui.separator();
                            if tool_button(ui, None, Some("📄 Файл…"), 18.0, connected, false).clicked() {
                                acts.push(Act::File(v.uid));
                            }
                            if tool_button(ui, None, Some("⌨ Команды"), 18.0, true, false).clicked() {
                                acts.push(Act::Cmd(v.uid));
                            }
                            ui.menu_button("⏻ Питание", |ui| {
                                for (a, n) in [(3u8, "Заблокировать"), (2, "Выйти из системы"), (1, "Перезагрузить"), (0, "Выключить")] {
                                    if ui.button(n).clicked() {
                                        acts.push(Act::Power(v.uid, a));
                                        ui.close_menu();
                                    }
                                }
                            });
                            ui.separator();
                            if tool_button(ui, None, Some("✖ Отключиться"), 18.0, true, false).clicked() {
                                close = true;
                            }
                            ui.label(RichText::new(format!("{:.0} к/с · {:.0} КБ/с", v.rate.0, v.rate.1)).small().weak());
                        });
                        ui.add_space(2.0);
                    });
                    egui::CentralPanel::default().frame(egui::Frame::none().fill(Color32::from_rgb(30, 30, 30))).show(ctx, |ui| {
                        let Some(tex) = v.tex.clone() else {
                            ui.centered_and_justified(|ui| {
                                ui.label(RichText::new(&status).color(Color32::WHITE).size(16.0));
                            });
                            return;
                        };
                        let size = tex.size_vec2();
                        let avail = ui.available_size();
                        let scale = if v.fit { (avail.x / size.x).min(avail.y / size.y).max(0.05) } else { 1.0 };
                        let image = egui::Image::from_texture(SizedTexture::new(tex.id(), size * scale)).sense(Sense::click_and_drag());
                        let resp = if v.fit {
                            ui.centered_and_justified(|ui| ui.add(image)).inner
                        } else {
                            egui::ScrollArea::both().auto_shrink([false, false]).show(ui, |ui| ui.add(image)).inner
                        };
                        let rect = resp.rect;
                        // remote cursor marker
                        let cp = rect.min + egui::vec2(cursor.0 as f32, cursor.1 as f32) * scale;
                        if rect.contains(cp) && !resp.hovered() {
                            ui.painter().add(egui::Shape::convex_polygon(
                                vec![cp, cp + egui::vec2(0.0, 16.0), cp + egui::vec2(4.5, 12.0), cp + egui::vec2(11.0, 11.0)],
                                Color32::WHITE,
                                egui::Stroke::new(1.0f32, Color32::BLACK),
                            ));
                        }
                        if !(connected && !v.view_only) {
                            return;
                        }
                        if resp.clicked() || resp.drag_started() || resp.secondary_clicked() {
                            resp.request_focus();
                        }
                        ui.memory_mut(|m| {
                            m.set_focus_lock_filter(
                                resp.id,
                                egui::EventFilter { tab: true, horizontal_arrows: true, vertical_arrows: true, escape: true },
                            )
                        });
                        let focused_vp = ctx.input(|i| i.viewport().focused.unwrap_or(false));
                        let to_remote = |pos: egui::Pos2| -> (i32, i32) {
                            let q = (pos - rect.min) / scale;
                            (q.x.clamp(0.0, size.x - 1.0) as i32, q.y.clamp(0.0, size.y - 1.0) as i32)
                        };
                        let set_mods = |v: &mut Viewer, m: egui::Modifiers| {
                            for (now, before, vk) in [(m.shift, v.mods.shift, VK_SHIFT), (m.ctrl, v.mods.ctrl, VK_CONTROL), (m.alt, v.mods.alt, VK_MENU)] {
                                if now != before {
                                    v.send(RI::Key { vk, down: now });
                                }
                            }
                            v.mods = m;
                        };
                        if !focused_vp {
                            // release held modifiers so nothing gets stuck on the remote side
                            if v.mods != egui::Modifiers::NONE {
                                set_mods(v, egui::Modifiers::NONE);
                            }
                            return;
                        }
                        let events = ctx.input(|i| i.events.clone());
                        let mut last_move = None;
                        for ev in events {
                            match ev {
                                egui::Event::PointerMoved(pos) if rect.contains(pos) => last_move = Some(to_remote(pos)),
                                egui::Event::PointerButton { pos, button, pressed, modifiers } if rect.contains(pos) || !pressed => {
                                    set_mods(v, modifiers);
                                    let p = to_remote(pos);
                                    v.send(RI::Mouse { x: p.0, y: p.1 });
                                    let b = match button {
                                        egui::PointerButton::Primary => 0,
                                        egui::PointerButton::Secondary => 1,
                                        _ => 2,
                                    };
                                    v.send(RI::Button { b, down: pressed });
                                }
                                egui::Event::Key { key, pressed, modifiers, .. } => {
                                    set_mods(v, modifiers);
                                    if let Some(vk) = vk_of(key) {
                                        if is_special(vk) || modifiers.ctrl || modifiers.alt || modifiers.command {
                                            v.send(RI::Key { vk, down: pressed });
                                        }
                                    }
                                }
                                egui::Event::Text(t) => {
                                    if !(v.mods.ctrl || v.mods.alt) {
                                        v.send(RI::Text(t));
                                    }
                                }
                                egui::Event::Copy => v.combo(&[0x43]),
                                egui::Event::Cut => v.combo(&[0x58]),
                                egui::Event::Paste(_) => v.combo(&[0x56]),
                                _ => {}
                            }
                        }
                        if let Some((x, y)) = last_move {
                            if v.last_mouse != Some((x, y)) {
                                v.last_mouse = Some((x, y));
                                v.send(RI::Mouse { x, y });
                            }
                        }
                        let m = ctx.input(|i| i.modifiers);
                        set_mods(v, m);
                        if resp.hovered() {
                            let d = ctx.input(|i| i.raw_scroll_delta);
                            if d != egui::Vec2::ZERO {
                                v.send(RI::Wheel { dx: (d.x / 50.0 * 120.0) as i32, dy: (d.y / 50.0 * 120.0) as i32 });
                            }
                        }
                    });
                },
            );
            if close {
                v.send(RI::Close);
                v.open = false;
            }
        }
        let before = self.viewers.len();
        self.viewers.retain(|v| v.open);
        if self.viewers.len() != before {
            self.flash("Удалённый сеанс закрыт");
        }
    }

    /// Host side: the always-visible "your PC is controlled" indicator.
    pub(super) fn host_popups(&mut self, ctx: &egui::Context) {
        let anchor = self.main_pos.unwrap_or(egui::pos2(100.0, 100.0));
        let (ax, ay, aw, _) = crate::platform::work_area_at(anchor.x as i32 + 50, anchor.y as i32 + 50);
        let ppp = ctx.pixels_per_point();
        let (ax, ay, aw) = (ax as f32 / ppp, ay as f32 / ppp, aw as f32 / ppp);
        if !self.host_sessions.is_empty() {
            let (w, h) = (460.0f32, 12.0 + 30.0 * self.host_sessions.len() as f32);
            let mut stop: Vec<u64> = Vec::new();
            let sessions = &self.host_sessions;
            ctx.show_viewport_immediate(
                egui::ViewportId::from_hash_of("remote_indicator"),
                egui::ViewportBuilder::default()
                    .with_title("Удалённое подключение — Nassi RS")
                    .with_decorations(false)
                    .with_always_on_top()
                    .with_taskbar(false)
                    .with_resizable(false)
                    .with_inner_size([w, h])
                    .with_position(egui::pos2(ax + (aw - w) / 2.0, ay + 2.0)),
                |ctx, _| {
                    egui::CentralPanel::default().frame(egui::Frame::none().fill(Color32::from_rgb(200, 40, 40)).inner_margin(6.0)).show(ctx, |ui| {
                        for s in sessions {
                            ui.horizontal(|ui| {
                                ui.label(
                                    RichText::new(format!(
                                        "🖥 {}: {} ({})",
                                        if s.view_only { "Ваш экран просматривает" } else { "Вашим ПК управляет" },
                                        s.who,
                                        fmt_dur(s.since.elapsed().as_secs())
                                    ))
                                    .color(Color32::WHITE)
                                    .strong(),
                                );
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    if ui.button("Отключить").clicked() {
                                        stop.push(s.sid);
                                    }
                                });
                            });
                        }
                    });
                },
            );
            for sid in stop {
                if let Some(s) = self.host_sessions.iter().find(|s| s.sid == sid) {
                    s.stop.store(true, Ordering::Relaxed);
                    let msg = format!("Сеанс {} прерван вами", s.who);
                    self.log(Level::Info, "Удалённый доступ", msg);
                }
                self.host_sessions.retain(|s| s.sid != sid);
            }
            ctx.request_repaint_after(Duration::from_millis(1000));
        }
    }
}
