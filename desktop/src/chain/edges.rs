//! Where the connections run, in world units: a port of `frontend/src/lib/map/edges.ts`,
//! `treeEdges` for the automatic layout and `freeEdges` for the dragged one. Change both
//! together.

use std::collections::{HashMap, HashSet};

/// A point in world units.
pub(crate) type P = (f32, f32);

/// Fixed node width, world units; the height is two grid cells.
pub(crate) const NODE_W: f32 = 180.0;
const PARALLEL_SPACING: f32 = 14.0;
const BEND_SPACING: f32 = 16.0;
pub(crate) const CORNER_RADIUS: f32 = 10.0;
const LANE_MARGIN: f32 = 40.0;
const RAIL_PADDING: f32 = 40.0;

/// A connection as the router needs it.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Link {
    pub id: i64,
    pub from: i64,
    pub to: i64,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Shape {
    /// Corners of an elbow, rounded when drawn.
    Elbow([P; 4]),
    /// A curve between two rail ends, each with a dot.
    Curve(P, P),
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Geometry {
    pub id: i64,
    pub shape: Shape,
    /// Where the badge cluster hangs.
    pub center: P,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct Rect {
    min_x: f32,
    min_y: f32,
    max_x: f32,
    max_y: f32,
    center_x: f32,
    center_y: f32,
}

fn rect_at(p: P, node_h: f32) -> Rect {
    Rect {
        min_x: p.0,
        min_y: p.1,
        max_x: p.0 + NODE_W,
        max_y: p.1 + node_h,
        center_x: p.0 + NODE_W / 2.0,
        center_y: p.1 + node_h / 2.0,
    }
}

fn midpoint(a: P, b: P) -> P {
    (f32::midpoint(a.0, b.0), f32::midpoint(a.1, b.1))
}

#[expect(
    clippy::as_conversions,
    clippy::cast_precision_loss,
    reason = "a handful of holes"
)]
fn float(i: usize) -> f32 {
    i as f32
}

/// How far each of several holes between one pair is shifted off its rail.
fn parallel_offsets(links: &[Link], node_h: f32) -> HashMap<i64, f32> {
    let mut groups: Vec<((i64, i64), Vec<i64>)> = Vec::new();
    for l in links {
        let key = (l.from.min(l.to), l.from.max(l.to));
        match groups.iter_mut().find(|(k, _)| *k == key) {
            Some((_, ids)) => ids.push(l.id),
            None => groups.push((key, vec![l.id])),
        }
    }
    let mut offsets = HashMap::new();
    for (_, ids) in groups {
        if ids.len() < 2 {
            continue;
        }
        let n = ids.len();
        let spacing = PARALLEL_SPACING.min(node_h * 0.7 / float(n - 1));
        for (i, id) in ids.into_iter().enumerate() {
            offsets.insert(id, (float(i) - float(n - 1) / 2.0) * spacing);
        }
    }
    offsets
}

fn rail_endpoint(min_x: f32, max_x: f32, center_y: f32, toward_x: f32) -> P {
    let padding = RAIL_PADDING.min((max_x - min_x) / 2.0);
    (toward_x.clamp(min_x + padding, max_x - padding), center_y)
}

/// `freeEdges`: each end slides along a rail through its node's middle, toward the other.
pub(crate) fn free(links: &[Link], positions: &HashMap<i64, P>, node_h: f32) -> Vec<Geometry> {
    let offsets = parallel_offsets(links, node_h);
    links
        .iter()
        .filter_map(|l| {
            let a = positions.get(&l.from)?;
            let b = positions.get(&l.to)?;
            let rail = node_h / 2.0 + offsets.get(&l.id).copied().unwrap_or(0.0);
            let from = rail_endpoint(a.0, a.0 + NODE_W, a.1 + rail, b.0 + NODE_W / 2.0);
            let to = rail_endpoint(b.0, b.0 + NODE_W, b.1 + rail, a.0 + NODE_W / 2.0);
            Some(Geometry {
                id: l.id,
                shape: Shape::Curve(from, to),
                center: midpoint(from, to),
            })
        })
        .collect()
}

/// The two control points of `curveBetween`.
pub(crate) fn curve_controls(from: P, to: P) -> (P, P) {
    (
        (from.0 + (to.0 - from.0) / 1.5, from.1),
        (to.0 - (to.0 - from.0) / 1.5, to.1),
    )
}

#[derive(Debug, Clone)]
struct Routed {
    id: i64,
    /// `ends[0]` is where it leaves, `ends[1]` where it lands.
    ends: [P; 2],
    normals: [P; 2],
    source: Rect,
    target: Rect,
    bend: Option<f32>,
    detour: bool,
    distance: f32,
    signed: f32,
}

impl Routed {
    fn horizontal(&self) -> bool {
        self.normals[0].0 != 0.0
    }
}

/// Both ends out of the right side, round the nodes between two in one column.
fn detour_ends(source: &Rect, target: &Rect) -> ([P; 2], [P; 2]) {
    (
        [
            (source.max_x, source.center_y),
            (target.max_x, target.center_y),
        ],
        [(1.0, 0.0), (1.0, 0.0)],
    )
}

/// Left and right sides whenever the boxes are apart horizontally, so the long run drops
/// through the lane between columns.
fn facing_ends(source: &Rect, target: &Rect) -> ([P; 2], [P; 2]) {
    let dx = target.center_x - source.center_x;
    let dy = target.center_y - source.center_y;
    let separated_x = target.min_x > source.max_x || source.min_x > target.max_x;
    let separated_y = target.min_y > source.max_y || source.min_y > target.max_y;
    if separated_x || (!separated_y && dx.abs() >= dy.abs()) {
        let rightward = dx >= 0.0;
        let s = if rightward { 1.0 } else { -1.0 };
        return (
            [
                (
                    if rightward {
                        source.max_x
                    } else {
                        source.min_x
                    },
                    source.center_y,
                ),
                (
                    if rightward {
                        target.min_x
                    } else {
                        target.max_x
                    },
                    target.center_y,
                ),
            ],
            [(s, 0.0), (-s, 0.0)],
        );
    }
    let downward = dy >= 0.0;
    let s = if downward { 1.0 } else { -1.0 };
    (
        [
            (
                source.center_x,
                if downward { source.max_y } else { source.min_y },
            ),
            (
                target.center_x,
                if downward { target.min_y } else { target.max_y },
            ),
        ],
        [(0.0, s), (0.0, -s)],
    )
}

fn blocked_in_column(source: &Rect, target: &Rect, column: &[Rect]) -> bool {
    let top = source.center_y.min(target.center_y);
    let bottom = source.center_y.max(target.center_y);
    column
        .iter()
        .any(|o| o != source && o != target && o.min_y < bottom && o.max_y > top)
}

fn into_lane(x: f32, columns: &[f32], from: f32, to: f32) -> f32 {
    let near = from.min(to);
    let far = from.max(to);
    for &left in columns {
        if left + NODE_W <= near || left >= far {
            continue;
        }
        if x > left - LANE_MARGIN / 2.0 && x < left + NODE_W + LANE_MARGIN {
            return (left + NODE_W + LANE_MARGIN).clamp(near, far);
        }
    }
    x
}

/// Lane positions honouring `precedes`, keeping the original order where they leave a choice.
fn lane_order(precedes: &[HashSet<usize>]) -> Vec<usize> {
    let mut indegree = vec![0usize; precedes.len()];
    for targets in precedes {
        for &t in targets {
            indegree[t] += 1;
        }
    }
    let mut position = vec![0usize; precedes.len()];
    let mut placed = vec![false; precedes.len()];
    for slot in 0..precedes.len() {
        let mut pick = None;
        for lane in 0..precedes.len() {
            if placed[lane] {
                continue;
            }
            if indegree[lane] == 0 {
                pick = Some(lane);
                break;
            }
            if pick.is_none() {
                pick = Some(lane);
            }
        }
        let Some(pick) = pick else { break };
        placed[pick] = true;
        position[pick] = slot;
        for &t in &precedes[pick] {
            if !placed[t] {
                indegree[t] = indegree[t].saturating_sub(1);
            }
        }
    }
    position
}

fn elbow_corners(e: &Routed) -> [P; 2] {
    let [from, to] = e.ends;
    if e.horizontal() {
        let mid = e.bend.unwrap_or_else(|| f32::midpoint(from.0, to.0));
        [(mid, from.1), (mid, to.1)]
    } else {
        let mid = e.bend.unwrap_or_else(|| f32::midpoint(from.1, to.1));
        [(from.0, mid), (to.0, mid)]
    }
}

fn bits(v: f32) -> u32 {
    // -0 and 0 are the same place.
    (v + 0.0).to_bits()
}

/// A node side: its box's center and the side's normal, as bits.
type SideKey = (u32, u32, u32, u32);

/// One end of a routed edge on a shared node side.
struct Port {
    edge: usize,
    end: usize,
    sort_key: f32,
}

/// `treeEdges`: elbows out of facing sides, shared sides fanned, the runs of a corridor
/// packed onto as few lanes as keep them apart.
pub(crate) fn tree(links: &[Link], positions: &HashMap<i64, P>, node_h: f32) -> Vec<Geometry> {
    let mut rects: HashMap<i64, Rect> = HashMap::new();
    let mut by_column: HashMap<u32, Vec<Rect>> = HashMap::new();
    for (&id, &p) in positions {
        let rect = rect_at(p, node_h);
        rects.insert(id, rect);
        by_column.entry(bits(rect.min_x)).or_default().push(rect);
    }
    let mut columns: Vec<f32> = by_column.keys().map(|&b| f32::from_bits(b)).collect();
    columns.sort_by(f32::total_cmp);

    let mut routed: Vec<Routed> = Vec::new();
    for l in links {
        let (Some(&source), Some(&target)) = (rects.get(&l.from), rects.get(&l.to)) else {
            continue;
        };
        let detour = bits(source.min_x) == bits(target.min_x)
            && blocked_in_column(
                &source,
                &target,
                by_column
                    .get(&bits(source.min_x))
                    .map_or(&[][..], Vec::as_slice),
            );
        let (ends, normals) = if detour {
            detour_ends(&source, &target)
        } else {
            facing_ends(&source, &target)
        };
        routed.push(Routed {
            id: l.id,
            ends,
            normals,
            source,
            target,
            bend: None,
            detour,
            distance: 0.0,
            signed: 0.0,
        });
    }

    spread_ports(&mut routed);
    pack_corridors(&mut routed);

    // Detours stack outwards so several between the same roots stay apart.
    let mut detours: Vec<(u32, Vec<usize>)> = Vec::new();
    for (i, e) in routed.iter().enumerate().filter(|(_, e)| e.detour) {
        let column = bits(e.source.min_x);
        match detours.iter_mut().find(|(x, _)| *x == column) {
            Some((_, group)) => group.push(i),
            None => detours.push((column, vec![i])),
        }
    }
    for (column, mut group) in detours {
        let column_x = f32::from_bits(column);
        group.sort_by(|&a, &b| routed[a].ends[0].1.total_cmp(&routed[b].ends[0].1));
        for (i, e) in group.into_iter().enumerate() {
            routed[e].bend = Some(column_x + NODE_W + LANE_MARGIN + float(i) * BEND_SPACING);
        }
    }

    for e in &mut routed {
        if e.detour || !e.horizontal() {
            continue;
        }
        let x = e
            .bend
            .unwrap_or_else(|| f32::midpoint(e.ends[0].0, e.ends[1].0));
        e.bend = Some(into_lane(x, &columns, e.ends[0].0, e.ends[1].0));
    }

    routed
        .iter()
        .map(|e| {
            let corners = elbow_corners(e);
            Geometry {
                id: e.id,
                shape: Shape::Elbow([e.ends[0], corners[0], corners[1], e.ends[1]]),
                center: midpoint(corners[0], corners[1]),
            }
        })
        .collect()
}

/// Spread the ends sharing a node side along it, ordered by the far end so lines never
/// cross; then let a quiet end of a level edge follow its busy one.
fn spread_ports(routed: &mut [Routed]) {
    let mut shared: Vec<(SideKey, Vec<Port>)> = Vec::new();
    let mut register = |key: SideKey, port: Port| match shared.iter_mut().find(|(k, _)| *k == key) {
        Some((_, ports)) => ports.push(port),
        None => shared.push((key, vec![port])),
    };
    for (i, e) in routed.iter().enumerate() {
        for end in 0..2 {
            let (own, other) = if end == 0 {
                (e.source, e.target)
            } else {
                (e.target, e.source)
            };
            let normal = e.normals[end];
            let key = (
                bits(own.center_x),
                bits(own.center_y),
                bits(normal.0),
                bits(normal.1),
            );
            let sort_key = if normal.0 == 0.0 {
                other.center_x
            } else {
                other.center_y
            };
            register(
                key,
                Port {
                    edge: i,
                    end,
                    sort_key,
                },
            );
        }
    }
    let mut spread_count: HashMap<(usize, usize), usize> = HashMap::new();
    for (_, mut ports) in shared {
        let n = ports.len();
        for p in &ports {
            spread_count.insert((p.edge, p.end), n);
        }
        if n < 2 {
            continue;
        }
        let first = &routed[ports[0].edge];
        let along_y = first.normals[ports[0].end].0 != 0.0;
        let own = if ports[0].end == 0 {
            first.source
        } else {
            first.target
        };
        let extent = if along_y {
            own.max_y - own.min_y
        } else {
            own.max_x - own.min_x
        };
        let spacing = PARALLEL_SPACING.min(extent * 0.7 / float(n - 1));
        ports.sort_by(|a, b| a.sort_key.total_cmp(&b.sort_key));
        for (i, p) in ports.iter().enumerate() {
            let offset = (float(i) - float(n - 1) / 2.0) * spacing;
            let end = &mut routed[p.edge].ends[p.end];
            if along_y {
                end.1 += offset;
            } else {
                end.0 += offset;
            }
        }
    }

    for (i, e) in routed.iter_mut().enumerate() {
        if e.detour {
            continue;
        }
        let horizontal = e.horizontal();
        let level = if horizontal {
            (e.source.center_y - e.target.center_y).abs() < 0.5
        } else {
            (e.source.center_x - e.target.center_x).abs() < 0.5
        };
        if !level {
            continue;
        }
        let from_shared = spread_count.get(&(i, 0)).copied().unwrap_or(1);
        let to_shared = spread_count.get(&(i, 1)).copied().unwrap_or(1);
        if from_shared == to_shared {
            continue;
        }
        let (follow, lead) = if from_shared < to_shared {
            (0, 1)
        } else {
            (1, 0)
        };
        if horizontal {
            e.ends[follow].1 = e.ends[lead].1;
        } else {
            e.ends[follow].0 = e.ends[lead].0;
        }
    }
}

/// Group the runs by the corridor they cross and pack each corridor onto as few lanes as
/// keep overlapping runs apart, ordered so no run doubles back over a level neighbour's tail.
fn pack_corridors(routed: &mut [Routed]) {
    let mut fans: Vec<((bool, u32, u32), Vec<usize>)> = Vec::new();
    for (i, e) in routed.iter_mut().enumerate() {
        if e.detour {
            continue;
        }
        let horizontal = e.horizontal();
        let source_first = if horizontal {
            e.source.center_x <= e.target.center_x
        } else {
            e.source.center_y <= e.target.center_y
        };
        let (primary, other) = if source_first {
            (e.source, e.target)
        } else {
            (e.target, e.source)
        };
        let along = if horizontal {
            other.center_y - primary.center_y
        } else {
            other.center_x - primary.center_x
        };
        e.distance = along.abs();
        e.signed = along;
        let across = if horizontal {
            (e.ends[0].0, e.ends[1].0)
        } else {
            (e.ends[0].1, e.ends[1].1)
        };
        let key = (
            horizontal,
            bits(across.0.min(across.1)),
            bits(across.0.max(across.1)),
        );
        match fans.iter_mut().find(|(k, _)| *k == key) {
            Some((_, group)) => group.push(i),
            None => fans.push((key, vec![i])),
        }
    }
    for (_, group) in fans {
        if group.len() < 2 {
            continue;
        }
        pack(routed, &group);
    }
}

fn pack(routed: &mut [Routed], group: &[usize]) {
    let horizontal = routed[group[0]].horizontal();
    let reach = |e: &Routed| -> (f32, f32) {
        if horizontal {
            (e.ends[0].1.min(e.ends[1].1), e.ends[0].1.max(e.ends[1].1))
        } else {
            (e.ends[0].0.min(e.ends[1].0), e.ends[0].0.max(e.ends[1].0))
        }
    };
    let mut lanes: Vec<Vec<usize>> = Vec::new();
    let mut lane_of: HashMap<usize, usize> = HashMap::new();
    let mut ordered = group.to_vec();
    ordered.sort_by(|&a, &b| {
        routed[b]
            .distance
            .total_cmp(&routed[a].distance)
            .then(routed[a].signed.total_cmp(&routed[b].signed))
    });
    for &e in &ordered {
        let span = reach(&routed[e]);
        let fits = |lane: &Vec<usize>| {
            lane.iter().all(|&o| {
                let (s, end) = reach(&routed[o]);
                span.0 >= end || span.1 <= s
            })
        };
        let lane = if let Some(l) = lanes.iter().position(fits) {
            l
        } else {
            lanes.push(Vec::new());
            lanes.len() - 1
        };
        lanes[lane].push(e);
        lane_of.insert(e, lane);
    }

    let ends = group.iter().flat_map(|&e| {
        let r = &routed[e];
        if horizontal {
            [r.ends[0].0, r.ends[1].0]
        } else {
            [r.ends[0].1, r.ends[1].1]
        }
    });
    let (near, far) = ends.fold((f32::INFINITY, f32::NEG_INFINITY), |(lo, hi), v| {
        (lo.min(v), hi.max(v))
    });

    let middle = f32::midpoint(near, far);
    let tails = |e: &Routed| -> [(f32, bool); 2] {
        if horizontal {
            [
                (e.ends[0].1, e.ends[0].0 < middle),
                (e.ends[1].1, e.ends[1].0 < middle),
            ]
        } else {
            [
                (e.ends[0].0, e.ends[0].1 < middle),
                (e.ends[1].0, e.ends[1].1 < middle),
            ]
        }
    };
    let mut precedes: Vec<HashSet<usize>> = vec![HashSet::new(); lanes.len()];
    for (i, &a) in group.iter().enumerate() {
        for &b in &group[i + 1..] {
            let (lane_a, lane_b) = (lane_of[&a], lane_of[&b]);
            if lane_a == lane_b {
                continue;
            }
            for tail_a in tails(&routed[a]) {
                for tail_b in tails(&routed[b]) {
                    if (tail_a.0 - tail_b.0).abs() >= 0.5 || tail_a.1 == tail_b.1 {
                        continue;
                    }
                    let (near_lane, far_lane) = if tail_a.1 {
                        (lane_a, lane_b)
                    } else {
                        (lane_b, lane_a)
                    };
                    precedes[near_lane].insert(far_lane);
                }
            }
        }
    }
    let position = lane_order(&precedes);
    let count = float(lanes.len() + 1);
    for &e in group {
        let slot = float(position[lane_of[&e]] + 1);
        routed[e].bend = Some(near + (far - near) * slot / count);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn link(id: i64, from: i64, to: i64) -> Link {
        Link { id, from, to }
    }

    /// A hub with three holes into the next column: three ports down its right side, in the
    /// order of their far ends, and the runs on lanes of their own where they overlap.
    #[test]
    fn a_fan_gets_its_own_ports() {
        let positions = HashMap::from([
            (1, (20.0, 80.0)),
            (2, (340.0, 20.0)),
            (3, (340.0, 80.0)),
            (4, (340.0, 140.0)),
        ]);
        let links = [link(1, 1, 2), link(2, 1, 3), link(3, 1, 4)];
        let geometry = tree(&links, &positions, 40.0);
        let starts: Vec<P> = geometry
            .iter()
            .map(|g| match g.shape {
                Shape::Elbow(p) => p[0],
                Shape::Curve(p, _) => p,
            })
            .collect();
        assert_eq!(starts[0], (200.0, 86.0));
        assert_eq!(starts[1], (200.0, 100.0));
        assert_eq!(starts[2], (200.0, 114.0));
        // The level one runs straight across; up and down share one lane.
        let Shape::Elbow(level) = geometry[1].shape else {
            panic!()
        };
        assert_eq!(level[3].1, level[0].1);
    }

    #[test]
    fn a_free_edge_slides_along_its_rail() {
        let positions = HashMap::from([(1, (0.0, 0.0)), (2, (400.0, 100.0))]);
        let geometry = free(&[link(1, 1, 2)], &positions, 40.0);
        assert_eq!(
            geometry[0].shape,
            Shape::Curve((140.0, 20.0), (440.0, 120.0))
        );
    }
}
