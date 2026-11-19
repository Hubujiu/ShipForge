use super::*;
use crate::Store;

impl Store {
    /// Accept an installation notice after the HTTP transport verifies the App signature.
    pub fn github_installation_notice(&mut self, event: &str, body: &Value) -> Result<()> {
        let install = body["installation"]["id"]
            .as_i64()
            .filter(|n| *n > 0)
            .ok_or_else(|| invalid("missing installation"))?
            .to_string();
        let tx = self.db.transaction().map_err(storage)?;
        if event == "installation" {
            match body["action"].as_str() {
                Some("created") => {
                    if let Some(sender) = body["sender"]["id"].as_i64() {
                        tx.execute("INSERT INTO forge_installers VALUES(?,?) ON CONFLICT(installation) DO UPDATE SET actor=excluded.actor",params![install,sender.to_string()]).map_err(storage)?;
                    }
                }
                Some("deleted" | "suspend") => {
                    tx.execute(
                        "DELETE FROM forge_installers WHERE installation=?",
                        [&install],
                    )
                    .map_err(storage)?;
                    tx.execute("UPDATE forge_pairs SET active=0 WHERE platform='github' AND installation=?",[&install]).map_err(storage)?;
                    tx.execute(
                        "DELETE FROM forge_accounts WHERE platform='github' AND installation=?",
                        [&install],
                    )
                    .map_err(storage)?;
                }
                _ => (),
            }
        } else if event == "installation_repositories" && body["action"] == "removed" {
            for repo in body["repositories_removed"]
                .as_array()
                .into_iter()
                .flatten()
            {
                if let Some(id) = repo["id"].as_i64() {
                    tx.execute("UPDATE forge_pairs SET active=0 WHERE platform='github' AND installation=? AND repository=?",params![install,id.to_string()]).map_err(storage)?;
                }
            }
        }
        tx.commit().map_err(storage)
    }
    /// GitHub's route is authenticated by its App HMAC; GitLab has a separate secret per pairing.
    pub fn route_forge_hook(
        &mut self,
        platform: &str,
        pair_id: &str,
        secret: &str,
        _event: &str,
        delivery: &str,
        payload: &Value,
    ) -> Result<Vec<Value>> {
        if delivery.is_empty() || delivery.len() > 250 || delivery.chars().any(char::is_control) {
            return Err(invalid("invalid delivery identifier"));
        }
        let mut pairs = Vec::new();
        if platform == "github" {
            let number = |v: &Value| {
                v.as_i64()
                    .filter(|n| *n > 0)
                    .map(|n| n.to_string())
                    .ok_or_else(|| invalid("missing source identifier"))
            };
            let mut list=self.db.prepare("SELECT id,space,platform,actor FROM forge_pairs WHERE active=1 AND platform='github' AND installation=? AND repository=?").map_err(storage)?;
            for row in list
                .query_map(
                    params![
                        number(&payload["installation"]["id"])?,
                        number(&payload["repository"]["id"])?
                    ],
                    route,
                )
                .map_err(storage)?
            {
                pairs.push(row.map_err(storage)?);
            }
        } else {
            let b=self.db.query_row("SELECT id,space,platform,actor FROM forge_pairs WHERE id=? AND platform IN('gitlab','gitlab-self') AND hook_hash=? AND active=1",params![pair_id,crate::worker::digest(secret)],route).optional().map_err(storage)?.ok_or_else(||"unauthorized: invalid project webhook".to_owned())?;
            pairs.push(b);
        }
        Ok(pairs)
    }
}
fn route(r: &rusqlite::Row<'_>) -> rusqlite::Result<Value> {
    Ok(
        json!({"id":r.get::<_,String>(0)?,"space":r.get::<_,String>(1)?,"platform":r.get::<_,String>(2)?,"actor":r.get::<_,String>(3)?}),
    )
}
