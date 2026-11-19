use super::{
    UiAction, Workbench,
    machine_management::{Action, Dialog},
    onboarding_ui::{activate_accessibility, buttons, settle},
};
use crate::{catalog::Page, forms::Mode};
use gpui_kit::{prelude::*, *};
use serde_json::{Value, json};
use shipforge_client::Client;
#[::core::prelude::v1::test]
#[ignore = "Runs real Windows D3D and inspects its native accessibility tree"]
fn hidden_storage_confirmation_and_model_configuration() {
    let t = tempfile::tempdir().unwrap();
    let data = t.path().to_owned();
    let mut c = Client::local(&data).unwrap();
    let key = "fixture-only-worker-key-for-storage-receipts";
    let mut call = |q: Value| c.execute(q.clone()).unwrap_or_else(|e| panic!("{q} · {e}"));
    call(
        json!({"op":"onboarding.setup","organization_name":"Storage QA","organization_slug":"storage-qa","work_type":"software","task_types":["task"]}),
    );
    call(
        json!({"op":"machine.register","machine_id":"box","machine_key":key,"title":"QA machine","runtimes":["fixture"]}),
    );
    call(
        json!({"op":"machine.runtime.publish","machine_id":"box","machine_key":key,"capabilities":[{"name":"fixture","kind":"runtime","status":"available","launch":{"models":{"small":["high"]},"default_model":"small","environment_names":["QA_THEME"],"argument_choices":{"--style":["short"]}}}]}),
    );
    call(
        json!({"op":"create","kind":"agent","id":"a","title":"QA Agent","fields":{"runtime":"fixture","machine_id":"box"}}),
    );
    let r = call(json!({"op":"run.enqueue","handler_id":"a","instruction":"Native fixture"}));
    let claim = call(json!({"op":"run.claim","machine_id":"box","machine_key":key}));
    call(
        json!({"op":"run.report","id":r["id"],"machine_id":"box","machine_key":key,"lease":claim["lease"],"status":"done","result":"done","artifacts":[]}),
    );
    call(json!({"op":"machine.storage.request","id":"box","action":"scan"}));
    let ticket = call(json!({"op":"machine.storage.take","machine_id":"box","machine_key":key}));
    let entry = json!({"id":"c".repeat(64),"category":"run","bytes":8192,"files":2,"snapshot":"a".repeat(64),"runs":[r["id"]]});
    call(
        json!({"op":"machine.storage.finish","id":ticket["id"],"lease":ticket["lease"],"machine_id":"box","machine_key":key,"outcome":"success","inventory":{"entries":[entry],"total_bytes":8192}}),
    );
    gpui_kit::application().with_assets(crate::assets::Assets).run(move|cx|{
        gpui_kit::init(cx);let(handle,view)=gpui_kit::open_window(WindowOptions{show:false,focus:false,window_bounds:Some(WindowBounds::Windowed(Bounds::centered(None,size(px(1440.),px(1400.)),cx))),..Default::default()},cx,|w,cx|cx.new(|cx|Workbench::new(Some(data.clone()),false,false,w,cx))).unwrap();
        handle.update(cx,|_,w,cx|{
            activate_accessibility(w);settle(&view,w,cx);let act=|a,w:&mut Window,cx:&mut App|{view.update(cx,|s,cx|s.act(a,w,cx));settle(&view,w,cx);};let has=|s:&str,w:&Window|buttons(w).iter().any(|b|b==s);
            act(UiAction::Go(Page::Machines),w,cx);let m=view.read(cx).find("machine","box").unwrap().clone();act(UiAction::Open(m),w,cx);assert!(has("管理存储空间",w),"{:?}",buttons(w));
            act(UiAction::MachineManagement(Action::Storage("box".into())),w,cx);assert!(has("重新统计",w)&&has("清理工作目录",w));
            act(UiAction::Form(Mode::MachineManagement(Dialog::CleanStorage("box".into(),entry.clone()))),w,cx);assert!(has("确认清理",w));act(UiAction::CloseForm,w,cx);assert_eq!(c.execute(json!({"op":"machine.storage.preview","id":"box"})).unwrap()["jobs"].as_array().unwrap().len(),1);
            act(UiAction::Form(Mode::MachineManagement(Dialog::CleanStorage("box".into(),entry.clone()))),w,cx);act(UiAction::Submit,w,cx);assert_eq!(c.execute(json!({"op":"machine.storage.preview","id":"box"})).unwrap()["jobs"][0]["action"],"clean");act(UiAction::CloseForm,w,cx);
            act(UiAction::Go(Page::Agents),w,cx);let agent=view.read(cx).find("agent","a").unwrap().clone();act(UiAction::Open(agent.clone()),w,cx);assert!(has("模型与运行参数",w));act(UiAction::MachineManagement(Action::AgentLaunch("a".into())),w,cx);assert!(has("small",w)&&has("small · high",w),"{:?}",buttons(w));
            act(UiAction::Apply(json!({"op":"update","kind":"agent","id":"a","expected_rev":agent.rev,"fields":{"model":"small","reasoning_effort":"high"}}),"已保存模型设置".into()),w,cx);
            assert_eq!(c.execute(json!({"op":"agent.runtime.options","id":"a"})).unwrap()["reasoning_effort"],"high");
        }).unwrap();cx.quit();
    });
}
