use super::*;
use std::{
    collections::HashMap,
    net::TcpStream,
    sync::{Mutex, OnceLock},
    time::{Duration, Instant},
};
use tungstenite::{Message, WebSocket, stream::MaybeTlsStream};
struct Gateway {
    ws: WebSocket<MaybeTlsStream<TcpStream>>,
    heartbeat: Duration,
    last: Instant,
    ack: bool,
}
static SOCKETS: OnceLock<Mutex<HashMap<String, Gateway>>> = OnceLock::new();
fn sockets() -> &'static Mutex<HashMap<String, Gateway>> {
    SOCKETS.get_or_init(Default::default)
}
pub(super) fn close(id: &str) {
    if let Ok(mut sockets) = sockets().lock() {
        sockets.remove(id);
    }
}
fn send(g: &mut Gateway, v: Value) -> R<()> {
    g.ws.send(Message::Text(v.to_string().into()))
        .map_err(|_| "network: QQ gateway write interrupted".into())
}
fn connect(s: &mut Value) -> R<Gateway> {
    let data = qq::api(s, "/gateway", None)?;
    let u = net::trusted(field(&data, "url")?, "wss", &["sgroup.qq.com"])?;
    let (mut ws, _) = tungstenite::client::connect_with_config(
        u.as_str(),
        Some(
            tungstenite::protocol::WebSocketConfig::default()
                .max_message_size(Some(4 * 1024 * 1024))
                .max_frame_size(Some(4 * 1024 * 1024)),
        ),
        0,
    )
    .map_err(|_| "network: QQ gateway unavailable")?;
    let tcp = match ws.get_mut() {
        MaybeTlsStream::Plain(t) => t,
        MaybeTlsStream::Rustls(t) => &mut t.sock,
        _ => return Err("provider: unsupported QQ gateway transport".into()),
    };
    tcp.set_read_timeout(Some(Duration::from_millis(650)))
        .map_err(|_| "network: QQ socket timeout unavailable")?;
    tcp.set_write_timeout(Some(Duration::from_secs(5)))
        .map_err(|_| "network: QQ socket timeout unavailable")?;
    Ok(Gateway {
        ws,
        heartbeat: Duration::from_secs(36),
        last: Instant::now(),
        ack: true,
    })
}
fn event(c: &mut Client, s: &Value, t: &str, d: &Value) -> R<bool> {
    let (scene, peer, sender) = match t {
        "C2C_MESSAGE_CREATE" => {
            let p = field(&d["author"], "user_openid").or_else(|_| field(&d["author"], "id"))?;
            ("direct", p, p)
        }
        "GROUP_AT_MESSAGE_CREATE" => (
            "group",
            field(d, "group_openid")?,
            field(&d["author"], "member_openid").or_else(|_| field(&d["author"], "id"))?,
        ),
        _ => return Ok(false),
    };
    let body = d["content"].as_str().unwrap_or("").trim();
    if body.is_empty() {
        return Ok(false);
    }
    let out = adapter(
        c,
        s,
        "bot.receive",
        json!({"scene":scene,"peer":peer,"sender":sender,"remote":field(d,"id")?,"body":body}),
    )?;
    Ok(out["duplicate"] == false)
}
pub(super) fn poll(c: &mut Client, v: &vault::Vault, s: &mut Value) -> R<usize> {
    let id = field(s, "id")?.to_owned();
    let previous = sockets()
        .lock()
        .map_err(|_| "storage: QQ socket registry unavailable")?
        .remove(&id);
    let mut g = match previous {
        Some(g) => g,
        None => connect(s)?,
    };
    let mut n = 0;
    for _ in 0..64 {
        if g.last.elapsed() >= g.heartbeat {
            if !g.ack {
                return Err("network: QQ heartbeat acknowledgement expired".into());
            }
            send(&mut g, json!({"op":1,"d":s["seq"]}))?;
            g.ack = false;
            g.last = Instant::now();
        }
        let msg = match g.ws.read() {
            Ok(Message::Text(raw)) => {
                serde_json::from_str::<Value>(&raw).map_err(|_| "provider: invalid QQ event")?
            }
            Ok(Message::Close(_)) | Err(tungstenite::Error::ConnectionClosed) => return Ok(n),
            Ok(_) => continue,
            Err(tungstenite::Error::Io(e))
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
                ) =>
            {
                break;
            }
            Err(_) => return Err("network: QQ gateway disconnected".into()),
        };
        match msg["op"].as_i64() {
            Some(10) => {
                g.heartbeat = Duration::from_millis(
                    msg["d"]["heartbeat_interval"]
                        .as_u64()
                        .unwrap_or(36000)
                        .clamp(500, 120000),
                );
                let token = format!("QQBot {}", qq::token(s)?);
                if let (Some(session), Some(seq)) = (s["session"].as_str(), s["seq"].as_i64()) {
                    send(
                        &mut g,
                        json!({"op":6,"d":{"token":token,"session_id":session,"seq":seq}}),
                    )?;
                } else {
                    send(
                        &mut g,
                        json!({"op":2,"d":{"token":token,"intents":1<<25,"shard":[0,1],"properties":{"$os":std::env::consts::OS,"$browser":"ShipForge","$device":"ShipForge"}}}),
                    )?;
                }
            }
            Some(0) => {
                let name = field(&msg, "t")?;
                if name == "READY" {
                    s["session"] = json!(field(&msg["d"], "session_id")?);
                } else if event(c, s, name, &msg["d"])? {
                    n += 1;
                }
                if msg["s"].is_i64() {
                    s["seq"] = msg["s"].clone();
                }
                v.save(&id, s)?;
            }
            Some(1) => {
                send(&mut g, json!({"op":1,"d":s["seq"]}))?;
                g.last = Instant::now();
                g.ack = false;
            }
            Some(7) => return Ok(n),
            Some(9) => {
                if msg["d"] != true {
                    s["session"] = Value::Null;
                    s["seq"] = Value::Null;
                    v.save(&id, s)?;
                }
                return Ok(n);
            }
            Some(11) => g.ack = true,
            _ => {}
        }
    }
    sockets()
        .lock()
        .map_err(|_| "storage: QQ socket registry unavailable")?
        .insert(id, g);
    Ok(n)
}
