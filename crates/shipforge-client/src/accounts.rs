use crate::{Client, credentials::Connection};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{fs, io::Write, path::Path};
use uuid::Uuid;

fn storage(e: impl std::fmt::Display) -> String {
    format!("storage: {e}")
}
type Result<T> = std::result::Result<T, String>;

#[derive(Clone, Serialize, Deserialize)]
struct Account {
    id: String,
    actor: Option<String>,
    #[serde(default)]
    email_account: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    display: Option<String>,
    connection: Connection,
    #[serde(default)]
    needs_login: bool,
    #[serde(default)]
    place: Option<Place>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Place {
    space: String,
    page: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    view: Option<String>,
}
#[derive(Default, Serialize, Deserialize)]
struct Book {
    active: Option<String>,
    accounts: Vec<Account>,
}

impl Book {
    fn read(dir: &Path) -> Result<Self> {
        let bytes = match fs::read(dir.join("connection.json")) {
            Ok(b) => b,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(e) => return Err(storage(e)),
        };
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Disk {
            Current(Book),
            Previous(Connection),
        }
        match serde_json::from_slice(&bytes).map_err(storage)? {
            Disk::Current(book) => Ok(book),
            Disk::Previous(connection) => {
                let digest = Sha256::digest(&bytes);
                let suffix = digest
                    .iter()
                    .take(16)
                    .map(|byte| format!("{byte:02x}"))
                    .collect::<String>();
                let id = format!("legacy-{suffix}");
                Ok(Self {
                    active: Some(id.clone()),
                    accounts: vec![Account {
                        id,
                        connection,
                        actor: None,
                        email_account: false,
                        display: None,
                        needs_login: false,
                        place: None,
                    }],
                })
            }
        }
    }
    fn edit<T>(dir: &Path, f: impl FnOnce(&mut Self) -> Result<T>) -> Result<T> {
        fs::create_dir_all(dir).map_err(storage)?;
        let guard = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(dir.join(".account-lock"))
            .map_err(storage)?;
        guard.lock().map_err(storage)?;
        let mut book = Self::read(dir)?;
        let out = f(&mut book)?;
        let path = dir.join("connection.json");
        if book.accounts.is_empty() {
            match fs::remove_file(path) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(storage(e)),
            }
        } else {
            let mut file = tempfile::NamedTempFile::new_in(dir).map_err(storage)?;
            serde_json::to_writer(&mut file, &book).map_err(storage)?;
            file.flush()
                .and_then(|()| file.as_file().sync_all())
                .map_err(storage)?;
            file.persist(path).map_err(storage)?;
        }
        Ok(out)
    }
    fn current(&self) -> Option<&Account> {
        self.accounts
            .iter()
            .find(|a| Some(&a.id) == self.active.as_ref())
    }
}
pub(crate) fn connection(dir: &Path) -> Result<Option<(String, String)>> {
    Book::read(dir)?
        .current()
        .filter(|a| !a.needs_login)
        .map(|a| Ok((a.connection.url.clone(), a.connection.secret()?)))
        .transpose()
}
pub(crate) fn save(
    dir: &Path,
    url: &str,
    token: &str,
    who: &Value,
    expected: Option<&str>,
) -> Result<()> {
    let connection = Connection::new(url, token)?;
    let actor = who["actor"]
        .as_str()
        .ok_or("server: missing account identity")?;
    let email_account = who["email_account"] == true;
    let display = who["display"].as_str().map(str::to_owned);
    Book::edit(dir, |book| {
        let same = |a: &Account| {
            a.connection.url == url
                && (a.actor.as_deref() == Some(actor)
                    || (a.actor.is_none()
                        && a.connection
                            .secret()
                            .as_deref()
                            .is_ok_and(|saved| saved == token)))
        };
        let existing = if let Some(id) = expected {
            let a = book
                .accounts
                .iter_mut()
                .find(|a| a.id == id)
                .ok_or("not_found: saved account")?;
            if a.connection.url != url || a.actor.as_deref().is_some_and(|name| name != actor) {
                return Err("forbidden: sign in with the same team and account".into());
            }
            Some(a)
        } else {
            book.accounts.iter_mut().find(|a| same(a))
        };
        let id = if let Some(a) = existing {
            if a.actor.is_none() {
                a.place = None;
            }
            a.actor = Some(actor.to_string());
            a.email_account = email_account;
            a.display = display.clone();
            a.needs_login = false;
            a.connection = connection;
            a.id.clone()
        } else {
            let id = Uuid::new_v4().to_string();
            book.accounts.push(Account {
                id: id.clone(),
                connection,
                actor: Some(actor.to_string()),
                email_account,
                display,
                needs_login: false,
                place: None,
            });
            id
        };
        book.active = Some(id);
        Ok(())
    })
}
pub(crate) fn list(dir: &Path) -> Result<Value> {
    let book = Book::read(dir)?;
    let entries: Vec<_> = book
        .accounts
        .iter()
        .map(|a| {
            json!({"id":a.id,"actor":a.actor,"url":a.connection.url,"email_account":a.email_account,"display":a.display,
        "active":book.active.as_deref()==Some(&a.id),"needs_login":a.needs_login})
        })
        .collect();
    Ok(json!({"accounts":entries,"active":book.active}))
}
pub(crate) fn logout(dir: &Path, id: Option<&str>) -> Result<()> {
    Book::edit(dir, |book| {
        let id = id.map(str::to_string).or_else(|| book.active.clone());
        if let Some(id) = id {
            if !book.accounts.iter().any(|a| a.id == id) {
                return Err("not_found: saved account".into());
            }
            book.accounts.retain(|a| a.id != id);
            if book.active.as_ref() == Some(&id) {
                book.active = None;
            }
        }
        Ok(())
    })
}
pub(crate) fn remember(dir: &Path, q: &Value) -> Result<Value> {
    let id = q["id"].as_str().ok_or("invalid: expected account id")?;
    let place = serde_json::from_value::<Place>(q["location"].clone())
        .map_err(|_| "invalid: expected a location with space and page")?;
    if place.kind.is_some() != place.id.is_some()
        || [&place.space, &place.page]
            .into_iter()
            .chain(place.kind.iter())
            .chain(place.id.iter())
            .chain(place.view.iter())
            .any(|s| s.is_empty() || s.len() > 160 || s.chars().any(char::is_control))
    {
        return Err("invalid: invalid account location".into());
    }
    Book::edit(dir, |book| {
        if book.active.as_deref() != Some(id) {
            return Err("conflict: the active account changed".into());
        }
        let a = book
            .accounts
            .iter_mut()
            .find(|a| a.id == id)
            .ok_or("not_found: saved account")?;
        a.place = Some(place);
        Ok(json!({"saved":true}))
    })
}
fn same_credential(book: &mut Book, snapshot: &Account) -> Result<usize> {
    book.accounts
        .iter()
        .position(|a| a.id == snapshot.id && a.connection.token == snapshot.connection.token)
        .ok_or_else(|| "conflict: the saved credential changed; open the account list again".into())
}
fn expired(dir: &Path, account: &Account) -> Result<()> {
    Book::edit(dir, |book| {
        let index = same_credential(book, account)?;
        book.accounts[index].needs_login = true;
        book.accounts[index].display = None;
        if book.active.as_ref() == Some(&account.id) {
            book.active = None;
        }
        Ok(())
    })
}
fn location(client: &mut Client, account: &Account) -> Result<Value> {
    let spaces = client.execute(json!({"op":"space.list","allow_cached":false}))?;
    let joined = spaces
        .as_array()
        .ok_or("server: invalid space list")?
        .iter()
        .filter(|s| {
            s["joined"] != false
                && s["fields"]["joined"] != false
                && s["fields"]["deactivated"] != true
        })
        .filter_map(|s| s["id"].as_str())
        .collect::<Vec<_>>();
    if let Some(place) = &account.place
        && joined.contains(&place.space.as_str())
    {
        let mut result = json!(place);
        for (kind, id, key) in [
            ("view", place.view.as_ref(), "view"),
            (
                place.kind.as_deref().unwrap_or(""),
                place.id.as_ref(),
                "selected",
            ),
        ] {
            if let Some(id) = id {
                match client.execute(json!({"op":"get","kind":kind,"space":place.space,"id":id,"allow_cached":false})){
                    Ok(record)=>result[key]=record,
                    Err(e) if e.starts_with("not_found:")||e.starts_with("forbidden:")=>return Ok(json!({"space":place.space,"page":"tasks"})),
                    Err(e)=>return Err(e)
                }
            }
        }
        return Ok(result);
    }
    Ok(joined
        .iter()
        .find(|s| **s == "default")
        .or_else(|| joined.first())
        .map_or_else(
            || json!({"space":"default","page":"spaces"}),
            |id| json!({"space":id,"page":"tasks"}),
        ))
}
pub(crate) fn switch(dir: &Path, id: &str) -> Result<(Client, Value)> {
    let mut book = Book::read(dir)?;
    let account = book
        .accounts
        .iter_mut()
        .find(|a| a.id == id)
        .ok_or("not_found: saved account")?;
    if account.needs_login {
        return Err("unauthorized: sign in to this saved account again".into());
    }
    let mut client =
        Client::remote_in(dir, &account.connection.url, &account.connection.secret()?)?;
    let identity = match client.execute(json!({"op":"access.whoami"})) {
        Ok(who) => who,
        Err(e) if e.starts_with("unauthorized:") => {
            expired(dir, account)?;
            return Err(e);
        }
        Err(e) => return Err(e),
    };
    let actor = identity["actor"]
        .as_str()
        .ok_or("server: expected an account identity")?;
    if account.actor.as_deref().is_some_and(|known| known != actor) {
        return Err("forbidden: identity changed; authenticate this account again".into());
    }
    let place = location(&mut client, account)?;
    Book::edit(dir, |book| {
        let index = same_credential(book, account)?;
        book.accounts[index].actor = Some(actor.to_string());
        book.accounts[index].email_account = identity["email_account"] == true;
        book.accounts[index].display = identity["display"].as_str().map(str::to_owned);
        book.active = Some(id.to_string());
        Ok(())
    })?;
    Ok((client, json!({"id":id,"location":place})))
}
pub(crate) fn startup(dir: &Path, client: &mut Client) -> Result<Value> {
    let book = Book::read(dir)?;
    let Some(account) = book.current() else {
        let needs_login = book.accounts.iter().any(|a| a.needs_login);
        return Ok(
            json!({"needs_login":needs_login,"location":{"page":if needs_login{"settings"}else{"tasks"}}}),
        );
    };
    match switch(dir, &account.id) {
        Ok((new, result)) => {
            *client = new;
            Ok(result)
        }
        Err(e) if e.starts_with("unauthorized:") => {
            *client = Client::local(dir)?;
            Ok(json!({"needs_login":true,"location":{"page":"settings"}}))
        }
        Err(e) if e.starts_with("offline:") => {
            Ok(json!({"id":account.id,"offline":true,"location":account.place}))
        }
        Err(e) => Err(e),
    }
}
