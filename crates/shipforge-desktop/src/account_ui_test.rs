use crate::{
    app::{
        UiAction, Workbench,
        onboarding_ui::{activate_accessibility, buttons, settle},
    },
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
};

struct Team(Child);
impl Drop for Team {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
fn server(args: &Path, root: &Path) -> Command {
    let mut cmd = Command::new(args);
    cmd.creation_flags(0x08000000)
        .args(["--data-dir"])
        .arg(root);
    for key in [
        "SHIPFORGE_JIRA_ORGANIZATION",
        "SHIPFORGE_JIRA_BASE_URL",
        "SHIPFORGE_GITHUB_WEBHOOKS",
        "SHIPFORGE_SLACK_CONFIG",
    ] {
        cmd.env_remove(key);
    }
    cmd
}
#[::core::prelude::v1::test]
#[ignore = "Runs a hidden native Windows window against a live local ShipForge server"]
fn hidden_accounts_and_personal_tokens() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("server");
    let device = temp.path().join("desktop");
    let path = std::env::current_exe().unwrap();
    let binary = path
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("shipforge-server.exe");
    let issued = server(&binary, &root)
        .args(["token", "issue", "--actor", "owner", "--scope", "write"])
        .output()
        .unwrap();
    assert!(issued.status.success());
    let owner: Value = serde_json::from_slice(&issued.stdout).unwrap();
    let mut service = Team(
        server(&binary, &root)
            .args(["serve", "--listen", "127.0.0.1:0"])
            .stdout(Stdio::piped())
            .spawn()
            .unwrap(),
    );
    let mut line = String::new();
    BufReader::new(service.0.stdout.take().unwrap())
        .read_line(&mut line)
        .unwrap();
    let online: Value = serde_json::from_str(&line).unwrap();
    let url = format!("http://{}", online["listen"].as_str().unwrap());
    let mut admin = Client::remote_in(temp.path(), &url, owner["token"].as_str().unwrap()).unwrap();
    admin.execute(json!({"op":"onboarding.setup","organization_name":"Local QA","organization_slug":"local-qa","work_type":"software","task_types":["task"]})).unwrap();
    for actor in ["writer", "reader"] {
        admin.execute(json!({"op":"create","kind":"member","id":actor,"title":actor,"fields":{"role":"member"}})).unwrap();
        let credential=admin.execute(json!({"op":"access.issue","actor":actor,"scope":if actor=="writer"{"write"}else{"read"}})).unwrap();
        Client::save_connection(Some(&device), &url, credential["token"].as_str().unwrap())
            .unwrap();
    }
    let mut client = Client::from_dir(Some(&device)).unwrap();
    let rows = client.execute(json!({"op":"profiles.list"})).unwrap();
    let writer = rows["accounts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["actor"] == "writer")
        .unwrap()["id"]
        .clone();
    client.execute(json!({"op":"profiles.remember","id":rows["active"],"location":{"space":"default","page":"docs"}})).unwrap();
    gpui_kit::application()
        .with_assets(crate::assets::Assets)
        .run(move |cx| {
            gpui_kit::init(cx);
            let (window, view) = gpui_kit::open_window(
                WindowOptions {
                    show: false,
                    focus: false,
                    window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                        None,
                        size(px(1400.), px(1050.)),
                        cx,
                    ))),
                    ..Default::default()
                },
                cx,
                |w, cx| cx.new(|cx| Workbench::new(Some(device.clone()), false, false, w, cx)),
            )
            .unwrap();
            window
                .update(cx, |_, w, cx| {
                    let act = |action, w: &mut Window, cx: &mut App| {
                        view.update(cx, |v, cx| v.act(action, w, cx))
                    };
                    activate_accessibility(w);
                    settle(&view, w, cx);
                    assert_eq!(view.read(cx).loc.page, Page::Docs);
                    assert_eq!(view.read(cx).actor, "reader");
                    assert!(buttons(w).iter().any(|s| s == "账号列表"));
                    act(UiAction::Accounts, w, cx);
                    settle(&view, w, cx);
                    assert!(buttons(w).iter().any(|s| s == "切换账号"));
                    act(
                        UiAction::SwitchAccount(writer.as_str().unwrap().into()),
                        w,
                        cx,
                    );
                    settle(&view, w, cx);
                    assert_eq!(view.read(cx).actor, "writer");
                    act(UiAction::Go(Page::Settings), w, cx);
                    settle(&view, w, cx);
                    assert!(buttons(w).iter().any(|s| s == "个人访问令牌"));
                    act(UiAction::PersonalTokens, w, cx);
                    settle(&view, w, cx);
                    act(UiAction::NewPersonalToken(false), w, cx);
                    settle(&view, w, cx);
                    let f = view.read(cx).form.as_ref().unwrap();
                    let index = f
                        .entries
                        .iter()
                        .position(|e| e.spec.key == "lifetime")
                        .unwrap();
                    let Control::Text(Field::Single(input)) = &f.entries[0].control else {
                        panic!("name field")
                    };
                    input
                        .clone()
                        .update(cx, |s, cx| s.set_value("GPU QA", w, cx));
                    act(UiAction::Choose(index, "90d".into()), w, cx);
                    act(UiAction::Submit, w, cx);
                    settle(&view, w, cx);
                    let Mode::Output { content, .. } = &view.read(cx).form.as_ref().unwrap().mode
                    else {
                        panic!("one-time credential")
                    };
                    let mut personal = Client::remote_in(&device, &url, content).unwrap();
                    assert_eq!(
                        personal.execute(json!({"op":"access.whoami"})).unwrap()["actor"],
                        "writer"
                    );
                    act(UiAction::CloseForm, w, cx);
                    act(UiAction::PersonalTokens, w, cx);
                    settle(&view, w, cx);
                    let list = &view.read(cx).tokens;
                    assert_eq!(list.len(), 1);
                    assert!(list[0].get("token").is_none());
                    assert_eq!(list[0]["label"], "GPU QA");
                    assert!(buttons(w).iter().any(|s| s == "吊销令牌"));
                    act(
                        UiAction::Form(Mode::Confirm {
                            label: "吊销令牌".into(),
                            command: json!({"op":"access.personal.revoke","id":list[0]["id"]}),
                        }),
                        w,
                        cx,
                    );
                    act(UiAction::Submit, w, cx);
                    settle(&view, w, cx);
                    assert!(view.read(cx).tokens.is_empty());
                    assert!(
                        personal
                            .execute(json!({"op":"access.whoami"}))
                            .unwrap_err()
                            .starts_with("unauthorized:")
                    );
                    let credential_id = view.read(cx).access_identity["token_id"].clone();
                    admin
                        .execute(json!({"op":"access.revoke","id":credential_id}))
                        .unwrap();
                    act(UiAction::Refresh, w, cx);
                    settle(&view, w, cx);
                    assert_eq!(view.read(cx).mode, "local");
                    assert_eq!(view.read(cx).loc.section, "accounts");
                    assert!(buttons(w).iter().any(|s| s == "重新登录账号"));
                    let new = admin
                        .execute(json!({"op":"access.issue","actor":"writer","scope":"write"}))
                        .unwrap();
                    act(
                        UiAction::Form(Mode::Reauthenticate {
                            account: writer.as_str().unwrap().into(),
                            url: url.clone(),
                        }),
                        w,
                        cx,
                    );
                    let Control::Text(Field::Single(input)) =
                        &view.read(cx).form.as_ref().unwrap().entries[0].control
                    else {
                        panic!("expected the credential field")
                    };
                    input.clone().update(cx, |s, cx| {
                        s.set_value(new["token"].as_str().unwrap().to_string(), w, cx)
                    });
                    act(UiAction::Submit, w, cx);
                    settle(&view, w, cx);
                    assert_eq!(view.read(cx).mode, "team");
                    assert_eq!(view.read(cx).actor, "writer");
                    assert_eq!(view.read(cx).active_profile.as_deref(), writer.as_str());
                })
                .unwrap();
            cx.quit();
        });
    drop(service);
}
