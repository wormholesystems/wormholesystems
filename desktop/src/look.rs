//! The website's map panels, for the cards: their colors, the panel header, the small
//! uppercase labels and the rows. From `frontend/src/app.css`, `MapPanel*.svelte` and the
//! cards in `routes/maps/[id]/components/panels`; change both together.

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;

use gpui_kit::component::Theme;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, Div, ElementId, Entity, FontWeight, Hsla, Image, InteractiveElement, IntoElement,
    MouseButton, ParentElement, Stateful, StatefulInteractiveElement, Styled, div, img, px, svg,
};

use crate::overlay::Floating;
use crate::sigs::{Command, Grab, Sigs};
use crate::theme::oklch;
use crate::ui::rpx;

/// `font-mono`: what the website's stack lands on per platform.
pub(crate) const MONO: &str = if cfg!(target_os = "windows") {
    "Consolas"
} else if cfg!(target_os = "macos") {
    "Menlo"
} else {
    "Liberation Mono"
};

/// `font-sans`, which the website sets to Inter.
pub(crate) const SANS: &str = "Inter";

/// `bg-card`.
pub(crate) fn card(theme: &Theme) -> Hsla {
    if theme.mode.is_dark() {
        oklch(0.205, 0.0, 0.0)
    } else {
        oklch(1.0, 0.0, 0.0)
    }
}

// Tailwind's palette, the shades the panels use.
pub(crate) fn green_400() -> Hsla {
    oklch(0.792, 0.209, 151.711)
}
pub(crate) fn green_500() -> Hsla {
    oklch(0.723, 0.219, 149.579)
}
pub(crate) fn amber_400() -> Hsla {
    oklch(0.828, 0.189, 84.429)
}
pub(crate) fn amber_500() -> Hsla {
    oklch(0.769, 0.188, 70.08)
}
pub(crate) fn red_400() -> Hsla {
    oklch(0.704, 0.191, 22.216)
}
pub(crate) fn red_500() -> Hsla {
    oklch(0.637, 0.237, 25.331)
}
pub(crate) fn sky_500() -> Hsla {
    oklch(0.685, 0.169, 237.323)
}

/// `jumpTone`: close, further, far.
pub(crate) fn jump_tone(jumps: usize) -> Hsla {
    if jumps < 8 {
        green_400()
    } else if jumps < 15 {
        amber_400()
    } else {
        red_400()
    }
}

/// `effectTextColor`.
pub(crate) fn effect_color(name: &str, theme: &Theme) -> Hsla {
    let n = name.to_lowercase();
    if n.contains("pulsar") {
        oklch(0.707, 0.165, 254.624)
    } else if n.contains("magnetar") {
        oklch(0.718, 0.202, 349.761)
    } else if n.contains("wolf") {
        oklch(0.666, 0.179, 58.318)
    } else if n.contains("black") {
        oklch(0.708, 0.0, 0.0)
    } else if n.contains("cataclysmic") {
        oklch(0.852, 0.199, 91.936)
    } else if n.contains("red giant") {
        red_400()
    } else {
        theme.muted_foreground
    }
}

/// A node's border by the map's intel status, `--color-status-*`.
pub(crate) fn status_color(status: Option<&str>, theme: &Theme) -> Hsla {
    match status {
        Some("active") => oklch(0.705, 0.213, 47.604),
        Some("empty") => oklch(0.765, 0.177, 163.223),
        Some("friendly") => oklch(0.623, 0.214, 259.815),
        Some("hostile") => oklch(0.637, 0.237, 25.331),
        Some("unscanned") => oklch(0.715, 0.143, 215.221),
        _ if theme.mode.is_dark() => oklch(0.371, 0.0, 0.0),
        _ => oklch(0.87, 0.0, 0.0),
    }
}

/// The class, short and in its color, in mono like `ClassBadge`.
pub(crate) fn class_badge(class: Option<i64>, security: Option<f64>) -> Div {
    let (label, color) = crate::theme::class_meta(class, security);
    div()
        .flex_none()
        .font_family(MONO)
        .text_color(color)
        .child(label)
}

/// `MapPanelHeader`: the title in small mono capitals on a muted band, things on the right,
/// and the card's close button. Pressing the title drags the card.
pub(crate) fn header(
    entity: &Entity<Sigs>,
    card: Floating,
    title: impl IntoElement,
    actions: Option<AnyElement>,
    grab: Option<Grab>,
    theme: &Theme,
) -> Div {
    div()
        .flex_none()
        .h(rpx(36.0))
        .flex()
        .items_center()
        .justify_between()
        .gap_2()
        .px(rpx(12.0))
        .border_b_1()
        .border_color(theme.border.opacity(0.5))
        .bg(theme.muted.opacity(0.3))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .h_full()
                .flex()
                .items_center()
                .gap_1()
                .overflow_hidden()
                .font_family(MONO)
                .text_size(rpx(10.0))
                .text_color(theme.muted_foreground)
                .when_some(grab, |d, grab| {
                    d.cursor_move()
                        .on_mouse_down(MouseButton::Left, move |e, w, cx| grab(e, w, cx))
                })
                .child(title),
        )
        .child(
            div()
                .flex_none()
                .flex()
                .items_center()
                .gap_2()
                .children(actions)
                .child(close(entity, card, theme)),
        )
}

fn close(entity: &Entity<Sigs>, card: Floating, theme: &Theme) -> Stateful<Div> {
    let entity = entity.clone();
    div()
        .id(ElementId::from(format!("hide-{card:?}")))
        .p(px(2.0))
        .rounded(px(4.0))
        .cursor_pointer()
        .text_color(theme.muted_foreground)
        .hover(|s| s.bg(theme.muted).text_color(theme.foreground))
        .on_click(move |_, _, cx| entity.update(cx, |_, cx| cx.emit(Command::HideCard(card))))
        .child(
            svg()
                .path("icons/x.svg")
                .size(rpx(12.0))
                .text_color(theme.muted_foreground),
        )
}

/// A count after a title, amber like the website's.
pub(crate) fn count(n: usize) -> Div {
    div()
        .font_family(MONO)
        .text_color(amber_400())
        .child(n.to_string())
}

/// The small uppercase label over a section.
pub(crate) fn label(text: &str, theme: &Theme) -> Div {
    div()
        .text_size(rpx(10.0))
        .text_color(theme.muted_foreground)
        .child(text.to_uppercase())
}

/// A band inside a card that starts another list, like a card header of its own.
pub(crate) fn band(title: impl IntoElement, theme: &Theme) -> Div {
    div()
        .flex_none()
        .h(rpx(28.0))
        .flex()
        .items_center()
        .gap_2()
        .px(rpx(12.0))
        .border_y_1()
        .border_color(theme.border.opacity(0.5))
        .bg(theme.muted.opacity(0.3))
        .font_family(MONO)
        .text_size(rpx(10.0))
        .text_color(theme.muted_foreground)
        .child(title)
}

/// A list row: hairline under it, lit on hover.
pub(crate) fn row(id: impl Into<ElementId>, theme: &Theme) -> Stateful<Div> {
    div()
        .id(id)
        .flex()
        .items_center()
        .gap_2()
        .px(rpx(12.0))
        .py(rpx(4.0))
        .text_size(rpx(12.0))
        .border_b_1()
        .border_color(theme.border.opacity(0.3))
        .hover(|s| s.bg(theme.muted.opacity(0.3)))
}

/// A picture from EVE's image server, or a muted square while it loads.
pub(crate) fn picture(images: &Images, url: String, size: f32, theme: &Theme) -> AnyElement {
    match images.get(url) {
        Some(image) => img(image)
            .flex_none()
            .size(rpx(size))
            .rounded(px(3.0))
            .into_any_element(),
        None => div()
            .flex_none()
            .size(rpx(size))
            .rounded(px(3.0))
            .bg(theme.muted)
            .into_any_element(),
    }
}

pub(crate) fn portrait(character_id: i64) -> String {
    format!("https://images.evetech.net/characters/{character_id}/portrait?size=64")
}

pub(crate) fn ship(type_id: i64) -> String {
    format!("https://images.evetech.net/types/{type_id}/icon?size=32")
}

/// An alliance's, corporation's or faction's logo; the image server keeps factions with the
/// corporations.
pub(crate) fn logo(kind: &str, id: i64) -> String {
    let path = if kind == "alliance" {
        "alliances"
    } else {
        "corporations"
    };
    format!("https://images.evetech.net/{path}/{id}/logo?size=64")
}

/// Pictures from EVE's image server by URL. Asking for one not here yet notes it for the app
/// to fetch; one that failed stays missing rather than being asked for again.
#[derive(Default)]
pub(crate) struct Images {
    loaded: RefCell<HashMap<String, Option<Arc<Image>>>>,
    wanted: RefCell<Vec<String>>,
}

impl Images {
    pub(crate) fn get(&self, url: String) -> Option<Arc<Image>> {
        if let Some(image) = self.loaded.borrow().get(&url) {
            return image.clone();
        }
        let mut wanted = self.wanted.borrow_mut();
        if !wanted.contains(&url) {
            wanted.push(url);
        }
        None
    }

    /// The pictures asked for since last time, marked as on their way.
    pub(crate) fn take_wanted(&self) -> Vec<String> {
        let wanted = std::mem::take(&mut *self.wanted.borrow_mut());
        let mut loaded = self.loaded.borrow_mut();
        wanted
            .into_iter()
            .filter(|url| loaded.insert(url.clone(), None).is_none())
            .collect()
    }

    pub(crate) fn arrived(&self, url: String, image: Arc<Image>) {
        self.loaded.borrow_mut().insert(url, Some(image));
    }
}

/// `text-sm font-medium`, the size of a card's main line.
pub(crate) fn strong(text: impl Into<gpui_kit::SharedString>) -> Div {
    div()
        .text_size(rpx(14.0))
        .font_weight(FontWeight::MEDIUM)
        .child(text.into())
}
