use super::{
    UiAction, Workbench,
    onboarding_ui::{activate_accessibility, buttons, settle},
    personal_bots_ui::{Action, BotForm},
};
use crate::{
    catalog::Page,
    controls::Field,
    forms::{Control, Format, Mode},
};
use gpui_kit::{prelude::*, *};
use serde_json::json;
use std::{
    io::{BufRead, BufReader},
    os::windows::process::CommandExt,
    process::{Command, Stdio},
};

#[::core::prelude::v1::test]
#[ignore = "Requires a Windows D3D window and a local synthetic WeCom WebSocket"]
fn hidden_wecom_agent_connection() {
    if std::env::var_os("SHIPFORGE_WECOM_NATIVE_CHILD").is_none() {
        let script =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scripts/wecom_fixture.py");
        let mut fixture = Command::new("python")
            .arg(script)
            .stdout(Stdio::piped())
            .creation_flags(0x08000000)
            .spawn()
            .unwrap();
        let mut origin = String::new();
        BufReader::new(fixture.stdout.take().unwrap())
            .read_line(&mut origin)
            .unwrap();
        let result = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "app::wecom_ui_test::hidden_wecom_agent_connection",
                "--ignored",
                "--nocapture",
            ])
            .env("SHIPFORGE_WECOM_NATIVE_CHILD", "1")
            .env("SHIPFORGE_WECOM_LOOPBACK", origin.trim())
            .status();
        let _ = fixture.kill();
        let _ = fixture.wait();
        assert!(result.unwrap().success());
        return;
    }
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().to_owned();
    let mut client = shipforge_client::Client::local(&dir).unwrap();
    client.execute(json!({"op":"onboarding.setup","organization_name":"Messaging","organization_slug":"wecom-ui","work_type":"software","task_types":["task"]})).unwrap();
    client.execute(json!({"op":"create","kind":"agent","id":"helper","title":"Workspace helper","fields":{"runtime":"test"}})).unwrap();
    gpui_kit::application()
        .with_assets(crate::assets::Assets)
        .run(move |cx| {
            gpui_kit::init(cx);
            let (handle, view) = gpui_kit::open_window(
                WindowOptions {
                    show: false,
                    focus: false,
                    window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                        None,
                        size(px(1450.), px(1024.)),
                        cx,
                    ))),
                    ..Default::default()
                },
                cx,
                |w, cx| cx.new(|cx| Workbench::new(Some(dir.clone()), false, false, w, cx)),
            )
            .unwrap();
            handle
                .update(cx, |_, w, cx| {
                    activate_accessibility(w);
                    settle(&view, w, cx);
                    let act = |action, w: &mut Window, cx: &mut App| {
                        view.update(cx, |app, cx| app.act(action, w, cx));
                        settle(&view, w, cx);
                    };
                    let label = |w: &Window, name: &str| {
                        let all = buttons(w);
                        assert!(all.iter().any(|s| s == name), "{name} in {all:?}");
                    };
                    let set = |key: &str, value: &str, w: &mut Window, cx: &mut App| {
                        let e = view
                            .read(cx)
                            .form
                            .as_ref()
                            .unwrap()
                            .entries
                            .iter()
                            .find(|e| e.spec.key == key)
                            .unwrap();
                        if key == "secret" {
                            assert!(e.spec.format == Format::Secret);
                        }
                        let Control::Text(Field::Single(input)) = &e.control else {
                            panic!("expected native text field")
                        };
                        input
                            .clone()
                            .update(cx, |input, cx| input.set_value(value.to_owned(), w, cx));
                    };
                    act(UiAction::Go(Page::Connectors), w, cx);
                    label(w, "企业微信智能体");
                    act(UiAction::Bot(Action::Cloud("wecom")), w, cx);
                    label(w, "绑定 Agent");
                    act(UiAction::Bot(Action::Candidates("wecom")), w, cx);
                    label(w, "Workspace helper");
                    act(UiAction::Bot(Action::Pair("wecom", "helper".into())), w, cx);
                    label(w, "验证并绑定");
                    label(w, "打开企业微信管理后台");
                    set("bot_id", "wecom-test", w, cx);
                    set("secret", "isolated-wecom-test-secret", w, cx);
                    act(UiAction::Submit, w, cx);
                    label(w, "接收范围");
                    label(w, "解除绑定");
                    let bound = client
                        .execute(json!({"op":"bot.bindings","available_for":"wecom"}))
                        .unwrap();
                    assert_eq!(bound["available_agents"], json!([]));
                    let b = &bound["items"][0];
                    assert_eq!(b["state"], "connected");
                    act(
                        UiAction::Bot(Action::Agent("wecom", "helper".into())),
                        w,
                        cx,
                    );
                    let f = view.read(cx).form.as_ref().unwrap();
                    assert!(matches!(&f.mode, Mode::Bot(BotForm::Policy(_))));
                    assert_eq!(
                        f.entries.iter().map(|e| e.spec.key).collect::<Vec<_>>(),
                        ["dm", "groups"]
                    );
                    act(UiAction::Choose(1, "false".into()), w, cx);
                    act(UiAction::Submit, w, cx);
                    let saved = client.execute(json!({"op":"bot.bindings"})).unwrap();
                    assert_eq!(saved["items"][0]["groups"], false);
                    assert_eq!(saved["items"][0]["dm"], true);
                    act(
                        UiAction::Form(Mode::Bot(BotForm::Disconnect(b.clone()))),
                        w,
                        cx,
                    );
                    act(UiAction::Submit, w, cx);
                    let final_state = client
                        .execute(json!({"op":"bot.bindings","available_for":"wecom"}))
                        .unwrap();
                    assert_eq!(final_state["items"][0]["state"], "disconnected");
                    assert_eq!(final_state["available_agents"][0]["id"], "helper");
                })
                .unwrap();
            cx.quit();
        });
}
