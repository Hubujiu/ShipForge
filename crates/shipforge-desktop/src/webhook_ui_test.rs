use super::{
    UiAction, Workbench,
    onboarding_ui::{activate_accessibility, buttons, settle},
    webhook_ui::{Action, HookForm},
};
use crate::{
    catalog::Page,
    controls::Field,
    forms::{Control, Mode},
};
use gpui_kit::{prelude::*, *};
use serde_json::json;

#[::core::prelude::v1::test]
#[ignore = "Requires a Windows D3D window"]
fn hidden_webhook_management() {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().to_owned();
    let mut client = shipforge_client::Client::local(&dir).unwrap();
    client.execute(json!({"op":"onboarding.setup","organization_name":"Webhooks","organization_slug":"hook-test","work_type":"software","task_types":["task"]})).unwrap();
    client.execute(json!({"op":"create","kind":"agent","id":"helper","title":"Automation helper","fields":{"runtime":"test"}})).unwrap();
    client.execute(json!({"op":"create","kind":"automation","id":"rule","title":"Receive events","fields":{"enabled":true,"handler_kind":"agent","handler_id":"helper","instruction":"Inspect payload","triggers":[]}})).unwrap();
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
                        size(px(1480.), px(1020.)),
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
                    let act = |a, w: &mut Window, cx: &mut App| {
                        view.update(cx, |v, cx| v.act(a, w, cx));
                        settle(&view, w, cx);
                    };
                    let label = |w: &Window, l: &str| {
                        assert!(buttons(w).iter().any(|b| b == l), "missing {l}");
                    };
                    let record =
                        |cx: &App| view.read(cx).find("automation", "rule").unwrap().clone();
                    act(UiAction::Go(Page::Automations), w, cx);
                    act(UiAction::Open(record(cx)), w, cx);
                    act(UiAction::Form(Mode::Edit(record(cx))), w, cx);
                    label(w, "添加触发器");
                    act(UiAction::AddTrigger, w, cx);
                    let typ = view
                        .read(cx)
                        .form
                        .as_ref()
                        .unwrap()
                        .entries
                        .iter()
                        .position(|e| e.trigger.is_some() && e.spec.key == "type")
                        .unwrap();
                    act(UiAction::Choose(typ, "webhook".into()), w, cx);
                    act(UiAction::Submit, w, cx);
                    label(w, "复制 URL");
                    let Mode::Webhook(HookForm::Issued(links)) =
                        &view.read(cx).form.as_ref().unwrap().mode
                    else {
                        panic!("one-time URL dialog expected")
                    };
                    let t = links[0]["trigger_id"].as_str().unwrap().to_owned();
                    let token = links[0]["url"]
                        .as_str()
                        .unwrap()
                        .rsplit('/')
                        .next()
                        .unwrap()
                        .to_owned();
                    act(UiAction::CloseForm, w, cx);
                    label(w, "Webhook 投递记录");
                    let mut store =
                        shipforge_core::Store::open(&dir.join("workspace.sqlite3")).unwrap();
                    let ip = "127.0.0.1".parse().unwrap();
                    assert!(
                        store
                            .automation_hook_preflight(&token, ip)
                            .unwrap()
                            .is_none()
                    );
                    assert_eq!(
                        store
                            .automation_hook_receive(&token, ip, b"{}", None, None, None)
                            .unwrap()
                            .status,
                        202
                    );
                    act(UiAction::Form(Mode::Edit(record(cx))), w, cx);
                    label(w, "安全设置");
                    act(
                        UiAction::Webhook(Action::Settings("rule".into(), t.clone())),
                        w,
                        cx,
                    );
                    label(w, "重新生成 URL");
                    label(w, "保存安全设置");
                    let f = view.read(cx).form.as_ref().unwrap();
                    let signing = f
                        .entries
                        .iter()
                        .position(|e| e.spec.key == "signing")
                        .unwrap();
                    let filtering = f
                        .entries
                        .iter()
                        .position(|e| e.spec.key == "ip_filter")
                        .unwrap();
                    let Control::Text(Field::Multi(input)) = &f
                        .entries
                        .iter()
                        .find(|e| e.spec.key == "allowlist")
                        .unwrap()
                        .control
                    else {
                        panic!("native IP list expected")
                    };
                    input
                        .clone()
                        .update(cx, |s, cx| s.set_value("127.0.0.7/8\n::1", w, cx));
                    act(UiAction::Choose(signing, "true".into()), w, cx);
                    act(UiAction::Choose(filtering, "true".into()), w, cx);
                    act(UiAction::Submit, w, cx);
                    label(w, "查看签名密钥");
                    let q = json!({"op":"automation.hook.settings","id":"rule","trigger_id":t});
                    let settings = client.execute(q).unwrap();
                    assert_eq!(settings["allowlist"], json!(["127.0.0.0/8", "::1"]));
                    assert!(settings.get("secret").is_none() && settings.get("url").is_none());
                    act(
                        UiAction::Webhook(Action::Perform(
                            "automation.hook.secret",
                            settings.clone(),
                        )),
                        w,
                        cx,
                    );
                    label(w, "复制签名密钥");
                    act(UiAction::CloseForm, w, cx);
                    assert!(matches!(
                        &view.read(cx).form.as_ref().unwrap().mode,
                        Mode::Webhook(HookForm::Settings(_))
                    ));
                    act(UiAction::CloseForm, w, cx);
                    assert!(matches!(
                        &view.read(cx).form.as_ref().unwrap().mode,
                        Mode::Edit(_)
                    ));
                    act(UiAction::CloseForm, w, cx);
                    act(
                        UiAction::Webhook(Action::History("rule".into(), t, None)),
                        w,
                        cx,
                    );
                    let Mode::Webhook(HookForm::History(log)) =
                        &view.read(cx).form.as_ref().unwrap().mode
                    else {
                        panic!("history expected")
                    };
                    assert_eq!(log["items"][0]["source_ip"], "127.0.0.1");
                    act(UiAction::CloseForm, w, cx);
                    act(
                        UiAction::Form(Mode::Webhook(HookForm::Confirm(
                            "automation.hook.rotate",
                            settings,
                        ))),
                        w,
                        cx,
                    );
                    act(UiAction::Submit, w, cx);
                    label(w, "复制 URL");
                    assert_eq!(
                        store
                            .automation_hook_preflight(&token, ip)
                            .unwrap()
                            .unwrap()
                            .status,
                        404
                    );
                })
                .unwrap();
            cx.quit();
        });
}
