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
fn hidden_personal_bot_binding() {
    // Give only the child test process a fixture origin; other native tests keep their own environment.
    if std::env::var_os("SHIPFORGE_PERSONAL_BOT_UI_CHILD").is_none() {
        let upstream = tiny_http::Server::http("127.0.0.1:0").unwrap();
        let root = format!("http://{}", upstream.server_addr());
        let thread = std::thread::spawn(move || {
            for _ in 0..2 {
                let req = upstream
                    .recv_timeout(std::time::Duration::from_secs(25))
                    .unwrap()
                    .unwrap();
                let data = if req.url().contains("get_bot_qrcode") {
                    json!({"qrcode":"ticket","qrcode_img_content":"weixin://synthetic-qr"})
                } else {
                    assert!(req.url().starts_with("/ilink/bot/get_qrcode_status?"));
                    json!({"status":"expired"})
                };
                req.respond(tiny_http::Response::from_string(data.to_string()))
                    .unwrap();
            }
        });
        let child = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "app::personal_bots_ui_test::hidden_personal_bot_binding",
                "--ignored",
                "--nocapture",
            ])
            .env("SHIPFORGE_PERSONAL_BOT_UI_CHILD", "1")
            .env("SHIPFORGE_BOT_LOOPBACK", root)
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
    let bound=client.execute(json!({"op":"bot.attach","id":"fixture","provider":"qq","account":"12345","agent":"helper","device_key":"synthetic-device-key-for-windows-test"})).unwrap();
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
                    label(w, "微信与 QQ");
                    act(UiAction::Bot(Action::List), w, cx);
                    for name in [
                        "微信扫码",
                        "QQ 扫码",
                        "使用 QQ 凭据",
                        "接收范围",
                        "启动后台接收",
                        "同步一次",
                        "解除绑定",
                    ] {
                        label(w, name);
                    }
                    act(UiAction::Form(Mode::Bot(BotForm::Start("wechat"))), w, cx);
                    act(UiAction::Pick(0), w, cx);
                    label(w, "Synthetic Helper");
                    act(UiAction::Choose(0, "helper".into()), w, cx);
                    act(UiAction::Submit, w, cx);
                    label(w, "检查授权状态");
                    label(w, "重新生成二维码");
                    let Mode::Bot(BotForm::Qr(q)) = &view.read(cx).form.as_ref().unwrap().mode
                    else {
                        panic!("QR result")
                    };
                    assert!(q["qr_matrix"].as_array().unwrap().len() > 20);
                    act(UiAction::Submit, w, cx);
                    let Mode::Bot(BotForm::Qr(q)) = &view.read(cx).form.as_ref().unwrap().mode
                    else {
                        panic!("QR state")
                    };
                    assert_eq!(q["state"], "expired");
                    act(
                        UiAction::Form(Mode::Bot(BotForm::Start("credentials"))),
                        w,
                        cx,
                    );
                    assert!(
                        view.read(cx)
                            .form
                            .as_ref()
                            .unwrap()
                            .entries
                            .iter()
                            .any(|e| e.spec.key == "app_secret"
                                && e.spec.format == crate::forms::Format::Secret)
                    );
                    act(
                        UiAction::Form(Mode::Bot(BotForm::Policy(bound.clone()))),
                        w,
                        cx,
                    );
                    assert_eq!(view.read(cx).form.as_ref().unwrap().entries.len(), 2);
                    act(UiAction::Submit, w, cx);
                    label(w, "解除绑定");
                    act(
                        UiAction::Form(Mode::Bot(BotForm::Disconnect(bound.clone()))),
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
