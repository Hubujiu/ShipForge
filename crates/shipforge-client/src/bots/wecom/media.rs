use super::*;
use aes::{
    Aes256,
    cipher::{BlockDecrypt, KeyInit},
};
use md5::{Digest, Md5};
const LIMIT: usize = 4 * 1024 * 1024;

fn decrypt(mut bytes: Vec<u8>, raw: &str) -> R<Vec<u8>> {
    let key = STANDARD
        .decode(raw)
        .or_else(|_| base64::engine::general_purpose::STANDARD_NO_PAD.decode(raw))
        .map_err(|_| "provider: invalid WeCom media key")?;
    let key: [u8; 32] = key
        .try_into()
        .map_err(|_| "provider: invalid WeCom AES key length")?;
    if bytes.is_empty() || !bytes.len().is_multiple_of(32) {
        return Err("provider: malformed WeCom media block".into());
    }
    let cipher = Aes256::new((&key).into());
    let mut previous = [0u8; 16];
    previous.copy_from_slice(&key[..16]);
    for block in bytes.as_chunks_mut::<16>().0 {
        let encrypted = *block;
        cipher.decrypt_block(block.into());
        for (i, iv) in block.iter_mut().zip(previous) {
            *i ^= iv;
        }
        previous = encrypted;
    }
    let pad = usize::from(*bytes.last().unwrap_or(&0));
    if !(1..=32).contains(&pad)
        || pad > bytes.len()
        || !bytes[bytes.len() - pad..]
            .iter()
            .all(|v| usize::from(*v) == pad)
    {
        return Err("provider: invalid WeCom media padding".into());
    }
    bytes.truncate(bytes.len() - pad);
    Ok(bytes)
}
fn download(node: &Value, kind: &str, left: usize) -> R<(Value, usize)> {
    let url = net::trusted_with(
        field(node, "url")?,
        "https",
        &["weixin.qq.com", "qpic.cn", "wecom.work", "weixin.qq.com.cn"],
        net::fixture_from(FIXTURE)?,
    )?;
    let mut res = net::http()
        .get(url.as_str())
        .call()
        .map_err(|_| "network: WeCom attachment unavailable")?;
    if !res.status().is_success() {
        return Err(format!(
            "provider: WeCom attachment returned {}",
            res.status().as_u16()
        ));
    }
    let name = res
        .headers()
        .get("Content-Disposition")
        .and_then(|x| x.to_str().ok())
        .and_then(|s| {
            s.split(';')
                .find_map(|p| p.trim().strip_prefix("filename="))
        })
        .map(|s| s.trim_matches('"').to_owned());
    let bytes = res
        .body_mut()
        .with_config()
        .limit((left + 32) as u64)
        .read_to_vec()
        .map_err(|_| "provider: WeCom attachment exceeds the size limit or is incomplete")?;
    let clear = decrypt(bytes, field(node, "aeskey")?)?;
    let len = clear.len();
    if len == 0 || len > left {
        return Err("provider: WeCom attachment outside allowed size".into());
    }
    let (fallback, mime) = match kind {
        "image" if clear.starts_with(b"\x89PNG\r\n\x1a\n") => ("image.png", "image/png"),
        "image" if clear.starts_with(b"\xff\xd8\xff") => ("image.jpg", "image/jpeg"),
        "video" => ("video.mp4", "video/mp4"),
        _ => ("attachment.bin", "application/octet-stream"),
    };
    let name = name
        .as_deref()
        .or(node["filename"].as_str())
        .unwrap_or(fallback)
        .split(['\\', '/'])
        .next_back()
        .unwrap_or(fallback);
    let name = name
        .chars()
        .filter(|c| !c.is_control() && !"<>:\"|?*".contains(*c))
        .take(150)
        .collect::<String>();
    let name = if matches!(name.as_str(), "" | "." | "..") {
        fallback
    } else {
        &name
    };
    Ok((
        json!({"filename":name,"media_type":mime,"content_base64":STANDARD.encode(clear)}),
        len,
    ))
}
pub(super) fn read(body: &Value) -> R<Option<(String, Vec<Value>)>> {
    let mut text = Vec::new();
    let mut files = Vec::new();
    let mut remaining = LIMIT;
    let parts = if body["msgtype"] == "mixed" {
        body["mixed"]["msg_item"]
            .as_array()
            .map(Vec::as_slice)
            .unwrap_or(&[])
    } else {
        std::slice::from_ref(body)
    };
    for part in parts.iter().take(32) {
        let kind = part["msgtype"].as_str().unwrap_or("");
        if matches!(kind, "text" | "voice") {
            if let Some(s) = part[kind]["content"]
                .as_str()
                .map(str::trim)
                .filter(|s| !s.is_empty())
            {
                text.push(s.to_owned());
            }
        } else if matches!(kind, "file" | "image" | "video") && files.len() < 6 {
            let (file, len) = download(&part[kind], kind, remaining)?;
            remaining -= len;
            files.push(file);
        }
    }
    if text.is_empty() && !files.is_empty() {
        text.push("请查看企业微信附件".into());
    }
    if text.is_empty() {
        return Ok(None);
    }
    Ok(Some((text.join("\n"), files)))
}
pub(super) fn upload(
    c: &mut Client,
    v: &vault::Vault,
    s: &Value,
    g: &mut socket::Gateway,
    work: &Value,
) -> R<Value> {
    let file=c.execute(json!({"op":"attachment.download","space":s["space"],"kind":"artifact","id":work["artifact"],"attachment_id":work["attachment"],"allow_cached":false}))?;
    let bytes = STANDARD
        .decode(
            file["content_base64"]
                .as_str()
                .ok_or("invalid: artifact bytes missing")?,
        )
        .map_err(|_| "invalid: invalid artifact bytes")?;
    if bytes.is_empty() || bytes.len() > 50 * 1024 * 1024 {
        return Err("invalid: WeCom file must be within 50 MiB".into());
    }
    let chunk = 512 * 1024;
    let md5 = Md5::digest(&bytes)
        .iter()
        .map(|v| format!("{v:02x}"))
        .collect::<String>();
    let opened=g.command(v,s,"aibot_upload_media_init",None,json!({"type":"file","filename":file["filename"],"total_size":bytes.len(),"total_chunks":bytes.len().div_ceil(chunk),"md5":md5}))?;
    let id = field(&opened["body"], "upload_id")?;
    for (index, part) in bytes.chunks(chunk).enumerate() {
        g.command(
            v,
            s,
            "aibot_upload_media_chunk",
            None,
            json!({"upload_id":id,"chunk_index":index,"base64_data":STANDARD.encode(part)}),
        )?;
    }
    let result = g.command(
        v,
        s,
        "aibot_upload_media_finish",
        None,
        json!({"upload_id":id}),
    )?;
    Ok(json!({"msgtype":"file","file":{"media_id":field(&result["body"],"media_id")?}}))
}
