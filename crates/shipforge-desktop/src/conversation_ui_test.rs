use super::{
    UiAction, Workbench,
    conversation_ui::{Action, CommandForm},
    onboarding_ui::{activate_accessibility, buttons, settle},
};
use crate::{
    catalog::Page,
    controls::Field,
    forms::{Control, Mode},
};
use gpui_kit::{prelude::*, *};
use serde_json::{Value, json};
use shipforge_client::Client;

#[::core::prelude::v1::test]
#[ignore = "Runs a hidden native D3D window on a Windows workstation"]
fn hidden_conversation_commands_and_document_window() {
    let profile = tempfile::tempdir().unwrap();
    let directory = profile.path().to_owned();
    let mut client = Client::local(&directory).unwrap();
    let mut exec = |q: Value| {
        client
            .execute(q.clone())
            .unwrap_or_else(|e| panic!("{q}: {e}"))
    };
    exec(
        json!({"op":"onboarding.setup","organization_name":"Workspace","organization_slug":"command-tests","work_type":"software","task_types":["task"]}),
    );
    for (kind, id, fields) in [
        (
            "document",
            "notes",
            json!({"body":"# Notes","visibility":"space"}),
        ),
        ("agent", "inspector", json!({"runtime":"sample"})),
        ("task", "check", json!({"status":"todo"})),
    ] {
        exec(json!({"op":"create","kind":kind,"id":id,"title":id,"fields":fields}));
    }
    let link = exec(json!({"op":"document.share","id":"notes"}));
    exec(json!({"op":"update","kind":"task","id":"check","fields":{"description":link["link"]}}));
    exec(
        json!({"op":"machine.register","machine_id":"demo","title":"Demo","machine_key":"synthetic-desktop-test-machine-key","runtimes":["sample"],"slash_commands":{"sample":["/plan"]}}),
    );
    let cmd = exec(
        json!({"op":"shortcut.save","title":"Inspect task","handler_id":"inspector","prompt":"Review result"}),
    );
    gpui_kit::application().with_assets(crate::assets::Assets).run(move|cx|{
        gpui_kit::init(cx);
        let(handle,view)=gpui_kit::open_window(WindowOptions{show:false,focus:false,window_bounds:Some(WindowBounds::Windowed(Bounds::centered(None,size(px(1540.),px(1040.)),cx))),..Default::default()},cx,|w,cx|cx.new(|cx|Workbench::new(Some(directory.clone()),false,false,w,cx))).unwrap();
        handle.update(cx,|_,w,cx|{
            activate_accessibility(w);settle(&view,w,cx);
            let act=|a,w:&mut Window,cx:&mut App|view.update(cx,|v,cx|v.act(a,w,cx));
            let label=|window:&Window,s:&str|{let current=buttons(window);assert!(current.iter().any(|l|l==s),"missing {s}: {current:?}");};
            let task=view.read(cx).find("task","check").unwrap().clone();act(UiAction::Open(task.clone()),w,cx);settle(&view,w,cx);
            label(w,"快捷指令");label(w,"预览引用文档");
            act(UiAction::Conversation(Action::Embeds(task.clone())),w,cx);settle(&view,w,cx);label(w,"打开 notes");
            act(UiAction::Conversation(Action::Open(link["link"].as_str().unwrap().into())),w,cx);settle(&view,w,cx);
            assert_eq!(view.read(cx).loc.selected.as_ref().unwrap().kind,"document");label(w,"文档大纲");label(w,"复制文档链接");
            act(UiAction::OpenSearch(task.clone()),w,cx);settle(&view,w,cx);
            act(UiAction::Conversation(Action::Shortcuts(task.clone())),w,cx);settle(&view,w,cx);label(w,"Inspect task");
            act(UiAction::Conversation(Action::Manage),w,cx);settle(&view,w,cx);label(w,"新建快捷指令");label(w,"取消收藏");
            act(UiAction::Conversation(Action::Run(cmd["id"].as_str().unwrap().into(),"check".into())),w,cx);settle(&view,w,cx);
            let room=view.read(cx).loc.selected.as_ref().unwrap().clone();assert_eq!(room.kind,"chat");
            act(UiAction::Form(Mode::Message(room.clone())),w,cx);
            let input=view.read(cx).form.as_ref().unwrap().entries.iter().find(|e|e.spec.key=="body").unwrap().clone();
            let Control::Text(Field::Multi(field))=&input.control else{panic!("message composer");};
            field.update(cx,|s,cx|s.set_value("/p",w,cx));input.handle(cx).focus(w,cx);settle(&view,w,cx);label(w,"/plan");
            w.dispatch_keystroke(Keystroke::parse("tab").unwrap(),cx);settle(&view,w,cx);
            assert_eq!(input.raw(cx),"/plan ");
            assert_eq!(client.execute(json!({"op":"chat.history","id":room.id})).unwrap().as_array().unwrap().len(),1);
            field.update(cx,|s,cx|s.set_value("/",w,cx));settle(&view,w,cx);label(w,"/plan");w.dispatch_keystroke(Keystroke::parse("escape").unwrap(),cx);settle(&view,w,cx);
            assert!(view.read(cx).form.is_some());assert!(!buttons(w).iter().any(|s|s=="/plan"));
            field.update(cx,|s,cx|s.set_value("/plan now",w,cx));act(UiAction::Submit,w,cx);settle(&view,w,cx);
            let job=client.execute(json!({"op":"run.claim","machine_id":"demo","machine_key":"synthetic-desktop-test-machine-key"})).unwrap();
            client.execute(json!({"op":"run.report","id":job["run"]["id"],"machine_id":"demo","machine_key":"synthetic-desktop-test-machine-key","lease":job["lease"],"status":"done","result":"Checked"})).unwrap();
            act(UiAction::Refresh,w,cx);settle(&view,w,cx);
            let messages=client.execute(json!({"op":"chat.history","id":room.id})).unwrap();
            let answer=messages.as_array().unwrap().iter().find(|m|m["fields"]["body"]=="Checked").unwrap();
            let share=client.execute(json!({"op":"chat.share.create","id":room.id,"message_ids":[answer["id"]],"include_questions":true})).unwrap();
            act(UiAction::Refresh,w,cx);settle(&view,w,cx);label(w,"保存分享图片");
            act(UiAction::Form(Mode::Conversation(CommandForm::Image(share["share"].as_str().unwrap().into()))),w,cx);
            let path=directory.join("checked.png");
            let file=view.read(cx).form.as_ref().unwrap().entries[0].clone();let Control::Text(Field::Single(f))=&file.control else{panic!("file picker");};
            f.update(cx,|v,cx|v.set_value(path.to_string_lossy().into_owned(),w,cx));
            act(UiAction::Submit,w,cx);settle(&view,w,cx);assert!(std::fs::read(path).unwrap().starts_with(b"\x89PNG"));
            act(UiAction::Go(Page::Tasks),w,cx);settle(&view,w,cx);let task=view.read(cx).find("task","check").unwrap().clone();
            act(UiAction::Open(task.clone()),w,cx);settle(&view,w,cx);label(w,"已审阅，没问题");
            act(UiAction::Form(Mode::Confirm{label:"发送「已审阅，没问题」".into(),command:json!({"op":"task.review","space":task.space,"id":task.id,"expected_rev":task.rev})}),w,cx);
            act(UiAction::Submit,w,cx);settle(&view,w,cx);assert!(!buttons(w).iter().any(|s|s=="已审阅，没问题"));
            let checked=client.execute(json!({"op":"get","kind":"task","id":"check"})).unwrap();assert_eq!(checked["fields"]["awaiting_review"],false);assert_eq!(checked["fields"]["status"],"todo");
        }).unwrap();cx.quit();
    });
}
