use clap::{Args, Parser, Subcommand};
use serde_json::{Value, json};
use shipforge_client::Client;
use std::io::Read;
use std::path::PathBuf;
use std::process::ExitCode;

#[derive(Parser)]
#[command(
    name = "shipforge",
    version,
    about = "ShipForge local and team workspace CLI"
)]
struct Cli {
    #[arg(long, global = true)]
    data_dir: Option<PathBuf>,
    #[arg(long, global = true, default_value = "default")]
    space: String,
    #[arg(long, global = true)]
    pretty: bool,
    #[command(subcommand)]
    command: Command,
}
#[derive(Subcommand)]
enum Command {
    /// Run any protocol operation. With no argument, read JSON from stdin.
    Request(Input),
    /// Write the JSON Schema for supported requests.
    Schema,
    /// List records of a kind.
    List {
        kind: String,
        #[arg(long)]
        limit: Option<u64>,
    },
    Get {
        kind: String,
        id: String,
    },
    Create {
        kind: String,
        title: String,
        #[arg(long, default_value = "{}")]
        fields: String,
        #[arg(long)]
        id: Option<String>,
    },
    Update {
        kind: String,
        id: String,
        #[arg(long)]
        title: Option<String>,
        #[arg(long)]
        fields: Option<String>,
        #[arg(long)]
        rev: Option<i64>,
    },
    Delete {
        kind: String,
        id: String,
        #[arg(long)]
        rev: Option<i64>,
    },
    Search {
        query: String,
    },
    /// Set a group's avatar from a local image using its current revision.
    GroupAvatar {
        id: String,
        path: PathBuf,
        #[arg(long)]
        rev: i64,
    },
    /// Attach a local file, or create a private document if no document ID is given.
    Upload {
        path: PathBuf,
        #[arg(long, default_value = "document")]
        kind: String,
        #[arg(long)]
        id: Option<String>,
        #[arg(long)]
        media_type: Option<String>,
    },
    /// Save a frozen conversation share as a PNG at a new local path.
    ShareImage {
        share: String,
        path: PathBuf,
    },
    /// Download one attachment to a new local file.
    Download {
        kind: String,
        id: String,
        attachment_id: String,
        path: PathBuf,
        #[arg(long)]
        version: Option<u64>,
    },
    /// Verify and save a team connection. Read the token from stdin or SHIPFORGE_TOKEN.
    Connect {
        url: String,
        #[arg(long)]
        token_stdin: bool,
        /// Reauthenticate this saved account and keep its last location.
        #[arg(long)]
        account: Option<String>,
    },
    /// Remove the saved team connection from this device.
    Disconnect,
    /// Inspect or send offline drafts.
    Sync {
        #[arg(value_parser=["status","pending","flush"])]
        action: String,
    },
}
#[derive(Args)]
struct Input {
    #[arg(conflicts_with = "file")]
    json: Option<String>,
    #[arg(long)]
    file: Option<PathBuf>,
}
fn main() -> ExitCode {
    let cli = Cli::parse();
    let pretty = cli.pretty;
    let (message, exit) = match execute(cli) {
        Ok(v) => (json!({"protocol":1,"ok":true,"data":v}), ExitCode::SUCCESS),
        Err(e) => {
            let (code, message) = e.split_once(": ").unwrap_or(("error", &e));
            (
                json!({"protocol":1,"ok":false,"error":{"code":code,"message":message}}),
                ExitCode::FAILURE,
            )
        }
    };
    if pretty {
        println!("{}", serde_json::to_string_pretty(&message).unwrap());
    } else {
        println!("{message}");
    }
    exit
}
fn stdin() -> Result<String, String> {
    let mut text = String::new();
    std::io::stdin()
        .take(8 * 1024 * 1024 + 1)
        .read_to_string(&mut text)
        .map_err(|e| format!("invalid: {e}"))?;
    if text.len() > 8 * 1024 * 1024 {
        return Err("invalid: input exceeds 8 MiB".into());
    }
    Ok(text)
}
fn parse(text: &str) -> Result<Value, String> {
    serde_json::from_str(text.trim_start_matches('\u{feff}')).map_err(|e| format!("invalid: {e}"))
}
fn execute(cli: Cli) -> Result<Value, String> {
    let mut body = match cli.command {
        Command::Schema => return Ok(shipforge_client::schema()),
        Command::Connect {
            url,
            token_stdin,
            account,
        } => {
            let token = if token_stdin {
                stdin()?
            } else {
                std::env::var("SHIPFORGE_TOKEN")
                    .map_err(|_| "invalid: pass --token-stdin or use SHIPFORGE_TOKEN".to_owned())?
            };
            Client::sign_in(
                cli.data_dir.as_deref(),
                &url,
                token.trim(),
                account.as_deref(),
            )?;
            return Ok(json!({"connected":true}));
        }
        Command::Disconnect => {
            Client::forget_connection(cli.data_dir.as_deref())?;
            return Ok(json!({"disconnected":true}));
        }
        Command::Request(input) => {
            let raw = match (input.json, input.file) {
                (Some(s), _) => s,
                (_, Some(p)) => std::fs::read_to_string(p).map_err(|e| format!("invalid: {e}"))?,
                _ => stdin()?,
            };
            parse(&raw)?
        }
        Command::List { kind, limit } => {
            let mut r = json!({"op":"list","kind":kind});
            if let Some(n) = limit {
                r["limit"] = json!(n)
            }
            r
        }
        Command::Get { kind, id } => json!({"op":"get","kind":kind,"id":id}),
        Command::Create {
            kind,
            title,
            fields,
            id,
        } => {
            let mut r = json!({"op":"create","kind":kind,"title":title,"fields":parse(&fields)?});
            if let Some(id) = id {
                r["id"] = json!(id)
            }
            r
        }
        Command::Update {
            kind,
            id,
            title,
            fields,
            rev,
        } => {
            let mut r = json!({"op":"update","kind":kind,"id":id});
            if let Some(t) = title {
                r["title"] = json!(t);
            }
            if let Some(f) = fields {
                r["fields"] = parse(&f)?;
            }
            if let Some(n) = rev {
                r["expected_rev"] = json!(n);
            }
            r
        }
        Command::Delete { kind, id, rev } => {
            let mut r = json!({"op":"delete","kind":kind,"id":id});
            if let Some(n) = rev {
                r["expected_rev"] = json!(n);
            }
            r
        }
        Command::Search { query } => json!({"op":"search","query":query}),
        Command::GroupAvatar { id, path, rev } => {
            let mut file = shipforge_client::upload_file(&path)?;
            file["op"] = json!("chat.avatar.set");
            file["id"] = json!(id);
            file["expected_rev"] = json!(rev);
            file
        }
        Command::Upload {
            path,
            kind,
            id,
            media_type,
        } => {
            let mut request = shipforge_client::upload_file(&path)?;
            request["kind"] = json!(kind);
            if let Some(id) = id {
                request["id"] = json!(id);
            }
            if let Some(mime) = media_type {
                request["media_type"] = json!(mime);
            }
            request
        }
        Command::ShareImage { share, path } => {
            let key = share
                .trim_end_matches('/')
                .rsplit('/')
                .next()
                .unwrap_or(&share);
            let mut response = Client::from_dir(cli.data_dir.as_deref())?
                .execute(json!({"op":"chat.share.image","share":key}))?;
            shipforge_client::save_attachment(&path, &response)?;
            response.as_object_mut().unwrap().remove("content_base64");
            response["path"] = json!(path);
            return Ok(response);
        }
        Command::Download {
            kind,
            id,
            attachment_id,
            path,
            version,
        } => {
            let mut request = json!({"op":"attachment.download","space":cli.space,"kind":kind,"id":id,"attachment_id":attachment_id});
            if let Some(rev) = version {
                request["version"] = json!(rev);
            }
            let mut reply = Client::from_dir(cli.data_dir.as_deref())?.execute(request)?;
            shipforge_client::save_attachment(&path, &reply)?;
            if let Some(meta) = reply.as_object_mut() {
                meta.remove("content_base64");
            }
            reply["path"] = json!(path);
            return Ok(reply);
        }
        Command::Sync { action } => json!({"op":format!("sync.{action}")}),
    };
    if !body.is_object() {
        return Err("invalid: request must be a JSON object".into());
    }
    if body["op"]
        .as_str()
        .is_some_and(|op| shipforge_client::DEVICE_OPS.contains(&op))
    {
        let dir = match cli.data_dir {
            Some(dir) => dir,
            None => shipforge_client::data_dir()?,
        };
        return shipforge_client::device_command(&dir, &body);
    }
    if body.get("space").is_none() {
        body["space"] = json!(cli.space);
    }
    Client::from_dir(cli.data_dir.as_deref())?.execute(body)
}
