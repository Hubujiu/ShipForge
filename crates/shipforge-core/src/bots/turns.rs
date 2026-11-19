use super::*;

pub(super) fn receive(c: &Context<'_>, b: &Binding, q: &Value) -> Result<Value> {
    let scene = required(q, "scene")?;
    let peer = short(q, "peer")?;
    let sender = short(q, "sender")?;
    let remote = short(q, "remote")?;
    if !matches!(scene, "direct" | "group") {
        return Err(invalid("unsupported external conversation"));
    }
    if !lark::provider(&b.provider) && !b.permits(scene) {
        return Ok(json!({"ignored":"disabled"}));
    }
    if !lark::provider(&b.provider) && scene == "direct" && sender != peer
        || b.provider == "wechat" && sender != b.account
    {
        return Ok(json!({"ignored":"wrong_recipient"}));
    }
    let body = required(q, "body")?.trim();
    if body.is_empty() || body.len() > 262144 {
        return Err(invalid("external message text is empty or too long"));
    }
    let media = lark::provider(&b.provider) || b.provider == "wecom";
    let fingerprint = if media {
        json!([sender, body, q["files"]])
    } else {
        json!([sender, body])
    };
    let digest = crate::worker::digest(&fingerprint.to_string());
    type Prior = (
        String,
        String,
        String,
        String,
        Option<String>,
        Option<String>,
    );
    let prior:Option<Prior>=c.db.query_row("SELECT bot,digest,id,chat,message,run FROM external_bot_turns WHERE provider=? AND account=? AND scene=? AND peer=? AND remote=?",params![b.provider,b.account,scene,peer,remote],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?))).optional().map_err(storage)?;
    if let Some((bot, hash, turn, chat, message, run)) = prior {
        if bot != b.id {
            return Ok(json!({"ignored":"previous_binding"}));
        }
        if hash != digest {
            return Err(conflict(
                "external message ID already identifies another payload",
            ));
        }
        if !room_visible(c, &chat)? {
            return Ok(json!({"ignored":"retired_conversation"}));
        }
        return Ok(json!({"turn":turn,"chat":chat,"message":message,"run":run,"duplicate":true}));
    }
    let new_prompt = if media {
        body.strip_prefix("/new")
            .filter(|rest| rest.is_empty() || rest.starts_with(char::is_whitespace))
            .map(str::trim)
    } else {
        None
    };
    let fresh = b.provider == "wechat" && body == "/new" || new_prompt.is_some();
    let prompt = new_prompt.unwrap_or(body);
    let acknowledgement = fresh && new_prompt.is_none_or(str::is_empty);
    let existing: Option<String> =
        c.db.query_row(
            "SELECT chat FROM external_bot_rooms WHERE bot=? AND scene=? AND peer=?",
            params![b.id, scene, peer],
            |r| r.get(0),
        )
        .optional()
        .map_err(storage)?;
    let chat = match existing {
        Some(chat) if !fresh && room_visible(c, &chat)? => chat,
        _ => {
            let title = format!(
                "{} · {}",
                if b.provider == "feishu" {
                    "飞书"
                } else if b.provider == "lark" {
                    "Lark"
                } else if b.provider == "wecom" {
                    "企业微信"
                } else if b.provider == "wechat" {
                    "微信"
                } else if scene == "group" {
                    "QQ 群"
                } else {
                    "QQ"
                },
                peer
            );
            let room = crate::domain::write(
                c,
                &json!({"op":"create","kind":"chat","title":title,"fields":{"agent":b.agent,"visibility":"private","external_provider":b.provider}}),
            )?;
            let chat = required(&room, "id")?.to_owned();
            c.db.execute("INSERT INTO external_bot_rooms(bot,scene,peer,chat) VALUES(?,?,?,?) ON CONFLICT(bot,scene,peer) DO UPDATE SET chat=excluded.chat",params![b.id,scene,peer,chat]).map_err(storage)?;
            chat
        }
    };
    let turn = Uuid::new_v4().to_string();
    let (message, run) = if acknowledgement {
        (None, None)
    } else {
        let sent = crate::chat::execute(c, &json!({"id":chat,"body":prompt}), "chat.send")?;
        let msg = c.readable("message", required(&sent, "message")?)?;
        let mut fields = msg.fields.clone();
        fields["external_provider"] = json!(b.provider);
        fields["external_author"] = json!(sender);
        fields["external_scene"] = json!(scene);
        let mut current = c.update(&msg, &msg.title, fields)?;
        if media && let Some(files) = q["files"].as_array() {
            if files.len() > 6 {
                return Err(invalid("up to six inbound files are supported"));
            }
            for file in files {
                current = crate::attachments::append(c, &current, file)?;
            }
        }
        (Some(msg.id), Some(required(&sent, "run")?.to_owned()))
    };
    c.db.execute("INSERT INTO external_bot_turns(id,bot,provider,account,scene,peer,remote,digest,chat,message,run,prepared) VALUES(?,?,?,?,?,?,?,?,?,?,?,?)",params![turn,b.id,b.provider,b.account,scene,peer,remote,digest,chat,message,run,acknowledgement]).map_err(storage)?;
    if acknowledgement {
        deliveries::insert(c, &turn, 1, &json!({"text":"已开启新会话"}))?;
    }
    Ok(json!({"turn":turn,"chat":chat,"message":message,"run":run,"duplicate":false}))
}
