//! The navigation card, the website's watch list and Pilots card in one: how many jumps it is
//! from where you are to every system on the watch lists and to each of the map's pilots,
//! counted the way the website counts them. Click one and it becomes your autopilot
//! destination.

use std::collections::{HashMap, HashSet};
use std::time::Instant;

use gpui_kit::component::{ActiveTheme as _, Theme};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, App, ClickEvent, Div, ElementId, Entity, FontWeight, InteractiveElement,
    IntoElement, ParentElement, StatefulInteractiveElement, Styled, Window, div, px, relative, svg,
};

use crate::api::{NamedSystem, Pilot, WatchEntry};
use crate::look::{self, Images, MONO};
use crate::overlay::Floating;
use crate::route::{Preference, ScoutHole};
use crate::sigs::{Command, Grab, Sigs};
use crate::ui::rpx;

/// What the navigation and chain cards know beyond the map, for the whole map.
#[derive(Debug, Clone, Default)]
pub(crate) struct Nav {
    pub watch: Vec<WatchEntry>,
    pub pilots: Vec<Pilot>,
    pub scout: Vec<ScoutHole>,
    pub ignored: HashSet<i64>,
    /// Names of the systems the rows mention, kept across fetches.
    pub names: HashMap<i64, NamedSystem>,
    pub fetched: Option<Instant>,
}

impl Nav {
    /// The systems the rows need named that are not yet.
    pub(crate) fn unnamed(&self) -> Vec<i64> {
        let mut ids: Vec<i64> = self
            .watch
            .iter()
            .map(|w| w.solar_system_id)
            .chain(self.pilots.iter().filter_map(|p| p.solar_system_id))
            .filter(|id| !self.names.contains_key(id))
            .collect();
        ids.sort_unstable();
        ids.dedup();
        ids
    }
}

/// The route preference as the card's title says it.
fn preference(p: Preference) -> &'static str {
    match p {
        Preference::Shorter => "Shortest",
        Preference::Safer => "Safer",
        Preference::LessSecure => "Less secure",
    }
}

/// Pinned first, then the nearest; the unreachable last.
fn watch_order(watch: &mut [&WatchEntry], jumps: impl Fn(i64) -> Option<usize>) {
    watch.sort_by_key(|w| {
        let j = jumps(w.solar_system_id);
        (!w.is_pinned, j.is_none(), j)
    });
}

pub(crate) fn panel(entity: &Entity<Sigs>, grab: Option<Grab>, cx: &App) -> AnyElement {
    let theme = cx.theme().clone();
    let sigs = entity.read(cx);
    let title = div()
        .flex()
        .items_center()
        .gap_1()
        .child("NAVIGATION")
        .child(
            div()
                .ml_1()
                .text_color(theme.muted_foreground.opacity(0.6))
                .child(preference(sigs.route_preference)),
        );
    let origin = sigs.here.as_ref().map(|h| {
        div()
            .max_w(rpx(120.0))
            .truncate()
            .text_size(rpx(10.0))
            .text_color(theme.muted_foreground)
            .child(format!("from {}", h.system))
            .into_any_element()
    });
    let header = look::header(entity, Floating::Navigation, title, origin, grab, &theme);
    let routes = sigs.routes.as_deref();
    let jumps = |id: i64| routes.and_then(|r| r.jumps(id));
    let empty = crate::nav::Nav::default();
    let nav = sigs.nav.as_ref().unwrap_or(&empty);

    let mut watch: Vec<&WatchEntry> = nav.watch.iter().collect();
    watch_order(&mut watch, jumps);
    let mut list = div().flex().flex_col().child(columns(&theme));
    if watch.is_empty() {
        list = list.child(
            div()
                .p(rpx(12.0))
                .flex()
                .justify_center()
                .font_family(MONO)
                .text_size(rpx(10.0))
                .text_color(theme.muted_foreground.opacity(0.6))
                .child("NOTHING ON THE WATCHLIST"),
        );
    }
    for w in watch {
        let system = nav.names.get(&w.solar_system_id);
        list = list.child(watch_row(
            entity,
            w,
            system,
            jumps(w.solar_system_id),
            &theme,
        ));
    }

    list = list.child(pilots(entity, sigs, nav, &jumps, &theme));

    div()
        .size_full()
        .flex()
        .flex_col()
        .font_family(look::SANS)
        .child(header)
        .child(
            div()
                .id("nav-body")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .child(list),
        )
        .into_any_element()
}

/// The map's pilots other than you, nearest first, as the website's Pilots card lists them.
fn pilots(
    entity: &Entity<Sigs>,
    sigs: &Sigs,
    nav: &Nav,
    jumps: &dyn Fn(i64) -> Option<usize>,
    theme: &Theme,
) -> Div {
    let me = sigs.here.as_ref().map(|h| h.character.as_str());
    let mut pilots: Vec<&Pilot> = nav
        .pilots
        .iter()
        .filter(|p| Some(p.name.as_str()) != me)
        .collect();
    pilots.sort_by_key(|p| {
        let j = p.solar_system_id.and_then(jumps);
        (j.is_none(), j, p.name.clone())
    });

    let mut list = div().flex().flex_col();
    let title = div()
        .flex()
        .items_center()
        .gap_2()
        .child(div().size(px(6.0)).rounded_full().bg(look::green_500()))
        .child("PILOTS")
        .child(look::count(pilots.len()));
    list = list.child(look::band(title, theme));
    if pilots.is_empty() {
        list = list.child(
            div()
                .px(rpx(12.0))
                .py(rpx(16.0))
                .text_size(rpx(12.0))
                .text_color(theme.muted_foreground)
                .child("No pilots online. Anyone who turns on Share location for this map shows up here."),
        );
    }
    let snapshot = sigs.snapshot.as_deref();
    for p in pilots {
        let alias = p.solar_system_id.and_then(|id| {
            snapshot?
                .systems
                .iter()
                .find(|s| s.solar_system_id == id)?
                .alias
                .clone()
                .filter(|a| !a.trim().is_empty())
        });
        let at = Where {
            system: p.solar_system_id.and_then(|id| nav.names.get(&id)),
            alias,
            jumps: p.solar_system_id.and_then(jumps),
        };
        list = list.child(pilot_row(entity, p, at, &sigs.images, theme));
    }

    list
}

/// The watch list's column headings.
fn columns(theme: &Theme) -> Div {
    div()
        .flex()
        .items_center()
        .gap_2()
        .px(rpx(12.0))
        .py(rpx(6.0))
        .border_b_1()
        .border_color(theme.border.opacity(0.3))
        .bg(theme.muted.opacity(0.2))
        .font_family(MONO)
        .text_size(rpx(10.0))
        .text_color(theme.muted_foreground)
        .child(div().flex_1().min_w_0().child("SYSTEM"))
        .child(div().w(relative(0.32)).child("REGION"))
        .child(
            div()
                .w(rpx(56.0))
                .pr(rpx(16.0))
                .flex()
                .justify_end()
                .child("J"),
        )
}

/// `42j` in its tone, or `--` for a system the routes cannot reach.
fn jumps_cell(jumps: Option<usize>, theme: &Theme) -> Div {
    let cell = div().flex_none().w(rpx(36.0)).flex().justify_end();
    match jumps {
        Some(j) => cell
            .text_size(rpx(12.0))
            .font_weight(FontWeight::MEDIUM)
            .text_color(look::jump_tone(j))
            .child(format!("{j}j")),
        None => cell
            .text_size(rpx(10.0))
            .text_color(theme.muted_foreground.opacity(0.6))
            .child("--"),
    }
}

/// Clicking a row sets the destination.
fn destination(
    entity: &Entity<Sigs>,
    system: i64,
) -> impl Fn(&ClickEvent, &mut Window, &mut App) + 'static {
    let entity = entity.clone();
    move |_, _, cx| entity.update(cx, |_, cx| cx.emit(Command::SetDestination(system)))
}

/// A watched system: class, name, region and statics, the jumps, and a pin when pinned.
fn watch_row(
    entity: &Entity<Sigs>,
    w: &WatchEntry,
    system: Option<&NamedSystem>,
    jumps: Option<usize>,
    theme: &Theme,
) -> impl IntoElement {
    let id = w.solar_system_id;
    let statics = system.map(|s| s.statics.as_slice()).unwrap_or_default();
    let name = system.map_or_else(|| id.to_string(), |s| s.name.clone());
    let pin = if w.is_pinned {
        svg()
            .path("icons/pin.svg")
            .size(rpx(12.0))
            .text_color(look::amber_400())
            .into_any_element()
    } else {
        div().size(rpx(12.0)).into_any_element()
    };
    look::row(
        ElementId::from(("watch", u64::try_from(id).unwrap_or(0))),
        theme,
    )
    .cursor_pointer()
    .on_click(destination(entity, id))
    .child(
        div()
            .flex_1()
            .min_w_0()
            .flex()
            .items_center()
            .gap_2()
            .child(look::class_badge(
                system.and_then(|s| s.wormhole_class_id),
                system.map(|s| s.security),
            ))
            .child(
                div()
                    .min_w_0()
                    .truncate()
                    .text_color(theme.foreground)
                    .child(name),
            ),
    )
    .child(
        div()
            .w(relative(0.32))
            .flex()
            .items_center()
            .gap_1()
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_color(theme.muted_foreground)
                    .child(system.map(|s| s.region.clone()).unwrap_or_default()),
            )
            .children(statics.iter().map(|st| {
                let (dest, color) = crate::theme::dest_class_meta(st.dest_class);
                div()
                    .flex_none()
                    .text_size(rpx(10.0))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(color)
                    .child(dest)
            })),
    )
    .child(
        div()
            .flex_none()
            .flex()
            .items_center()
            .gap_1()
            .child(jumps_cell(jumps, theme))
            .child(pin),
    )
}

/// Where a pilot is, and how far.
struct Where<'a> {
    system: Option<&'a NamedSystem>,
    alias: Option<String>,
    jumps: Option<usize>,
}

/// A pilot, as the website's Pilots card has them: portrait, name, ship, where, jumps.
fn pilot_row(
    entity: &Entity<Sigs>,
    p: &Pilot,
    at: Where<'_>,
    images: &Images,
    theme: &Theme,
) -> impl IntoElement {
    let row = look::row(
        ElementId::from(("nav-pilot", u64::try_from(p.character_id).unwrap_or(0))),
        theme,
    );
    let row = match p.solar_system_id {
        Some(id) => row.cursor_pointer().on_click(destination(entity, id)),
        None => row,
    };
    let place = match at.system {
        Some(s) => div()
            .flex()
            .items_center()
            .gap(rpx(6.0))
            .min_w_0()
            .child(look::class_badge(s.wormhole_class_id, Some(s.security)).text_size(rpx(10.0)))
            .when_some(at.alias, |d, a| {
                d.child(div().flex_none().font_weight(FontWeight::MEDIUM).child(a))
                    .child(div().flex_none().child("·"))
            })
            .child(div().min_w_0().truncate().child(s.name.clone())),
        None => div()
            .text_color(theme.muted_foreground.opacity(0.6))
            .child("Unknown"),
    };
    row.child(look::picture(
        images,
        look::portrait(p.character_id),
        20.0,
        theme,
    ))
    .child(
        div()
            .flex_none()
            .w(rpx(96.0))
            .flex()
            .items_center()
            .gap_1()
            .child(
                div()
                    .truncate()
                    .when(p.is_mine, |d| {
                        d.font_weight(FontWeight::MEDIUM)
                            .text_color(theme.foreground)
                    })
                    .child(p.name.clone()),
            )
            .when(p.is_docked, |d| {
                d.child(
                    div()
                        .flex_none()
                        .text_size(rpx(10.0))
                        .text_color(theme.muted_foreground)
                        .child("(D)"),
                )
            }),
    )
    .child(
        div()
            .flex_1()
            .min_w_0()
            .flex()
            .items_center()
            .gap(rpx(6.0))
            .when_some(p.ship_type_id, |d, id| {
                d.child(look::picture(images, look::ship(id), 16.0, theme))
            })
            .child(
                div()
                    .min_w_0()
                    .truncate()
                    .font_family(MONO)
                    .text_size(rpx(10.0))
                    .text_color(theme.muted_foreground)
                    .child(p.ship_type.clone().unwrap_or_else(|| "Unknown ship".into())),
            ),
    )
    .child(div().flex_1().min_w_0().child(place))
    .child(jumps_cell(at.jumps, theme))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(solar_system_id: i64) -> WatchEntry {
        WatchEntry {
            solar_system_id,
            is_pinned: false,
        }
    }

    #[test]
    fn pinned_first_then_nearest_then_unreachable() {
        let entries = [
            entry(1),
            entry(2),
            WatchEntry {
                is_pinned: true,
                ..entry(3)
            },
            entry(4),
        ];
        let jumps = HashMap::from([(2, 9), (3, 20), (4, 2)]);
        let mut watch: Vec<&WatchEntry> = entries.iter().collect();
        watch_order(&mut watch, |id| jumps.get(&id).copied());
        let order: Vec<i64> = watch.iter().map(|w| w.solar_system_id).collect();
        assert_eq!(order, vec![3, 4, 2, 1]);
    }
}
