use super::{
    UiAction, Workbench,
    onboarding_ui::{activate_accessibility, buttons, settle},
};
use crate::{catalog::Page, controls::Field, forms::Mode};
use gpui_kit::{prelude::*, *};
use serde_json::{Value, json};
use shipforge_client::Client;

#[::core::prelude::v1::test]
#[ignore = "Runs a hidden native D3D window on a Windows workstation"]
fn hidden_label_workflows() {
    let directory = tempfile::tempdir().unwrap();
    let dir = directory.path().to_path_buf();
    let mut client = Client::local(&dir).unwrap();
    client.execute(json!({"op":"onboarding.setup","organization_name":"Test","organization_slug":"labels","work_type":"software","task_types":["task"]})).unwrap();
    client
        .execute(json!({"op":"create","kind":"tag_group","id":"size","title":"Size"}))
        .unwrap();
    for (id, title) in [("small", "Small"), ("large", "Large")] {
        client
            .execute(
                json!({"op":"create","kind":"tag","id":id,"title":title,"fields":{"group":"size"}}),
            )
            .unwrap();
    }
    for id in ["a", "b"] {
        client.execute(json!({"op":"create","kind":"task","title":id,"id":id,"fields":{"labels":["small"]}})).unwrap();
    }
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
                        size(px(1400.), px(940.)),
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
                    let act = |action, w: &mut Window, cx: &mut App| {
                        view.update(cx, |v, cx| v.act(action, w, cx))
                    };
                    let choose = |key: &str, value: &str, w: &mut Window, cx: &mut App| {
                        let index = view
                            .read(cx)
                            .form
                            .as_ref()
                            .unwrap()
                            .entries
                            .iter()
                            .position(|e| e.spec.key == key)
                            .unwrap();
                        act(UiAction::Choose(index, value.into()), w, cx);
                    };
                    act(UiAction::Go(Page::Tags), w, cx);
                    settle(&view, w, cx);
                    assert!(buttons(w).iter().any(|l| l == "合并标签"));
                    assert!(buttons(w).iter().any(|l| l == "查找标签"));
                    act(UiAction::Go(Page::Tasks), w, cx);
                    settle(&view, w, cx);
                    assert!(buttons(w).iter().any(|l| l == "批量处理标签"));
                    act(UiAction::Form(Mode::BulkTags), w, cx);
                    settle(&view, w, cx);
                    choose("tasks", "a", w, cx);
                    choose("tasks", "b", w, cx);
                    let open_labels = |w: &mut Window, cx: &mut App| {
                        let index = view
                            .read(cx)
                            .form
                            .as_ref()
                            .unwrap()
                            .entries
                            .iter()
                            .position(|e| e.spec.key == "labels")
                            .unwrap();
                        act(UiAction::Pick(index), w, cx);
                        let Some(Field::Single(input)) =
                            view.read(cx).form.as_ref().unwrap().picker_search.clone()
                        else {
                            panic!("expected the label search field")
                        };
                        assert!(input.read(cx).focus_handle(cx).is_focused(w));
                        input
                    };
                    open_labels(w, cx).update(cx, |s, cx| s.set_value("Lar", w, cx));
                    settle(&view, w, cx);
                    assert!(buttons(w).iter().any(|l| l == "Large"));
                    assert!(!buttons(w).iter().any(|l| l == "Small"));
                    choose("labels", "large", w, cx);
                    act(UiAction::Submit, w, cx);
                    settle(&view, w, cx);
                    for id in ["a", "b"] {
                        assert_eq!(
                            client
                                .execute(json!({"op":"get","kind":"task","id":id}))
                                .unwrap()["fields"]["labels"],
                            json!(["large"])
                        );
                    }
                    act(UiAction::Filter, w, cx);
                    settle(&view, w, cx);
                    choose("filter_labels", "small", w, cx);
                    act(UiAction::Submit, w, cx);
                    settle(&view, w, cx);
                    assert!(view.read(cx).task_rows().is_empty());
                    act(UiAction::Go(Page::TagGroups), w, cx);
                    settle(&view, w, cx);
                    assert!(
                        view.read(cx)
                            .list("tag_group")
                            .iter()
                            .any(|r| r.id == "size")
                    );
                    act(UiAction::Go(Page::Tags), w, cx);
                    settle(&view, w, cx);
                    act(UiAction::Form(Mode::MergeTags), w, cx);
                    settle(&view, w, cx);
                    choose("source", "large", w, cx);
                    choose("target", "small", w, cx);
                    act(UiAction::Submit, w, cx);
                    settle(&view, w, cx);
                    assert!(matches!(
                        &view.read(cx).form.as_ref().unwrap().mode,
                        Mode::Confirm { .. }
                    ));
                    act(UiAction::Submit, w, cx);
                    settle(&view, w, cx);
                    let after: Value = client
                        .execute(json!({"op":"get","kind":"task","id":"a"}))
                        .unwrap();
                    assert_eq!(after["fields"]["labels"], json!(["small"]));
                    act(UiAction::Go(Page::Tasks), w, cx);
                    settle(&view, w, cx);
                    let task: crate::catalog::Record = serde_json::from_value(after).unwrap();
                    act(UiAction::Form(Mode::Edit(task)), w, cx);
                    settle(&view, w, cx);
                    let new_name = "Support, Europe";
                    open_labels(w, cx).update(cx, |s, cx| s.set_value(new_name, w, cx));
                    settle(&view, w, cx);
                    assert!(
                        buttons(w)
                            .iter()
                            .any(|l| l == &format!("创建并选中标签「{new_name}」"))
                    );
                    choose("labels", new_name, w, cx);
                    act(UiAction::Submit, w, cx);
                    settle(&view, w, cx);
                    let rows = client.execute(json!({"op":"list","kind":"tag"})).unwrap();
                    let created = rows
                        .as_array()
                        .unwrap()
                        .iter()
                        .find(|t| t["title"] == new_name)
                        .unwrap();
                    assert_eq!(created["fields"]["color"].as_str().unwrap().len(), 7);
                    let task = client
                        .execute(json!({"op":"get","kind":"task","id":"a"}))
                        .unwrap();
                    assert_eq!(task["fields"]["labels"], json!(["small", created["id"]]));
                    act(
                        UiAction::Form(Mode::Edit(serde_json::from_value(task).unwrap())),
                        w,
                        cx,
                    );
                    settle(&view, w, cx);
                    assert_eq!(
                        view.read(cx).form.as_ref().unwrap().values(cx).unwrap()["labels"],
                        json!(["small", created["id"]])
                    );
                })
                .unwrap();
            cx.quit();
        });
}
