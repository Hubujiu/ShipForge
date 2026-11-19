use super::*;
use prost::Message as Proto;
use std::{
    collections::HashMap,
    net::TcpStream,
    sync::{Mutex, OnceLock},
    time::{Duration, Instant},
};
use tungstenite::{Message, WebSocket, stream::MaybeTlsStream};

#[derive(Clone, PartialEq, Proto)]
struct Header {
    #[prost(string, required, tag = "1")]
    key: String,
    #[prost(string, required, tag = "2")]
    value: String,
}
#[derive(Clone, PartialEq, Proto)]
struct Frame {
    #[prost(uint64, required, tag = "1")]
    seq_id: u64,
    #[prost(uint64, required, tag = "2")]
    log_id: u64,
    #[prost(int32, required, tag = "3")]
    service: i32,
    #[prost(int32, required, tag = "4")]
    method: i32,
    #[prost(message, repeated, tag = "5")]
    headers: Vec<Header>,
    #[prost(string, optional, tag = "6")]
    payload_encoding: Option<String>,
    #[prost(string, optional, tag = "7")]
    payload_type: Option<String>,
    #[prost(bytes = "vec", optional, tag = "8")]
    payload: Option<Vec<u8>>,
    #[prost(string, optional, tag = "9")]
    log_id_new: Option<String>,
}
impl Frame {
    fn get(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|h| h.key == name)
            .map(|h| h.value.as_str())
    }
}
struct Pieces {
    at: Instant,
    bytes: usize,
    parts: Vec<Option<Vec<u8>>>,
}
struct Gateway {
    ws: WebSocket<MaybeTlsStream<TcpStream>>,
    service: i32,
    ping: Duration,
    last: Instant,
    awaiting: bool,
    pieces: HashMap<String, Pieces>,
}
static SOCKETS: OnceLock<Mutex<HashMap<String, Gateway>>> = OnceLock::new();
fn sockets() -> &'static Mutex<HashMap<String, Gateway>> {
    SOCKETS.get_or_init(Default::default)
}
pub(in crate::bots) fn close(id: &str) {
    if let Ok(mut map) = sockets().lock() {
        map.remove(id);
    }
}
fn write(g: &mut Gateway, frame: &Frame) -> R<()> {
    g.ws.send(Message::Binary(frame.encode_to_vec().into()))
        .map_err(|_| "network: Lark gateway write interrupted".into())
}
fn ping(g: &mut Gateway) -> R<()> {
    let f = Frame {
        service: g.service,
        method: 0,
        headers: vec![Header {
            key: "type".into(),
            value: "ping".into(),
        }],
        ..Default::default()
    };
    write(g, &f)?;
    g.last = Instant::now();
    g.awaiting = true;
    Ok(())
}
fn interval(cfg: &Value) -> Duration {
    Duration::from_secs(cfg["PingInterval"].as_u64().unwrap_or(90).clamp(1, 3600))
}
fn connect(s: &Value) -> R<Gateway> {
    let brand = field(s, "provider")?;
    let d = net::request(
        &url(brand, "/callback/ws/endpoint")?,
        Some(json!({"AppID":field(s,"account")?,"AppSecret":field(s,"secret")?})),
        &[("locale", "zh".into())],
    )?;
    if d["code"] == 514 {
        return Err("provider: Lark application revoked or invalid".into());
    }
    if let Some(code) = d["code"].as_i64().filter(|c| *c != 0) {
        return Err(format!(
            "provider: Lark gateway refused connection ({code})"
        ));
    }
    let addr = safe(brand, field(&d["data"], "URL")?, "wss")?;
    let service = addr
        .query_pairs()
        .find(|(key, _)| key == "service_id")
        .and_then(|(_, s)| s.parse::<i32>().ok())
        .ok_or("provider: missing gateway service identity")?;
    let limits = tungstenite::protocol::WebSocketConfig::default()
        .max_message_size(Some(4 * 1024 * 1024))
        .max_frame_size(Some(4 * 1024 * 1024));
    let (mut ws, _) = tungstenite::client::connect_with_config(addr.as_str(), Some(limits), 0)
        .map_err(|_| "network: Lark gateway unavailable")?;
    let tcp = match ws.get_mut() {
        MaybeTlsStream::Plain(s) => s,
        MaybeTlsStream::Rustls(s) => &mut s.sock,
        _ => return Err("network: unsupported gateway transport".into()),
    };
    tcp.set_read_timeout(Some(Duration::from_millis(600)))
        .map_err(|_| "network: gateway read timeout unavailable")?;
    tcp.set_write_timeout(Some(Duration::from_secs(6)))
        .map_err(|_| "network: gateway write timeout unavailable")?;
    let mut g = Gateway {
        ws,
        service,
        ping: interval(&d["data"]["ClientConfig"]),
        last: Instant::now(),
        awaiting: false,
        pieces: HashMap::new(),
    };
    ping(&mut g)?;
    Ok(g)
}
fn assembled(g: &mut Gateway, f: &Frame) -> R<Option<Vec<u8>>> {
    let sum = f
        .get("sum")
        .unwrap_or("1")
        .parse::<usize>()
        .map_err(|_| "provider: invalid fragment count")?;
    let part = f.payload.as_deref().unwrap_or_default();
    if sum == 1 {
        return Ok(Some(part.to_vec()));
    }
    let seq = f
        .get("seq")
        .and_then(|s| s.parse::<usize>().ok())
        .filter(|i| *i < sum)
        .ok_or("provider: invalid fragment index")?;
    let key = f
        .get("message_id")
        .filter(|s| !s.is_empty() && s.len() <= 256)
        .ok_or("provider: invalid fragment identity")?;
    if !(2..=64).contains(&sum) {
        return Err("provider: too many fragments".into());
    }
    g.pieces
        .retain(|_, p| p.at.elapsed() < Duration::from_secs(30));
    if !g.pieces.contains_key(key) && g.pieces.len() >= 8 {
        return Err("provider: too many incomplete events".into());
    }
    let entry = g.pieces.entry(key.into()).or_insert_with(|| Pieces {
        at: Instant::now(),
        bytes: 0,
        parts: vec![None; sum],
    });
    if entry.parts.len() != sum {
        return Err("provider: inconsistent fragment count".into());
    }
    if let Some(prev) = &entry.parts[seq] {
        if prev != part {
            return Err("provider: conflicting fragment".into());
        }
    } else {
        entry.bytes += part.len();
        if entry.bytes > 4 * 1024 * 1024 {
            return Err("provider: event too large".into());
        }
        entry.parts[seq] = Some(part.to_vec());
    }
    if entry.parts.iter().any(Option::is_none) {
        return Ok(None);
    }
    let entry = g.pieces.remove(key).unwrap();
    Ok(Some(entry.parts.into_iter().flatten().flatten().collect()))
}
pub(in crate::bots) fn poll(c: &mut Client, v: &vault::Vault, s: &mut Value) -> R<usize> {
    let id = field(s, "id")?.to_owned();
    let previous = sockets()
        .lock()
        .map_err(|_| "storage: gateway registry unavailable")?
        .remove(&id);
    let mut g = match previous {
        Some(g) => g,
        None => connect(s)?,
    };
    let mut count = 0;
    for _ in 0..64 {
        if g.last.elapsed() >= g.ping {
            if g.awaiting {
                return Err("network: Lark gateway heartbeat timed out".into());
            }
            ping(&mut g)?;
        }
        let mut f = match g.ws.read() {
            Ok(Message::Binary(b)) => {
                Frame::decode(b).map_err(|_| "provider: malformed gateway frame")?
            }
            Ok(Message::Close(_)) | Err(tungstenite::Error::ConnectionClosed) => return Ok(count),
            Ok(_) => continue,
            Err(tungstenite::Error::Io(e))
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
                ) =>
            {
                break;
            }
            Err(_) => return Err("network: Lark gateway disconnected".into()),
        };
        if f.method == 0 {
            if f.get("type") == Some("pong") {
                g.awaiting = false;
                if let Ok(cfg) =
                    serde_json::from_slice::<Value>(f.payload.as_deref().unwrap_or_default())
                    && cfg["PingInterval"].is_u64()
                {
                    g.ping = interval(&cfg);
                }
            }
            continue;
        }
        if f.method != 1 || f.get("type") != Some("event") {
            continue;
        }
        let Some(raw) = assembled(&mut g, &f)? else {
            continue;
        };
        let at = Instant::now();
        let envelope = serde_json::from_slice::<Value>(&raw)
            .map_err(|_| "provider: malformed event".to_owned());
        let processed = envelope.and_then(|e| messages::receive(c, v, s, &e));
        let code = if processed.is_ok() { 200 } else { 500 };
        f.headers.push(Header {
            key: "biz_rt".into(),
            value: at.elapsed().as_millis().to_string(),
        });
        f.payload = Some(
            json!({"code":code,"headers":null,"data":null})
                .to_string()
                .into_bytes(),
        );
        write(&mut g, &f)?;
        count += usize::from(processed?);
    }
    sockets()
        .lock()
        .map_err(|_| "storage: gateway registry unavailable")?
        .insert(id, g);
    Ok(count)
}
