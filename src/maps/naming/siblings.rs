//! The siblings strategy: each system numbers its own holes, `1`, `2`, `3`, without its
//! own alias in front, so one further out starts at `1` again. Names stay short however
//! deep the chain goes, and the map, not the alias, says which way home is.

use std::collections::HashSet;

use super::bookmark::Hole;
use super::chain::{
    Direction, Leg, WORMHOLE_LETTERS, is_ignored_alias, leg, lowest_free, render_with_formats,
};
use super::{Destination, NamingMap, NamingStrategy, Node, Target};
use crate::maps::AliasScheme;
use crate::maps::map::MapNaming;

#[derive(Debug, Clone, Copy, Default)]
pub struct Siblings;

impl NamingStrategy for Siblings {
    fn alias(&self, map: &NamingMap, origin: &Node, target: &Target) -> Option<String> {
        let Leg::Chain { kspace } = leg(origin, target) else {
            return None;
        };
        let taken = children_aliases(map, origin);
        Some(next_sibling(&taken, map.settings.alias_scheme, kspace))
    }

    fn bookmark(
        &self,
        map: &NamingMap,
        destination: &Destination,
        hole: &Hole,
        near: Option<&Node>,
    ) -> String {
        let system = &destination.system;
        let way_home = system.is_home
            || (near.is_some()
                && is_ignored_alias(system.alias.as_deref(), &map.settings.ignored_alias))
            || match (destination.node, near) {
                (Some(dest), Some(near)) => is_toward_home(map, dest, near.id),
                _ => false,
            };
        render_with_formats(
            system,
            hole,
            &map.settings,
            near.and_then(|n| n.alias.as_deref()),
            if way_home {
                Direction::Home
            } else {
                Direction::Onward
            },
        )
    }

    fn example_aliases(&self, settings: &MapNaming) -> Vec<String> {
        // Three holes off one system, then the first off the next one out.
        let mut aliases: Vec<String> = Vec::new();
        for _ in 0..3 {
            aliases.push(next_sibling(&aliases, settings.alias_scheme, None));
        }
        aliases.push(next_sibling(&[], settings.alias_scheme, None));
        aliases
    }
}

/// The aliases of `origin`'s children: its neighbours, less the one it was reached from.
/// That one is closer to home; on a map with no home every neighbour counts.
fn children_aliases(map: &NamingMap, origin: &Node) -> Vec<String> {
    let distance = map.distances_from_home();
    let own = distance.get(&origin.id);
    map.neighbours(origin.id)
        .filter(|n| match (own, distance.get(&n.id)) {
            (Some(own), Some(theirs)) => theirs >= own,
            _ => true,
        })
        .filter_map(|n| n.alias.clone())
        .collect()
}

/// Whether `destination`, seen from its neighbour `near`, is the way home: one step closer.
fn is_toward_home(map: &NamingMap, destination: i64, near: i64) -> bool {
    let distance = map.distances_from_home();
    match (distance.get(&destination), distance.get(&near)) {
        (Some(d), Some(n)) => d < n,
        _ => false,
    }
}

/// The lowest name not among `taken`: a number, or in the alphabetical scheme a letter, with
/// k-space exits taking their reserved letter and a number (`H1`, `H2`). Names that are
/// neither, a custom `Farm` say, take nothing.
pub fn next_sibling(taken: &[String], scheme: AliasScheme, kspace: Option<char>) -> String {
    let taken: Vec<String> = taken.iter().map(|a| a.trim().to_uppercase()).collect();
    let numbered = |prefix: &str| -> HashSet<u32> {
        taken
            .iter()
            .filter_map(|a| a.strip_prefix(prefix))
            .filter(|tail| !tail.is_empty() && tail.bytes().all(|b| b.is_ascii_digit()))
            .filter_map(|tail| tail.parse().ok())
            .collect()
    };
    match (scheme, kspace) {
        (AliasScheme::Numeric, _) => lowest_free(&numbered("")).to_string(),
        (AliasScheme::Alphabetical, Some(letter)) => {
            let prefix = letter.to_string();
            format!("{prefix}{}", lowest_free(&numbered(&prefix)))
        }
        (AliasScheme::Alphabetical, None) => {
            let used: HashSet<usize> = taken
                .iter()
                .filter(|a| a.len() == 1)
                .filter_map(|a| WORMHOLE_LETTERS.iter().position(|&l| l == a.as_bytes()[0]))
                .collect();
            let index = (0..=used.len()).find(|i| !used.contains(i)).unwrap_or(0);
            // Past 22 the sequence repeats rather than blocking the jump.
            char::from(WORMHOLE_LETTERS[index.min(WORMHOLE_LETTERS.len() - 1)]).to_string()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::maps::NamingStrategyKind;
    use crate::maps::naming::bookmark::BookmarkSystem;
    use crate::maps::naming::{Edge, NodeSystem};

    fn settings() -> MapNaming {
        MapNaming {
            strategy: NamingStrategyKind::Siblings,
            alias_scheme: AliasScheme::Numeric,
            ignored_alias: "HOME".into(),
            bookmark_wormhole: String::new(),
            bookmark_kspace: String::new(),
            bookmark_return: String::new(),
        }
    }

    fn node(id: i64, alias: Option<&str>, class: i32) -> Node {
        Node {
            id,
            alias: alias.map(Into::into),
            is_home: false,
            system: Some(NodeSystem {
                solar_system_id: 31_000_000 + id,
                name: format!("J{id}"),
                region: "R".into(),
                class: Some(class),
                security: -1.0,
                occupier: None,
            }),
        }
    }

    fn edge(id: i64, from: i64, to: i64) -> Edge {
        Edge {
            id,
            from,
            to,
            size: None,
            mass_status: None,
            time_status: None,
        }
    }

    /// The legacy screenshot: home (Farm) with two holes, `1` and `2`; `1` has two of its
    /// own, `1` and `2` again; `2` has one, `1`.
    fn farm() -> NamingMap {
        NamingMap::new(settings(), farm_nodes(), farm_edges())
    }

    fn farm_nodes() -> Vec<Node> {
        let mut home = node(1, Some("Farm"), 5);
        home.is_home = true;
        vec![
            home,
            node(2, Some("1"), 5),
            node(3, Some("2"), 5),
            node(4, Some("1"), 6),
            node(5, Some("2"), 4),
            node(6, Some("1"), 3),
        ]
    }

    fn farm_edges() -> Vec<Edge> {
        vec![
            edge(10, 1, 2),
            edge(11, 1, 3),
            edge(12, 2, 4),
            edge(13, 2, 5),
            edge(14, 3, 6),
        ]
    }

    fn wormhole() -> Target {
        Target {
            class: Some(5),
            security: Some(-1.0),
        }
    }

    fn alias_from(map: &NamingMap, origin: i64) -> Option<String> {
        Siblings.alias(map, map.node(origin).unwrap(), &wormhole())
    }

    #[test]
    fn numbers_each_systems_holes_afresh() {
        let map = farm();
        assert_eq!(alias_from(&map, 1).as_deref(), Some("3"));
        assert_eq!(alias_from(&map, 2).as_deref(), Some("3"));
        assert_eq!(alias_from(&map, 3).as_deref(), Some("2"));
    }

    #[test]
    fn the_way_home_takes_no_number_from_the_children() {
        // Node 4 is `1`, reached from its parent, also `1`. Counting the parent would make
        // its first child `2`.
        let map = farm();
        assert_eq!(alias_from(&map, 4).as_deref(), Some("1"));
    }

    #[test]
    fn fills_a_gap_before_growing_and_counts_past_nine() {
        let taken: Vec<String> = ["1", "3"].map(Into::into).to_vec();
        assert_eq!(next_sibling(&taken, AliasScheme::Numeric, None), "2");
        let ten: Vec<String> = (1..=10).map(|n| n.to_string()).collect();
        assert_eq!(next_sibling(&ten, AliasScheme::Numeric, None), "11");
    }

    #[test]
    fn alphabetical_letters_and_kspace_exits() {
        let taken: Vec<String> = ["A", "B", "H1"].map(Into::into).to_vec();
        assert_eq!(next_sibling(&taken, AliasScheme::Alphabetical, None), "C");
        assert_eq!(
            next_sibling(&taken, AliasScheme::Alphabetical, Some('H')),
            "H2"
        );
        assert_eq!(
            next_sibling(&taken, AliasScheme::Alphabetical, Some('L')),
            "L1"
        );
    }

    #[test]
    fn leaves_plain_travel_unnamed() {
        let mut jita = node(7, None, 7);
        jita.system.as_mut().unwrap().security = 0.9;
        let mut nodes = farm_nodes();
        nodes.push(jita);
        let map = NamingMap::new(settings(), nodes, farm_edges());
        let highsec = Target {
            class: Some(7),
            security: Some(0.9),
        };
        assert_eq!(Siblings.alias(&map, map.node(7).unwrap(), &highsec), None);
    }

    fn bookmark(map: &NamingMap, dest: i64, near: i64) -> String {
        let destination = Destination {
            node: Some(dest),
            system: map.node(dest).unwrap().bookmark_system(),
        };
        let hole = Hole {
            signature_id: Some("ABC-123".into()),
            ..Hole::default()
        };
        Siblings.bookmark(map, &destination, &hole, map.node(near))
    }

    #[test]
    fn the_way_home_is_read_off_the_map_not_the_alias() {
        let map = farm();
        // From `1` (node 4), its parent `1` (node 2) is the way back, though no alias says so.
        assert_eq!(bookmark(&map, 2, 4), "*1 ABC C5");
        // And the other way it leads on.
        assert_eq!(bookmark(&map, 4, 2), "1 ABC C6");
        // Home itself is always the way back.
        assert_eq!(bookmark(&map, 1, 2), "*Farm ABC C5");
    }

    #[test]
    fn a_map_without_home_never_guesses_a_way_back() {
        let mut nodes = farm_nodes();
        nodes[0].is_home = false;
        let map = NamingMap::new(settings(), nodes, farm_edges());
        assert_eq!(bookmark(&map, 2, 4), "1 ABC C5");
    }

    #[test]
    fn an_unflown_hole_leads_on() {
        let map = farm();
        let destination = Destination {
            node: None,
            system: BookmarkSystem {
                alias: Some("3".into()),
                wormhole_class_id: Some(3),
                ..BookmarkSystem::default()
            },
        };
        let text = Siblings.bookmark(&map, &destination, &Hole::default(), map.node(2));
        assert_eq!(text, "3 C3");
    }

    #[test]
    fn previews_three_siblings_and_a_fresh_start() {
        assert_eq!(Siblings.example_aliases(&settings()), ["1", "2", "3", "1"]);
        let alphabetical = MapNaming {
            alias_scheme: AliasScheme::Alphabetical,
            ..settings()
        };
        assert_eq!(
            Siblings.example_aliases(&alphabetical),
            ["A", "B", "C", "A"]
        );
    }
}
