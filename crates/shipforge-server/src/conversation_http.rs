use super::*;

pub(super) fn matches(path: &str) -> bool {
    path.starts_with("/document/") || path.starts_with("/share/") && path.ends_with("/image.png")
}
fn get(
    request: &Request,
    auth: &Auth,
    store: &mut Store,
) -> Result<(&'static str, Vec<u8>), String> {
    if request.method() != &Method::Get {
        return Err("method: use GET".into());
    }
    let document = request.url().strip_prefix("/document/");
    let op = if document.is_some() {
        "document.open"
    } else {
        "chat.share.image"
    };
    let actor = request
        .headers()
        .iter()
        .find(|h| h.field.equiv("Authorization"))
        .map(|h| {
            let token = h
                .value
                .as_str()
                .strip_prefix("Bearer ")
                .ok_or_else(|| "unauthorized: invalid bearer token".to_owned())?;
            let p = auth.authenticate(token)?;
            auth::authorize(&p, &json!({"op":op}))?;
            Ok::<_, String>(p.actor)
        })
        .transpose()?;
    if let Some(path) = document {
        let actor = actor.ok_or("unauthorized: sign in to this workspace")?;
        let doc = store.execute(
            &actor,
            json!({"op":op,"link":format!("shipforge://document/{path}")}),
        )?;
        let text = format!(
            "{}\n\n{}",
            doc["title"].as_str().unwrap_or(""),
            doc["fields"]["body"].as_str().unwrap_or("")
        );
        Ok(("text/plain; charset=utf-8", text.into_bytes()))
    } else {
        let key = request
            .url()
            .strip_prefix("/share/")
            .and_then(|s| s.strip_suffix("/image.png"))
            .ok_or("not_found: share")?;
        let snapshot = store.shared_conversation(key, actor.as_deref())?;
        Ok(("image/png", shipforge_core::render_share_png(&snapshot)?))
    }
}
pub(super) fn respond(request: Request, auth: &Auth, store: &mut Store) {
    let (status, media, data) = match get(&request, auth, store) {
        Ok((media, bytes)) => (200, media, bytes),
        Err(error) => {
            let (code, message) = error
                .split_once(": ")
                .unwrap_or(("server", "request failed"));
            let status = match code {
                "unauthorized" => 401,
                "forbidden" => 403,
                "not_found" => 404,
                "method" => 405,
                "invalid" => 400,
                _ => 500,
            };
            let message = if status == 500 {
                "request failed"
            } else {
                message
            };
            (
                status,
                "application/json; charset=utf-8",
                json!({"protocol":1,"ok":false,"error":{"code":code,"message":message}})
                    .to_string()
                    .into_bytes(),
            )
        }
    };
    let response = Response::from_data(data)
        .with_status_code(StatusCode(status))
        .with_header(Header::from_bytes("Content-Type", media).unwrap())
        .with_header(Header::from_bytes("Cache-Control", "private, no-store").unwrap())
        .with_header(Header::from_bytes("Referrer-Policy", "no-referrer").unwrap())
        .with_header(Header::from_bytes("X-Content-Type-Options", "nosniff").unwrap());
    let _ = request.respond(response);
}
