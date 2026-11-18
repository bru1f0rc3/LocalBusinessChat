//! Application state and logic (network events, timers, persistence).
//! Rendering lives in `ui.rs`.

use crate::config::*;
use crate::emoticons::Pack;
use crate::icons::Icons;
use crate::net::{Net, NetEvent, Shared};
use crate::proto::*;
use eframe::egui::{self, TextureHandle};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::io::Write;
use std::net::{IpAddr, SocketAddr};
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Page {
    Users,
    Chat,
    Private,
    Groups,
    Events,
}

/// Search inside one conversation (magnifier in the top right corner of a chat).
#[derive(Default)]
pub struct ChatSearch {
    pub open: bool,
    pub query: String,
    /// index into the list of matches (oldest first)
    pub cur: usize,
    /// the query the matches were computed for (to reset `cur` when it changes)
    pub last_query: String,
    pub focus: bool,
    /// scroll to the current match on the next frame
    pub scroll: bool,
    /// show only the matching messages
    pub only: bool,
    /// jump to this message id once matches are known
    pub jump_to: Option<u64>,
}

impl ChatSearch {
    pub fn active(&self) -> bool {
        self.open && !self.query.trim().is_empty()
    }
    pub fn toggle(&mut self) {
        self.open = !self.open;
        self.focus = self.open;
        if !self.open {
            self.query.clear();
            self.last_query.clear();
        }
    }
    pub fn start(&mut self) {
        self.open = true;
        self.focus = true;
    }
}

/// A group conversation ("беседа").
pub struct Conv {
    pub info: GroupInfo,
    pub lines: Vec<Line>,
    pub ids: HashSet<u64>,
    pub unread: u32,
    pub input: ChatInput,
    pub dirty: bool,
    pub meta_dirty: bool,
    pub search: ChatSearch,
}

impl Conv {
    pub fn is_member(&self, uid: u64) -> bool {
        self.info.members.contains(&uid)
    }
    pub fn pending_count(&self) -> usize {
        self.lines.iter().filter(|l| !l.pending.is_empty()).count()
    }
}

/// Somebody is connected to THIS desktop.
pub struct HostSession {
    pub sid: u64,
    pub who: String,
    pub view_only: bool,
    pub stop: Arc<AtomicBool>,
    pub since: Instant,
}

pub struct ConnectDlg {
    pub uid: u64,
    pub view_only: bool,
    pub password: String,
}

pub struct Peer {
    pub uid: u64,
    pub p: Presence,
    pub ip: IpAddr,
    pub last_seen: Instant,
    pub ping_ms: Option<u32>,
    pub details: Option<UserDetails>,
    pub details_rev: i64,
    pub avatar: Option<TextureHandle>,
    pub typing_until: Option<Instant>,
    pub refresh_pending: bool,
}

impl Peer {
    pub fn tcp_addr(&self) -> SocketAddr {
        SocketAddr::new(self.ip, self.p.tcp_port)
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct Line {
    pub id: u64,
    pub time: i64,
    /// sender uid; 0 = system line
    pub from: u64,
    pub nick: String,
    pub color: [u8; 3],
    pub text: String,
    pub me: bool,
    pub delivered: bool,
    /// group chats: members that have not acknowledged this (my) message yet
    pub pending: Vec<u64>,
}

#[derive(Default)]
pub struct ChatInput {
    pub text: String,
    pub history: Vec<String>,
    pub hist_pos: Option<usize>,
    pub focus: bool,
}

pub struct PrivateChat {
    pub uid: u64,
    pub lines: Vec<Line>,
    pub ids: HashSet<u64>,
    pub search: ChatSearch,
    pub input: ChatInput,
    pub unread: u32,
    pub dirty: bool,
    pub last_auto_answer: Option<Instant>,
    pub last_typing_sent: Option<Instant>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum InfoTab {
    Details,
    Processes,
    Screenshot,
    Clipboard,
}

pub struct InfoWin {
    pub uid: u64,
    pub tab: InfoTab,
    pub details: Option<UserDetails>,
    pub avatar: Option<TextureHandle>,
    pub procs: Vec<ProcInfo>,
    pub sel_pid: Option<u32>,
    pub shot: Option<(TextureHandle, u32, u32)>,
    pub shot_jpeg: Vec<u8>,
    pub monitors: u8,
    pub monitor: u8,
    pub quality: u8,
    pub fit: bool,
    pub clipboard: String,
    pub status: String,
    pub busy: bool,
    pub auto_secs: u32,
    pub last_auto: Option<Instant>,
}

#[derive(Clone, Copy, Debug)]
pub enum Pending {
    Details { uid: u64, for_window: bool },
    Processes(u64),
    Kill(u64),
    Screenshot(u64),
    Clipboard(u64),
    Exec(u64),
    Power(u64),
}

#[derive(Clone, Debug, PartialEq)]
pub enum TState {
    Offered,
    Waiting,
    Running,
    Done,
    Failed(String),
    Declined,
}

pub struct Transfer {
    pub id: u64,
    pub peer: u64,
    pub nick: String,
    pub name: String,
    pub size: u64,
    pub incoming: bool,
    pub state: TState,
    pub done: u64,
    pub path: Option<PathBuf>,
    pub cancel: Arc<AtomicBool>,
    pub started: Instant,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Level {
    Info,
    Warn,
    Error,
}

pub struct EventRec {
    pub n: u32,
    pub time: chrono::DateTime<chrono::Local>,
    pub level: Level,
    pub source: String,
    pub text: String,
}

pub struct Tip {
    pub title: String,
    pub text: String,
    pub until: Instant,
    pub chat_uid: Option<u64>,
    pub group: Option<u64>,
}

#[derive(Default)]
pub struct Dialogs {
    pub setup: Option<crate::ui::SetupState>,
    pub details_edit: Option<(UserDetails, Option<TextureHandle>)>,
    pub state_edit: Option<(usize, StateDef)>,
    pub new_group: Option<String>,
    pub nick_edit: Option<String>,
    pub transfers: bool,
    pub about: bool,
    pub help: bool,
    /// new conversation: (title, selected members)
    pub new_conv: Option<(String, Vec<u64>)>,
    pub rename_conv: Option<(u64, String)>,
    pub connect: Option<ConnectDlg>,
    /// (uid, action) waiting for confirmation
    pub power: Option<(u64, u8, String)>,
    /// "clear private history with uid?" confirmation
    pub clear_chat: Option<u64>,
    /// Действия → «История принятых файлов» (open while Some)
    pub received: Option<crate::received::Filter>,
}

pub struct App {
    pub ctx: egui::Context,
    pub cfg: Config,
    pub cfg_dirty: bool,
    pub paths: Paths,
    pub assets: Option<PathBuf>,
    #[allow(dead_code)]
    pub profile: Option<String>,
    pub net: Option<Net>,
    pub rx: Option<Receiver<NetEvent>>,
    pub shared: Arc<Mutex<Shared>>,
    pub icons: Icons,
    pub emo: Option<Pack>,
    pub emo_packs: Vec<PathBuf>,

    pub page: Page,
    pub peers: BTreeMap<u64, Peer>,
    pub selected: Option<u64>,
    pub sort: (crate::config::Col, bool),

    pub global: Vec<Line>,
    pub global_ids: HashSet<u64>,
    pub global_input: ChatInput,
    pub global_unread: bool,
    pub global_search: ChatSearch,
    /// "Личные сообщения": selected dialog and the search box above the dialog list
    pub private_sel: Option<u64>,
    pub dm_filter: String,
    pub dm_filter_focus: bool,
    /// search box of the users list
    pub users_filter: String,
    pub users_filter_focus: bool,
    /// a message came while I was away: blink the taskbar button / tray icon until I am back
    pub alert: bool,
    pub alert_since: Instant,
    pub alert_flashing: bool,
    pub chats: HashMap<u64, PrivateChat>,
    pub groups: BTreeMap<u64, Conv>,
    pub group_sel: Option<u64>,
    pub viewers: Vec<crate::remote::Viewer>,
    /// standalone command-line windows (uid -> state)
    pub cmds: HashMap<u64, crate::remote::CmdState>,
    pub host_sessions: Vec<HostSession>,
    pub flash: Option<(String, Instant)>,
    pub infos: HashMap<u64, InfoWin>,
    pub pending: HashMap<u64, Pending>,
    pub transfers: Vec<Transfer>,
    /// history of received files (src/received.rs), oldest first
    pub received: Vec<crate::received::Received>,
    pub events: Vec<EventRec>,
    pub events_unread: bool,
    pub tips: VecDeque<Tip>,
    pub dlg: Dialogs,

    pub started: Instant,
    pub state_since: Instant,
    pub auto_away_prev: Option<u8>,
    pub last_presence: Instant,
    pub last_ping: Instant,
    pub last_resend: Instant,
    pub last_tick: Instant,
    pub refresh_deadline: Option<Instant>,
    pub focused: bool,
    pub visible: bool,
    pub minimized: bool,
    pub quit: bool,
    pub start_hidden: bool,
    pub os_name: String,
    pub computer: String,
    pub os_user: String,
    pub tray: Option<crate::tray::Tray>,
    pub main_pos: Option<egui::Pos2>,
    pub updater: crate::update::Updater,
    next_event_n: u32,
}

fn new_id() -> u64 {
    rand::random::<u64>() | 1
}

pub fn fmt_dur(secs: u64) -> String {
    let (h, m, s) = (secs / 3600, (secs / 60) % 60, secs % 60);
    if h >= 24 {
        format!("{}д {:02}:{:02}:{:02}", h / 24, h % 24, m, s)
    } else {
        format!("{:02}:{:02}:{:02}", h, m, s)
    }
}

pub fn fmt_size(b: u64) -> String {
    let f = b as f64;
    if f >= 1024.0 * 1024.0 * 1024.0 {
        format!("{:.2} ГБ", f / 1024.0 / 1024.0 / 1024.0)
    } else if f >= 1024.0 * 1024.0 {
        format!("{:.1} МБ", f / 1024.0 / 1024.0)
    } else if f >= 1024.0 {
        format!("{:.1} КБ", f / 1024.0)
    } else {
        format!("{b} Б")
    }
}

fn read_jsonl(path: &std::path::Path, keep_last: usize) -> Vec<Line> {
    let Ok(s) = std::fs::read_to_string(path) else { return Vec::new() };
    let mut v: Vec<Line> = s.lines().filter_map(|l| serde_json::from_str(l).ok()).collect();
    if v.len() > keep_last {
        v.drain(..v.len() - keep_last);
    }
    v
}

fn append_jsonl(path: &std::path::Path, line: &Line) {
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
        if let Ok(s) = serde_json::to_string(line) {
            let _ = writeln!(f, "{s}");
        }
    }
}

impl App {
    pub fn new(ctx: &egui::Context, profile: Option<String>) -> App {
        let ctx = ctx.clone();
        let paths = Paths::new(profile.as_deref());
        let (mut cfg, is_new, cfg_warn) = Config::load(&paths);
        let assets = Paths::find_assets(&cfg);
        let icons = Icons::load(&ctx);
        let emo_packs = crate::emoticons::find_packs(assets.as_deref());

        let mut app = App {
            ctx: ctx.clone(),
            cfg_dirty: false,
            paths: paths.clone(),
            assets,
            profile,
            net: None,
            rx: None,
            shared: Arc::new(Mutex::new(Shared::default())),
            icons,
            emo: None,
            emo_packs,
            page: Page::Users,
            peers: BTreeMap::new(),
            selected: None,
            sort: (Col::Nick, true),
            global: Vec::new(),
            global_ids: HashSet::new(),
            global_input: ChatInput::default(),
            global_unread: false,
            global_search: ChatSearch::default(),
            private_sel: None,
            dm_filter: String::new(),
            dm_filter_focus: false,
            users_filter: String::new(),
            users_filter_focus: false,
            alert: false,
            alert_since: Instant::now(),
            alert_flashing: false,
            chats: HashMap::new(),
            groups: BTreeMap::new(),
            group_sel: None,
            viewers: Vec::new(),
            cmds: HashMap::new(),
            host_sessions: Vec::new(),
            flash: None,
            infos: HashMap::new(),
            pending: HashMap::new(),
            transfers: Vec::new(),
            received: Vec::new(),
            events: Vec::new(),
            events_unread: false,
            tips: VecDeque::new(),
            dlg: Dialogs::default(),
            started: Instant::now(),
            state_since: Instant::now(),
            auto_away_prev: None,
            last_presence: Instant::now() - Duration::from_secs(60),
            last_ping: Instant::now(),
            last_resend: Instant::now(),
            last_tick: Instant::now() - Duration::from_secs(5),
            refresh_deadline: None,
            focused: true,
            visible: true,
            minimized: false,
            quit: false,
            start_hidden: false,
            os_name: crate::platform::os_name(),
            computer: crate::platform::computer_name(),
            os_user: crate::platform::os_user(),
            tray: None,
            main_pos: None,
            updater: crate::update::Updater::default(),
            next_event_n: 1,
            cfg: Config::default(),
        };

        // Hard-bind the interface: chosen once, stored, never silently replaced.
        if cfg.conn.bind_ip.trim().is_empty() {
            if let Some(ip) = crate::net::detect_local_ip() {
                cfg.conn.bind_ip = ip.to_string();
                app.log(Level::Info, "Сеть", format!("Интерфейс выбран автоматически и закреплён в настройках: {ip}"));
            }
        }
        app.cfg = cfg;
        let _ = app.cfg.save(&app.paths);
        if let Some(w) = cfg_warn {
            app.log(Level::Warn, "Настройки", w);
        }
        if is_new {
            app.log(Level::Info, "Настройки", format!("Новый профиль: {}", app.paths.data.display()));
        }
        app.log(Level::Info, "Nassi RS", format!("Nassi RS {} запущен", VERSION));
        if let Some(r) = crate::RENDER_INFO.get() {
            app.log(Level::Info, "Отрисовка", r.clone());
        }
        match &app.assets {
            Some(a) => app.log(Level::Info, "Ресурсы", format!("Смайлы и звуки: {}", a.display())),
            None => app.log(Level::Warn, "Ресурсы", "Папка со смайлами/звуками не найдена (Настройка → Ресурсы)"),
        }
        app.load_emoticons();

        // history
        app.received = crate::received::load(&app.paths);
        app.global = read_jsonl(&app.paths.global, app.cfg.chat.history_lines);
        app.global_ids = app.global.iter().map(|l| l.id).collect();
        if let Ok(rd) = std::fs::read_dir(&app.paths.chats) {
            for e in rd.flatten() {
                let p = e.path();
                let Some(uid) = p.file_stem().and_then(|s| s.to_str()).and_then(|s| u64::from_str_radix(s, 16).ok()) else { continue };
                let lines = read_jsonl(&p, usize::MAX);
                let ids = lines.iter().map(|l| l.id).collect();
                app.chats.insert(uid, PrivateChat::new(uid, lines, ids));
            }
        }

        if let Ok(rd) = std::fs::read_dir(&app.paths.groups) {
            for e in rd.flatten() {
                let p = e.path();
                if p.extension().and_then(|x| x.to_str()) != Some("json") {
                    continue;
                }
                let Some(info) = std::fs::read_to_string(&p).ok().and_then(|s| serde_json::from_str::<GroupInfo>(&s).ok()) else { continue };
                let lines = read_jsonl(&p.with_extension("jsonl"), usize::MAX);
                let ids = lines.iter().map(|l| l.id).collect();
                app.groups.insert(info.id, Conv { info, lines, ids, unread: 0, input: ChatInput::default(), dirty: false, meta_dirty: false, search: ChatSearch::default() });
            }
        }

        app.update_shared();
        app.start_net();
        app.tray = crate::tray::Tray::new(&ctx);
        app
    }

    pub fn start_net(&mut self) {
        match Net::start(&self.cfg.conn, self.cfg.uid, self.ctx.clone(), self.shared.clone()) {
            Ok((net, rx, warns)) => {
                for w in warns {
                    self.log(Level::Warn, "Сеть", w);
                }
                self.log(
                    Level::Info,
                    "Сеть",
                    format!(
                        "UDP {}:{} (broadcast 255.255.255.255:{}), TCP порт {}",
                        net.bound_ip, net.port, net.port, net.tcp_port
                    ),
                );
                self.net = Some(net);
                self.rx = Some(rx);
                if self.cfg.state != ST_OFFLINE {
                    self.send_hello();
                }
            }
            Err(e) => self.log(Level::Error, "Сеть", format!("Сеть не запущена: {e}")),
        }
    }

    pub fn load_emoticons(&mut self) {
        self.emo = None;
        let mut candidates: Vec<PathBuf> = Vec::new();
        if !self.cfg.emoticon_pack.is_empty() {
            let p = PathBuf::from(&self.cfg.emoticon_pack);
            candidates.push(if p.is_absolute() { p } else { self.assets.clone().unwrap_or_default().join(p) });
        }
        // default: Kolobok (animated) like the original, else the first one found
        candidates.extend(self.emo_packs.iter().filter(|p| p.to_string_lossy().to_lowercase().contains("kolobok")).cloned());
        candidates.extend(self.emo_packs.iter().cloned());
        for c in candidates {
            match Pack::load(&self.ctx, &c) {
                Ok(p) => {
                    self.log(Level::Info, "Смайлы", format!("Пакет «{}»: {} смайлов", p.name, p.items.len()));
                    self.emo = Some(p);
                    return;
                }
                Err(e) => self.log(Level::Warn, "Смайлы", format!("{}: {e}", c.display())),
            }
        }
    }

    // ------------------------------------------------------------ helpers

    pub fn log(&mut self, level: Level, source: impl Into<String>, text: impl Into<String>) {
        let rec = EventRec { n: self.next_event_n, time: chrono::Local::now(), level, source: source.into(), text: text.into() };
        self.next_event_n += 1;
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&self.paths.events) {
            let _ = writeln!(f, "{} [{:?}] {}: {}", rec.time.format("%Y-%m-%d %H:%M:%S"), rec.level, rec.source, rec.text);
        }
        if level != Level::Info && self.page != Page::Events {
            self.events_unread = true;
        }
        self.events.push(rec);
        if self.events.len() > 5000 {
            self.events.drain(..1000);
        }
    }

    pub fn play(&self, event: &str) {
        if !self.cfg.sounds.enabled || self.cfg.cur_state().no_sounds {
            return;
        }
        let Some((on, file)) = self.cfg.sounds.events.get(event) else { return };
        if !*on || file.is_empty() {
            return;
        }
        let p = PathBuf::from(file);
        let p = if p.is_absolute() { p } else { self.assets.clone().unwrap_or_default().join("Sounds").join(p) };
        if p.exists() {
            crate::platform::play_wav(&p);
        }
    }

    pub fn tip(&mut self, title: impl Into<String>, text: impl Into<String>, chat_uid: Option<u64>) {
        if !self.cfg.general.tip_enabled || self.cfg.cur_state().no_popups || (self.focused && self.visible && !self.minimized) {
            return;
        }
        let until = Instant::now() + Duration::from_secs(self.cfg.general.tip_secs.max(1) as u64);
        self.tips.push_back(Tip { title: title.into(), text: text.into(), until, chat_uid, group: None });
        while self.tips.len() > 4 {
            self.tips.pop_front();
        }
    }

    pub fn tip_group(&mut self, title: impl Into<String>, text: impl Into<String>, group: u64) {
        self.tip(title, text, None);
        if let Some(t) = self.tips.back_mut() {
            t.group = Some(group);
        }
    }

    /// Am I away from the window: it is not active, hidden in the tray, minimized,
    /// or nobody has touched the keyboard/mouse for a while.
    pub fn user_away(&self) -> bool {
        !self.focused || !self.visible || self.minimized || crate::platform::idle_secs() >= self.cfg.general.away_secs.max(5)
    }

    /// Something arrived (a message in any chat, a signal, a file): if I am away, the
    /// application starts blinking (taskbar button + tray icon) until I come back.
    pub fn attention(&mut self) {
        if !self.cfg.general.flash_on_message || !self.user_away() {
            return;
        }
        if !self.alert {
            self.alert = true;
            self.alert_since = Instant::now();
        }
        self.start_flash();
    }

    fn main_hwnd(&self) -> Option<isize> {
        let h = self.tray.as_ref().map(|t| t.hwnd.load(std::sync::atomic::Ordering::Relaxed)).unwrap_or(0);
        if h != 0 {
            return Some(h);
        }
        let h = crate::platform::find_main_window()?;
        if let Some(t) = &self.tray {
            t.hwnd.store(h, std::sync::atomic::Ordering::Relaxed);
        }
        Some(h)
    }

    fn start_flash(&mut self) {
        let hwnd = self.main_hwnd().filter(|_| cfg!(windows));
        if !self.visible {
            // in the tray: bring the window back to the taskbar, minimized and not
            // activated, so the user sees the blinking button but is not interrupted
            // (no window pops up on the screen, the focus stays where it is)
            if let Some(h) = hwnd {
                crate::platform::show_in_taskbar(h);
                self.visible = true;
                self.minimized = true;
            }
            // elsewhere the tray mode is a minimized window, it is in the taskbar already
        }
        match hwnd {
            Some(h) => crate::platform::flash_window(h, true),
            None => self.ctx.send_viewport_cmd(egui::ViewportCommand::RequestUserAttention(egui::UserAttentionType::Critical)),
        }
        self.alert_flashing = true;
    }

    /// Called every frame: stop blinking once I am back at the active window,
    /// keep the tray icon blinking meanwhile.
    pub fn update_alert(&mut self) {
        if !self.alert {
            return;
        }
        let back = self.focused && self.visible && !self.minimized && crate::platform::idle_secs() < 2 && self.alert_since.elapsed() > Duration::from_millis(700);
        if back || !self.cfg.general.flash_on_message {
            self.stop_alert();
            return;
        }
        if self.visible && !self.alert_flashing {
            // came out of the tray without becoming active: blink the taskbar button too
            self.start_flash();
        }
        let on = (self.alert_since.elapsed().as_millis() / 500) % 2 == 0;
        if let Some(t) = &self.tray {
            t.set_alert(on);
        }
        self.ctx.request_repaint_after(Duration::from_millis(250));
    }

    pub fn stop_alert(&mut self) {
        self.alert = false;
        if self.alert_flashing {
            if let Some(h) = self.main_hwnd() {
                crate::platform::flash_window(h, false);
            }
            self.ctx.send_viewport_cmd(egui::ViewportCommand::RequestUserAttention(egui::UserAttentionType::Reset));
            self.alert_flashing = false;
        }
        if let Some(t) = &self.tray {
            t.set_alert(false);
        }
    }

    pub fn show_main(&mut self) {
        if !self.visible {
            match self.tray.as_ref().map(|t| t.hwnd.load(std::sync::atomic::Ordering::Relaxed)).filter(|h| *h != 0) {
                Some(h) => crate::platform::set_stealth(h, false),
                None => self.ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true)),
            }
        }
        self.visible = true;
        self.minimized = false;
        self.ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
        self.ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
    }

    /// The program was started again while this copy runs (see platform::single_instance):
    /// show this window instead. One thread for the whole process; a second App (the OpenGL
    /// self-test failed and the window was recreated) just takes over as the target.
    pub fn listen_for_second_start(&self, ev: isize) {
        use std::sync::atomic::{AtomicBool, AtomicIsize};
        use std::sync::{Mutex, Once};
        type Target = (egui::Context, Arc<AtomicIsize>, Arc<AtomicBool>);
        static TARGET: Mutex<Option<Target>> = Mutex::new(None);
        static START: Once = Once::new();
        if ev == 0 {
            return;
        }
        let (hwnd, show) = match &self.tray {
            Some(t) => (t.hwnd.clone(), t.show_requested.clone()),
            None => (Arc::new(AtomicIsize::new(0)), Arc::new(AtomicBool::new(false))),
        };
        if let Ok(mut t) = TARGET.lock() {
            *t = Some((self.ctx.clone(), hwnd, show));
        }
        START.call_once(|| {
            let _ = std::thread::Builder::new().name("second-start".into()).spawn(move || {
                while crate::platform::wait_second_start(ev) {
                    let t = TARGET.lock().ok().and_then(|t| t.clone());
                    if let Some((ctx, hwnd, show)) = t {
                        crate::tray::restore(&ctx, &hwnd, &show);
                    }
                }
            });
        });
    }

    /// Go to the tray. The window stays alive (transparent) so networking keeps working.
    pub fn hide_main(&mut self) {
        let Some(tray) = &self.tray else {
            self.ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
            return;
        };
        let mut h = tray.hwnd.load(std::sync::atomic::Ordering::Relaxed);
        if h == 0 {
            h = crate::platform::find_main_window().unwrap_or(0);
            tray.hwnd.store(h, std::sync::atomic::Ordering::Relaxed);
        }
        if h != 0 {
            crate::platform::set_stealth(h, true);
        } else {
            self.ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
        }
        self.visible = false;
    }

    pub fn peer_name(&self, uid: u64) -> String {
        if uid == self.cfg.uid {
            return self.cfg.nick.clone();
        }
        if let Some(p) = self.peers.get(&uid) {
            return p.p.nick.clone();
        }
        self.cfg.known.get(&uid).map(|k| k.0.clone()).unwrap_or_else(|| format!("{uid:x}"))
    }

    pub fn find_peer(&self, name: &str) -> Option<u64> {
        let n = name.trim().to_lowercase();
        self.peers
            .values()
            .find(|p| p.p.nick.to_lowercase() == n)
            .or_else(|| self.peers.values().find(|p| p.p.computer.to_lowercase() == n))
            .or_else(|| self.peers.values().find(|p| p.p.nick.to_lowercase().starts_with(&n)))
            .map(|p| p.uid)
    }

    pub fn update_shared(&mut self) {
        let mut sh = self.shared.lock().unwrap();
        sh.allow = self.cfg.effective_allow();
        sh.details = self.cfg.details.clone();
        sh.offline = self.cfg.state == ST_OFFLINE;
    }

    /// Short confirmation shown in the status bar ("Сигнал отправлен" etc).
    pub fn flash(&mut self, s: impl Into<String>) {
        self.flash = Some((s.into(), Instant::now()));
    }

    pub fn save_cfg(&mut self) {
        if let Err(e) = self.cfg.save(&self.paths) {
            self.log(Level::Error, "Настройки", format!("Не удалось сохранить настройки: {e}"));
        }
        self.cfg_dirty = false;
    }

    // ------------------------------------------------------------ presence

    pub fn presence(&self) -> Presence {
        let allow = self.cfg.effective_allow();
        let st = self.cfg.cur_state();
        Presence {
            nick: self.cfg.nick.clone(),
            computer: self.computer.clone(),
            os_user: self.os_user.clone(),
            state: self.cfg.state,
            state_title: st.title.clone(),
            state_text: st.text.clone(),
            color: self.cfg.color,
            idle_secs: crate::platform::idle_secs(),
            in_state_secs: self.state_since.elapsed().as_secs() as u32,
            fg_title: allow.fg_title.then(crate::platform::foreground_title),
            win_state: if !self.visible { 3 } else if self.minimized { 1 } else { 0 },
            uptime_secs: self.started.elapsed().as_secs() as u32,
            os_uptime_secs: crate::platform::os_uptime_secs(),
            os_name: self.os_name.clone(),
            version: VERSION.into(),
            tcp_port: self.net.as_ref().map(|n| n.tcp_port).unwrap_or(0),
            allow,
            details_rev: self.cfg.details.modified,
            time: now_unix(),
        }
    }

    pub fn send_hello(&mut self) {
        let p = self.presence();
        if let Some(n) = &self.net {
            n.broadcast(Body::Hello(p));
        }
        self.last_presence = Instant::now();
    }

    pub fn send_presence(&mut self) {
        if self.cfg.state == ST_OFFLINE {
            return;
        }
        let p = self.presence();
        if let Some(n) = &self.net {
            n.broadcast(Body::Presence(p));
        }
        self.last_presence = Instant::now();
    }

    pub fn refresh_users(&mut self) {
        if self.cfg.state == ST_OFFLINE {
            return;
        }
        for p in self.peers.values_mut() {
            p.refresh_pending = true;
        }
        self.refresh_deadline = Some(Instant::now() + Duration::from_millis(self.cfg.general.refresh_timeout_ms.max(500) as u64));
        if let Some(n) = &self.net {
            n.broadcast(Body::Query);
        }
        self.send_presence();
    }

    pub fn set_state(&mut self, st: u8, auto: bool) {
        if st as usize >= self.cfg.states.len() || st == self.cfg.state {
            return;
        }
        let was_offline = self.cfg.state == ST_OFFLINE;
        if !auto {
            self.auto_away_prev = None;
        }
        if st == ST_OFFLINE {
            if let Some(n) = &self.net {
                n.broadcast(Body::Bye);
            }
            self.peers.clear();
        }
        self.cfg.state = st;
        self.state_since = Instant::now();
        self.cfg_dirty = true;
        self.update_shared();
        let title = self.cfg.cur_state().title.clone();
        self.log(Level::Info, "Состояние", format!("Состояние: {title}{}", if auto { " (автоматически)" } else { "" }));
        self.flash(format!("Состояние: {title}"));
        self.play("state");
        if was_offline {
            self.send_hello();
        } else {
            self.send_presence();
        }
    }

    pub fn set_nick(&mut self, nick: &str) {
        let nick = nick.trim();
        if nick.is_empty() || nick == self.cfg.nick {
            return;
        }
        let old = std::mem::replace(&mut self.cfg.nick, nick.to_string());
        if !self.cfg.nicks.iter().any(|n| n == nick) {
            self.cfg.nicks.push(nick.to_string());
            if self.cfg.nicks.len() > 10 {
                self.cfg.nicks.remove(0);
            }
        }
        self.cfg_dirty = true;
        self.log(Level::Info, "Ник", format!("{old} теперь {nick}"));
        self.send_presence();
    }

    // ------------------------------------------------------------ timers

    pub fn tick(&mut self) {
        let now = Instant::now();
        if now.duration_since(self.last_tick) < Duration::from_millis(1000) {
            return;
        }
        self.last_tick = now;
        self.update_tick();

        // auto away / return
        let idle = crate::platform::idle_secs();
        if self.cfg.away.on_idle
            && self.auto_away_prev.is_none()
            && idle >= self.cfg.away.idle_min.max(1) * 60
            && self.cfg.state != ST_AWAY
            && self.cfg.state != ST_OFFLINE
            && !self.cfg.cur_state().no_auto_away
        {
            let prev = self.cfg.state;
            self.set_state(ST_AWAY, true);
            self.auto_away_prev = Some(prev);
        } else if let Some(prev) = self.auto_away_prev {
            if idle < 3 {
                self.auto_away_prev = None;
                if self.cfg.state == ST_AWAY {
                    self.set_state(prev, true);
                }
            }
        }

        if self.cfg.state != ST_OFFLINE {
            if now.duration_since(self.last_presence) > Duration::from_secs(5) {
                self.send_presence();
            }
            if now.duration_since(self.last_ping) > Duration::from_secs(5) {
                self.last_ping = now;
                if let Some(n) = &self.net {
                    let t = self.started.elapsed().as_millis() as u64;
                    for p in self.peers.values() {
                        n.send(p.uid, p.ip, Body::Ping(t));
                    }
                }
            }
            if now.duration_since(self.last_resend) > Duration::from_secs(20) {
                self.last_resend = now;
                let uids: Vec<u64> = self.peers.keys().copied().collect();
                for u in uids {
                    self.resend_pending(u);
                    self.resend_group_pending(u);
                }
            }
        }

        // expire peers
        let timeout = Duration::from_secs(25);
        let gone: Vec<u64> = self
            .peers
            .values()
            .filter(|p| {
                now.duration_since(p.last_seen) > timeout
                    || (p.refresh_pending && self.refresh_deadline.is_some_and(|d| now > d))
            })
            .map(|p| p.uid)
            .collect();
        let refresh_done = self.refresh_deadline.is_some_and(|d| now > d);
        if refresh_done {
            self.refresh_deadline = None;
        }
        for u in gone {
            self.peer_left(u, "не отвечает");
        }
        if refresh_done {
            let n = self.peers.len() + 1;
            self.flash(format!("Список обновлён: в сети {n}"));
        }

        self.tips.retain(|t| t.until > now);
        self.host_sessions.retain(|s| !s.stop.load(std::sync::atomic::Ordering::Relaxed));
        let dirty: Vec<u64> = self.groups.values().filter(|c| c.dirty || c.meta_dirty).map(|c| c.info.id).collect();
        for g in dirty {
            self.save_group(g);
        }

        // persist chats whose delivery flags changed
        let dirty: Vec<u64> = self.chats.values().filter(|c| c.dirty).map(|c| c.uid).collect();
        for u in dirty {
            self.rewrite_chat(u);
        }
        if self.cfg_dirty {
            self.save_cfg();
        }
        // auto refresh info windows
        let due: Vec<u64> = self
            .infos
            .values()
            .filter(|w| w.auto_secs > 0 && !w.busy && w.last_auto.map_or(true, |t| t.elapsed().as_secs() >= w.auto_secs as u64))
            .map(|w| w.uid)
            .collect();
        for u in due {
            if let Some(w) = self.infos.get_mut(&u) {
                w.last_auto = Some(now);
            }
            self.info_refresh(u);
        }
    }

    fn peer_left(&mut self, uid: u64, why: &str) {
        if let Some(p) = self.peers.remove(&uid) {
            self.log(Level::Info, &p.p.computer, format!("{} ушёл ({why})", p.p.nick));
            self.play("user_leave");
            if self.selected == Some(uid) {
                self.selected = None;
            }
        }
    }

    // ------------------------------------------------------------ network events

    pub fn poll_net(&mut self) {
        let mut evs = Vec::new();
        if let Some(rx) = &self.rx {
            while let Ok(e) = rx.try_recv() {
                evs.push(e);
                if evs.len() > 2000 {
                    break;
                }
            }
        }
        for e in evs {
            match e {
                NetEvent::Packet { env, src } => {
                    if self.cfg.state != ST_OFFLINE {
                        self.on_packet(env, src);
                    }
                }
                NetEvent::Reply { req_id, result } => self.on_reply(req_id, result),
                NetEvent::Served { from, ip, what } => {
                    let who = self.peers.get(&from).map(|p| format!("{} ({})", p.p.nick, p.p.computer)).unwrap_or(ip.to_string());
                    self.log(Level::Warn, "Удалённый доступ", format!("{who} запросил: {what}"));
                }
                NetEvent::Transfer { id, done, total, finished, error } => self.on_transfer(id, done, total, finished, error),
                NetEvent::RemoteStarted { sid, from, ip, view_only, stop } => {
                    let who = self.peers.get(&from).map(|p| format!("{} ({})", p.p.nick, p.p.computer)).unwrap_or(ip.to_string());
                    self.log(
                        Level::Warn,
                        "Удалённый доступ",
                        format!("{who} подключился (пароль администратора): {}", if view_only { "просмотр" } else { "управление" }),
                    );
                    self.host_sessions.push(HostSession { sid, who, view_only, stop, since: Instant::now() });
                }
                NetEvent::RemoteEnded { sid } => {
                    if let Some(s) = self.host_sessions.iter().find(|s| s.sid == sid) {
                        let msg = format!("{} отключился", s.who);
                        self.log(Level::Info, "Удалённый доступ", msg);
                    }
                    self.host_sessions.retain(|s| s.sid != sid);
                }
                NetEvent::RemoteLog(m) => self.log(Level::Warn, "Удалённый доступ", m),
            }
        }
    }

    fn touch_peer(&mut self, uid: u64, p: Presence, src: SocketAddr) -> bool {
        let is_new = !self.peers.contains_key(&uid);
        let ip = src.ip();
        let entry = self.peers.entry(uid).or_insert_with(|| Peer {
            uid,
            p: p.clone(),
            ip,
            last_seen: Instant::now(),
            ping_ms: None,
            details: None,
            details_rev: 0,
            avatar: None,
            typing_until: None,
            refresh_pending: false,
        });
        let old_nick = std::mem::replace(&mut entry.p, p);
        entry.ip = ip;
        entry.last_seen = Instant::now();
        entry.refresh_pending = false;
        let nick = entry.p.nick.clone();
        let comp = entry.p.computer.clone();
        let rev = entry.p.details_rev;
        let need_details = rev != 0 && rev != entry.details_rev;
        if self.cfg.known.get(&uid).map(|k| (&k.0, &k.1)) != Some((&nick, &comp)) {
            self.cfg.known.insert(uid, (nick.clone(), comp.clone()));
            self.cfg_dirty = true;
        }
        if !is_new && old_nick.nick != nick {
            self.log(Level::Info, &comp, format!("{} теперь {}", old_nick.nick, nick));
        }
        if is_new {
            self.log(Level::Info, &comp, format!("{nick} в сети ({})", ip));
            self.play("user_join");
            self.resend_pending(uid);
            self.group_peer_appeared(uid);
        }
        if need_details {
            self.request_details(uid, false);
        }
        is_new
    }

    fn on_packet(&mut self, env: Envelope, src: SocketAddr) {
        let from = env.from;
        // ignore packets from unknown peers except hello/presence
        match env.body {
            Body::Hello(p) => {
                self.touch_peer(from, p, src);
                let me = self.presence();
                if let Some(n) = &self.net {
                    n.send(from, src.ip(), Body::Presence(me));
                }
            }
            Body::Presence(p) => {
                self.touch_peer(from, p, src);
            }
            Body::Bye => self.peer_left(from, "вышел"),
            Body::Query => {
                // small random delay is unnecessary on a LAN; answer directly
                let me = self.presence();
                if let Some(n) = &self.net {
                    n.send(from, src.ip(), Body::Presence(me));
                }
            }
            Body::Ping(t) => {
                if let Some(n) = &self.net {
                    n.send(from, src.ip(), Body::Pong(t));
                }
            }
            Body::Pong(t) => {
                let now = self.started.elapsed().as_millis() as u64;
                if let Some(p) = self.peers.get_mut(&from) {
                    p.ping_ms = Some(now.saturating_sub(t) as u32);
                }
            }
            Body::Global { id, text, me } => {
                if !self.global_ids.insert(id) {
                    return;
                }
                let (nick, color, comp) = match self.peers.get(&from) {
                    Some(p) => (p.p.nick.clone(), p.p.color, p.p.computer.clone()),
                    None => (self.peer_name(from), [90, 90, 90], String::new()),
                };
                let line = Line { id, time: now_unix(), from, nick: nick.clone(), color, text: text.clone(), me, delivered: true, pending: Vec::new() };
                self.push_global(line);
                if self.page != Page::Chat || self.user_away() {
                    self.global_unread = true;
                }
                self.play(if me { "me" } else { "global" });
                self.tip(format!("Общий чат — {nick}"), text, None);
                let _ = comp;
                self.attention();
                if self.cfg.general.popup_on_global && !self.cfg.cur_state().no_popups {
                    self.show_main();
                }
            }
            Body::Private { id, text, sent } => {
                if let Some(n) = &self.net {
                    n.send(from, src.ip(), Body::PrivateAck(id));
                }
                let (nick, color) = match self.peers.get(&from) {
                    Some(p) => (p.p.nick.clone(), p.p.color),
                    None => (self.peer_name(from), [90, 90, 90]),
                };
                let reading = self.page == Page::Private && self.private_sel == Some(from) && !self.user_away();
                let chat = self.chat_mut(from);
                if !chat.ids.insert(id) {
                    return; // duplicate (resend)
                }
                let line = Line { id, time: sent, from, nick: nick.clone(), color, text: text.clone(), me: false, delivered: true, pending: Vec::new() };
                chat.lines.push(line.clone());
                if !reading {
                    chat.unread += 1;
                }
                let path = self.chat_path(from);
                append_jsonl(&path, &line);
                self.play("private");
                self.tip(format!("Личное сообщение — {nick}"), text, Some(from));
                self.attention();
                if self.cfg.general.popup_on_private && !self.cfg.cur_state().no_popups && !self.focused {
                    self.show_main();
                    self.open_chat(from);
                }
                self.auto_answer(from);
            }
            Body::PrivateAck(id) => {
                if let Some(c) = self.chats.get_mut(&from) {
                    if let Some(l) = c.lines.iter_mut().find(|l| l.id == id && l.from != from) {
                        if !l.delivered {
                            l.delivered = true;
                            c.dirty = true;
                        }
                    }
                }
            }
            Body::Typing => {
                if let Some(p) = self.peers.get_mut(&from) {
                    p.typing_until = Some(Instant::now() + Duration::from_secs(4));
                }
            }
            Body::Signal { broadcast } => {
                let nick = self.peer_name(from);
                self.log(Level::Info, "Сигнал", format!("Сигнал от {nick}{}", if broadcast { " (всем)" } else { "" }));
                self.play("signal");
                self.tip("Сигнал", format!("{nick} подаёт сигнал"), Some(from));
                self.attention();
            }
            Body::FileOffer { id, name, size } => {
                let nick = self.peer_name(from);
                if self.transfers.iter().any(|t| t.id == id && t.incoming) {
                    return;
                }
                self.log(Level::Info, "Файлы", format!("{nick} предлагает файл «{name}» ({})", fmt_size(size)));
                self.transfers.push(Transfer {
                    id,
                    peer: from,
                    nick: nick.clone(),
                    name: name.clone(),
                    size,
                    incoming: true,
                    state: TState::Offered,
                    done: 0,
                    path: None,
                    cancel: Arc::new(AtomicBool::new(false)),
                    started: Instant::now(),
                });
                self.dlg.transfers = true;
                self.play("file");
                self.tip("Входящий файл", format!("{nick}: {name} ({})", fmt_size(size)), None);
                self.attention();
            }
            Body::FileAnswer { id, accept } => {
                if let Some(t) = self.transfers.iter_mut().find(|t| t.id == id && !t.incoming) {
                    t.state = if accept { TState::Running } else { TState::Declined };
                    let msg = format!("{} {} файл «{}»", t.nick, if accept { "принимает" } else { "отклонил" }, t.name);
                    if !accept {
                        self.shared.lock().unwrap().files.remove(&id);
                    }
                    self.log(Level::Info, "Файлы", msg);
                }
            }
            Body::GroupInfo(info) => self.on_group_info(from, info),
            Body::GroupQuery(conv) => {
                if let (Some(c), Some(p)) = (self.groups.get(&conv), self.peers.get(&from)) {
                    if c.is_member(from) && c.is_member(self.cfg.uid) {
                        if let Some(n) = &self.net {
                            n.send(from, p.ip, Body::GroupInfo(c.info.clone()));
                        }
                    }
                }
            }
            Body::GroupAck { conv, id } => {
                if let Some(c) = self.groups.get_mut(&conv) {
                    if let Some(l) = c.lines.iter_mut().find(|l| l.id == id) {
                        let before = l.pending.len();
                        l.pending.retain(|u| *u != from);
                        if l.pending.len() != before {
                            c.dirty = true;
                        }
                    }
                }
            }
            Body::GroupMsg { conv, rev, id, text, sent, me } => self.on_group_msg(from, src, conv, rev, id, text, sent, me),
        }
    }

    // ------------------------------------------------------------ group chats

    fn group_path(&self, id: u64) -> PathBuf {
        self.paths.groups.join(format!("{id:016x}.json"))
    }

    pub fn save_group(&mut self, id: u64) {
        let path = self.group_path(id);
        let Some(c) = self.groups.get_mut(&id) else { return };
        if c.meta_dirty {
            if let Ok(s) = serde_json::to_string_pretty(&c.info) {
                let _ = std::fs::write(&path, s);
            }
            c.meta_dirty = false;
        }
        if c.dirty {
            let mut s = String::new();
            for l in &c.lines {
                if let Ok(j) = serde_json::to_string(l) {
                    s.push_str(&j);
                    s.push('\n');
                }
            }
            let jl = path.with_extension("jsonl");
            let tmp = path.with_extension("tmp");
            if std::fs::write(&tmp, s).is_ok() {
                let _ = std::fs::rename(&tmp, &jl);
            }
            c.dirty = false;
        }
    }

    fn names_for(&self, members: &[u64]) -> Vec<(u64, String)> {
        members.iter().map(|u| (*u, self.peer_name(*u))).collect()
    }

    /// Send the conversation description to all online members (except me).
    fn push_group_info(&self, id: u64, extra: &[u64]) {
        let (Some(c), Some(n)) = (self.groups.get(&id), &self.net) else { return };
        let mut to: Vec<u64> = c.info.members.clone();
        to.extend_from_slice(extra);
        to.sort();
        to.dedup();
        for u in to {
            if u == self.cfg.uid {
                continue;
            }
            if let Some(p) = self.peers.get(&u) {
                n.send(u, p.ip, Body::GroupInfo(c.info.clone()));
            }
        }
    }

    fn group_system(&mut self, id: u64, text: String) {
        if let Some(c) = self.groups.get_mut(&id) {
            let line = Line { id: new_id(), time: now_unix(), from: 0, text, delivered: true, ..Default::default() };
            c.ids.insert(line.id);
            c.lines.push(line);
            c.dirty = true;
        }
    }

    pub fn create_group(&mut self, title: String, mut members: Vec<u64>) -> u64 {
        members.retain(|u| *u != self.cfg.uid);
        members.insert(0, self.cfg.uid);
        members.dedup();
        let id = new_id();
        let title = if title.trim().is_empty() {
            let names: Vec<String> = members.iter().map(|u| self.peer_name(*u)).collect();
            names.join(", ")
        } else {
            title.trim().to_string()
        };
        let info = GroupInfo { id, title: title.clone(), owner: self.cfg.uid, names: self.names_for(&members), members, rev: now_ms() };
        self.groups.insert(id, Conv { info, lines: Vec::new(), ids: HashSet::new(), unread: 0, input: ChatInput::default(), dirty: true, meta_dirty: true, search: ChatSearch::default() });
        self.group_system(id, format!("{} создал(а) беседу «{title}»", self.cfg.nick));
        self.push_group_info(id, &[]);
        self.group_sel = Some(id);
        self.page = Page::Groups;
        if let Some(c) = self.groups.get_mut(&id) {
            c.input.focus = true;
        }
        self.flash(format!("Беседа «{title}» создана"));
        id
    }

    fn update_group(&mut self, id: u64, f: impl FnOnce(&mut GroupInfo), notify_extra: &[u64]) {
        let members = {
            let Some(c) = self.groups.get_mut(&id) else { return };
            f(&mut c.info);
            c.info.rev = now_ms().max(c.info.rev + 1);
            c.meta_dirty = true;
            c.info.members.clone()
        };
        let names = self.names_for(&members);
        if let Some(c) = self.groups.get_mut(&id) {
            c.info.names = names;
        }
        self.push_group_info(id, notify_extra);
    }

    pub fn group_add(&mut self, id: u64, uid: u64) {
        if self.groups.get(&id).map_or(true, |c| c.is_member(uid) || !c.is_member(self.cfg.uid)) {
            return;
        }
        let nick = self.peer_name(uid);
        self.update_group(id, |i| i.members.push(uid), &[]);
        self.group_system(id, format!("{} добавил(а) {nick}", self.cfg.nick));
        self.resend_group_pending(uid);
        self.flash(format!("{nick} добавлен(а) в беседу"));
    }

    pub fn group_remove(&mut self, id: u64, uid: u64) {
        if !self.groups.get(&id).is_some_and(|c| c.is_member(uid)) {
            return;
        }
        let nick = self.peer_name(uid);
        self.update_group(id, |i| i.members.retain(|u| *u != uid), &[uid]);
        if uid == self.cfg.uid {
            self.group_system(id, "Вы вышли из беседы".into());
            self.flash("Вы вышли из беседы");
        } else {
            self.group_system(id, format!("{} исключил(а) {nick}", self.cfg.nick));
            self.flash(format!("{nick} исключён(а) из беседы"));
        }
    }

    pub fn group_rename(&mut self, id: u64, title: String) {
        let title = title.trim().to_string();
        if title.is_empty() {
            return;
        }
        self.update_group(id, |i| i.title = title.clone(), &[]);
        self.group_system(id, format!("{} переименовал(а) беседу в «{title}»", self.cfg.nick));
    }

    pub fn delete_group(&mut self, id: u64) {
        if self.groups.get(&id).is_some_and(|c| c.is_member(self.cfg.uid)) {
            self.group_remove(id, self.cfg.uid);
        }
        self.groups.remove(&id);
        let p = self.group_path(id);
        let _ = std::fs::remove_file(&p);
        let _ = std::fs::remove_file(p.with_extension("jsonl"));
        if self.group_sel == Some(id) {
            self.group_sel = None;
        }
    }

    pub fn send_group(&mut self, id: u64, text: String, me: bool) {
        let my = self.cfg.uid;
        let Some(c) = self.groups.get(&id) else { return };
        if !c.is_member(my) {
            return;
        }
        let rev = c.info.rev;
        let others: Vec<u64> = c.info.members.iter().copied().filter(|u| *u != my).collect();
        let mid = new_id();
        let sent = now_unix();
        let line = Line {
            id: mid,
            time: sent,
            from: my,
            nick: self.cfg.nick.clone(),
            color: self.cfg.color,
            text: text.clone(),
            me,
            delivered: true,
            pending: others.clone(),
        };
        if let (Some(n), true) = (&self.net, self.cfg.state != ST_OFFLINE) {
            for u in &others {
                if let Some(p) = self.peers.get(u) {
                    n.send(*u, p.ip, Body::GroupMsg { conv: id, rev, id: mid, text: text.clone(), sent, me });
                }
            }
        }
        let path = self.group_path(id).with_extension("jsonl");
        append_jsonl(&path, &line);
        let c = self.groups.get_mut(&id).unwrap();
        c.ids.insert(mid);
        c.lines.push(line);
    }

    /// Re-send my undelivered group messages to `uid`.
    pub fn resend_group_pending(&mut self, uid: u64) {
        let (Some(n), Some(p)) = (&self.net, self.peers.get(&uid)) else { return };
        if self.cfg.state == ST_OFFLINE {
            return;
        }
        for c in self.groups.values() {
            if !c.is_member(uid) {
                continue;
            }
            for l in c.lines.iter().filter(|l| l.pending.contains(&uid)) {
                n.send(uid, p.ip, Body::GroupMsg { conv: c.info.id, rev: c.info.rev, id: l.id, text: l.text.clone(), sent: l.time, me: l.me });
            }
        }
    }

    fn group_peer_appeared(&mut self, uid: u64) {
        if let (Some(n), Some(p)) = (&self.net, self.peers.get(&uid)) {
            for c in self.groups.values() {
                if c.is_member(uid) && c.is_member(self.cfg.uid) {
                    n.send(uid, p.ip, Body::GroupInfo(c.info.clone()));
                }
            }
        }
        self.resend_group_pending(uid);
    }

    fn on_group_info(&mut self, from: u64, info: GroupInfo) {
        let my = self.cfg.uid;
        let id = info.id;
        match self.groups.get_mut(&id) {
            Some(c) => {
                // only a current member may change the conversation
                if info.rev <= c.info.rev || !c.info.members.contains(&from) {
                    return;
                }
                let was_member = c.is_member(my);
                let now_member = info.members.contains(&my);
                let title_changed = c.info.title != info.title;
                let added: Vec<u64> = info.members.iter().copied().filter(|u| !c.info.members.contains(u)).collect();
                let removed: Vec<u64> = c.info.members.iter().copied().filter(|u| !info.members.contains(u)).collect();
                c.info = info.clone();
                c.meta_dirty = true;
                let who = info.name_of(from).map(|s| s.to_string()).unwrap_or_else(|| self.peer_name(from));
                if was_member && !now_member {
                    self.group_system(id, format!("{who} исключил(а) вас из беседы"));
                } else if !was_member && now_member {
                    self.group_system(id, format!("{who} снова добавил(а) вас в беседу"));
                }
                for u in added {
                    let n = info.name_of(u).map(|s| s.to_string()).unwrap_or_else(|| self.peer_name(u));
                    self.group_system(id, format!("{who} добавил(а) {n}"));
                }
                for u in removed {
                    if u == my {
                        continue;
                    }
                    let n = info.name_of(u).map(|s| s.to_string()).unwrap_or_else(|| self.peer_name(u));
                    if u == from {
                        self.group_system(id, format!("{n} вышел(ла) из беседы"));
                    } else {
                        self.group_system(id, format!("{who} исключил(а) {n}"));
                    }
                }
                if title_changed {
                    self.group_system(id, format!("Беседа переименована: «{}»", info.title));
                }
            }
            None => {
                if !info.members.contains(&my) || !info.members.contains(&from) {
                    return;
                }
                let who = info.name_of(from).map(|s| s.to_string()).unwrap_or_else(|| self.peer_name(from));
                let title = info.title.clone();
                self.groups.insert(id, Conv { info, lines: Vec::new(), ids: HashSet::new(), unread: 1, input: ChatInput::default(), dirty: true, meta_dirty: true, search: ChatSearch::default() });
                self.group_system(id, format!("{who} пригласил(а) вас в беседу «{title}»"));
                self.log(Level::Info, "Беседы", format!("{who} пригласил(а) вас в беседу «{title}»"));
                self.play("private");
                self.tip("Новая беседа", format!("{who}: «{title}»"), None);
                self.attention();
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn on_group_msg(&mut self, from: u64, src: SocketAddr, conv: u64, rev: u64, id: u64, text: String, sent: i64, me: bool) {
        let my = self.cfg.uid;
        if let Some(n) = &self.net {
            n.send(from, src.ip(), Body::GroupAck { conv, id });
        }
        let known_rev = self.groups.get(&conv).map(|c| c.info.rev);
        if known_rev.map_or(true, |r| rev > r) {
            if let Some(n) = &self.net {
                n.send(from, src.ip(), Body::GroupQuery(conv));
            }
        }
        if !self.groups.contains_key(&conv) {
            // unknown yet: placeholder until GroupInfo arrives
            let info = GroupInfo { id: conv, title: "Беседа".into(), owner: from, members: vec![from, my], names: self.names_for(&[from, my]), rev: 0 };
            self.groups.insert(conv, Conv { info, lines: Vec::new(), ids: HashSet::new(), unread: 0, input: ChatInput::default(), dirty: true, meta_dirty: true, search: ChatSearch::default() });
        }
        let (nick, color) = match self.peers.get(&from) {
            Some(p) => (p.p.nick.clone(), p.p.color),
            None => (self.peer_name(from), [90, 90, 90]),
        };
        let selected = self.page == Page::Groups && self.group_sel == Some(conv) && !self.user_away();
        let c = self.groups.get_mut(&conv).unwrap();
        if !c.is_member(my) && known_rev.is_some_and(|r| rev <= r) {
            return; // I was removed; sender has stale info (they get my GroupInfo via query)
        }
        if !c.is_member(from) && known_rev.is_some_and(|r| rev <= r) {
            return; // not a member of this conversation
        }
        if !c.ids.insert(id) {
            return;
        }
        let line = Line { id, time: sent, from, nick: nick.clone(), color, text: text.clone(), me, delivered: true, pending: Vec::new() };
        c.lines.push(line.clone());
        if !selected {
            c.unread += 1;
        }
        let title = c.info.title.clone();
        let path = self.group_path(conv).with_extension("jsonl");
        append_jsonl(&path, &line);
        self.play("global");
        self.tip_group(format!("{title} — {nick}"), text, conv);
        self.attention();
    }

    pub fn open_group(&mut self, id: u64) {
        self.group_sel = Some(id);
        self.page = Page::Groups;
        if let Some(c) = self.groups.get_mut(&id) {
            c.unread = 0;
            c.input.focus = true;
        }
    }

    // ------------------------------------------------------------ remote desktop (viewer side)

    pub fn remote_connect(&mut self, uid: u64, view_only: bool, password: String) {
        let (Some(n), Some(p)) = (&self.net, self.peers.get(&uid)) else {
            self.flash("Пользователь не в сети");
            return;
        };
        let v = crate::remote::Viewer::connect(n.cipher.clone(), self.cfg.uid, uid, p.p.nick.clone(), p.tcp_addr(), view_only, password, self.ctx.clone());
        let nick = p.p.nick.clone();
        self.log(Level::Info, "Удалённый доступ", format!("Подключение к {nick} ({})", if view_only { "просмотр" } else { "управление" }));
        self.flash(format!("Подключение к {nick}…"));
        self.viewers.push(v);
    }

    pub fn remote_exec(&mut self, uid: u64, cmd: String) {
        let Some(p) = self.peers.get(&uid) else {
            if let Some(c) = self.cmds.get_mut(&uid) {
                c.output.push_str("\nПользователь не в сети\n");
            }
            return;
        };
        let addr = p.tcp_addr();
        let c = self.cmds.entry(uid).or_default();
        if c.password.is_empty() {
            c.output.push_str("\n⛔ Введите пароль администратора\n");
            return;
        }
        let id = new_id();
        c.busy = true;
        c.output.push_str(&format!("\n> {cmd}\n"));
        let pw = c.password.clone();
        self.pending.insert(id, Pending::Exec(uid));
        if let Some(n) = &self.net {
            n.request_auth(id, addr, Request::Exec(cmd), pw);
        }
    }

    pub fn remote_power(&mut self, uid: u64, action: u8, password: String) {
        let Some(p) = self.peers.get(&uid) else { return };
        let id = new_id();
        self.pending.insert(id, Pending::Power(uid));
        if let Some(n) = &self.net {
            n.request_auth(id, p.tcp_addr(), Request::Power(action), password);
        }
        let what = ["Выключение", "Перезагрузка", "Выход из системы", "Блокировка"].get(action as usize).copied().unwrap_or("?");
        let nick = p.p.nick.clone();
        self.log(Level::Info, "Удалённый доступ", format!("{what}: команда отправлена {nick}"));
    }

    fn auto_answer(&mut self, uid: u64) {
        let st = self.cfg.cur_state().clone();
        if !st.auto_answer || st.text.trim().is_empty() {
            return;
        }
        let c = self.chat_mut(uid);
        if c.last_auto_answer.is_some_and(|t| t.elapsed() < Duration::from_secs(600)) {
            return;
        }
        c.last_auto_answer = Some(Instant::now());
        self.send_private(uid, format!("[Автоответ — {}] {}", st.title, st.text));
    }

    // ------------------------------------------------------------ chats

    pub fn chat_path(&self, uid: u64) -> PathBuf {
        self.paths.chats.join(format!("{uid:016x}.jsonl"))
    }

    pub fn chat_mut(&mut self, uid: u64) -> &mut PrivateChat {
        self.chats.entry(uid).or_insert_with(|| PrivateChat::new(uid, Vec::new(), HashSet::new()))
    }

    pub fn open_chat(&mut self, uid: u64) {
        if uid == self.cfg.uid {
            return;
        }
        let c = self.chat_mut(uid);
        c.unread = 0;
        c.input.focus = true;
        self.private_sel = Some(uid);
        self.page = Page::Private;
    }

    fn rewrite_chat(&mut self, uid: u64) {
        let path = self.chat_path(uid);
        if let Some(c) = self.chats.get_mut(&uid) {
            c.dirty = false;
            let mut s = String::new();
            for l in &c.lines {
                if let Ok(j) = serde_json::to_string(l) {
                    s.push_str(&j);
                    s.push('\n');
                }
            }
            let tmp = path.with_extension("tmp");
            if std::fs::write(&tmp, s).is_ok() {
                let _ = std::fs::rename(&tmp, &path);
            }
        }
    }

    pub fn clear_chat(&mut self, uid: u64) {
        if let Some(c) = self.chats.get_mut(&uid) {
            c.lines.retain(|l| !l.delivered && l.from != uid); // keep undelivered outgoing
            c.dirty = true;
        }
    }

    pub fn send_private(&mut self, uid: u64, text: String) {
        let id = new_id();
        let line = Line {
            id,
            time: now_unix(),
            from: self.cfg.uid,
            nick: self.cfg.nick.clone(),
            color: self.cfg.color,
            text: text.clone(),
            me: false,
            delivered: false,
            pending: Vec::new(),
        };
        let path = self.chat_path(uid);
        append_jsonl(&path, &line);
        let c = self.chat_mut(uid);
        c.ids.insert(id);
        c.lines.push(line);
        if let (Some(n), Some(p)) = (&self.net, self.peers.get(&uid)) {
            if self.cfg.state != ST_OFFLINE {
                n.send(uid, p.ip, Body::Private { id, text, sent: now_unix() });
            }
        }
    }

    /// Re-send undelivered private messages to a peer that is online.
    pub fn resend_pending(&mut self, uid: u64) {
        let (Some(n), Some(p)) = (&self.net, self.peers.get(&uid)) else { return };
        if self.cfg.state == ST_OFFLINE {
            return;
        }
        if let Some(c) = self.chats.get(&uid) {
            for l in c.lines.iter().filter(|l| !l.delivered && l.from == self.cfg.uid) {
                n.send(uid, p.ip, Body::Private { id: l.id, text: l.text.clone(), sent: l.time });
            }
        }
    }

    pub fn send_typing(&mut self, uid: u64) {
        let Some(p) = self.peers.get(&uid) else { return };
        let ip = p.ip;
        let c = self.chat_mut(uid);
        if c.last_typing_sent.is_some_and(|t| t.elapsed() < Duration::from_secs(3)) {
            return;
        }
        c.last_typing_sent = Some(Instant::now());
        if let Some(n) = &self.net {
            n.send(uid, ip, Body::Typing);
        }
    }

    pub fn push_global(&mut self, line: Line) {
        append_jsonl(&self.paths.global, &line);
        self.global.push(line);
        let max = self.cfg.chat.history_lines.max(100);
        if self.global.len() > max + 200 {
            self.global.drain(..self.global.len() - max);
        }
    }

    pub fn system_line(&mut self, text: impl Into<String>) {
        let line = Line { id: new_id(), time: now_unix(), from: 0, text: text.into(), delivered: true, ..Default::default() };
        self.global.push(line);
    }

    pub fn send_global(&mut self, text: String, me: bool) {
        if self.cfg.state == ST_OFFLINE {
            self.system_line("Вы в состоянии «Не в сети» — сообщение не отправлено");
            return;
        }
        let id = new_id();
        self.global_ids.insert(id);
        let line = Line {
            id,
            time: now_unix(),
            from: self.cfg.uid,
            nick: self.cfg.nick.clone(),
            color: self.cfg.color,
            text: text.clone(),
            me,
            delivered: true,
            pending: Vec::new(),
        };
        self.push_global(line);
        if let Some(n) = &self.net {
            n.broadcast(Body::Global { id, text, me });
        }
    }

    /// Handle a phrase typed in Global Chat (commands start with "/").
    pub fn global_command(&mut self, input: &str) {
        let s = input.trim_end();
        if s.trim().is_empty() {
            return;
        }
        if !s.starts_with('/') || s.starts_with("//") {
            let s = s.strip_prefix('/').filter(|_| s.starts_with("//")).unwrap_or(s);
            self.send_global(s.to_string(), false);
            return;
        }
        let (cmd, rest) = s[1..].split_once(' ').map(|(a, b)| (a, b.trim())).unwrap_or((&s[1..], ""));
        match cmd.to_lowercase().as_str() {
            "me" => self.send_global(rest.to_string(), true),
            "nick" | "n" => self.set_nick(rest),
            "here" | "h" => {
                let mut names: Vec<String> = self.peers.values().map(|p| format!("{} ({})", p.p.nick, p.p.computer)).collect();
                names.insert(0, format!("{} (вы)", self.cfg.nick));
                self.system_line(format!("В сети {}: {}", names.len(), names.join(", ")));
            }
            "message" | "m" | "msg" => {
                let (name, text) = rest.split_once(' ').unwrap_or((rest, ""));
                match self.find_peer(name) {
                    Some(u) if !text.trim().is_empty() => {
                        self.send_private(u, text.trim().to_string());
                        self.open_chat(u);
                    }
                    Some(u) => self.open_chat(u),
                    None => self.system_line(format!("Пользователь «{name}» не найден")),
                }
            }
            "signal" | "s" => {
                if rest.is_empty() || rest.eq_ignore_ascii_case("/a") {
                    self.signal(None);
                } else if let Some(u) = self.find_peer(rest) {
                    self.signal(Some(u));
                } else {
                    self.system_line(format!("Пользователь «{rest}» не найден"));
                }
            }
            "info" | "i" => match self.find_peer(rest.split_whitespace().next().unwrap_or("")) {
                Some(u) => self.open_info(u, InfoTab::Details),
                None => self.system_line("Пользователь не найден"),
            },
            "clear" => self.global.clear(),
            "help" | "?" => self.system_line(
                "Команды: /me текст · /nick ник · /here · /m ник текст (личное) · /s [ник] (сигнал) · /i ник (информация) · /clear",
            ),
            _ => self.system_line(format!("Неизвестная команда /{cmd}. Справка: /help")),
        }
    }

    pub fn signal(&mut self, to: Option<u64>) {
        let Some(n) = &self.net else { return };
        match to {
            Some(u) => {
                if let Some(p) = self.peers.get(&u) {
                    n.send(u, p.ip, Body::Signal { broadcast: false });
                    let nick = p.p.nick.clone();
                    self.log(Level::Info, "Сигнал", format!("Сигнал отправлен: {nick}"));
                    self.flash(format!("🔔 Сигнал отправлен: {nick}"));
                }
            }
            None => {
                n.broadcast(Body::Signal { broadcast: true });
                self.log(Level::Info, "Сигнал", "Сигнал отправлен всем");
                self.flash("🔔 Сигнал отправлен всем");
            }
        }
    }

    pub fn appeal(&mut self, uid: u64) {
        let nick = self.peer_name(uid);
        self.global_input.text = format!("{nick}, {}", self.global_input.text);
        self.global_input.focus = true;
        self.page = Page::Chat;
    }

    // ------------------------------------------------------------ files

    pub fn offer_file(&mut self, uid: u64) {
        let Some(path) = rfd::FileDialog::new().set_title("Отправить файл").pick_file() else { return };
        self.offer_path(uid, path);
    }

    pub fn offer_path(&mut self, uid: u64, path: PathBuf) {
        let Some(p) = self.peers.get(&uid) else { return };
        let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
        let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        let id = new_id();
        self.shared.lock().unwrap().files.insert(id, (path.clone(), uid));
        if let Some(n) = &self.net {
            n.send(uid, p.ip, Body::FileOffer { id, name: name.clone(), size });
        }
        let nick = p.p.nick.clone();
        self.transfers.push(Transfer {
            id,
            peer: uid,
            nick: nick.clone(),
            name: name.clone(),
            size,
            incoming: false,
            state: TState::Waiting,
            done: 0,
            path: Some(path),
            cancel: Arc::new(AtomicBool::new(false)),
            started: Instant::now(),
        });
        self.dlg.transfers = true;
        self.log(Level::Info, "Файлы", format!("Файл «{name}» предложен {nick}"));
        self.flash(format!("📄 Файл «{name}» предложен {nick} — ждём ответа"));
    }

    pub fn accept_file(&mut self, id: u64, accept: bool) {
        let Some(t) = self.transfers.iter_mut().find(|t| t.id == id && t.incoming) else { return };
        let Some(p) = self.peers.get(&t.peer) else {
            t.state = TState::Failed("Отправитель не в сети".into());
            return;
        };
        if let Some(n) = &self.net {
            n.send(t.peer, p.ip, Body::FileAnswer { id, accept });
        }
        if !accept {
            t.state = TState::Declined;
            return;
        }
        let _ = std::fs::create_dir_all(&self.paths.downloads);
        let mut dest = self.paths.downloads.join(sanitize(&t.name));
        // don't overwrite a different finished file (but resume a .part with the same name)
        if dest.exists() && !crate::net::part_path(&dest).exists() {
            let stem = dest.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
            let ext = dest.extension().map(|s| format!(".{}", s.to_string_lossy())).unwrap_or_default();
            for i in 1.. {
                let c = self.paths.downloads.join(format!("{stem} ({i}){ext}"));
                if !c.exists() {
                    dest = c;
                    break;
                }
            }
        }
        t.path = Some(dest.clone());
        t.state = TState::Running;
        t.started = Instant::now();
        t.cancel.store(false, std::sync::atomic::Ordering::Relaxed);
        if let Some(n) = &self.net {
            n.download(id, p.tcp_addr(), dest, t.cancel.clone());
        }
    }

    fn on_transfer(&mut self, id: u64, done: u64, total: u64, finished: bool, error: Option<String>) {
        let Some(t) = self.transfers.iter_mut().find(|t| t.id == id) else { return };
        if total > 0 {
            t.done = done;
            t.size = total;
        }
        if t.state == TState::Waiting {
            t.state = TState::Running;
        }
        if finished {
            match error {
                Some(e) => {
                    t.state = TState::Failed(e.clone());
                    let msg = format!("«{}»: ошибка — {e}", t.name);
                    self.log(Level::Warn, "Файлы", msg);
                }
                None => {
                    if t.incoming || t.done >= t.size {
                        t.state = TState::Done;
                        t.done = t.size;
                        let msg = if t.incoming {
                            format!("Файл «{}» от {} получен: {}", t.name, t.nick, t.path.as_ref().map(|p| p.display().to_string()).unwrap_or_default())
                        } else {
                            format!("Файл «{}» отправлен {}", t.name, t.nick)
                        };
                        let rec = t.incoming.then(|| crate::received::Received {
                            time: chrono::Local::now().timestamp(),
                            from: t.peer,
                            nick: t.nick.clone(),
                            computer: String::new(),
                            name: t.name.clone(),
                            size: t.size,
                            path: t.path.as_ref().map(|p| p.display().to_string()).unwrap_or_default(),
                        });
                        self.log(Level::Info, "Файлы", msg);
                        if let Some(mut rec) = rec {
                            rec.computer = self.peers.get(&rec.from).map(|p| p.p.computer.clone()).unwrap_or_default();
                            crate::received::append(&self.paths, &rec);
                            self.received.push(rec);
                        }
                    }
                }
            }
        }
    }

    // ------------------------------------------------------------ remote info

    pub fn open_info(&mut self, uid: u64, tab: InfoTab) {
        let w = self.infos.entry(uid).or_insert_with(|| InfoWin {
            uid,
            tab,
            details: None,
            avatar: None,
            procs: Vec::new(),
            sel_pid: None,
            shot: None,
            shot_jpeg: Vec::new(),
            monitors: 1,
            monitor: 0,
            quality: 70,
            fit: true,
            clipboard: String::new(),
            status: String::new(),
            busy: false,
            auto_secs: 0,
            last_auto: None,
        });
        w.tab = tab;
        self.info_refresh(uid);
    }

    pub fn info_refresh(&mut self, uid: u64) {
        let Some(w) = self.infos.get(&uid) else { return };
        let tab = w.tab;
        if uid == self.cfg.uid {
            // local info without network
            let w = self.infos.get_mut(&uid).unwrap();
            match tab {
                InfoTab::Details => w.details = Some(self.cfg.details.clone()),
                InfoTab::Processes => w.procs = crate::platform::processes(),
                InfoTab::Clipboard => w.clipboard = arboard::Clipboard::new().and_then(|mut c| c.get_text()).unwrap_or_default(),
                InfoTab::Screenshot => {
                    let (mon, q) = (w.monitor, w.quality);
                    let r = crate::platform::screenshot(mon as usize).map(|(img, m)| {
                        let (iw, ih) = (img.width(), img.height());
                        let rgb = image::DynamicImage::ImageRgba8(img).to_rgb8();
                        let mut data = Vec::new();
                        let _ = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut data, q).encode_image(&rgb);
                        Response::Image { data, w: iw, h: ih, monitors: m as u8 }
                    });
                    let id = new_id();
                    self.pending.insert(id, Pending::Screenshot(uid));
                    self.on_reply(id, r);
                }
            }
            return;
        }
        let Some(p) = self.peers.get(&uid) else {
            if let Some(w) = self.infos.get_mut(&uid) {
                w.status = "Пользователь не в сети".into();
            }
            return;
        };
        let addr = p.tcp_addr();
        let (req, pend) = match tab {
            InfoTab::Details => (Request::Details, Pending::Details { uid, for_window: true }),
            InfoTab::Processes => (Request::Processes, Pending::Processes(uid)),
            InfoTab::Screenshot => (Request::Screenshot { monitor: w.monitor, jpeg_quality: w.quality }, Pending::Screenshot(uid)),
            InfoTab::Clipboard => (Request::Clipboard, Pending::Clipboard(uid)),
        };
        let id = new_id();
        self.pending.insert(id, pend);
        if let Some(w) = self.infos.get_mut(&uid) {
            w.busy = true;
            w.status = "Запрос…".into();
        }
        if let Some(n) = &self.net {
            n.request(id, addr, req);
        }
    }

    pub fn kill_remote(&mut self, uid: u64, pid: u32) {
        let Some(p) = self.peers.get(&uid) else { return };
        let id = new_id();
        self.pending.insert(id, Pending::Kill(uid));
        if let Some(n) = &self.net {
            n.request(id, p.tcp_addr(), Request::Kill(pid));
        }
    }

    pub fn request_details(&mut self, uid: u64, for_window: bool) {
        let Some(p) = self.peers.get(&uid) else { return };
        let id = new_id();
        self.pending.insert(id, Pending::Details { uid, for_window });
        if let Some(n) = &self.net {
            n.request(id, p.tcp_addr(), Request::Details);
        }
    }

    fn on_reply(&mut self, req_id: u64, result: Result<Response, String>) {
        let Some(pend) = self.pending.remove(&req_id) else { return };
        let uid = match pend {
            Pending::Details { uid, .. }
            | Pending::Processes(uid)
            | Pending::Kill(uid)
            | Pending::Screenshot(uid)
            | Pending::Clipboard(uid)
            | Pending::Exec(uid)
            | Pending::Power(uid) => uid,
        };
        match (&pend, &result) {
            (Pending::Exec(_), _) => {
                let c = self.cmds.entry(uid).or_default();
                c.busy = false;
                match &result {
                    Ok(Response::Text(t)) => c.output.push_str(t),
                    Ok(Response::Denied(m)) | Ok(Response::Error(m)) => c.output.push_str(&format!("⛔ {m}\n")),
                    Err(e) => c.output.push_str(&format!("⛔ {e}\n")),
                    _ => {}
                }
                if !c.output.ends_with('\n') {
                    c.output.push('\n');
                }
                return;
            }
            (Pending::Power(_), r) => {
                let nick = self.peer_name(uid);
                match r {
                    Ok(Response::Ok) => self.flash(format!("Команда питания выполнена: {nick}")),
                    Ok(Response::Denied(m)) | Ok(Response::Error(m)) => {
                        let m = m.clone();
                        self.flash(format!("⛔ {m}"));
                        self.log(Level::Warn, "Удалённый доступ", format!("{nick}: {m}"));
                    }
                    Err(e) => {
                        let e = e.clone();
                        self.flash(format!("⛔ {e}"));
                    }
                    _ => {}
                }
                return;
            }
            _ => {}
        }
        let ctx = self.ctx.clone();
        let status_err = |e: String| -> String { e };
        let res = match result {
            Ok(Response::Denied(m)) | Ok(Response::Error(m)) => Err(m),
            Ok(r) => Ok(r),
            Err(e) => Err(status_err(e)),
        };
        match (pend, res) {
            (Pending::Details { uid, for_window }, Ok(Response::Details(d))) => {
                let tex = avatar_texture(&ctx, uid, &d.picture_png);
                if let Some(p) = self.peers.get_mut(&uid) {
                    p.details_rev = d.modified;
                    p.avatar = tex.clone();
                    p.details = Some(d.clone());
                }
                if for_window {
                    if let Some(w) = self.infos.get_mut(&uid) {
                        w.avatar = tex;
                        w.details = Some(d);
                        w.status.clear();
                    }
                }
            }
            (Pending::Processes(_), Ok(Response::Processes(mut v))) => {
                v.sort_by(|a, b| b.title.is_empty().cmp(&a.title.is_empty()).then(a.name.to_lowercase().cmp(&b.name.to_lowercase())));
                if let Some(w) = self.infos.get_mut(&uid) {
                    w.status = format!("Процессов: {}", v.len());
                    w.procs = v;
                }
            }
            (Pending::Kill(_), Ok(_)) => {
                if let Some(w) = self.infos.get_mut(&uid) {
                    w.status = "Процесс завершён".into();
                    w.tab = InfoTab::Processes;
                }
                self.info_refresh(uid);
                return;
            }
            (Pending::Screenshot(_), Ok(Response::Image { data, w: iw, h: ih, monitors })) => {
                if let Ok(img) = image::load_from_memory(&data) {
                    let rgba = img.to_rgba8();
                    let ci = egui::ColorImage::from_rgba_unmultiplied([rgba.width() as usize, rgba.height() as usize], rgba.as_raw());
                    let tex = ctx.load_texture(format!("shot:{uid}"), ci, egui::TextureOptions::LINEAR);
                    if let Some(w) = self.infos.get_mut(&uid) {
                        w.status = format!("{}×{}, {} (мониторов: {})", iw, ih, fmt_size(data.len() as u64), monitors);
                        w.shot = Some((tex, iw, ih));
                        w.shot_jpeg = data;
                        w.monitors = monitors.max(1);
                    }
                }
            }
            (Pending::Clipboard(_), Ok(Response::Text(t))) => {
                if let Some(w) = self.infos.get_mut(&uid) {
                    w.status = format!("{} символов", t.chars().count());
                    w.clipboard = t;
                }
            }
            (_, Err(e)) => {
                if let Some(w) = self.infos.get_mut(&uid) {
                    w.status = e;
                }
            }
            _ => {}
        }
        if let Some(w) = self.infos.get_mut(&uid) {
            w.busy = false;
        }
    }

    pub fn shutdown(&mut self) {
        if let Some(n) = &self.net {
            if self.cfg.state != ST_OFFLINE {
                n.broadcast(Body::Bye);
            }
        }
        let dirty: Vec<u64> = self.chats.values().filter(|c| c.dirty).map(|c| c.uid).collect();
        for u in dirty {
            self.rewrite_chat(u);
        }
        let gs: Vec<u64> = self.groups.keys().copied().collect();
        for g in gs {
            self.save_group(g);
        }
        for v in &self.viewers {
            v.send(crate::proto::RemoteInput::Close);
        }
        for s in &self.host_sessions {
            s.stop.store(true, std::sync::atomic::Ordering::Relaxed);
        }
        self.save_cfg();
    }
}

impl PrivateChat {
    fn new(uid: u64, lines: Vec<Line>, ids: HashSet<u64>) -> Self {
        PrivateChat {
            uid,
            lines,
            ids,
            search: ChatSearch::default(),
            input: ChatInput::default(),
            unread: 0,
            dirty: false,
            last_auto_answer: None,
            last_typing_sent: None,
        }
    }
    pub fn empty(uid: u64) -> Self {
        Self::new(uid, Vec::new(), HashSet::new())
    }
    pub fn pending_count(&self) -> usize {
        self.lines.iter().filter(|l| !l.delivered && l.from != self.uid).count()
    }
}

pub fn avatar_texture(ctx: &egui::Context, uid: u64, png: &[u8]) -> Option<TextureHandle> {
    if png.is_empty() {
        return None;
    }
    let img = image::load_from_memory(png).ok()?.to_rgba8();
    let ci = egui::ColorImage::from_rgba_unmultiplied([img.width() as usize, img.height() as usize], img.as_raw());
    Some(ctx.load_texture(format!("avatar:{uid}"), ci, egui::TextureOptions::LINEAR))
}

fn sanitize(name: &str) -> String {
    let s: String = name.chars().map(|c| if "<>:\"/\\|?*".contains(c) || c.is_control() { '_' } else { c }).collect();
    let s = s.trim().trim_matches('.').to_string();
    if s.is_empty() {
        "file".into()
    } else {
        s
    }
}
