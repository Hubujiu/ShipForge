use super::*;
use crate::{
    app::{
        agent_directory,
        onboarding_ui::{activate_accessibility, buttons, settle},
    },
    catalog::Page,
    controls::Field,
    forms::Control,
};
use gpui_kit::{prelude::*, *};
use shipforge_client::Client;

const A: &str = "git@git.example.test:team/desktop.git";
const B: &str = "https://git.example.test/team/shared";
const C: &str = "https://git.example.test/team/agent";
const D: &str = "https://git.example.test/team/variant";

fn act(view: &Entity<Workbench>, action: UiAction, w: &mut Window, cx: &mut App) {
    view.update(cx, |v, cx| v.act(action, w, cx));
    settle(view, w, cx);
}
fn has(w: &Window, label: &str) -> bool {
    buttons(w).iter().any(|name| name == label)
}
fn current(view: &Entity<Workbench>, cx: &App) -> Dialog {
    match &view.read(cx).form.as_ref().expect("Git dialog").mode {
        Mode::GitWorkspace(dialog) => dialog.clone(),
        _ => panic!("unexpected dialog"),
    }
}
fn entry(view: &Entity<Workbench>, key: &str, cx: &App) -> usize {
    view.read(cx)
        .form
        .as_ref()
        .unwrap()
        .entries
        .iter()
        .position(|entry| entry.spec.key == key)
        .unwrap()
}
fn choose(view: &Entity<Workbench>, key: &str, value: &str, w: &mut Window, cx: &mut App) {
    let index = entry(view, key, cx);
    act(view, UiAction::Pick(index), w, cx);
    act(view, UiAction::Choose(index, value.into()), w, cx);
}
fn fill(view: &Entity<Workbench>, key: &str, value: &str, w: &mut Window, cx: &mut App) {
    let index = entry(view, key, cx);
    let control = view.read(cx).form.as_ref().unwrap().entries[index]
        .control
        .clone();
    let Control::Text(Field::Single(input)) = control else {
        panic!("expected text input")
    };
    input.update(cx, |input, cx| input.set_value(value.to_owned(), w, cx));
}
fn panel(view: &Entity<Workbench>, name: &str, next: Panel, w: &mut Window, cx: &mut App) {
    assert!(
        has(w, name),
        "missing native control {name}: {:?}",
        buttons(w)
    );
    let dialog = current(view, cx).showing(next);
    act(view, UiAction::Form(Mode::GitWorkspace(dialog)), w, cx);
}
fn save(view: &Entity<Workbench>, w: &mut Window, cx: &mut App) {
    act(view, UiAction::Submit, w, cx);
    let current = current(view, cx);
    assert!(
        matches!(current.panel, Panel::Overview),
        "write failed: {}",
        view.read(cx).form.as_ref().unwrap().error
    );
}
fn add(
    view: &Entity<Workbench>,
    url: &str,
    description: &str,
    branch: &str,
    w: &mut Window,
    cx: &mut App,
) {
    panel(view, "添加 Git 仓库", Panel::Repository(None), w, cx);
    fill(view, "url", url, w, cx);
    fill(view, "description", description, w, cx);
    fill(view, "branch", branch, w, cx);
    save(view, w, cx);
}
fn read(cli: &mut Client, target: &Target) -> Value {
    cli.execute(target.request("git.settings")).unwrap()
}
fn set_include(
    view: &Entity<Workbench>,
    mode: &str,
    selection: Option<&str>,
    w: &mut Window,
    cx: &mut App,
) {
    panel(view, "选择继承的空间仓库", Panel::Inclusion, w, cx);
    choose(view, "include_mode", mode, w, cx);
    if let Some(url) = selection {
        assert!(
            view.read(cx)
                .options("$git_space_repositories")
                .iter()
                .any(|(id, _)| id == url)
        );
        choose(view, "git_urls", url, w, cx);
    }
    save(view, w, cx);
}

#[::core::prelude::v1::test]
#[ignore = "Opens a hidden native Windows D3D window and its UI Automation tree"]
fn hidden_git_workspace_settings_and_repository_inheritance() {
    let fixture = tempfile::tempdir().unwrap();
    let path = fixture.path().to_owned();
    let mut cli = Client::local(&path).unwrap();
    for q in [
        json!({"op":"onboarding.setup","organization_name":"Workspace QA","organization_slug":"workspace-qa","work_type":"software","task_types":["bug"]}),
        json!({"op":"create","kind":"space","id":"labs","title":"Laboratory","fields":{"visibility":"private"}}),
        json!({"op":"create","kind":"agent","id":"runner","title":"Runner","fields":{"visibility":"space"}}),
        json!({"op":"create","kind":"member","id":"reader","title":"Reader","fields":{"role":"viewer"}}),
    ] {
        cli.execute(q).unwrap();
    }
    gpui_kit::application()
        .with_assets(crate::assets::Assets)
        .run(move |cx| {
            gpui_kit::init(cx);
            let (window, view) = gpui_kit::open_window(
                WindowOptions {
                    show: false,
                    focus: false,
                    window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                        None,
                        size(px(1520.), px(1280.)),
                        cx,
                    ))),
                    ..Default::default()
                },
                cx,
                |w, cx| cx.new(|cx| Workbench::new(Some(path.clone()), false, false, w, cx)),
            )
            .unwrap();
            window
                .update(cx, |_, w, cx| {
                    activate_accessibility(w);
                    settle(&view, w, cx);
                    organization_and_space(&view, &mut cli, w, cx);
                    agent_repositories_and_conflict(&view, &mut cli, w, cx);
                    variant_list(&view, &mut cli, w, cx);
                    read_only(&view, &path, w, cx);
                })
                .unwrap();
            cx.quit();
        });
}

fn organization_and_space(
    view: &Entity<Workbench>,
    cli: &mut Client,
    w: &mut Window,
    cx: &mut App,
) {
    let org = Target::organization("default");
    let space = Target::space("default");
    act(view, UiAction::Go(Page::Organizations), w, cx);
    assert!(has(w, "组织 Git 设置"));
    act(view, UiAction::GitWorkspace(org.clone()), w, cx);
    assert!(current(view, cx).data["settings"].is_null());
    assert!(has(w, "查看空间仓库 · Laboratory"));
    panel(view, "编辑提交身份", Panel::Author, w, cx);
    choose(view, "name_mode", "manual", w, cx);
    fill(view, "manual_name", "Release Bot", w, cx);
    save(view, w, cx);
    assert_eq!(read(cli, &org)["author"]["name"]["value"], "Release Bot");

    let labs = org.joined_space("labs");
    act(view, UiAction::GitWorkspace(labs), w, cx);
    assert_eq!(current(view, cx).data["id"], "labs");
    assert!(has(w, "返回组织 Git 设置"));
    act(view, UiAction::GitWorkspace(org.clone()), w, cx);
    act(view, UiAction::CloseForm, w, cx);
    act(view, UiAction::Go(Page::Spaces), w, cx);
    assert!(has(w, "空间 Git 设置"));
    act(view, UiAction::GitWorkspace(space.clone()), w, cx);
    assert_eq!(
        current(view, cx).data["author_sources"]["name"],
        "organization"
    );
    add(
        view,
        "git@git.example.test:team/desktop.git",
        "Desktop checkout",
        "main",
        w,
        cx,
    );
    add(view, B, "Shared repo", "", w, cx);
    assert!(read(cli, &space)["repositories"][1]["branch"].is_null());
    panel(
        view,
        &format!("编辑仓库 · {A}"),
        Panel::Repository(Some(A.into())),
        w,
        cx,
    );
    fill(view, "description", "Release checkout", w, cx);
    fill(view, "branch", "release/windows", w, cx);
    save(view, w, cx);
    panel(view, "编辑提交身份", Panel::Author, w, cx);
    choose(view, "email_mode", "manual", w, cx);
    fill(view, "manual_email", "release@example.test", w, cx);
    save(view, w, cx);
    assert_eq!(
        read(cli, &space)["repositories"][0]["description"],
        "Release checkout"
    );
    assert_eq!(read(cli, &org)["author_sources"]["email"], "default");
    act(view, UiAction::CloseForm, w, cx);
}

fn agent_repositories_and_conflict(
    view: &Entity<Workbench>,
    cli: &mut Client,
    w: &mut Window,
    cx: &mut App,
) {
    let agent = Target::agent("default", "runner");
    act(view, UiAction::Go(Page::Agents), w, cx);
    let runner = view.read(cx).find("agent", "runner").unwrap().clone();
    act(
        view,
        UiAction::Directory(agent_directory::Action::Open(runner)),
        w,
        cx,
    );
    assert!(has(w, "Agent Git 设置"));
    act(view, UiAction::GitWorkspace(agent.clone()), w, cx);
    let initial = current(view, cx).data;
    assert_eq!(
        initial["author_sources"],
        json!({"name":"organization","email":"space","coauthor":"default"})
    );
    assert_eq!(initial["repositories"].as_array().unwrap().len(), 2);
    add(view, C, "Agent only", "", w, cx);
    assert_eq!(
        current(view, cx).data["repositories"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    panel(
        view,
        &format!("编辑仓库 · {C}"),
        Panel::Repository(Some(C.into())),
        w,
        cx,
    );
    fill(view, "url", A, w, cx);
    save(view, w, cx);
    assert_eq!(
        current(view, cx).data["repositories"][0]["branch"],
        "release/windows"
    );
    panel(
        view,
        &format!("编辑仓库 · {A}"),
        Panel::Repository(Some(A.into())),
        w,
        cx,
    );
    fill(view, "url", C, w, cx);
    save(view, w, cx);
    set_include(view, "selected", Some(B), w, cx);
    assert_eq!(read(cli, &agent)["repositories"][1]["url"], B);
    set_include(view, "all", None, w, cx);
    assert_eq!(
        current(view, cx).data["repositories"]
            .as_array()
            .unwrap()
            .len(),
        3
    );
    set_include(view, "none", None, w, cx);
    assert_eq!(
        current(view, cx).data["repositories"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    panel(view, "编辑提交身份", Panel::Author, w, cx);
    choose(view, "coauthor_mode", "false", w, cx);
    save(view, w, cx);
    assert_eq!(
        current(view, cx).data["author_sources"]["coauthor"],
        "agent"
    );

    panel(view, "编辑提交身份", Panel::Author, w, cx);
    let second = read(cli, &agent);
    let mut external = agent.request("git.configure");
    external["settings"] = second["settings"].clone();
    external["settings"]["author"]["coauthor"] = json!(true);
    external["expected_rev"] = second["rev"].clone();
    cli.execute(external).unwrap();
    act(view, UiAction::Submit, w, cx);
    assert!(!view.read(cx).form.as_ref().unwrap().error.is_empty());
    assert!(has(w, "重新读取 Git 设置"));
    act(view, UiAction::GitWorkspace(agent.clone()), w, cx);
    assert_eq!(current(view, cx).data["author"]["coauthor"], true);
}

fn variant_list(view: &Entity<Workbench>, cli: &mut Client, w: &mut Window, cx: &mut App) {
    let agent = Target::agent("default", "runner");
    cli.execute(json!({"op":"agent.variant","id":"runner","target_space":"labs","variant_id":"branch","visibility":"space"})).unwrap();
    let variant = Target::agent("labs", "branch");
    act(view, UiAction::GitWorkspace(variant.clone()), w, cx);
    assert_eq!(
        current(view, cx).data["repositories_source"],
        "parent_agent"
    );
    assert_eq!(
        current(view, cx).data["author_sources"]["coauthor"],
        "parent_agent"
    );
    add(view, D, "Variant only", "topic", w, cx);
    assert_eq!(
        read(cli, &variant)["agent_repositories"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert_eq!(
        read(cli, &agent)["agent_repositories"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    panel(
        view,
        &format!("移除仓库绑定 · {C}"),
        Panel::Remove(C.into()),
        w,
        cx,
    );
    assert_eq!(
        read(cli, &variant)["agent_repositories"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    save(view, w, cx);
    assert_eq!(current(view, cx).data["agent_repositories"][0]["url"], D);
    panel(view, "恢复仓库清单继承", Panel::ResetRepositories, w, cx);
    save(view, w, cx);
    assert_eq!(current(view, cx).data["agent_repositories"][0]["url"], C);
    assert_eq!(
        current(view, cx).data["repositories_source"],
        "parent_agent"
    );
    panel(view, "重置本层 Git 设置", Panel::Reset, w, cx);
    save(view, w, cx);
    assert!(read(cli, &variant)["settings"].is_null());
}

fn read_only(view: &Entity<Workbench>, path: &std::path::Path, w: &mut Window, cx: &mut App) {
    let space = Target::space("default");
    let mut db = shipforge_core::Store::open(&path.join("workspace.sqlite3")).unwrap();
    let readonly = db.execute("reader", space.request("git.settings")).unwrap();
    assert_eq!(readonly["can_configure"], false);
    let mut denied = space.request("git.configure");
    denied["expected_rev"] = readonly["rev"].clone();
    denied["settings"] = Value::Null;
    assert!(db.execute("reader", denied).is_err());
    view.update(cx, |v, cx| {
        v.git_reply(Reply::Opened(space), readonly, w, cx)
    });
    settle(view, w, cx);
    assert!(has(w, "重新读取 Git 设置"));
    assert!(!has(w, "编辑提交身份") && !has(w, "添加 Git 仓库") && !has(w, "重置本层 Git 设置"));
}
