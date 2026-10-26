//! The tray icon. With it the app keeps running when its window is closed, overlay and keys
//! included, and quits from the tray's menu. Linux speaks StatusNotifierItem over D-Bus
//! (Waybar, KDE, GNOME with the AppIndicator extension); with no tray listening there is
//! none, and closing the window quits.

use gpui_kit::component::ThemeMode;
use tray_icon::menu::{Menu, MenuEvent, MenuId, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, MouseButton, TrayIcon, TrayIconBuilder, TrayIconEvent};

/// What the tray asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TrayEvent {
    Open,
    ToggleOverlay,
    Quit,
}

pub(crate) struct Tray {
    icon: TrayIcon,
    /// The theme the logo is drawn for.
    mode: ThemeMode,
    open: MenuId,
    overlay: MenuId,
    quit: MenuId,
}

impl Tray {
    /// Put the icon up, or `None` where there is no tray to put it in.
    pub(crate) fn new() -> Option<Tray> {
        if !host_available() {
            return None;
        }
        let open = MenuItem::new("Open WormholeSystems", true, None);
        let overlay = MenuItem::new("Show or hide the overlay", true, None);
        let quit = MenuItem::new("Quit", true, None);
        let menu = Menu::new();
        menu.append_items(&[&open, &overlay, &PredefinedMenuItem::separator(), &quit])
            .ok()?;
        let builder = TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            .with_tooltip("WormholeSystems");
        // The menu bar tints a template icon to fit, light or dark.
        #[cfg(target_os = "macos")]
        let builder = builder.with_icon_templated(icon(ThemeMode::Light)?);
        #[cfg(not(target_os = "macos"))]
        let builder = builder.with_icon(icon(ThemeMode::Light)?);
        let icon = builder
            // A click opens the window; the menu is on the right button. The menu bar
            // has no right button to speak of, so there a click is the menu.
            .with_menu_on_left_click(cfg!(target_os = "macos"))
            .build()
            .ok()?;
        Some(Tray {
            icon,
            mode: ThemeMode::Light,
            open: open.id().clone(),
            overlay: overlay.id().clone(),
            quit: quit.id().clone(),
        })
    }

    /// Draw the logo white for a dark theme, black for a light one. The menu bar tints its
    /// template itself.
    pub(crate) fn set_mode(&mut self, mode: ThemeMode) {
        if mode == self.mode || cfg!(target_os = "macos") {
            return;
        }
        self.mode = mode;
        if let Some(icon) = icon(mode) {
            let _ = self.icon.set_icon(Some(icon));
        }
    }

    /// What happened since the last call.
    pub(crate) fn events(&self) -> Vec<TrayEvent> {
        let mut out = Vec::new();
        while let Ok(event) = MenuEvent::receiver().try_recv() {
            if event.id == self.open {
                out.push(TrayEvent::Open);
            } else if event.id == self.overlay {
                out.push(TrayEvent::ToggleOverlay);
            } else if event.id == self.quit {
                out.push(TrayEvent::Quit);
            }
        }
        while let Ok(event) = TrayIconEvent::receiver().try_recv() {
            let open = match event {
                TrayIconEvent::Click { button, .. } => {
                    button == MouseButton::Left && !cfg!(target_os = "macos")
                }
                TrayIconEvent::DoubleClick { .. } => true,
                _ => false,
            };
            if open {
                out.push(TrayEvent::Open);
            }
        }
        out
    }
}

/// The logo alone, white or black.
fn icon(mode: ThemeMode) -> Option<Icon> {
    use resvg::tiny_skia::{Pixmap, Transform};
    use resvg::usvg::{Options, Tree};

    const SIZE: u16 = 64;
    let fill = match mode {
        ThemeMode::Dark => "white",
        ThemeMode::Light => "black",
    };
    let svg = std::str::from_utf8(include_bytes!("../assets/logo.svg"))
        .ok()?
        .replace("fill=\"black\"", &format!("fill=\"{fill}\""));
    let tree = Tree::from_str(&svg, &Options::default()).ok()?;
    let (w, h) = (tree.size().width(), tree.size().height());
    let side = f32::from(SIZE);
    let scale = side / w.max(h);
    let mut pixmap = Pixmap::new(u32::from(SIZE), u32::from(SIZE))?;
    let transform = Transform::from_row(
        scale,
        0.0,
        0.0,
        scale,
        (side - w * scale) / 2.0,
        (side - h * scale) / 2.0,
    );
    resvg::render(&tree, transform, &mut pixmap.as_mut());
    let rgba = pixmap
        .pixels()
        .iter()
        .flat_map(|p| {
            let c = p.demultiply();
            [c.red(), c.green(), c.blue(), c.alpha()]
        })
        .collect();
    Icon::from_rgba(rgba, u32::from(SIZE), u32::from(SIZE)).ok()
}

/// Whether a StatusNotifier host is there to show the icon. The tray library assumes one
/// is, and without it the icon would be nowhere and the app unreachable once closed.
#[cfg(target_os = "linux")]
fn host_available() -> bool {
    let Ok(bus) = zbus::blocking::Connection::session() else {
        return false;
    };
    bus.call_method(
        Some("org.freedesktop.DBus"),
        "/org/freedesktop/DBus",
        Some("org.freedesktop.DBus"),
        "NameHasOwner",
        &("org.kde.StatusNotifierWatcher",),
    )
    .and_then(|reply| reply.body().deserialize::<bool>())
    .unwrap_or(false)
}

#[cfg(not(target_os = "linux"))]
fn host_available() -> bool {
    true
}
