use super::{
    UiAction, Workbench,
    agent_directory::{Action, DirectoryForm},
    onboarding_ui::{activate_accessibility, buttons, settle},
};
use crate::{catalog::Page, forms::Mode};
use gpui_kit::{prelude::*, *};
use serde_json::json;
use shipforge_client::Client;

#[::core::prelude::v1::test]
#[ignore = "Runs a native Windows D3D window and UI Automation tree"]
fn hidden_agent_directory_archive_variant_and_crew() {
    let tmp = tempfile::tempdir().unwrap();
    let data = tmp.path().to_owned();
    let mut cli = Client::local(&data).unwrap();
    for q in [
        json!({"op":"onboarding.setup","organization_name":"Agent QA","organization_slug":"agent-qa","work_type":"software","task_types":["bug"]}),
        json!({"op":"create","kind":"space","id":"second","title":"Second space","fields":{"visibility":"public"}}),
        json!({"op":"create","kind":"agent","id":"review","title":"Directory fixture","fields":{"instructions":"Inspect","runtime":"local-tool"}}),
        json!({"op":"create","kind":"agent","id":"helper","title":"Crew helper"}),
        json!({"op":"create","kind":"crew","id":"team","title":"Team fixture","fields":{"leader_id":"review","members":["review","helper"]}}),
        json!({"op":"run.enqueue","handler_id":"review","instruction":"Inspect"}),
    ] {
        cli.execute(q).unwrap();
    }
    gpui_kit::application().with_assets(crate::assets::Assets).run(move|cx|{
        gpui_kit::init(cx);
        let(window,view)=gpui_kit::open_window(WindowOptions{show:false,focus:false,window_bounds:Some(WindowBounds::Windowed(Bounds::centered(None,size(px(1580.),px(1500.)),cx))),..Default::default()},cx,|w,cx|cx.new(|cx|Workbench::new(Some(data.clone()),false,false,w,cx))).unwrap();
        window.update(cx,|_,w,cx|{
            activate_accessibility(w);settle(&view,w,cx);
            let act=|a,w:&mut Window,cx:&mut App|{view.update(cx,|v,cx|v.act(a,w,cx));settle(&view,w,cx);};
            let has=|w:&Window,label:&str|buttons(w).iter().any(|s|s==label);
            act(UiAction::Go(Page::Agents),w,cx);
            assert!(has(w,"Directory fixture")&&has(w,"已归档")&&has(w,"搜索目录"));
            let r=view.read(cx).find("agent","review").unwrap().clone();
            act(UiAction::Directory(Action::Open(r.clone())),w,cx);
            assert!(has(w,"创建分身")&&has(w,"归档成员")&&has(w,"发起对话")&&has(w,"运行历史"));
            act(UiAction::ViewAgentHistory(r.clone()),w,cx);
            assert!(!view.read(cx).list("agent_history").is_empty());assert!(has(w,"返回设置"));
            act(UiAction::Directory(Action::Settings),w,cx);
            act(UiAction::Form(Mode::Confirm{label:"归档成员".into(),command:json!({"op":"agent.archive","id":"review","kind":"agent","space":"default"})}),w,cx);
            act(UiAction::Submit,w,cx);assert!(has(w,"恢复使用"));
            act(UiAction::CloseDetail,w,cx);assert!(!has(w,"Directory fixture"));
            act(UiAction::Section("archived".into()),w,cx);assert!(has(w,"Directory fixture"));
            let archived=view.read(cx).list("agent_archived").iter().find(|v|v.id=="review").unwrap().clone();
            act(UiAction::Directory(Action::Open(archived)),w,cx);
            act(UiAction::Form(Mode::Confirm{label:"恢复使用".into(),command:json!({"op":"agent.restore","id":"review","kind":"agent","space":"default"})}),w,cx);
            act(UiAction::Submit,w,cx);
            let restored=view.read(cx).loc.selected.clone().unwrap();
            act(UiAction::Form(Mode::Directory(DirectoryForm::Variant(restored))),w,cx);
            act(UiAction::Pick(0),w,cx);act(UiAction::Choose(0,"second".into()),w,cx);act(UiAction::Submit,w,cx);
            assert_eq!(view.read(cx).loc.space,"second");assert!(has(w,"覆盖或恢复继承"));
            let original=view.read(cx).find("space","default").unwrap().clone();act(UiAction::SelectSpace(original),w,cx);
            act(UiAction::Go(Page::Crews),w,cx);let crew=view.read(cx).find("crew","team").unwrap().clone();act(UiAction::Directory(Action::Open(crew)),w,cx);
            assert!(has(w,"设为小队专属"));
            act(UiAction::Form(Mode::Confirm{label:"设为小队专属".into(),command:json!({"op":"crew.exclusive","id":"team","agent":"helper","exclusive":true})}),w,cx);
            act(UiAction::Submit,w,cx);assert!(has(w,"取消小队专属"));
            act(UiAction::Go(Page::Agents),w,cx);assert!(!has(w,"Crew helper"));
        }).unwrap();cx.quit();
    });
}
