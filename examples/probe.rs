//! Network probe for testing: announces itself, lists peers and sends a private
//! message to a peer uid, waiting for the delivery acknowledgement.
//!
//! cargo run --example probe -- [target_uid_hex] [port]

#[path = "../src/proto.rs"]
#[allow(dead_code)]
mod proto;

use proto::*;
use std::net::{Ipv4Addr, SocketAddr, UdpSocket};
use std::time::{Duration, Instant};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let target = args.get(1).and_then(|s| u64::from_str_radix(s, 16).ok());
    let port: u16 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(50240);
    let cipher = Cipher::new("");
    let uid: u64 = 0x5eed_0000_0000_0001;
    // everything is addressed to the shared port, so listen on it (with reuse)
    let sock = socket2::Socket::new(socket2::Domain::IPV4, socket2::Type::DGRAM, None).unwrap();
    sock.set_reuse_address(true).unwrap();
    sock.set_broadcast(true).unwrap();
    sock.bind(&SocketAddr::from((Ipv4Addr::UNSPECIFIED, port)).into()).unwrap();
    let s: UdpSocket = sock.into();
    s.set_read_timeout(Some(Duration::from_millis(200))).unwrap();
    let bcast = SocketAddr::from((Ipv4Addr::BROADCAST, port));
    let me = Presence { nick: "probe".into(), computer: "PROBE".into(), version: "probe".into(), ..Default::default() };
    s.send_to(&cipher.seal(&Envelope { from: uid, to: None, body: Body::Hello(me.clone()) }), bcast).unwrap();

    let mut sent_at = None;
    let msg_id = 0xabcdef;
    let start = Instant::now();
    let mut buf = vec![0u8; 65536];
    while start.elapsed() < Duration::from_secs(12) {
        if let (Some(t), None) = (target, sent_at) {
            if start.elapsed() > Duration::from_millis(800) {
                let env = Envelope { from: uid, to: Some(t), body: Body::Private { id: msg_id, text: "probe message".into(), sent: now_unix() } };
                // the target may be on this same PC (shared port) -> addressed broadcast
                s.send_to(&cipher.seal(&env), bcast).unwrap();
                sent_at = Some(Instant::now());
                println!("sent private message to {t:x}");
            }
        }
        let Ok((n, src)) = s.recv_from(&mut buf) else { continue };
        let Some(env) = cipher.open::<Envelope>(&buf[..n]) else { continue };
        if env.from == uid {
            continue;
        }
        match env.body {
            Body::Presence(p) | Body::Hello(p) => println!("peer {:x} {} @ {} [{}] win_state={}", env.from, p.nick, src, p.state_title, p.win_state),
            Body::PrivateAck(id) if id == msg_id => {
                println!("ACK from {:x} after {:?}", env.from, sent_at.map(|t| t.elapsed()));
                break;
            }
            other => println!("{:x}: {:?}", env.from, std::mem::discriminant(&other)),
        }
    }
    let _ = s.send_to(&cipher.seal(&Envelope { from: uid, to: None, body: Body::Bye }), bcast);
}
