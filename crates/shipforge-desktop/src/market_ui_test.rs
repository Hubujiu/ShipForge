use super::{
    UiAction, Workbench,
    market_ui::LibraryForm,
    onboarding_ui::{activate_accessibility, buttons, settle},
};
use crate::{catalog::Page, forms::Mode};
use gpui_kit::{prelude::*, *};
use serde_json::json;
use shipforge_client::Client;

#[::core::prelude::v1::test]
#[ignore = "Runs a hidden Windows GPUI window with UI Automation"]
fn hidden_market_install_search_import_and_crew_directory() {
    let tmp = tempfile::tempdir().unwrap();
    let data = tmp.path().to_owned();
    let skill = data.join("example.skill");
    std::fs::write(&skill, "# Windows fixture\nRead the file").unwrap();
    let mut cli = Client::local(&data).unwrap();
    for q in [
        json!({"op":"onboarding.setup","organization_name":"QA","organization_slug":"qa","work_type":"software","task_types":["bug"]}),
        json!({"op":"create","kind":"agent","id":"helper","title":"Helper"}),
        json!({"op":"create","kind":"crew","id":"crew","title":"Crew","fields":{"leader_id":"helper"}}),
    ] {
        cli.execute(q).unwrap();
    }
    gpui_kit::application().with_assets(crate::assets::Assets).run(move|cx|{
        gpui_kit::init(cx);
        let(window,view)=gpui_kit::open_window(WindowOptions{show:false,focus:false,window_bounds:Some(WindowBounds::Windowed(Bounds::centered(None,size(px(1480.),px(1200.)),cx))),..Default::default()},cx,|w,cx|cx.new(|cx|Workbench::new(Some(data.clone()),false,false,w,cx))).unwrap();
        window.update(cx,|_,w,cx|{
            activate_accessibility(w);settle(&view,w,cx);
            let act=|action,w:&mut Window,cx:&mut App|{view.update(cx,|v,cx|v.act(action,w,cx));settle(&view,w,cx);};
            let has=|w:&Window,name:&str|buttons(w).iter().any(|b|b==name);
            act(UiAction::Go(Page::Market),w,cx);assert!(has(w,"搜索市场")&&has(w,"安装技能")&&has(w,"发布市场版本"));
            let card=view.read(cx).list("catalog").iter().find(|r|r.id=="acceptance-check").unwrap().clone();
            act(UiAction::LibraryDetails(card.id.clone()),w,cx);assert!(matches!(&view.read(cx).form.as_ref().unwrap().mode,Mode::Output{content,..} if content.contains("验收")));
            act(UiAction::CloseForm,w,cx);
            act(UiAction::Form(Mode::Library(LibraryForm::Install(card))),w,cx);
            act(UiAction::Pick(0),w,cx);act(UiAction::Choose(0,"helper".into()),w,cx);act(UiAction::Submit,w,cx);
            act(UiAction::CloseForm,w,cx);assert!(has(w,"卸载技能"));
            assert_eq!(cli.execute(json!({"op":"agent.resolve","id":"helper"})).unwrap()["skills"],json!(["template-acceptance-check"]));
            act(UiAction::Form(Mode::Library(LibraryForm::Search)),w,cx);
            act(UiAction::Pick(1),w,cx);act(UiAction::Choose(1,"office".into()),w,cx);act(UiAction::Submit,w,cx);
            assert!(!has(w,"查看详情"));
            act(UiAction::Go(Page::Skills),w,cx);assert!(has(w,"导入技能文件夹")&&has(w,"从机器发现技能")&&has(w,"空间连接器"));
            let fields=serde_json::Map::from_iter([("path".into(),json!(skill))]);
            view.update(cx,|v,cx|v.open_prefilled(Mode::Library(LibraryForm::File),fields,w,cx));
            act(UiAction::Submit,w,cx);act(UiAction::CloseForm,w,cx);
            assert!(cli.execute(json!({"op":"list","kind":"skill"})).unwrap().as_array().unwrap().iter().any(|r|r["title"]=="Windows fixture"));
            act(UiAction::Go(Page::Crews),w,cx);let crew=view.read(cx).find("crew","crew").unwrap().clone();act(UiAction::Directory(super::agent_directory::Action::Open(crew)),w,cx);
            assert!(has(w,"开启同机共享任务目录"));
            act(UiAction::Apply(json!({"op":"update","kind":"crew","id":"crew","fields":{"share_task_directory":true}}),"设置已更新".into()),w,cx);
            assert!(has(w,"关闭同机共享任务目录"));
        }).unwrap();cx.quit();
    });
}
