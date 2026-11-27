//! A popup of the overlay's in a window of its own: a transparent layer-shell surface over
//! the overlay's screen with the popup drawn in it where its button is. The overlay only
//! hears of clicks that land on it and never takes the keyboard, so there a popup could not
//! be closed by a click in the game, nor searched. This surface gets every click, and one
//! outside the popup closes it; a type picker's takes the keyboard for its search, where
//! Enter picks the first match and Esc closes.
//!
//! What is open stays in [`Sigs`], as it does for the popups drawn in place: the window
//! shows the same choices, their clicks emit the same commands, and it goes when its popup
//! is no longer the one open.
//!
//! Linux only: nowhere else is there a layer shell, and the overlay draws its popups in
//! place.

use gpui_kit::component::ActiveTheme as _;
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::{
    Anchor, AnyWindowHandle, App, AppContext, Bounds, Context, DisplayId, Entity,
    InteractiveElement, IntoElement, KeyDownEvent, MouseButton, MouseDownEvent, ParentElement,
    Render, Styled, Subscription, Window, WindowBackgroundAppearance, WindowBounds, WindowKind,
    WindowOptions, anchored, div, point, px, size,
};

use crate::api::SignatureEdit;
use crate::sigs::{self, Command, PickerKind, Popup, Side, Sigs};

pub(crate) struct Popover {
    sigs: Entity<Sigs>,
    popup: Popup,
    /// Where the pointer was when it opened, on its button.
    at: (f32, f32),
    /// A type picker's search.
    search: Option<Entity<InputState>>,
    gone: bool,
    _subscriptions: Vec<Subscription>,
}

/// The signature a popup is about and whether it is a type picker's.
fn subject(popup: Popup) -> Option<(i64, bool)> {
    match popup {
        Popup::Picker(picker) => Some((picker.sig, picker.kind == PickerKind::Type)),
        Popup::Quick => None,
    }
}

impl Popover {
    fn new(
        sigs: Entity<Sigs>,
        popup: Popup,
        at: (f32, f32),
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) -> Self {
        let mut subscriptions = vec![cx.observe_in(&sigs, window, |this, _, window, cx| {
            let sigs = this.sigs.read(cx);
            let there = subject(this.popup).is_none_or(|(sig, _)| sigs.signature(sig).is_some());
            if sigs.is_open(this.popup) && there {
                cx.notify();
            } else if !std::mem::replace(&mut this.gone, true) {
                window.remove_window();
            }
        })];
        let search =
            subject(popup).filter(|(_, types)| *types).map(|_| {
                let input = cx.new(|cx| InputState::new(window, cx).placeholder(sigs::TYPE_SEARCH));
                input.update(cx, |input, cx| input.focus(window, cx));
                subscriptions.push(cx.subscribe_in(&input, window, |this, _, event, _, cx| {
                    match event {
                        InputEvent::PressEnter { .. } => this.pick_first(cx),
                        InputEvent::Change => cx.notify(),
                        _ => {}
                    }
                }));
                input
            });
        Popover {
            sigs,
            popup,
            at,
            search,
            gone: false,
            _subscriptions: subscriptions,
        }
    }

    /// Close the popup; the window follows.
    fn dismiss(&mut self, cx: &mut Context<'_, Self>) {
        let popup = self.popup;
        self.sigs.update(cx, |sigs, cx| {
            if sigs.close(popup) {
                cx.notify();
            }
        });
    }

    /// Enter in the search: the first type listed is the one.
    fn pick_first(&mut self, cx: &mut Context<'_, Self>) {
        let (Some((key, _)), Some(search)) = (subject(self.popup), &self.search) else {
            return;
        };
        let query = search.read(cx).value();
        let sigs = self.sigs.read(cx);
        let first = sigs
            .signature(key)
            .and_then(|sig| sigs.type_choices(sig, &query).first());
        let Some(first) = first else { return };
        let popup = self.popup;
        self.sigs.update(cx, |sigs, cx| {
            sigs.close(popup);
            cx.emit(Command::Edit(key, SignatureEdit::Type(Some(first))));
            cx.notify();
        });
    }
}

impl Render for Popover {
    fn render(&mut self, window: &mut Window, cx: &mut Context<'_, Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let outside = || cx.listener(|this, _: &MouseDownEvent, _, cx| this.dismiss(cx));
        let (left, right) = (outside(), outside());
        let escape = cx.listener(|this, event: &KeyDownEvent, _, cx| {
            if event.keystroke.key == "escape" {
                cx.stop_propagation();
                this.dismiss(cx);
            }
        });
        let sigs = self.sigs.read(cx);
        // Outside the kit's root, so the text size is set here: the overlay's.
        window.set_rem_size(px(crate::ui::scaled_font(sigs.overlay.text_size)));
        let content = match self.popup {
            Popup::Quick => Some(sigs::quick_settings(&self.sigs, sigs, &theme)),
            Popup::Picker(picker) => sigs.signature(picker.sig).map(|sig| {
                let connection = sig
                    .connection_id
                    .and_then(|c| sigs.snapshot.as_ref()?.connection(c));
                sigs::picker(
                    &self.sigs,
                    sigs,
                    sig,
                    connection,
                    picker.kind,
                    &theme,
                    self.search.as_ref(),
                    cx,
                )
            }),
        };
        let hang = self.popup.hang();
        // Under the pointer, which is on the button, and kept on the screen.
        let (x, y) = self.at;
        let (corner, x) = match hang.side {
            Side::Left => (Anchor::TopLeft, x - 24.0),
            Side::Right => (Anchor::TopRight, x + 12.0),
        };
        div()
            .size_full()
            .text_color(theme.foreground)
            .font_family(crate::theme::UI)
            .on_mouse_down(MouseButton::Left, left)
            .on_mouse_down(MouseButton::Right, right)
            .capture_key_down(escape)
            .children(content.map(|content| {
                anchored()
                    .position(point(px(x), px(y + 14.0)))
                    .anchor(corner)
                    .snap_to_window_with_margin(px(8.0))
                    .child(sigs::popup_surface(&hang, content, cx))
            }))
    }
}

/// Open `popup` by `at` on `display`, the screen the overlay is on. Only where there is a
/// layer shell to open it on.
pub(crate) fn open(
    sigs: Entity<Sigs>,
    popup: Popup,
    at: (f32, f32),
    display: Option<DisplayId>,
    cx: &mut App,
) -> anyhow::Result<AnyWindowHandle> {
    use gpui_kit::layer_shell::{Anchor, KeyboardInteractivity, Layer, LayerShellOptions};
    let typed = subject(popup).is_some_and(|(_, types)| types);
    let options = WindowOptions {
        titlebar: None,
        // Anchored to every edge, like the overlay: the compositor makes it the size of
        // the screen, so the overlay's coordinates are this one's.
        window_bounds: Some(WindowBounds::Windowed(Bounds::new(
            point(px(0.0), px(0.0)),
            size(px(0.0), px(0.0)),
        ))),
        display_id: display,
        app_id: Some("wormholesystems-popover".into()),
        window_background: WindowBackgroundAppearance::Transparent,
        kind: WindowKind::LayerShell(LayerShellOptions {
            namespace: "wormholesystems-popover".into(),
            layer: Layer::Overlay,
            anchor: Anchor::TOP | Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT,
            // Only a search is typed into; the rest leave the keys to the game.
            keyboard_interactivity: if typed {
                KeyboardInteractivity::Exclusive
            } else {
                KeyboardInteractivity::None
            },
            ..Default::default()
        }),
        focus: typed,
        ..Default::default()
    };
    // Opened with GPUI's own `open_window`, as the overlay is: the kit's root would
    // make the surface bigger than the screen.
    let handle = cx.open_window(options, move |window, cx| {
        cx.new(|cx| Popover::new(sigs, popup, at, window, cx))
    })?;
    Ok(handle.into())
}
