use super::{
    UiAction, Workbench,
    onboarding_ui::{activate_accessibility, buttons, settle},
    workspace_ui::{Action, WorkForm},
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
#[ignore = "Runs a hidden native D3D window on a Windows workstation"]
fn hidden_work_collaboration_window() {
    let profile = tempfile::tempdir().unwrap();
    let dir = profile.path().to_owned();
    let mut client = Client::local(&dir).unwrap();
    client.execute(json!({"op":"onboarding.setup","organization_name":"Workspace","organization_slug":"work-qa","work_type":"software","task_types":["task"]})).unwrap();
    for (kind, id, fields) in [
        ("field", "obsolete", json!({"value_type":"text"})),
        ("field", "score", json!({"value_type":"number"})),
        (
            "task_type",
            "source",
            json!({"field_bindings":[{"field":"obsolete"}]}),
        ),
        (
            "task_type",
            "destination",
            json!({"field_bindings":[{"field":"score","required":true}]}),
        ),
        (
            "task",
            "demo-work",
            json!({"type":"source","custom":{"obsolete":"remove upon confirmation"}}),
        ),
        (
            "comment",
            "discussion",
            json!({"task":"demo-work","body":"Discuss"}),
        ),
        (
            "view",
            "saved",
            json!({"visibility":"private","layout":"list"}),
        ),
    ] {
        client
            .execute(json!({"op":"create","kind":kind,"id":id,"title":id,"fields":fields}))
            .unwrap();
    }
    gpui_kit::application().with_assets(crate::assets::Assets).run(move|cx|{
        gpui_kit::init(cx);
        let (handle,view)=gpui_kit::open_window(WindowOptions{show:false,focus:false,window_bounds:Some(WindowBounds::Windowed(Bounds::centered(None,size(px(1560.),px(1080.)),cx))),..Default::default()},cx,|w,cx|cx.new(|cx|Workbench::new(Some(dir.clone()),false,false,w,cx))).unwrap();
        handle.update(cx,|_,w,cx|{
            activate_accessibility(w);settle(&view,w,cx);
            let act=|a,w:&mut Window,cx:&mut App|view.update(cx,|v,cx|v.act(a,w,cx));
            let form_field=|key:&str,cx:&App|view.read(cx).form.as_ref().unwrap().entries.iter().position(|e|e.spec.key==key).unwrap();
            let set=|key:&str,value:&str,w:&mut Window,cx:&mut App|{
                let i=form_field(key,cx);
                let f=match &view.read(cx).form.as_ref().unwrap().entries[i].control{Control::Text(Field::Single(f))=>f.clone(),_=>panic!("expected text input")};
                f.update(cx,|f,cx|f.set_value(value,w,cx));
            };
            act(UiAction::Go(Page::Tasks),w,cx);settle(&view,w,cx);
            let original=view.read(cx).find("task","demo-work").unwrap().clone();
            act(UiAction::Edit(original.clone()),w,cx);
            act(UiAction::Choose(form_field("type",cx),"destination".into()),w,cx);
            assert!(view.read(cx).form.as_ref().unwrap().entries.iter().any(|e|e.bound.as_ref().is_some_and(|b|b.id=="score" && b.label.contains("必填"))));
            set("bound_value","2.5",w,cx);
            act(UiAction::Submit,w,cx);settle(&view,w,cx);
            assert!(matches!(&view.read(cx).form.as_ref().unwrap().mode,Mode::Work(WorkForm::Confirm{names,..}) if names.contains("obsolete")));
            assert_eq!(client.execute(json!({"op":"get","kind":"task","id":"demo-work"})).unwrap()["fields"]["custom"]["obsolete"],"remove upon confirmation");
            act(UiAction::Submit,w,cx);settle(&view,w,cx);
            let changed=client.execute(json!({"op":"get","kind":"task","id":"demo-work"})).unwrap();assert_eq!(changed["fields"]["type"],"destination");assert_eq!(changed["fields"]["custom"],json!({"score":2.5}));
            act(UiAction::Work(Action::Locate{task:"demo-work".into(),comment:"discussion".into()}),w,cx);settle(&view,w,cx);
            assert!(buttons(w).iter().any(|b|b=="用此评论解决"));
            act(UiAction::Apply(json!({"op":"comment.resolve","id":"discussion","task_id":"demo-work","expected_rev":1}),"saved".into()),w,cx);settle(&view,w,cx);
            if !buttons(w).iter().any(|b|b=="重新打开"){act(UiAction::Work(Action::Toggle("discussion".into())),w,cx);settle(&view,w,cx);}
            assert!(buttons(w).iter().any(|b|b=="重新打开"));
            act(UiAction::Go(Page::Tasks),w,cx);settle(&view,w,cx);act(UiAction::Work(Action::ShowViews),w,cx);settle(&view,w,cx);
            let labels=buttons(w);for label in ["另存为新视图","复制视图链接","置顶视图","打开视图链接"]{assert!(labels.iter().any(|b|b==label),"missing {label}: {labels:?}");}
            let saved=view.read(cx).find("view","saved").unwrap().clone();act(UiAction::OpenView(saved),w,cx);settle(&view,w,cx);
            act(UiAction::SaveView,w,cx);set("title","New saved view",w,cx);act(UiAction::Submit,w,cx);settle(&view,w,cx);
            let views=client.execute(json!({"op":"view.catalog"})).unwrap();assert!(views.as_array().unwrap().iter().any(|v|v["title"]=="New saved view" && v["fields"]["layout"]=="list"));
            let link=client.execute(json!({"op":"view.share","id":"saved"})).unwrap();view.update(cx,|work,_|work.open_view_link(link["link"].as_str().unwrap().into()));settle(&view,w,cx);
            assert_eq!(view.read(cx).loc.view.as_ref().unwrap().id,"saved");
        }).unwrap();cx.quit();
    });
}
