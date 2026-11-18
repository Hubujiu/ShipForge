#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod appearance;
mod assignees;
use app::personal;
mod assets;
mod backend;
mod catalog;
mod controls;
mod files;
mod forms;
mod locale;
mod machine;
mod motion;
mod shell;
mod views;

use app::Workbench;
use gpui_kit::{
    component::{Theme, ThemeMode, TitleBar},
    *,
};
use std::path::PathBuf;

fn print_information(message: &str) {
    #[cfg(windows)]
    {
        use std::ffi::c_void;
        #[link(name = "kernel32")]
        unsafe extern "system" {
            fn GetStdHandle(kind: u32) -> *mut c_void;
            fn GetFileType(handle: *mut c_void) -> u32;
            fn AttachConsole(process_id: u32) -> i32;
        }
        // A GUI executable inherits redirected stdout, but a terminal may need to be attached.
        unsafe {
            let stdout = GetStdHandle(-11_i32 as u32);
            if stdout.is_null() || stdout as isize == -1 || GetFileType(stdout) == 0 {
                AttachConsole(u32::MAX);
            }
        }
    }
    println!("{message}");
}

fn main() {
    let mut demo = false;
    let mut smoke = false;
    let mut dir: Option<PathBuf> = None;
    let mut invitation = None;
    let mut args = std::env::args_os().skip(1);
    while let Some(arg) = args.next() {
        if arg == "--version" || arg == "-V" {
            print_information(concat!("shipforge-desktop ", env!("CARGO_PKG_VERSION")));
            return;
        }
        if arg == "--help" || arg == "-h" {
            print_information(
                "ShipForge Windows 工作台\nshipforge-desktop [--data-dir <目录>] [--demo] [--smoke-test] [--open-invitation <邀请链接>]\n--demo 使用内存示例数据。",
            );
            return;
        }
        if arg == "--smoke" || arg == "--smoke-test" {
            smoke = true;
        } else if arg == "--demo" {
            demo = true;
        } else if arg == "--open-invitation" {
            invitation = args
                .next()
                .and_then(|arg| arg.into_string().ok())
                .filter(|s| {
                    s.strip_prefix("shipforge://invite/").is_some_and(|body| {
                        body.len() == 80
                            && body.bytes().all(|b| {
                                b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_')
                            })
                    })
                });
            if invitation.is_none() || args.next().is_some() {
                print_information("邀请链接无效");
                std::process::exit(2);
            }
        } else if arg == "--data-dir" {
            let Some(value) = args
                .next()
                .filter(|arg| !arg.to_string_lossy().starts_with("--"))
            else {
                print_information("--data-dir 需要目录路径");
                std::process::exit(2);
            };
            dir = Some(value.into());
        } else {
            print_information(&format!("未知参数 {}", arg.to_string_lossy()));
            std::process::exit(2);
        }
    }
    application().with_assets(assets::Assets).run(move |cx| {
        init(cx);
        cx.set_quit_mode(QuitMode::LastWindowClosed);
        Theme::change(ThemeMode::Light, None, cx);
        let theme = Theme::global_mut(cx);
        theme.font_family = if cfg!(target_os = "windows") {
            "Microsoft YaHei UI"
        } else {
            ".SystemUIFont"
        }
        .into();
        theme.font_size = px(14.);
        theme.colors.accent = rgb(0x6e68a0).into();
        theme.colors.primary = rgb(0x23252a).into();
        theme.colors.primary_foreground = white();
        theme.colors.foreground = rgb(0x26282e).into();
        theme.colors.background = rgb(0xfcfcfd).into();
        theme.colors.border = rgb(0xe6e6ec).into();
        let bounds = Bounds::centered(None, size(px(1440.), px(900.)), cx);
        open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                window_min_size: Some(size(px(660.), px(460.))),
                titlebar: Some(TitlebarOptions {
                    title: Some(
                        if demo {
                            "ShipForge · 演示"
                        } else {
                            "ShipForge"
                        }
                        .into(),
                    ),
                    ..TitleBar::title_bar_options()
                }),
                window_decorations: Some(WindowDecorations::Client),
                ..TitleBar::window_options()
            },
            cx,
            move |window, cx| {
                cx.new(|cx| {
                    let mut workbench = Workbench::new(dir.clone(), demo, smoke, window, cx);
                    if let Some(link) = invitation.clone() {
                        workbench.open_invitation(link, window, cx);
                    }
                    workbench
                })
            },
        )
        .expect("打开 ShipForge 窗口");
        cx.activate(true);
    });
}
