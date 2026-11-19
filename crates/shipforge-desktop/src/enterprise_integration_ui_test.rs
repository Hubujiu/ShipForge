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
#[ignore = "Runs a hidden native Windows window"]
fn hidden_teambition_cnb_operations() {
    let fixture = tempfile::tempdir().unwrap();
    let directory = fixture.path().to_owned();
    let mut client = Client::local(&directory).unwrap();
    client.execute(json!({"op":"onboarding.setup","organization_name":"Pipeline QA","organization_slug":"pipeline-qa","work_type":"software","task_types":["bug","task"]})).unwrap();
    for (id, fields) in [
        (
            "asana",
            json!({"provider":"asana","enterprise_id":"11","remote_scope":"21","enabled":true,"vendor_write":true}),
        ),
        (
            "tb",
            json!({"provider":"teambition","enterprise_id":"fixture","remote_scope":"project","enabled":true}),
        ),
        (
            "cnb",
            json!({"provider":"cnb","cnb_group":"group","enabled":true}),
        ),
    ] {
        client
            .execute(json!({"op":"create","kind":"connector","id":id,"title":id,"fields":fields}))
            .unwrap();
    }
    let binding = client
        .execute(json!({"op":"imports.binding","id":"asana"}))
        .unwrap();
    let draft=client.execute(json!({"op":"imports.stage","id":"asana","project":"21","source_account":"11","binding_rev":binding["revision"],"checkpoint":binding["checkpoint"],"import_mode":"history","issues":[{"gid":"51","resource_type":"task","workspace":{"gid":"11"},"memberships":[{"project":{"gid":"21"}}],"completed":false,"name":"Native Asana task","notes":"Synthetic task","modified_at":"2025-09-01T00:00:00Z"}]})).unwrap();
    let imported = client
        .execute(json!({"op":"imports.commit","id":"asana","batch":draft["batch"]}))
        .unwrap();
    let task: crate::catalog::Record = serde_json::from_value(
        client
            .execute(json!({"op":"get","kind":"task","id":imported["items"][0]["task"]}))
            .unwrap(),
    )
    .unwrap();
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
                        size(px(1500.), px(1450.)),
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
                    for (id, ops) in [
                        (
                            "asana",
                            vec![
                                "asana.accounts",
                                "asana.credential",
                                "asana.projects",
                                "asana.preview",
                                "asana.synchronize",
                                "asana.push",
                            ],
                        ),
                        (
                            "tb",
                            vec![
                                "teambition.application",
                                "teambition.personal",
                                "teambition.projects",
                                "teambition.preview",
                                "teambition.push",
                                "imports.sync_control",
                            ],
                        ),
                        (
                            "cnb",
                            vec![
                                "cnb.credential",
                                "cnb.repositories",
                                "cnb.pair",
                                "cnb.ci",
                                "cnb.activity",
                                "cnb.deliver",
                            ],
                        ),
                    ] {
                        let row = app.read(cx).find("connector", id).unwrap().clone();
                        act(UiAction::Open(row.clone()), w, cx);
                        let labels = buttons(w);
                        for op in ops {
                            assert!(
                                labels.iter().any(|name| name == workflow_name(op)),
                                "missing {op}"
                            );
                            act(
                                UiAction::Form(Mode::Workflow {
                                    op,
                                    record: row.clone(),
                                }),
                                w,
                                cx,
                            );
                            let form = app.read(cx).form.as_ref().unwrap();
                            match op {
                                "teambition.application" => {
                                    assert!(form.entries.iter().any(|e| e.spec.key == "app_secret"
                                        && e.spec.format == Format::Secret))
                                }
                                "teambition.personal"
                                | "cnb.credential"
                                | "asana.credential"
                                | "asana.accounts" => assert!(
                                    form.entries.iter().any(|e| e.spec.key == "access_token"
                                        && e.spec.format == Format::Secret)
                                ),
                                "teambition.push" => assert!(
                                    form.entries
                                        .iter()
                                        .any(|e| e.spec.key == "confirm_external")
                                ),
                                "asana.push" => {
                                    assert!(
                                        form.entries.iter().any(|e| e.spec.key == "comment_id")
                                    );
                                    assert!(
                                        form.entries
                                            .iter()
                                            .any(|e| e.spec.key == "confirm_comment")
                                    );
                                }
                                "cnb.pair" => {
                                    assert!(form.entries.iter().any(|e| e.spec.key == "rotate"))
                                }
                                "imports.sync_control" => {
                                    act(UiAction::Submit, w, cx);
                                    assert_eq!(
                                        Client::local(&directory)
                                            .unwrap()
                                            .execute(json!({"op":"imports.binding","id":"tb"}))
                                            .unwrap()["sync_paused"],
                                        true
                                    );
                                }
                                "cnb.activity" => {
                                    act(UiAction::Submit, w, cx);
                                    assert!(matches!(
                                        app.read(cx).form.as_ref().map(|f| &f.mode),
                                        Some(Mode::Output { .. })
                                    ));
                                }
                                _ => {}
                            }
                            act(UiAction::CloseForm, w, cx);
                        }
                        act(UiAction::CloseDetail, w, cx);
                    }
                    act(UiAction::Open(task.clone()), w, cx);
                    act(UiAction::Form(Mode::Comment(task.clone())), w, cx);
                    let form = app.read(cx).form.as_ref().unwrap();
                    assert!(form.entries.iter().any(|e| e.spec.key == "publish_asana"));
                    assert_eq!(form.values(cx).unwrap()["publish_asana"], false);
                    act(UiAction::CloseForm, w, cx);
                })
                .unwrap();
            cx.quit();
        });
}
