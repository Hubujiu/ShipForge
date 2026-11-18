use super::{After, UiAction, Workbench};
use crate::{
    appearance::{SWATCHES, paint, surface},
    catalog::Page,
    controls::{
        BG, DIM, INK, LINE, MUTED, SELECTED, VIOLET, col, headline, icon, outlined, row, text,
    },
    forms::Mode,
};
use gpui_kit::{base::Button, prelude::*, *};
use serde_json::{Value, json};
use shipforge_core::preferences::{FRAME_COLORS, Preferences, THEMES, normalize_shortcut};

const THEME_NAMES: [&str; 5] = ["跟随系统", "浅色", "深色", "魔法蓝", "经典深色"];
const ACCENT_NAMES: [&str; 13] = [
    "不设置外边框颜色",
    "内置外边框颜色 1",
    "内置外边框颜色 2",
    "内置外边框颜色 3",
    "内置外边框颜色 4",
    "内置外边框颜色 5",
    "内置外边框颜色 6",
    "内置外边框颜色 7",
    "内置外边框颜色 8",
    "内置外边框颜色 9",
    "内置外边框颜色 10",
    "内置外边框颜色 11",
    "内置外边框颜色 12",
];
const COMMANDS: [(&str, &str, &str); 3] = [
    ("command_menu", "命令菜单", "快速打开搜索、导航和快捷命令。"),
    (
        "move_task",
        "将任务移动到空间",
        "选择任务和接收该任务的空间。",
    ),
    (
        "find_in_view",
        "在当前视图查找",
        "在当前任务列表或视图中搜索。",
    ),
];
pub const LANGUAGES: &[(&str, &str)] = &[("en", "English"), ("zh-Hans", "简体中文")];
#[derive(Default)]
pub struct Personal {
    pub options: Preferences,
    pub rev: i64,
    recording: Option<&'static str>,
    language_picker: bool,
}
impl Personal {
    pub fn shortcut_label(&self, key: &str) -> String {
        self.options
            .shortcuts
            .get(key)
            .map(|s| {
                s.split('-')
                    .map(|p| match p {
                        "ctrl" => "Ctrl".into(),
                        "alt" => "Alt".into(),
                        "shift" => "Shift".into(),
                        other => other.to_ascii_uppercase(),
                    })
                    .collect::<Vec<String>>()
                    .join("+")
            })
            .unwrap_or_default()
    }
}
#[derive(Clone)]
pub enum Action {
    Page(&'static str),
    Change(&'static str, Value),
    Record(&'static str),
    Reset(&'static str),
    LanguagePicker,
    Move,
    Find,
    Chat(String),
    Command(String),
}
fn button_action(a: Action) -> UiAction {
    UiAction::Personal(a)
}
fn panel() -> Div {
    col()
        .w_full()
        .p(px(15.))
        .gap(px(9.))
        .bg(surface())
        .border_1()
        .border_color(paint(LINE))
        .rounded(px(9.))
}
fn intro(title: &str, description: &str) -> Div {
    col()
        .gap(px(2.))
        .child(headline(title, 16.))
        .child(text(description, 13., MUTED))
}
impl Workbench {
    pub(super) fn personal_action(&mut self, a: Action, w: &mut Window, cx: &mut Context<Self>) {
        match a {
            Action::Page(id) => {
                if self.loc.page != Page::Settings {
                    self.navigate(Page::Settings);
                }
                self.loc.section = id.into();
                self.loc.selected = None;
                self.personal.recording = None;
                self.personal.language_picker = false;
                self.send(json!({"op":"preferences.get"}), After::Personal, false);
            }
            Action::LanguagePicker => {
                self.personal.language_picker = !self.personal.language_picker
            }
            Action::Change(key, value) => {
                self.personal.language_picker = false;
                self.save_preference(json!({(key):value}));
            }
            Action::Record(id) => {
                self.personal.recording = Some(id);
                self.root_focus.focus(w, cx);
            }
            Action::Reset(id) => self.save_preference(json!({"shortcuts":{(id):null}})),
            Action::Move => self.open_form(Mode::MoveTask, w, cx),
            Action::Find => {
                if !self.loc.page.is_tasks() && self.loc.view.is_none() {
                    self.navigate(Page::Tasks);
                }
                self.open_form(Mode::FindInView, w, cx);
            }
            Action::Chat(id) => self.personal_conversation(&id),
            Action::Command(id) => {
                if self.find("task", &id).is_some() {
                    let command = json!({"op":"task.command","space":self.loc.space,"id":id});
                    let after = if self.personal.options.open_after_command {
                        After::RunConversation
                    } else {
                        After::Notice("已提交任务指令".into())
                    };
                    self.send(command, after, true);
                }
            }
        }
    }
    fn save_preference(&mut self, patch: Value) {
        if self.busy {
            self.notify_text("正在保存，请稍后重试");
            return;
        }
        let mut q = json!({"op":"preferences.update","preferences":patch});
        if self.personal.rev > 0 {
            q["expected_rev"] = json!(self.personal.rev);
        }
        self.send(q, After::Personal, true);
    }
    pub(super) fn personal_conversation(&mut self, id: &str) {
        self.send(
            json!({"op":"task.conversation","space":self.loc.space,"id":id}),
            After::TaskConversation,
            true,
        );
    }
    fn composer_submits(&self, shift: bool, w: &mut Window, cx: &mut App) -> bool {
        self.form.as_ref().is_some_and(|form|matches!(&form.mode,Mode::Message(_)|Mode::Comment(_)|Mode::Workflow{op:"run.reply",..})
            && send_key(&self.personal.options,shift)
            && form.entries.iter().any(|entry|matches!(&entry.control,crate::forms::Control::Text(field) if field.handle(cx).is_focused(w) && !field.composing(w,cx))))
    }
    pub fn personal_enter(
        &mut self,
        event: &gpui_kit::component::input::Enter,
        w: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !event.secondary && self.composer_submits(event.shift, w, cx) {
            w.prevent_default();
            cx.stop_propagation();
            self.act(UiAction::Submit, w, cx);
        } else {
            cx.propagate();
        }
    }
    pub(super) fn personal_key(
        &mut self,
        e: &KeyDownEvent,
        w: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let k = &e.keystroke;
        let key = k.key.as_str();
        let m = k.modifiers;
        if e.prefer_character_input {
            return false;
        }
        if !matches!(self.loc.section.as_str(), "shortcuts") || self.loc.page != Page::Settings {
            self.personal.recording = None;
        }
        let combo = [
            (m.control, "ctrl"),
            (m.alt, "alt"),
            (m.shift, "shift"),
            (m.platform, "win"),
        ]
        .into_iter()
        .filter_map(|(on, s)| on.then_some(s))
        .chain([key])
        .collect::<Vec<_>>()
        .join("-");
        if let Some(id) = self.personal.recording {
            w.prevent_default();
            cx.stop_propagation();
            if matches!(
                key,
                "shift" | "alt" | "control" | "ctrl" | "win" | "platform"
            ) || e.is_held
            {
                return true;
            }
            if key == "escape" {
                self.personal.recording = None;
            } else {
                match normalize_shortcut(&combo) {
                    Ok(binding)
                        if self
                            .personal
                            .options
                            .shortcuts
                            .iter()
                            .any(|(cmd, k)| cmd != id && k == &binding) =>
                    {
                        self.notify_text("这个快捷键已分配给其他命令")
                    }
                    Ok(binding) => {
                        self.personal.recording = None;
                        self.save_preference(json!({"shortcuts":{(id):binding}}));
                    }
                    Err(_) => self.notify_text("组合键无效或已被系统保留，可按 Esc 取消"),
                }
            }
            cx.notify();
            return true;
        }
        if self.form.is_some() {
            if key == "enter"
                && !m.alt
                && !m.control
                && !m.platform
                && self.composer_submits(m.shift, w, cx)
            {
                w.prevent_default();
                cx.stop_propagation();
                if !e.is_held {
                    self.act(UiAction::Submit, w, cx);
                }
                return true;
            }
            return false;
        }
        let action = self
            .personal
            .options
            .shortcuts
            .iter()
            .find_map(|(command, assigned)| {
                if assigned != &combo {
                    None
                } else {
                    match command.as_str() {
                        "command_menu" => Some(UiAction::Search),
                        "move_task" => Some(button_action(Action::Move)),
                        "find_in_view" => Some(button_action(Action::Find)),
                        _ => None,
                    }
                }
            });
        if let Some(action) = action {
            w.prevent_default();
            cx.stop_propagation();
            if !e.is_held {
                self.act(action, w, cx);
            }
            true
        } else {
            false
        }
    }
    fn option(
        &self,
        id: String,
        label: &str,
        action: Action,
        selected: bool,
        cx: &Context<Self>,
    ) -> Button {
        self.button(
            id,
            crate::locale::label(label).to_owned(),
            button_action(action),
            cx,
        )
        .when(selected, |b| {
            b.bg(paint(SELECTED)).font_weight(FontWeight::SEMIBOLD)
        })
    }
    pub fn personal_navigation(&self, cx: &Context<Self>) -> Div {
        let mut group = col()
            .gap(px(2.))
            .mb(px(12.))
            .child(text("你的设置", 11., DIM).pl(px(9.)));
        for (id, label, glyph) in [
            ("preferences", "偏好设置", "sliders-horizontal"),
            ("shortcuts", "快捷键", "settings-2"),
        ] {
            group = group.child(
                self.option(
                    format!("personal-nav-{id}"),
                    label,
                    Action::Page(id),
                    self.loc.page == Page::Settings && self.loc.section == id,
                    cx,
                )
                .w_full()
                .justify_start()
                .child(icon(glyph, 14.))
                .child(crate::locale::label(label)),
            );
        }
        group
    }
    pub fn personal_page(&self, cx: &Context<Self>) -> AnyElement {
        let keyboard = self.loc.section == "shortcuts";
        let mut content = col()
            .w_full()
            .max_w(px(752.))
            .px(px(16.))
            .py(px(44.))
            .gap(px(16.));
        if keyboard {
            content = content.child(intro("键盘快捷键", "控制消息和应用级快捷键的行为。"));
            let mut modes = panel().child(headline("发送消息", 14.)).child(text(
                "适用于对话、任务评论和回复。",
                12.,
                MUTED,
            ));
            for (id, label, detail) in [
                ("enter", "Enter 发送", "Shift+Enter 换行"),
                ("shift-enter", "Shift+Enter 发送", "Enter 换行"),
            ] {
                let selected = self.personal.options.send_with == id;
                modes = modes.child(
                    self.option(
                        format!("send-{id}"),
                        label,
                        Action::Change("send_with", json!(id)),
                        selected,
                        cx,
                    )
                    .h_auto()
                    .p(px(10.))
                    .justify_start()
                    .border_1()
                    .border_color(paint(if selected { INK } else { LINE }))
                    .child(
                        div()
                            .size(px(14.))
                            .rounded_full()
                            .border_2()
                            .border_color(paint(if selected { INK } else { LINE }))
                            .when(selected, |b| b.bg(paint(VIOLET))),
                    )
                    .child(
                        col()
                            .gap(px(3.))
                            .child(headline(label, 13.))
                            .child(text(detail, 12., MUTED)),
                    ),
                );
            }
            let mut commands = panel();
            for (id, label, desc) in COMMANDS {
                let rec = self.personal.recording == Some(id);
                let shortcut = self.personal.shortcut_label(id);
                commands = commands.child(
                    row()
                        .w_full()
                        .flex_wrap()
                        .gap(px(7.))
                        .py(px(7.))
                        .child(
                            col()
                                .flex_1()
                                .child(headline(label, 13.))
                                .child(text(desc, 12., MUTED)),
                        )
                        .child(
                            text(shortcut, 12., INK)
                                .px(px(7.))
                                .bg(paint(SELECTED))
                                .rounded(px(4.)),
                        )
                        .child(
                            outlined(self.option(
                                format!("record-{id}"),
                                label,
                                Action::Record(id),
                                rec,
                                cx,
                            ))
                            .child(crate::locale::label(if rec {
                                "按组合键 · Esc 取消"
                            } else {
                                "录制"
                            })),
                        )
                        .child(
                            self.option(
                                format!("reset-{id}"),
                                "重置",
                                Action::Reset(id),
                                false,
                                cx,
                            )
                            .child(crate::locale::label("重置")),
                        ),
                );
            }
            content = content
                .child(modes)
                .child(intro("应用命令", "自定义用于导航和任务流程的全局快捷键。").mt(px(16.)))
                .child(commands);
        } else {
            let mut themes = row().flex_wrap().gap(px(7.));
            let mut accents = row().flex_wrap().gap(px(8.));
            for (i, id) in THEMES.iter().enumerate() {
                let (label, selected) = (THEME_NAMES[i], self.personal.options.theme == *id);
                let paper = [0xffffff, 0xffffff, 0x211f25, 0x222540, 0x151515][i];
                let rail = [0xefedf6, 0xf0eef8, 0x263337, 0x2c3642, 0x313a3a][i];
                let preview = row()
                    .h(px(68.))
                    .w_full()
                    .rounded(px(7.))
                    .overflow_hidden()
                    .border_2()
                    .border_color(if selected { paint(VIOLET) } else { paint(LINE) })
                    .bg(gpui_kit::rgb(paper))
                    .child(div().h_full().w(px(11.)).bg(gpui_kit::rgb(rail)))
                    .child(
                        div()
                            .h_full()
                            .w(px(18.))
                            .border_r_1()
                            .border_color(gpui_kit::rgb(rail)),
                    )
                    .when(*id == "system", |b| {
                        b.child(
                            div()
                                .ml_auto()
                                .h_full()
                                .w(relative(0.5))
                                .bg(gpui_kit::rgb(0x211f25)),
                        )
                    });
                themes = themes.child(
                    self.option(
                        format!("theme-{id}"),
                        label,
                        Action::Change("theme", json!(id)),
                        false,
                        cx,
                    )
                    .flex_1()
                    .min_w(px(79.))
                    .h_auto()
                    .p(px(2.))
                    .flex_col()
                    .items_start()
                    .gap(px(4.))
                    .child(preview)
                    .child(crate::locale::label(label)),
                );
            }
            for (i, id) in FRAME_COLORS.iter().enumerate() {
                let selected = self.personal.options.frame_color == *id;
                accents = accents.child(
                    self.option(
                        format!("accent-{id}"),
                        ACCENT_NAMES[i],
                        Action::Change("frame_color", json!(id)),
                        false,
                        cx,
                    )
                    .size(px(29.))
                    .rounded_full()
                    .p_0()
                    .border_2()
                    .border_color(if selected { paint(VIOLET) } else { paint(LINE) })
                    .bg(gpui_kit::rgb(SWATCHES[i].0)),
                );
            }
            let lang = LANGUAGES
                .iter()
                .find(|(id, _)| *id == self.personal.options.language)
                .map_or("简体中文", |(_, text)| *text);
            let mut language = panel().child(
                row()
                    .w_full()
                    .child(col().flex_1().child(headline("语言", 13.)).child(text(
                        "选择此账号使用的应用语言。",
                        12.,
                        MUTED,
                    )))
                    .child(
                        outlined(self.option(
                            "language-picker".into(),
                            "语言",
                            Action::LanguagePicker,
                            false,
                            cx,
                        ))
                        .min_w(px(140.))
                        .justify_between()
                        .child(lang)
                        .child(icon("chevron-down", 12.)),
                    ),
            );
            if self.personal.language_picker {
                let mut choices = row().flex_wrap().gap(px(5.));
                for (id, label) in LANGUAGES {
                    choices = choices.child(
                        self.option(
                            format!("lang-{id}"),
                            label,
                            Action::Change("language", json!(id)),
                            self.personal.options.language == *id,
                            cx,
                        )
                        .child(*label),
                    );
                }
                language = language.child(choices);
            }
            let mut sessions = panel();
            for (id, label, description, on) in [
                (
                    "open_after_assignment",
                    "指派给 Agent 后自动打开会话",
                    "指派给 Agent 或小队时打开执行者的任务会话。",
                    self.personal.options.open_after_assignment,
                ),
                (
                    "open_after_command",
                    "使用快捷指令后自动打开会话",
                    "从任务发起执行指令后打开该任务的会话。",
                    self.personal.options.open_after_command,
                ),
            ] {
                sessions = sessions.child(
                    row()
                        .w_full()
                        .gap(px(10.))
                        .child(col().flex_1().child(headline(label, 13.)).child(text(
                            description,
                            12.,
                            MUTED,
                        )))
                        .child(
                            self.option(
                                format!("toggle-{id}"),
                                label,
                                Action::Change(id, json!(!on)),
                                on,
                                cx,
                            )
                            .child(crate::locale::label(if on { "开启" } else { "关闭" })),
                        ),
                );
            }
            content = content
                .child(intro("主题", "选择主题。跟随系统会使用操作系统的外观。"))
                .child(
                    panel()
                        .child(themes)
                        .child(text("外边框颜色", 12., MUTED))
                        .child(accents.pt(px(4.))),
                )
                .child(intro("常规", "设置此账号的语言。").mt(px(12.)))
                .child(language)
                .child(intro("密度", "紧凑度、行高和字号设置仍在开发中。").mt(px(12.)))
                .child(panel().child(text("即将推出", 13., MUTED)))
                .child(intro("任务会话", "控制哪些场景会自动打开任务会话。").mt(px(12.)))
                .child(sessions);
        }
        col()
            .id("personal-settings-scroll")
            .size_full()
            .overflow_y_scroll()
            .items_center()
            .bg(paint(BG))
            .child(content)
            .into_any_element()
    }
}
fn send_key(p: &Preferences, shift: bool) -> bool {
    shift == (p.send_with == "shift-enter")
}

#[cfg(all(test, windows))]
mod native_test {
    use super::*;
    use crate::controls::Field;
    use gpui_kit::component::ActiveTheme;
    use std::time::{Duration, Instant};

    fn settle(view: &Entity<Workbench>, window: &mut Window, cx: &mut App) {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            view.update(cx, |app, cx| app.tick(window, cx));
            let ready = {
                let app = view.read(cx);
                app.personal.rev > 0 && !app.busy && !app.loading && app.pending.is_empty()
            };
            if ready {
                break;
            }
            assert!(Instant::now() < deadline, "desktop backend did not settle");
            std::thread::sleep(Duration::from_millis(5));
        }
        window.draw(cx).clear(cx);
    }
    #[::core::prelude::v1::test]
    #[ignore = "Runs a hidden native D3D window on a Windows workstation"]
    fn hidden_preferences_window() {
        gpui_kit::application()
            .with_assets(crate::assets::Assets)
            .run(|cx| {
                gpui_kit::init(cx);
                let (handle, view) = gpui_kit::open_window(
                    WindowOptions {
                        show: false,
                        focus: false,
                        window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                            None,
                            size(px(1440.), px(900.)),
                            cx,
                        ))),
                        ..Default::default()
                    },
                    cx,
                    |window, cx| cx.new(|cx| Workbench::new(None, true, false, window, cx)),
                )
                .unwrap();
                handle
                    .update(cx, |_, w, cx| {
                        crate::app::onboarding_ui::activate_accessibility(w);
                        settle(&view, w, cx);
                        let act = |action, w: &mut Window, cx: &mut App| {
                            view.update(cx, |app, cx| app.act(UiAction::Personal(action), w, cx))
                        };
                        let key = |stroke, w: &mut Window, cx: &mut App| {
                            w.dispatch_keystroke(Keystroke::parse(stroke).unwrap(), cx);
                        };
                        act(Action::Page("preferences"), w, cx);
                        settle(&view, w, cx);
                        for theme in ["dark", "magic-blue", "classic-dark", "light", "system"] {
                            act(Action::Change("theme", json!(theme)), w, cx);
                            settle(&view, w, cx);
                            assert_eq!(view.read(cx).personal.options.theme, theme);
                            if theme != "system" {
                                assert_eq!(cx.theme().is_dark(), theme != "light");
                            }
                        }
                        act(Action::Change("language", json!("en")), w, cx);
                        settle(&view, w, cx);
                        assert_eq!(Page::Tasks.info().label, "Tasks");
                        assert_eq!(crate::locale::label("外边框颜色"), "Window frame color");
                        let labels = crate::app::onboarding_ui::buttons(w);
                        for original in THEME_NAMES.into_iter().chain(ACCENT_NAMES).chain([
                            "语言",
                            "指派给 Agent 后自动打开会话",
                            "使用快捷指令后自动打开会话",
                        ]) {
                            let english = crate::locale::label(original);
                            assert_ne!(english, original);
                            assert!(
                                labels.iter().any(|s| s == english),
                                "missing English preference button {english}"
                            );
                        }
                        for (field, value) in [
                            ("open_after_assignment", false),
                            ("open_after_command", true),
                        ] {
                            act(Action::Change(field, json!(value)), w, cx);
                            settle(&view, w, cx);
                            let saved = &view.read(cx).personal.options;
                            assert_eq!(
                                if field == "open_after_assignment" {
                                    saved.open_after_assignment
                                } else {
                                    saved.open_after_command
                                },
                                value
                            );
                        }
                        act(Action::Page("shortcuts"), w, cx);
                        settle(&view, w, cx);
                        let labels = crate::app::onboarding_ui::buttons(w);
                        for original in [
                            "Enter 发送",
                            "Shift+Enter 发送",
                            "命令菜单",
                            "将任务移动到空间",
                            "在当前视图查找",
                            "重置",
                        ] {
                            assert!(
                                labels.iter().any(|s| s == crate::locale::label(original)),
                                "missing English shortcut control {original}"
                            );
                        }
                        act(Action::Record("command_menu"), w, cx);
                        key("ctrl-f", w, cx);
                        assert_eq!(view.read(cx).personal.recording, Some("command_menu"));
                        key("alt-shift-j", w, cx);
                        settle(&view, w, cx);
                        assert_eq!(
                            view.read(cx).personal.options.shortcuts["command_menu"],
                            "alt-shift-j"
                        );
                        key("alt-shift-j", w, cx);
                        assert!(matches!(
                            view.read(cx).form.as_ref().map(|f| &f.mode),
                            Some(Mode::Search)
                        ));
                        view.update(cx, |a, cx| a.act(UiAction::CloseForm, w, cx));
                        act(Action::Reset("command_menu"), w, cx);
                        settle(&view, w, cx);
                        key("ctrl-shift-m", w, cx);
                        assert!(matches!(
                            view.read(cx).form.as_ref().map(|f| &f.mode),
                            Some(Mode::MoveTask)
                        ));
                        view.update(cx, |a, cx| a.act(UiAction::CloseForm, w, cx));
                        key("ctrl-f", w, cx);
                        assert!(matches!(
                            view.read(cx).form.as_ref().map(|f| &f.mode),
                            Some(Mode::FindInView)
                        ));
                        view.update(cx, |a, cx| a.act(UiAction::CloseForm, w, cx));
                        for (mode, newline, send) in [
                            ("enter", "shift-enter", "enter"),
                            ("shift-enter", "enter", "shift-enter"),
                        ] {
                            act(Action::Change("send_with", json!(mode)), w, cx);
                            settle(&view, w, cx);
                            view.update(cx, |a, cx| {
                                let task = a.list("task")[0].clone();
                                a.open_form(Mode::Comment(task), w, cx);
                            });
                            settle(&view, w, cx);
                            let editor = match &view.read(cx).form.as_ref().unwrap().entries[0]
                                .control
                            {
                                crate::forms::Control::Text(Field::Multi(state)) => state.clone(),
                                _ => panic!("multiline comment editor"),
                            };
                            editor.update(cx, |s, cx| {
                                s.replace_and_mark_text_in_range(None, "拼", Some(0..1), w, cx)
                            });
                            key(send, w, cx);
                            assert!(
                                view.read(cx).form.is_some(),
                                "IME composition must not send"
                            );
                            editor.update(cx, |s, cx| {
                                s.unmark_text(w, cx);
                                s.set_value("test", w, cx);
                            });
                            key(newline, w, cx);
                            assert!(
                                editor.read(cx).value().contains('\n'),
                                "expected a newline for {newline}"
                            );
                            key(send, w, cx);
                            settle(&view, w, cx);
                            assert!(view.read(cx).form.is_none(), "expected submit for {send}");
                        }
                    })
                    .unwrap();
                cx.quit();
            });
    }
}
