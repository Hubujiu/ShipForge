use super::{
    UiAction, Workbench,
    onboarding_ui::{activate_accessibility, buttons, settle},
    space_ui::Action,
};
use crate::{catalog::Page, forms::Mode};
use gpui_kit::{prelude::*, *};
use serde_json::json;
use shipforge_client::Client;

#[::core::prelude::v1::test]
#[ignore = "Runs a hidden native D3D window on a Windows workstation"]
fn hidden_space_lifecycle_and_navigation_window() {
    let fixture = tempfile::tempdir().unwrap();
    let dir = fixture.path().to_owned();
    let mut client = Client::local(&dir).unwrap();
    client.execute(json!({"op":"onboarding.setup","organization_name":"Space QA","organization_slug":"space-qa","work_type":"software","task_types":["requirement","bug"]})).unwrap();
    client.execute(json!({"op":"create","kind":"space","id":"qa","title":"Navigation QA","fields":{"visibility":"public","work_type":"software"}})).unwrap();
    client.execute(json!({"op":"create","kind":"task","space":"qa","id":"unfinished","title":"Archive acceptance"})).unwrap();
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
                        size(px(1400.), px(1300.)),
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
                    let act = |a, w: &mut Window, cx: &mut App| {
                        view.update(cx, |v, cx| v.act(a, w, cx));
                        settle(&view, w, cx);
                    };
                    let has = |w: &Window, label: &str| buttons(w).iter().any(|b| b == label);
                    act(UiAction::Go(Page::Spaces), w, cx);
                    for label in ["使用中", "已停用", "进入空间", "停用空间"] {
                        assert!(
                            has(w, label),
                            "missing directory action {label}: {:?}",
                            buttons(w)
                        );
                    }
                    let space = view.read(cx).find("space", "qa").unwrap().clone();
                    act(UiAction::SelectSpace(space), w, cx);
                    assert!(has(w, "更多") && has(w, "迭代"));
                    assert!(!has(w, "专项") && !has(w, "客户请求"));
                    act(UiAction::Go(Page::SpaceModules), w, cx);
                    assert!(has(w, "固定到侧栏") && has(w, "启用模块"));
                    act(UiAction::Go(Page::Projects), w, cx);
                    assert!(has(w, "专项"));
                    act(UiAction::Go(Page::Tasks), w, cx);
                    assert!(!has(w, "专项"));
                    act(UiAction::Go(Page::SpaceModules), w, cx);
                    act(UiAction::Space(Action::Pin("projects".into(), true)), w, cx);
                    act(UiAction::Go(Page::Tasks), w, cx);
                    assert!(has(w, "专项"));
                    act(UiAction::Go(Page::CustomerRequests), w, cx);
                    assert_eq!(view.read(cx).loc.page, Page::SpaceModules);
                    act(
                        UiAction::Space(Action::Enable("customer_requests".into())),
                        w,
                        cx,
                    );
                    act(UiAction::Go(Page::CustomerRequests), w, cx);
                    assert_eq!(view.read(cx).loc.page, Page::CustomerRequests);
                    assert!(has(w, "新建客户请求") && has(w, "客户请求"));
                    act(UiAction::Go(Page::Spaces), w, cx);
                    let space = view.read(cx).find("space", "qa").unwrap().clone();
                    act(UiAction::Space(Action::Preview(space)), w, cx);
                    assert!(matches!(
                        view.read(cx).form.as_ref().map(|f| &f.mode),
                        Some(Mode::DeactivateSpace { unfinished: 1, .. })
                    ));
                    act(UiAction::Submit, w, cx);
                    assert!(!view.read(cx).form.as_ref().unwrap().error.is_empty());
                    act(UiAction::Choose(0, "cancel".into()), w, cx);
                    act(UiAction::Submit, w, cx);
                    assert_eq!(view.read(cx).loc.section, "archived");
                    assert!(has(w, "查看历史") && has(w, "恢复空间"));
                    let archived = view.read(cx).find("space", "qa").unwrap().clone();
                    act(UiAction::SelectSpace(archived.clone()), w, cx);
                    assert!(!view.read(cx).writable());
                    assert!(!has(w, "新建任务") && !has(w, "在此新建任务"));
                    assert!(view.read(cx).find("task", "unfinished").is_some());
                    act(UiAction::Go(Page::Spaces), w, cx);
                    act(UiAction::Section("archived".into()), w, cx);
                    act(UiAction::Space(Action::Restore(archived)), w, cx);
                    assert!(has(w, "确认"));
                    act(UiAction::Submit, w, cx);
                    assert!(view.read(cx).writable());
                    let mut client = Client::local(&dir).unwrap();
                    let task = client
                        .execute(json!({"op":"get","kind":"task","id":"unfinished","space":"qa"}))
                        .unwrap();
                    assert_eq!(task["fields"]["status"], "cancelled");
                })
                .unwrap();
            cx.quit();
        });
}
