use super::{
    UiAction, Workbench,
    onboarding_ui::{activate_accessibility, buttons, settle},
};
use crate::{
    catalog::{Page, Record},
    controls::Field,
    forms::{Control, Mode},
};
use gpui_kit::{prelude::*, *};
use serde_json::json;
use shipforge_client::Client;

#[::core::prelude::v1::test]
#[ignore = "Runs a hidden native D3D window on Windows"]
fn hidden_organization_status_workflow() {
    let profile = tempfile::tempdir().unwrap();
    let directory = profile.path().to_owned();
    let mut client = Client::local(&directory).unwrap();
    client.execute(json!({"op":"onboarding.setup","organization_name":"Workflow QA","organization_slug":"workflow-qa","work_type":"software","task_types":["bug","task"]})).unwrap();
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
                        size(px(1480.), px(1340.)),
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
                    let act = |a, w: &mut Window, cx: &mut App| {
                        view.update(cx, |v, cx| v.act(a, w, cx));
                        settle(&view, w, cx);
                    };
                    let idx = |key: &str, cx: &App| {
                        view.read(cx)
                            .form
                            .as_ref()
                            .unwrap()
                            .entries
                            .iter()
                            .position(|e| e.spec.key == key)
                            .unwrap()
                    };
                    let text = |key: &str, value: &str, w: &mut Window, cx: &mut App| {
                        let i = idx(key, cx);
                        let Control::Text(Field::Single(input)) =
                            &view.read(cx).form.as_ref().unwrap().entries[i].control
                        else {
                            panic!("expected text")
                        };
                        input.clone().update(cx, |f, cx| f.set_value(value, w, cx));
                    };
                    act(UiAction::Go(Page::Statuses), w, cx);
                    assert!(buttons(w).iter().any(|b| b == "新建组织状态"));
                    act(
                        UiAction::Form(Mode::Workflow {
                            op: "status.library.save",
                            record: Record::default(),
                        }),
                        w,
                        cx,
                    );
                    text("title", "Fallback QA", w, cx);
                    text("zh", "质量复核", w, cx);
                    text("en", "Quality check", w, cx);
                    act(
                        UiAction::Choose(idx("category", cx), "in_progress".into()),
                        w,
                        cx,
                    );
                    act(UiAction::Choose(idx("collapsed", cx), "true".into()), w, cx);
                    act(UiAction::Submit, w, cx);
                    let state = view
                        .read(cx)
                        .list("status_library")
                        .iter()
                        .find(|r| r.title == "Fallback QA")
                        .expect("created in native form")
                        .clone();
                    let source = state.id.clone();
                    assert_eq!(state.localized_title(), "质量复核");
                    act(UiAction::Go(Page::Types), w, cx);
                    let bug = view.read(cx).find("task_type", "bug").unwrap().clone();
                    act(UiAction::Open(bug.clone()), w, cx);
                    assert!(buttons(w).iter().any(|b| b == "配置类型工作流"));
                    act(
                        UiAction::Form(Mode::Workflow {
                            op: "task_type.workflow.save",
                            record: bug,
                        }),
                        w,
                        cx,
                    );
                    for id in [
                        format!("org/{source}"),
                        "org/todo".into(),
                        "org/done".into(),
                    ] {
                        act(UiAction::Choose(idx("statuses", cx), id), w, cx);
                    }
                    act(
                        UiAction::Choose(idx("default_status", cx), format!("org/{source}")),
                        w,
                        cx,
                    );
                    act(UiAction::Submit, w, cx);
                    let checked = client
                        .execute(json!({"op":"task_type.workflow","id":"bug","language":"en"}))
                        .unwrap();
                    assert_eq!(checked["statuses"][0]["name"], "Quality check");
                    assert_eq!(checked["default_status"], source);
                    act(UiAction::Go(Page::Tasks), w, cx);
                    act(UiAction::Create("task".into()), w, cx);
                    act(UiAction::Choose(idx("type", cx), "bug".into()), w, cx);
                    assert_eq!(
                        view.read(cx).form.as_ref().unwrap().values(cx).unwrap()["status"],
                        source
                    );
                    crate::locale::select("en");
                    assert_eq!(
                        view.read(cx).options("$task_statuses").first(),
                        Some(&(source.clone(), "Quality check".to_owned()))
                    );
                    assert_eq!(view.read(cx).options("$task_statuses").len(), 3);
                    assert!(
                        view.read(cx)
                            .options("$statuses")
                            .iter()
                            .any(|(k, n)| k == &source && n == "Quality check")
                    );
                    text("title", "Native workflow sample", w, cx);
                    act(UiAction::Submit, w, cx);
                    let task = view
                        .read(cx)
                        .list("task")
                        .iter()
                        .find(|r| r.title == "Native workflow sample")
                        .unwrap()
                        .clone();
                    assert_eq!(task.field("status"), source);
                    act(UiAction::CloseDetail, w, cx);
                    act(UiAction::Layout("list".into()), w, cx);
                    assert!(buttons(w).contains(&"Quality check".to_owned()));
                    assert!(!buttons(w).contains(&task.title), "default fold");
                    let loc = &view.read(cx).loc;
                    act(
                        UiAction::FoldStatuses(
                            format!("{}--{}-{source}", loc.space, loc.task_type),
                            false,
                        ),
                        w,
                        cx,
                    );
                    assert!(buttons(w).contains(&task.title), "manual expansion");
                    act(UiAction::Layout("board".into()), w, cx);
                    assert!(w.debug_a11y_tree_json().unwrap().contains("Quality check"));
                    crate::locale::select("zh-CN");
                    act(UiAction::Go(Page::Statuses), w, cx);
                    let state = view
                        .read(cx)
                        .find("status_library", &source)
                        .unwrap()
                        .clone();
                    act(
                        UiAction::Form(Mode::Workflow {
                            op: "status.library.removal",
                            record: state,
                        }),
                        w,
                        cx,
                    );
                    act(UiAction::Choose(idx("target", cx), "todo".into()), w, cx);
                    act(UiAction::Submit, w, cx);
                    assert!(matches!(
                        view.read(cx).form.as_ref().map(|f| &f.mode),
                        Some(Mode::Confirm { .. })
                    ));
                    act(UiAction::Submit, w, cx);
                    assert!(view.read(cx).find("status_library", &source).is_none());
                    assert_eq!(
                        client
                            .execute(json!({"op":"get","kind":"task","id":task.id}))
                            .unwrap()["fields"]["status"],
                        "todo"
                    );
                })
                .unwrap();
            cx.quit();
        });
}
