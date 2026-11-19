use super::{
    UiAction, Workbench,
    onboarding_ui::{activate_accessibility, buttons, settle},
    repository_ui::{Action, Form},
};
use crate::{
    catalog::Page,
    controls::Field,
    forms::{Control, Mode},
};
use gpui_kit::{prelude::*, *};
use serde_json::{Value, json};

#[::core::prelude::v1::test]
#[ignore = "Uses native Windows text, selection and credential controls in a hidden D3D window"]
fn hidden_repository_connection() {
    if std::env::var_os("SHIPFORGE_FORGE_UI_CHILD").is_none() {
        let srv = tiny_http::Server::http("127.0.0.1:0").unwrap();
        let addr = format!("http://{}", srv.server_addr());
        let stopped = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let end = stopped.clone();
        let sent = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let remote = sent.clone();
        let thread = std::thread::spawn(move || {
            while !end.load(std::sync::atomic::Ordering::Acquire) {
                let Some(mut req) = srv
                    .recv_timeout(std::time::Duration::from_millis(100))
                    .unwrap()
                else {
                    continue;
                };
                let data = match req.url().split('?').next().unwrap() {
                    "/api/v4/user" => json!({"id":18,"username":"workshop"}),
                    "/api/v4/personal_access_tokens/self" => {
                        json!({"active":true,"user_id":18,"scopes":["api"],"expires_at":"2030-01-01"})
                    }
                    "/api/v4/projects" => {
                        json!([{"id":32,"path_with_namespace":"sample/ui","visibility":"private","permissions":{"project_access":{"access_level":40}}}])
                    }
                    "/api/v4/projects/32/issues/5" => {
                        json!({"iid":5,"project_id":32,"confidential":false})
                    }
                    "/api/v4/projects/32/issues/5/notes" => {
                        assert_eq!(req.method(), &tiny_http::Method::Post);
                        let payload: Value = serde_json::from_reader(req.as_reader()).unwrap();
                        assert!(
                            payload["body"]
                                .as_str()
                                .unwrap()
                                .starts_with("Windows source reply\n\n<!-- shipforge:")
                        );
                        remote.store(true, std::sync::atomic::Ordering::Release);
                        json!({"id":733})
                    }
                    path => panic!("unexpected GUI repository endpoint {path}"),
                };
                req.respond(tiny_http::Response::from_string(data.to_string()))
                    .unwrap();
            }
        });
        let child = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "app::repository_ui_test::hidden_repository_connection",
                "--ignored",
                "--nocapture",
            ])
            .env("SHIPFORGE_FORGE_UI_CHILD", "1")
            .env("SHIPFORGE_FORGE_SELF_LOOPBACK", addr)
            .status()
            .unwrap();
        stopped.store(true, std::sync::atomic::Ordering::Release);
        thread.join().unwrap();
        assert!(child.success());
        assert!(sent.load(std::sync::atomic::Ordering::Acquire));
        return;
    }
    let home = tempfile::tempdir().unwrap();
    let dir = home.path().to_owned();
    let mut client = shipforge_client::Client::local(&dir).unwrap();
    client.execute(json!({"op":"onboarding.setup","organization_name":"Delivery","organization_slug":"repo-ui","work_type":"software","task_types":["task"]})).unwrap();
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
                    let act = |a, w: &mut Window, cx: &mut App| {
                        view.update(cx, |v, cx| v.act(a, w, cx));
                        settle(&view, w, cx);
                    };
                    let label = |w: &Window, name: &str| {
                        let labels = buttons(w);
                        assert!(labels.iter().any(|s| s == name), "{name} in {labels:?}");
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
                        let Control::Text(Field::Single(input)) = &e.control else {
                            panic!("expected native single-line field")
                        };
                        input
                            .clone()
                            .update(cx, |s, cx| s.set_value(value.to_owned(), w, cx));
                    };
                    let platform = "gitlab-self";
                    act(UiAction::Go(Page::Connectors), w, cx);
                    for name in ["GitHub", "GitLab.com", "自建 GitLab"] {
                        label(w, name);
                    }
                    act(UiAction::Forge(Action::Status(platform)), w, cx);
                    label(w, "连接账号");
                    act(
                        UiAction::Form(Mode::Forge(Form::Pat("organization"))),
                        w,
                        cx,
                    );
                    set("instance", "https://gitlab.example.test", w, cx);
                    set("token", "synthetic-native-token", w, cx);
                    act(UiAction::Submit, w, cx);
                    label(w, "选择代码仓库");
                    act(UiAction::Forge(Action::Repos(platform)), w, cx);
                    label(w, "配对到此空间");
                    let Mode::Forge(Form::Repos(_, list)) =
                        &view.read(cx).form.as_ref().unwrap().mode
                    else {
                        panic!("missing repositories")
                    };
                    let repo = list[0].clone();
                    act(
                        UiAction::Form(Mode::Forge(Form::Pair(platform, repo))),
                        w,
                        cx,
                    );
                    act(UiAction::Pick(0), w, cx);
                    label(w, "双向");
                    act(UiAction::Choose(0, "both".into()), w, cx);
                    act(UiAction::Submit, w, cx);
                    label(w, "复制项目 Webhook 密钥");
                    let binding = client
                        .execute(json!({"op":"forge.status","id":platform}))
                        .unwrap()["pairs"][0]
                        .clone();
                    assert_eq!(binding["direction"], "both");
                    let at=chrono::Utc::now().to_rfc3339();
                    let issue=client.execute(json!({"op":"forge.internal","id":platform,"action":"event","pair":binding["id"],"event":"issue","event_id":"native-new","payload":{"object_kind":"issue","project":{"id":32,"path_with_namespace":"sample/ui"},"object_attributes":{"iid":5,"action":"open","title":"Native source issue","description":"Fixture","state":"opened","created_at":at,"updated_at":at}}})).unwrap();
                    let task=issue["task"].as_str().unwrap();
                    act(UiAction::CloseForm,w,cx);act(UiAction::Go(Page::Tasks),w,cx);
                    let record=serde_json::from_value(client.execute(json!({"op":"get","kind":"task","id":task})).unwrap()).unwrap();
                    act(UiAction::Open(record),w,cx);label(w,"自建 GitLab 来源");
                    act(UiAction::Forge(Action::Thread(platform,task.into())),w,cx);label(w,"回复到来源");
                    act(UiAction::Form(Mode::Forge(Form::Reply(platform,task.into(),binding["id"].as_str().unwrap().into(),"native-note".into()))),w,cx);
                    let entry=&view.read(cx).form.as_ref().unwrap().entries[0];let Control::Text(Field::Multi(input))=&entry.control else{panic!("expected native source composer")};
                    input.clone().update(cx,|input,cx|input.set_value("Windows source reply",w,cx));
                    act(UiAction::Submit,w,cx);
                    let Mode::Forge(Form::Receipt(receipt))=&view.read(cx).form.as_ref().unwrap().mode else{panic!("missing source send receipt")};assert_eq!(receipt["fallback"],true);assert_eq!(receipt["remote_id"],"733");
                    act(UiAction::Forge(Action::Status(platform)), w, cx);
                    label(w, "配对设置");
                    act(
                        UiAction::Form(Mode::Forge(Form::Disconnect(platform, "organization"))),
                        w,
                        cx,
                    );
                    act(UiAction::Submit, w, cx);
                    let status = client
                        .execute(json!({"op":"forge.status","id":platform}))
                        .unwrap();
                    assert_eq!(status["account"], Value::Null);
                    assert_eq!(status["pairs"], json!([]));
                })
                .unwrap();
            cx.quit();
        });
}
