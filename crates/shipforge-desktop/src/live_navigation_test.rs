use super::{
    UiAction, Workbench,
    onboarding_ui::{activate_accessibility, buttons, settle},
};
use crate::{
    catalog::Page,
    controls::Field,
    forms::{Control, Mode},
};
use gpui_kit::{prelude::*, *};
use serde_json::{Value, json};
use shipforge_client::Client;
use std::{
    io::{BufRead, BufReader},
    os::windows::process::CommandExt,
    path::Path,
    process::{Child, Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
use tiny_http::{Response, Server};

struct Team(Child);
impl Drop for Team {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
fn team_command(db: &Path) -> Command {
    let exe = std::env::current_exe().unwrap();
    let mut cmd = Command::new(
        exe.parent()
            .unwrap()
            .parent()
            .unwrap()
            .join("shipforge-server.exe"),
    );
    cmd.creation_flags(0x08000000).arg("--data-dir").arg(db);
    cmd
}
fn wait_for(
    view: &Entity<Workbench>,
    w: &mut Window,
    cx: &mut App,
    description: &str,
    check: impl Fn(&Workbench) -> bool,
) {
    let start = Instant::now();
    loop {
        view.update(cx, |v, cx| v.tick(w, cx));
        if check(view.read(cx)) {
            break;
        }
        assert!(
            start.elapsed() < Duration::from_secs(14),
            "timed out waiting for {description}"
        );
        std::thread::sleep(Duration::from_millis(25));
    }
    w.refresh();
    w.draw(cx).clear(cx);
}
fn act(view: &Entity<Workbench>, action: UiAction, w: &mut Window, cx: &mut App) {
    view.update(cx, |v, cx| v.act(action, w, cx));
    settle(view, w, cx);
}
#[::core::prelude::v1::test]
#[ignore = "Exercises Windows proxy drafts, saving a device policy and opening the getting-started guide from About"]
fn hidden_device_network_and_about() {
    use super::device_ui::Action;
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().to_owned();
    let mut client = Client::local(&dir).unwrap();
    client.execute(json!({"op":"onboarding.setup","organization_name":"Network","organization_slug":"device-test","work_type":"software","task_types":["task"]})).unwrap();
    let server = Server::http("127.0.0.1:0").unwrap();
    let target = format!("http://{}/local", server.server_addr());
    let handler = std::thread::spawn(move || {
        let req = server
            .recv_timeout(Duration::from_secs(20))
            .unwrap()
            .unwrap();
        assert_eq!(req.method(), &tiny_http::Method::Head);
        assert_eq!(req.url(), "/local");
        req.respond(Response::from_string("").with_status_code(204))
            .unwrap();
    });
    gpui_kit::application()
        .with_assets(crate::assets::Assets)
        .run(move |cx| {
            gpui_kit::init(cx);
            let (window, view) = gpui_kit::open_window(
                WindowOptions {
                    focus: false,
                    show: false,
                    window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                        None,
                        size(px(1420.), px(1080.)),
                        cx,
                    ))),
                    ..Default::default()
                },
                cx,
                |w, cx| cx.new(|cx| Workbench::new(Some(dir.clone()), false, false, w, cx)),
            )
            .unwrap();
            window
                .update(cx, |_, w, cx| {
                    activate_accessibility(w);
                    settle(&view, w, cx);
                    act(&view, UiAction::Go(Page::Settings), w, cx);
                    assert!(buttons(w).iter().any(|s| s == "网络代理"));
                    assert!(buttons(w).iter().any(|s| s == "关于 ShipForge"));
                    act(&view, UiAction::Device(Action::Settings), w, cx);
                    assert!(buttons(w).iter().any(|s| s == "测试连接"));
                    act(&view, UiAction::Choose(0, "manual".into()), w, cx);
                    fill(&view, "endpoint", "http://127.0.0.1:9", w, cx);
                    fill(&view, "test_url", &target, w, cx);
                    act(&view, UiAction::Device(Action::Test), w, cx);
                    wait_for(&view, w, cx, "loopback probe", |v| {
                        v.form
                            .as_ref()
                            .and_then(|f| f.preview.as_ref())
                            .is_some_and(|p| p["status"] == 204)
                    });
                    assert_eq!(
                        client.execute(json!({"op":"network.settings"})).unwrap()["rev"],
                        0
                    );
                    act(&view, UiAction::Submit, w, cx);
                    assert_eq!(
                        client.execute(json!({"op":"network.settings"})).unwrap()["mode"],
                        "manual"
                    );
                    act(&view, UiAction::Device(Action::Settings), w, cx);
                    assert_eq!(
                        view.read(cx).form.as_ref().unwrap().entries[0].raw(cx),
                        "manual"
                    );
                    act(&view, UiAction::Choose(0, "direct".into()), w, cx);
                    act(&view, UiAction::Submit, w, cx);
                    act(&view, UiAction::Device(Action::About), w, cx);
                    assert!(buttons(w).iter().any(|s| s == "检查更新"));
                    assert!(buttons(w).iter().any(|s| s == "使用手册"));
                    act(&view, UiAction::Device(Action::GettingStarted), w, cx);
                    assert!(view.read(cx).form.is_none());
                    assert_eq!(view.read(cx).loc.page, Page::GettingStarted);
                })
                .unwrap();
            cx.quit();
        });
    handler.join().unwrap();
}
fn fill(view: &Entity<Workbench>, key: &str, value: &str, w: &mut Window, cx: &mut App) {
    let entry = view
        .read(cx)
        .form
        .as_ref()
        .unwrap()
        .entries
        .iter()
        .find(|e| e.spec.key == key)
        .unwrap();
    match &entry.control {
        Control::Text(Field::Single(input)) => {
            input.clone().update(cx, |s, cx| s.set_value(value, w, cx))
        }
        Control::Text(Field::Multi(input)) => {
            input.clone().update(cx, |s, cx| s.set_value(value, w, cx))
        }
        _ => panic!("text input expected"),
    }
}

#[::core::prelude::v1::test]
#[ignore = "Runs two hidden Windows GPUI windows with separate authenticated clients and an interruptible HTTP connection"]
fn hidden_two_windows_reconnect_directory_and_revocation() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("team");
    let a = tmp.path().join("device-a");
    let b = tmp.path().join("device-b");
    let output = team_command(&root)
        .args(["token", "issue", "--actor", "owner", "--scope", "write"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let owner: Value = serde_json::from_slice(&output.stdout).unwrap();
    let mut team = Team(
        team_command(&root)
            .args(["serve", "--listen", "127.0.0.1:0"])
            .stdout(Stdio::piped())
            .spawn()
            .unwrap(),
    );
    let mut line = String::new();
    BufReader::new(team.0.stdout.take().unwrap())
        .read_line(&mut line)
        .unwrap();
    let url = format!(
        "http://{}",
        serde_json::from_str::<Value>(&line).unwrap()["listen"]
            .as_str()
            .unwrap()
    );
    let mut admin = Client::remote_in(tmp.path(), &url, owner["token"].as_str().unwrap()).unwrap();
    admin.execute(json!({"op":"onboarding.setup","organization_name":"Native QA","organization_slug":"native-qa","work_type":"software","task_types":["task"]})).unwrap();
    admin.execute(json!({"op":"create","kind":"space","id":"side","title":"Second space","fields":{"visibility":"private"}})).unwrap();
    let relay = Server::http("127.0.0.1:0").unwrap();
    let relay_url = format!("http://{}", relay.server_addr());
    let available = Arc::new(AtomicBool::new(true));
    let up = available.clone();
    let upstream = url.clone();
    let finished = Arc::new(AtomicBool::new(false));
    let end = finished.clone();
    let proxy = std::thread::spawn(move || {
        let http = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .build()
            .new_agent();
        while !end.load(Ordering::SeqCst) {
            let Some(mut req) = relay.recv_timeout(Duration::from_millis(80)).unwrap() else {
                continue;
            };
            if !up.load(Ordering::SeqCst) {
                let _ = req.respond(Response::empty(503));
                continue;
            }
            let credential = req
                .headers()
                .iter()
                .find(|h| h.field.equiv("Authorization"))
                .unwrap()
                .value
                .as_str()
                .to_owned();
            let body: Value = serde_json::from_reader(req.as_reader()).unwrap();
            let mut reply = http
                .post(format!("{upstream}/v1/command"))
                .header("Authorization", credential)
                .send_json(body)
                .unwrap();
            let status = reply.status().as_u16();
            let _ = req.respond(
                Response::from_string(reply.body_mut().read_to_string().unwrap())
                    .with_status_code(status),
            );
        }
    });
    for (id, device, endpoint) in [("alice", &a, &url), ("bob", &b, &relay_url)] {
        for scope in ["default", "side"] {
            admin.execute(json!({"op":"create","kind":"member","space":scope,"id":id,"title":"Same name","fields":{"role":"member"}})).unwrap();
        }
        let cred = admin
            .execute(json!({"op":"access.issue","actor":id,"scope":"write"}))
            .unwrap();
        Client::save_connection(Some(device), endpoint, cred["token"].as_str().unwrap()).unwrap();
    }
    for scope in ["default", "side"] {
        admin.execute(json!({"op":"create","kind":"task","space":scope,"id":"target","title":"Search twin"})).unwrap();
    }
    gpui_kit::application()
        .with_assets(crate::assets::Assets)
        .run(move |cx| {
            gpui_kit::init(cx);
            let open = |dir: &Path, cx: &mut App| {
                gpui_kit::open_window(
                    WindowOptions {
                        show: false,
                        focus: false,
                        window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                            None,
                            size(px(1380.), px(1100.)),
                            cx,
                        ))),
                        ..Default::default()
                    },
                    cx,
                    |w, cx| {
                        cx.new(|cx| Workbench::new(Some(dir.to_path_buf()), false, false, w, cx))
                    },
                )
                .unwrap()
            };
            let (wa, alice) = open(&a, cx);
            let (wb, bob) = open(&b, cx);
            wa.update(cx, |_, w, cx| {
                activate_accessibility(w);
                settle(&alice, w, cx);
                act(&alice, UiAction::Search, w, cx);
                fill(&alice, "query", "Search twin", w, cx);
                act(&alice, UiAction::Submit, w, cx);
                let rows = &alice.read(cx).form.as_ref().unwrap().search_results;
                assert_eq!(rows.len(), 2);
                let target = rows.iter().find(|r| r.space == "side").unwrap().clone();
                act(&alice, UiAction::OpenSearch(target), w, cx);
                assert_eq!(alice.read(cx).loc.space, "side");
                act(&alice, UiAction::Form(Mode::QuickCreate), w, cx);
                for label in ["新建任务", "与真人对话", "与 Agent 对话", "新建群聊"]
                {
                    assert!(buttons(w).iter().any(|l| l == label));
                }
                act(&alice, UiAction::Form(Mode::StartChat("member")), w, cx);
                act(&alice, UiAction::Choose(0, "bob".into()), w, cx);
                act(&alice, UiAction::Submit, w, cx);
                assert_eq!(alice.read(cx).loc.selected.as_ref().unwrap().kind, "chat");
            })
            .unwrap();
            let room = alice.read(cx).loc.selected.as_ref().unwrap().id.clone();
            wb.update(cx, |_, w, cx| {
                activate_accessibility(w);
                settle(&bob, w, cx);
                act(&bob, UiAction::People, w, cx);
                act(&bob, UiAction::SetPeople("member"), w, cx);
                act(&bob, UiAction::Form(Mode::PeopleFilter), w, cx);
                fill(&bob, "query", "Same name", w, cx);
                act(&bob, UiAction::Submit, w, cx);
                let peers = bob.read(cx).list("people_directory");
                assert_eq!(peers.iter().filter(|r| r.id == "alice").count(), 2);
                let peer = peers
                    .iter()
                    .find(|r| r.space == "side" && r.id == "alice")
                    .unwrap()
                    .clone();
                act(&bob, UiAction::OpenSearch(peer), w, cx);
                assert!(buttons(w).iter().any(|l| l == "与真人对话"));
                act(&bob, UiAction::Direct("alice".into()), w, cx);
                assert_eq!(bob.read(cx).loc.selected.as_ref().unwrap().id, room);
            })
            .unwrap();
            let send = |body: &str, w: &mut Window, cx: &mut App| {
                let r = alice.read(cx).loc.selected.as_ref().unwrap().clone();
                act(&alice, UiAction::Form(Mode::Message(r)), w, cx);
                fill(&alice, "body", body, w, cx);
                act(&alice, UiAction::Submit, w, cx);
            };
            wa.update(cx, |_, w, cx| send("From the other native window", w, cx))
                .unwrap();
            wb.update(cx, |_, w, cx| {
                wait_for(&bob, w, cx, "live chat update", |v| {
                    v.list("message")
                        .iter()
                        .any(|r| r.field("body") == "From the other native window")
                });
                available.store(false, Ordering::SeqCst);
                wait_for(&bob, w, cx, "offline protection", |v| {
                    v.offline && v.loc.selected.is_none() && v.list("message").is_empty()
                });
            })
            .unwrap();
            wa.update(cx, |_, w, cx| send("Written while peer offline", w, cx))
                .unwrap();
            available.store(true, Ordering::SeqCst);
            wb.update(cx, |_, w, cx| {
                wait_for(&bob, w, cx, "restored room and messages", |v| {
                    !v.offline
                        && v.loc.selected.as_ref().is_some_and(|r| r.id == room)
                        && v.list("message").len() == 2
                });
                assert_eq!(
                    bob.read(cx)
                        .list("message")
                        .iter()
                        .filter(|m| m.field("body") == "Written while peer offline")
                        .count(),
                    1
                );
                admin
                    .execute(json!({"op":"delete","kind":"member","space":"side","id":"bob"}))
                    .unwrap();
                wait_for(&bob, w, cx, "scope removal", |v| {
                    v.loc.space == "default" && v.loc.selected.is_none() && !v.reading
                });
                assert!(bob.read(cx).list("message").is_empty());
                act(&bob, UiAction::Back, w, cx);
                assert_ne!(bob.read(cx).loc.space, "side");
                act(&bob, UiAction::Search, w, cx);
                fill(&bob, "query", "Search twin", w, cx);
                act(&bob, UiAction::Submit, w, cx);
                let results = &bob.read(cx).form.as_ref().unwrap().search_results;
                assert_eq!(results.len(), 1);
                assert_eq!(results[0].space, "default");
                assert_eq!(bob.read(cx).loc.page, Page::Tasks);
            })
            .unwrap();
            finished.store(true, Ordering::SeqCst);
            proxy.join().unwrap();
            drop(team);
            cx.quit();
        });
}

#[::core::prelude::v1::test]
#[ignore = "Runs the native organization defaults and priority pickers in a hidden Windows GPUI window"]
fn hidden_organization_priority_windows() {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().to_path_buf();
    let mut client = Client::local(&dir).unwrap();
    client.execute(json!({"op":"onboarding.setup","organization_name":"Organization defaults","organization_slug":"defaults-qa","work_type":"software","task_types":["task"]})).unwrap();
    client.execute(json!({"op":"create","kind":"member","id":"partner","title":"QA Partner","fields":{"role":"admin"}})).unwrap();
    gpui_kit::application()
        .with_assets(crate::assets::Assets)
        .run(move |cx| {
            gpui_kit::init(cx);
            let (window, view) = gpui_kit::open_window(
                WindowOptions {
                    focus: false,
                    show: false,
                    window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                        None,
                        size(px(1420.), px(1080.)),
                        cx,
                    ))),
                    ..Default::default()
                },
                cx,
                |w, cx| cx.new(|cx| Workbench::new(Some(dir.clone()), false, false, w, cx)),
            )
            .unwrap();
            window
                .update(cx, |_, w, cx| {
                    activate_accessibility(w);
                    settle(&view, w, cx);
                    act(&view, UiAction::Go(Page::Organizations), w, cx);
                    assert!(buttons(w).iter().any(|s| s == "组织默认设置"));
                    assert!(buttons(w).iter().any(|s| s == "添加组织所有者"));
                    let org = view.read(cx).list("organization_settings")[0].clone();
                    act(
                        &view,
                        UiAction::Form(Mode::OrgOwnership(org, "grant")),
                        w,
                        cx,
                    );
                    act(&view, UiAction::Pick(0), w, cx);
                    assert!(buttons(w).iter().any(|s| s.contains("QA Partner")));
                    act(&view, UiAction::Choose(0, "partner".into()), w, cx);
                    act(&view, UiAction::Submit, w, cx);
                    assert_eq!(
                        client
                            .execute(json!({"op":"organization.settings"}))
                            .unwrap()["owners"],
                        2
                    );
                    let change = |levels: &str, w: &mut Window, cx: &mut App| {
                        let org = view.read(cx).list("organization_settings")[0].clone();
                        act(&view, UiAction::Form(Mode::OrgDefaults(org)), w, cx);
                        act(&view, UiAction::Choose(0, "en".into()), w, cx);
                        act(&view, UiAction::Choose(1, levels.into()), w, cx);
                        act(&view, UiAction::Submit, w, cx);
                    };
                    change("5", w, cx);
                    act(&view, UiAction::Go(Page::Tasks), w, cx);
                    act(&view, UiAction::Create("task".into()), w, cx);
                    let pri = view
                        .read(cx)
                        .form
                        .as_ref()
                        .unwrap()
                        .entries
                        .iter()
                        .position(|e| e.spec.key == "priority")
                        .unwrap();
                    act(&view, UiAction::Pick(pri), w, cx);
                    assert!(buttons(w).iter().any(|v| v == "最低"));
                    act(&view, UiAction::Choose(pri, "lowest".into()), w, cx);
                    fill(&view, "title", "Retain original", w, cx);
                    act(&view, UiAction::Submit, w, cx);
                    let old = view.read(cx).loc.selected.as_ref().unwrap().clone();
                    assert_eq!(old.field("priority"), "lowest");
                    act(&view, UiAction::Go(Page::Organizations), w, cx);
                    change("4", w, cx);
                    act(&view, UiAction::Go(Page::Tasks), w, cx);
                    act(&view, UiAction::Create("task".into()), w, cx);
                    act(&view, UiAction::Pick(pri), w, cx);
                    assert!(!buttons(w).iter().any(|v| v == "最低"));
                    act(&view, UiAction::CloseForm, w, cx);
                    act(&view, UiAction::Edit(old), w, cx);
                    assert!(
                        view.read(cx)
                            .options("$priorities")
                            .iter()
                            .any(|(id, _)| id == "lowest")
                    );
                })
                .unwrap();
            cx.quit();
        });
}

#[::core::prelude::v1::test]
#[ignore = "Opens a real delivery link in a signed-in native Windows window"]
fn hidden_mail_link_opens_the_permission_checked_task() {
    let tmp = tempfile::tempdir().unwrap();
    let service = tmp.path().join("service");
    std::fs::create_dir(&service).unwrap();
    let device = tmp.path().join("native");
    let mut store = shipforge_core::Store::open(&service.join("workspace.sqlite3")).unwrap();
    let recipient = store.verified_login("native@example.test").unwrap()["actor"]
        .as_str()
        .unwrap()
        .to_owned();
    store.execute("owner",json!({"op":"onboarding.setup","organization_name":"Delivery","organization_slug":"delivery","work_type":"software","task_types":["task"]})).unwrap();
    store.execute("owner",json!({"op":"create","kind":"member","id":recipient,"title":"Recipient","fields":{"role":"member"}})).unwrap();
    store
        .execute(
            &recipient,
            json!({"op":"inbox.configure","channel":"inbox","key":"all","enabled":false}),
        )
        .unwrap();
    store.execute("owner",json!({"op":"create","kind":"task","id":"mail-target","title":"Delivery target","fields":{"assignees":[recipient]}})).unwrap();
    let link = store.claim_email().unwrap().unwrap().link;
    let token = team_command(&service)
        .args(["token", "issue", "--actor", &recipient, "--scope", "write"])
        .output()
        .unwrap();
    let token: Value = serde_json::from_slice(&token.stdout).unwrap();
    let mut team = Team(
        team_command(&service)
            .args(["serve", "--listen", "127.0.0.1:0"])
            .stdout(Stdio::piped())
            .spawn()
            .unwrap(),
    );
    let mut line = String::new();
    BufReader::new(team.0.stdout.take().unwrap())
        .read_line(&mut line)
        .unwrap();
    let listen: Value = serde_json::from_str(&line).unwrap();
    Client::save_connection(
        Some(&device),
        &format!("http://{}", listen["listen"].as_str().unwrap()),
        token["token"].as_str().unwrap(),
    )
    .unwrap();
    gpui_kit::application()
        .with_assets(crate::assets::Assets)
        .run(move |cx| {
            gpui_kit::init(cx);
            let (window, view) = gpui_kit::open_window(
                WindowOptions {
                    focus: false,
                    show: false,
                    ..Default::default()
                },
                cx,
                |w, cx| {
                    cx.new(|cx| {
                        let mut wb = Workbench::new(Some(device.clone()), false, false, w, cx);
                        wb.inbox.initial_link = Some(link.clone());
                        wb
                    })
                },
            )
            .unwrap();
            window
                .update(cx, |_, w, cx| {
                    activate_accessibility(w);
                    wait_for(&view, w, cx, "email landing in task", |wb| {
                        wb.loc
                            .selected
                            .as_ref()
                            .is_some_and(|r| r.id == "mail-target" && r.title == "Delivery target")
                    });
                    act(
                        &view,
                        UiAction::Inbox(super::inbox_ui::Action::Settings(Some("email".into()))),
                        w,
                        cx,
                    );
                    assert!(buttons(w).iter().any(|v| v == "邮件投递记录"));
                })
                .unwrap();
            drop(team);
            cx.quit();
        });
}
