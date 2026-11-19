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
        people: (&'static str, String),
        handler: Option<(String, String)>,
        chat: Option<String>,
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
        account: Option<String>,
        ticket: u64,
    },
    RememberAccount(Value),
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
    Ready(Result<(String, Value), String>),
    Rows {
        generation: u64,
        space: String,
        access: Option<Result<String, String>>,
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
                let report = |client: &mut Result<Client, String>| {
                    let c=client.as_mut().map_err(|e|e.clone())?;
                    let session=if demo{Value::Null}else{c.desktop_session()?};
                    Ok((if demo { "demo" } else { c.mode() }.to_owned(),session))
                };
                if events.send(Event::Ready(report(&mut client))).is_err() {
                    return;
                }
                while let Ok(request) = requests.recv() {
                    match request {
                        Request::RememberAccount(command)=>{
                            // A stale window cannot overwrite the active account's saved route.
                            let _=execute(&mut client,command);
                        }
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
                            people,
                            handler,
                            chat,
                            generation,
                            space,
                            mut kinds,
                        } => {
                            let mut data = Rows::new();
                            let mut errors = BTreeMap::new();
                            let access = client.as_ref().is_ok_and(|c| c.mode()=="team").then(|| {
                                execute(&mut client,json!({"op":"whoami","space":space})).and_then(|reply|
                                    reply["role"].as_str().map(str::to_owned).ok_or_else(||"invalid: missing space role".into()))
                            });
                            if matches!(&access,Some(Err(e)) if !e.starts_with("offline:")) {
                                kinds.retain(|kind| kind=="space");
                            }
                            for kind in kinds {
                                let command = if kind=="people_directory" {
                                    let mut q=json!({"op":"directory.people","space":space,"query":people.1,"limit":5000});
                                    if !people.0.is_empty(){q["kind"]=json!(people.0);}
                                    q
                                } else if kind=="assistant_home" {
                                    let mut q=json!({"op":"assistant.history","space":space});
                                    if chat.is_some(){q["room_scope"]=json!(space);}
                                    q
                                } else if kind=="status_library" {
                                    json!({"op":"status.library","space":space})
                                } else if kind=="organization_settings" {
                                    json!({"op":"organization.settings","space":space})
                                } else if kind == "catalog" {
                                    json!({"op":"catalog.list","space":space})
                                } else if kind=="message" && let Some(id)=chat.as_ref(){
                                    json!({"op":"chat.history","space":space,"id":id})
                                } else if matches!(kind.as_str(),"agent_settings"|"agent_history") && let Some((k,id))=&handler{
                                    json!({"op":if kind=="agent_settings"{"agent.settings"}else{"agent.history"},"space":space,"kind":k,"id":id})
                                } else if matches!(kind.as_str(),"agent_directory"|"agent_archived"|"crew_directory"|"crew_archived"){
                                    json!({"op":"agent.directory","kind":if kind.starts_with("crew"){"crew"}else{"agent"},"scope":"organization","space":space,"view":if kind.ends_with("archived"){"archived"}else{"all"}})
                                } else if kind == "chat_sidebar" {
                                    json!({"op":"chat.sidebar","space":space})
                                } else if kind == "chat_invitation" {
                                    json!({"op":"chat.invitations","space":space})
                                } else if kind == "member_invitation" {
                                    json!({"op":"invitation.list","space":space})
                                } else if kind == "space" {
                                    json!({"op":"space.list"})
                                } else if kind == "iteration" {
                                    json!({"op":"iteration.timeline","space":space})
                                } else if kind=="view" {
                                    json!({"op":"view.catalog","space":space})
                                } else if kind == "iteration_settings" {
                                    json!({"op":"iteration.settings","space":space})
                                } else {
                                    json!({"op":"list","kind":kind,"space":space})
                                };
                                let result = execute(&mut client, command).and_then(|value| {
                                    let value = if kind=="status_library" {
                                        json!(value["statuses"].as_array().into_iter().flatten().map(|row|{
                                            let mut fields=row["fields"].clone();
                                            fields["usage"]=row["usage"].clone();
                                            fields["local_id"]=row["local_id"].clone();
                                            fields["can_manage"]=value["can_manage"].clone();
                                            json!({"kind":kind,"id":row["id"],"title":row["title"],"space":space,"rev":row["rev"],"fields":fields})
                                        }).collect::<Vec<_>>())
                                    } else if kind=="assistant_home" {
                                        json!([{"kind":kind,"id":"personal","space":space,"title":value["title"],"fields":value}])
                                    } else if kind=="organization_settings" {
                                        json!([{"kind":kind,"id":value["id"],"space":space,"title":value["name"],"rev":value["rev"],"fields":value}])
                                    }else if kind=="agent_settings" && let Some((_,id))=&handler{
                                        json!([{"kind":"agent_settings","id":id,"space":space,"fields":value}])
                                    }else if kind=="agent_history"{
                                        json!(value.as_array().into_iter().flatten().enumerate().map(|(i,v)|json!({"kind":"agent_history","id":i.to_string(),"space":space,"title":v["title"],"fields":v})).collect::<Vec<_>>())
                                    }else if kind == "iteration_settings" {
                                        json!([value])
                                    } else {
                                        value
                                    };
                                    serde_json::from_value::<Vec<Record>>(value)
                                        .map_err(|e| e.to_string())
                                });
                                match result {
                                    Ok(mut records) => {
                                        if let Some((k,id))=&handler && k==&kind && !records.iter().any(|r|&r.id==id) && let Ok(record)=execute(&mut client,json!({"op":"get","kind":k,"space":space,"id":id})) && let Ok(record)=serde_json::from_value(record){records.push(record);}
                                        if kind=="chat" && let Some(id)=chat.as_ref() && !records.iter().any(|r|&r.id==id)
                                            && let Ok(room)=execute(&mut client,json!({"op":"get","kind":"chat","space":space,"id":id}))
                                            && let Ok(room)=serde_json::from_value(room){records.push(room);}
                                        data.insert(kind, records);
                                    }
                                    Err(error) => {
                                        errors.insert(kind, error);
                                    }
                                }
                            }
                            if let Ok(c) = &mut client
                                && c.mode()=="team"
                                && (errors.values().any(|e|e.starts_with("unauthorized:"))
                                    || matches!(&access,Some(Err(e)) if e.starts_with("unauthorized:")))
                                && std::env::var_os("SHIPFORGE_SERVER").is_none()
                                && let Ok((mode,session))=report(&mut client)
                                && session["needs_login"]==true
                            {
                                let _=events.send(Event::Ready(Ok((mode,session))));
                                continue;
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
                                    access,
                                    data,
                                    errors,
                                })
                                .is_err()
                            {
                                break;
                            }
                        }
                        Request::Call {ticket, command} if !demo && client.is_err() && command["op"].as_str().is_some_and(|op|shipforge_client::DEVICE_OPS.contains(&op))=>{
                            let result=dir.clone().map(Ok).unwrap_or_else(shipforge_client::data_dir)
                                .and_then(|p|shipforge_client::device_command(&p,&command));
                            let _=events.send(Event::Reply{ticket,result});
                        }
                        Request::Call {ticket, command} if !demo && matches!(command["op"].as_str(),Some("bot.qr.check"|"bot.sync"))=>{
                            let directory=dir.clone();let results=events.clone();
                            std::thread::spawn(move||{let result=Client::from_dir(directory.as_deref()).and_then(|mut c|c.execute(command));let _=results.send(Event::Reply{ticket,result});});
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
                            let profile_change=matches!(command["op"].as_str(),Some("profiles.use"|"profiles.forget"|"profiles.login.finish"|"profiles.logout"|"profiles.close.finish"));
                            let result = execute(&mut client, command);
                            if profile_change && result.is_ok(){let _=events.send(Event::Ready(report(&mut client)));}
                            if events.send(Event::Reply { ticket, result }).is_err() {
                                break;
                            }
                        }
                        Request::Connect { url, token, account, ticket } => {
                            let result = if demo {
                                Err("演示空间使用内存数据。请打开正式工作台连接团队。".to_owned())
                            } else {
                                dir.clone()
                                    .map(Ok)
                                    .unwrap_or_else(shipforge_client::data_dir)
                                    .and_then(|directory| {
                                        Client::remote_in(&directory, &url, &token)
                                    })
                                    .and_then(|new| {
                                        Client::sign_in(dir.as_deref(), &url, &token,account.as_deref())?;
                                        client = Ok(new);
                                        Ok(Value::Null)
                                    })
                            };
                            if result.is_ok() {
                                let _ = events.send(Event::Ready(report(&mut client)));
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
                                let _ = events.send(Event::Ready(report(&mut client)));
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
