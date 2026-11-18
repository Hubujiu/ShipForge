use super::{
    UiAction, Workbench,
    onboarding::{Action, Menu},
};
use crate::{
    catalog::Page,
    controls::Field,
    forms::{Control, Mode},
};
use gpui_kit::{prelude::*, *};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use serde_json::{Value, json};
use shipforge_client::Client;
use shipforge_core::onboarding::groups;
use std::{
    path::Path,
    time::{Duration, Instant},
};

pub(crate) fn settle(view: &Entity<Workbench>, window: &mut Window, cx: &mut App) {
    let deadline = Instant::now() + Duration::from_secs(6);
    loop {
        view.update(cx, |app, cx| app.tick(window, cx));
        let app = view.read(cx);
        if app.personal.rev > 0 && !app.busy && !app.loading && app.pending.is_empty() {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "onboarding backend did not finish"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
    window.refresh();
    window.draw(cx).clear(cx);
}

fn progress(dir: &Path) -> Value {
    Client::local(dir)
        .unwrap()
        .execute(json!({"op":"onboarding.get"}))
        .unwrap()
}

// Ask the hidden test window for its native UI Automation provider exactly as a
// Windows accessibility client does. No application window needs foreground focus.
pub(crate) fn activate_accessibility(window: &Window) {
    #[link(name = "user32")]
    unsafe extern "system" {
        fn SendMessageW(hwnd: isize, msg: u32, wp: usize, lp: isize) -> isize;
    }
    let RawWindowHandle::Win32(native) = HasWindowHandle::window_handle(window).unwrap().as_raw()
    else {
        panic!("a Windows HWND is required")
    };
    // SAFETY: the handle is borrowed from the live window on its owning thread.
    unsafe {
        SendMessageW(native.hwnd.get(), 0x003d, 0, -25);
    }
}

pub(crate) fn buttons(window: &Window) -> Vec<String> {
    let json: Value = serde_json::from_str(
        &window
            .debug_a11y_tree_json()
            .expect("native accessibility tree"),
    )
    .unwrap();
    json["nodes"]
        .as_object()
        .unwrap()
        .values()
        .filter(|n| n["aria"]["role"] == "Button")
        .filter_map(|n| n["aria"]["label"].as_str().map(str::to_owned))
        .collect()
}

#[::core::prelude::v1::test]
#[ignore = "Runs a hidden native D3D window on a Windows workstation"]
fn hidden_onboarding_window() {
    let profile = tempfile::tempdir().unwrap();
    let dir = profile.path().to_owned();
    gpui_kit::application().with_assets(crate::assets::Assets).run(move |cx| {
        gpui_kit::init(cx);
        let (handle, view) = gpui_kit::open_window(WindowOptions {
            show:false, focus:false,
            window_bounds:Some(WindowBounds::Windowed(Bounds::centered(None,size(px(1400.),px(1800.)),cx))),
            ..Default::default()
        }, cx, |window,cx| cx.new(|cx| Workbench::new(Some(dir.clone()), false, false, window, cx))).unwrap();
        handle.update(cx, |_, w, cx| {
            activate_accessibility(w);
            settle(&view,w,cx);
            let act = |action,w:&mut Window,cx:&mut App| view.update(cx,|a,cx|a.act(action,w,cx));
            let guide = |action,w:&mut Window,cx:&mut App| act(UiAction::Guide(action),w,cx);
            let form = |cx:&App| view.read(cx).form.as_ref().map(|f|f.mode.clone());
            assert!(matches!(form(cx),Some(Mode::SetupOrganization)));
            assert!(buttons(w).iter().any(|s|s=="继续"));
            assert_eq!(progress(&dir)["setup_required"],true);
            act(UiAction::Submit,w,cx);
            assert!(!view.read(cx).form.as_ref().unwrap().error.is_empty());
            for (index,text) in [(0,"Onboarding Test"),(1,"onboarding-test")] {
                let Control::Text(Field::Single(input))=&view.read(cx).form.as_ref().unwrap().entries[index].control else {panic!("setup text field")};
                input.clone().update(cx,|s,cx|s.set_value(text,w,cx));
            }
            act(UiAction::Submit,w,cx);
            settle(&view,w,cx);
            assert!(matches!(form(cx),Some(Mode::SetupSpace{organization,slug}) if organization=="Onboarding Test" && slug=="onboarding-test"));
            assert!(buttons(w).iter().any(|s|s=="进入工作区"));
            act(UiAction::Choose(0,"other".into()),w,cx);
            act(UiAction::Choose(1,"story".into()),w,cx);
            act(UiAction::Submit,w,cx);
            settle(&view,w,cx);
            assert!(form(cx).is_none());
            assert_eq!(view.read(cx).loc.page,Page::GettingStarted);
            let mut store=Client::local(&dir).unwrap();
            let org=store.execute(json!({"op":"get","kind":"organization","id":"default"})).unwrap();
            assert_eq!(org["title"],"Onboarding Test");
            assert_eq!(org["fields"]["slug"],"onboarding-test");
            let types=store.execute(json!({"op":"list","kind":"task_type"})).unwrap();
            let enabled:std::collections::BTreeSet<_>=types.as_array().unwrap().iter().filter(|r|r["fields"]["enabled"]==true).map(|r|r["id"].as_str().unwrap()).collect();
            assert_eq!(enabled,std::collections::BTreeSet::from(["story","task"]));
            let before=progress(&dir);
            assert_eq!(before["setup_required"],false);
            assert_eq!(before["sidebar_visible"],true);
            assert_eq!(before["sidebar_expanded"],false);
            guide(Action::Sidebar,w,cx); settle(&view,w,cx);
            let quick=["connect_machine","create_agent","create_task","assign_agent"];
            assert_eq!(groups()[0].steps.iter().take(4).map(|s|s.id.as_str()).collect::<Vec<_>>(),quick);
            for id in quick {
                guide(Action::Start(id),w,cx);settle(&view,w,cx);
                let screen=form(cx);
                assert!(match id {
                    "connect_machine"=>matches!(screen,Some(Mode::Machine{stop:false})),
                    "create_agent"=>matches!(screen,Some(Mode::MissingMachine)),
                    "create_task"=>matches!(screen,Some(Mode::Create(k)) if k=="task"),
                    _=>matches!(screen,Some(Mode::GuideTarget{step:"assign_agent",kind:"task"})),
                }, "sidebar target for {id}");
                act(UiAction::CloseForm,w,cx);
            }
            guide(Action::Full,w,cx);settle(&view,w,cx);
            assert_eq!(groups().len(),6);
            assert_eq!(groups().iter().map(|g|g.steps.len()).sum::<usize>(),36);
            for (index,group) in groups().iter().enumerate() {
                if index>0 {guide(Action::Group(&group.id),w,cx);}
                settle(&view,w,cx);
                let visible=buttons(w);
                for item in &group.steps {
                    assert!(visible.contains(&item.title),"missing rendered guide step {}",item.id);
                    guide(Action::Step(&item.id),w,cx);
                    settle(&view,w,cx);
                    assert!(buttons(w).contains(&item.button),"expected action for expanded instructions {}",item.id);
                    guide(Action::Step(&item.id),w,cx);
                }
                guide(Action::Group(&group.id),w,cx);
            }
            assert_eq!(progress(&dir)["points"],before["points"]);
            guide(Action::Menu(Menu::Sidebar),w,cx);settle(&view,w,cx);
            assert!(buttons(w).iter().any(|s|s=="不再显示"));
            guide(Action::Display(false),w,cx);settle(&view,w,cx);
            assert!(!view.read(cx).guide.visible());
            assert_eq!(progress(&dir)["sidebar_visible"],false);
            guide(Action::Menu(Menu::Page),w,cx);settle(&view,w,cx);
            assert!(buttons(w).iter().any(|s|s=="显示侧边栏向导"));
            guide(Action::Display(true),w,cx);settle(&view,w,cx);
            assert!(view.read(cx).guide.visible());
            assert_eq!(progress(&dir)["sidebar_visible"],true);
            act(UiAction::Personal(super::personal::Action::Change("language",json!("en"))),w,cx);
            settle(&view,w,cx);
            for group in groups() {
                guide(Action::Group(&group.id),w,cx);settle(&view,w,cx);
                let rendered=buttons(w);
                for item in &group.steps {
                    assert!(rendered.iter().any(|s|s==crate::locale::label(&item.title)));
                    assert_ne!(crate::locale::label(&item.title),item.title);
                    assert_ne!(crate::locale::label(&item.hint),item.hint);
                    guide(Action::Step(&item.id),w,cx);settle(&view,w,cx);
                    assert!(buttons(w).iter().any(|s|s==crate::locale::label(&item.button)));
                    guide(Action::Step(&item.id),w,cx);
                }
                guide(Action::Group(&group.id),w,cx);
            }
            act(UiAction::Go(Page::Members),w,cx);settle(&view,w,cx);
            let member_buttons=buttons(w);
            for label in ["Invitations","Invite by email","Invitation link","Accept invitation"]{
                assert!(member_buttons.iter().any(|s|s==label),"missing translated control {label}");
            }
            act(UiAction::Form(Mode::Invite{email:true}),w,cx);settle(&view,w,cx);
            assert!(buttons(w).iter().any(|s|s=="Generate invitation link"));
            let role=view.read(cx).form.as_ref().unwrap().entries.iter().position(|e|e.spec.key=="role").unwrap();
            act(UiAction::Pick(role),w,cx);settle(&view,w,cx);
            assert!(buttons(w).iter().any(|s|s=="Member"));
            act(UiAction::CloseForm,w,cx);
            act(UiAction::Form(Mode::JoinInvite),w,cx);settle(&view,w,cx);
            assert!(buttons(w).iter().any(|s|s=="Accept invitation"));
        }).unwrap();
        cx.quit();
    });
}
