// No console window behind the app on Windows.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod alias;
mod api;
mod app;
mod chatlog;
mod config;
mod focus;
mod fresh;
mod hotkeys;
mod jump;
mod naming;
#[cfg(not(target_os = "linux"))]
mod native;
mod overlay;
mod plan;
mod scan;
mod sigs;
mod snap;
mod theme;
mod tray;
mod ui;
mod update;

use std::time::Duration;

use gpui_kit::component::ActiveTheme as _;
use gpui_kit::{
    AnyWindowHandle, App, AppContext, Bounds, Entity, Global, TitlebarOptions, WindowBounds,
    WindowOptions, px, size,
};

use tray::{Tray, TrayEvent};

/// A line on stderr, the app's only log.
#[expect(clippy::print_stderr, reason = "stderr is the desktop app's only log")]
pub(crate) fn log(line: std::fmt::Arguments<'_>) {
    eprintln!("{line}");
}

/// What outlives the main window: the app itself, which keeps running in the tray with its
/// overlay and keys, and the window when one is open.
struct Shell {
    app: Option<Entity<app::DesktopApp>>,
    window: Option<AnyWindowHandle>,
    tray: Option<Tray>,
}

impl Global for Shell {}

fn main() {
    let application = gpui_kit::application().with_assets(theme::Assets);
    // macOS: clicking the dock icon with the window closed.
    application.on_reopen(open_main);
    application.run(|cx| {
        gpui_kit::init(cx);
        theme::init(cx);
        cx.set_global(Shell {
            app: None,
            window: None,
            tray: Tray::new(),
        });
        open_main(cx);
        // The overlay and the alias dialog are windows too, so the app runs on with the
        // main one closed. Without a tray there would be nothing left to quit it from.
        cx.on_window_closed(|cx, id| {
            let shell = cx.global_mut::<Shell>();
            if shell.window.is_some_and(|w| w.window_id() == id) {
                shell.window = None;
                if shell.tray.is_none() {
                    cx.quit();
                }
            }
        })
        .detach();
        cx.spawn(async move |cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(100))
                    .await;
                cx.update(|cx| {
                    let mode = cx.theme().mode;
                    if let Some(tray) = cx.global_mut::<Shell>().tray.as_mut() {
                        tray.set_mode(mode);
                    }
                    let events = cx
                        .global::<Shell>()
                        .tray
                        .as_ref()
                        .map(Tray::events)
                        .unwrap_or_default();
                    for event in events {
                        on_tray(event, cx);
                    }
                });
            }
        })
        .detach();
    });
}

fn on_tray(event: TrayEvent, cx: &mut App) {
    match event {
        TrayEvent::Open => open_main(cx),
        TrayEvent::ToggleOverlay => {
            if let Some(app) = cx.global::<Shell>().app.clone() {
                app.update(cx, app::DesktopApp::toggle_overlay);
            }
        }
        TrayEvent::Quit => cx.quit(),
    }
}

/// Bring the main window up, opening it again around the running app if it was closed.
fn open_main(cx: &mut App) {
    let shell = cx.global::<Shell>();
    if let Some(window) = shell.window
        && window
            .update(cx, |_, window, _| window.activate_window())
            .is_ok()
    {
        return;
    }
    let running = cx.global::<Shell>().app.clone();
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
            None,
            size(px(980.), px(700.)),
            cx,
        ))),
        titlebar: Some(TitlebarOptions {
            title: Some("WormholeSystems".into()),
            ..Default::default()
        }),
        ..Default::default()
    };
    let opened = gpui_kit::open_window(options, cx, |window, cx| {
        running.unwrap_or_else(|| cx.new(|cx| app::DesktopApp::new(window, cx)))
    });
    match opened {
        Ok((window, app)) => {
            #[cfg(windows)]
            {
                let mode = cx.theme().mode;
                let _ = window.update(cx, |_, window, _| native::set_window_icon(window, mode));
            }
            cx.activate(true);
            let shell = cx.global_mut::<Shell>();
            shell.window = Some(window);
            shell.app = Some(app);
        }
        Err(e) => log(format_args!("could not open the window: {e:#}")),
    }
}
