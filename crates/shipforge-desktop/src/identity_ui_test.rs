use super::IdentityForm;
use crate::{
    app::{
        UiAction, Workbench,
        onboarding_ui::{activate_accessibility, buttons, settle},
    },
    controls::Field,
    forms::{Control, Mode},
};
use gpui_kit::{prelude::*, *};
use serde_json::Value;
use std::{
    io::{BufRead, BufReader},
    net::TcpListener,
    os::windows::process::CommandExt,
    process::{Child, Command, Stdio},
    sync::mpsc,
    time::Duration,
};

struct Team(Child);
impl Drop for Team {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[::core::prelude::v1::test]
#[ignore = "Runs a hidden Windows window, team service and private mailbox"]
fn hidden_verified_account_lifecycle() {
    let tmp = tempfile::tempdir().unwrap();
    let device = tmp.path().join("device");
    let port = TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap();
    let base = format!("http://{port}");
    let gateway = tiny_http::Server::http("127.0.0.1:0").unwrap();
    let gateway_url = format!("http://{}/emails", gateway.server_addr());
    let (out, mail) = mpsc::channel::<String>();
    std::thread::spawn(move || {
        for _ in 0..2 {
            let Some(mut request) = gateway.recv_timeout(Duration::from_secs(24)).unwrap() else {
                break;
            };
            let body: Value = serde_json::from_reader(request.as_reader()).unwrap();
            let link = body["text"]
                .as_str()
                .unwrap()
                .lines()
                .find(|l| l.starts_with("http://"))
                .unwrap()
                .to_string();
            request
                .respond(tiny_http::Response::from_string("{}"))
                .unwrap();
            if out.send(link).is_err() {
                break;
            }
        }
    });
    let exe = std::env::current_exe().unwrap();
    let mut service = Team(
        Command::new(
            exe.parent()
                .unwrap()
                .parent()
                .unwrap()
                .join("shipforge-server.exe"),
        )
        .creation_flags(0x08000000)
        .arg("--data-dir")
        .arg(tmp.path().join("team"))
        .args(["serve", "--listen", &port.to_string()])
        .env("SHIPFORGE_IDENTITY_BASE_URL", &base)
        .env("SHIPFORGE_IDENTITY_MAIL_URL", gateway_url)
        .env("SHIPFORGE_IDENTITY_MAIL_FROM", "example@example.test")
        .env("SHIPFORGE_IDENTITY_MAIL_TOKEN", "test-only")
        .stdout(Stdio::piped())
        .spawn()
        .unwrap(),
    );
    let mut line = String::new();
    BufReader::new(service.0.stdout.take().unwrap())
        .read_line(&mut line)
        .unwrap();
    assert!(line.contains("listen"));
    gpui_kit::application().with_assets(crate::assets::Assets).run(move |cx|{
        gpui_kit::init(cx);
        let (handle,view)=gpui_kit::open_window(WindowOptions{show:false,focus:false,window_bounds:Some(WindowBounds::Windowed(Bounds::centered(None,size(px(1400.),px(950.)),cx))),..Default::default()},cx,|w,cx|cx.new(|cx|Workbench::new(Some(device.clone()),false,false,w,cx))).unwrap();
        handle.update(cx,|_,w,cx|{
            activate_accessibility(w);
            let act=|a,w:&mut Window,cx:&mut App|{view.update(cx,|s,cx|s.act(a,w,cx));settle(&view,w,cx);};
            let set=|key:&str,value:&str,w:&mut Window,cx:&mut App|{
                let form=view.read(cx).form.as_ref().unwrap();
                let field=form.entries.iter().find(|e|e.spec.key==key).unwrap();
                let Control::Text(Field::Single(input))=&field.control else{panic!("expected single line field")};
                input.clone().update(cx,|s,cx|s.set_value(value.to_owned(),w,cx));
            };
            let approve=|cx:&App|{
                let Mode::Identity(IdentityForm::Verify{pair,..})=&view.read(cx).form.as_ref().unwrap().mode else{panic!("expected native pairing code")};
                let link=mail.recv_timeout(Duration::from_secs(4)).unwrap();
                assert_eq!(ureq::get(&link).call().unwrap().status(),200);
                let mut url=url::Url::parse(&link).unwrap();
                let body=format!("{}&pair={pair}",url.query().unwrap());
                url.set_query(None);
                let html=ureq::post(url.as_str()).header("Content-Type","application/x-www-form-urlencoded").send(body.as_bytes()).unwrap().body_mut().read_to_string().unwrap();
                let link=html.split_once("href=\"shipforge://identity/").unwrap().1.split('"').next().unwrap();
                shipforge_client::identity_link_id(&format!("shipforge://identity/{link}")).unwrap()
            };
            settle(&view,w,cx);
            act(UiAction::Accounts,w,cx);
            assert!(buttons(w).iter().any(|s|s=="使用邮箱登录或注册"));
            act(UiAction::Form(Mode::Identity(IdentityForm::Login(None))),w,cx);
            set("url",&base,w,cx);set("email","native@example.test",w,cx);
            act(UiAction::Submit,w,cx);let callback=approve(cx);
            act(UiAction::CloseForm,w,cx);
            view.update(cx,|s,_|s.open_identity_link(callback));settle(&view,w,cx);
            assert!(matches!(&view.read(cx).form.as_ref().unwrap().mode,Mode::Identity(IdentityForm::Verify{closing:false,..})));
            act(UiAction::Submit,w,cx);
            assert!(view.read(cx).actor.starts_with("acct-"),"actor {:?}, error {:?}",view.read(cx).actor,view.read(cx).error);
            assert!(view.read(cx).loc.space.starts_with("org-"));
            act(UiAction::Accounts,w,cx);
            assert!(buttons(w).iter().any(|s|s=="永久注销账号"));
            act(UiAction::IdentityProfile,w,cx);
            set("name","Example name",w,cx);act(UiAction::Submit,w,cx);
            act(UiAction::IdentityProfile,w,cx);
            assert!(matches!(&view.read(cx).form.as_ref().unwrap().mode,Mode::Identity(IdentityForm::Profile(v)) if v["name"]=="Example name"));
            act(UiAction::CloseForm,w,cx);act(UiAction::IdentityClose,w,cx);
            assert!(matches!(&view.read(cx).form.as_ref().unwrap().mode,Mode::Identity(IdentityForm::Impact(v)) if v["organizations"][0]["action"]=="dissolve_solo"));
            act(UiAction::Submit,w,cx);let callback=approve(cx);
            act(UiAction::CloseForm,w,cx);
            view.update(cx,|s,_|s.open_identity_link(callback));settle(&view,w,cx);
            assert!(matches!(&view.read(cx).form.as_ref().unwrap().mode,Mode::Identity(IdentityForm::Verify{closing:true,..})));
            set("confirmation","注销",w,cx);act(UiAction::Submit,w,cx);
            assert_eq!(view.read(cx).mode,"local");
        }).unwrap();
        cx.quit();
    });
    drop(service);
}
