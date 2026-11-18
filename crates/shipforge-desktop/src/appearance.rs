use crate::controls::{BG, DIM, ERROR, GREEN, INK, LINE, MUTED, PALE, SELECTED, VIOLET};
use gpui_kit::{
    component::{Theme, ThemeMode},
    *,
};
use shipforge_core::preferences::{FRAME_COLORS, Preferences};
use std::cell::Cell;

// Frame colors apply only to the title bar and the exposed outer window surface.
pub const SWATCHES: [(u32, u32); 13] = [
    (0xf2f3f5, 0x596575),
    (0xf1f2f7, 0x6e68a0),
    (0xfbecf3, 0xab477b),
    (0xfaeeee, 0xb25057),
    (0xfaefe9, 0xb26337),
    (0xf7f1e3, 0x8e722c),
    (0xeef3ec, 0x5c8053),
    (0xe6f6f5, 0x367d79),
    (0xeaf3fa, 0x4779a8),
    (0xf2f0fe, 0x8066b7),
    (0xf2f2f2, 0x676767),
    (0xfaeef0, 0xa15265),
    (0xf7efec, 0x946851),
];
#[derive(Clone, Copy, PartialEq, Eq)]
struct Palette {
    tone: u8,
    accent: usize,
}
thread_local! { static CURRENT: Cell<Palette> = const {Cell::new(Palette {tone:0,accent:1})}; }

fn mix(a: u32, b: u32, n: u32) -> u32 {
    let f =
        |shift: u32| ((((a >> shift) & 255) * (100 - n) + ((b >> shift) & 255) * n) / 100) << shift;
    f(16) | f(8) | f(0)
}
impl Palette {
    fn new(p: &Preferences, system_dark: bool) -> Self {
        let tone = match p.theme.as_str() {
            "dark" => 1,
            "magic-blue" => 2,
            "classic-dark" => 3,
            "system" if system_dark => 1,
            _ => 0,
        };
        Self {
            tone,
            accent: FRAME_COLORS
                .iter()
                .position(|id| *id == p.frame_color)
                .unwrap_or(1),
        }
    }
    fn color(self, c: u32) -> u32 {
        let (tint, frame) = SWATCHES[self.accent];
        if self.tone == 0 {
            return match c {
                VIOLET => 0x6e68a0,
                0xf2f2f8 | 0xf0f0f6 => {
                    if self.accent == 0 {
                        0xf2f3f5
                    } else {
                        tint
                    }
                }
                _ => c,
            };
        }
        let base = [0, 0x1c1d21, 0x20233d, 0x121212][self.tone as usize];
        match c {
            0xffffff | 0xfafafe | 0xfafafd => mix(base, 0xffffff, 2),
            BG | 0xf8f8fb => base,
            PALE | 0xf0f1f5 => mix(base, 0xffffff, 5),
            0xf2f2f8 | 0xf0f0f6 => {
                if self.accent == 0 {
                    base
                } else {
                    mix(base, frame, 12)
                }
            }
            LINE => mix(base, 0xffffff, 15),
            SELECTED | 0xe8e8f1 => mix(base, 0x6e68a0, 38),
            0xe7e8ee => mix(base, 0xffffff, 11),
            INK => 0xf1f0f6,
            MUTED => 0xbfc1ce,
            DIM => 0xa8aab7,
            VIOLET => mix(0x6e68a0, 0xffffff, 42),
            GREEN => 0x79c1ab,
            ERROR => 0xffa3a7,
            0xffeeef => 0x41272e,
            _ => mix(c, 0xffffff, 30),
        }
    }
}
pub fn paint(c: u32) -> Rgba {
    gpui_kit::rgb(CURRENT.get().color(c))
}
pub fn surface() -> Hsla {
    paint(0xffffff).into()
}
pub fn apply(p: &Preferences, w: &mut Window, cx: &mut App, force: bool) {
    let palette = Palette::new(
        p,
        matches!(
            w.appearance(),
            WindowAppearance::Dark | WindowAppearance::VibrantDark
        ),
    );
    if CURRENT.replace(palette) == palette && !force {
        return;
    }
    Theme::change(
        if palette.tone == 0 {
            ThemeMode::Light
        } else {
            ThemeMode::Dark
        },
        Some(w),
        cx,
    );
    let t = Theme::global_mut(cx);
    t.font_family = if cfg!(windows) {
        "Microsoft YaHei UI"
    } else {
        ".SystemUIFont"
    }
    .into();
    t.font_size = px(14.);
    t.colors.accent = paint(VIOLET).into();
    t.colors.primary = paint(INK).into();
    t.colors.primary_foreground = surface();
    t.colors.foreground = paint(INK).into();
    t.colors.background = paint(BG).into();
    t.colors.border = paint(LINE).into();
    w.refresh();
}

#[cfg(test)]
mod tests {
    use super::*;
    #[::core::prelude::v1::test]
    fn window_frames_leave_controls_unchanged_and_system_uses_current_appearance() {
        let mut p = Preferences::default();
        let base = Palette::new(&p, false);
        for frame in FRAME_COLORS {
            p.frame_color = frame.to_string();
            let next = Palette::new(&p, false);
            for token in [INK, LINE, SELECTED, VIOLET, 0xffffff] {
                assert_eq!(base.color(token), next.color(token));
            }
            assert_eq!(next.color(0xf0f0f6), SWATCHES[next.accent].0);
        }
        p.theme = "system".into();
        assert_eq!(Palette::new(&p, false).tone, 0);
        assert_eq!(Palette::new(&p, true).tone, 1);
        let mut colors = std::collections::BTreeSet::new();
        for theme in ["light", "dark", "magic-blue", "classic-dark"] {
            p.theme = theme.into();
            colors.insert(Palette::new(&p, false).color(BG));
        }
        assert_eq!(colors.len(), 4);
    }
}
