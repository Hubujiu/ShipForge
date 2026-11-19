use super::{
    UiAction, Workbench,
    onboarding_ui::{activate_accessibility, buttons, settle},
};
use crate::{
    catalog::Page,
    controls::Field,
    forms::{Control, Format, Mode, workflow_name},
};
use gpui_kit::{prelude::*, *};
use serde_json::json;
use shipforge_client::Client;

#[::core::prelude::v1::test]
#[ignore = "Runs a hidden native D3D window on a Windows workstation"]
fn hidden_tracker_connectors_and_import_dialogs() {
    let fixture = tempfile::tempdir().unwrap();
    let dir = fixture.path().to_owned();
    let mut client = Client::local(&dir).unwrap();
    client.execute(json!({"op":"onboarding.setup","organization_name":"Tracker QA","organization_slug":"tracker-qa","work_type":"software","task_types":["bug","task"]})).unwrap();
    for (key, fields) in [
        (
            "zen",
            json!({"provider":"zentao","instance_url":"https://tracker.invalid","scope_kind":"product","remote_scope":"7"}),
        ),
        (
            "pc",
            json!({"provider":"pingcode","enterprise_id":"qa","remote_scope":"demo","pingcode_auth":"authorization"}),
        ),
    ] {
        client
            .execute(json!({"op":"create","kind":"connector","id":key,"title":key,"fields":fields}))
            .unwrap();
    }
    let binding = client
        .execute(json!({"op":"imports.binding","id":"pc"}))
        .unwrap();
    let preview=client.execute(json!({"op":"imports.stage","id":"pc","binding_rev":binding["revision"],"checkpoint":binding["checkpoint"],"project":"demo","issues":[{"id":"123","project":{"id":"demo"},"title":"Native tracker fixture","state":{"id":"open","name":"open","type":"pending"},"type":{"id":"native"},"updated_at":1700000000}]})).unwrap();
    let batch = preview["batch"].as_str().unwrap().to_owned();
    drop(client);
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
                        size(px(1440.), px(1400.)),
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
                        view.update(cx, |app, cx| app.act(action, w, cx));
                        settle(&view, w, cx);
                    };
                    act(UiAction::Go(Page::Connectors), w, cx);
                    for (key, credential, discover, preview, secret) in [
                        (
                            "zen",
                            "zentao.credential",
                            "zentao.projects",
                            "zentao.preview",
                            "password",
                        ),
                        (
                            "pc",
                            "pingcode.credential",
                            "pingcode.projects",
                            "pingcode.preview",
                            "code",
                        ),
                    ] {
                        let connector = view.read(cx).find("connector", key).unwrap().clone();
                        act(UiAction::Open(connector.clone()), w, cx);
                        let labels = buttons(w);
                        for op in [
                            credential,
                            discover,
                            preview,
                            "imports.synchronize",
                            "imports.commit",
                            "imports.rollback",
                            "imports.push",
                            "imports.push_history",
                        ] {
                            assert!(
                                labels.iter().any(|l| l == workflow_name(op)),
                                "{op} missing among {labels:?}"
                            );
                        }
                        act(
                            UiAction::Form(Mode::Workflow {
                                op: credential,
                                record: connector.clone(),
                            }),
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
                                .any(|e| e.spec.key == secret && e.spec.format == Format::Secret)
                        );
                        act(UiAction::CloseForm, w, cx);
                        act(
                            UiAction::Form(Mode::Workflow {
                                op: preview,
                                record: connector,
                            }),
                            w,
                            cx,
                        );
                        act(UiAction::Choose(0, "changes".into()), w, cx);
                        assert_eq!(
                            view.read(cx).form.as_ref().unwrap().values(cx).unwrap()["import_mode"],
                            "changes"
                        );
                        act(UiAction::CloseForm, w, cx);
                        act(UiAction::CloseDetail, w, cx);
                    }
                    let connector = view.read(cx).find("connector", "pc").unwrap().clone();
                    for (op, expected) in [("imports.commit", 1), ("imports.rollback", 0)] {
                        act(
                            UiAction::Form(Mode::Workflow {
                                op,
                                record: connector.clone(),
                            }),
                            w,
                            cx,
                        );
                        let input = match &view.read(cx).form.as_ref().unwrap().entries[0].control {
                            Control::Text(Field::Single(input)) => input.clone(),
                            _ => panic!("expected batch ID input"),
                        };
                        input.update(cx, |field, cx| field.set_value(batch.clone(), w, cx));
                        act(UiAction::Submit, w, cx);
                        assert!(matches!(
                            view.read(cx).form.as_ref().map(|f| &f.mode),
                            Some(Mode::Output { .. })
                        ));
                        let mut client = Client::local(&dir).unwrap();
                        let rows = client.execute(json!({"op":"list","kind":"task"})).unwrap();
                        assert_eq!(rows.as_array().unwrap().len(), expected);
                        act(UiAction::CloseForm, w, cx);
                    }
                })
                .unwrap();
            cx.quit();
        });
}
