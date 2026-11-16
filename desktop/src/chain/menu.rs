//! The map's right-click menus, as the website has them: on a system (`NodeMenu.svelte` around
//! `SystemMenuItems.svelte`), on a connection (`ConnectionMenu.svelte`) and on the empty map
//! (`MapMenu.svelte`), drawn like its dropdown menus with their submenus flying out to the
//! side. What needs the website's system search (adding a system or a connection, naming a
//! ghost) is left to the website.

use gpui_kit::component::Theme;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, Bounds, Div, ElementId, Entity, FontWeight, Hsla, InteractiveElement, IntoElement,
    ParentElement, Pixels, Point, StatefulInteractiveElement, Styled, anchored, canvas, deferred,
    div, px, svg,
};
use serde_json::json;

use crate::api::{
    Connection, ConnectionKind, MapSnapshot, MassStatus, TimeStatus, Waypoint, WormholeSize,
};
use crate::look;
use crate::sigs::{Command, MapChange, Sigs};
use crate::theme::oklch;

/// What was right-clicked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Target {
    /// A system or ghost, by placement.
    Node(i64),
    Connection(i64),
    /// The map around them.
    Canvas,
}

/// An open menu: where it was opened, in window coordinates, and on what.
#[derive(Debug, Clone)]
pub(crate) struct Open {
    pub at: Point<Pixels>,
    pub target: Target,
    /// The submenu showing, by its row in the menu.
    sub: Option<usize>,
    /// The connection's bookmark names, once they have arrived.
    bookmarks: Option<Vec<String>>,
    /// Where it was drawn, which the overlay takes clicks on.
    pub bounds: Option<Bounds<Pixels>>,
}

impl Open {
    pub(crate) fn new(at: Point<Pixels>, target: Target) -> Self {
        Open {
            at,
            target,
            sub: None,
            bookmarks: None,
            bounds: None,
        }
    }

    pub(crate) fn bookmarks_for(&mut self, connection: i64, names: Vec<String>) {
        if self.target == Target::Connection(connection) {
            self.bookmarks = Some(names);
        }
    }
}

/// What a row does.
#[derive(Debug, Clone)]
enum Act {
    Change(MapChange),
    Copy(String),
    Url(String),
}

/// What sits before a row's label.
#[derive(Debug, Clone)]
enum Lead {
    None,
    Icon(&'static str),
    /// A colored dot, a status's.
    Dot(Hsla),
    /// A size letter in a fixed column.
    Letter(&'static str),
    Portrait(i64),
}

/// What sits at a row's right end.
#[derive(Debug, Clone)]
enum Tail {
    None,
    Hint(&'static str),
    Check,
    HintCheck(&'static str),
    Icon(&'static str, Hsla),
}

#[derive(Debug, Clone)]
enum Entry {
    Item {
        lead: Lead,
        label: String,
        tail: Tail,
        /// `None` shows it disabled.
        act: Option<Act>,
        danger: bool,
    },
    Sub {
        lead: Lead,
        label: String,
        width: f32,
        items: Vec<Entry>,
    },
    Separator,
    /// A group's heading, small capitals.
    Label(String),
    /// A line of small print.
    Note(String),
}

impl Entry {
    fn item(lead: Lead, label: impl Into<String>, act: Act) -> Entry {
        Entry::Item {
            lead,
            label: label.into(),
            tail: Tail::None,
            act: Some(act),
            danger: false,
        }
    }

    fn with_tail(mut self, new: Tail) -> Entry {
        if let Entry::Item { tail, .. } = &mut self {
            *tail = new;
        }
        self
    }

    fn sub(lead: Lead, label: &str, width: f32, items: Vec<Entry>) -> Entry {
        Entry::Sub {
            lead,
            label: label.into(),
            width,
            items,
        }
    }

    /// Its height in the menu, fixed so a submenu can line up with the row it flies out of.
    fn height(&self) -> f32 {
        match self {
            Entry::Item { .. } | Entry::Sub { .. } => 32.0,
            Entry::Separator => 9.0,
            Entry::Label(_) => 28.0,
            Entry::Note(_) => 22.0,
        }
    }
}

fn post(action: &'static str, body: serde_json::Value, done: impl Into<String>) -> Act {
    Act::Change(MapChange::Post {
        action,
        body,
        done: done.into(),
    })
}

/// `STATUS_OPTIONS` with their icons.
const STATUSES: [(&str, &str); 6] = [
    ("unknown", "icons/circle-question-mark.svg"),
    ("friendly", "icons/shield-check.svg"),
    ("hostile", "icons/skull.svg"),
    ("active", "icons/activity.svg"),
    ("unscanned", "icons/radar.svg"),
    ("empty", "icons/circle-dashed.svg"),
];

fn capitalized(s: &str) -> String {
    let mut chars = s.chars();
    chars
        .next()
        .map(|c| c.to_uppercase().chain(chars).collect())
        .unwrap_or_default()
}

/// The rows for what was clicked, or none when there is nothing to offer.
fn entries(sigs: &Sigs, snapshot: &MapSnapshot, open: &Open, theme: &Theme) -> Vec<Entry> {
    match open.target {
        Target::Node(id) => node(sigs, snapshot, id, theme),
        Target::Connection(id) => snapshot
            .connections
            .iter()
            .find(|c| c.id == id)
            .map(|c| connection(sigs, snapshot, c, open.bookmarks.as_deref()))
            .unwrap_or_default(),
        Target::Canvas => canvas_menu(snapshot),
    }
}

/// `NodeMenu.svelte`.
fn node(sigs: &Sigs, snapshot: &MapSnapshot, id: i64, theme: &Theme) -> Vec<Entry> {
    let mut rows = Vec::new();
    let placement = |value: serde_json::Value| -> serde_json::Value {
        let mut body = json!({ "map_solar_system_id": id });
        if let (Some(body), Some(more)) = (body.as_object_mut(), value.as_object()) {
            body.extend(more.clone());
        }
        body
    };
    let (pinned, home) = if let Some(s) = snapshot.systems.iter().find(|s| s.id == id) {
        let name = s
            .alias
            .clone()
            .filter(|a| !a.is_empty())
            .unwrap_or_else(|| s.name.clone());
        rows.push(Entry::item(
            Lead::Icon("icons/pin.svg"),
            if s.is_pinned { "Unpin" } else { "Pin" },
            post(
                "systems/set-pinned",
                placement(json!({ "value": !s.is_pinned })),
                format!("{} {name}", if s.is_pinned { "Unpinned" } else { "Pinned" }),
            ),
        ));
        let statuses = STATUSES
            .iter()
            .map(|&(status, icon)| {
                Entry::item(
                    Lead::None,
                    capitalized(status),
                    post(
                        "systems/set-status",
                        placement(json!({ "status": status })),
                        format!("{name} is {status}"),
                    ),
                )
                .with_tail(Tail::Icon(icon, look::status_color(Some(status), theme)))
            })
            .collect();
        rows.push(Entry::sub(
            Lead::Icon("icons/map.svg"),
            "Status",
            160.0,
            statuses,
        ));
        rows.push(Entry::Separator);
        rows.extend(system_items(sigs, s));
        rows.push(Entry::item(
            Lead::Icon("icons/house.svg"),
            if s.is_home {
                "Unset Home System"
            } else {
                "Set as Home System"
            },
            post(
                "systems/set-home",
                placement(json!({ "value": !s.is_home })),
                if s.is_home {
                    format!("{name} is no longer home")
                } else {
                    format!("{name} is home")
                },
            ),
        ));
        (s.is_pinned, s.is_home)
    } else if let Some(g) = snapshot.ghosts.iter().find(|g| g.id == id) {
        (g.is_pinned, g.is_home)
    } else {
        return rows;
    };
    if !pinned && !home {
        if !rows.is_empty() {
            rows.push(Entry::Separator);
        }
        rows.push(Entry::Item {
            lead: Lead::Icon("icons/trash.svg"),
            label: "Remove".into(),
            tail: Tail::None,
            act: Some(post(
                "systems/remove",
                json!({ "map_solar_system_ids": [id] }),
                "Removed from the map",
            )),
            danger: true,
        });
    }
    rows
}

/// `SystemMenuItems.svelte`: the watchlist, the external sites, waypoints, the route
/// planner's ignore list, and the rally point.
fn system_items(sigs: &Sigs, s: &crate::api::PlacedSystem) -> Vec<Entry> {
    let mut rows = Vec::new();
    let nav = sigs.nav.as_ref();
    let watched = nav.is_some_and(|n| {
        n.watch
            .iter()
            .any(|w| w.solar_system_id == s.solar_system_id)
    });
    if !watched {
        rows.push(Entry::item(
            Lead::Icon("icons/eye.svg"),
            "Add to watchlist",
            post(
                "watchlist/add",
                json!({ "solar_system_id": s.solar_system_id }),
                format!("{} is on the watchlist", s.name),
            ),
        ));
        rows.push(Entry::Separator);
    }
    let wormhole = wormhole_space(s);
    rows.push(Entry::sub(
        Lead::Icon("icons/external-link.svg"),
        "External",
        192.0,
        external(s),
    ));
    if !wormhole {
        rows.push(Entry::sub(
            Lead::Icon("icons/navigation.svg"),
            "Set destination",
            192.0,
            waypoints(sigs, s, Waypoint::Destination),
        ));
        rows.push(Entry::sub(
            Lead::Icon("icons/map-pin.svg"),
            "Add waypoint",
            192.0,
            waypoints(sigs, s, Waypoint::Added),
        ));
    }
    let ignored = nav.is_some_and(|n| n.ignored.contains(&s.solar_system_id));
    rows.push(Entry::sub(
        Lead::Icon("icons/route.svg"),
        "Route planner",
        208.0,
        vec![Entry::item(
            Lead::Icon("icons/ban.svg"),
            if ignored {
                "Stop ignoring for everyone"
            } else {
                "Ignore for everyone"
            },
            post(
                if ignored {
                    "ignored/remove"
                } else {
                    "ignored/add"
                },
                json!({ "solar_system_id": s.solar_system_id }),
                if ignored {
                    format!("Routes go through {} again", s.name)
                } else {
                    format!("Routes avoid {}", s.name)
                },
            ),
        )],
    ));
    rows.push(Entry::Separator);
    rows.push(Entry::item(
        Lead::Icon("icons/flag.svg"),
        if s.is_rally {
            "Clear Rally Point"
        } else {
            "Set as Rally Point"
        },
        post(
            "systems/set-rally",
            json!({ "map_solar_system_id": s.id, "value": !s.is_rally }),
            if s.is_rally {
                "Rally point cleared".to_owned()
            } else {
                format!("Rally on {}", s.name)
            },
        ),
    ));
    rows
}

/// `systemLinkGroups`: Dotlan and zKillboard.
/// ESI refuses a destination in wormhole space, and Dotlan has no jump range there.
fn wormhole_space(s: &crate::api::PlacedSystem) -> bool {
    s.wormhole_class_id
        .is_some_and(|c| matches!(c, 1..=6 | 12..=18))
}

fn external(s: &crate::api::PlacedSystem) -> Vec<Entry> {
    let wormhole = wormhole_space(s);
    let underscore = |v: &str| v.replace(' ', "_");
    let link = |icon, label: &str, url: String| Entry::item(Lead::Icon(icon), label, Act::Url(url));
    let dotlan = "https://evemaps.dotlan.net";
    let mut rows = vec![
        Entry::Label("Dotlan".into()),
        link(
            "icons/globe.svg",
            "System",
            format!("{dotlan}/system/{}", underscore(&s.name)),
        ),
        link(
            "icons/map.svg",
            "Region Map",
            format!(
                "{dotlan}/map/{}/{}",
                underscore(&s.region),
                underscore(&s.name)
            ),
        ),
    ];
    if !wormhole {
        rows.push(link(
            "icons/compass.svg",
            "Jump Range",
            format!("{dotlan}/range/Revelation,5/{}", underscore(&s.name)),
        ));
    }
    rows.extend([
        Entry::Separator,
        Entry::Label("zKillboard".into()),
        link(
            "icons/globe.svg",
            "System",
            format!("https://zkillboard.com/system/{}/", s.solar_system_id),
        ),
        link(
            "icons/compass.svg",
            "Constellation",
            format!(
                "https://zkillboard.com/constellation/{}/",
                s.constellation_id
            ),
        ),
        link(
            "icons/map.svg",
            "Region",
            format!("https://zkillboard.com/region/{}/", s.region_id),
        ),
    ]);
    rows
}

/// `WaypointSubmenus.svelte`: each of your characters online, and all of them.
fn waypoints(sigs: &Sigs, s: &crate::api::PlacedSystem, how: Waypoint) -> Vec<Entry> {
    let mine: Vec<&crate::api::Pilot> = sigs
        .nav
        .iter()
        .flat_map(|n| &n.pilots)
        .filter(|p| p.is_mine)
        .collect();
    if mine.is_empty() {
        return vec![Entry::Item {
            lead: Lead::None,
            label: "No characters online".into(),
            tail: Tail::None,
            act: None,
            danger: false,
        }];
    }
    let change = |character: Option<i64>, who: &str| {
        Act::Change(MapChange::Waypoint {
            character,
            destination: s.solar_system_id,
            how,
            done: if how == Waypoint::Destination {
                format!("{who} heading to {}", s.name)
            } else {
                format!("{} added to {who}'s route", s.name)
            },
        })
    };
    let mut rows: Vec<Entry> = mine
        .iter()
        .map(|p| {
            Entry::item(
                Lead::Portrait(p.character_id),
                p.name.clone(),
                change(Some(p.character_id), &p.name),
            )
        })
        .collect();
    if mine.len() > 1 {
        rows.push(Entry::Separator);
        rows.push(Entry::item(
            Lead::Icon("icons/users.svg"),
            "All Characters",
            change(None, "Everyone"),
        ));
    }
    rows
}

/// The size a hole's jump mass gives, `sizeForJumpMass`.
fn size_for_jump_mass(kg: f64) -> Option<&'static str> {
    if kg <= 0.0 {
        None
    } else if kg <= 5_000_000.0 {
        Some("small")
    } else if kg <= 62_000_000.0 {
        Some("medium")
    } else if kg <= 375_000_000.0 {
        Some("large")
    } else {
        Some("xl")
    }
}

/// `ConnectionMenu.svelte`.
fn connection(
    sigs: &Sigs,
    snapshot: &MapSnapshot,
    c: &Connection,
    bookmarks: Option<&[String]>,
) -> Vec<Entry> {
    let patch = |field: &str, value: serde_json::Value, done: String| {
        let mut body = json!({ "connection_id": c.id });
        body[field] = value;
        post("connections/set-status", body, done)
    };
    let check = |on: bool, hint: &'static str| match (on, hint.is_empty()) {
        (true, true) => Tail::Check,
        (true, false) => Tail::HintCheck(hint),
        (false, true) => Tail::None,
        (false, false) => Tail::Hint(hint),
    };
    let mut rows = Vec::new();
    if let Some(names) = bookmarks.filter(|b| !b.is_empty()) {
        rows.push(Entry::sub(
            Lead::Icon("icons/copy.svg"),
            "Copy name",
            260.0,
            names
                .iter()
                .map(|n| Entry::item(Lead::None, n.clone(), Act::Copy(n.clone())))
                .collect(),
        ));
    }

    rows.extend(degradation(c, &patch));
    rows.push(Entry::sub(
        Lead::Icon("icons/ship.svg"),
        "Ship Size",
        192.0,
        sizes(sigs, snapshot, c, &patch),
    ));

    let stargate = c.kind == ConnectionKind::Stargate;
    rows.push(Entry::sub(
        Lead::Icon("icons/waypoints.svg"),
        "Connection type",
        160.0,
        vec![
            Entry::item(
                Lead::None,
                "Wormhole",
                patch("kind", "wormhole".into(), "It is a wormhole".into()),
            )
            .with_tail(check(!stargate, "")),
            Entry::item(
                Lead::None,
                "Stargate",
                patch("kind", "stargate".into(), "It is a stargate".into()),
            )
            .with_tail(if stargate {
                Tail::Check
            } else {
                Tail::Icon("icons/triangle-alert.svg", look::amber_500())
            }),
        ],
    ));
    rows.push(Entry::Separator);
    rows.push(Entry::Item {
        lead: Lead::Icon("icons/trash.svg"),
        label: "Remove".into(),
        tail: Tail::None,
        act: Some(post(
            "connections/remove",
            json!({ "connection_id": c.id }),
            "Connection removed",
        )),
        danger: true,
    });
    rows
}

/// The Lifetime and Mass Status submenus, each state with its dot, threshold and a check
/// on the current one.
fn degradation(
    c: &Connection,
    patch: &dyn Fn(&str, serde_json::Value, String) -> Act,
) -> Vec<Entry> {
    let check = |on: bool, hint: &'static str| match (on, hint.is_empty()) {
        (true, true) => Tail::Check,
        (true, false) => Tail::HintCheck(hint),
        (false, true) => Tail::None,
        (false, false) => Tail::Hint(hint),
    };
    let mut rows = Vec::new();
    let lifetimes = [
        (
            TimeStatus::Stable,
            "stable",
            "Healthy",
            "",
            oklch(0.556, 0.007, 260.0),
        ),
        (
            TimeStatus::Eol,
            "eol",
            "End of Life",
            "< 4h",
            crate::theme::eol(),
        ),
        (
            TimeStatus::Critical,
            "critical",
            "Critical",
            "< 1h",
            crate::theme::critical(),
        ),
        (
            TimeStatus::Expired,
            "expired",
            "Expired",
            "imminent",
            crate::theme::critical(),
        ),
    ];
    let current = c.time_status.unwrap_or(TimeStatus::Stable);
    rows.push(Entry::sub(
        Lead::Icon("icons/clock.svg"),
        "Lifetime",
        192.0,
        lifetimes
            .into_iter()
            .map(|(status, value, label, hint, color)| {
                Entry::item(
                    Lead::Dot(color),
                    label,
                    patch("time_status", value.into(), format!("Lifetime: {label}")),
                )
                .with_tail(check(current == status, hint))
            })
            .collect(),
    ));

    let masses = [
        (
            MassStatus::Stable,
            "stable",
            "Fresh",
            "≥ 50%",
            oklch(0.556, 0.007, 260.0),
        ),
        (
            MassStatus::Reduced,
            "reduced",
            "Reduced",
            "< 50%",
            crate::theme::reduced(),
        ),
        (
            MassStatus::Critical,
            "critical",
            "Critical",
            "≤ 15%",
            crate::theme::critical(),
        ),
    ];
    let current = c.mass_status.unwrap_or(MassStatus::Stable);
    rows.push(Entry::sub(
        Lead::Icon("icons/weight.svg"),
        "Mass Status",
        192.0,
        masses
            .into_iter()
            .map(|(status, value, label, hint, color)| {
                Entry::item(
                    Lead::Dot(color),
                    label,
                    patch("mass_status", value.into(), format!("Mass: {label}")),
                )
                .with_tail(check(current == status, hint))
            })
            .collect(),
    ));

    rows
}

/// The Ship Size submenu: unknown and the four sizes, all fixed when a linked signature's
/// type already says.
fn sizes(
    sigs: &Sigs,
    snapshot: &MapSnapshot,
    c: &Connection,
    patch: &dyn Fn(&str, serde_json::Value, String) -> Act,
) -> Vec<Entry> {
    let locking = snapshot
        .signatures
        .iter()
        .filter(|s| s.connection_id == Some(c.id))
        .filter_map(|s| {
            sigs.catalog
                .types
                .iter()
                .find(|t| Some(t.id) == s.signature_type_id)
        })
        .find(|t| t.max_jump_mass.and_then(size_for_jump_mass).is_some());
    let mut rows = Vec::new();
    if let Some(t) = locking {
        let name = t.signature.clone().unwrap_or_else(|| t.name.clone());
        rows.push(Entry::Note(format!("Set by {name}")));
    }
    let options: [(Option<WormholeSize>, &'static str, &str, serde_json::Value); 5] = [
        (None, "?", "Unknown", serde_json::Value::Null),
        (Some(WormholeSize::Small), "S", "Frigate", "small".into()),
        (Some(WormholeSize::Medium), "M", "Medium", "medium".into()),
        (Some(WormholeSize::Large), "L", "Large", "large".into()),
        (Some(WormholeSize::Xl), "XL", "Extra Large", "xl".into()),
    ];
    rows.extend(options.into_iter().map(|(size, letter, label, value)| {
        Entry::Item {
            lead: Lead::Letter(letter),
            label: label.into(),
            tail: if c.size == size {
                Tail::Check
            } else {
                Tail::None
            },
            act: locking
                .is_none()
                .then(|| patch("size", value, format!("Size: {label}"))),
            danger: false,
        }
    }));
    rows
}

/// What nothing pinned or home can reach, `orphanedSystems`.
fn orphaned(snapshot: &MapSnapshot) -> Vec<i64> {
    let anchors: Vec<i64> = snapshot
        .systems
        .iter()
        .filter(|s| s.is_pinned || s.is_home)
        .map(|s| s.id)
        .chain(
            snapshot
                .ghosts
                .iter()
                .filter(|g| g.is_pinned || g.is_home)
                .map(|g| g.id),
        )
        .collect();
    if anchors.is_empty() {
        return Vec::new();
    }
    let mut reachable: std::collections::HashSet<i64> = anchors.iter().copied().collect();
    let mut queue = anchors;
    while let Some(at) = queue.pop() {
        for c in &snapshot.connections {
            let next = if c.from_system == at {
                c.to_system
            } else if c.to_system == at {
                c.from_system
            } else {
                continue;
            };
            if reachable.insert(next) {
                queue.push(next);
            }
        }
    }
    snapshot
        .systems
        .iter()
        .map(|s| s.id)
        .chain(snapshot.ghosts.iter().map(|g| g.id))
        .filter(|id| !reachable.contains(id))
        .collect()
}

/// `MapMenu.svelte`, less adding a system: cleaning and clearing, each asked about first.
fn canvas_menu(snapshot: &MapSnapshot) -> Vec<Entry> {
    let mut rows = Vec::new();
    let orphans = orphaned(snapshot);
    if !orphans.is_empty() {
        let n = orphans.len();
        rows.push(Entry::sub(
            Lead::Icon("icons/brush-cleaning.svg"),
            &format!("Clean map ({n})"),
            240.0,
            vec![
                Entry::Note("Nothing pinned or home reaches these.".into()),
                Entry::Item {
                    lead: Lead::Icon("icons/trash.svg"),
                    label: format!("Remove {n} system{}", if n == 1 { "" } else { "s" }),
                    tail: Tail::None,
                    act: Some(post(
                        "systems/remove",
                        json!({ "map_solar_system_ids": orphans }),
                        "Map cleaned",
                    )),
                    danger: true,
                },
            ],
        ));
    }
    rows.push(Entry::sub(
        Lead::Icon("icons/eraser.svg"),
        "Clear map",
        240.0,
        vec![
            Entry::Note("All systems except home and pinned ones.".into()),
            Entry::Item {
                lead: Lead::Icon("icons/eraser.svg"),
                label: "Clear map".into(),
                tail: Tail::None,
                act: Some(post("clear", json!({}), "Map cleared")),
                danger: true,
            },
        ],
    ));
    rows
}

/// The open menu, drawn above everything at the point it was opened.
pub(crate) fn render(
    entity: &Entity<Sigs>,
    sigs: &Sigs,
    view: &std::rc::Rc<std::cell::RefCell<super::View>>,
    theme: &Theme,
) -> Option<AnyElement> {
    let open = view.borrow().menu.clone()?;
    let snapshot = sigs.snapshot.as_deref()?;
    let rows = entries(sigs, snapshot, &open, theme);
    if rows.is_empty() {
        return None;
    }
    let sub = open.sub.and_then(|i| match rows.get(i) {
        Some(Entry::Sub { items, width, .. }) => {
            let offset: f32 = rows[..i].iter().map(Entry::height).sum();
            Some((offset, *width, items.clone()))
        }
        _ => None,
    });
    let ctx = Ctx {
        entity: entity.clone(),
        view: std::rc::Rc::clone(view),
        images: std::rc::Rc::clone(&sigs.images),
        theme: theme.clone(),
    };
    let measure = std::rc::Rc::clone(view);
    let body = div()
        .flex()
        .items_start()
        .occlude()
        .font_family(look::SANS)
        .child(ctx.panel("menu", &rows, 208.0, open.sub))
        .when_some(sub, |d, (offset, width, items)| {
            d.child(
                div()
                    .mt(px(offset))
                    .ml(px(-4.0))
                    .child(ctx.panel("submenu", &items, width, None)),
            )
        })
        .child(
            canvas(
                |_, _, _| {},
                move |bounds, (), window, _| {
                    let mut view = measure.borrow_mut();
                    if let Some(m) = view.menu.as_mut().filter(|m| m.bounds != Some(bounds)) {
                        // The overlay takes clicks where the menu is, which it learns here.
                        m.bounds = Some(bounds);
                        window.refresh();
                    }
                },
            )
            .absolute()
            .size_full(),
        );
    Some(
        deferred(
            anchored()
                .position(open.at)
                .snap_to_window_with_margin(px(8.0))
                .child(body),
        )
        .with_priority(1)
        .into_any_element(),
    )
}

/// What the rows need to draw and act.
struct Ctx {
    entity: Entity<Sigs>,
    view: std::rc::Rc<std::cell::RefCell<super::View>>,
    images: std::rc::Rc<look::Images>,
    theme: Theme,
}

impl Ctx {
    /// A dropdown's panel: `bg-popover`, a border, rounded, `p-1`, a shadow.
    fn panel(&self, key: &'static str, rows: &[Entry], width: f32, open: Option<usize>) -> Div {
        let t = &self.theme;
        div()
            .w(px(width))
            .p(px(4.0))
            .rounded(px(6.0))
            .border_1()
            .border_color(t.border)
            .bg(t.popover)
            .text_color(t.popover_foreground)
            .shadow_md()
            .flex()
            .flex_col()
            .children(
                rows.iter()
                    .enumerate()
                    .map(|(i, row)| self.row(key, i, row, open)),
            )
    }

    /// Row `i`, `open` naming the row whose submenu shows.
    fn row(&self, key: &'static str, i: usize, row: &Entry, open: Option<usize>) -> AnyElement {
        let t = &self.theme;
        match row {
            Entry::Separator => div()
                .h(px(9.0))
                .mx(px(-4.0))
                .flex()
                .items_center()
                .child(div().h(px(1.0)).w_full().bg(t.border))
                .into_any_element(),
            Entry::Label(text) => div()
                .h(px(28.0))
                .px(px(8.0))
                .flex()
                .items_center()
                .text_size(px(10.4))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(t.muted_foreground)
                .child(text.to_uppercase())
                .into_any_element(),
            Entry::Note(text) => div()
                .h(px(22.0))
                .px(px(8.0))
                .flex()
                .items_center()
                .text_size(px(10.0))
                .text_color(t.muted_foreground)
                .child(text.clone())
                .into_any_element(),
            Entry::Sub { lead, label, .. } => {
                let view = std::rc::Rc::clone(&self.view);
                self.line(key, i, lead, label)
                    .when(open == Some(i), |d| {
                        d.bg(t.accent).text_color(t.accent_foreground)
                    })
                    .on_hover(move |hovered, window, _| {
                        if *hovered {
                            if let Some(m) = view.borrow_mut().menu.as_mut() {
                                m.sub = Some(i);
                            }
                            window.refresh();
                        }
                    })
                    .child(
                        svg()
                            .ml_auto()
                            .flex_none()
                            .path("icons/chevron-right.svg")
                            .size(px(16.0))
                            .text_color(t.muted_foreground),
                    )
                    .into_any_element()
            }
            Entry::Item {
                lead,
                label,
                tail,
                act,
                danger,
            } => {
                let mut line = self
                    .line(key, i, lead, label)
                    .when(*danger, |d| d.text_color(t.danger))
                    .child(self.tail(tail));
                if key == "menu" {
                    let view = std::rc::Rc::clone(&self.view);
                    line = line.on_hover(move |hovered, window, _| {
                        if *hovered {
                            if let Some(m) = view.borrow_mut().menu.as_mut() {
                                m.sub = None;
                            }
                            window.refresh();
                        }
                    });
                }
                match act.clone() {
                    None => line.opacity(0.5).into_any_element(),
                    Some(act) => {
                        let entity = self.entity.clone();
                        let view = std::rc::Rc::clone(&self.view);
                        line.cursor_pointer()
                            .on_click(move |_, window, cx| {
                                view.borrow_mut().menu = None;
                                window.refresh();
                                match act.clone() {
                                    Act::Url(url) => cx.open_url(&url),
                                    Act::Copy(text) => {
                                        entity.update(cx, |_, cx| cx.emit(Command::Copy(text)))
                                    }
                                    Act::Change(change) => entity
                                        .update(cx, |_, cx| cx.emit(Command::ChangeMap(change))),
                                }
                            })
                            .into_any_element()
                    }
                }
            }
        }
    }

    /// A row: `px-2 py-1.5 text-sm`, its icon `size-4` muted, lit on hover.
    fn line(
        &self,
        key: &'static str,
        i: usize,
        lead: &Lead,
        label: &str,
    ) -> gpui_kit::Stateful<Div> {
        let t = &self.theme;
        div()
            .id(ElementId::from((key, i)))
            .h(px(32.0))
            .px(px(8.0))
            .flex()
            .items_center()
            .gap(px(8.0))
            .rounded(px(4.0))
            .text_size(px(14.0))
            .hover(|s| s.bg(t.accent).text_color(t.accent_foreground))
            .child(self.lead(lead))
            .child(div().min_w_0().truncate().child(label.to_owned()))
    }

    fn lead(&self, lead: &Lead) -> AnyElement {
        let t = &self.theme;
        match lead {
            Lead::None => div().into_any_element(),
            Lead::Icon(path) => svg()
                .flex_none()
                .path(*path)
                .size(px(16.0))
                .text_color(t.muted_foreground)
                .into_any_element(),
            Lead::Dot(color) => div()
                .flex_none()
                .size(px(8.0))
                .rounded_full()
                .bg(*color)
                .into_any_element(),
            Lead::Letter(letter) => div()
                .flex_none()
                .w(px(24.0))
                .flex()
                .justify_center()
                .font_family(look::MONO)
                .text_size(px(10.0))
                .text_color(t.muted_foreground)
                .child(*letter)
                .into_any_element(),
            Lead::Portrait(id) => div()
                .flex_none()
                .rounded(px(8.0))
                .overflow_hidden()
                .child(look::picture(&self.images, look::portrait(*id), 20.0, t))
                .into_any_element(),
        }
    }

    fn tail(&self, tail: &Tail) -> AnyElement {
        let t = &self.theme;
        let hint = |text: &'static str| div().text_color(t.muted_foreground).child(text);
        let check = || {
            svg()
                .flex_none()
                .path("icons/check.svg")
                .size(px(14.0))
                .text_color(t.popover_foreground)
        };
        let end = div()
            .ml_auto()
            .flex_none()
            .flex()
            .items_center()
            .gap(px(8.0));
        match tail {
            Tail::None => end.into_any_element(),
            Tail::Hint(text) => end.child(hint(text)).into_any_element(),
            Tail::Check => end.child(check()).into_any_element(),
            Tail::HintCheck(text) => end.child(hint(text)).child(check()).into_any_element(),
            Tail::Icon(path, color) => end
                .child(svg().path(*path).size(px(14.0)).text_color(*color))
                .into_any_element(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jump_mass_sets_the_size() {
        assert_eq!(size_for_jump_mass(5_000_000.0), Some("small"));
        assert_eq!(size_for_jump_mass(62_000_000.0), Some("medium"));
        assert_eq!(size_for_jump_mass(375_000_000.0), Some("large"));
        assert_eq!(size_for_jump_mass(1_000_000_000.0), Some("xl"));
        assert_eq!(size_for_jump_mass(0.0), None);
    }
}
