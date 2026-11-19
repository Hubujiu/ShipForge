use super::{
    UiAction, Workbench,
    chat_actions::Action,
    onboarding_ui::{activate_accessibility, buttons, settle},
};
use crate::{
    catalog::Page,
    controls::Field,
    forms::{Control, Mode},
};
use gpui_kit::{prelude::*, *};
use serde_json::json;
use shipforge_client::Client;

#[::core::prelude::v1::test]
#[ignore = "Runs a hidden Windows D3D window and its native accessibility tree"]
fn hidden_chat_list_fork_and_binding() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().to_owned();
    let mut cli = Client::local(&dir).unwrap();
    for q in [
        json!({"op":"onboarding.setup","organization_name":"Chat QA","organization_slug":"chat-qa","work_type":"software","task_types":["requirement","bug"]}),
        json!({"op":"create","kind":"agent","id":"synthetic","title":"Synthetic agent","fields":{"runtime":"local-tool"}}),
        json!({"op":"create","kind":"chat","id":"room","title":"Conversation fixture","fields":{"agent":"synthetic"}}),
        json!({"op":"create","kind":"task","id":"target","title":"Unique search target"}),
        json!({"op":"create","kind":"message","id":"first","title":"First message","fields":{"chat":"room","body":"Fork here"}}),
    ] {
        cli.execute(q).unwrap();
    }
    drop(cli);
    gpui_kit::application().with_assets(crate::assets::Assets).run(move|cx|{
        gpui_kit::init(cx);
        let (window,view)=gpui_kit::open_window(WindowOptions{show:false,focus:false,window_bounds:Some(WindowBounds::Windowed(Bounds::centered(None,size(px(1500.),px(1600.)),cx))),..Default::default()},cx,|w,cx|cx.new(|cx|Workbench::new(Some(dir.clone()),false,false,w,cx))).unwrap();
        window.update(cx,|_,w,cx|{
            activate_accessibility(w);settle(&view,w,cx);
            let act=|a,w:&mut Window,cx:&mut App|{view.update(cx,|v,cx|v.act(a,w,cx));settle(&view,w,cx);};
            let has=|w:&Window,label:&str|buttons(w).iter().any(|s|s==label);
            act(UiAction::Go(Page::Chats),w,cx);
            assert!(has(w,"Conversation fixture")&&has(w,"置顶")&&has(w,"不显示"));
            let organize=|op,w:&mut Window,cx:&mut App|act(UiAction::Chat(Action::Organize{room:"room".into(),action:op}),w,cx);
            organize("pin",w,cx);
            assert!(has(w,"取消置顶"));
            organize("hide",w,cx);
            assert!(!has(w,"Conversation fixture")&&has(w,"撤销"));
            act(UiAction::Chat(Action::Undo),w,cx);
            assert!(has(w,"Conversation fixture")&&has(w,"取消置顶"));
            organize("hide",w,cx);
            let mut cli=Client::local(&dir).unwrap();
            cli.execute(json!({"op":"create","kind":"message","id":"later","title":"Another turn","fields":{"chat":"room","body":"Later message"}})).unwrap();
            act(UiAction::Refresh,w,cx);
            assert!(has(w,"Conversation fixture")&&has(w,"置顶")&&!has(w,"撤销"));
            let room=view.read(cx).find("chat","room").unwrap().clone();
            act(UiAction::OpenSearch(room),w,cx);
            assert!(has(w,"从此消息分叉")&&has(w,"绑定到已有任务")&&has(w,"将对话转为任务"));
            act(UiAction::Chat(Action::Fork{room:"room".into(),message:"first".into()}),w,cx);
            let fork=view.read(cx).loc.selected.as_ref().unwrap().clone();
            assert_ne!(fork.id,"room");
            assert_eq!(view.read(cx).list("message").len(),1);
            assert_eq!(view.read(cx).list("message")[0].field("body"),"Fork here");
            act(UiAction::Form(Mode::BindChat(fork.clone())),w,cx);
            let Control::Text(Field::Single(input))=&view.read(cx).form.as_ref().unwrap().entries[0].control else{panic!("search box")};
            input.clone().update(cx,|s,cx|s.set_value("Unique search target",w,cx));
            act(UiAction::Submit,w,cx);
            assert!(has(w,"Unique search target"));
            act(UiAction::Chat(Action::Bind{room:fork.clone(),task:"target".into()}),w,cx);
            assert!(has(w,"Unique search target"));
            assert_eq!(view.read(cx).loc.selected.as_ref().unwrap().field("task"),"target");
            let task=view.read(cx).find("task","target").unwrap().clone();
            act(UiAction::OpenSearch(task),w,cx);
            assert!(has(w,&fork.title));
            let source=cli.execute(json!({"op":"get","kind":"chat","id":"room"})).unwrap();
            assert!(source["fields"]["task"].is_null());
        }).unwrap();
        cx.quit();
    });
}

#[::core::prelude::v1::test]
#[ignore = "Runs an actual Windows D3D window with a lease-reported conversation workspace"]
fn hidden_chat_workspace_choices() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().to_owned();
    let mut cli = Client::local(&dir).unwrap();
    let key = "fixture-key-for-native-conversation";
    for q in [
        json!({"op":"onboarding.setup","organization_name":"Workspace QA","organization_slug":"workspace-qa","work_type":"software","task_types":["requirement"]}),
        json!({"op":"create","kind":"agent","id":"builder","title":"Git helper","fields":{"runtime":"local-tool"}}),
        json!({"op":"create","kind":"chat","id":"room","title":"Existing conversation","fields":{"agent":"builder"}}),
        json!({"op":"machine.register","machine_id":"box","machine_key":key,"title":"Fixture host","runtimes":["local-tool"]}),
    ] {
        cli.execute(q).unwrap();
    }
    let anchor = cli
        .execute(json!({"op":"chat.send","id":"room","body":"Begin a working copy"}))
        .unwrap()["message"]
        .as_str()
        .unwrap()
        .to_owned();
    let in_progress = cli
        .execute(json!({"op":"run.claim","machine_id":"box","machine_key":key}))
        .unwrap();
    cli.execute(json!({"op":"run.report","machine_id":"box","machine_key":key,"id":in_progress["run"]["id"],
        "lease":in_progress["lease"],"status":"done","result":"Worktree is ready","workspace_checkpoint":{"git":true,"repository":"https://git.example.test/team/qa.git"}})).unwrap();
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
                        size(px(1400.), px(1080.)),
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
                    let act = |action, w: &mut Window, cx: &mut App| {
                        view.update(cx, |v, cx| v.act(action, w, cx));
                        settle(&view, w, cx);
                    };
                    act(UiAction::Go(Page::Chats), w, cx);
                    let room = view
                        .read(cx)
                        .find("chat", "room")
                        .expect("fixture in the chat directory")
                        .clone();
                    act(UiAction::OpenSearch(room), w, cx);
                    assert!(buttons(w).iter().any(|label| label == "使用工作目录分叉"));
                    act(
                        UiAction::Chat(Action::ForkWorkspace {
                            room: "room".into(),
                            message: anchor.clone(),
                        }),
                        w,
                        cx,
                    );
                    assert!(matches!(
                        view.read(cx).form.as_ref().map(|f| &f.mode),
                        Some(Mode::Workflow {
                            op: "chat.fork.workspace",
                            ..
                        })
                    ));
                    act(UiAction::Choose(0, "branch".into()), w, cx);
                    let field = view
                        .read(cx)
                        .form
                        .as_ref()
                        .unwrap()
                        .entries
                        .iter()
                        .find(|entry| entry.spec.key == "checkout_branch")
                        .unwrap();
                    let Control::Text(Field::Single(input)) = &field.control else {
                        panic!("branch input")
                    };
                    input
                        .clone()
                        .update(cx, |s, cx| s.set_value("release/windows", w, cx));
                    act(UiAction::Submit, w, cx);
                    let id = view.read(cx).loc.selected.as_ref().unwrap().id.clone();
                    assert_ne!(id, "room");
                    let fork = cli
                        .execute(json!({"op":"get","kind":"chat","id":id}))
                        .unwrap();
                    assert_eq!(
                        fork["fields"]["directory_fork"]["checkout_branch"],
                        "release/windows"
                    );
                    assert_eq!(fork["fields"]["directory_fork"]["mode"], "branch");
                })
                .unwrap();
            cx.quit();
        });
}
