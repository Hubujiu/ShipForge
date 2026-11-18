use crate::{
    app::{UiAction, Workbench},
    motion::Timing,
};
use gpui_kit::{
    base::Button,
    component::{
        ActiveTheme, Icon,
        input::{Input, InputEvent, InputState, Textarea, TextareaState},
    },
    prelude::*,
    *,
};

pub const INK: u32 = 0x23242a;
use crate::appearance::{paint as rgb, surface as white};
pub const MUTED: u32 = 0x686b77;
pub const DIM: u32 = 0x858895;
pub const LINE: u32 = 0xe5e6ec;
pub const BG: u32 = 0xfcfcfd;
pub const PALE: u32 = 0xf5f5f8;
pub const SELECTED: u32 = 0xededf6;
pub const GREEN: u32 = 0x3a8a78;
pub const VIOLET: u32 = 0x7064a4;
pub const ERROR: u32 = 0xaf3c48;
pub fn row() -> Div {
    div().flex().items_center()
}
pub fn col() -> Div {
    div().flex().flex_col()
}
pub fn text(s: impl crate::locale::DisplayText, size: f32, color: u32) -> Div {
    div()
        .text_size(px(size))
        .line_height(relative(1.5))
        .text_color(rgb(color))
        .child(s.display_text())
}
pub fn icon(name: &str, size: f32) -> Icon {
    Icon::default()
        .path(format!("icons/{name}.svg"))
        .size(px(size))
        .flex_shrink_0()
}
pub fn headline(s: impl crate::locale::DisplayText, size: f32) -> Div {
    text(s, size, INK).font_weight(FontWeight::SEMIBOLD)
}
pub fn chip(s: impl crate::locale::DisplayText, background: u32, color: u32) -> Div {
    row()
        .px(px(7.))
        .py(px(1.))
        .rounded(px(5.))
        .bg(rgb(background))
        .child(text(s, 11., color))
}
pub fn mark(color: u32) -> Div {
    div()
        .size(px(8.))
        .rounded_full()
        .flex_shrink_0()
        .bg(rgb(color))
}
pub fn shadow(color: Hsla, alpha: f32, spread: f32) -> BoxShadow {
    BoxShadow {
        inset: false,
        color: color.opacity(alpha),
        offset: point(px(0.), px(0.)),
        blur_radius: px(0.),
        spread_radius: px(spread),
    }
}
fn blend(a: u32, b: u32, t: f32) -> Hsla {
    let channel = |shift: u32| {
        let from = ((a >> shift) & 255) as f32;
        let to = ((b >> shift) & 255) as f32;
        (from + (to - from) * t).round() as u32
    };
    rgb(channel(16) << 16 | channel(8) << 8 | channel(0)).into()
}
#[derive(Clone)]
pub enum Field {
    Single(Entity<InputState>),
    Multi(Entity<TextareaState>),
}
impl Field {
    pub fn new(
        value: &str,
        placeholder: &str,
        multi: bool,
        secret: bool,
        window: &mut Window,
        cx: &mut Context<Workbench>,
    ) -> Self {
        if multi {
            let state = cx.new(|cx| {
                TextareaState::new(window, cx)
                    .rows(4)
                    .placeholder(placeholder.to_owned())
                    .default_value(value.to_owned())
            });
            cx.subscribe_in(&state, window, |_, _, _: &InputEvent, _, cx| cx.notify())
                .detach();
            Self::Multi(state)
        } else {
            let state = cx.new(|cx| {
                InputState::new(window, cx)
                    .masked(secret)
                    .placeholder(placeholder.to_owned())
                    .default_value(value.to_owned())
            });
            cx.subscribe_in(&state, window, |_, _, _: &InputEvent, _, cx| cx.notify())
                .detach();
            Self::Single(state)
        }
    }
    pub fn value(&self, cx: &App) -> String {
        match self {
            Self::Single(s) => s.read(cx).value().to_string(),
            Self::Multi(s) => s.read(cx).value().to_string(),
        }
    }
    pub fn handle(&self, cx: &App) -> FocusHandle {
        match self {
            Self::Single(s) => s.read(cx).focus_handle(cx),
            Self::Multi(s) => s.read(cx).focus_handle(cx),
        }
    }
    pub fn composing(&self, w: &mut Window, cx: &mut App) -> bool {
        match self {
            Self::Single(state) => state.update(cx, |s, cx| s.marked_text_range(w, cx).is_some()),
            Self::Multi(state) => state.update(cx, |s, cx| s.marked_text_range(w, cx).is_some()),
        }
    }
}
impl Workbench {
    pub fn button(
        &self,
        id: impl Into<ElementId>,
        label: impl crate::locale::DisplayText,
        action: UiAction,
        cx: &Context<Self>,
    ) -> Button {
        let accent = cx.theme().accent;
        Button::new(id)
            .accessibility_label(label.display_text())
            .h(px(30.))
            .px(px(8.))
            .gap(px(6.))
            .flex()
            .items_center()
            .justify_center()
            .text_size(px(13.))
            .line_height(relative(1.5))
            .text_color(rgb(INK))
            .rounded(px(6.))
            .cursor_pointer()
            .hover(|s| s.bg(rgb(0xe7e8ee)))
            .focus_visible(move |s| s.shadow(vec![shadow(accent, 0.4, 2.)]))
            .on_click(cx.listener(move |this, _, window, cx| {
                cx.stop_propagation();
                this.act(action.clone(), window, cx)
            }))
    }
    pub fn tool_button(
        &self,
        id: &str,
        label: &str,
        glyph: &str,
        action: UiAction,
        cx: &Context<Self>,
    ) -> Button {
        self.button(id.to_owned(), label.to_owned(), action, cx)
            .w(px(30.))
            .px_0()
            .child(icon(glyph, 15.))
            .tooltip({
                let label = label.to_owned();
                move |w, c| gpui_kit::component::tooltip::Tooltip::new(label.clone()).build(w, c)
            })
    }
    pub fn field_frame(
        &mut self,
        id: &str,
        handle: &FocusHandle,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let t = self.motion.value(
            format!("focus-{id}"),
            if handle.is_focused(window) { 1. } else { 0. },
            Timing::FOCUS,
            window,
            cx,
        );
        row()
            .w_full()
            .min_w_0()
            .px(px(8.))
            .rounded(px(7.))
            .border_1()
            .border_color(blend(LINE, INK, 0.4 * t))
            .shadow(vec![shadow(cx.theme().accent, 0.4 * t, 2.)])
            .bg(white())
    }
    pub fn field(
        &mut self,
        id: &str,
        label: &str,
        field: &Field,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let frame = self.field_frame(id, &field.handle(cx), window, cx);
        match field {
            Field::Single(s) => frame
                .h(px(35.))
                .child(
                    Input::new(s)
                        .id(id.to_owned())
                        .appearance(false)
                        .bordered(false)
                        .focus_bordered(false)
                        .aria_label(label.to_owned())
                        .accessibility_id(id.to_owned())
                        .text_size(px(13.))
                        .h(px(32.)),
                )
                .into_any_element(),
            Field::Multi(s) => frame
                .h(px(100.))
                .child(
                    Textarea::new(s)
                        .appearance(false)
                        .bordered(false)
                        .aria_label(label.to_owned())
                        .accessibility_id(id.to_owned())
                        .text_size(px(13.))
                        .h(px(96.)),
                )
                .into_any_element(),
        }
    }
}
pub fn primary(button: Button) -> Button {
    button
        .bg(rgb(INK))
        .text_color(white())
        .px(px(11.))
        .font_weight(FontWeight::MEDIUM)
}
pub fn outlined(button: Button) -> Button {
    button.border_1().border_color(rgb(LINE)).bg(white())
}
