//! Remote-desktop host test: authenticates with a password, receives a few
//! frames (no input is sent), then runs a command through Request::Exec.
//!
//! cargo run --example rprobe -- <ip:tcp_port> <password>

#[path = "../src/proto.rs"]
#[allow(dead_code)]
mod proto;

use proto::*;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::{Duration, Instant};

fn wf<T: serde::Serialize>(s: &mut TcpStream, c: &Cipher, v: &T) {
    let d = c.seal(v);
    s.write_all(&(d.len() as u32).to_le_bytes()).unwrap();
    s.write_all(&d).unwrap();
}

fn rf<T: serde::de::DeserializeOwned>(s: &mut TcpStream, c: &Cipher) -> T {
    let mut l = [0u8; 4];
    s.read_exact(&mut l).unwrap();
    let mut b = vec![0u8; u32::from_le_bytes(l) as usize];
    s.read_exact(&mut b).unwrap();
    c.open(&b).expect("decrypt")
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let addr = a.get(1).expect("ip:port").clone();
    let pw = a.get(2).cloned().unwrap_or_default();
    let c = Cipher::new("");
    let uid: u64 = 0x5eed_0000_0000_0002;

    let mut s = TcpStream::connect(&addr).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(20))).unwrap();
    wf(&mut s, &c, &(uid, Request::RemoteHello { view_only: false }));
    let Response::Challenge { nonce, has_password, confirm } = rf::<Response>(&mut s, &c) else { panic!("no challenge") };
    println!("challenge: has_password={has_password} confirm={confirm}");
    let proof = Some(remote_proof(&remote_key(&pw), &nonce));
    wf(&mut s, &c, &RemoteAuth { proof });
    match rf::<Response>(&mut s, &c) {
        Response::Ok => println!("auth OK"),
        Response::Denied(m) => {
            println!("DENIED: {m}");
            return;
        }
        other => panic!("unexpected {:?}", std::mem::discriminant(&other)),
    }
    wf(&mut s, &c, &RemoteInput::Settings { quality: 50, fps: 5, monitor: 2 });
    let t = Instant::now();
    let mut frames = 0;
    let mut tiles = 0;
    let mut bytes = 0;
    while t.elapsed() < Duration::from_secs(3) {
        if let Response::Frame(f) = rf::<Response>(&mut s, &c) {
            frames += 1;
            tiles += f.tiles.len();
            bytes += f.tiles.iter().map(|t| t.jpeg.len()).sum::<usize>();
            if frames == 1 {
                println!("first frame: {}x{} monitors={} tiles={} cursor={:?}", f.w, f.h, f.monitors, f.tiles.len(), f.cursor);
            }
        }
    }
    println!("{frames} frames, {tiles} tiles, {} KB in 3 s", bytes / 1024);
    wf(&mut s, &c, &RemoteInput::Close);
    drop(s);

    let mut s = TcpStream::connect(&addr).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(70))).unwrap();
    wf(&mut s, &c, &(uid, Request::Exec("whoami & hostname".into())));
    let mut r = rf::<Response>(&mut s, &c);
    if let Response::Challenge { nonce, .. } = &r {
        wf(&mut s, &c, &RemoteAuth { proof: Some(remote_proof(&remote_key(&pw), nonce)) });
        r = rf::<Response>(&mut s, &c);
    }
    match r {
        Response::Text(t) => println!("exec output:\n{}", t.trim()),
        Response::Denied(m) | Response::Error(m) => println!("exec denied: {m}"),
        _ => println!("exec: unexpected"),
    }
}
