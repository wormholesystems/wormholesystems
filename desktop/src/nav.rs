//! The navigation card: how many jumps it is from where you are to every system on the
//! watch lists and to each of the map's pilots, the way the website's watch list counts
//! them. Click one and it becomes your autopilot destination.

use std::collections::{HashMap, HashSet};
use std::time::Instant;

use gpui_kit::component::{ActiveTheme as _, Theme};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, App, Div, ElementId, Entity, FontWeight, InteractiveElement, IntoElement,
    ParentElement, Stateful, StatefulInteractiveElement, Styled, div, svg,
};

use crate::api::{NamedSystem, Pilot, WatchEntry};
use crate::route::ScoutHole;
use crate::sigs::{Command, Grab, Sigs};
use crate::theme;
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

/// A row: a system, its jumps, and who or what it is for.
struct Row {
    key: String,
    system: i64,
    label: Option<String>,
    pinned: bool,
    jumps: Option<usize>,
}

pub(crate) fn panel(entity: &Entity<Sigs>, grab: Option<Grab>, cx: &App) -> AnyElement {
    let theme = cx.theme().clone();
    let sigs = entity.read(cx);
    let pad = rpx(8.0);
    let routes = sigs.routes.as_deref();
    let from = sigs.here.as_ref().map(|h| h.system.clone());
    let sub = match (&from, routes) {
        (Some(system), Some(_)) => format!("From {system}"),
        (Some(system), None) => format!("From {system}, working out routes…"),
        (None, _) => "Log in a character; the chat logs say where you are.".into(),
    };

    let header = crate::card::header(
        entity,
        crate::overlay::Floating::Navigation,
        div().child("Navigation"),
        sub,
        grab,
        &theme,
    );

    let jumps = |id: i64| routes.and_then(|r| r.jumps(id));
    let me = sigs.here.as_ref().map(|h| h.character.as_str());
    let (watch, pilots) = sigs.nav.as_ref().map_or((Vec::new(), Vec::new()), |nav| {
        let watch = sort(
            nav.watch
                .iter()
                .map(|w| Row {
                    key: format!("w{}", w.solar_system_id),
                    system: w.solar_system_id,
                    label: None,
                    pinned: w.is_pinned,
                    jumps: jumps(w.solar_system_id),
                })
                .collect(),
        );
        let pilots = sort(
            nav.pilots
                .iter()
                .filter(|p| Some(p.name.as_str()) != me)
                .filter_map(|p| {
                    let system = p.solar_system_id?;
                    Some(Row {
                        key: format!("p{}", p.name),
                        system,
                        label: Some(if p.corporation_ticker.is_empty() {
                            p.name.clone()
                        } else {
                            format!("{} [{}]", p.name, p.corporation_ticker)
                        }),
                        pinned: false,
                        jumps: jumps(system),
                    })
                })
                .collect(),
        );
        (watch, pilots)
    });

    let names = sigs.nav.as_ref().map(|n| &n.names);
    let section = |title: &'static str, rows: Vec<Row>, empty: &'static str| {
        let mut d = div().flex().flex_col().text_xs().child(
            div()
                .px(pad)
                .pt_2()
                .pb_1()
                .text_color(theme.muted_foreground)
                .font_weight(FontWeight::SEMIBOLD)
                .child(title),
        );
        if rows.is_empty() {
            d = d.child(
                div()
                    .px(pad)
                    .pb_1()
                    .text_color(theme.muted_foreground)
                    .child(empty),
            );
        }
        for r in rows {
            let system = names.and_then(|n| n.get(&r.system));
            d = d.child(row(entity, &r, system, &theme));
        }
        d
    };

    div()
        .size_full()
        .flex()
        .flex_col()
        .child(header)
        .child(
            div()
                .id("nav-body")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .pb_2()
                .child(section("Watch list", watch, "Nothing on the watch list."))
                .child(section("Pilots", pilots, "Nobody else is online.")),
        )
        .into_any_element()
}

/// Pinned first, then the nearest; the unreachable last.
fn sort(mut rows: Vec<Row>) -> Vec<Row> {
    rows.sort_by_key(|r| (!r.pinned, r.jumps.is_none(), r.jumps));
    rows
}

fn row(
    entity: &Entity<Sigs>,
    r: &Row,
    system: Option<&NamedSystem>,
    theme: &Theme,
) -> Stateful<Div> {
    let (class, color) = system.map_or(("?", theme.muted_foreground), |s| {
        theme::class_meta(s.wormhole_class_id, Some(s.security))
    });
    let name = system.map_or_else(|| format!("#{}", r.system), |s| s.name.clone());
    let region = system.map(|s| s.region.clone()).unwrap_or_default();
    let jumps = r
        .jumps
        .map_or(("—".to_string(), theme.muted_foreground), |j| {
            (j.to_string(), crate::route::tone(j))
        });
    let destination = r.system;
    let entity = entity.clone();
    div()
        .id(ElementId::from(r.key.clone()))
        .flex()
        .items_center()
        .gap_2()
        .px(rpx(8.0))
        .py(rpx(3.0))
        .cursor_pointer()
        .hover(|s| s.bg(theme.muted))
        .on_click(move |_, _, cx| {
            entity.update(cx, |_, cx| cx.emit(Command::SetDestination(destination)));
        })
        .child(
            div()
                .flex_none()
                .w(rpx(28.0))
                .text_color(color)
                .child(class),
        )
        .child(
            div()
                .flex_none()
                .max_w(rpx(150.0))
                .truncate()
                .child(r.label.clone().unwrap_or(name.clone())),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .text_color(theme.muted_foreground)
                .child(if r.label.is_some() { name } else { region }),
        )
        .when(r.pinned, |d| {
            d.child(
                svg()
                    .path("icons/flag.svg")
                    .size(rpx(10.0))
                    .flex_none()
                    .text_color(theme.muted_foreground),
            )
        })
        .child(
            div()
                .flex_none()
                .w(rpx(24.0))
                .text_right()
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(jumps.1)
                .child(jumps.0),
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(system: i64, jumps: Option<usize>) -> Row {
        Row {
            key: String::new(),
            system,
            label: None,
            pinned: false,
            jumps,
        }
    }

    #[test]
    fn pinned_first_then_nearest_then_unreachable() {
        let rows = sort(vec![
            r(1, None),
            r(2, Some(9)),
            Row {
                pinned: true,
                ..r(3, Some(20))
            },
            r(4, Some(2)),
        ]);
        let order: Vec<i64> = rows.iter().map(|r| r.system).collect();
        assert_eq!(order, vec![3, 4, 2, 1]);
    }
}
