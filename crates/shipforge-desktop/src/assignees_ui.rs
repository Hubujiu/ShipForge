use crate::{
    app::{
        UiAction, Workbench,
        onboarding_ui::{activate_accessibility, buttons, settle},
    },
    catalog::Page,
    controls::Field,
    forms::{Control, Format},
};
use gpui_kit::{prelude::*, *};
use serde_json::{Value, json};
use shipforge_client::Client;

#[::core::prelude::v1::test]
#[ignore = "Runs a hidden native D3D window on a Windows workstation"]
fn hidden_multi_assignee_window() {
    let profile = tempfile::tempdir().unwrap();
    let dir = profile.path().to_owned();
    let mut client = Client::local(&dir).unwrap();
    client.execute(json!({"op":"onboarding.setup","organization_name":"Acceptance","organization_slug":"acceptance","work_type":"software","task_types":["task"]})).unwrap();
    client
        .execute(json!({"op":"preferences.update","preferences":{"open_after_assignment":false}}))
        .unwrap();
    for id in ["alice", "bob"] {
        client.execute(json!({"op":"create","kind":"member","id":id,"title":id,"fields":{"role":"member"}})).unwrap();
    }
    client.execute(json!({"op":"create","kind":"agent","id":"runner","title":"Review worker","fields":{"visibility":"space"}})).unwrap();
    client.execute(json!({"op":"create","kind":"view","id":"people","title":"Assigned members","fields":{"layout":"board","group":"assignee"}})).unwrap();
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
                        size(px(1500.), px(1000.)),
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
                        view.update(cx, |a, cx| a.act(action, w, cx))
                    };
                    let index = |key: &str, cx: &App| {
                        view.read(cx)
                            .form
                            .as_ref()
                            .unwrap()
                            .entries
                            .iter()
                            .position(|field| field.spec.key == key)
                            .unwrap()
                    };
                    act(UiAction::Go(Page::Tasks), w, cx);
                    settle(&view, w, cx);
                    act(UiAction::Create("task".into()), w, cx);
                    settle(&view, w, cx);
                    assert!(matches!(
                        view.read(cx).form.as_ref().unwrap().entries[index("assignees", cx)]
                            .spec
                            .format,
                        Format::Ref("member")
                    ));
                    act(UiAction::CloseForm, w, cx);
                    act(UiAction::Go(Page::Spaces), w, cx);
                    settle(&view, w, cx);
                    let space = view.read(cx).find("space", "default").unwrap().clone();
                    act(UiAction::Edit(space), w, cx);
                    settle(&view, w, cx);
                    let i = index("allow_multiple_assignees", cx);
                    act(UiAction::Choose(i, "true".into()), w, cx);
                    act(UiAction::Submit, w, cx);
                    settle(&view, w, cx);
                    assert_eq!(
                        client
                            .execute(json!({"op":"get","kind":"space","id":"default"}))
                            .unwrap()["fields"]["allow_multiple_assignees"],
                        true
                    );
                    act(UiAction::Go(Page::Tasks), w, cx);
                    settle(&view, w, cx);
                    act(UiAction::Create("task".into()), w, cx);
                    settle(&view, w, cx);
                    let i = index("assignees", cx);
                    assert!(matches!(
                        view.read(cx).form.as_ref().unwrap().entries[i].spec.format,
                        Format::Many("member")
                    ));
                    let Control::Text(Field::Single(title)) =
                        &view.read(cx).form.as_ref().unwrap().entries[index("title", cx)].control
                    else {
                        panic!("task title")
                    };
                    title
                        .clone()
                        .update(cx, |s, cx| s.set_value("Membership check", w, cx));
                    act(UiAction::Choose(i, "alice".into()), w, cx);
                    act(UiAction::Choose(i, "bob".into()), w, cx);
                    let worker = index("executor", cx);
                    act(UiAction::Choose(worker, "agent:runner".into()), w, cx);
                    act(UiAction::Submit, w, cx);
                    settle(&view, w, cx);
                    let saved = client.execute(json!({"op":"list","kind":"task"})).unwrap();
                    assert_eq!(saved[0]["fields"]["assignees"], json!(["alice", "bob"]));
                    assert_eq!(
                        saved[0]["fields"]["executor"],
                        json!({"kind":"agent","id":"runner"})
                    );
                    act(UiAction::Go(Page::Views), w, cx);
                    settle(&view, w, cx);
                    let grouped = view.read(cx).find("view", "people").unwrap().clone();
                    act(UiAction::OpenView(grouped), w, cx);
                    settle(&view, w, cx);
                    let visible = buttons(w);
                    assert_eq!(
                        visible
                            .iter()
                            .filter(|s| s.as_str() == "Membership check")
                            .count(),
                        2,
                        "one native card for each assigned member"
                    );
                    assert_eq!(
                        view.read(cx).task_rows().len(),
                        1,
                        "one unique task in the view"
                    );
                    let r = view.read(cx).task_rows().remove(0);
                    act(UiAction::Edit(r), w, cx);
                    let i = index("assignees", cx);
                    act(UiAction::Choose(i, "alice".into()), w, cx);
                    act(UiAction::Submit, w, cx);
                    settle(&view, w, cx);
                    let updated = client.execute(json!({"op":"list","kind":"task"})).unwrap();
                    assert_eq!(updated[0]["fields"]["assignees"], json!(["bob"]));
                    assert_ne!(updated[0]["fields"]["executor"], Value::Null);
                })
                .unwrap();
            cx.quit();
        });
}
