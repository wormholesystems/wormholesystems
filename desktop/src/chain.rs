//! The chain around you: the systems a few holes out from where you are, you in the middle
//! and each branch to the side the map has it on, the holes drawn in the colors of their
//! mass and lifetime. Click a system to set it as your destination.

use std::collections::{HashMap, HashSet, VecDeque};

use gpui_kit::component::{ActiveTheme as _, Theme};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, App, Bounds, ElementId, Entity, Hsla, InteractiveElement, IntoElement,
    ParentElement, PathBuilder, Pixels, StatefulInteractiveElement, Styled, canvas, div, point, px,
    relative,
};

use crate::api::{Connection, ConnectionKind, MapSnapshot, MassStatus, TimeStatus};
use crate::overlay::Floating;
use crate::sigs::{Command, Grab, Sigs};
use crate::theme;
use crate::ui::rpx;

/// How many holes out the card reaches, and how many systems it shows at most.
const DEPTH: usize = 3;
const MOST: usize = 24;
/// A node's size, in rems at the base font size.
const NODE_W: f32 = 80.0;
const NODE_H: f32 = 22.0;

/// A system or ghost on the card. Each has a slot, a column by how many holes out it is and
/// on which side, a row among the others there; `at` is the slot's center and `slot` its
/// size, both as fractions of the drawing area.
#[derive(Debug, Clone, PartialEq)]
struct Node {
    placement: i64,
    /// `None` for a hole nobody has flown.
    solar_system_id: Option<i64>,
    class: Option<i64>,
    security: Option<f64>,
    label: String,
    at: (f32, f32),
    slot: (f32, f32),
}

/// A placement the search reached: how many holes out, and through which of the center's
/// neighbours, the branch it hangs off.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Reached {
    id: i64,
    depth: usize,
    branch: i64,
}

/// The placements within `DEPTH` holes of `center`, nearest first, at most `MOST`.
fn around(snapshot: &MapSnapshot, center: i64) -> Vec<Reached> {
    let mut links: HashMap<i64, Vec<i64>> = HashMap::new();
    for c in &snapshot.connections {
        links.entry(c.from_system).or_default().push(c.to_system);
        links.entry(c.to_system).or_default().push(c.from_system);
    }
    let mut seen = HashSet::from([center]);
    let first = Reached {
        id: center,
        depth: 0,
        branch: center,
    };
    let mut order = vec![first];
    let mut queue = VecDeque::from([first]);
    while let Some(at) = queue.pop_front() {
        if at.depth == DEPTH {
            continue;
        }
        for &next in links.get(&at.id).into_iter().flatten() {
            if order.len() >= MOST {
                return order;
            }
            if seen.insert(next) {
                let reached = Reached {
                    id: next,
                    depth: at.depth + 1,
                    branch: if at.depth == 0 { next } else { at.branch },
                };
                order.push(reached);
                queue.push_back(reached);
            }
        }
    }
    order
}

/// A map coordinate, for telling left from right and ordering rows.
#[expect(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    reason = "map positions are a few thousand pixels at most"
)]
fn narrow(v: f64) -> f32 {
    v as f32
}

#[expect(
    clippy::as_conversions,
    clippy::cast_precision_loss,
    reason = "a handful of rows and columns"
)]
fn fraction(i: usize, of: usize) -> f32 {
    i as f32 / of as f32
}

/// A hole between two points of the drawing area, in its color.
type Line = ((f32, f32), (f32, f32), Hsla);

/// Name and place on the map of a placement, real or ghost.
fn describe(snapshot: &MapSnapshot, id: i64) -> Option<(Node, (f32, f32))> {
    let node = |solar_system_id, class, security, label| Node {
        placement: id,
        solar_system_id,
        class,
        security,
        label,
        at: (0.0, 0.0),
        slot: (0.0, 0.0),
    };
    if let Some(s) = snapshot.systems.iter().find(|s| s.id == id) {
        let label = s
            .alias
            .clone()
            .filter(|a| !a.trim().is_empty())
            .unwrap_or_else(|| s.name.clone());
        let n = node(
            Some(s.solar_system_id),
            s.wormhole_class_id,
            Some(s.security_status),
            label,
        );
        return Some((n, (narrow(s.position_x), narrow(s.position_y))));
    }
    let g = snapshot.ghosts.iter().find(|g| g.id == id)?;
    let n = node(None, None, None, g.alias.clone().unwrap_or_default());
    Some((n, (narrow(g.position_x), narrow(g.position_y))))
}

/// Lay the reached placements out in columns: the center in the middle, each branch to the
/// side the map has it on, one column per hole out; rows in the map's top-to-bottom order.
fn layout(snapshot: &MapSnapshot, reached: &[Reached]) -> Vec<Node> {
    let described: Vec<(Reached, Node, (f32, f32))> = reached
        .iter()
        .filter_map(|r| {
            let (node, pos) = describe(snapshot, r.id)?;
            Some((*r, node, pos))
        })
        .collect();
    let Some(center_x) = described.first().map(|(_, _, pos)| pos.0) else {
        return Vec::new();
    };
    let branch_x: HashMap<i64, f32> = described
        .iter()
        .filter(|(r, _, _)| r.depth == 1)
        .map(|(r, _, pos)| (r.id, pos.0))
        .collect();
    // Negative to the left of the center, positive to the right.
    let column = |r: &Reached| -> isize {
        let depth = isize::try_from(r.depth).unwrap_or(0);
        let left = branch_x.get(&r.branch).is_some_and(|&x| x < center_x);
        if left { -depth } else { depth }
    };
    let lo = described
        .iter()
        .map(|(r, _, _)| column(r))
        .min()
        .unwrap_or(0);
    let hi = described
        .iter()
        .map(|(r, _, _)| column(r))
        .max()
        .unwrap_or(0);
    let cols = usize::try_from(hi - lo + 1).unwrap_or(1);
    let mut by_column: Vec<Vec<(f32, Node)>> = vec![Vec::new(); cols];
    for (r, node, pos) in described {
        let c = usize::try_from(column(&r) - lo).unwrap_or(0);
        by_column[c].push((pos.1, node));
    }
    let mut out = Vec::new();
    for (c, mut column) in by_column.into_iter().enumerate() {
        column.sort_by(|a, b| a.0.total_cmp(&b.0));
        let rows = column.len();
        for (row, (_, mut node)) in column.into_iter().enumerate() {
            node.slot = (fraction(1, cols), fraction(1, rows));
            node.at = (
                fraction(2 * c + 1, 2 * cols),
                fraction(2 * row + 1, 2 * rows),
            );
            out.push(node);
        }
    }
    out
}

/// A hole's color: its mass first, then its lifetime.
fn hole_color(c: &Connection, theme: &Theme) -> Hsla {
    match (c.kind, c.mass_status, c.time_status) {
        (ConnectionKind::Stargate, _, _) => theme.border,
        (_, Some(MassStatus::Critical), _)
        | (_, _, Some(TimeStatus::Critical | TimeStatus::Expired)) => theme::critical(),
        (_, Some(MassStatus::Reduced), _) => theme::reduced(),
        (_, _, Some(TimeStatus::Eol)) => theme::eol(),
        _ => theme.muted_foreground,
    }
}

pub(crate) fn panel(entity: &Entity<Sigs>, grab: Option<Grab>, cx: &App) -> AnyElement {
    let theme = cx.theme().clone();
    let sigs = entity.read(cx);
    let snapshot = sigs.snapshot.as_ref();
    // Where you are, else the home system, so there is still a chain to look at.
    let here = sigs.system().map(|s| s.id);
    let center =
        here.or_else(|| snapshot.and_then(|s| s.systems.iter().find(|s| s.is_home).map(|s| s.id)));
    let nodes = match (snapshot, center) {
        (Some(snapshot), Some(center)) => layout(snapshot, &around(snapshot, center)),
        _ => Vec::new(),
    };
    let sub = match (snapshot, here, center) {
        (None, _, _) => "No map yet.".to_string(),
        (Some(_), Some(_), _) => format!("{} systems around you", nodes.len()),
        (Some(_), None, Some(_)) => "You are off the map; around home".into(),
        (Some(_), None, None) => "You are off the map.".into(),
    };

    let header = crate::card::header(
        entity,
        Floating::Chain,
        div().child("Chain"),
        sub,
        grab,
        &theme,
    );

    // Who is where, by solar system.
    let mut pilots: HashMap<i64, usize> = HashMap::new();
    for p in sigs.nav.iter().flat_map(|n| &n.pilots) {
        if let Some(id) = p.solar_system_id {
            *pilots.entry(id).or_default() += 1;
        }
    }

    let at: HashMap<i64, (f32, f32)> = nodes.iter().map(|n| (n.placement, n.at)).collect();
    let lines: Vec<Line> = snapshot
        .map(|s| s.connections.as_slice())
        .unwrap_or_default()
        .iter()
        .filter_map(|c| {
            Some((
                *at.get(&c.from_system)?,
                *at.get(&c.to_system)?,
                hole_color(c, &theme),
            ))
        })
        .collect();
    let lines = drawn(lines);
    let boxes = nodes.iter().map(|n| {
        let count = n.solar_system_id.and_then(|id| pilots.get(&id)).copied();
        node(entity, n, here, count, &theme)
    });

    div()
        .size_full()
        .flex()
        .flex_col()
        .child(header)
        .child(
            div()
                .flex_1()
                .min_h_0()
                .p_1()
                .child(div().relative().size_full().child(lines).children(boxes)),
        )
        .into_any_element()
}

/// The holes, as lines across the drawing area.
fn drawn(lines: Vec<Line>) -> impl IntoElement {
    canvas(
        |_, _, _| {},
        move |bounds: Bounds<Pixels>, (), window, _| {
            let to_px = |(x, y): (f32, f32)| {
                point(
                    bounds.origin.x + bounds.size.width * x,
                    bounds.origin.y + bounds.size.height * y,
                )
            };
            for &(a, b, color) in &lines {
                let mut path = PathBuilder::stroke(px(1.5));
                path.move_to(to_px(a));
                path.line_to(to_px(b));
                if let Ok(path) = path.build() {
                    window.paint_path(path, color);
                }
            }
        },
    )
    .absolute()
    .top_0()
    .left_0()
    .size_full()
}

/// A system's box, centered on its place: class, name, and how many pilots are there.
fn node(
    entity: &Entity<Sigs>,
    n: &Node,
    here: Option<i64>,
    count: Option<usize>,
    theme: &Theme,
) -> impl IntoElement {
    let (class, color) = theme::class_meta(n.class, n.security);
    let current = Some(n.placement) == here;
    let destination = n.solar_system_id;
    let entity = entity.clone();
    let slot = div()
        .absolute()
        .left(relative(n.at.0 - n.slot.0 / 2.0))
        .top(relative(n.at.1 - n.slot.1 / 2.0))
        .w(relative(n.slot.0))
        .h(relative(n.slot.1))
        .p(px(1.0))
        .flex()
        .items_center()
        .justify_center();
    let node = div()
        .id(ElementId::from((
            "chain-node",
            u64::try_from(n.placement).unwrap_or(0),
        )))
        .size_full()
        .max_w(rpx(NODE_W))
        .max_h(rpx(NODE_H))
        .px_1()
        .flex()
        .items_center()
        .gap_1()
        .text_xs()
        .overflow_hidden()
        .border_1()
        .bg(theme.background)
        .border_color(if current { theme.primary } else { theme.border })
        .when(current, |d| d.text_color(theme.primary))
        .when_some(destination, |d, id| {
            d.cursor_pointer()
                .hover(|s| s.border_color(theme.primary))
                .on_click(move |_, _, cx| {
                    entity.update(cx, |_, cx| cx.emit(Command::SetDestination(id)));
                })
        })
        .child(div().flex_none().text_color(color).child(class))
        .child(div().flex_1().min_w_0().truncate().child(n.label.clone()))
        .when_some(count, |d, count| {
            d.child(
                div()
                    .flex_none()
                    .px(rpx(3.0))
                    .bg(theme.primary.opacity(0.25))
                    .text_color(theme.foreground)
                    .child(count.to_string()),
            )
        });
    slot.child(node)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::{MapInfo, PlacedSystem};

    fn system(id: i64, x: f64, y: f64) -> PlacedSystem {
        PlacedSystem {
            id,
            solar_system_id: 31_000_000 + id,
            name: format!("J{id}"),
            position_x: x,
            position_y: y,
            ..Default::default()
        }
    }

    fn hole(id: i64, a: i64, b: i64) -> Connection {
        Connection {
            id,
            from_system: a,
            to_system: b,
            kind: ConnectionKind::Wormhole,
            mass_status: None,
            time_status: None,
            preserve_mass: false,
            created_at: None,
        }
    }

    /// 1-2-3-4-5 in a line, so 5 is four holes from 1.
    fn chain() -> MapSnapshot {
        MapSnapshot {
            map: MapInfo { name: "m".into() },
            systems: (1..=5)
                .map(|i| system(i, f64::from(u8::try_from(i).unwrap()) * 100.0, 50.0))
                .collect(),
            ghosts: Vec::new(),
            connections: (1..5).map(|i| hole(i, i, i + 1)).collect(),
            signatures: Vec::new(),
        }
    }

    fn ids(reached: &[Reached]) -> Vec<i64> {
        reached.iter().map(|r| r.id).collect()
    }

    #[test]
    fn reaches_three_holes_out() {
        assert_eq!(ids(&around(&chain(), 1)), vec![1, 2, 3, 4]);
        assert_eq!(ids(&around(&chain(), 3)), vec![3, 2, 4, 1, 5]);
    }

    #[test]
    fn puts_you_in_the_middle_and_each_branch_on_its_side() {
        let snapshot = chain();
        let nodes = layout(&snapshot, &around(&snapshot, 3));
        let at: HashMap<i64, (f32, f32)> = nodes.iter().map(|n| (n.placement, n.at)).collect();
        // Five columns: 1, 2 to the left, 3, then 4, 5 to the right.
        assert_eq!(at[&1], (0.1, 0.5));
        assert_eq!(at[&3], (0.5, 0.5));
        assert_eq!(at[&5], (0.9, 0.5));
        assert!(nodes.iter().all(|n| (n.slot.0 - 0.2).abs() < 1e-6));
    }
}
