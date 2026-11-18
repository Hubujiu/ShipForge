mod auth;
mod github;
mod jira;
mod slack;

use auth::{Auth, Principal};
use clap::{Parser, Subcommand};
use serde_json::{Value, json};
use shipforge_core::Store;
use std::io::{Read, Write};
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use tiny_http::{Header, Method, Request, Response, Server, StatusCode};

#[derive(Parser)]
#[command(version, about = "ShipForge team workspace service")]
struct Args {
    #[arg(
        long,
        env = "SHIPFORGE_SERVER_DATA",
        global = true,
        default_value = "shipforge-team-data"
    )]
    data_dir: PathBuf,
    #[command(subcommand)]
    action: Action,
}
#[derive(Subcommand)]
enum Action {
    /// Serve the team API. Place TLS in front of network-facing deployments.
    Serve {
        #[arg(long, default_value = "127.0.0.1:47138")]
        listen: SocketAddr,
        /// JSON configuration of Slack Events API routes and local secret variable names.
        #[arg(long, env = "SHIPFORGE_SLACK_CONFIG")]
        slack_config: Option<PathBuf>,
        /// JSON routes for signed GitHub push and pull_request webhooks.
        #[arg(long, env = "SHIPFORGE_GITHUB_WEBHOOKS")]
        github_config: Option<PathBuf>,
        /// Organization exposed at /rest/api/2 and /rest/api/3 to Jira clients.
        #[arg(long, env = "SHIPFORGE_JIRA_ORGANIZATION")]
        jira_organization: Option<String>,
        /// Canonical HTTP(S) address of this Jira-compatible site (including a reverse proxy prefix).
        #[arg(long, env = "SHIPFORGE_JIRA_BASE_URL", requires = "jira_organization")]
        jira_base_url: Option<String>,
    },
    /// Manage bearer tokens on the server's own machine.
    Token {
        #[command(subcommand)]
        command: Token,
    },
}
#[derive(Subcommand)]
enum Token {
    Issue {
        #[arg(long)]
        actor: String,
        #[arg(long, default_value = "read")]
        scope: String,
        #[arg(long, default_value = "")]
        label: String,
        #[arg(long, default_value = "default")]
        space: String,
        #[arg(long)]
        resource: Option<String>,
        #[arg(long)]
        binding: Option<String>,
    },
    List,
    Revoke {
        id: String,
    },
}

fn main() -> ExitCode {
    match run(Args::parse()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("{err}");
            ExitCode::FAILURE
        }
    }
}
fn run(args: Args) -> Result<(), String> {
    let auth = Auth::open(&args.data_dir)?;
    match args.action {
        Action::Token { command } => {
            let answer = match command {
                Token::Issue {
                    actor,
                    scope,
                    label,
                    space,
                    resource,
                    binding,
                } => auth.issue(
                    &actor,
                    &scope,
                    &label,
                    &json!({"space":space,"resource":resource,"binding":binding}),
                )?,
                Token::List => auth.list()?,
                Token::Revoke { id } => auth.revoke(&id)?,
            };
            println!("{answer}");
            Ok(())
        }
        Action::Serve {
            listen,
            slack_config,
            github_config,
            jira_organization,
            jira_base_url,
        } => serve(
            &args.data_dir,
            listen,
            slack::Routes::load(slack_config.as_deref())?,
            github::Routes::load(github_config.as_deref())?,
            jira_organization,
            jira_base_url,
        ),
    }
}

fn serve(
    dir: &Path,
    addr: SocketAddr,
    routes: slack::Routes,
    github: github::Routes,
    jira_organization: Option<String>,
    jira_base_url: Option<String>,
) -> Result<(), String> {
    let database = dir.join("workspace.sqlite3");
    let server = Server::http(addr).map_err(|e| format!("server: {e}"))?;
    let mut scheduler = Store::open(&database)?;
    std::thread::Builder::new()
        .name("shipforge-iteration-clock".into())
        .spawn(move || {
            loop {
                if let Err(error) = scheduler.maintain_iterations() {
                    eprintln!("Iteration maintenance: {error}");
                }
                std::thread::sleep(std::time::Duration::from_secs(30));
            }
        })
        .map_err(|e| format!("server: {e}"))?;
    let jira = jira::Site::new(
        jira_organization,
        jira_base_url,
        &server.server_addr().to_string(),
    )?;
    // Migrate once, before accepting requests; use one connection per serving thread.
    let mut workers = Vec::new();
    for _ in 0..4 {
        workers.push((Store::open(&database)?, Auth::open(dir)?));
    }
    println!(
        "{}",
        json!({"listen":server.server_addr().to_string(),"protocol":1})
    );
    std::io::stdout().flush().map_err(|e| e.to_string())?;
    std::thread::scope(|scope| {
        for (mut store, auth) in workers {
            let server = &server;
            let routes = &routes;
            let github = &github;
            let jira = &jira;
            scope.spawn(move||{
                for mut request in server.incoming_requests() {
                    if request.url().starts_with("/rest/api/") {
                        jira.respond(request, &auth, &mut store);
                        continue;
                    }
                    let native_slack=request.url().starts_with("/v1/slack/");
                    let share=request.url().starts_with("/share/");
                    let answer=handle(&mut request,&auth,&mut store,routes,github);
                    let (status,body)=match answer {
                        Ok(data) if native_slack => (200,data),
                        Ok(data) if share => (200,data),
                        Ok(data)=>(200,json!({"protocol":1,"ok":true,"data":data})),
                        Err(error)=>{
                            let (code,message)=error.split_once(": ").unwrap_or(("server",&error));
                            let status=match code {"unauthorized"=>401,"forbidden"=>403,"not_found"=>404,"conflict"=>409,"too_large"=>413,"method"=>405,"invalid"=>400,"storage"|"server"=>500,_=>400};
                            let client_message=if status==500 {eprintln!("ShipForge: {error}");"workspace operation could not be completed"}else{message};
                            (status,json!({"protocol":1,"ok":false,"error":{"code":code,"message":client_message}}))
                        }
                    };
                    let (text,content_type)=if share && status==200 {(shipforge_core::share_text(&body),"text/plain; charset=utf-8")}else if native_slack && status==200 && body.is_null() {(String::new(),"text/plain; charset=utf-8")}else{(body.to_string(),"application/json; charset=utf-8")};
                    let response=Response::from_string(text).with_status_code(StatusCode(status))
                        .with_header(Header::from_bytes("Content-Type",content_type).unwrap())
                        .with_header(Header::from_bytes("Referrer-Policy","no-referrer").unwrap())
                        .with_header(Header::from_bytes("Cache-Control","no-store").unwrap())
                        .with_header(Header::from_bytes("X-Content-Type-Options","nosniff").unwrap());
                    let _=request.respond(response);
                }
            });
        }
    });
    Ok(())
}

fn handle(
    request: &mut Request,
    auth: &Auth,
    store: &mut Store,
    routes: &slack::Routes,
    github: &github::Routes,
) -> Result<Value, String> {
    if let Some(share) = request.url().strip_prefix("/share/") {
        if request.method() != &Method::Get {
            return Err("method: use GET".into());
        }
        let actor = request
            .headers()
            .iter()
            .find(|h| h.field.equiv("Authorization"))
            .map(|h| {
                let secret = h
                    .value
                    .as_str()
                    .strip_prefix("Bearer ")
                    .ok_or_else(|| "unauthorized: invalid bearer token".to_owned())?;
                let p = auth.authenticate(secret)?;
                auth::authorize(&p, &json!({"op":"chat.share.read"}))?;
                Ok::<_, String>(p.actor)
            })
            .transpose()?;
        return store.shared_conversation(share, actor.as_deref());
    }
    if request.method() == &Method::Get && request.url() == "/healthz" {
        return Ok(json!({"service":"ShipForge","version":env!("CARGO_PKG_VERSION"),"protocol":1}));
    }
    if request.method() != &Method::Post {
        return Err("method: use POST".into());
    }
    let path = request.url().to_owned();
    if let Some(route) = path.strip_prefix("/v1/github/") {
        return github.handle(route, request, auth, store);
    }
    if let Some(route) = path.strip_prefix("/v1/slack/") {
        return routes.handle(route, request, auth, store);
    }
    let integration_path = path.strip_prefix("/v1/integrations/");
    if path != "/v1/command" && !path.starts_with("/v1/hooks/") && integration_path.is_none() {
        return Err("not_found: route".into());
    }
    let bearer = request
        .headers()
        .iter()
        .find(|h| h.field.equiv("Authorization"))
        .and_then(|h| h.value.as_str().strip_prefix("Bearer "));
    let vendor_token = integration_path.and_then(|_| {
        request
            .headers()
            .iter()
            .find(|h| h.field.equiv("X-Gitlab-Token"))
            .map(|h| h.value.as_str())
    });
    let secret = bearer
        .or(vendor_token)
        .ok_or_else(|| "unauthorized: bearer token required".to_owned())?;
    let principal = auth.authenticate(secret)?;
    if vendor_token.is_some() && principal.scope != "hook" {
        return Err("forbidden: direct integration requires a bound hook token".into());
    }
    const MAX: usize = 8 * 1024 * 1024;
    if request.body_length().is_some_and(|n| n > MAX) {
        return Err("too_large: request exceeds 8 MiB".into());
    }
    let mut buffer = Vec::new();
    request
        .as_reader()
        .take(MAX as u64 + 1)
        .read_to_end(&mut buffer)
        .map_err(|_| "invalid: unable to read request".to_owned())?;
    if buffer.len() > MAX {
        return Err("too_large: request exceeds 8 MiB".into());
    }
    let mut body: Value = serde_json::from_slice(&buffer)
        .map_err(|_| "invalid: valid UTF-8 JSON required".to_owned())?;
    if !body.is_object() {
        return Err("invalid: request must be an object".into());
    }
    if let Some(route) = integration_path {
        let (space, connector) = split_target(route)?;
        let event = request
            .headers()
            .iter()
            .find(|h| {
                h.field.equiv("X-Gitlab-Event-UUID")
                    || h.field.equiv("X-ShipForge-Event")
                    || h.field.equiv("X-Atlassian-Webhook-Identifier")
            })
            .map(|h| h.value.as_str().to_owned());
        if body.get("payload").is_none() {
            body = json!({"payload":body});
        }
        if let Some(event) = event {
            body["event_id"] = json!(event);
        }
        body["op"] = json!("provider.ingest");
        body["space"] = json!(space);
        body["id"] = json!(connector);
    } else if let Some(route) = path.strip_prefix("/v1/hooks/") {
        let (space, rule) = split_target(route)?;
        body["op"] = json!("automation.fire");
        body["source"] = json!("webhook");
        body["space"] = json!(space);
        body["id"] = json!(rule);
    }
    dispatch(auth, &principal, store, body)
}
fn split_target(route: &str) -> Result<(&str, &str), String> {
    let (space, target) = route
        .split_once('/')
        .ok_or("invalid: expected space and target ID")?;
    if space.is_empty() || target.is_empty() || target.contains('/') {
        return Err("invalid: incorrect hook route".into());
    }
    Ok((space, target))
}
fn dispatch(
    auth: &Auth,
    p: &Principal,
    store: &mut Store,
    mut body: Value,
) -> Result<Value, String> {
    let op = body["op"]
        .as_str()
        .ok_or_else(|| "invalid: op is required".to_owned())?;
    if op.starts_with("access.") {
        return auth::execute(auth, p, &body);
    }
    auth::authorize(p, &body)?;
    if op == "automation.fire" && p.scope == "hook" {
        body["source"] = json!("webhook");
    } else if op == "provider.ingest" && body.get("provider").is_none() {
        let connector = store.execute(
            &p.actor,
            json!({"op":"get","kind":"connector","space":body["space"],"id":body["id"]}),
        )?;
        body["provider"] = connector["fields"]["provider"].clone();
    }
    store.execute(&p.actor, body)
}
