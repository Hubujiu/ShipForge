use super::{
    UiAction, Workbench,
    machine_management::{Action, Dialog},
    onboarding_ui::{activate_accessibility, buttons, settle},
};
use crate::{catalog::Page, forms::Mode};
use gpui_kit::{prelude::*, *};
use serde_json::json;
use shipforge_client::Client;

#[::core::prelude::v1::test]
#[ignore = "Runs an isolated native Windows D3D window and user-owned worker settings"]
fn hidden_window_service_preferences_and_runtime_controls() {
    let tmp = tempfile::tempdir().unwrap();
    let data = tmp.path().to_owned();
    let cfg = data.join("machine.json");
    std::fs::write(
        &cfg,
        json!({"name":"QA machine","machine_id":"qa-box","runtimes":{}}).to_string(),
    )
    .unwrap();
    let mut client = Client::local(&data).unwrap();
    const KEY: &str = "hidden-machine-control-fake-key-for-native-test";
    for q in [
        json!({"op":"onboarding.setup","organization_name":"Machine QA","organization_slug":"machine-qa","work_type":"software","task_types":["task"]}),
        json!({"op":"machine.register","machine_id":"qa-box","machine_key":KEY,"title":"QA machine"}),
        json!({"op":"machine.runtime.publish","machine_id":"qa-box","machine_key":KEY,"capabilities":[{"name":"demo-engine","status":"available","version":"1.2.3","configured":true,"testable":true,"upgradable":true}]}),
    ] {
        client.execute(q).unwrap();
    }
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
                        size(px(1550.), px(1400.)),
                        cx,
                    ))),
                    ..Default::default()
                },
                cx,
                |w, cx| cx.new(|cx| Workbench::new(Some(data.clone()), false, false, w, cx)),
            )
            .unwrap();
            handle
                .update(cx, |_, w, cx| {
                    activate_accessibility(w);
                    super::machine_management::test_hidden_icon(w);
                    settle(&view, w, cx);
                    let act = |a, w: &mut Window, cx: &mut App| {
                        view.update(cx, |s, cx| s.act(a, w, cx));
                        settle(&view, w, cx);
                    };
                    let has = |w: &Window, s: &str| buttons(w).iter().any(|v| v == s);
                    let index = |key: &str, cx: &App| {
                        view.read(cx)
                            .form
                            .as_ref()
                            .unwrap()
                            .entries
                            .iter()
                            .position(|e| e.spec.key == key)
                            .unwrap()
                    };
                    act(UiAction::Go(Page::Machines), w, cx);
                    assert!(has(w, "本机服务设置"));
                    act(UiAction::MachineManagement(Action::Settings), w, cx);
                    assert!(has(w, "读取配置"));
                    act(UiAction::Submit, w, cx);
                    assert!(
                        has(w, "重新启动工作机"),
                        "{:?} {:?}",
                        view.read(cx).error,
                        buttons(w)
                    );
                    act(UiAction::Choose(index("mode", cx), "desktop".into()), w, cx);
                    act(
                        UiAction::Choose(index("close_to_tray", cx), "false".into()),
                        w,
                        cx,
                    );
                    act(UiAction::Submit, w, cx);
                    act(UiAction::MachineManagement(Action::Settings), w, cx);
                    act(UiAction::Submit, w, cx);
                    let Mode::MachineManagement(Dialog::Policy(_, status)) =
                        &view.read(cx).form.as_ref().unwrap().mode
                    else {
                        panic!("policy missing")
                    };
                    assert_eq!(status["policy"]["mode"], "desktop");
                    assert_eq!(status["policy"]["close_to_tray"], false);
                    view.update(cx, |s, cx| assert!(!s.machine_window_close(w, cx)));
                    settle(&view, w, cx);
                    assert!(has(w, "确认退出"));
                    act(UiAction::CloseForm, w, cx);
                    let machine = view.read(cx).find("machine", "qa-box").unwrap().clone();
                    act(UiAction::Open(machine), w, cx);
                    assert!(has(w, "运行时能力"));
                    act(
                        UiAction::MachineManagement(Action::Inventory("qa-box".into())),
                        w,
                        cx,
                    );
                    assert!(has(w, "重新检测") && has(w, "测试模型连接") && has(w, "升级运行时"));
                    act(
                        UiAction::Form(Mode::MachineManagement(Dialog::Command(
                            "qa-box".into(),
                            "demo-engine".into(),
                            "test",
                        ))),
                        w,
                        cx,
                    );
                    assert!(has(w, "确认执行"));
                    act(UiAction::Submit, w, cx);
                    let v = client
                        .execute(json!({"op":"machine.runtimes","id":"qa-box"}))
                        .unwrap();
                    assert_eq!(v["recent"][0]["state"], "queued");
                    assert_eq!(v["recent"][0]["action"], "test");
                })
                .unwrap();
            cx.quit();
        });
}
