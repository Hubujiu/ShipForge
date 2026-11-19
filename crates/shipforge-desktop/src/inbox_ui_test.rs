use super::*;
use crate::app::onboarding_ui::{activate_accessibility, buttons, settle};
use shipforge_core::Store;

#[::core::prelude::v1::test]
#[ignore = "Requires a Windows D3D/UI Automation session"]
fn hidden_inbox_and_routing_window() {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().to_owned();
    let mut store = Store::open(&dir.join("workspace.sqlite3")).unwrap();
    store.execute("owner",json!({"op":"onboarding.setup","organization_name":"Notices","organization_slug":"inbox-qa","work_type":"software","task_types":["task"]})).unwrap();
    store.execute("owner",json!({"op":"create","kind":"member","id":"alice","title":"Alice","fields":{"role":"member"}})).unwrap();
    store.execute("alice",json!({"op":"create","kind":"task","id":"urgent","title":"Open from notification","fields":{"assignee":"owner"}})).unwrap();
    store.execute("alice",json!({"op":"create","kind":"comment","id":"reply","title":"Please review","fields":{"task":"urgent","mentions":["owner"]}})).unwrap();
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
                        size(px(1440.), px(1000.)),
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
                    act(UiAction::Go(Page::Inbox), w, cx);
                    act(UiAction::Inbox(Action::Section("important".into())), w, cx);
                    let labels = buttons(w);
                    assert!(
                        labels.iter().any(|l| l == "Open from notification"),
                        "{labels:?}"
                    );
                    let r = view.read(cx).inbox.rows[0].clone();
                    act(UiAction::Inbox(Action::Open(r.clone())), w, cx);
                    assert_eq!(view.read(cx).loc.selected.as_ref().unwrap().id, "urgent");
                    assert_eq!(view.read(cx).loc.page, Page::Inbox);
                    act(UiAction::CloseDetail, w, cx);
                    assert_eq!(view.read(cx).loc.section, "important");
                    let r = view.read(cx).inbox.rows[0].clone();
                    assert_eq!(r.fields["read"], true);
                    act(UiAction::Inbox(Action::Change(r, "snooze".into())), w, cx);
                    act(UiAction::Submit, w, cx);
                    act(UiAction::Inbox(Action::Section("snoozed".into())), w, cx);
                    let r = view.read(cx).inbox.rows[0].clone();
                    act(UiAction::Inbox(Action::Change(r, "restore".into())), w, cx);
                    act(
                        UiAction::Inbox(Action::Settings(Some("desktop".into()))),
                        w,
                        cx,
                    );
                    assert!(buttons(w).iter().any(|s| s.contains("恢复渠道默认设置")));
                    act(
                        UiAction::Inbox(Action::Toggle {
                            channel: "desktop".into(),
                            key: "assignment".into(),
                            enabled: false,
                        }),
                        w,
                        cx,
                    );
                    assert_eq!(
                        store
                            .execute("owner", json!({"op":"inbox.settings"}))
                            .unwrap()["overrides"]["desktop"]["assignment"],
                        false
                    );
                    act(UiAction::Inbox(Action::Reset("desktop".into())), w, cx);
                    assert!(
                        store
                            .execute("owner", json!({"op":"inbox.settings"}))
                            .unwrap()["overrides"]["desktop"]
                            .is_null()
                    );
                    act(UiAction::Inbox(Action::Settings(Some("inbox".into()))),w,cx);
                    act(UiAction::Inbox(Action::Toggle{channel:"inbox".into(),key:"all".into(),enabled:false}),w,cx);
                    let cursor=store.execute("owner",json!({"op":"inbox.feed"})).unwrap()["cursor"].clone();
                    let unread=store.execute("owner",json!({"op":"inbox.query","section":"all"})).unwrap()["unread"].clone();
                    store.execute("alice",json!({"op":"create","kind":"task","id":"banner-only","title":"Desktop delivery","fields":{"assignee":"owner"}})).unwrap();
                    let feed=store.execute("owner",json!({"op":"inbox.feed","after":cursor})).unwrap();
                    let link=feed["items"][0]["link"].as_str().unwrap();
                    view.update(cx,|v,_|v.inbox_link(link));
                    settle(&view,w,cx);
                    assert_eq!(view.read(cx).loc.selected.as_ref().unwrap().id,"banner-only");
                    assert_eq!(store.execute("owner",json!({"op":"inbox.query","section":"all"})).unwrap()["unread"],unread);
                    assert_eq!(store.execute("owner",json!({"op":"inbox.feed","after":cursor})).unwrap()["items"],json!([]));
                })
                .unwrap();
            cx.quit();
        });
}
