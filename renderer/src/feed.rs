//! Polling glitchwall's feed, the same way the page does: GET /feed?since=&key=&screen= once a second.
//! One thread per screen; it keeps the page's rules for `since` and hands each reply to the drawing loop.
use crate::json::Json;
use std::io::{Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpStream};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::Duration;

/// one reply, with the events this screen should show (already trimmed by the page's rules)
pub struct Reply { pub data: Json, pub events: Vec<Json> }

pub enum Msg { Reply(Reply), Down }

pub fn start(port: u16, key: String, screen: u32, awake: bool) -> Receiver<Msg> {
    let (tx, rx) = channel();
    std::thread::spawn(move || run(port, key, screen, awake, tx));
    rx
}

fn run(port: u16, key: String, screen: u32, awake: bool, tx: Sender<Msg>) {
    let mut since: i64 = 0;
    loop {
        let msg = match get(port, &format!("/feed?since={since}&key={}&screen={screen}", enc(&key))) {
            Some(data) => {
                let mut evs: Vec<Json> = data.get("events").items().to_vec();
                let last = data.get("last").num().unwrap_or(0.0) as i64;
                if last < since { since = 0; }                       // the feed restarted: start over
                let sleeping = data.get("sleep").truthy() && !awake;
                let hidden = data.get("paused").truthy() || data.get("mode").str() == Some("art");
                if (sleeping || hidden) && !evs.is_empty() {         // nobody sees them: skip, don't pile up
                    since = id(&evs[evs.len() - 1]); evs.clear();
                }
                if !evs.is_empty() {
                    if since == 0 && evs.len() > 6 { evs.drain(..evs.len() - 6); }   // on start, replay just the last few
                    since = id(&evs[evs.len() - 1]);
                }
                Msg::Reply(Reply { data, events: evs })
            }
            None => Msg::Down,
        };
        if tx.send(msg).is_err() { return; }   // the screen went away
        std::thread::sleep(Duration::from_secs(1));
    }
}

fn id(e: &Json) -> i64 { e.get("id").num().unwrap_or(0.0) as i64 }

fn enc(s: &str) -> String {
    s.bytes().map(|b| if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") }).collect()
}

/// a plain HTTP/1.0 GET to the local feed; None if it isn't there or refuses
fn get(port: u16, path: &str) -> Option<Json> {
    let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
    let mut s = TcpStream::connect_timeout(&addr, Duration::from_secs(2)).ok()?;
    s.set_read_timeout(Some(Duration::from_secs(5))).ok()?;
    write!(s, "GET {path} HTTP/1.0\r\nHost: 127.0.0.1:{port}\r\nUser-Agent: glitchwall-render\r\n\r\n").ok()?;
    let mut buf = Vec::new();
    s.read_to_end(&mut buf).ok()?;
    let text = String::from_utf8_lossy(&buf);
    let (head, body) = text.split_once("\r\n\r\n")?;
    if !head.starts_with("HTTP/1.0 200") && !head.starts_with("HTTP/1.1 200") { return None; }
    Json::parse(body).ok()
}
