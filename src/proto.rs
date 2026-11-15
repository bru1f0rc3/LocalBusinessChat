//! Wire protocol.
//!
//! UDP: every datagram is `MAGIC | nonce(12) | ChaCha20-Poly1305(bincode(Envelope))`.
//! TCP (same port number): length-prefixed frames sealed the same way.
//! The key is derived from the optional "network password", so only peers
//! with the same password understand each other.

use chacha20poly1305::aead::{Aead, KeyInit};
use chacha20poly1305::{ChaCha20Poly1305, Key, Nonce};
use rand::RngCore;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const MAGIC: [u8; 4] = *b"NRS4";
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
pub const MAX_DATAGRAM: usize = 60_000;

#[derive(Clone)]
pub struct Cipher(ChaCha20Poly1305);

impl Cipher {
    pub fn new(password: &str) -> Self {
        let mut h = Sha256::new();
        h.update(b"nassi-rs/network-key/v3\0");
        h.update(password.as_bytes());
        let k = h.finalize();
        Cipher(ChaCha20Poly1305::new(Key::from_slice(&k)))
    }

    pub fn seal<T: Serialize>(&self, v: &T) -> Vec<u8> {
        let plain = bincode::serialize(v).expect("serialize");
        let mut nonce = [0u8; 12];
        rand::thread_rng().fill_bytes(&mut nonce);
        let ct = self.0.encrypt(Nonce::from_slice(&nonce), plain.as_slice()).expect("encrypt");
        let mut out = Vec::with_capacity(4 + 12 + ct.len());
        out.extend_from_slice(&MAGIC);
        out.extend_from_slice(&nonce);
        out.extend_from_slice(&ct);
        out
    }

    pub fn open<T: DeserializeOwned>(&self, data: &[u8]) -> Option<T> {
        if data.len() < 32 || data[..4] != MAGIC {
            return None;
        }
        let plain = self.0.decrypt(Nonce::from_slice(&data[4..16]), &data[16..]).ok()?;
        bincode::deserialize(&plain).ok()
    }
}

pub fn hash64(parts: &[&[u8]]) -> u64 {
    let mut h = Sha256::new();
    for p in parts {
        h.update(p);
        h.update([0u8]);
    }
    let d = h.finalize();
    u64::from_le_bytes(d[..8].try_into().unwrap())
}

pub fn now_unix() -> i64 {
    chrono::Utc::now().timestamp()
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct Allow {
    pub fg_title: bool,
    pub processes: bool,
    pub screenshot: bool,
    pub clipboard: bool,
    pub remote_kill: bool,
    /// remote desktop: 0 disabled, 1 with user confirmation, 2 password only, 3 password or confirmation
    pub remote: u8,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Presence {
    pub nick: String,
    pub computer: String,
    pub os_user: String,
    /// 0 Ready, 1 Working, 2 DND, 3 Private, 4 Away, 5 Offline, 6.. custom
    pub state: u8,
    pub state_title: String,
    pub state_text: String,
    pub color: [u8; 3],
    pub idle_secs: u32,
    pub in_state_secs: u32,
    pub fg_title: Option<String>,
    /// 0 normal, 1 minimized, 2 maximized, 3 tray
    pub win_state: u8,
    pub uptime_secs: u32,
    pub os_uptime_secs: u32,
    pub os_name: String,
    pub version: String,
    pub tcp_port: u16,
    pub allow: Allow,
    pub details_rev: i64,
    pub time: i64,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Envelope {
    pub from: u64,
    pub to: Option<u64>,
    pub body: Body,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum Body {
    Hello(Presence),
    Presence(Presence),
    Bye,
    /// "Refresh users list": everybody answers with Presence.
    Query,
    Ping(u64),
    Pong(u64),

    /// Global chat phrase (to everybody).
    Global { id: u64, text: String, me: bool },
    /// Private chat message; receiver answers `PrivateAck(id)`. Resent until acked.
    Private { id: u64, text: String, sent: i64 },
    PrivateAck(u64),
    Typing,

    Signal { broadcast: bool },

    FileOffer { id: u64, name: String, size: u64 },
    FileAnswer { id: u64, accept: bool },

    /// Group conversation description; newest `rev` wins. Sent to all members.
    GroupInfo(GroupInfo),
    /// Group message, sent to every member separately; acked with GroupAck.
    GroupMsg { conv: u64, rev: u64, id: u64, text: String, sent: i64, me: bool },
    GroupAck { conv: u64, id: u64 },
    /// "I don't know this conversation (or have an old version) - send me GroupInfo".
    GroupQuery(u64),
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct GroupInfo {
    pub id: u64,
    pub title: String,
    pub owner: u64,
    pub members: Vec<u64>,
    /// nick cache for members (uid, nick)
    pub names: Vec<(u64, String)>,
    /// change time in ms; newest wins
    pub rev: u64,
}

impl GroupInfo {
    pub fn name_of(&self, uid: u64) -> Option<&str> {
        self.names.iter().find(|n| n.0 == uid).map(|n| n.1.as_str())
    }
}

pub fn now_ms() -> u64 {
    chrono::Utc::now().timestamp_millis() as u64
}

// ---------------------------------------------------------------- remote desktop

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Tile {
    pub x: u16,
    pub y: u16,
    pub w: u16,
    pub h: u16,
    pub jpeg: Vec<u8>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct RemoteFrame {
    pub w: u32,
    pub h: u32,
    pub monitors: u8,
    /// cursor position relative to the captured area
    pub cursor: (i32, i32),
    pub tiles: Vec<Tile>,
}

/// Viewer -> host messages during a remote desktop session.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum RemoteInput {
    Mouse { x: i32, y: i32 },
    /// 0 left, 1 right, 2 middle
    Button { b: u8, down: bool },
    Wheel { dx: i32, dy: i32 },
    Key { vk: u16, down: bool },
    Text(String),
    Settings { quality: u8, fps: u8, monitor: u8 },
    Full,
    Clipboard(String),
    Close,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct RemoteAuth {
    /// sha256(key || nonce) where key = remote_key(password); None = ask the user
    pub proof: Option<Vec<u8>>,
}

pub fn remote_key(password: &str) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(b"nassi-rs/remote-access/v1\0");
    h.update(password.as_bytes());
    h.finalize().into()
}

/// Key of the built-in administrator password (only its hash is compiled in).
/// The password is fixed and cannot be changed from the program.
pub const ADMIN_KEY_HEX: &str = "c94c9ce8b004d1da172727f79c8906ed9fc2bb5e82dce8f2f3ca24654cb00084";

pub fn admin_key() -> [u8; 32] {
    let mut k = [0u8; 32];
    for (i, b) in k.iter_mut().enumerate() {
        *b = u8::from_str_radix(&ADMIN_KEY_HEX[i * 2..i * 2 + 2], 16).unwrap_or(0);
    }
    k
}

pub fn remote_proof(key: &[u8], nonce: &[u8]) -> Vec<u8> {
    let mut h = Sha256::new();
    h.update(key);
    h.update(nonce);
    h.finalize().to_vec()
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum Request {
    Details,
    Screenshot { monitor: u8, jpeg_quality: u8 },
    Processes,
    Kill(u32),
    Clipboard,
    File { id: u64, offset: u64 },
    /// Start a remote desktop session (answered with Challenge, then RemoteAuth is sent).
    RemoteHello { view_only: bool },
    /// Run a command line (needs a granted control session).
    Exec(String),
    /// 0 shutdown, 1 reboot, 2 log off, 3 lock (needs a granted control session).
    Power(u8),
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct UserDetails {
    pub full_name: String,
    pub gender: u8,
    pub age: u16,
    pub company: String,
    pub department: String,
    pub position: String,
    pub phone_work: String,
    pub phone_mobile: String,
    pub email: String,
    pub address: String,
    pub notes: String,
    pub picture_png: Vec<u8>,
    pub modified: i64,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ProcInfo {
    pub pid: u32,
    pub name: String,
    pub title: String,
    pub active: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum Response {
    Denied(String),
    Error(String),
    Ok,
    Details(UserDetails),
    Image { data: Vec<u8>, w: u32, h: u32, monitors: u8 },
    Processes(Vec<ProcInfo>),
    Text(String),
    /// Followed by `Chunk` frames, terminated by an empty chunk.
    FileStart { size: u64, offset: u64 },
    Chunk(Vec<u8>),
    Challenge { nonce: Vec<u8>, has_password: bool, confirm: bool },
    Frame(RemoteFrame),
}
