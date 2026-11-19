mod config;
mod github;
mod host;
mod mcp;
mod process;
mod slack;
use clap::{Parser, Subcommand};
use config::{Config, Result};
use serde_json::json;
use shipforge_client::Client;
use std::io::Read;
use std::path::{Path, PathBuf};

#[derive(Parser)]
#[command(
    version,
    about = "Launch approved external runtimes and synchronize ShipForge integrations"
)]
struct Options {
    #[command(subcommand)]
    command: Action,
}
#[derive(Subcommand)]
enum Action {
    /// Create a new machine configuration. Existing files are preserved.
    Init {
        #[arg(long)]
        config: PathBuf,
        #[arg(long, default_value = "default")]
        space: String,
        #[arg(long)]
        workspace: Option<PathBuf>,
        #[arg(long)]
        program: Option<PathBuf>,
        #[arg(long, default_value = "local-tool")]
        runtime: String,
        #[arg(long = "arg", allow_hyphen_values = true)]
        args: Vec<String>,
        /// Add the deterministic external context-inspection runtime for an end-to-end check.
        #[arg(long)]
        example: bool,
    },
    /// Register the machine and execute configured runtimes. Once drains one batch.
    Run {
        #[arg(long)]
        config: PathBuf,
        #[arg(long)]
        once: bool,
    },
    /// Ask the local supervisor to finish active jobs and exit.
    Stop {
        #[arg(long)]
        config: PathBuf,
    },
    /// Call a document or an approved local MCP tool as an agent.
    ToolCall {
        #[arg(long)]
        config: PathBuf,
        #[arg(long)]
        agent: String,
        #[arg(long)]
        tool: String,
        #[arg(long)]
        arguments_file: Option<PathBuf>,
    },
    /// Report the locations of common installed runtime executables.
    Probe,
    /// Deliver queued public task updates to their Slack threads, once or continuously.
    SlackDeliver {
        #[arg(long, default_value = "default")]
        space: String,
        #[arg(long)]
        connector: String,
        #[arg(long, default_value = "SHIPFORGE_SLACK_BOT_TOKEN")]
        token_env: String,
        #[arg(long)]
        watch: bool,
    },
    /// Deliver mapped Bugly status changes from the durable queue.
    BuglyDeliver {
        #[arg(long, default_value = "default")]
        space: String,
        #[arg(long)]
        connector: String,
        #[arg(long)]
        watch: bool,
    },
    /// Pull an incremental batch from a ZenTao or PingCode connector on this machine.
    TrackerSync {
        #[arg(long, default_value = "default")]
        space: String,
        #[arg(long)]
        connector: String,
        #[arg(long)]
        watch: bool,
        #[arg(long,default_value_t=300,value_parser=clap::value_parser!(u64).range(15..=86400))]
        interval: u64,
    },
    /// Import new and changed GitHub issues through an existing github connector.
    GithubSync {
        #[arg(long)]
        config: PathBuf,
        #[arg(long)]
        connector: String,
        /// Import just one issue by number.
        #[arg(long)]
        issue: Option<u64>,
    },
    /// Explicitly send one locally updated mapped issue to GitHub.
    GithubPush {
        #[arg(long)]
        config: PathBuf,
        #[arg(long)]
        connector: String,
        #[arg(long)]
        task: String,
    },
    /// Read one JSON run context on stdin and print a deterministic external runtime result.
    InspectContext,
}
fn execute() -> Result<serde_json::Value> {
    match Options::parse().command {
        Action::Init {
            config,
            workspace,
            program,
            runtime,
            args,
            example,
            space,
        } => {
            Config::initialize(&config, workspace, program, &runtime, args, example, space)?;
            Ok(json!({"config":config}))
        }
        Action::Run { config, once } => host::run(&config, once),
        Action::Stop { config } => host::request_stop(&config),
        Action::ToolCall {
            config,
            agent,
            tool,
            arguments_file,
        } => {
            let cfg = Config::load(&config)?;
            let arguments = match arguments_file {
                Some(p) => serde_json::from_slice(&std::fs::read(p).map_err(|e| e.to_string())?)
                    .map_err(|e| e.to_string())?,
                None => json!({}),
            };
            let descriptor = Client::from_env()?.execute(
                json!({"op":"tool.authorize","space":cfg.space,"id":agent,"tool_id":tool}),
            )?;
            let dir = cfg.workspace.clone().unwrap_or(config::state_dir(&config)?);
            mcp::call(&cfg, &dir, &tool, &descriptor, arguments)
        }
        Action::SlackDeliver {
            space,
            connector,
            token_env,
            watch,
        } => slack::run(
            &mut Client::from_env()?,
            &space,
            &connector,
            &token_env,
            watch,
        ),
        Action::Probe => Ok(json!(
            ["git", "codex", "claude", "gemini", "node", "python"]
                .iter()
                .map(|s| json!({"name":s,"program":config::program_path(Path::new(s)).ok()}))
                .collect::<Vec<_>>()
        )),
        Action::BuglyDeliver {
            space,
            connector,
            watch,
        } => {
            let mut client = Client::from_env()?;
            loop {
                let outcome =
                    client.execute(json!({"op":"bugly.deliver","space":space,"id":connector}));
                if !watch {
                    return outcome;
                }
                if let Err(error) = outcome {
                    eprintln!("{error}");
                }
                std::thread::sleep(std::time::Duration::from_secs(15));
            }
        }
        Action::TrackerSync {
            space,
            connector,
            watch,
            interval,
        } => {
            let mut client = Client::from_env()?;
            loop {
                let result = client
                    .execute(json!({"op":"imports.synchronize","space":space,"id":connector}));
                if !watch {
                    return result;
                }
                match result {
                    Ok(data) => println!(
                        "{}",
                        json!({"state":data["state"],"batch":data["batch"],"counts":data["counts"]})
                    ),
                    Err(error) => eprintln!("{error}"),
                }
                std::thread::sleep(std::time::Duration::from_secs(interval));
            }
        }
        Action::GithubSync {
            config,
            connector,
            issue,
        } => github::sync(
            &mut Client::from_env()?,
            &Config::load(&config)?.space,
            &connector,
            issue,
        ),
        Action::GithubPush {
            config,
            connector,
            task,
        } => github::push(
            &mut Client::from_env()?,
            &Config::load(&config)?.space,
            &connector,
            &task,
        ),
        Action::InspectContext => {
            let mut bytes = Vec::new();
            std::io::stdin()
                .take(5 * 1024 * 1024)
                .read_to_end(&mut bytes)
                .map_err(|e| e.to_string())?;
            let runtime: serde_json::Value =
                serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
            Ok(
                json!({"status":"done","result":format!("Context received for {}",runtime["title"].as_str().unwrap_or("(untitled)")),"artifacts":[]}),
            )
        }
    }
}
fn main() {
    match execute() {
        Ok(result) => println!("{result}"),
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1)
        }
    }
}
