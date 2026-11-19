use super::{
    UiAction, Workbench,
    onboarding_ui::{activate_accessibility, buttons, settle},
    personal_bots_ui::{Action, BotForm},
};
use crate::{catalog::Page, forms::Mode};
use gpui_kit::{prelude::*, *};
use serde_json::json;
use shipforge_client::Client;

#[::core::prelude::v1::test]
#[ignore = "Runs native Windows controls in a hidden D3D window"]
fn hidden_lark_bot_regions() {
    // Give only the child test process a fixture origin; other native tests keep their own environment.
    if std::env::var_os("SHIPFORGE_LARK_NATIVE_CHILD").is_none() {
        let upstream = tiny_http::Server::http("127.0.0.1:0").unwrap();
        let root = format!("http://{}", upstream.server_addr());
        let thread = std::thread::spawn(move || {
            for _ in 0..4 {
                let mut req = upstream
                    .recv_timeout(std::time::Duration::from_secs(30))
                    .unwrap()
                    .unwrap();
                assert_eq!(req.url(), "/oauth/v1/app/registration");
                let mut form = String::new();
                req.as_reader().read_to_string(&mut form).unwrap();
                let data = if form.contains("action=begin") {
                    json!({"device_code":"ui-hidden-secret-ticket","user_code":"visible","expire_in":300,"interval":1})
                } else {
                    json!({"error":"expired_token"})
                };
                req.respond(tiny_http::Response::from_string(data.to_string()))
                    .unwrap();
            }
        });
        let child = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "app::lark_ui_test::hidden_lark_bot_regions",
                "--ignored",
                "--nocapture",
            ])
            .env("SHIPFORGE_LARK_NATIVE_CHILD", "1")
            .env("SHIPFORGE_FEISHU_LOOPBACK", &root)
            .env("SHIPFORGE_LARK_LOOPBACK", root)
            .status()
            .unwrap();
        assert!(child.success());
        thread.join().unwrap();
        return;
    }
    let fixture = tempfile::tempdir().unwrap();
    let directory = fixture.path().to_owned();
    let mut client = Client::local(&directory).unwrap();
    client.execute(json!({"op":"onboarding.setup","organization_name":"External","organization_slug":"external-chats","work_type":"software","task_types":["task"]})).unwrap();
    client.execute(json!({"op":"create","kind":"agent","id":"helper","title":"Synthetic Helper","fields":{"runtime":"test"}})).unwrap();
    client.execute(json!({"op":"create","kind":"agent","id":"occupied","title":"Already connected","fields":{"runtime":"test"}})).unwrap();
    let bound=client.execute(json!({"op":"bot.attach","id":"cn-bound","provider":"feishu","account":"cli_native","agent":"occupied","device_key":"synthetic-device-key-for-windows-test"})).unwrap();
    let settings = client
        .execute(json!({"op":"bot.lark.policy","id":bound["id"],"plain_chats":["oc_review"]}))
        .unwrap();
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
                |w, cx| cx.new(|cx| Workbench::new(Some(directory.clone()), false, false, w, cx)),
            )
            .unwrap();
            handle
                .update(cx, |_, w, cx| {
                    activate_accessibility(w);
                    settle(&view, w, cx);
                    let act = |action, w: &mut Window, cx: &mut App| {
                        view.update(cx, |v, cx| v.act(action, w, cx));
                        settle(&view, w, cx);
                    };
                    let label = |w: &Window, name: &str| {
                        let all = buttons(w);
                        assert!(all.iter().any(|v| v == name), "{name}: {all:?}");
                    };
                    act(UiAction::Go(Page::Connectors), w, cx);
                    label(w, "飞书智能体");
                    label(w, "Lark 智能体");
                    for brand in ["feishu", "lark"] {
                        act(UiAction::Bot(Action::Cloud(brand)), w, cx);
                        label(w, "绑定 Agent");
                        if brand == "feishu" {
                            label(w, "接收范围");
                        }
                        act(UiAction::Bot(Action::Candidates(brand)), w, cx);
                        label(w, "Synthetic Helper");
                        let labels = buttons(w);
                        assert_eq!(
                            labels.iter().any(|v| v == "Already connected"),
                            brand == "lark"
                        );
                        act(UiAction::Bot(Action::Pair(brand, "helper".into())), w, cx);
                        label(w, "打开授权链接");
                        label(w, "重新生成二维码");
                        let Mode::Bot(BotForm::Qr(q)) = &view.read(cx).form.as_ref().unwrap().mode
                        else {
                            panic!("missing QR")
                        };
                        assert_eq!(q["provider"], brand);
                        assert!(q["qr_matrix"].as_array().unwrap().len() > 30);
                        assert!(
                            q["qr_content"]
                                .as_str()
                                .unwrap()
                                .contains(if brand == "lark" {
                                    "open.larksuite.com"
                                } else {
                                    "open.feishu.cn"
                                })
                        );
                        act(UiAction::Submit, w, cx);
                        let Mode::Bot(BotForm::Qr(q)) = &view.read(cx).form.as_ref().unwrap().mode
                        else {
                            panic!("expected QR status")
                        };
                        assert_eq!(q["state"], "expired");
                    }
                    act(
                        UiAction::Bot(Action::Agent("feishu", "occupied".into())),
                        w,
                        cx,
                    );
                    let f = view.read(cx).form.as_ref().unwrap();
                    assert!(matches!(&f.mode, Mode::Bot(BotForm::Policy(_))));
                    let plain = f
                        .entries
                        .iter()
                        .find(|e| e.spec.key == "plain_chats")
                        .unwrap();
                    assert_eq!(plain.raw(cx), "oc_review");
                    let i = f
                        .entries
                        .iter()
                        .position(|e| e.spec.key == "reactions")
                        .unwrap();
                    act(UiAction::Choose(i, "true".into()), w, cx);
                    act(UiAction::Submit, w, cx);
                    label(w, "解除绑定");
                    let after = client.execute(json!({"op":"bot.bindings"})).unwrap();
                    assert_eq!(after["items"][0]["settings"]["reactions"], true);
                    assert_eq!(
                        after["items"][0]["settings"]["plain_chats"],
                        json!(["oc_review"])
                    );
                    act(
                        UiAction::Form(Mode::Bot(BotForm::Disconnect(settings.clone()))),
                        w,
                        cx,
                    );
                    act(UiAction::Submit, w, cx);
                    assert_eq!(
                        client.execute(json!({"op":"bot.bindings"})).unwrap()["items"][0]["state"],
                        "disconnected"
                    );
                })
                .unwrap();
            cx.quit();
        });
}
