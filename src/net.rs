//! Networking: ONE port. UDP broadcast to 255.255.255.255:<port> for
//! everything addressed to all, UDP unicast to <peer ip>:<port> for private
//! traffic, and a TCP listener on the same port number for bulky data
//! (files, screenshots, user details). The socket is bound to one fixed
//! interface IP chosen in the settings.

use crate::proto::*;
use socket2::{Domain, Protocol, Socket, Type};
use std::collections::HashMap;
use std::io::{self, Read, Write};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, SocketAddrV4, TcpListener, TcpStream, UdpSocket};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub const DEFAULT_PORT: u16 = 50240;

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(default)]
pub struct ConnCfg {
    /// Fixed interface IP. Chosen once (auto-detected on first start) and kept.
    pub bind_ip: String,
    pub port: u16,
    pub network_password: String,
}

impl Default for ConnCfg {
    fn default() -> Self {
        ConnCfg { bind_ip: String::new(), port: DEFAULT_PORT, network_password: String::new() }
    }
}

pub enum NetEvent {
    Packet { env: Envelope, src: SocketAddr },
    Reply { req_id: u64, result: Result<Response, String> },
    Served { from: u64, ip: IpAddr, what: &'static str },
    Transfer { id: u64, done: u64, total: u64, finished: bool, error: Option<String> },
    RemoteStarted { sid: u64, from: u64, ip: IpAddr, view_only: bool, stop: Arc<AtomicBool> },
    RemoteEnded { sid: u64 },
    RemoteLog(String),
}

#[derive(Default)]
pub struct Shared {
    pub allow: Allow,
    pub details: UserDetails,
    /// offered file id -> (path, recipient uid)
    pub files: HashMap<u64, (PathBuf, u64)>,
    pub offline: bool,
}

pub struct Net {
    pub uid: u64,
    pub(crate) cipher: Cipher,
    sock: UdpSocket,
    pub port: u16,
    pub tcp_port: u16,
    pub local_ip: Ipv4Addr,
    pub bound_ip: Ipv4Addr,
    tx: Sender<NetEvent>,
    ctx: eframe::egui::Context,
}

/// All IPv4 interfaces: (adapter name, ip).
pub fn interfaces() -> Vec<(String, Ipv4Addr)> {
    let mut v = Vec::new();
    if let Ok(list) = if_addrs::get_if_addrs() {
        for itf in list {
            if let IpAddr::V4(ip) = itf.ip() {
                if !ip.is_loopback() {
                    v.push((itf.name.clone(), ip));
                }
            }
        }
    }
    v.sort_by_key(|(_, ip)| *ip);
    v.dedup_by_key(|(_, ip)| *ip);
    v
}

/// The address Windows would send from to reach the outside world, i.e. the adapter with
/// the default route. connect() on UDP sends nothing, it only picks the route.
fn route_ip() -> Option<Ipv4Addr> {
    let s = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).ok()?;
    s.connect((Ipv4Addr::new(8, 8, 8, 8), 53)).ok()?;
    match s.local_addr().ok()?.ip() {
        IpAddr::V4(ip) if !ip.is_unspecified() => Some(ip),
        _ => None,
    }
}

fn score(name: &str, ip: Ipv4Addr, route: Option<Ipv4Addr>) -> i32 {
    if ip.is_link_local() || ip.is_unspecified() {
        return -1000;
    }
    let name = name.to_lowercase();
    let mut s = 0;
    for bad in [
        "vpn", "tun", "tap", "xray", "wg", "wireguard", "virtual", "vethernet", "vmware", "vbox", "hyper-v",
        "bluetooth", "zerotier", "tailscale", "happ", "clash", "sing", "hamachi", "radmin", "openvpn",
    ] {
        if name.contains(bad) {
            s -= 100;
        }
    }
    s += match ip.octets() {
        [192, 168, ..] => 30,
        [10, ..] => 20,
        [172, b, ..] if (16..32).contains(&b) => 10,
        _ => 0,
    };
    if name.contains("ethernet") || name.starts_with("eth") {
        s += 5;
    }
    if name.contains("wi-fi") || name.contains("wlan") || name.contains("wireless") {
        s += 4;
    }
    // VirtualBox host-only adapter: its default network, and Windows calls it just "Ethernet 3"
    if ip.octets()[..3] == [192, 168, 56] {
        s -= 50;
    }
    // the adapter with the default route is the real LAN (unless it is a VPN, see above)
    if route == Some(ip) {
        s += 15;
    }
    s
}

/// Best physical LAN adapter (skips VPN / virtual / link-local).
pub fn detect_local_ip() -> Option<Ipv4Addr> {
    best_ip(interfaces(), route_ip())
}

fn best_ip(list: Vec<(String, Ipv4Addr)>, route: Option<Ipv4Addr>) -> Option<Ipv4Addr> {
    list.into_iter().max_by_key(|(n, ip)| score(n, *ip, route)).map(|(_, ip)| ip)
}

#[cfg(test)]
mod iface_tests {
    use super::*;

    #[test]
    fn real_lan_wins_over_virtualbox_and_vpn() {
        let list = || {
            vec![
                ("Ethernet".to_string(), Ipv4Addr::new(192, 168, 1, 62)),
                ("Ethernet 3".to_string(), Ipv4Addr::new(192, 168, 56, 1)), // VirtualBox Host-Only
                ("happ-xray".to_string(), Ipv4Addr::new(172, 19, 0, 1)),
                ("Bluetooth Network Connection".to_string(), Ipv4Addr::new(169, 254, 130, 33)),
            ]
        };
        let lan = Some(Ipv4Addr::new(192, 168, 1, 62));
        // VPN holds the default route, no route known, normal default route
        for route in [Some(Ipv4Addr::new(172, 19, 0, 1)), None, lan] {
            assert_eq!(best_ip(list(), route), lan, "route {route:?}");
        }
        assert_eq!(detect_local_ip().is_some(), !interfaces().is_empty());
    }
}

fn udp_socket(ip: Ipv4Addr, port: u16) -> io::Result<Socket> {
    let s = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))?;
    s.set_reuse_address(true)?;
    s.set_broadcast(true)?;
    s.bind(&SocketAddr::from((ip, port)).into())?;
    Ok(s)
}

impl Net {
    /// Returns the network, the event receiver and warnings for the event log.
    pub fn start(
        cfg: &ConnCfg,
        uid: u64,
        ctx: eframe::egui::Context,
        shared: Arc<Mutex<Shared>>,
    ) -> io::Result<(Net, Receiver<NetEvent>, Vec<String>)> {
        let (tx, rx) = channel();
        let mut warn = Vec::new();
        let cipher = Cipher::new(&cfg.network_password);
        let port = if cfg.port == 0 { DEFAULT_PORT } else { cfg.port };

        let wanted: Option<Ipv4Addr> = cfg.bind_ip.trim().parse().ok();
        let have = interfaces();
        let mut bind_ip = Ipv4Addr::UNSPECIFIED;
        match wanted {
            Some(ip) if have.iter().any(|(_, a)| *a == ip) => bind_ip = ip,
            Some(ip) => warn.push(format!(
                "Интерфейс {ip} из настроек сейчас отсутствует — слушаю на всех интерфейсах (настройка сохранена)"
            )),
            None => warn.push("Интерфейс не выбран — слушаю на всех интерфейсах".into()),
        }

        let sock = match udp_socket(bind_ip, port) {
            Ok(s) => s,
            Err(e) if !bind_ip.is_unspecified() => {
                warn.push(format!("Не удалось привязаться к {bind_ip}:{port} ({e}) — использую 0.0.0.0"));
                bind_ip = Ipv4Addr::UNSPECIFIED;
                udp_socket(bind_ip, port)?
            }
            Err(e) => return Err(e),
        };
        let sock: UdpSocket = sock.into();
        let local_ip = if bind_ip.is_unspecified() { wanted.or_else(detect_local_ip).unwrap_or(Ipv4Addr::LOCALHOST) } else { bind_ip };

        {
            let s = sock.try_clone()?;
            let tx = tx.clone();
            let cipher = cipher.clone();
            let ctx = ctx.clone();
            std::thread::Builder::new().name("udp".into()).spawn(move || {
                let mut buf = vec![0u8; 65536];
                loop {
                    match s.recv_from(&mut buf) {
                        Ok((n, src)) => {
                            let Some(env) = cipher.open::<Envelope>(&buf[..n]) else { continue };
                            if env.from == uid || env.to.is_some_and(|t| t != uid) {
                                continue;
                            }
                            if tx.send(NetEvent::Packet { env, src }).is_err() {
                                break;
                            }
                            ctx.request_repaint();
                        }
                        Err(_) => std::thread::sleep(Duration::from_millis(20)),
                    }
                }
            })?;
        }

        let listener = match TcpListener::bind((bind_ip, port)) {
            Ok(l) => l,
            Err(_) => {
                // a second copy on the same PC: the port is taken, use any free one
                TcpListener::bind((bind_ip, 0))?
            }
        };
        let tcp_port = listener.local_addr()?.port();
        {
            let tx = tx.clone();
            let cipher = cipher.clone();
            let ctx = ctx.clone();
            std::thread::Builder::new().name("tcp".into()).spawn(move || {
                for stream in listener.incoming().flatten() {
                    let (tx, cipher, shared, ctx) = (tx.clone(), cipher.clone(), shared.clone(), ctx.clone());
                    std::thread::spawn(move || {
                        let _ = serve(stream, &cipher, &shared, &tx, &ctx);
                        ctx.request_repaint();
                    });
                }
            })?;
        }

        Ok((Net { uid, cipher, sock, port, tcp_port, local_ip, bound_ip: bind_ip, tx, ctx }, rx, warn))
    }

    fn send_raw(&self, env: &Envelope, to: SocketAddr) {
        let data = self.cipher.seal(env);
        if data.len() <= MAX_DATAGRAM {
            let _ = self.sock.send_to(&data, to);
        }
    }

    fn bcast_addr(&self) -> SocketAddr {
        SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::BROADCAST, self.port))
    }

    /// To everybody: 255.255.255.255:<port>.
    pub fn broadcast(&self, body: Body) {
        self.send_raw(&Envelope { from: self.uid, to: None, body }, self.bcast_addr());
    }

    /// To one peer. Peers on this same PC share the port, so they get an
    /// addressed broadcast instead of unicast.
    pub fn send(&self, to_uid: u64, ip: IpAddr, body: Body) {
        let env = Envelope { from: self.uid, to: Some(to_uid), body };
        let same_pc = ip.is_loopback() || ip == IpAddr::V4(self.local_ip);
        if same_pc {
            self.send_raw(&env, self.bcast_addr());
        } else {
            self.send_raw(&env, SocketAddr::new(ip, self.port));
        }
    }

    /// Like `request`, but answers the host's admin-password challenge.
    pub fn request_auth(&self, req_id: u64, addr: SocketAddr, req: Request, password: String) {
        let (tx, cipher, uid, ctx) = (self.tx.clone(), self.cipher.clone(), self.uid, self.ctx.clone());
        std::thread::spawn(move || {
            let result = (|| -> io::Result<Response> {
                let mut s = TcpStream::connect_timeout(&addr, Duration::from_secs(5))?;
                s.set_read_timeout(Some(Duration::from_secs(75)))?;
                write_frame(&mut s, &cipher, &(uid, req))?;
                let first: Response = read_frame(&mut s, &cipher)?;
                crate::remote::answer_challenge(&mut s, &cipher, first, &password)
            })()
            .map_err(|e| e.to_string());
            let _ = tx.send(NetEvent::Reply { req_id, result });
            ctx.request_repaint();
        });
    }

    pub fn request(&self, req_id: u64, addr: SocketAddr, req: Request) {
        let (tx, cipher, uid, ctx) = (self.tx.clone(), self.cipher.clone(), self.uid, self.ctx.clone());
        std::thread::spawn(move || {
            let result = request_blocking(&cipher, uid, addr, &req).map_err(|e| e.to_string());
            let _ = tx.send(NetEvent::Reply { req_id, result });
            ctx.request_repaint();
        });
    }

    pub fn download(&self, id: u64, addr: SocketAddr, dest: PathBuf, cancel: Arc<AtomicBool>) {
        let (tx, cipher, uid, ctx) = (self.tx.clone(), self.cipher.clone(), self.uid, self.ctx.clone());
        std::thread::spawn(move || {
            let r = download_blocking(&cipher, uid, addr, id, &dest, &tx, &ctx, &cancel);
            let _ = tx.send(NetEvent::Transfer { id, done: 0, total: 0, finished: true, error: r.err().map(|e| e.to_string()) });
            ctx.request_repaint();
        });
    }
}

pub(crate) const MAX_FRAME: usize = 256 * 1024 * 1024;

pub(crate) fn write_frame<T: serde::Serialize>(w: &mut impl Write, cipher: &Cipher, v: &T) -> io::Result<()> {
    let data = cipher.seal(v);
    w.write_all(&(data.len() as u32).to_le_bytes())?;
    w.write_all(&data)
}

pub(crate) fn read_frame<T: serde::de::DeserializeOwned>(r: &mut impl Read, cipher: &Cipher) -> io::Result<T> {
    let mut len = [0u8; 4];
    r.read_exact(&mut len)?;
    let len = u32::from_le_bytes(len) as usize;
    if len > MAX_FRAME {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "frame too large"));
    }
    let mut buf = vec![0u8; len];
    r.read_exact(&mut buf)?;
    cipher.open(&buf).ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "неверный кадр (другой сетевой пароль?)"))
}

fn other(m: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::Other, m.into())
}

pub fn request_blocking(cipher: &Cipher, uid: u64, addr: SocketAddr, req: &Request) -> io::Result<Response> {
    let mut s = TcpStream::connect_timeout(&addr, Duration::from_secs(5))?;
    s.set_read_timeout(Some(Duration::from_secs(30)))?;
    write_frame(&mut s, cipher, &(uid, req.clone()))?;
    read_frame(&mut s, cipher)
}

pub fn part_path(dest: &std::path::Path) -> PathBuf {
    let mut p = dest.as_os_str().to_owned();
    p.push(".part");
    PathBuf::from(p)
}

#[allow(clippy::too_many_arguments)]
fn download_blocking(
    cipher: &Cipher,
    uid: u64,
    addr: SocketAddr,
    id: u64,
    dest: &std::path::Path,
    tx: &Sender<NetEvent>,
    ctx: &eframe::egui::Context,
    cancel: &AtomicBool,
) -> io::Result<()> {
    let part = part_path(dest);
    let offset = std::fs::metadata(&part).map(|m| m.len()).unwrap_or(0);
    let mut s = TcpStream::connect_timeout(&addr, Duration::from_secs(5))?;
    s.set_read_timeout(Some(Duration::from_secs(60)))?;
    write_frame(&mut s, cipher, &(uid, Request::File { id, offset }))?;
    let (size, offset) = match read_frame::<Response>(&mut s, cipher)? {
        Response::FileStart { size, offset } => (size, offset),
        Response::Denied(m) | Response::Error(m) => return Err(other(m)),
        _ => return Err(other("неожиданный ответ")),
    };
    let mut f = std::fs::OpenOptions::new().create(true).write(true).truncate(false).open(&part)?;
    f.set_len(offset)?;
    std::io::Seek::seek(&mut f, std::io::SeekFrom::Start(offset))?;
    let mut done = offset;
    let mut last = std::time::Instant::now();
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err(other("Отменено (можно докачать позже)"));
        }
        match read_frame::<Response>(&mut s, cipher)? {
            Response::Chunk(c) if c.is_empty() => break,
            Response::Chunk(c) => {
                f.write_all(&c)?;
                done += c.len() as u64;
                if last.elapsed() > Duration::from_millis(150) {
                    last = std::time::Instant::now();
                    let _ = tx.send(NetEvent::Transfer { id, done, total: size, finished: false, error: None });
                    ctx.request_repaint();
                }
            }
            Response::Error(m) => return Err(other(m)),
            _ => return Err(other("неожиданный ответ")),
        }
    }
    drop(f);
    if done != size {
        return Err(other("Передача прервана — можно докачать"));
    }
    let _ = std::fs::remove_file(dest);
    std::fs::rename(&part, dest)?;
    let _ = tx.send(NetEvent::Transfer { id, done, total: size, finished: false, error: None });
    Ok(())
}

fn serve(mut s: TcpStream, cipher: &Cipher, shared: &Arc<Mutex<Shared>>, tx: &Sender<NetEvent>, ctx: &eframe::egui::Context) -> io::Result<()> {
    s.set_read_timeout(Some(Duration::from_secs(15)))?;
    let ip = s.peer_addr()?.ip();
    let (from, req): (u64, Request) = read_frame(&mut s, cipher)?;
    let (allow, offline) = {
        let sh = shared.lock().unwrap();
        (sh.allow.clone(), sh.offline)
    };
    let deny = |s: &mut TcpStream, m: &str| write_frame(s, cipher, &Response::Denied(m.into()));
    if offline {
        return deny(&mut s, "Пользователь не в сети");
    }
    let resp = match req {
        Request::RemoteHello { view_only } => {
            return crate::remote::serve_session(s, cipher, shared, tx, ctx, from, ip, view_only);
        }
        Request::Exec(cmd) => {
            if !crate::remote::check_admin_password(&mut s, cipher, tx, ip, "командная строка")? {
                return deny(&mut s, "Неверный пароль администратора");
            }
            let _ = tx.send(NetEvent::RemoteLog(format!("Выполнение команды от {ip}: {cmd}")));
            Response::Text(crate::platform::exec(&cmd, Duration::from_secs(60)))
        }
        Request::Power(a) => {
            if !crate::remote::check_admin_password(&mut s, cipher, tx, ip, "питание")? {
                return deny(&mut s, "Неверный пароль администратора");
            }
            let what = ["выключение", "перезагрузка", "выход из системы", "блокировка"].get(a as usize).copied().unwrap_or("?");
            let _ = tx.send(NetEvent::RemoteLog(format!("Команда питания от {ip}: {what}")));
            match crate::platform::power(a) {
                Ok(()) => Response::Ok,
                Err(e) => Response::Error(e),
            }
        }
        Request::Details => Response::Details(shared.lock().unwrap().details.clone()),
        Request::Screenshot { monitor, jpeg_quality } => {
            if !allow.screenshot {
                return deny(&mut s, "Просмотр экрана запрещён удалённым пользователем");
            }
            let _ = tx.send(NetEvent::Served { from, ip, what: "снимок экрана" });
            match crate::platform::screenshot(monitor as usize) {
                Ok((img, monitors)) => {
                    let (w, h) = (img.width(), img.height());
                    let rgb = image::DynamicImage::ImageRgba8(img).to_rgb8();
                    let mut data = Vec::new();
                    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut data, jpeg_quality.clamp(10, 100))
                        .encode_image(&rgb)
                        .map_err(|e| other(e.to_string()))?;
                    Response::Image { data, w, h, monitors: monitors as u8 }
                }
                Err(e) => Response::Error(e),
            }
        }
        Request::Processes => {
            if !allow.processes {
                return deny(&mut s, "Просмотр процессов запрещён удалённым пользователем");
            }
            let _ = tx.send(NetEvent::Served { from, ip, what: "список процессов" });
            Response::Processes(crate::platform::processes())
        }
        Request::Kill(pid) => {
            if !allow.processes || !allow.remote_kill {
                return deny(&mut s, "Удалённое завершение процессов запрещено");
            }
            let _ = tx.send(NetEvent::Served { from, ip, what: "завершение процесса" });
            match crate::platform::kill(pid) {
                Ok(()) => Response::Ok,
                Err(e) => Response::Error(e),
            }
        }
        Request::Clipboard => {
            if !allow.clipboard {
                return deny(&mut s, "Просмотр буфера обмена запрещён удалённым пользователем");
            }
            let _ = tx.send(NetEvent::Served { from, ip, what: "буфер обмена" });
            Response::Text(arboard::Clipboard::new().and_then(|mut c| c.get_text()).unwrap_or_default())
        }
        Request::File { id, offset } => {
            let entry = shared.lock().unwrap().files.get(&id).cloned();
            let Some((path, to)) = entry else {
                return write_frame(&mut s, cipher, &Response::Error("Файл больше не предлагается".into()));
            };
            if to != from {
                return deny(&mut s, "Файл предназначен другому пользователю");
            }
            let mut f = std::fs::File::open(&path)?;
            let size = f.metadata()?.len();
            // a .part longer than the file belongs to another file: start over
            let offset = if offset > size { 0 } else { offset };
            std::io::Seek::seek(&mut f, std::io::SeekFrom::Start(offset))?;
            write_frame(&mut s, cipher, &Response::FileStart { size, offset })?;
            let mut buf = vec![0u8; 256 * 1024];
            let mut done = offset;
            let mut last = std::time::Instant::now();
            loop {
                let n = f.read(&mut buf)?;
                write_frame(&mut s, cipher, &Response::Chunk(buf[..n].to_vec()))?;
                if n == 0 {
                    break;
                }
                done += n as u64;
                if last.elapsed() > Duration::from_millis(200) {
                    last = std::time::Instant::now();
                    let _ = tx.send(NetEvent::Transfer { id, done, total: size, finished: false, error: None });
                }
            }
            let _ = tx.send(NetEvent::Transfer { id, done, total: size, finished: true, error: None });
            return Ok(());
        }
    };
    write_frame(&mut s, cipher, &resp)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A leftover .part of a *different* (longer) file with the same name must not
    /// produce a "successfully received" file made of the old bytes.
    #[test]
    fn resume_with_foreign_longer_part_restarts_from_zero() {
        let dir = std::env::temp_dir().join(format!("nassi-test-{:x}", rand::random::<u64>()));
        std::fs::create_dir_all(&dir).unwrap();
        let src = dir.join("src.txt");
        std::fs::write(&src, b"hello world").unwrap();
        let dest = dir.join("dest.txt");
        std::fs::write(part_path(&dest), b"XXXXXXXXXXXXXXXXXXXXXXXX").unwrap();

        let me = 1u64;
        let cipher = Cipher::new("");
        let shared = Arc::new(Mutex::new(Shared::default()));
        shared.lock().unwrap().files.insert(7, (src.clone(), me));
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let (tx, _rx) = channel();
        let ctx = eframe::egui::Context::default();
        {
            let (cipher, shared, tx, ctx) = (cipher.clone(), shared.clone(), tx.clone(), ctx.clone());
            std::thread::spawn(move || {
                let (s, _) = listener.accept().unwrap();
                let _ = serve(s, &cipher, &shared, &tx, &ctx);
            });
        }
        let r = download_blocking(&cipher, me, addr, 7, &dest, &tx, &ctx, &AtomicBool::new(false));
        let got = std::fs::read(&dest);
        let _ = std::fs::remove_dir_all(&dir);
        r.unwrap();
        assert_eq!(got.unwrap(), b"hello world");
    }
}
