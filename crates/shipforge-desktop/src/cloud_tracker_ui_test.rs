use super::{
    UiAction, Workbench,
    onboarding_ui::{activate_accessibility, buttons, settle},
};
use crate::{
    catalog::Page,
    forms::{Format, Mode, workflow_name},
};
use gpui_kit::{prelude::*, *};
use serde_json::json;
use shipforge_client::Client;

#[::core::prelude::v1::test]
#[ignore = "Runs a hidden native D3D window on Windows"]
fn hidden_cloud_tracker_operations() {
    let fixture = tempfile::tempdir().unwrap();
    let directory = fixture.path().to_owned();
    let mut client = Client::local(&directory).unwrap();
    client.execute(json!({"op":"onboarding.setup","organization_name":"Cloud Tracker QA","organization_slug":"cloud-tracker-qa","work_type":"software","task_types":["bug","task"]})).unwrap();
    for (id, fields) in [
        (
            "lin",
            json!({"provider":"linear","cloud_auth":"oauth","enterprise_id":"org","remote_scope":"project"}),
        ),
        (
            "click",
            json!({"provider":"clickup","cloud_auth":"oauth","enterprise_id":"team","remote_scope":"list"}),
        ),
        (
            "one",
            json!({"provider":"ones","ones_url":"https://qa.invalid","enterprise_id":"team","remote_scope":"project"}),
        ),
    ] {
        client
            .execute(json!({"op":"create","kind":"connector","id":id,"title":id,"fields":fields}))
            .unwrap();
    }
    drop(client);
    gpui_kit::application()
        .with_assets(crate::assets::Assets)
        .run(move |cx| {
            gpui_kit::init(cx);
            let (window, app) = gpui_kit::open_window(
                WindowOptions {
                    show: false,
                    focus: false,
                    window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                        None,
                        size(px(1500.), px(1400.)),
                        cx,
                    ))),
                    ..Default::default()
                },
                cx,
                |w, cx| cx.new(|cx| Workbench::new(Some(directory.clone()), false, false, w, cx)),
            )
            .unwrap();
            window
                .update(cx, |_, w, cx| {
                    activate_accessibility(w);
                    settle(&app, w, cx);
                    let act = |a, w: &mut Window, cx: &mut App| {
                        app.update(cx, |v, cx| v.act(a, w, cx));
                        settle(&app, w, cx);
                    };
                    act(UiAction::Go(Page::Connectors), w, cx);
                    for (id, authorize, credentials, discover, preview) in [
                        (
                            "lin",
                            Some("linear.authorize"),
                            "linear.credential",
                            "linear.projects",
                            "linear.preview",
                        ),
                        (
                            "click",
                            Some("clickup.authorize"),
                            "clickup.credential",
                            "clickup.projects",
                            "clickup.preview",
                        ),
                        (
                            "one",
                            None,
                            "ones.credential",
                            "ones.projects",
                            "ones.preview",
                        ),
                    ] {
                        let row = app.read(cx).find("connector", id).unwrap().clone();
                        act(UiAction::Open(row.clone()), w, cx);
                        let labels = buttons(w);
                        for op in [
                            credentials,
                            discover,
                            preview,
                            "imports.push",
                            "imports.rollback",
                            "imports.sync_control",
                        ] {
                            assert!(
                                labels.iter().any(|l| l == workflow_name(op)),
                                "missing {op}: {labels:?}"
                            );
                        }
                        if let Some(op) = authorize {
                            assert!(labels.iter().any(|l| l == workflow_name(op)));
                            act(
                                UiAction::Form(Mode::Workflow {
                                    op,
                                    record: row.clone(),
                                }),
                                w,
                                cx,
                            );
                            assert!(
                                app.read(cx)
                                    .form
                                    .as_ref()
                                    .unwrap()
                                    .entries
                                    .iter()
                                    .any(|e| e.spec.key == "redirect_uri")
                            );
                            act(UiAction::CloseForm, w, cx);
                        }
                        act(
                            UiAction::Form(Mode::Workflow {
                                op: credentials,
                                record: row.clone(),
                            }),
                            w,
                            cx,
                        );
                        assert!(app.read(cx).form.as_ref().unwrap().entries.iter().any(|e| {
                            e.spec.key == "access_token" && e.spec.format == Format::Secret
                        }));
                        act(UiAction::CloseForm, w, cx);
                        act(
                            UiAction::Form(Mode::Workflow {
                                op: preview,
                                record: row.clone(),
                            }),
                            w,
                            cx,
                        );
                        act(UiAction::Choose(0, "changes".into()), w, cx);
                        assert_eq!(
                            app.read(cx).form.as_ref().unwrap().values(cx).unwrap()["import_mode"],
                            "changes"
                        );
                        act(UiAction::CloseForm, w, cx);
                        act(
                            UiAction::Form(Mode::Workflow {
                                op: "imports.sync_control",
                                record: row,
                            }),
                            w,
                            cx,
                        );
                        act(UiAction::Submit, w, cx);
                        let mut client = Client::local(&directory).unwrap();
                        assert_eq!(
                            client
                                .execute(json!({"op":"imports.binding","id":id}))
                                .unwrap()["sync_paused"],
                            true
                        );
                        act(UiAction::CloseForm, w, cx);
                        act(UiAction::CloseDetail, w, cx);
                    }
                })
                .unwrap();
            cx.quit();
        });
}
