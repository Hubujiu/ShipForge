use crate::catalog::Record;
use serde_json::{Value, json};
use shipforge_client::Client;
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    sync::mpsc::{self, Receiver, Sender},
};

pub type Rows = BTreeMap<String, Vec<Record>>;
pub enum Request {
    Attachment {
        ticket: u64,
        command: Value,
        path: PathBuf,
    },
    Machine {
        ticket: u64,
        space: String,
        fields: Value,
        stop: bool,
    },
    Read {
        generation: u64,
        space: String,
        kinds: Vec<String>,
    },
    Call {
        ticket: u64,
        command: Value,
    },
    Connect {
        url: String,
        token: String,
        ticket: u64,
    },
    Disconnect {
        ticket: u64,
    },
    Export {
        ticket: u64,
        command: Value,
        path: PathBuf,
    },
    Import {
        ticket: u64,
        command: Value,
        path: PathBuf,
    },
}
pub enum Event {
    Ready(Result<String, String>),
    Rows {
        generation: u64,
        space: String,
        data: Rows,
        errors: BTreeMap<String, String>,
    },
    Reply {
        ticket: u64,
        result: Result<Value, String>,
    },
}
pub struct Backend {
    pub sender: Sender<Request>,
    pub results: Receiver<Event>,
}
impl Backend {
    pub fn open(dir: Option<PathBuf>, demo: bool) -> Self {
        let (sender, requests) = mpsc::channel();
        let (events, results) = mpsc::channel();
        std::thread::Builder::new()
            .name("shipforge-workspace".into())
            .spawn(move || {
                let mut client = if demo {
                    Client::memory()
                } else {
                    Client::from_dir(dir.as_deref())
                };
                if demo {
                    client = client.and_then(|mut c| {
                        seed_demo(&mut c)?;
                        Ok(c)
                    });
                }
                let report = |client: &Result<Client, String>| {
                    client
                        .as_ref()
                        .map(|c| if demo { "demo" } else { c.mode() }.to_owned())
                        .map_err(Clone::clone)
                };
                if events.send(Event::Ready(report(&client))).is_err() {
                    return;
                }
                while let Ok(request) = requests.recv() {
                    match request {
                        Request::Machine {
                            ticket,
                            space,
                            fields,
                            stop,
                        } => {
                            let result = if demo {
                                Err("演示模式不连接本机工作机。".into())
                            } else {
                                crate::machine::control(dir.as_deref(), &space, &fields, stop)
                            };
                            let _ = events.send(Event::Reply { ticket, result });
                        }
                        Request::Read {
                            generation,
                            space,
                            kinds,
                        } => {
                            let mut data = Rows::new();
                            let mut errors = BTreeMap::new();
                            for kind in kinds {
                                let command = if kind == "catalog" {
                                    json!({"op":"catalog.list","space":space})
                                } else if kind == "space" {
                                    json!({"op":"space.list"})
                                } else {
                                    json!({"op":"list","kind":kind,"space":space})
                                };
                                let result = execute(&mut client, command).and_then(|value| {
                                    serde_json::from_value::<Vec<Record>>(value)
                                        .map_err(|e| e.to_string())
                                });
                                match result {
                                    Ok(records) => {
                                        data.insert(kind, records);
                                    }
                                    Err(error) => {
                                        errors.insert(kind, error);
                                    }
                                }
                            }
                            if let Ok(c) = &mut client
                                && c.mode() == "team"
                                && let Ok(sync) = c.execute(json!({"op":"sync.status"}))
                                && sync["online"] == true
                                && sync["pending"].as_u64().is_some_and(|n| n > 0)
                                && let Err(msg) = c.execute(json!({"op":"sync.flush"}))
                            {
                                errors.insert("sync".into(), msg);
                            }
                            if events
                                .send(Event::Rows {
                                    generation,
                                    space,
                                    data,
                                    errors,
                                })
                                .is_err()
                            {
                                break;
                            }
                        }
                        Request::Call {
                            ticket,
                            mut command,
                        } => {
                            if matches!(
                                command["op"].as_str(),
                                Some("create" | "update" | "delete")
                            ) && matches!(
                                command["kind"].as_str(),
                                Some(
                                    "task"
                                        | "document"
                                        | "comment"
                                        | "view"
                                        | "project"
                                        | "iteration"
                                        | "message"
                                )
                            ) && (command["op"] == "create" || command["expected_rev"].is_i64())
                                && client.as_ref().is_ok_and(|c| c.mode() == "team")
                            {
                                command["queue_if_offline"] = json!(true);
                            }
                            let result = execute(&mut client, command);
                            if events.send(Event::Reply { ticket, result }).is_err() {
                                break;
                            }
                        }
                        Request::Connect { url, token, ticket } => {
                            let result = if demo {
                                Err("演示空间使用内存数据。请打开正式工作台连接团队。".to_owned())
                            } else {
                                dir.clone()
                                    .map(Ok)
                                    .unwrap_or_else(shipforge_client::data_dir)
                                    .and_then(|directory| {
                                        Client::remote_in(&directory, &url, &token)
                                    })
                                    .and_then(|mut new| {
                                        new.execute(json!({"op":"space.list"}))?;
                                        Client::save_connection(dir.as_deref(), &url, &token)?;
                                        client = Ok(new);
                                        Ok(Value::Null)
                                    })
                            };
                            if result.is_ok() {
                                let _ = events.send(Event::Ready(report(&client)));
                            }
                            let _ = events.send(Event::Reply { ticket, result });
                        }
                        Request::Disconnect { ticket } => {
                            let result = Client::forget_connection(dir.as_deref())
                                .and_then(|()| Client::from_dir(dir.as_deref()))
                                .map(|new| {
                                    client = Ok(new);
                                    Value::Null
                                });
                            if result.is_ok() {
                                let _ = events.send(Event::Ready(report(&client)));
                            }
                            let _ = events.send(Event::Reply { ticket, result });
                        }
                        Request::Attachment {
                            ticket,
                            command,
                            path,
                        } => {
                            let result = client
                                .as_mut()
                                .map_err(|e| e.clone())
                                .and_then(|c| crate::files::attachment(c, command, &path));
                            let _ = events.send(Event::Reply { ticket, result });
                        }
                        Request::Export {
                            ticket,
                            command,
                            path,
                        } => {
                            let result = execute(&mut client, command).and_then(|value| {
                                let file = fs::OpenOptions::new()
                                    .create_new(true)
                                    .write(true)
                                    .open(&path)
                                    .map_err(|e| format!("写入 {} 失败：{e}", path.display()))?;
                                if let Err(e) = serde_json::to_writer_pretty(file, &value) {
                                    let _ = fs::remove_file(&path);
                                    return Err(e.to_string());
                                }
                                Ok(json!({"path":path}))
                            });
                            let _ = events.send(Event::Reply { ticket, result });
                        }
                        Request::Import {
                            ticket,
                            mut command,
                            path,
                        } => {
                            let result = read_json(&path).and_then(|data| {
                                command["data"] = data;
                                execute(&mut client, command)
                            });
                            let _ = events.send(Event::Reply { ticket, result });
                        }
                    }
                }
            })
            .expect("启动工作区读写线程");
        Self { sender, results }
    }
}
fn execute(client: &mut Result<Client, String>, command: Value) -> Result<Value, String> {
    client.as_mut().map_err(|e| e.clone())?.execute(command)
}
fn read_json(path: &Path) -> Result<Value, String> {
    let data = fs::read(path).map_err(|e| format!("读取 {} 失败：{e}", path.display()))?;
    serde_json::from_slice(&data).map_err(|e| e.to_string())
}
fn seed_demo(client: &mut Client) -> Result<(), String> {
    let entries: Vec<Value> =
        serde_json::from_str(include_str!("../assets/demo.json")).map_err(|e| e.to_string())?;
    for entry in entries {
        client.execute(entry)?;
    }
    Ok(())
}
