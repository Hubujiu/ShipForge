use super::{
    UiAction, Workbench,
    automation_run_ui::Action,
    onboarding_ui::{activate_accessibility, buttons, settle},
};
use crate::{catalog::Page, forms::Mode};
use gpui_kit::{prelude::*, *};
use serde_json::json;
use shipforge_client::Client;

#[::core::prelude::v1::test]
#[ignore = "Uses a hidden native D3D window and the Windows accessibility tree"]
fn hidden_automation_history_pages_conversation_and_lifecycle() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().to_owned();
    let mut cli = Client::local(&dir).unwrap();
    for q in [
        json!({"op":"onboarding.setup","organization_name":"Rule history QA","organization_slug":"rule-history-qa","work_type":"software","task_types":["task"]}),
        json!({"op":"create","kind":"agent","id":"executor","title":"Verifier","fields":{}}),
        json!({"op":"create","kind":"automation","id":"rule","title":"Release check","fields":{"handler_id":"executor","instruction":"Run acceptance","enabled":true}}),
    ] {
        cli.execute(q).unwrap();
    }
    for i in 0..9 {
        cli.execute(json!({"op":"automation.fire","id":"rule","event_id":format!("gui-{i}")}))
            .unwrap();
    }
    gpui_kit::application().with_assets(crate::assets::Assets).run(move|cx|{
        gpui_kit::init(cx);
        let(handle,view)=gpui_kit::open_window(WindowOptions{show:false,focus:false,window_bounds:Some(WindowBounds::Windowed(Bounds::centered(None,size(px(1500.),px(1800.)),cx))),..Default::default()},cx,|w,cx|cx.new(|cx|Workbench::new(Some(dir.clone()),false,false,w,cx))).unwrap();
        handle.update(cx,|_,w,cx|{
            activate_accessibility(w);settle(&view,w,cx);
            let act=|a,w:&mut Window,cx:&mut App|{view.update(cx,|v,cx|v.act(a,w,cx));settle(&view,w,cx);};
            let shown=|w:&mut Window,label:&str|buttons(w).iter().any(|s|s==label);
            act(UiAction::Go(Page::Automations),w,cx);let rule=view.read(cx).find("automation","rule").unwrap().clone();act(UiAction::Open(rule),w,cx);
            assert!(shown(w,"暂停规则") && shown(w,"下一页"),"{:?}",buttons(w));
            let cursor=view.read(cx).find("automation_history","rule").unwrap().field("next_cursor").to_owned();
            act(UiAction::RuleHistory(Action::Next("rule".into(),cursor)),w,cx);
            assert!(shown(w,"上一页") && !shown(w,"下一页"));
            let page=view.read(cx).find("automation_history","rule").unwrap();let first=page.fields["items"][0]["id"].as_str().unwrap().to_owned();
            act(UiAction::RuleHistory(Action::Open("rule".into(),first.clone())),w,cx);
            let chat=view.read(cx).loc.selected.as_ref().unwrap();assert_eq!(chat.kind,"chat");assert_eq!(chat.field("automation_run"),first);assert!(shown(w,"发送消息"));
            act(UiAction::Go(Page::Automations),w,cx);
            for (action,label,expected) in [("pause","暂停规则","paused"),("resume","恢复规则","enabled"),("archive","归档规则","archived")]{
                let rule=view.read(cx).find("automation","rule").unwrap().clone();act(UiAction::Open(rule.clone()),w,cx);assert!(shown(w,label));
                act(UiAction::Form(Mode::Confirm{label:label.into(),command:json!({"op":"automation.lifecycle","id":"rule","space":rule.space,"action":action,"expected_rev":rule.rev})}),w,cx);act(UiAction::Submit,w,cx);
                assert_eq!(cli.execute(json!({"op":"automation.status","id":"rule"})).unwrap()["state"],expected);
            }
        }).unwrap();cx.quit();
    });
}
