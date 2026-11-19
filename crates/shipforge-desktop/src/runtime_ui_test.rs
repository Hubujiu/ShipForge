use super::{
    UiAction, Workbench,
    onboarding_ui::{activate_accessibility, buttons, settle},
};
use crate::{catalog::Page, forms::Mode};
use gpui_kit::{prelude::*, *};
use serde_json::json;
use shipforge_client::Client;

#[::core::prelude::v1::test]
#[ignore = "Runs a native Windows D3D window and UI Automation tree"]
fn hidden_runtime_policy_and_directory_selector() {
    let t = tempfile::tempdir().unwrap();
    let data = t.path().to_owned();
    let mut client = Client::local(&data).unwrap();
    for q in [
        json!({"op":"onboarding.setup","organization_name":"Runtime QA","organization_slug":"runtime-qa","work_type":"software","task_types":["task"]}),
        json!({"op":"machine.register","machine_id":"box","machine_key":"native-test-key-without-a-real-machine","title":"QA machine","runtimes":["local-tool"],"directories":[{"id":"First approved","identity":"a".repeat(64),"available":true,"direct":true,"git":false},{"id":"Second approved","identity":"b".repeat(64),"available":true,"direct":true,"git":false}]}),
        json!({"op":"create","kind":"agent","id":"one","title":"Runtime settings","fields":{"machine_id":"box","directory_mode":"direct","working_hours":{"timezone":"UTC","week":{}}}}),
        json!({"op":"create","kind":"task","id":"due","title":"Scheduled work","fields":{"status":"todo","assignee":"one","assignee_kind":"agent"}}),
    ] {
        client.execute(q).unwrap();
    }
    let queued = client.execute(json!({"op":"list","kind":"run"})).unwrap()[0]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    gpui_kit::application().with_assets(crate::assets::Assets).run(move|cx|{
        gpui_kit::init(cx);
        let(handle,view)=gpui_kit::open_window(WindowOptions{show:false,focus:false,window_bounds:Some(WindowBounds::Windowed(Bounds::centered(None,size(px(1450.),px(1720.)),cx))),..Default::default()},cx,|w,cx|cx.new(|cx|Workbench::new(Some(data.clone()),false,false,w,cx))).unwrap();
        handle.update(cx,|_,w,cx|{
            activate_accessibility(w);settle(&view,w,cx);
            let act=|a,w:&mut Window,cx:&mut App|{view.update(cx,|s,cx|s.act(a,w,cx));settle(&view,w,cx);};
            let index=|key:&str,cx:&App|view.read(cx).form.as_ref().unwrap().entries.iter().position(|e|e.spec.key==key).unwrap();
            let has=|w:&Window,name:&str|buttons(w).iter().any(|s|s==name);
            act(UiAction::Go(Page::Machines),w,cx);assert!(has(w,"本机进程与日志")&&has(w,"授权本机工作目录"));
            act(UiAction::Form(Mode::LocalDirectory{diagnose:false}),w,cx);assert!(has(w,"浏览文件"));act(UiAction::CloseForm,w,cx);
            act(UiAction::Go(Page::Agents),w,cx);
            let agent=view.read(cx).find("agent","one").unwrap().clone();act(UiAction::Form(Mode::Edit(agent)),w,cx);
            let profiles=index("profiles",cx);act(UiAction::Pick(profiles),w,cx);
            assert_eq!(view.read(cx).options("$approved_directories").len(),2);
            act(UiAction::Choose(profiles,"First approved".into()),w,cx);act(UiAction::Choose(profiles,"Second approved".into()),w,cx);
            let quota=index("chat_capacity_mode",cx);act(UiAction::Choose(quota,"shared".into()),w,cx);
            act(UiAction::Submit,w,cx);
            let saved=client.execute(json!({"op":"get","kind":"agent","id":"one"})).unwrap();assert_eq!(saved["fields"]["profiles"],json!(["First approved","Second approved"]));assert_eq!(saved["fields"]["chat_capacity_mode"],"shared");
            act(UiAction::Go(Page::Tasks),w,cx);let task=view.read(cx).find("task","due").unwrap().clone();act(UiAction::Open(task),w,cx);assert!(has(w,"立即触发"),"{:?}",buttons(w));
            act(UiAction::Apply(json!({"op":"run.trigger_now","id":queued}),"已进入可执行队列".into()),w,cx);assert!(!has(w,"立即触发"));
            act(UiAction::Go(Page::Runs),w,cx);let run=view.read(cx).find("run",&queued).unwrap().clone();act(UiAction::Open(run),w,cx);
            assert!(has(w,"运行诊断"));act(UiAction::Inspect(json!({"op":"run.diagnostics","id":queued}),"运行诊断".into()),w,cx);assert!(matches!(&view.read(cx).form.as_ref().unwrap().mode,Mode::Output{heading,..}if heading=="运行诊断"));
        }).unwrap();cx.quit();
    });
}
