//! Translate interface labels at their presentation boundary. User-entered content stays verbatim.
use gpui_kit::SharedString;
use std::cell::Cell;
use std::collections::HashMap;
use std::sync::LazyLock;

thread_local! { static ENGLISH: Cell<bool> = const { Cell::new(false) }; }
static EN: LazyLock<HashMap<&'static str, &'static str>> = LazyLock::new(|| {
    include_str!("../assets/en.tsv")
        .lines()
        .filter_map(|l| l.split_once('\t'))
        .collect()
});
pub fn select(language: &str) {
    ENGLISH.set(language == "en");
}
pub fn label(s: &str) -> &str {
    if ENGLISH.get() {
        EN.get(s).copied().unwrap_or(s)
    } else {
        s
    }
}
pub fn new_title(noun: &str) -> String {
    if ENGLISH.get() {
        format!("New {}", label(noun))
    } else {
        format!("新建{noun}")
    }
}
pub fn edit_title(noun: &str) -> String {
    if ENGLISH.get() {
        format!("Edit {}", label(noun))
    } else {
        format!("编辑{noun}")
    }
}
pub fn rank(rank: u32) -> String {
    if ENGLISH.get() {
        format!("Rank {rank}")
    } else {
        format!("第 {rank} 名")
    }
}
pub fn select_title(noun: &str) -> String {
    if ENGLISH.get() {
        format!("Select {}", label(noun))
    } else {
        format!("选择{noun}")
    }
}
pub fn choice<'a>(key: &str, id: &str, title: &'a str) -> &'a str {
    if ENGLISH.get() && key == "role" && id == "member" {
        "Member"
    } else {
        label(title)
    }
}
pub fn disable_invitation(title: &str) -> String {
    if ENGLISH.get() {
        format!("Disable invitation link for {}", label(title))
    } else {
        format!("停用「{title}」的邀请链接")
    }
}
pub trait DisplayText {
    fn display_text(self) -> SharedString;
}
impl DisplayText for &str {
    fn display_text(self) -> SharedString {
        label(self).into()
    }
}
impl DisplayText for String {
    fn display_text(self) -> SharedString {
        self.into()
    }
}
impl DisplayText for SharedString {
    fn display_text(self) -> SharedString {
        self
    }
}
