use super::{
    UiAction, Workbench,
    onboarding_ui::{activate_accessibility, buttons, settle},
    review_ui::{Action, ReviewForm},
};
use crate::{catalog::Page, forms::Mode};
use gpui_kit::{prelude::*, *};
use serde_json::json;
use shipforge_client::Client;

#[::core::prelude::v1::test]
#[ignore = "Runs a hidden native D3D window on a Windows workstation"]
fn hidden_review_planning_window() {
    let profile = tempfile::tempdir().unwrap();
    let directory = profile.path().to_owned();
    let mut client = Client::local(&directory).unwrap();
    client.execute(json!({"op":"onboarding.setup","organization_name":"Workspace","organization_slug":"plan-check","work_type":"software","task_types":["task"]})).unwrap();
    client
        .execute(json!({"op":"triage.configure","enabled":true}))
        .unwrap();
    for (kind, id, fields) in [
        ("project", "launch", json!({})),
        ("task", "waiting", json!({"status":"triage"})),
        (
            "task",
            "scheduled",
            json!({"project":"launch","start_date":"2026-11-01","due_date":"2026-11-10"}),
        ),
    ] {
        client
            .execute(json!({"op":"create","kind":kind,"id":id,"title":id,"fields":fields}))
            .unwrap();
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
                        size(px(1540.), px(1040.)),
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
                    let act =
                        |a, w: &mut Window, cx: &mut App| view.update(cx, |v, cx| v.act(a, w, cx));
                    act(UiAction::Go(Page::Triage), w, cx);
                    settle(&view, w, cx);
                    let labels = buttons(w);
                    for label in [
                        "创建待分拣任务",
                        "分拣设置",
                        "新建规则",
                        "显示稍后处理",
                        "waiting",
                    ] {
                        assert!(
                            labels.iter().any(|v| v == label),
                            "missing {label}: {labels:?}"
                        );
                    }
                    let task = view.read(cx).find("task", "waiting").unwrap().clone();
                    act(UiAction::Review(Action::Open(task.clone())), w, cx);
                    settle(&view, w, cx);
                    let labels = buttons(w);
                    for label in ["接受", "标记重复", "拒绝", "搁置", "复制任务链接"]
                    {
                        assert!(
                            labels.iter().any(|v| v == label),
                            "missing {label}: {labels:?}"
                        );
                    }
                    act(
                        UiAction::Form(Mode::Review(ReviewForm::Decide {
                            rows: vec![task],
                            choice: "defer",
                        })),
                        w,
                        cx,
                    );
                    act(UiAction::Submit, w, cx);
                    settle(&view, w, cx);
                    let hidden = client.execute(json!({"op":"triage.queue"})).unwrap();
                    assert_eq!(hidden["deferred"], 1);
                    assert!(hidden["items"].as_array().unwrap().is_empty());
                    act(UiAction::CloseDetail, w, cx);
                    act(UiAction::Review(Action::Toggle("deferred")), w, cx);
                    settle(&view, w, cx);
                    assert!(buttons(w).iter().any(|v| v == "waiting"));
                    act(UiAction::Go(Page::Projects), w, cx);
                    settle(&view, w, cx);
                    let project = view.read(cx).find("project", "launch").unwrap().clone();
                    act(UiAction::Open(project), w, cx);
                    settle(&view, w, cx);
                    let labels = buttons(w);
                    for label in ["计划区间", "开始日", "截止日", "scheduled"] {
                        assert!(
                            labels.iter().any(|v| v == label),
                            "missing {label}: {labels:?}"
                        );
                    }
                    let link = client
                        .execute(json!({"op":"task.share","id":"scheduled"}))
                        .unwrap();
                    view.update(cx, |v, _| {
                        v.open_task_link(link["link"].as_str().unwrap().into())
                    });
                    settle(&view, w, cx);
                    assert_eq!(view.read(cx).loc.selected.as_ref().unwrap().id, "scheduled");
                })
                .unwrap();
            cx.quit();
        });
}
