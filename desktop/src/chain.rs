//! The chain around you, drawn the way the website's map draws it in its tree layout: the
//! systems a few holes out from where you are, you in the middle and each branch to the side
//! the map has it on, joined by elbows with rounded corners that fan out of a shared side,
//! dashed and colored by the hole's mass and lifetime, with the same badges. A port of the
//! node in `SystemNode.svelte` and the edges in `frontend/src/lib/map/edges.ts` and
//! `MapEdge.svelte`; change both together. Click a system to set it as your destination.

use std::collections::{HashMap, HashSet, VecDeque};
use std::rc::Rc;

use gpui_kit::component::{ActiveTheme as _, Theme};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, App, Bounds, Div, ElementId, Entity, FontWeight, Hsla, InteractiveElement,
    IntoElement, ParentElement, PathBuilder, Pixels, StatefulInteractiveElement, Styled, canvas,
    div, point, px, svg,
};

use crate::api::{
    Connection, ConnectionKind, MapSnapshot, MassStatus, Static, TimeStatus, WormholeSize,
};
use crate::look;
use crate::overlay::Floating;
use crate::sigs::{Command, Grab, Sigs};
use crate::theme;
use crate::ui::rpx;

/// How many holes out the card reaches, and how many systems it shows at most.
const DEPTH: usize = 3;
const MOST: usize = 24;
/// A node's largest size, in pixels at the base text size: the website's is 180 by 40.
const NODE_W: f32 = 160.0;
const NODE_H: f32 = 34.0;
/// A node with room for its first line only.
const NODE_H_LINE: f32 = 20.0;
/// What the website keeps between holes sharing a side, and between their bends.
const PARALLEL_SPACING: f32 = 14.0;
const CORNER_RADIUS: f32 = 10.0;
const LANE: f32 = 12.0;

/// A system or ghost on the card. Each has a slot, a column by how many holes out it is and
/// on which side, a row among the others there; `at` is the slot's center and `slot` its
/// size, both as fractions of the drawing area.
#[derive(Debug, Clone)]
struct Node {
    placement: i64,
    /// `None` for a hole nobody has flown.
    solar_system_id: Option<i64>,
    class: Option<i64>,
    security: Option<f64>,
    alias: Option<String>,
    name: Option<String>,
    region: String,
    statics: Vec<Static>,
    status: Option<String>,
    occupier: Option<String>,
    home: bool,
    column: usize,
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

/// A count as a length: rows, columns, holes, all a handful.
#[expect(
    clippy::as_conversions,
    clippy::cast_precision_loss,
    reason = "a handful of rows, columns and holes"
)]
fn float(i: usize) -> f32 {
    i as f32
}

fn fraction(i: usize, of: usize) -> f32 {
    float(i) / float(of)
}

/// What the card shows of a placement, real or ghost, and where the map has it.
fn describe(snapshot: &MapSnapshot, id: i64) -> Option<(Node, (f32, f32))> {
    let blank = Node {
        placement: id,
        solar_system_id: None,
        class: None,
        security: None,
        alias: None,
        name: None,
        region: String::new(),
        statics: Vec::new(),
        status: None,
        occupier: None,
        home: false,
        column: 0,
        at: (0.0, 0.0),
        slot: (0.0, 0.0),
    };
    let named = |a: &Option<String>| a.clone().filter(|a| !a.trim().is_empty());
    if let Some(s) = snapshot.systems.iter().find(|s| s.id == id) {
        let node = Node {
            solar_system_id: Some(s.solar_system_id),
            class: s.wormhole_class_id,
            security: Some(s.security_status),
            alias: named(&s.alias),
            name: Some(s.name.clone()),
            region: s.region.clone(),
            statics: s.statics.clone(),
            status: s.status.clone(),
            occupier: named(&s.occupying_group),
            home: s.is_home,
            ..blank
        };
        return Some((node, (narrow(s.position_x), narrow(s.position_y))));
    }
    let g = snapshot.ghosts.iter().find(|g| g.id == id)?;
    let node = Node {
        alias: named(&g.alias),
        ..blank
    };
    Some((node, (narrow(g.position_x), narrow(g.position_y))))
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
            node.column = c;
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

/// A box in pixels within the drawing area.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Rect {
    x: f32,
    y: f32,
    w: f32,
    h: f32,
}

impl Rect {
    fn right(&self) -> f32 {
        self.x + self.w
    }
    fn center_y(&self) -> f32 {
        self.y + self.h / 2.0
    }
}

/// Every node's box in an area of that size, `scale` the text size as a factor, and whether
/// the rows are too tight for the second line.
fn boxes(nodes: &[Node], area: (f32, f32), scale: f32) -> (HashMap<i64, Rect>, bool) {
    let pad = 2.0;
    let tight = nodes
        .iter()
        .any(|n| area.1 * n.slot.1 < NODE_H * scale + 2.0 * pad);
    let tall = if tight { NODE_H_LINE } else { NODE_H } * scale;
    let rects = nodes
        .iter()
        .map(|n| {
            let w = (NODE_W * scale).min(area.0 * n.slot.0 * 0.72);
            let h = tall.min(area.1 * n.slot.1 - 2.0 * pad).max(4.0);
            let (cx, cy) = (area.0 * n.at.0, area.1 * n.at.1);
            let rect = Rect {
                x: cx - w / 2.0,
                y: cy - h / 2.0,
                w,
                h,
            };
            (n.placement, rect)
        })
        .collect();
    (rects, tight)
}

/// A hole drawn as an elbow: its corners, its look, and where its badges hang.
#[derive(Debug, Clone, PartialEq)]
struct Elbow {
    connection: i64,
    points: Vec<(f32, f32)>,
    /// Where its badges hang: on the run into the end that shares its side with the fewest
    /// other holes, so the badges of a fan each get a row of their own.
    center: (f32, f32),
    /// How many holes share the side each end leaves from.
    crowd: [usize; 2],
}

/// One end of a hole on the side of a box it leaves from.
struct Port {
    edge: usize,
    /// Which end of the hole: 0 for its first point, 3 for its last.
    end: usize,
    /// The other end's height, which orders the ports so lines never cross.
    toward: f32,
}

/// Route the holes between the boxes as the website's tree layout does: out of facing sides,
/// a shared side fanned so each hole has its own port, the bends of a fan staggered so the
/// runs nest rather than lie on each other, and two boxes in one column joined round their
/// right sides.
fn route(
    nodes: &[Node],
    rects: &HashMap<i64, Rect>,
    connections: &[Connection],
    scale: f32,
) -> Vec<Elbow> {
    let column: HashMap<i64, usize> = nodes.iter().map(|n| (n.placement, n.column)).collect();
    let mut elbows = Vec::new();
    let mut sides: HashMap<(i64, bool), Vec<Port>> = HashMap::new();
    // Which way each hole leaves, for staggering its bend among the others of its fan.
    let mut fans: HashMap<(i64, bool), Vec<usize>> = HashMap::new();
    let mut detours = HashSet::new();
    for c in connections {
        let (Some(a), Some(b)) = (rects.get(&c.from_system), rects.get(&c.to_system)) else {
            continue;
        };
        let (ca, cb) = (column[&c.from_system], column[&c.to_system]);
        // Left end first.
        let ((left, lr, lc), (right, rr)) = if ca <= cb {
            ((c.from_system, a, ca), (c.to_system, b))
        } else {
            ((c.to_system, b, cb), (c.from_system, a))
        };
        let detour = lc == column[&right];
        let from = (lr.right(), lr.center_y());
        let to = if detour {
            (rr.right(), rr.center_y())
        } else {
            (rr.x, rr.center_y())
        };
        let i = elbows.len();
        elbows.push(Elbow {
            connection: c.id,
            points: vec![
                from,
                (f32::midpoint(from.0, to.0), from.1),
                (f32::midpoint(from.0, to.0), to.1),
                to,
            ],
            center: from,
            crowd: [1, 1],
        });
        sides.entry((left, true)).or_default().push(Port {
            edge: i,
            end: 0,
            toward: to.1,
        });
        sides.entry((right, detour)).or_default().push(Port {
            edge: i,
            end: 3,
            toward: from.1,
        });
        fans.entry((left, true)).or_default().push(i);
        if detour {
            detours.insert(i);
        }
    }
    // Fan the ports of a shared side, ordered by where their other ends are.
    for ((placement, _), mut ports) in sides {
        for port in &ports {
            elbows[port.edge].crowd[usize::from(port.end == 3)] = ports.len();
        }
        if ports.len() < 2 {
            continue;
        }
        let h = rects[&placement].h;
        let n = ports.len();
        let spacing = (PARALLEL_SPACING * h / 40.0).min(h * 0.7 / float(n - 1));
        ports.sort_by(|a, b| a.toward.total_cmp(&b.toward));
        for (i, port) in ports.iter().enumerate() {
            let offset = (float(i) - float(n - 1) / 2.0) * spacing;
            elbows[port.edge].points[port.end].1 += offset;
        }
    }
    stagger(&mut elbows, fans, &detours, scale);
    elbows
}

/// Stagger the bends of each fan, take the detours round the right, line the bends up with
/// the ports, and hang the badges.
fn stagger(
    elbows: &mut [Elbow],
    fans: HashMap<(i64, bool), Vec<usize>>,
    detours: &HashSet<usize>,
    scale: f32,
) {
    // Stagger the bends of a fan: going up, the highest bends first; going down, the lowest;
    // so each run stays clear of the next.
    for (_, fan) in fans {
        let (mut up, mut down): (Vec<usize>, Vec<usize>) = fan
            .into_iter()
            // A level hole runs straight across and has no bend to place.
            .filter(|&i| {
                !detours.contains(&i) && (elbows[i].points[3].1 - elbows[i].points[0].1).abs() > 0.5
            })
            .partition(|&i| elbows[i].points[3].1 < elbows[i].points[0].1);
        up.sort_by(|&a, &b| elbows[a].points[3].1.total_cmp(&elbows[b].points[3].1));
        down.sort_by(|&a, &b| elbows[b].points[3].1.total_cmp(&elbows[a].points[3].1));
        for group in [up, down] {
            let n = group.len();
            for (rank, i) in group.into_iter().enumerate() {
                let e = &mut elbows[i];
                let (x0, x1) = (e.points[0].0, e.points[3].0);
                let t = if n > 1 {
                    0.25 + 0.5 * float(rank) / float(n - 1)
                } else {
                    0.5
                };
                let bend = x0 + (x1 - x0) * t;
                e.points[1] = (bend, e.points[0].1);
                e.points[2] = (bend, e.points[3].1);
            }
        }
    }
    for &i in detours {
        let e = &mut elbows[i];
        let bend = e.points[0].0.max(e.points[3].0) + LANE * scale;
        e.points[1] = (bend, e.points[0].1);
        e.points[2] = (bend, e.points[3].1);
    }
    for e in elbows.iter_mut() {
        // The bends follow the ports, which the fanning moved.
        e.points[1].1 = e.points[0].1;
        e.points[2].1 = e.points[3].1;
        e.center = if e.crowd[1] <= e.crowd[0] {
            (f32::midpoint(e.points[2].0, e.points[3].0), e.points[3].1)
        } else {
            (f32::midpoint(e.points[0].0, e.points[1].0), e.points[0].1)
        };
    }
}

/// `edgeColor`: a stargate, then critical, reduced, end of life, else the plain edge.
fn edge_color(c: &Connection, theme: &Theme) -> Hsla {
    match (c.kind, c.mass_status, c.time_status) {
        (ConnectionKind::Stargate, _, _) => look::sky_500(),
        (_, Some(MassStatus::Critical), _)
        | (_, _, Some(TimeStatus::Critical | TimeStatus::Expired)) => theme::critical(),
        (_, Some(MassStatus::Reduced), _) => theme::reduced(),
        (_, _, Some(TimeStatus::Eol)) => theme::eol(),
        _ if theme.mode.is_dark() => crate::theme::oklch(0.371, 0.0, 0.0),
        _ => crate::theme::oklch(0.87, 0.0, 0.0),
    }
}

/// A degraded wormhole is dashed.
fn dashed(c: &Connection) -> bool {
    c.kind == ConnectionKind::Wormhole
        && (matches!(
            c.mass_status,
            Some(MassStatus::Reduced | MassStatus::Critical)
        ) || c.time_status.is_some_and(|t| t != TimeStatus::Stable))
}

/// The path through an elbow's corners, rounded.
fn elbow_path(
    points: &[(f32, f32)],
    radius: f32,
    origin: (Pixels, Pixels),
    dash: Option<f32>,
    width: f32,
) -> PathBuilder {
    let at = |(x, y): (f32, f32)| point(origin.0 + px(x), origin.1 + px(y));
    let mut path = PathBuilder::stroke(px(width));
    if let Some(unit) = dash {
        path = path.dash_array(&[px(2.0 * unit), px(6.0 * unit)]);
    }
    let pts: Vec<(f32, f32)> = points
        .iter()
        .enumerate()
        .filter(|&(i, p)| {
            i == 0 || {
                let q = points[i - 1];
                (p.0 - q.0).hypot(p.1 - q.1) > 0.01
            }
        })
        .map(|(_, p)| *p)
        .collect();
    let Some(&first) = pts.first() else {
        return path;
    };
    path.move_to(at(first));
    for i in 1..pts.len().saturating_sub(1) {
        let (prev, curr, next) = (pts[i - 1], pts[i], pts[i + 1]);
        let len_in = (curr.0 - prev.0).hypot(curr.1 - prev.1);
        let len_out = (next.0 - curr.0).hypot(next.1 - curr.1);
        let r = radius.min(len_in / 2.0).min(len_out / 2.0);
        let start = (
            curr.0 + (prev.0 - curr.0) / len_in * r,
            curr.1 + (prev.1 - curr.1) / len_in * r,
        );
        let end = (
            curr.0 + (next.0 - curr.0) / len_out * r,
            curr.1 + (next.1 - curr.1) / len_out * r,
        );
        path.line_to(at(start));
        path.curve_to(at(end), at(curr));
    }
    if let Some(&last) = pts.last() {
        path.line_to(at(last));
    }
    path
}

pub(crate) fn panel(entity: &Entity<Sigs>, grab: Option<Grab>, cx: &App) -> AnyElement {
    let theme = cx.theme().clone();
    let sigs = entity.read(cx);
    let snapshot = sigs.snapshot.as_deref();
    // Where you are, else the home system, so there is still a chain to look at.
    let here = sigs.system().map(|s| s.id);
    let center =
        here.or_else(|| snapshot.and_then(|s| s.systems.iter().find(|s| s.is_home).map(|s| s.id)));
    let nodes = match (snapshot, center) {
        (Some(snapshot), Some(center)) => layout(snapshot, &around(snapshot, center)),
        _ => Vec::new(),
    };
    let title = div()
        .flex()
        .items_center()
        .gap_2()
        .child("CHAIN")
        .when(!nodes.is_empty(), |d| d.child(look::count(nodes.len())))
        .when(here.is_none() && center.is_some(), |d| {
            d.child(
                div()
                    .text_color(theme.muted_foreground.opacity(0.6))
                    .child("around home"),
            )
        });
    let header = look::header(entity, Floating::Chain, title, None, grab, &theme);
    let body = div().flex_1().min_h_0().relative().overflow_hidden();
    let Some(snapshot) = snapshot.filter(|_| !nodes.is_empty()) else {
        return div()
            .size_full()
            .flex()
            .flex_col()
            .font_family(look::SANS)
            .child(header)
            .child(
                body.px(rpx(12.0))
                    .py(rpx(16.0))
                    .text_size(rpx(12.0))
                    .text_color(theme.muted_foreground)
                    .child(if snapshot.is_none() {
                        "No map yet."
                    } else {
                        "You are off the map, and it has no home system."
                    }),
            )
            .into_any_element();
    };

    // Laid out in pixels, in the area the last paint measured; the first frame guesses it
    // from the card's size.
    let scale = f32::from(sigs.overlay.text_size.max(1)) / 100.0;
    let area = sigs.chain_area.get().unwrap_or_else(|| {
        let (w, h) = Floating::Chain.place(sigs).dimensions();
        (w - 2.0, h - 38.0 * scale)
    });
    let (rects, tight) = boxes(&nodes, area, scale);
    let elbows = route(&nodes, &rects, &snapshot.connections, scale);
    let connections: HashMap<i64, &Connection> =
        snapshot.connections.iter().map(|c| (c.id, c)).collect();

    let tally = Tally::of(snapshot, sigs.nav.as_ref());
    let drawn = drawing(
        &elbows,
        &connections,
        Rc::clone(&sigs.chain_area),
        scale,
        &theme,
    );
    let badges = elbows.iter().filter_map(|e| {
        let c = connections.get(&e.connection)?;
        // Tight rows get a smaller pill, so neighbouring badges keep apart.
        badge(c, e.center, if tight { 11.0 } else { 14.0 }, &theme)
    });
    let boxes = nodes.iter().filter_map(|n| {
        let rect = *rects.get(&n.placement)?;
        let marks = Marks {
            tight,
            ..tally.marks(n, here)
        };
        Some(node(entity, n, rect, &marks, &theme))
    });

    div()
        .size_full()
        .flex()
        .flex_col()
        .font_family(look::SANS)
        .child(header)
        .child(body.child(drawn).children(badges).children(boxes))
        .into_any_element()
}

/// What the nodes count: pilots, signatures, and holes nobody has mapped yet.
struct Tally {
    pilots: HashMap<i64, usize>,
    /// Signatures, the uncategorised, and the wormholes, by solar system.
    signatures: HashMap<i64, (usize, usize, usize)>,
    /// Connections, by placement.
    holes: HashMap<i64, usize>,
}

impl Tally {
    fn of(snapshot: &MapSnapshot, nav: Option<&crate::nav::Nav>) -> Tally {
        let mut pilots: HashMap<i64, usize> = HashMap::new();
        for p in nav.iter().flat_map(|n| &n.pilots) {
            if let Some(id) = p.solar_system_id {
                *pilots.entry(id).or_default() += 1;
            }
        }
        let mut signatures: HashMap<i64, (usize, usize, usize)> = HashMap::new();
        for s in &snapshot.signatures {
            let e = signatures.entry(s.solar_system_id).or_default();
            e.0 += 1;
            if s.group.is_empty() || s.group == "unknown" {
                e.1 += 1;
            }
            if s.group == "wormhole" {
                e.2 += 1;
            }
        }
        let mut holes: HashMap<i64, usize> = HashMap::new();
        for c in &snapshot.connections {
            *holes.entry(c.from_system).or_default() += 1;
            *holes.entry(c.to_system).or_default() += 1;
        }
        Tally {
            pilots,
            signatures,
            holes,
        }
    }

    fn marks(&self, n: &Node, here: Option<i64>) -> Marks {
        let id = n.solar_system_id.unwrap_or(0);
        let counts = self.signatures.get(&id).copied().unwrap_or_default();
        Marks {
            current: Some(n.placement) == here,
            pilots: self.pilots.get(&id).copied().unwrap_or(0),
            signatures: (counts.0, counts.1),
            unmapped: counts
                .2
                .saturating_sub(self.holes.get(&n.placement).copied().unwrap_or(0)),
            tight: false,
        }
    }
}

/// A hole's corners, color, and whether it dashes.
type Line = (Vec<(f32, f32)>, Hsla, bool);

/// The holes, drawn under the nodes. Measures the area as it paints, and lays the card out
/// again when that changed.
fn drawing(
    elbows: &[Elbow],
    connections: &HashMap<i64, &Connection>,
    measured: Rc<std::cell::Cell<Option<(f32, f32)>>>,
    scale: f32,
    theme: &Theme,
) -> impl IntoElement {
    let lines: Vec<Line> = elbows
        .iter()
        .filter_map(|e| {
            let c = connections.get(&e.connection)?;
            Some((e.points.clone(), edge_color(c, theme), dashed(c)))
        })
        .collect();
    canvas(
        |_, _, _| {},
        move |bounds: Bounds<Pixels>, (), window, _| {
            let size = (f32::from(bounds.size.width), f32::from(bounds.size.height));
            let moved = measured
                .get()
                .is_none_or(|m| (m.0 - size.0).abs() > 0.5 || (m.1 - size.1).abs() > 0.5);
            if moved {
                measured.set(Some(size));
                window.refresh();
            }
            for (points, color, dashed) in &lines {
                let dash = dashed.then_some(scale);
                let path = elbow_path(
                    points,
                    CORNER_RADIUS * scale,
                    (bounds.origin.x, bounds.origin.y),
                    dash,
                    1.5,
                );
                if let Ok(path) = path.build() {
                    window.paint_path(path, *color);
                }
            }
        },
    )
    .absolute()
    .top_0()
    .left_0()
    .size_full()
}

/// The badge pill on a hole: a stargate's orbit, the size, the mass and the lifetime.
fn badge(c: &Connection, at: (f32, f32), glyph: f32, theme: &Theme) -> Option<AnyElement> {
    let mass = match c.mass_status {
        Some(MassStatus::Reduced) => Some(theme::reduced()),
        Some(MassStatus::Critical) => Some(theme::critical()),
        _ => None,
    };
    let time = match c.time_status {
        Some(TimeStatus::Eol) => Some(theme::eol()),
        Some(TimeStatus::Critical | TimeStatus::Expired) => Some(theme::critical()),
        _ => None,
    };
    let size = (c.kind == ConnectionKind::Wormhole)
        .then_some(c.size)
        .flatten()
        .map(|s| match s {
            WormholeSize::Small => "S",
            WormholeSize::Medium => "M",
            WormholeSize::Large => "L",
            WormholeSize::Xl => "XL",
        });
    let gate = c.kind == ConnectionKind::Stargate;
    let count = usize::from(gate)
        + usize::from(size.is_some())
        + usize::from(mass.is_some())
        + usize::from(time.is_some());
    if count == 0 {
        return None;
    }
    let tall = glyph + 6.0;
    let width = float(count) * (glyph + 4.0) + 8.0;
    let icon = |path: &'static str, color: Hsla| svg().path(path).size(px(glyph)).text_color(color);
    let pill = div()
        .absolute()
        .left(px(at.0 - width / 2.0))
        .top(px(at.1 - tall / 2.0))
        .w(px(width))
        .h(px(tall))
        .flex()
        .items_center()
        .justify_center()
        .gap(px(2.0))
        .px_1()
        .rounded_full()
        .border_1()
        .border_color(if theme.mode.is_dark() {
            crate::theme::oklch(0.371, 0.0, 0.0)
        } else {
            crate::theme::oklch(0.87, 0.0, 0.0)
        })
        .bg(if theme.mode.is_dark() {
            crate::theme::oklch(0.205, 0.0, 0.0)
        } else {
            crate::theme::oklch(1.0, 0.0, 0.0)
        })
        .when(gate, |d| d.child(icon("icons/orbit.svg", look::sky_500())))
        .when_some(size, |d, s| {
            d.child(
                div()
                    .text_size(px(glyph - 1.0))
                    .font_weight(FontWeight::BOLD)
                    .text_color(crate::theme::oklch(0.556, 0.0, 0.0))
                    .child(s),
            )
        })
        .when_some(mass, |d, color| d.child(icon("icons/weight.svg", color)))
        .when_some(time, |d, color| d.child(icon("icons/clock.svg", color)));
    Some(pill.into_any_element())
}

/// What a node shows beyond its system.
struct Marks {
    current: bool,
    pilots: usize,
    /// Signatures, and how many are not categorised yet.
    signatures: (usize, usize),
    unmapped: usize,
    /// No room for the second line.
    tight: bool,
}

/// The status icon the website puts in a node's header.
fn status_icon(status: &str) -> Option<&'static str> {
    match status {
        "friendly" => Some("icons/shield-check.svg"),
        "hostile" => Some("icons/skull.svg"),
        "active" => Some("icons/activity.svg"),
        "unscanned" => Some("icons/radar.svg"),
        "empty" => Some("icons/circle-dashed.svg"),
        _ => None,
    }
}

/// A node as the website draws one: its status in the border, class and name, the icons on
/// the right, and the statics or the region under them.
fn node(entity: &Entity<Sigs>, n: &Node, r: Rect, marks: &Marks, theme: &Theme) -> AnyElement {
    let status = look::status_color(n.status.as_deref(), theme);
    let entity = entity.clone();
    let destination = n.solar_system_id;
    div()
        .id(ElementId::from((
            "chain-node",
            u64::try_from(n.placement).unwrap_or(0),
        )))
        .absolute()
        .left(px(r.x))
        .top(px(r.y))
        .w(px(r.w))
        .h(px(r.h))
        .flex()
        .flex_col()
        .justify_center()
        .px(rpx(8.0))
        .py(rpx(2.0))
        .rounded(px(4.0))
        .border_1()
        .border_color(status)
        .when(n.solar_system_id.is_none(), Styled::border_dashed)
        .bg(look::card(theme))
        .shadow_sm()
        .text_size(rpx(11.0))
        .line_height(rpx(13.0))
        .hover(|s| s.border_color(crate::theme::oklch(0.795, 0.184, 86.047)))
        .when(marks.current, |d| {
            // The website's amber ring round the system you are on.
            d.child(
                div()
                    .absolute()
                    .top(px(-5.0))
                    .left(px(-5.0))
                    .right(px(-5.0))
                    .bottom(px(-5.0))
                    .rounded(px(7.0))
                    .border_2()
                    .border_color(look::amber_500()),
            )
        })
        .when_some(destination, |d, id| {
            d.cursor_pointer().on_click(move |_, _, cx| {
                entity.update(cx, |_, cx| cx.emit(Command::SetDestination(id)));
            })
        })
        .child(
            div()
                .flex()
                .min_w_0()
                .items_center()
                .gap_1()
                .child(look::class_badge(n.class, n.security).font_weight(FontWeight::MEDIUM))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .gap_1()
                        .child(node_name(n, theme))
                        .when_some(n.occupier.clone(), |d, o| {
                            d.child(
                                div()
                                    .truncate()
                                    .text_color(theme.muted_foreground)
                                    .child(format!("({o})")),
                            )
                        }),
                )
                .child(node_icons(n, marks, status)),
        )
        .when(!marks.tight, |d| d.child(node_second(n, marks, theme)))
        .into_any_element()
}

fn icon(path: &'static str, color: Hsla) -> gpui_kit::Svg {
    svg()
        .flex_none()
        .path(path)
        .size(rpx(14.0))
        .text_color(color)
}

/// The icons on the right of a node's first line: status, home, signatures, unmapped holes.
fn node_icons(n: &Node, marks: &Marks, status: Hsla) -> Div {
    let mut icons = div().ml_auto().flex_none().flex().items_center().gap_1();
    if let Some(path) = n.status.as_deref().and_then(status_icon) {
        icons = icons.child(icon(path, status));
    }
    if n.home {
        icons = icons.child(icon("icons/house.svg", look::amber_400()));
    }
    if marks.signatures.0 > 0 {
        let color = if marks.signatures.1 > 0 {
            crate::theme::oklch(0.645, 0.246, 16.439)
        } else {
            look::amber_500()
        };
        icons = icons.child(icon("icons/satellite.svg", color));
    }
    if marks.unmapped > 0 {
        icons = icons.child(icon("icons/fan.svg", look::sky_500()));
    }
    icons
}

/// The alias with the name after it, or the name alone, or that nobody has been through.
fn node_name(n: &Node, theme: &Theme) -> Div {
    match (&n.alias, &n.name) {
        (Some(alias), name) => div()
            .flex()
            .min_w_0()
            .gap_1()
            .child(
                div()
                    .flex_none()
                    .font_weight(FontWeight::MEDIUM)
                    .child(alias.clone()),
            )
            .child(
                div()
                    .truncate()
                    .text_color(theme.muted_foreground)
                    .child(name.clone().unwrap_or_else(|| "Unmapped".into())),
            ),
        (None, Some(name)) => div()
            .truncate()
            .font_weight(FontWeight::MEDIUM)
            .child(name.clone()),
        (None, None) => div()
            .truncate()
            .font_weight(FontWeight::MEDIUM)
            .italic()
            .text_color(theme.muted_foreground)
            .child("Unmapped"),
    }
}

/// A node's second line: who is there, and the statics, or the region in known space.
fn node_second(n: &Node, marks: &Marks, theme: &Theme) -> Div {
    let wormhole = n.class.is_some_and(|c| !matches!(c, 7..=9 | 25));
    let rest = if wormhole && !n.statics.is_empty() {
        div()
            .flex()
            .items_center()
            .justify_end()
            .gap(rpx(6.0))
            .children(n.statics.iter().map(|s| {
                let (dest, color) = crate::theme::dest_class_meta(s.dest_class);
                div()
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(color)
                    .child(dest)
            }))
    } else {
        div()
            .truncate()
            .text_color(theme.muted_foreground)
            .child(n.region.clone())
    };
    div()
        .flex()
        .items_center()
        .gap_1()
        .text_size(rpx(10.0))
        .when(marks.pilots > 0, |d| {
            d.child(icon("icons/users.svg", look::green_500()).size(rpx(10.0)))
                .child(
                    div()
                        .text_color(look::green_500())
                        .child(marks.pilots.to_string()),
                )
        })
        .child(div().flex_1().min_w_0().child(rest))
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
            size: None,
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

    /// A hub with three holes out to one column: they fan out of its right side in their
    /// order, and the bends nest so no two vertical runs share a line.
    #[test]
    fn a_fan_gets_its_own_ports_and_nested_bends() {
        let mut snapshot = chain();
        snapshot.systems = vec![
            system(1, 0.0, 100.0),
            system(2, 300.0, 0.0),
            system(3, 300.0, 100.0),
            system(4, 300.0, 200.0),
        ];
        snapshot.connections = vec![hole(1, 1, 2), hole(2, 1, 3), hole(3, 1, 4)];
        let nodes = layout(&snapshot, &around(&snapshot, 1));
        let (rects, _) = boxes(&nodes, (400.0, 300.0), 1.0);
        let elbows = route(&nodes, &rects, &snapshot.connections, 1.0);
        let starts: Vec<f32> = elbows.iter().map(|e| e.points[0].1).collect();
        assert!(starts[0] < starts[1] && starts[1] < starts[2], "{starts:?}");
        let up = &elbows[0];
        let down = &elbows[2];
        assert!(
            (up.points[1].0 - down.points[1].0).abs() < 1e-3,
            "one each way, both first"
        );
        assert!(
            elbows
                .iter()
                .all(|e| e.points[0].0 < e.points[1].0 && e.points[1].0 < e.points[3].0)
        );
    }
}
