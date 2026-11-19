use super::{
    UiAction, Workbench,
    onboarding_ui::{activate_accessibility, buttons, settle},
    task_assistant_ui::Action,
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
fn hidden_space_task_assistant_lifecycle() {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().to_owned();
    let mut cli = Client::local(&dir).unwrap();
    cli.execute(json!({"op":"onboarding.setup","organization_name":"System assistant QA","organization_slug":"assistant-qa","work_type":"software","task_types":["task"]})).unwrap();
    cli.execute(json!({"op":"create","kind":"connector","id":"team","title":"QA Slack channel","fields":{"provider":"slack","enabled":true,"outbound":true,"workspace_id":"T1","channel_id":"C1","app_id":"A","bot_id":"B"}})).unwrap();
    gpui_kit::application().with_assets(crate::assets::Assets).run(move|cx|{
        gpui_kit::init(cx);
        let (handle,view)=gpui_kit::open_window(WindowOptions{show:false,focus:false,window_bounds:Some(WindowBounds::Windowed(Bounds::centered(None,size(px(1480.),px(1440.)),cx))),..Default::default()},cx,|w,cx|cx.new(|cx|Workbench::new(Some(dir.clone()),false,false,w,cx))).unwrap();
        handle.update(cx,|_,w,cx|{
            activate_accessibility(w);settle(&view,w,cx);
            let act=|a,w:&mut Window,cx:&mut App|{view.update(cx,|v,cx|v.act(a,w,cx));settle(&view,w,cx);};
            let idx=|key:&str,cx:&App|view.read(cx).form.as_ref().unwrap().entries.iter().position(|e|e.spec.key==key).unwrap();
            let text=|key:&str,new:&str,w:&mut Window,cx:&mut App|{let f=&view.read(cx).form.as_ref().unwrap().entries[idx(key,cx)].control;match f{Control::Text(Field::Multi(t))=>t.clone().update(cx,|s,cx|s.set_value(new,w,cx)),Control::Text(Field::Single(t))=>t.clone().update(cx,|s,cx|s.set_value(new,w,cx)),_=>panic!("expected text field")};};
            let slot=|cx:&App|->Record{view.read(cx).list("assistant_catalog").iter().find(|r|r.field("provider")=="slack"&&r.space==view.read(cx).loc.space).unwrap().clone()};
            act(UiAction::Go(Page::Connectors),w,cx);assert!(buttons(w).iter().any(|l|l=="空间任务助手"));
            act(UiAction::TaskAssist(Action::Browse),w,cx);assert!(buttons(w).iter().any(|l|l=="绑定任务助手"));
            act(UiAction::TaskAssist(Action::Template("en")),w,cx);
            text("description","A fixture in {{spaceName}}",w,cx);act(UiAction::Submit,w,cx);
            assert_eq!(cli.execute(json!({"op":"task_assistant.template"})).unwrap()["templates"]["en"]["description"],"A fixture in {{spaceName}}");
            act(UiAction::TaskAssist(Action::Settings(slot(cx),"task_assistant.bind","zh-CN")),w,cx);
            act(UiAction::Choose(idx("connector",cx),"team".into()),w,cx);text("runtime","local-tool",w,cx);
            act(UiAction::Submit,w,cx);let bound=slot(cx);assert_eq!(bound.field("state"),"connected","{:?}",view.read(cx).form.as_ref().map(|f|&f.error));
            assert!(buttons(w).iter().any(|l|l=="英文设置"));act(UiAction::TaskAssist(Action::Settings(bound,"task_assistant.configure","en")),w,cx);
            act(UiAction::Choose(idx("custom_description",cx),"true".into()),w,cx);text("description","Local description",w,cx);act(UiAction::Submit,w,cx);
            let settings=cli.execute(json!({"op":"task_assistant.settings","provider":"slack","language":"en"})).unwrap();assert_eq!(settings["effective"]["description"],"Local description");
            act(UiAction::TaskAssist(Action::Settings(slot(cx),"task_assistant.configure","en")),w,cx);
            act(UiAction::Choose(idx("custom_description",cx),"false".into()),w,cx);act(UiAction::Submit,w,cx);
            assert_eq!(cli.execute(json!({"op":"task_assistant.settings","provider":"slack","language":"en"})).unwrap()["effective"]["overridden"]["description"],false);
            let current=slot(cx);act(UiAction::Form(Mode::Confirm{label:"解绑渠道".into(),command:json!({"op":"task_assistant.unbind","space":current.space,"provider":"slack","expected_rev":current.rev})}),w,cx);act(UiAction::Submit,w,cx);
            assert_eq!(slot(cx).field("state"),"disconnected");assert!(buttons(w).iter().any(|l|l=="移除任务助手"));
        }).unwrap();cx.quit();
    });
}
