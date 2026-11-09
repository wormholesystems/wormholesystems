//! Routes from where you are: Dijkstra over the stargates plus the chain's holes and, when
//! wanted, EVE Scout's. A port of `frontend/src/lib/routing/algorithm.ts` and
//! `graph-build.ts`, so the jump counts here are the website's. Change both together.

use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap, HashSet};
use std::sync::Arc;

use serde::Deserialize;

use crate::api::{MapSnapshot, MassStatus, TimeStatus};

/// Zarzakh: its gates lock you in, so no route goes through it.
const ZARZAKH: i64 = 30_100_000;

/// The stargates, and every system's security, from `/api/routing-graph`.
#[derive(Debug, Default)]
pub(crate) struct Gates {
    /// Shared with jump planning, which only needs to know a gate when it sees one.
    pub adjacency: Arc<HashMap<i64, Vec<i64>>>,
    pub security: HashMap<i64, f64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Preference {
    #[default]
    Shorter,
    Safer,
    LessSecure,
}

/// The route settings the website keeps per map and user.
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub(crate) struct Settings {
    #[serde(rename = "route_preference")]
    pub preference: Preference,
    /// 0 to 100, how much the preference weighs security.
    pub security_penalty: f64,
    /// The worst lifetime and mass still flown through.
    #[serde(rename = "route_allow_time_status")]
    pub allow_time: TimeStatus,
    #[serde(rename = "route_allow_mass_status")]
    pub allow_mass: MassStatus,
    #[serde(rename = "route_use_evescout")]
    pub use_evescout: bool,
    #[serde(rename = "route_use_wormholes")]
    pub use_wormholes: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            preference: Preference::Shorter,
            security_penalty: 50.0,
            allow_time: TimeStatus::Critical,
            allow_mass: MassStatus::Critical,
            use_evescout: false,
            use_wormholes: true,
        }
    }
}

/// A hole out of Thera or Turnur, as EVE Scout lists it.
#[derive(Debug, Clone, Deserialize)]
pub(crate) struct ScoutHole {
    pub hub_solar_system_id: i64,
    pub solar_system_id: i64,
    pub mass_status: Option<MassStatus>,
    pub time_status: Option<TimeStatus>,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct Hole {
    to: i64,
    mass: Option<MassStatus>,
    time: Option<TimeStatus>,
}

/// Holes by the system they leave from, both ways.
pub(crate) type Holes = HashMap<i64, Vec<Hole>>;

/// The chain's connections between real systems, and EVE Scout's holes when asked for.
/// Stargate-kind connections (drawn for Ansiblex bridges) are plain gates.
pub(crate) fn holes(snapshot: &MapSnapshot, scout: &[ScoutHole], settings: &Settings) -> Holes {
    let placed: HashMap<i64, i64> = snapshot
        .systems
        .iter()
        .map(|s| (s.id, s.solar_system_id))
        .collect();
    let mut out: Holes = HashMap::new();
    let mut add = |a: i64, b: i64, mass, time| {
        if a == b {
            return;
        }
        out.entry(a).or_default().push(Hole { to: b, mass, time });
        out.entry(b).or_default().push(Hole { to: a, mass, time });
    };
    for c in &snapshot.connections {
        let (Some(&a), Some(&b)) = (placed.get(&c.from_system), placed.get(&c.to_system)) else {
            continue;
        };
        if c.kind == crate::api::ConnectionKind::Wormhole {
            if settings.use_wormholes {
                add(a, b, c.mass_status, c.time_status);
            }
        } else {
            add(a, b, None, None);
        }
    }
    if settings.use_evescout {
        for h in scout {
            add(
                h.hub_solar_system_id,
                h.solar_system_id,
                h.mass_status,
                h.time_status,
            );
        }
    }
    out
}

fn time_rank(t: TimeStatus) -> u8 {
    match t {
        TimeStatus::Stable => 0,
        TimeStatus::Eol => 1,
        TimeStatus::Critical => 2,
        TimeStatus::Expired => 3,
    }
}

fn mass_rank(m: MassStatus) -> u8 {
    match m {
        MassStatus::Stable => 0,
        MassStatus::Reduced => 1,
        MassStatus::Critical => 2,
    }
}

fn allowed(settings: &Settings, hole: &Hole) -> bool {
    time_rank(hole.time.unwrap_or(TimeStatus::Stable)) <= time_rank(settings.allow_time)
        && mass_rank(hole.mass.unwrap_or(MassStatus::Stable)) <= mass_rank(settings.allow_mass)
}

/// What a jump into a system of that security costs.
fn cost(settings: &Settings, security: Option<f64>) -> f64 {
    if settings.preference == Preference::Shorter {
        return 1.0;
    }
    let penalty = (0.15 * settings.security_penalty).exp();
    let sec = crate::classes::ccp_round_security(security.unwrap_or(0.0));
    match settings.preference {
        _ if sec <= 0.0 => 2.0 * penalty,
        Preference::Safer if sec < 0.45 => penalty,
        Preference::Safer => 0.9,
        _ if sec < 0.45 => 0.9,
        _ => penalty,
    }
}

#[derive(PartialEq)]
struct Queued {
    cost: f64,
    id: i64,
}

impl Eq for Queued {}

impl Ord for Queued {
    // Lowest cost first out of the max-heap.
    fn cmp(&self, other: &Self) -> Ordering {
        other.cost.total_cmp(&self.cost)
    }
}

impl PartialOrd for Queued {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// Every route out of one system.
pub(crate) struct Search {
    jumps: HashMap<i64, usize>,
}

impl Search {
    /// Search the whole graph from `from`, stepping around `ignored` systems and Zarzakh.
    /// Those are still fine places to arrive: a route can end in one, never pass through.
    pub(crate) fn new(
        gates: &Gates,
        holes: &Holes,
        from: i64,
        settings: &Settings,
        ignored: &HashSet<i64>,
    ) -> Search {
        let avoided = |id: i64| id != from && (id == ZARZAKH || ignored.contains(&id));
        let mut dist: HashMap<i64, f64> = HashMap::from([(from, 0.0)]);
        let mut jumps: HashMap<i64, usize> = HashMap::from([(from, 0)]);
        let mut settled = HashSet::new();
        let mut queue = BinaryHeap::from([Queued {
            cost: 0.0,
            id: from,
        }]);
        while let Some(Queued { cost: at, id }) = queue.pop() {
            if !settled.insert(id) || avoided(id) {
                continue;
            }
            let hops = jumps[&id];
            let by_gate = gates.adjacency.get(&id).into_iter().flatten().copied();
            let by_hole = holes
                .get(&id)
                .into_iter()
                .flatten()
                .filter(|h| allowed(settings, h))
                .map(|h| h.to);
            for next in by_gate.chain(by_hole) {
                let c = at + cost(settings, gates.security.get(&next).copied());
                if c < dist.get(&next).copied().unwrap_or(f64::INFINITY) {
                    dist.insert(next, c);
                    jumps.insert(next, hops + 1);
                    queue.push(Queued { cost: c, id: next });
                }
            }
        }
        Search { jumps }
    }

    /// Jumps to `to`, `None` when it cannot be reached.
    pub(crate) fn jumps(&self, to: i64) -> Option<usize> {
        self.jumps.get(&to).copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Gates both ways between each pair, high-sec unless said otherwise.
    fn gates(pairs: &[(i64, i64)], low: &[i64]) -> Gates {
        let mut adjacency: HashMap<i64, Vec<i64>> = HashMap::new();
        let mut security = HashMap::new();
        for &(a, b) in pairs {
            adjacency.entry(a).or_default().push(b);
            adjacency.entry(b).or_default().push(a);
            for id in [a, b] {
                security.insert(id, if low.contains(&id) { 0.3 } else { 0.9 });
            }
        }
        Gates {
            adjacency: Arc::new(adjacency),
            security,
        }
    }

    /// A line of gates 1-2-3-4-5, with 3 low-sec.
    const LINE: [(i64, i64); 4] = [(1, 2), (2, 3), (3, 4), (4, 5)];

    fn line() -> Gates {
        gates(&LINE, &[3])
    }

    fn hole(a: i64, b: i64, time: Option<TimeStatus>) -> Holes {
        let mut h: Holes = HashMap::new();
        h.entry(a).or_default().push(Hole {
            to: b,
            mass: None,
            time,
        });
        h.entry(b).or_default().push(Hole {
            to: a,
            mass: None,
            time,
        });
        h
    }

    #[test]
    fn counts_gate_jumps() {
        let s = Search::new(
            &line(),
            &Holes::new(),
            1,
            &Settings::default(),
            &HashSet::new(),
        );
        assert_eq!(s.jumps(1), Some(0));
        assert_eq!(s.jumps(5), Some(4));
        assert_eq!(s.jumps(99), None);
    }

    #[test]
    fn takes_a_hole_when_it_is_shorter_and_healthy_enough() {
        let gates = line();
        let shortcut = hole(1, 5, Some(TimeStatus::Eol));
        let s = Search::new(&gates, &shortcut, 1, &Settings::default(), &HashSet::new());
        assert_eq!(s.jumps(5), Some(1));
        let healthy_only = Settings {
            allow_time: TimeStatus::Stable,
            ..Settings::default()
        };
        let s = Search::new(&gates, &shortcut, 1, &healthy_only, &HashSet::new());
        assert_eq!(s.jumps(5), Some(4));
    }

    #[test]
    fn steps_around_ignored_systems_but_can_end_in_one() {
        // A long way round 3: 2-6-7-4.
        let gates = gates(&[LINE.as_slice(), &[(2, 6), (6, 7), (7, 4)]].concat(), &[3]);
        let ignored = HashSet::from([3]);
        let s = Search::new(&gates, &Holes::new(), 1, &Settings::default(), &ignored);
        assert_eq!(s.jumps(5), Some(5));
        assert_eq!(s.jumps(3), Some(2));
    }

    #[test]
    fn safer_prefers_high_sec() {
        // 2-8-9-4 is all high-sec, one jump longer than through low-sec 3.
        let gates = gates(&[LINE.as_slice(), &[(2, 8), (8, 9), (9, 4)]].concat(), &[3]);
        let safer = Settings {
            preference: Preference::Safer,
            ..Settings::default()
        };
        let s = Search::new(&gates, &Holes::new(), 1, &safer, &HashSet::new());
        assert_eq!(s.jumps(5), Some(5));
        let s = Search::new(
            &gates,
            &Holes::new(),
            1,
            &Settings::default(),
            &HashSet::new(),
        );
        assert_eq!(s.jumps(5), Some(4));
    }
}
