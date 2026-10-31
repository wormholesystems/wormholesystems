//! What to do about a jump read from the chat logs: which of the origin's signatures could be
//! the hole just flown, where the new system goes on the map, and the alias it is offered.
//!
//! Ports of the website's jump prompt, so a jump mapped from here lands the same as one
//! mapped there: `groupSignatures` in `frontend/src/lib/signatures/compatibility.ts`,
//! `freePosition` in `frontend/src/lib/map/geometry.ts`, `suggestAlias` in
//! `frontend/src/lib/naming/alias.ts` and `sizeForJumpMass` in `frontend/src/lib/map/mass.ts`.
//! Change both together.

use std::collections::{HashMap, HashSet};

use crate::api::{Ghost, MapSnapshot, PlacedSystem, Signature, SignatureType, WormholeSize};
use crate::classes::dest_includes;

/// Fixed node width on the website's canvas. Height is two grid cells.
const NODE_W: f64 = 180.0;
/// Clear space kept between placed nodes, in grid cells.
const NODE_GAP_CELLS: f64 = 1.0;

/// The website's canvas geometry, from `/api/grid-config`.
#[derive(Debug, Clone, Copy, serde::Deserialize)]
pub(crate) struct Grid {
    pub cell_size: f64,
    pub world_width: f64,
    pub world_height: f64,
}

impl Default for Grid {
    fn default() -> Self {
        Grid {
            cell_size: 20.0,
            world_width: 4000.0,
            world_height: 2000.0,
        }
    }
}

/// Only wormholes and unclassified signatures can be connections; the rest are sites.
fn can_be_connection(sig: &Signature) -> bool {
    sig.group == "wormhole" || sig.group == "unknown"
}

/// Whether a signature's type could lead into a system of `target_class`. An open or
/// unresolved type (a bare K162) always could.
fn can_lead_to_class(ty: Option<&SignatureType>, target_class: Option<i64>) -> bool {
    match (ty.and_then(|t| t.target_class), target_class) {
        (Some(destination), Some(target)) => dest_includes(destination, Some(target)),
        _ => true,
    }
}

/// A system's signatures sorted for the question "which one was it?", in scanner order.
#[derive(Debug, Default, Clone)]
pub(crate) struct Candidates {
    /// Unmapped, and the type (if any) can lead to the target's class.
    pub likely: Vec<Signature>,
    /// Unmapped, but typed for a class the target is not. Typed by hand off a paste, so
    /// demoted rather than hidden.
    pub unlikely: Vec<Signature>,
}

impl Candidates {
    /// Everything that can be picked, numbered from 1 in this order.
    pub(crate) fn numbered(&self) -> impl Iterator<Item = &Signature> {
        self.likely.iter().chain(&self.unlikely)
    }
}

/// Sort `signatures` the way the website's jump prompt does. A signature already on a
/// connection only stays a candidate when that connection still ends in a ghost (in
/// `unflown`): the jump is what says where it goes.
pub(crate) fn candidates(
    signatures: &[Signature],
    types: &HashMap<i64, &SignatureType>,
    target_class: Option<i64>,
    unflown: &HashSet<i64>,
) -> Candidates {
    let mut sorted: Vec<&Signature> = signatures.iter().collect();
    sorted.sort_by(|a, b| a.signature_id.cmp(&b.signature_id));
    let mut out = Candidates::default();
    for sig in sorted {
        if !can_be_connection(sig) {
            continue;
        }
        if sig.connection_id.is_some_and(|c| !unflown.contains(&c)) {
            continue;
        }
        let ty = sig.signature_type_id.and_then(|id| types.get(&id).copied());
        if can_lead_to_class(ty, target_class) {
            out.likely.push(sig.clone());
        } else {
            out.unlikely.push(sig.clone());
        }
    }
    out
}

/// The unmapped holes drawn off `origin`, as connection id to ghost placement id.
pub(crate) fn ghosts_from(origin: &PlacedSystem, snapshot: &MapSnapshot) -> HashMap<i64, i64> {
    let ghost_ids: HashSet<i64> = snapshot.ghosts.iter().map(|g| g.id).collect();
    snapshot
        .connections
        .iter()
        .filter_map(|c| {
            let other = if c.from_system == origin.id {
                c.to_system
            } else if c.to_system == origin.id {
                c.from_system
            } else {
                return None;
            };
            ghost_ids.contains(&other).then_some((c.id, other))
        })
        .collect()
}

/// The first free, grid-snapped slot at or after `base`: beside it, then down that column,
/// keeping a cell of clear space from every placed node.
pub(crate) fn free_position(placed: &[(f64, f64)], base: (f64, f64), g: Grid) -> (f64, f64) {
    let node_h = 2.0 * g.cell_size;
    let gap = NODE_GAP_CELLS * g.cell_size;
    let step_x = NODE_W + gap;
    let step_y = node_h + gap;
    let max_x = g.world_width - NODE_W;
    let max_y = g.world_height - node_h;
    let snap = |v: f64| (v / g.cell_size).round() * g.cell_size;
    let crowded = |x: f64, y: f64| {
        placed
            .iter()
            .any(|&(px, py)| (x - px).abs() < step_x && (y - py).abs() < step_y)
    };
    let bx = snap(base.0.clamp(0.0, max_x.max(0.0)));
    let by = snap(base.1.clamp(0.0, max_y.max(0.0)));
    if !crowded(bx, by) {
        return (bx, by);
    }
    for column in 1.. {
        let x = snap(bx + f64::from(column) * step_x);
        if x > max_x {
            break;
        }
        for row in 0.. {
            let y = snap(by + f64::from(row) * step_y);
            if y > max_y {
                break;
            }
            if !crowded(x, y) {
                return (x, y);
            }
        }
    }
    (bx, by)
}

/// Where every placement on the map sits, ghosts included.
pub(crate) fn positions(systems: &[PlacedSystem], ghosts: &[Ghost]) -> Vec<(f64, f64)> {
    systems
        .iter()
        .map(|s| (s.position_x, s.position_y))
        .chain(ghosts.iter().map(|g| (g.position_x, g.position_y)))
        .collect()
}

/// The ship size a hole takes, from the heaviest ship it lets through.
pub(crate) fn size_for_jump_mass(kg: Option<f64>) -> Option<WormholeSize> {
    let kg = kg.filter(|k| *k > 0.0)?;
    Some(if kg <= 5_000_000.0 {
        WormholeSize::Small
    } else if kg <= 62_000_000.0 {
        WormholeSize::Medium
    } else if kg <= 375_000_000.0 {
        WormholeSize::Large
    } else {
        WormholeSize::Xl
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sig(id: &str, group: &str, ty: Option<i64>, connection: Option<i64>) -> Signature {
        Signature {
            signature_id: id.into(),
            group: group.into(),
            signature_type_id: ty,
            connection_id: connection,
            ..Default::default()
        }
    }

    fn ty(id: i64, target_class: Option<i64>) -> SignatureType {
        SignatureType {
            id,
            signature: Some("H296".into()),
            name: "Wormhole".into(),
            signature_category_id: 1,
            target_class,
            max_jump_mass: None,
            extra: None,
            spawn_areas: Vec::new(),
            regions: Vec::new(),
            constellations: Vec::new(),
            systems: Vec::new(),
        }
    }

    #[test]
    fn candidates_rank_and_never_hide() {
        let (k162, to_c5) = (ty(1, None), ty(2, Some(5)));
        let types: HashMap<i64, &SignatureType> = [(1, &k162), (2, &to_c5)].into();
        let sigs = vec![
            sig("ZZZ-100", "wormhole", Some(2), None),
            sig("AAA-100", "wormhole", Some(1), None),
            sig("BBB-100", "data", None, None),
            sig("CCC-100", "unknown", None, None),
            sig("DDD-100", "wormhole", None, Some(7)),
            sig("EEE-100", "wormhole", None, Some(8)),
        ];
        let unflown = HashSet::from([8]);
        let c = candidates(&sigs, &types, Some(3), &unflown);
        let ids = |v: &[Signature]| v.iter().map(|s| s.signature_id.clone()).collect::<Vec<_>>();
        // Sites are never ways out, and a hole already flown is not this one; one that ends
        // in a ghost still could be.
        assert_eq!(ids(&c.likely), ["AAA-100", "CCC-100", "EEE-100"]);
        // Typed for a C5 when the target is a C3: demoted, not dropped.
        assert_eq!(ids(&c.unlikely), ["ZZZ-100"]);
        let numbered: Vec<_> = c.numbered().map(|s| s.signature_id.as_str()).collect();
        assert_eq!(numbered, ["AAA-100", "CCC-100", "EEE-100", "ZZZ-100"]);
    }

    #[test]
    fn a_k162_group_leads_to_each_of_its_classes() {
        let c1_to_3 = ty(1, Some(crate::classes::C1_TO_3));
        assert!(can_lead_to_class(Some(&c1_to_3), Some(2)));
        assert!(!can_lead_to_class(Some(&c1_to_3), Some(4)));
        let c4_to_5 = ty(2, Some(crate::classes::C4_TO_5));
        assert!(can_lead_to_class(Some(&c4_to_5), Some(5)));
        assert!(!can_lead_to_class(Some(&c4_to_5), Some(6)));
        let c729 = ty(3, Some(crate::classes::K_SPACE));
        assert!(can_lead_to_class(Some(&c729), Some(8)));
        assert!(!can_lead_to_class(Some(&c729), Some(25)));
    }

    const GRID: Grid = Grid {
        cell_size: 20.0,
        world_width: 4000.0,
        world_height: 2000.0,
    };

    #[test]
    fn free_position_snaps_and_keeps_a_gap() {
        assert_eq!(free_position(&[], (300.0, 200.0), GRID), (300.0, 200.0));
        assert_eq!(free_position(&[], (307.0, 193.0), GRID), (300.0, 200.0));
        let gap = NODE_GAP_CELLS * GRID.cell_size;
        // Anchored on a node: beside it, a full gap away.
        let spot = free_position(&[(400.0, 300.0)], (400.0, 300.0), GRID);
        assert_eq!(spot, (400.0 + NODE_W + gap, 300.0));
        // The next sibling goes under the first rather than marching right.
        let spot = free_position(&[(400.0, 300.0), (600.0, 300.0)], (400.0, 300.0), GRID);
        assert_eq!(spot, (600.0, 300.0 + 2.0 * GRID.cell_size + gap));
    }

    #[test]
    fn sizes_from_jump_mass() {
        assert_eq!(size_for_jump_mass(None), None);
        assert_eq!(
            size_for_jump_mass(Some(5_000_000.0)),
            Some(WormholeSize::Small)
        );
        assert_eq!(
            size_for_jump_mass(Some(62_000_000.0)),
            Some(WormholeSize::Medium)
        );
        assert_eq!(
            size_for_jump_mass(Some(375_000_000.0)),
            Some(WormholeSize::Large)
        );
        assert_eq!(
            size_for_jump_mass(Some(1_000_000_000.0)),
            Some(WormholeSize::Xl)
        );
    }
}
