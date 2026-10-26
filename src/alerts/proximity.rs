//! How far a system is from a chain, in gate jumps.
//!
//! A multi-source breadth-first search from every system on the map at once, over the static
//! stargate graph merged with the map's own wormhole connections. Unweighted, because an
//! alert only ever asks "how many jumps", never "which way is safer". Searching from all the
//! map's systems at once is what keeps it cheap: one traversal answers "how close is the
//! nearest part of my chain". The same search from one system answers "how far is the
//! target from here, through the chain", which is what an alert with a starting point asks.

use std::collections::{HashMap, HashSet, VecDeque};

use sqlx::PgPool;

/// Jovian stargates need access nobody has, so a route through Zarzakh is not a route.
const ZARZAKH: i64 = 30100000;

/// The stargate graph, loaded once and shared by every evaluation.
///
/// Around 8,000 systems and 14,000 gates: small enough to hold, expensive enough that
/// re-reading it per killmail would be the most expensive thing the ingest does.
#[derive(Debug)]
pub struct Universe {
    adjacency: HashMap<i64, Vec<i64>>,
}

impl Universe {
    pub async fn load(pool: &PgPool) -> sqlx::Result<Universe> {
        let rows = sqlx::query!(
            "select solar_system_id, destination_system_id from stargates
             where solar_system_id <> $1 and destination_system_id <> $1",
            ZARZAKH,
        )
        .fetch_all(pool)
        .await?;
        let mut adjacency: HashMap<i64, Vec<i64>> = HashMap::new();
        for row in rows {
            adjacency
                .entry(row.solar_system_id)
                .or_default()
                .push(row.destination_system_id);
        }
        Ok(Universe { adjacency })
    }

    fn neighbours(&self, id: i64) -> &[i64] {
        self.adjacency.get(&id).map_or(&[], Vec::as_slice)
    }
}

/// What the search found: how far, from where, and the way there.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Proximity {
    pub jumps: i32,
    /// The system on the map the route starts from.
    pub from: i64,
    /// Every system on the way, starting at `from` and ending at the target.
    pub route: Vec<i64>,
}

/// The nearest of `origins` to `target`, within `max_jumps`.
///
/// `chain` is the map's own connections, which are edges like any other: a target one gate
/// from a k-space exit at the far end of the chain is one jump away, however deep the chain
/// runs. Wormholes are free rather than a hop, matching what the client's router shows, and
/// they stay free partway along a route: from a single origin the way to the target may
/// gate to the chain, cross it, and gate on.
///
/// `avoid` is the map's ignore list: never stepped into on the way, matching the client's
/// router. The origins and the target are exempt, since a search that starts or ends in one
/// is asking about it on purpose.
pub fn nearest(
    universe: &Universe,
    origins: &[i64],
    chain: &[(i64, i64)],
    avoid: &[i64],
    target: i64,
    max_jumps: i32,
) -> Option<Proximity> {
    if origins.is_empty() || max_jumps < 0 {
        return None;
    }

    let mut wormholes: HashMap<i64, Vec<i64>> = HashMap::new();
    for (a, b) in chain {
        wormholes.entry(*a).or_default().push(*b);
        wormholes.entry(*b).or_default().push(*a);
    }
    let mut search = Search {
        blocked: avoid
            .iter()
            .copied()
            .filter(|id| *id != target && !origins.contains(id))
            .collect(),
        ..Search::default()
    };

    // Every origin is its own starting point at distance zero, before any of them reaches
    // another over a wormhole, so the message names somewhere you are.
    for origin in origins {
        if !search.distance.contains_key(origin) {
            search.visit(*origin, 0, None, *origin);
        }
    }
    for origin in origins {
        search.cross_wormholes(&wormholes, *origin);
    }
    if let Some(found) = search.found(target) {
        return Some(found);
    }

    while let Some(current) = search.queue.pop_front() {
        let steps = search.distance[&current];
        if steps >= max_jumps {
            continue;
        }
        let origin = search.origin_of[&current];
        for next in universe.neighbours(current) {
            if search.distance.contains_key(next) || search.blocked.contains(next) {
                continue;
            }
            search.visit(*next, steps + 1, Some(current), origin);
            search.cross_wormholes(&wormholes, *next);
            if let Some(found) = search.found(target) {
                return Some(found);
            }
        }
    }
    None
}

/// The breadth-first frontier: reached from where, via which system, and how far.
#[derive(Default)]
struct Search {
    came_from: HashMap<i64, i64>,
    origin_of: HashMap<i64, i64>,
    distance: HashMap<i64, i32>,
    queue: VecDeque<i64>,
    blocked: HashSet<i64>,
}

impl Search {
    fn visit(&mut self, id: i64, jumps: i32, via: Option<i64>, origin: i64) {
        self.distance.insert(id, jumps);
        self.origin_of.insert(id, origin);
        if let Some(via) = via {
            self.came_from.insert(id, via);
        }
        self.queue.push_back(id);
    }

    /// Everything the chain reaches from `id` is as far away as `id` itself. Done the
    /// moment a system is reached, so the queue stays in distance order.
    fn cross_wormholes(&mut self, wormholes: &HashMap<i64, Vec<i64>>, id: i64) {
        let mut stack = vec![id];
        while let Some(at) = stack.pop() {
            let jumps = self.distance[&at];
            let origin = self.origin_of[&at];
            for next in wormholes.get(&at).map_or(&[][..], Vec::as_slice) {
                if self.distance.contains_key(next) || self.blocked.contains(next) {
                    continue;
                }
                self.visit(*next, jumps, Some(at), origin);
                stack.push(*next);
            }
        }
    }

    fn found(&self, target: i64) -> Option<Proximity> {
        let jumps = *self.distance.get(&target)?;
        Some(Proximity {
            jumps,
            from: self.origin_of[&target],
            route: walk_back(&self.came_from, target),
        })
    }
}

fn walk_back(came_from: &HashMap<i64, i64>, target: i64) -> Vec<i64> {
    let mut route = vec![target];
    let mut at = target;
    while let Some(previous) = came_from.get(&at) {
        route.push(*previous);
        at = *previous;
    }
    route.reverse();
    route
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A line of five systems: 1 - 2 - 3 - 4 - 5.
    fn line() -> Universe {
        let mut adjacency: HashMap<i64, Vec<i64>> = HashMap::new();
        for id in 1..5 {
            adjacency.entry(id).or_default().push(id + 1);
            adjacency.entry(id + 1).or_default().push(id);
        }
        Universe { adjacency }
    }

    #[test]
    fn counts_gate_jumps_from_the_nearest_mapped_system() {
        let found = nearest(&line(), &[1, 4], &[], &[], 5, 10).unwrap();
        assert_eq!(found.jumps, 1);
        assert_eq!(found.from, 4);
        assert_eq!(found.route, vec![4, 5]);
    }

    #[test]
    fn a_mapped_system_is_zero_jumps_from_itself() {
        let found = nearest(&line(), &[3], &[], &[], 3, 5).unwrap();
        assert_eq!(found.jumps, 0);
        assert_eq!(found.route, vec![3]);
    }

    #[test]
    fn nothing_beyond_the_limit() {
        assert!(nearest(&line(), &[1], &[], &[], 5, 3).is_none());
        assert_eq!(nearest(&line(), &[1], &[], &[], 5, 4).unwrap().jumps, 4);
    }

    /// The chain is a shortcut: a wormhole from 1 to 4 puts 5 one jump from the map even
    /// though the gates alone would be four.
    #[test]
    fn the_chain_carries_the_search_with_it() {
        let found = nearest(&line(), &[1], &[(1, 4)], &[], 5, 2).unwrap();
        assert_eq!(found.jumps, 1);
        assert_eq!(found.from, 1);
        assert_eq!(found.route, vec![1, 4, 5]);
    }

    /// From a single origin the chain can sit partway along the route: 1 gates to 2, a
    /// wormhole from 2 lands in 4, and 5 is one more gate.
    #[test]
    fn a_wormhole_partway_along_the_route_is_still_free() {
        let found = nearest(&line(), &[1], &[(2, 4)], &[], 5, 2).unwrap();
        assert_eq!(found.jumps, 2);
        assert_eq!(found.route, vec![1, 2, 4, 5]);
        assert!(nearest(&line(), &[1], &[(2, 4)], &[], 5, 1).is_none());
    }

    #[test]
    fn an_empty_map_is_never_near_anything() {
        assert!(nearest(&line(), &[], &[], &[], 5, 10).is_none());
    }

    /// The only gate route from 1 to 5 runs through 3; ignoring 3 cuts it, and a wormhole
    /// landing in 3 is no way in either.
    #[test]
    fn an_ignored_system_is_never_stepped_into() {
        assert!(nearest(&line(), &[1], &[], &[3], 5, 10).is_none());
        assert!(nearest(&line(), &[1], &[(1, 3)], &[3], 5, 10).is_none());
        let found = nearest(&line(), &[1], &[(1, 4)], &[3], 5, 10).unwrap();
        assert_eq!(found.route, vec![1, 4, 5]);
    }

    /// Asking about an ignored system, or from one, still gets an answer: the list keeps
    /// routes from passing through, not from starting or ending there.
    #[test]
    fn the_origin_and_the_target_are_exempt() {
        assert_eq!(nearest(&line(), &[1], &[], &[3], 3, 10).unwrap().jumps, 2);
        assert_eq!(nearest(&line(), &[3], &[], &[3], 1, 10).unwrap().jumps, 2);
    }
}
