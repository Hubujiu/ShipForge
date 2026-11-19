use super::*;
use std::time::{SystemTime, UNIX_EPOCH};

pub(super) fn connect(
    binding: &Value,
    credentials: &Value,
    exchange: bool,
    renewal: bool,
) -> Result<(Api, Value)> {
    let base = origin("https://open.pingcode.com")?;
    let http = http();
    let mode = binding["pingcode_auth"].as_str().unwrap_or("token");
    let stored = if !exchange || mode == "token" {
        json!({"access_token":cred(credentials,"access_token")?})
    } else {
        let mut args = if mode == "authorization" && renewal {
            vec![
                ("grant_type", "refresh_token".into()),
                ("refresh_token", cred(credentials, "refresh_token")?),
            ]
        } else {
            let grant = if mode == "authorization" {
                "authorization_code"
            } else {
                "client_credentials"
            };
            vec![
                ("grant_type", grant.into()),
                ("client_id", cred(credentials, "client_id")?),
                ("client_secret", cred(credentials, "client_secret")?),
            ]
        };
        if mode == "authorization" && !renewal {
            args.push(("code", cred(credentials, "code")?));
        }
        let url = endpoint(&base, "/v2/auth/token", &args)?;
        // Avoid surfacing this URL: the official exchange places the client secret in the query.
        let token = unpack(
            http.get(url.as_str())
                .call()
                .map_err(|_| "vendor: PingCode authorization endpoint unavailable")?,
        )?;
        let mut saved = json!({"access_token":cred(&token,"access_token")?});
        if mode == "application" {
            saved["client_id"] = json!(cred(credentials, "client_id")?);
            saved["client_secret"] = json!(cred(credentials, "client_secret")?);
        } else if let Some(refresh) = token["refresh_token"].as_str() {
            saved["refresh_token"] = json!(refresh);
        }
        saved
    };
    let token = cred(&stored, "access_token")?;
    if !token.bytes().all(|b| b.is_ascii_graphic()) {
        return Err("invalid: invalid PingCode bearer token".into());
    }
    Ok((
        Api {
            http,
            root: format!("{base}/v2"),
            token,
            zen: false,
        },
        stored,
    ))
}

pub(super) fn check_enterprise(api: &Api, b: &Value) -> Result<()> {
    let team = api.get("/directory/team", &[])?;
    if team["id"] != b["enterprise_id"] {
        return Err("forbidden: PingCode token belongs to another enterprise".into());
    }
    Ok(())
}

pub(super) fn projects(api: &Api, size: usize) -> Result<Vec<Value>> {
    api.pages(
        "/pjm/projects",
        "values",
        &[("include_archived", "1".into())],
        size,
    )?
    .into_iter()
    .map(|row| Ok(json!({"id":id(&row["id"])?,"name":row["name"],"archived":row["is_archived"]})))
    .collect()
}

pub(super) fn issues(
    api: &Api,
    scope: &str,
    size: usize,
    since: Option<i64>,
) -> Result<Vec<Value>> {
    let mut query = vec![
        ("project_id", scope.to_owned()),
        ("include_archived", "1".into()),
        ("include_deleted", "0".into()),
    ];
    if let Some(ms) = since {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| "invalid: local clock predates epoch")?
            .as_secs();
        query.push(("updated_between", format!("{},{now}", ms / 1000)));
    }
    let items = api.pages("/pjm/workitems", "values", &query, size)?;
    for row in &items {
        if row["project"]["id"] != scope {
            return Err("forbidden: PingCode returned an item from another project".into());
        }
    }
    Ok(items)
}
