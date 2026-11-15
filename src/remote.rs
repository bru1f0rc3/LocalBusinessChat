//! Remote desktop (Radmin-like): host side streams changed screen tiles as JPEG,
//! viewer side sends mouse / keyboard input.
//!
//! Security: a session starts only if the host allows remote access AND either
//! the viewer proves knowledge of the host's remote password (challenge-response,
//! the password never travels) or the host user confirms the request. The host
//! always shows an indicator with a "disconnect" button.

use crate::net::{read_frame, write_frame, NetEvent, Shared};
use crate::proto::*;
use eframe::egui;
use rand::RngCore;
use std::io;
use std::net::{IpAddr, SocketAddr, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const TILE: u32 = 128;

#[derive(Clone, Copy)]
struct Settings {
    quality: u8,
    fps: u8,
    monitor: u8,
    full: bool,
}

fn other(m: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::Other, m.into())
}

fn encode_tile(img: &image::RgbaImage, x: u32, y: u32, w: u32, h: u32, q: u8) -> Vec<u8> {
    let mut rgb = Vec::with_capacity((w * h * 3) as usize);
    let stride = img.width() as usize * 4;
    let raw = img.as_raw();
    for row in y..y + h {
        let start = row as usize * stride + x as usize * 4;
        for px in raw[start..start + w as usize * 4].chunks_exact(4) {
            rgb.extend_from_slice(&px[..3]);
        }
    }
    let mut out = Vec::new();
    let _ = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, q).encode(&rgb, w, h, image::ExtendedColorType::Rgb8);
    out
}

fn tile_changed(a: &image::RgbaImage, b: &image::RgbaImage, x: u32, y: u32, w: u32, h: u32) -> bool {
    let stride = a.width() as usize * 4;
    let (ra, rb) = (a.as_raw(), b.as_raw());
    for row in y..y + h {
        let s = row as usize * stride + x as usize * 4;
        let e = s + w as usize * 4;
        if ra[s..e] != rb[s..e] {
            return true;
        }
    }
    false
}

/// Challenge-response check of the built-in administrator password, done for
/// EVERY remote action (desktop session, command, power). The password itself
/// never travels over the network. A wrong password costs 2 seconds.
pub fn check_admin_password(s: &mut TcpStream, cipher: &Cipher, tx: &Sender<NetEvent>, ip: IpAddr, what: &str) -> io::Result<bool> {
    let mut nonce = vec![0u8; 16];
    rand::thread_rng().fill_bytes(&mut nonce);
    write_frame(s, cipher, &Response::Challenge { nonce: nonce.clone(), has_password: true, confirm: false })?;
    s.set_read_timeout(Some(Duration::from_secs(30)))?;
    let auth: RemoteAuth = read_frame(s, cipher)?;
    let ok = auth.proof.as_deref() == Some(remote_proof(&admin_key(), &nonce).as_slice());
    if !ok {
        let _ = tx.send(NetEvent::RemoteLog(format!("Неверный пароль администратора от {ip} ({what})")));
        std::thread::sleep(Duration::from_secs(2));
    }
    Ok(ok)
}

/// Client side of `check_admin_password`: answer a Challenge if the host sends one.
pub fn answer_challenge(s: &mut TcpStream, cipher: &Cipher, first: Response, password: &str) -> io::Result<Response> {
    match first {
        Response::Challenge { nonce, .. } => {
            if password.is_empty() {
                return Err(other("Введите пароль администратора"));
            }
            write_frame(s, cipher, &RemoteAuth { proof: Some(remote_proof(&remote_key(password), &nonce)) })?;
            read_frame(s, cipher)
        }
        r => Ok(r),
    }
}

/// Host side. Called from the TCP server after `Request::RemoteHello`.
#[allow(clippy::too_many_arguments)]
pub fn serve_session(
    mut s: TcpStream,
    cipher: &Cipher,
    shared: &Arc<Mutex<Shared>>,
    tx: &Sender<NetEvent>,
    ctx: &egui::Context,
    from: u64,
    ip: IpAddr,
    view_only: bool,
) -> io::Result<()> {
    let _ = shared;
    if !check_admin_password(&mut s, cipher, tx, ip, if view_only { "просмотр экрана" } else { "управление" })? {
        return write_frame(&mut s, cipher, &Response::Denied("Неверный пароль администратора".into()));
    }
    write_frame(&mut s, cipher, &Response::Ok)?;

    let sid: u64 = rand::random();
    let stop = Arc::new(AtomicBool::new(false));
    let _ = tx.send(NetEvent::RemoteStarted { sid, from, ip, view_only, stop: stop.clone() });
    ctx.request_repaint();

    let settings = Arc::new(Mutex::new(Settings { quality: 60, fps: 10, monitor: 1, full: true }));
    // origin of the captured area, for mouse mapping
    let origin = Arc::new(Mutex::new((0i32, 0i32)));
    s.set_read_timeout(None)?;
    s.set_nodelay(true)?;
    {
        let mut r = s.try_clone()?;
        let (cipher, stop, settings, origin) = (cipher.clone(), stop.clone(), settings.clone(), origin.clone());
        std::thread::spawn(move || {
            while !stop.load(Ordering::Relaxed) {
                let Ok(m) = read_frame::<RemoteInput>(&mut r, &cipher) else { break };
                use crate::platform as p;
                match m {
                    RemoteInput::Close => break,
                    RemoteInput::Settings { quality, fps, monitor } => {
                        let mut st = settings.lock().unwrap();
                        if st.monitor != monitor {
                            st.full = true;
                        }
                        st.quality = quality.clamp(10, 95);
                        st.fps = fps.clamp(1, 30);
                        st.monitor = monitor;
                    }
                    RemoteInput::Full => settings.lock().unwrap().full = true,
                    _ if view_only => {}
                    RemoteInput::Mouse { x, y } => {
                        let o = *origin.lock().unwrap();
                        p::inject_mouse_move(o.0 + x, o.1 + y);
                    }
                    RemoteInput::Button { b, down } => p::inject_button(b, down),
                    RemoteInput::Wheel { dx, dy } => p::inject_wheel(dx, dy),
                    RemoteInput::Key { vk, down } => p::inject_key(vk, down),
                    RemoteInput::Text(t) => p::inject_text(&t),
                    RemoteInput::Clipboard(t) => {
                        let _ = arboard::Clipboard::new().and_then(|mut c| c.set_text(t));
                    }
                }
            }
            stop.store(true, Ordering::Relaxed);
        });
    }

    let mut prev: Option<image::RgbaImage> = None;
    let mut last_cursor = (i32::MIN, i32::MIN);
    let result = (|| -> io::Result<()> {
        while !stop.load(Ordering::Relaxed) {
            let t0 = Instant::now();
            let st = {
                let mut g = settings.lock().unwrap();
                let st = *g;
                g.full = false;
                st
            };
            let (img, ox, oy, monitors) = crate::platform::capture(st.monitor as usize).map_err(other)?;
            *origin.lock().unwrap() = (ox, oy);
            let (w, h) = (img.width(), img.height());
            let full = st.full || prev.as_ref().map_or(true, |p| p.dimensions() != (w, h));
            let mut tiles = Vec::new();
            let mut ty = 0;
            while ty < h {
                let th = TILE.min(h - ty);
                let mut tx_ = 0;
                while tx_ < w {
                    let tw = TILE.min(w - tx_);
                    if full || tile_changed(prev.as_ref().unwrap(), &img, tx_, ty, tw, th) {
                        tiles.push(Tile { x: tx_ as u16, y: ty as u16, w: tw as u16, h: th as u16, jpeg: encode_tile(&img, tx_, ty, tw, th, st.quality) });
                    }
                    tx_ += TILE;
                }
                ty += TILE;
            }
            let (cx, cy) = crate::platform::cursor_pos();
            let cursor = (cx - ox, cy - oy);
            if !tiles.is_empty() || cursor != last_cursor {
                last_cursor = cursor;
                write_frame(&mut s, cipher, &Response::Frame(RemoteFrame { w, h, monitors: monitors as u8, cursor, tiles }))?;
            }
            prev = Some(img);
            let period = Duration::from_millis(1000 / st.fps.max(1) as u64);
            if let Some(rest) = period.checked_sub(t0.elapsed()) {
                std::thread::sleep(rest);
            }
        }
        Ok(())
    })();
    stop.store(true, Ordering::Relaxed);
    let _ = s.shutdown(std::net::Shutdown::Both);
    let _ = tx.send(NetEvent::RemoteEnded { sid });
    ctx.request_repaint();
    result
}

// ------------------------------------------------------------------ viewer

#[derive(Default)]
pub struct ViewState {
    pub canvas: Option<image::RgbaImage>,
    pub dirty: bool,
    pub cursor: (i32, i32),
    pub monitors: u8,
    pub status: String,
    pub connected: bool,
    pub closed: bool,
    pub error: Option<String>,
    pub bytes: u64,
    pub frames: u64,
}

pub struct Viewer {
    pub id: u64,
    pub uid: u64,
    pub nick: String,
    pub state: Arc<Mutex<ViewState>>,
    pub tx: Sender<RemoteInput>,
    pub tex: Option<egui::TextureHandle>,
    pub view_only: bool,
    pub fit: bool,
    pub quality: u8,
    pub fps: u8,
    pub monitor: u8,
    pub mods: egui::Modifiers,
    pub open: bool,
    pub fullscreen: bool,
    pub last_mouse: Option<(i32, i32)>,
    pub stats_t: Instant,
    pub stats_bytes: u64,
    pub stats_frames: u64,
    pub rate: (f32, f32),
}

#[derive(Default)]
pub struct CmdState {
    pub open: bool,
    /// administrator password, checked by the host for every command
    pub password: String,
    pub input: String,
    pub output: String,
    pub busy: bool,
}

impl Viewer {
    #[allow(clippy::too_many_arguments)]
    pub fn connect(cipher: Cipher, my_uid: u64, uid: u64, nick: String, addr: SocketAddr, view_only: bool, password: String, ctx: egui::Context) -> Viewer {
        let state = Arc::new(Mutex::new(ViewState { status: "Подключение…".into(), ..Default::default() }));
        let (tx, rx) = channel::<RemoteInput>();
        {
            let state = state.clone();
            std::thread::spawn(move || {
                let r = run_viewer(&cipher, my_uid, addr, view_only, &password, &state, rx, &ctx);
                let mut st = state.lock().unwrap();
                st.closed = true;
                st.connected = false;
                if let Err(e) = r {
                    st.error = Some(e.to_string());
                    st.status = format!("Отключено: {e}");
                } else {
                    st.status = "Сеанс завершён".into();
                }
                ctx.request_repaint();
            });
        }
        Viewer {
            id: rand::random(),
            uid,
            nick,
            state,
            tx,
            tex: None,
            view_only,
            fit: true,
            quality: 60,
            fps: 10,
            monitor: 1,
            mods: egui::Modifiers::NONE,
            open: true,
            fullscreen: false,
            last_mouse: None,
            stats_t: Instant::now(),
            stats_bytes: 0,
            stats_frames: 0,
            rate: (0.0, 0.0),
        }
    }

    pub fn send(&self, m: RemoteInput) {
        let _ = self.tx.send(m);
    }

    pub fn send_settings(&self) {
        self.send(RemoteInput::Settings { quality: self.quality, fps: self.fps, monitor: self.monitor });
    }

    /// Press a key combination (e.g. Alt+Tab) on the remote side.
    pub fn combo(&self, vks: &[u16]) {
        for vk in vks {
            self.send(RemoteInput::Key { vk: *vk, down: true });
        }
        for vk in vks.iter().rev() {
            self.send(RemoteInput::Key { vk: *vk, down: false });
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn run_viewer(
    cipher: &Cipher,
    my_uid: u64,
    addr: SocketAddr,
    view_only: bool,
    password: &str,
    state: &Arc<Mutex<ViewState>>,
    rx: Receiver<RemoteInput>,
    ctx: &egui::Context,
) -> io::Result<()> {
    let mut s = TcpStream::connect_timeout(&addr, Duration::from_secs(5))?;
    s.set_nodelay(true)?;
    s.set_read_timeout(Some(Duration::from_secs(15)))?;
    write_frame(&mut s, cipher, &(my_uid, Request::RemoteHello { view_only }))?;
    let first = read_frame::<Response>(&mut s, cipher)?;
    {
        let mut st = state.lock().unwrap();
        st.status = "Проверка пароля…".into();
    }
    ctx.request_repaint();
    match answer_challenge(&mut s, cipher, first, password)? {
        Response::Ok => {}
        Response::Denied(m) | Response::Error(m) => return Err(other(m)),
        _ => return Err(other("неожиданный ответ")),
    }
    {
        let mut st = state.lock().unwrap();
        st.connected = true;
        st.status = "Подключено".into();
    }
    ctx.request_repaint();

    // writer: UI -> host
    let mut w = s.try_clone()?;
    let wcipher = cipher.clone();
    let done = Arc::new(AtomicBool::new(false));
    let done2 = done.clone();
    std::thread::spawn(move || {
        while let Ok(m) = rx.recv() {
            let close = matches!(m, RemoteInput::Close);
            if write_frame(&mut w, &wcipher, &m).is_err() || close {
                break;
            }
            if done2.load(Ordering::Relaxed) {
                break;
            }
        }
        let _ = w.shutdown(std::net::Shutdown::Both);
    });

    s.set_read_timeout(Some(Duration::from_secs(30)))?;
    let res = (|| -> io::Result<()> {
        loop {
            let mut len = [0u8; 4];
            std::io::Read::read_exact(&mut s, &mut len)?;
            let n = u32::from_le_bytes(len) as usize;
            if n > crate::net::MAX_FRAME {
                return Err(other("слишком большой кадр"));
            }
            let mut buf = vec![0u8; n];
            std::io::Read::read_exact(&mut s, &mut buf)?;
            let Some(resp) = cipher.open::<Response>(&buf) else { return Err(other("неверный кадр")) };
            let Response::Frame(f) = resp else { continue };
            // decode outside the lock
            let decoded: Vec<(Tile, Option<image::RgbImage>)> = f
                .tiles
                .into_iter()
                .map(|t| {
                    let img = image::load_from_memory_with_format(&t.jpeg, image::ImageFormat::Jpeg).ok().map(|i| i.to_rgb8());
                    (t, img)
                })
                .collect();
            let mut st = state.lock().unwrap();
            st.bytes += n as u64 + 4;
            st.frames += 1;
            st.monitors = f.monitors;
            st.cursor = f.cursor;
            let need_new = st.canvas.as_ref().map_or(true, |c| c.dimensions() != (f.w, f.h));
            if need_new {
                st.canvas = Some(image::RgbaImage::from_pixel(f.w, f.h, image::Rgba([0, 0, 0, 255])));
            }
            let canvas = st.canvas.as_mut().unwrap();
            for (t, img) in decoded {
                let Some(img) = img else { continue };
                for (x, y, p) in img.enumerate_pixels() {
                    let (cx, cy) = (t.x as u32 + x, t.y as u32 + y);
                    if cx < f.w && cy < f.h {
                        canvas.put_pixel(cx, cy, image::Rgba([p[0], p[1], p[2], 255]));
                    }
                }
            }
            st.dirty = true;
            drop(st);
            ctx.request_repaint();
        }
    })();
    done.store(true, Ordering::Relaxed);
    res
}

/// egui key -> Windows virtual-key code (by key name, layout independent).
pub fn vk_of(key: egui::Key) -> Option<u16> {
    let name = key.name();
    let vk = match name {
        "Down" | "ArrowDown" => 0x28,
        "Left" | "ArrowLeft" => 0x25,
        "Right" | "ArrowRight" => 0x27,
        "Up" | "ArrowUp" => 0x26,
        "Escape" | "Esc" => 0x1B,
        "Tab" => 0x09,
        "Backspace" => 0x08,
        "Enter" => 0x0D,
        "Space" => 0x20,
        "Insert" => 0x2D,
        "Delete" => 0x2E,
        "Home" => 0x24,
        "End" => 0x23,
        "PageUp" => 0x21,
        "PageDown" => 0x22,
        "Minus" | "-" => 0xBD,
        "Plus" | "+" | "Equals" | "=" => 0xBB,
        "Comma" | "," => 0xBC,
        "Period" | "." => 0xBE,
        "Semicolon" | ";" => 0xBA,
        "Slash" | "/" => 0xBF,
        "Backslash" | "\\" => 0xDC,
        "OpenBracket" | "[" => 0xDB,
        "CloseBracket" | "]" => 0xDD,
        "Backtick" | "`" => 0xC0,
        "Quote" | "'" => 0xDE,
        n if n.len() == 1 && n.as_bytes()[0].is_ascii_alphanumeric() => n.as_bytes()[0].to_ascii_uppercase() as u16,
        n if n.starts_with('F') && n[1..].parse::<u16>().is_ok() => {
            let k: u16 = n[1..].parse().unwrap();
            if (1..=24).contains(&k) {
                0x6F + k
            } else {
                return None;
            }
        }
        _ => return None,
    };
    Some(vk)
}

/// Keys that produce no text and must always be sent as virtual keys.
pub fn is_special(vk: u16) -> bool {
    matches!(vk, 0x08 | 0x09 | 0x0D | 0x1B | 0x21..=0x28 | 0x2D | 0x2E | 0x70..=0x87)
}

pub const VK_SHIFT: u16 = 0x10;
pub const VK_CONTROL: u16 = 0x11;
pub const VK_MENU: u16 = 0x12;
pub const VK_LWIN: u16 = 0x5B;
pub const VK_TAB: u16 = 0x09;
pub const VK_ESCAPE: u16 = 0x1B;

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    /// The viewer must not allocate whatever length the host announces.
    #[test]
    fn viewer_rejects_oversized_frame() {
        let cipher = Cipher::new("");
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let c2 = cipher.clone();
        std::thread::spawn(move || {
            let (mut s, _) = listener.accept().unwrap();
            let _: (u64, Request) = read_frame(&mut s, &c2).unwrap();
            write_frame(&mut s, &c2, &Response::Ok).unwrap();
            // announce a 300 MB frame, then hang up
            io::Write::write_all(&mut s, &(300u32 * 1024 * 1024).to_le_bytes()).unwrap();
        });
        let state = Arc::new(Mutex::new(ViewState::default()));
        let (_tx, rx) = channel();
        let err = run_viewer(&cipher, 1, addr, true, "", &state, rx, &egui::Context::default()).unwrap_err();
        assert!(err.to_string().contains("слишком большой"), "{err}");
    }
}
