use crate::net::ConnCfg;
use crate::proto::UserDetails;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

pub const STD_STATES: [&str; 6] = ["Готов", "Работаю", "Не беспокоить", "Приватный", "Отошёл", "Не в сети"];
pub const ST_PRIVATE: u8 = 3;
pub const ST_AWAY: u8 = 4;
pub const ST_OFFLINE: u8 = 5;
pub const MAX_CUSTOM_STATES: usize = 6;
pub const CFG_VERSION: u32 = 2;

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct StateDef {
    pub title: String,
    pub text: String,
    pub auto_answer: bool,
    pub no_sounds: bool,
    pub no_auto_away: bool,
    pub no_popups: bool,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClickAction {
    None,
    PrivateChat,
    Info,
    Appeal,
    Signal,
    SendFile,
}

impl ClickAction {
    pub const ALL: [ClickAction; 6] =
        [ClickAction::None, ClickAction::PrivateChat, ClickAction::Info, ClickAction::Appeal, ClickAction::Signal, ClickAction::SendFile];
    pub fn title(self) -> &'static str {
        match self {
            ClickAction::None => "Ничего",
            ClickAction::PrivateChat => "Личное сообщение",
            ClickAction::Info => "Информация",
            ClickAction::Appeal => "Обращение в общем чате",
            ClickAction::Signal => "Сигнал",
            ClickAction::SendFile => "Отправить файл",
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct GeneralCfg {
    pub close_to_tray: bool,
    pub popup_on_private: bool,
    pub popup_on_global: bool,
    pub tip_enabled: bool,
    pub tip_secs: u32,
    /// 0 TL, 1 TR, 2 BL, 3 BR
    pub tip_corner: u8,
    pub refresh_timeout_ms: u32,
    /// double click, ctrl+click, alt+click, shift+click
    pub click_actions: [ClickAction; 4],
    pub ui_scale: f32,
    pub start_minimized: bool,
    /// blink the taskbar button / tray icon when a message arrives while I am away
    pub flash_on_message: bool,
    /// seconds without keyboard/mouse input after which I count as "away from the computer"
    pub away_secs: u32,
}

impl Default for GeneralCfg {
    fn default() -> Self {
        GeneralCfg {
            close_to_tray: true,
            popup_on_private: false,
            popup_on_global: false,
            tip_enabled: true,
            tip_secs: 6,
            tip_corner: 3,
            refresh_timeout_ms: 3000,
            click_actions: [ClickAction::PrivateChat, ClickAction::Info, ClickAction::SendFile, ClickAction::Appeal],
            ui_scale: 1.0,
            start_minimized: false,
            flash_on_message: true,
            away_secs: 30,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct PrivacyCfg {
    /// true: the restrictions below apply in all states; false: only in the "Private" state.
    /// In the "Private" state everything (active window, processes, screen, clipboard) is hidden anyway.
    pub global: bool,
    pub no_fg_title: bool,
    pub no_processes: bool,
    pub no_screenshot: bool,
    pub no_clipboard: bool,
    pub no_remote_kill: bool,
}

impl Default for PrivacyCfg {
    fn default() -> Self {
        PrivacyCfg { global: true, no_fg_title: false, no_processes: true, no_screenshot: true, no_clipboard: true, no_remote_kill: true }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct ChatCfg {
    pub no_emoticons: bool,
    pub font_size: f32,
    pub macros: Vec<String>,
    pub autoscroll: bool,
    pub show_system: bool,
    pub history_lines: usize,
}

impl Default for ChatCfg {
    fn default() -> Self {
        ChatCfg {
            no_emoticons: false,
            font_size: 14.0,
            macros: vec![String::new(); 12],
            autoscroll: true,
            show_system: true,
            history_lines: 2000,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct SoundCfg {
    pub enabled: bool,
    /// event -> (enabled, wav file name or full path)
    pub events: BTreeMap<String, (bool, String)>,
}

pub const SOUND_EVENTS: [(&str, &str, &str); 9] = [
    ("global", "Сообщение в общем чате", "chat.wav"),
    ("me", "Действие /me", "me.wav"),
    ("private", "Личное сообщение", "message.wav"),
    ("signal", "Сигнал", "signal.wav"),
    ("user_join", "Пользователь появился", "user_join.wav"),
    ("user_leave", "Пользователь ушёл", "user_leave.wav"),
    ("file", "Входящий файл", "alert.wav"),
    ("state", "Смена состояния", "change.wav"),
    ("error", "Ошибка", "error.wav"),
];

impl Default for SoundCfg {
    fn default() -> Self {
        let mut events = BTreeMap::new();
        for (k, _, f) in SOUND_EVENTS {
            events.insert(k.to_string(), (k != "state", f.to_string()));
        }
        SoundCfg { enabled: true, events }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct AwayCfg {
    pub on_idle: bool,
    pub idle_min: u32,
}

impl Default for AwayCfg {
    fn default() -> Self {
        AwayCfg { on_idle: true, idle_min: 10 }
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum Col {
    Nick,
    Computer,
    State,
    InState,
    Idle,
    Ping,
    FgTitle,
    Ip,
    Uptime,
    Version,
    Os,
    OsUptime,
    Time,
}

impl Col {
    pub fn title(self) -> &'static str {
        match self {
            Col::Nick => "Ник",
            Col::Computer => "Компьютер",
            Col::State => "Состояние",
            Col::InState => "В состоянии",
            Col::Idle => "Бездействие",
            Col::Ping => "Пинг",
            Col::FgTitle => "Активное окно",
            Col::Ip => "IP-адрес",
            Col::Uptime => "Работает",
            Col::Version => "Версия",
            Col::Os => "ОС",
            Col::OsUptime => "Аптайм ОС",
            Col::Time => "Время",
        }
    }
}

pub fn default_columns() -> Vec<(Col, bool)> {
    vec![
        (Col::Nick, true),
        (Col::Computer, true),
        (Col::State, true),
        (Col::InState, true),
        (Col::Idle, true),
        (Col::Ping, true),
        (Col::FgTitle, true),
        (Col::Ip, true),
        (Col::Uptime, false),
        (Col::Version, false),
        (Col::Os, false),
        (Col::OsUptime, false),
        (Col::Time, false),
    ]
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct Config {
    pub uid: u64,
    pub nick: String,
    pub nicks: Vec<String>,
    pub color: [u8; 3],
    pub state: u8,
    pub states: Vec<StateDef>,
    pub conn: ConnCfg,
    pub general: GeneralCfg,
    pub privacy: PrivacyCfg,
    /// settings migrations (missing in old files -> 0)
    #[serde(default)]
    pub cfg_version: u32,
    pub chat: ChatCfg,
    pub sounds: SoundCfg,
    pub away: AwayCfg,
    pub emoticon_pack: String,
    pub assets_dir: String,
    pub groups: Vec<String>,
    pub user_groups: HashMap<u64, String>,
    pub groups_enabled: bool,
    pub details: UserDetails,
    pub columns: Vec<(Col, bool)>,
    pub users_panel: bool,
    /// nick/computer of peers ever seen, so private chats can be listed while they are offline
    pub known: HashMap<u64, (String, String)>,
    pub window: Option<[f32; 4]>,
}

pub fn palette_color(seed: u64) -> [u8; 3] {
    const P: [[u8; 3]; 8] =
        [[200, 30, 30], [30, 120, 30], [30, 60, 200], [160, 40, 160], [200, 110, 0], [0, 130, 140], [120, 70, 20], [90, 90, 90]];
    P[(seed % P.len() as u64) as usize]
}

impl Default for Config {
    fn default() -> Self {
        let user = crate::platform::os_user();
        let comp = crate::platform::computer_name();
        let mut states: Vec<StateDef> = STD_STATES.iter().map(|t| StateDef { title: t.to_string(), ..Default::default() }).collect();
        states[2].no_sounds = true;
        states[2].no_popups = true;
        states[4].text = "Нет на месте".into();
        Config {
            uid: rand::random::<u64>() | 1,
            nick: user.clone(),
            nicks: vec![user.clone()],
            color: palette_color(crate::proto::hash64(&[comp.as_bytes(), user.as_bytes()])),
            state: 0,
            states,
            conn: ConnCfg::default(),
            general: GeneralCfg::default(),
            privacy: PrivacyCfg::default(),
            cfg_version: CFG_VERSION,
            chat: ChatCfg::default(),
            sounds: SoundCfg::default(),
            away: AwayCfg::default(),
            emoticon_pack: String::new(),
            assets_dir: String::new(),
            groups: vec!["Общие".into(), "Друзья".into(), "Коллеги".into()],
            user_groups: HashMap::new(),
            groups_enabled: true,
            details: UserDetails::default(),
            columns: default_columns(),
            users_panel: true,
            known: HashMap::new(),
            window: None,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Paths {
    pub data: PathBuf,
    pub config: PathBuf,
    pub chats: PathBuf,
    pub global: PathBuf,
    pub events: PathBuf,
    pub downloads: PathBuf,
    pub groups: PathBuf,
}

impl Paths {
    pub fn new(profile: Option<&str>) -> Paths {
        let mut data = dirs::config_dir().unwrap_or_else(|| PathBuf::from(".")).join("NassiRS");
        if let Some(p) = profile {
            data = data.join("profiles").join(p);
        }
        let chats = data.join("Chats");
        let _ = std::fs::create_dir_all(&chats);
        let groups = data.join("Groups");
        let _ = std::fs::create_dir_all(&groups);
        Paths {
            config: data.join("config.json"),
            global: data.join("global_chat.jsonl"),
            events: data.join("events.log"),
            downloads: data.join("Received Files"),
            chats,
            groups,
            data,
        }
    }

    /// Folder with Emoticons/ and Sounds/: explicit setting, else the exe folder
    /// or one of its parents (installed: the program folder; development: the project root).
    pub fn find_assets(cfg: &Config) -> Option<PathBuf> {
        if !cfg.assets_dir.trim().is_empty() {
            let p = PathBuf::from(cfg.assets_dir.trim());
            if p.is_dir() {
                return Some(p);
            }
        }
        let exe = std::env::current_exe().ok()?;
        let mut dir: Option<&Path> = exe.parent();
        for _ in 0..6 {
            let d = dir?;
            if d.join("Emoticons").is_dir() || d.join("Sounds").is_dir() {
                return Some(d.to_path_buf());
            }
            dir = d.parent();
        }
        None
    }
}

impl Config {
    /// Returns (config, is_new, warning)
    pub fn load(paths: &Paths) -> (Config, bool, Option<String>) {
        let try_read = |p: &Path| std::fs::read_to_string(p).ok().and_then(|s| serde_json::from_str::<Config>(&s).ok());
        if let Some(mut c) = try_read(&paths.config) {
            c.fixup();
            return (c, false, None);
        }
        // main file missing or broken: fall back to the last good backup instead of resetting
        let bak = paths.config.with_extension("json.bak");
        if let Some(mut c) = try_read(&bak) {
            c.fixup();
            return (c, false, Some("config.json повреждён — настройки восстановлены из резервной копии".into()));
        }
        let warn = paths.config.exists().then(|| {
            let _ = std::fs::copy(&paths.config, paths.config.with_extension("json.broken"));
            "config.json повреждён — созданы настройки по умолчанию (старый файл: config.json.broken)".to_string()
        });
        (Config::default(), true, warn)
    }

    fn fixup(&mut self) {
        if self.cfg_version < 2 {
            // v2: the active window title is visible to others by default
            self.privacy.no_fg_title = false;
        }
        self.cfg_version = CFG_VERSION;
        let def = Config::default();
        if self.uid == 0 {
            self.uid = def.uid;
        }
        if self.states.len() < 6 {
            self.states = def.states.clone();
        }
        for (i, t) in STD_STATES.iter().enumerate() {
            self.states[i].title = t.to_string();
        }
        if self.state as usize >= self.states.len() || self.state == ST_OFFLINE {
            self.state = 0;
        }
        if self.columns.is_empty() {
            self.columns = default_columns();
        }
        for c in default_columns() {
            if !self.columns.iter().any(|x| x.0 == c.0) {
                self.columns.push(c);
            }
        }
        self.chat.macros.resize(12, String::new());
        for (k, _, f) in SOUND_EVENTS {
            self.sounds.events.entry(k.to_string()).or_insert((true, f.to_string()));
        }
        if self.nick.trim().is_empty() {
            self.nick = def.nick;
        }
        if self.general.away_secs == 0 {
            self.general.away_secs = 30;
        }
        if !(0.5..=3.0).contains(&self.general.ui_scale) {
            self.general.ui_scale = 1.0;
        }
        if self.conn.port == 0 {
            self.conn.port = crate::net::DEFAULT_PORT;
        }
    }

    /// Atomic save + backup of the previous good file, so settings never get lost.
    pub fn save(&self, paths: &Paths) -> Result<(), String> {
        let s = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        let tmp = paths.config.with_extension("json.tmp");
        std::fs::write(&tmp, s).map_err(|e| e.to_string())?;
        if paths.config.exists() {
            let _ = std::fs::copy(&paths.config, paths.config.with_extension("json.bak"));
        }
        std::fs::rename(&tmp, &paths.config).map_err(|e| e.to_string())
    }

    pub fn privacy_active(&self) -> bool {
        self.privacy.global || self.state == ST_PRIVATE
    }

    /// "Приватный" state: full privacy, regardless of the check boxes.
    pub fn private_state(&self) -> bool {
        self.state == ST_PRIVATE
    }

    pub fn effective_allow(&self) -> crate::proto::Allow {
        let p = &self.privacy;
        let on = self.privacy_active();
        let full = self.private_state();
        crate::proto::Allow {
            fg_title: !(full || (on && p.no_fg_title)),
            processes: !(full || (on && p.no_processes)),
            screenshot: !(full || (on && p.no_screenshot)),
            clipboard: !(full || (on && p.no_clipboard)),
            remote_kill: !p.no_remote_kill,
            // remote access is always on and protected by the built-in admin password
            remote: 2,
        }
    }

    pub fn cur_state(&self) -> &StateDef {
        &self.states[self.state as usize]
    }
}
