use super::*;
use std::{
    collections::HashMap,
    net::TcpStream,
    sync::{Mutex, OnceLock},
    time::{Duration, Instant},
};
use tungstenite::{Message, WebSocket, stream::MaybeTlsStream};

pub(super) struct Gateway {
    _lock: std::fs::File,
    ws: WebSocket<MaybeTlsStream<TcpStream>>,
    heartbeat: Instant,
}
static LIVE: OnceLock<Mutex<HashMap<String, Gateway>>> = OnceLock::new();
fn registry() -> &'static Mutex<HashMap<String, Gateway>> {
    LIVE.get_or_init(Default::default)
}
pub(in crate::bots) fn close(id: &str) {
    if let Ok(mut r) = registry().lock()
        && let Some(mut g) = r.remove(id)
    {
        let _ = g.ws.close(None);
    }
}
fn open(s: &Value, v: &vault::Vault) -> R<Gateway> {
    let lock = v.stream_lock(field(s, "id")?)?;
    let endpoint = if let Some(mut u) = net::fixture_from(FIXTURE)? {
        u.set_scheme("ws")
            .map_err(|_| "invalid: fixture WebSocket")?;
        u.to_string()
    } else {
        "wss://openws.work.weixin.qq.com".into()
    };
    let cfg = tungstenite::protocol::WebSocketConfig::default()
        .max_message_size(Some(2 * 1024 * 1024))
        .max_frame_size(Some(2 * 1024 * 1024));
    let (mut ws, _) = tungstenite::client::connect_with_config(endpoint.as_str(), Some(cfg), 0)
        .map_err(|_| "network: WeCom gateway unavailable")?;
    let tcp = match ws.get_mut() {
        MaybeTlsStream::Plain(s) => s,
        MaybeTlsStream::Rustls(s) => &mut s.sock,
        _ => return Err("network: unsupported WeCom transport".into()),
    };
    tcp.set_read_timeout(Some(Duration::from_millis(350)))
        .and_then(|()| tcp.set_write_timeout(Some(Duration::from_secs(5))))
        .map_err(|_| "network: WeCom socket timeouts unavailable")?;
    let mut g = Gateway {
        _lock: lock,
        ws,
        heartbeat: Instant::now(),
    };
    g.command(
        v,
        s,
        "aibot_subscribe",
        None,
        json!({"bot_id":s["account"],"secret":s["secret"]}),
    )?;
    Ok(g)
}
pub(super) fn session<T>(
    s: &Value,
    v: &vault::Vault,
    work: impl FnOnce(&mut Gateway) -> R<T>,
) -> R<T> {
    let id = field(s, "id")?;
    let cached = registry()
        .lock()
        .map_err(|_| "storage: WeCom session registry")?
        .remove(id);
    let mut g = match cached {
        Some(g) => g,
        None => open(s, v)?,
    };
    let result = work(&mut g);
    if result.as_ref().is_ok()
        || result
            .as_ref()
            .is_err_and(|e| e.starts_with("provider:") && !e.contains("authorization rejected"))
    {
        registry()
            .lock()
            .map_err(|_| "storage: WeCom session registry")?
            .insert(id.into(), g);
    }
    result
}
impl Gateway {
    fn read(&mut self, v: &vault::Vault, s: &Value) -> R<Option<Value>> {
        let raw = match self.ws.read() {
            Ok(Message::Text(t)) => t,
            Ok(Message::Ping(bytes)) => {
                self.ws
                    .send(Message::Pong(bytes))
                    .map_err(|_| "network: WeCom heartbeat interrupted")?;
                return Ok(Some(Value::Null));
            }
            Ok(Message::Close(_)) => return Err("network: WeCom connection closed".into()),
            Ok(_) => return Ok(Some(Value::Null)),
            Err(tungstenite::Error::Io(e))
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) =>
            {
                return Ok(None);
            }
            Err(_) => return Err("network: WeCom connection interrupted".into()),
        };
        let frame: Value =
            serde_json::from_str(raw.as_str()).map_err(|_| "provider: malformed WeCom frame")?;
        match frame["cmd"].as_str() {
            Some("aibot_msg_callback") if frame["body"]["aibotid"] == s["account"] => {
                v.buffer_wecom(field(s, "id")?, &frame)?;
                Ok(Some(Value::Null))
            }
            Some("aibot_event_callback")
                if frame["body"]["event"]["eventtype"] == "disconnected_event"
                    && (frame["body"]["aibotid"].is_null()
                        || frame["body"]["aibotid"] == s["account"]) =>
            {
                Err("conflict: WeCom session replaced by another connection".into())
            }
            Some(_) => Ok(Some(Value::Null)),
            None => Ok(Some(frame)),
        }
    }
    pub(super) fn command(
        &mut self,
        v: &vault::Vault,
        s: &Value,
        cmd: &str,
        req: Option<&str>,
        body: Value,
    ) -> R<Value> {
        let key = req
            .map(str::to_owned)
            .unwrap_or_else(|| format!("{cmd}_{}", Uuid::new_v4().simple()));
        let mut msg = json!({"cmd":cmd,"headers":{"req_id":key}});
        if !body.is_null() {
            msg["body"] = body;
        }
        self.ws
            .send(Message::Text(msg.to_string().into()))
            .map_err(|_| "network: WeCom command send interrupted")?;
        let deadline = Instant::now() + Duration::from_secs(8);
        while Instant::now() < deadline {
            let Some(reply) = self.read(v, s)? else {
                continue;
            };
            if reply["headers"]["req_id"] != key {
                continue;
            }
            let code = reply["errcode"]
                .as_i64()
                .ok_or("network: incomplete WeCom acknowledgement")?;
            if code == 0 {
                return Ok(reply);
            }
            if code == -1 {
                return Err("provider: WeCom temporarily unavailable".into());
            }
            if matches!(code, 45009 | 45011) {
                return Err("provider: WeCom rate limit".into());
            }
            if cmd == "aibot_subscribe"
                || cmd == "ping" && matches!(code, 40013 | 40014 | 40125 | 41001 | 42001)
            {
                return Err(format!("provider: WeCom authorization rejected ({code})"));
            }
            return Err(format!("provider: WeCom {cmd} refused ({code})"));
        }
        Err("network: WeCom acknowledgement timed out".into())
    }
    pub(super) fn collect(&mut self, v: &vault::Vault, s: &Value) -> R<()> {
        if self.heartbeat.elapsed() >= Duration::from_secs(25) {
            self.command(v, s, "ping", None, Value::Null)?;
            self.heartbeat = Instant::now();
        }
        for _ in 0..32 {
            if self.read(v, s)?.is_none() {
                break;
            }
        }
        Ok(())
    }
}
