use super::{
    UiAction, Workbench,
    onboarding_ui::{activate_accessibility, buttons, settle},
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
fn hidden_personal_assistant_and_group_avatar() {
    let profile = tempfile::tempdir().unwrap();
    let path = profile.path().to_owned();
    let mut member = Client::local(&path).unwrap();
    member.execute(json!({"op":"onboarding.setup","organization_name":"Assistant QA","organization_slug":"assistant-qa","work_type":"software","task_types":["task"]})).unwrap();
    member.execute(json!({"op":"machine.register","machine_id":"fixture","machine_key":"fixture-worker-key-for-private-assistant","runtimes":["local-tool"],"title":"Native fixture"})).unwrap();
    let group: Record = serde_json::from_value(
        member
            .execute(json!({"op":"chat.start","mode":"group","title":"Native group"}))
            .unwrap(),
    )
    .unwrap();
    let pic = path.join("group.png");
    std::fs::write(&pic,shipforge_client::png_data_url("data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAIAAAABCAIAAAB7QOjdAAAAD0lEQVR4nGMQsyr5H2EIAAdbAk2AAYTqAAAAAElFTkSuQmCC").unwrap()).unwrap();
    gpui_kit::application().with_assets(crate::assets::Assets).run(move|cx|{
        gpui_kit::init(cx);
        let (handle,view)=gpui_kit::open_window(WindowOptions{show:false,focus:false,window_bounds:Some(WindowBounds::Windowed(Bounds::centered(None,size(px(1450.),px(1200.)),cx))),..Default::default()},cx,|w,cx|cx.new(|cx|Workbench::new(Some(path.clone()),false,false,w,cx))).unwrap();
        handle.update(cx,|_,w,cx|{
            activate_accessibility(w);settle(&view,w,cx);
            let act=|action,w:&mut Window,cx:&mut App|{view.update(cx,|v,cx|v.act(action,w,cx));settle(&view,w,cx);};
            act(UiAction::Go(Page::Assistant),w,cx);
            let home=&view.read(cx).list("assistant_home")[0];assert!(home.fields["id"].is_string());assert_eq!(home.fields["rooms"],json!([]));
            assert!(buttons(w).iter().any(|label|label=="选择 Runtime 和模型"));
            let Field::Multi(input)=view.read(cx).assistant_draft.clone()else{panic!("missing native composer")};
            input.update(cx,|s,cx|s.set_value("Native private question",w,cx));
            act(UiAction::SendAssistantDraft,w,cx);
            let opened=view.read(cx).loc.selected.as_ref().expect("sent room opened");assert_eq!(opened.field("handler_kind"),"assistant");
            let sent=opened.id.clone();
            act(UiAction::Go(Page::Assistant),w,cx);
            assert!(view.read(cx).assistant_draft.value(cx).is_empty());assert!(buttons(w).iter().any(|name|name=="Native private question"));
            let item:Record=serde_json::from_value(view.read(cx).list("assistant_home")[0].fields["rooms"][0].clone()).unwrap();
            act(UiAction::OpenSearch(item),w,cx);assert_eq!(view.read(cx).loc.selected.as_ref().unwrap().id,sent);
            act(UiAction::OpenSearch(group.clone()),w,cx);
            assert!(buttons(w).iter().any(|name|name=="更换群头像"));
            act(UiAction::Form(Mode::Workflow{op:"chat.avatar.set",record:group.clone()}),w,cx);
            let Control::Text(Field::Single(input))=&view.read(cx).form.as_ref().unwrap().entries[0].control else{panic!("missing file picker")};
            input.clone().update(cx,|s,cx|s.set_value(pic.to_string_lossy().to_string(),w,cx));
            assert!(buttons(w).iter().any(|name|name=="浏览文件"));act(UiAction::Submit,w,cx);
            let info=member.execute(json!({"op":"chat.avatar","id":group.id})).unwrap();assert_eq!(info["avatar"]["width"],128);
            let tree:serde_json::Value=serde_json::from_str(&w.debug_a11y_tree_json().unwrap()).unwrap();
            assert!(tree["nodes"].as_object().unwrap().values().any(|n|n["aria"]["role"]=="Image" && n["aria"]["label"]=="群头像"));
            assert!(buttons(w).iter().any(|name|name=="移除群头像"));
            act(UiAction::Form(Mode::Confirm{label:"移除群头像".into(),command:json!({"op":"chat.avatar.set","space":group.space,"id":group.id,"expected_rev":info["rev"],"remove":true})}),w,cx);
            act(UiAction::Submit,w,cx);assert!(member.execute(json!({"op":"chat.avatar","id":group.id})).unwrap()["avatar"].is_null());
        }).unwrap();cx.quit();
    });
}
