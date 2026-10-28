//! The chain strategy, the one every map had before strategies: an alias extends its
//! parent's, so it carries the path (`1` → `11` → `112`, or `A` → `AB` → `ABC`), and a
//! bookmark says which way home is without reading the map. Bookmarks render through the
//! map's three formats.

use std::collections::HashSet;

use super::bookmark::{BookmarkSystem, Hole, class_from_security, class_short, is_wormhole_class};
use super::{Destination, NamingMap, NamingStrategy, Node, Target, render_bookmark};
use crate::maps::AliasScheme;
use crate::maps::map::MapNaming;

pub const DEFAULT_FORMAT_WORMHOLE: &str = "{alias} {sig} {class}";
pub const DEFAULT_FORMAT_KSPACE: &str = "{alias} {class} {sig} {name} {region}";
/// The leading `*` sorts the way home to the top of the in-game folder.
pub const DEFAULT_FORMAT_RETURN: &str = "*{alias} {sig} {class}";
pub const DEFAULT_IGNORED_ALIAS: &str = "HOME";

/// A-Z less H, L, N and P: those are reserved for k-space exits.
pub(super) const WORMHOLE_LETTERS: &[u8] = b"ABCDEFGIJKMOQRSTUVWXYZ";

#[derive(Debug, Clone, Copy, Default)]
pub struct Chain;

impl NamingStrategy for Chain {
    fn alias(&self, map: &NamingMap, origin: &Node, target: &Target) -> Option<String> {
        let Leg::Chain { kspace } = leg(origin, target) else {
            return None;
        };
        let parent = origin.alias.as_deref().filter(|a| !a.trim().is_empty());
        Some(guess_next_alias(
            parent,
            &map.aliases(),
            map.settings.alias_scheme,
            kspace,
            &map.settings.ignored_alias,
        ))
    }

    fn example_aliases(&self, settings: &MapNaming) -> Vec<String> {
        // The first three children of the root and the first grandchild.
        let mut aliases: Vec<String> = Vec::new();
        for _ in 0..3 {
            let next = guess_next_alias(
                Some(""),
                &aliases,
                settings.alias_scheme,
                None,
                &settings.ignored_alias,
            );
            aliases.push(next);
        }
        let child = guess_next_alias(
            aliases.first().map(String::as_str),
            &aliases,
            settings.alias_scheme,
            None,
            &settings.ignored_alias,
        );
        aliases.push(child);
        aliases
    }

    fn bookmark(
        &self,
        map: &NamingMap,
        destination: &Destination,
        hole: &Hole,
        near: Option<&Node>,
    ) -> String {
        format_bookmark(
            &destination.system,
            hole,
            &map.settings,
            near.and_then(|n| n.alias.as_deref()),
        )
    }
}

/// What a jump from one system to another is to the chain.
pub(super) enum Leg {
    /// A hop between two unaliased k-space systems: travel, and it gets no name.
    Travel,
    /// Part of the chain. `kspace` is the reserved letter of a k-space target, `None` for
    /// wormhole space or a target with no letter of its own.
    Chain { kspace: Option<char> },
}

/// A k-space exit of an aliased hole continues the chain; see [`Leg`].
pub(super) fn leg(origin: &Node, target: &Target) -> Leg {
    let aliased = origin
        .alias
        .as_deref()
        .is_some_and(|a| !a.trim().is_empty());
    let target_is_wormhole = is_wormhole_class(target.class);
    let origin_is_wormhole = is_wormhole_class(origin.system.as_ref().and_then(|s| s.class));
    if !target_is_wormhole && !origin_is_wormhole && !aliased {
        return Leg::Travel;
    }
    Leg::Chain {
        kspace: if target_is_wormhole {
            None
        } else {
            kspace_letter(target)
        },
    }
}

/// The reserved letter for a k-space target, or `None` for one with no letter of its own or
/// nothing known about it.
fn kspace_letter(target: &Target) -> Option<char> {
    let id = match (target.class, target.security) {
        (Some(class), _) => class,
        (None, Some(security)) => class_from_security(security),
        (None, None) => return None,
    };
    match class_short(id)? {
        "H" => Some('H'),
        "L" => Some('L'),
        "N" => Some('N'),
        "P" => Some('P'),
        _ => None,
    }
}

/// Whether `alias` is the map's ignored alias (e.g. `HOME`). An empty setting matches nothing.
pub fn is_ignored_alias(alias: Option<&str>, ignored: &str) -> bool {
    let ignored = ignored.trim();
    !ignored.is_empty() && alias.unwrap_or("").trim().eq_ignore_ascii_case(ignored)
}

/// The smallest positive integer not in `used`, so a freed alias is reused before growing.
pub(super) fn lowest_free(used: &HashSet<u32>) -> u32 {
    // One of the first `len + 1` is always free.
    let last = u32::try_from(used.len())
        .unwrap_or(u32::MAX)
        .saturating_add(1);
    (1..=last).find(|i| !used.contains(i)).unwrap_or(1)
}

/// The next child alias of `parent`, given every alias in use. Gaps are filled before the
/// sequence grows, and matching is case-insensitive. `kspace` is the target's reserved
/// letter, which only the alphabetical scheme uses.
pub fn guess_next_alias(
    parent: Option<&str>,
    aliases: &[String],
    scheme: AliasScheme,
    kspace: Option<char>,
    ignored: &str,
) -> String {
    let mut prefix = parent.unwrap_or("").trim().to_uppercase();
    // The home system is not a chain node, so its children start a fresh sequence.
    if is_ignored_alias(Some(&prefix), ignored) {
        prefix.clear();
    }
    let known: Vec<String> = aliases.iter().map(|a| a.trim().to_uppercase()).collect();

    if scheme == AliasScheme::Alphabetical {
        if let Some(letter) = kspace {
            // Anchored, so `AH1A` is not counted as an `AH` index.
            let marker = format!("{prefix}{letter}");
            let used: HashSet<u32> = known
                .iter()
                .filter_map(|a| a.strip_prefix(&marker))
                .filter(|tail| !tail.is_empty() && tail.bytes().all(|b| b.is_ascii_digit()))
                .filter_map(|tail| tail.parse().ok())
                .collect();
            return format!("{marker}{}", lowest_free(&used));
        }
        // Only aliases one character longer count, so k-space exits (`AH1`) and
        // grandchildren (`ABA`) are not mistaken for direct children.
        let used: HashSet<usize> = known
            .iter()
            .filter(|a| a.len() == prefix.len() + 1 && a.starts_with(&prefix))
            .filter_map(|a| {
                WORMHOLE_LETTERS
                    .iter()
                    .position(|&l| l == a.as_bytes()[prefix.len()])
            })
            .collect();
        let index = (0..=used.len()).find(|i| !used.contains(i)).unwrap_or(0);
        // Past 22 direct children the sequence repeats rather than blocking the jump.
        let letter = char::from(WORMHOLE_LETTERS[index.min(WORMHOLE_LETTERS.len() - 1)]);
        return format!("{prefix}{letter}");
    }

    let numeric: Vec<&String> = known
        .iter()
        .filter(|a| {
            a.len() > prefix.len()
                && a.starts_with(&prefix)
                && a[prefix.len()..].bytes().all(|b| b.is_ascii_digit())
        })
        .collect();
    // Drop grandchildren: `121` extends `12`, which itself extends the prefix.
    let used: HashSet<u32> = numeric
        .iter()
        .filter(|a| {
            !numeric
                .iter()
                .any(|o| o != *a && o.len() < a.len() && a.starts_with(o.as_str()))
        })
        .filter_map(|a| a[prefix.len()..].parse().ok())
        .collect();
    format!("{prefix}{}", lowest_free(&used))
}

/// Whether naming `destination` describes the way back up the chain. Chain aliases extend
/// their parent's, so an ancestor is always a prefix of the alias you are standing on, while
/// a sibling branch (`B` seen from `AB`) is not.
pub fn is_return_bookmark(
    destination: Option<&str>,
    opposite: Option<&str>,
    ignored: &str,
) -> bool {
    let opposite = opposite.unwrap_or("").trim();
    if opposite.is_empty() {
        return false;
    }
    if is_ignored_alias(destination, ignored) {
        return true;
    }
    let destination = destination.unwrap_or("").trim();
    !destination.is_empty()
        && opposite
            .to_lowercase()
            .starts_with(&destination.to_lowercase())
}

fn non_blank(format: &str) -> Option<&str> {
    Some(format).filter(|f| !f.is_empty())
}

/// The bookmark name for a system across a connection. `opposite_alias` is the alias at the
/// other end, where the bookmark is made: it fills `{current}`, and when it marks this
/// bookmark as the way back up the chain, or the system is home, the return format replaces
/// the wormhole or k-space one.
pub fn format_bookmark(
    system: &BookmarkSystem,
    hole: &Hole,
    settings: &MapNaming,
    opposite_alias: Option<&str>,
) -> String {
    let direction = if system.is_home
        || is_return_bookmark(
            system.alias.as_deref(),
            opposite_alias,
            &settings.ignored_alias,
        ) {
        Direction::Home
    } else {
        Direction::Onward
    };
    render_with_formats(system, hole, settings, opposite_alias, direction)
}

/// Which way a bookmark points, which picks the format it is written in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    /// Back toward home: the return format.
    Home,
    /// Further out: the wormhole or k-space format, by where it leads.
    Onward,
}

/// Render through the map's three formats, picked by `direction` and where it leads.
/// `current` fills `{current}`.
pub fn render_with_formats(
    system: &BookmarkSystem,
    hole: &Hole,
    settings: &MapNaming,
    current: Option<&str>,
    direction: Direction,
) -> String {
    let template = if direction == Direction::Home {
        non_blank(&settings.bookmark_return).unwrap_or(DEFAULT_FORMAT_RETURN)
    } else if is_wormhole_class(system.wormhole_class_id) {
        non_blank(&settings.bookmark_wormhole).unwrap_or(DEFAULT_FORMAT_WORMHOLE)
    } else {
        non_blank(&settings.bookmark_kspace).unwrap_or(DEFAULT_FORMAT_KSPACE)
    };
    render_bookmark(template, system, hole, current)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::maps::naming::NodeSystem;

    fn list(aliases: &[&str]) -> Vec<String> {
        aliases.iter().map(ToString::to_string).collect()
    }

    fn numeric(parent: Option<&str>, aliases: &[&str]) -> String {
        guess_next_alias(parent, &list(aliases), AliasScheme::Numeric, None, "")
    }

    fn alpha(parent: Option<&str>, aliases: &[&str], kspace: Option<char>) -> String {
        guess_next_alias(
            parent,
            &list(aliases),
            AliasScheme::Alphabetical,
            kspace,
            "",
        )
    }

    fn settings() -> MapNaming {
        MapNaming {
            strategy: crate::maps::NamingStrategyKind::Chain,
            alias_scheme: AliasScheme::Numeric,
            ignored_alias: DEFAULT_IGNORED_ALIAS.into(),
            bookmark_wormhole: String::new(),
            bookmark_kspace: String::new(),
            bookmark_return: String::new(),
        }
    }

    fn with_formats(wormhole: &str, kspace: &str, ret: &str) -> MapNaming {
        MapNaming {
            bookmark_wormhole: wormhole.into(),
            bookmark_kspace: kspace.into(),
            bookmark_return: ret.into(),
            ..settings()
        }
    }

    fn wormhole() -> BookmarkSystem {
        BookmarkSystem {
            alias: Some("D2".into()),
            name: "J122515".into(),
            region: Some("E-R00024".into()),
            wormhole_class_id: Some(5),
            security: Some(-0.99),
            occupier: None,
            is_home: false,
        }
    }

    fn kspace() -> BookmarkSystem {
        BookmarkSystem {
            alias: None,
            name: "Jita".into(),
            region: Some("The Forge".into()),
            wormhole_class_id: None,
            security: Some(0.95),
            occupier: None,
            is_home: false,
        }
    }

    fn hole() -> Hole {
        Hole {
            signature_id: Some("ABC-123".into()),
            ..Default::default()
        }
    }

    #[test]
    fn numeric_numbers_the_first_systems_from_the_root() {
        assert_eq!(numeric(None, &[]), "1");
        assert_eq!(numeric(None, &["1", "2"]), "3");
    }

    #[test]
    fn numeric_extends_the_parent() {
        assert_eq!(numeric(Some("1"), &["1"]), "11");
        assert_eq!(numeric(Some("12"), &["1", "12"]), "121");
    }

    #[test]
    fn numeric_fills_a_gap_before_growing() {
        assert_eq!(numeric(None, &["1", "3", "4"]), "2");
    }

    #[test]
    fn numeric_does_not_mistake_a_grandchild_for_a_child() {
        assert_eq!(numeric(Some("1"), &["1", "11", "111"]), "12");
    }

    #[test]
    fn matches_case_insensitively_and_suggests_upper_case() {
        assert_eq!(numeric(Some("a"), &["a", "a1"]), "A2");
    }

    #[test]
    fn alphabetical_walks_the_letters() {
        assert_eq!(alpha(Some("A"), &["A"], None), "AA");
        assert_eq!(alpha(Some("A"), &["A", "AA", "AB"], None), "AC");
    }

    #[test]
    fn alphabetical_skips_the_reserved_letters() {
        assert_eq!(
            alpha(Some(""), &["A", "B", "C", "D", "E", "F", "G"], None),
            "I"
        );
    }

    #[test]
    fn alphabetical_numbers_kspace_exits_per_letter() {
        assert_eq!(alpha(Some("A"), &["A"], Some('H')), "AH1");
        assert_eq!(alpha(Some("A"), &["A", "AH1"], Some('H')), "AH2");
        assert_eq!(alpha(Some("A"), &["A", "AH1"], Some('L')), "AL1");
    }

    #[test]
    fn alphabetical_does_not_count_a_branch_off_an_exit() {
        assert_eq!(alpha(Some("A"), &["A", "AH1", "AH1A"], Some('H')), "AH2");
    }

    #[test]
    fn alphabetical_only_counts_direct_children() {
        assert_eq!(alpha(Some("A"), &["A", "AA", "AAB"], None), "AB");
    }

    #[test]
    fn the_ignored_alias_matches_trimmed_and_case_insensitively() {
        assert!(is_ignored_alias(Some(" home "), "HOME"));
        assert!(!is_ignored_alias(Some("HOME"), ""));
        assert!(!is_ignored_alias(Some("A"), "HOME"));
    }

    #[test]
    fn home_starts_a_fresh_sequence() {
        let next = guess_next_alias(
            Some("HOME"),
            &list(&["HOME", "1"]),
            AliasScheme::Numeric,
            None,
            "HOME",
        );
        assert_eq!(next, "2");
    }

    fn node(id: i64, alias: Option<&str>, class: Option<i32>, security: f64) -> Node {
        Node {
            id,
            alias: alias.map(Into::into),
            is_home: false,
            system: Some(NodeSystem {
                solar_system_id: 30_000_000 + id,
                name: format!("S{id}"),
                region: "R".into(),
                class,
                security,
                occupier: None,
            }),
        }
    }

    fn map_of(nodes: Vec<Node>) -> NamingMap {
        NamingMap::new(settings(), nodes, Vec::new())
    }

    #[test]
    fn names_a_wormhole_reached_from_anywhere() {
        let origin = node(1, Some("1"), Some(7), 0.9);
        let map = map_of(vec![origin.clone()]);
        let target = Target {
            class: Some(5),
            security: Some(-1.0),
        };
        assert_eq!(Chain.alias(&map, &origin, &target).as_deref(), Some("11"));
    }

    #[test]
    fn continues_the_chain_into_kspace_from_an_aliased_hole() {
        let origin = node(1, Some("1"), Some(5), -1.0);
        let map = map_of(vec![origin.clone()]);
        let target = Target {
            class: Some(7),
            security: Some(0.9),
        };
        assert_eq!(Chain.alias(&map, &origin, &target).as_deref(), Some("11"));
    }

    #[test]
    fn leaves_plain_travel_unnamed() {
        let origin = node(1, None, Some(7), 0.9);
        let map = map_of(vec![origin.clone()]);
        let target = Target {
            class: Some(7),
            security: Some(0.9),
        };
        assert_eq!(Chain.alias(&map, &origin, &target), None);
    }

    #[test]
    fn reserves_a_letter_for_kspace_and_none_for_an_unknown_far_side() {
        let origin = node(1, Some("A"), Some(5), -1.0);
        let mut map = map_of(vec![origin.clone()]);
        map.settings.alias_scheme = AliasScheme::Alphabetical;
        let highsec = Target {
            class: None,
            security: Some(0.9),
        };
        assert_eq!(Chain.alias(&map, &origin, &highsec).as_deref(), Some("AH1"));
        let unknown = Target::default();
        assert_eq!(Chain.alias(&map, &origin, &unknown).as_deref(), Some("AA"));
    }

    #[test]
    fn uses_the_wormhole_and_kspace_formats() {
        assert_eq!(
            format_bookmark(&wormhole(), &hole(), &settings(), None),
            "D2 ABC C5"
        );
        assert_eq!(
            format_bookmark(&kspace(), &hole(), &settings(), None),
            "HS ABC Jita The Forge"
        );
    }

    #[test]
    fn honours_a_format_the_map_has_set() {
        let formats = with_formats("{class} {alias} [{sig}]", "", "");
        assert_eq!(
            format_bookmark(&wormhole(), &hole(), &formats, None),
            "C5 D2 [ABC]"
        );
    }

    #[test]
    fn an_ancestor_is_the_way_back_and_a_sibling_is_not() {
        assert!(is_return_bookmark(Some("A"), Some("AB"), "HOME"));
        assert!(!is_return_bookmark(Some("B"), Some("AB"), "HOME"));
        assert!(is_return_bookmark(Some("HOME"), Some("AB"), "HOME"));
        assert!(!is_return_bookmark(Some("A"), None, "HOME"));
    }

    #[test]
    fn picks_the_return_format_for_the_way_home() {
        let back = BookmarkSystem {
            alias: Some("A".into()),
            ..wormhole()
        };
        assert_eq!(
            format_bookmark(&back, &hole(), &settings(), Some("AB")),
            "*A ABC C5"
        );
    }

    #[test]
    fn names_the_system_the_bookmark_is_made_in_with_current() {
        let formats = with_formats("{alias} {sig} {current}", "", "*{alias} {sig} {current}");
        let deeper = BookmarkSystem {
            alias: Some("AB".into()),
            ..wormhole()
        };
        let back = BookmarkSystem {
            alias: Some("A".into()),
            ..wormhole()
        };
        assert_eq!(
            format_bookmark(&deeper, &hole(), &formats, Some("A")),
            "AB ABC A"
        );
        assert_eq!(
            format_bookmark(&back, &hole(), &formats, Some("AB")),
            "*A ABC AB"
        );
        assert_eq!(
            format_bookmark(&wormhole(), &hole(), &formats, None),
            "D2 ABC"
        );
    }

    #[test]
    fn home_is_the_way_back_even_without_an_alias() {
        let home = BookmarkSystem {
            name: "Turnur".into(),
            region: Some("Metropolis".into()),
            security: Some(0.4),
            is_home: true,
            ..kspace()
        };
        assert_eq!(
            format_bookmark(&home, &hole(), &settings(), Some("1")),
            "* ABC LS"
        );
    }

    #[test]
    fn an_unmapped_hole_says_only_what_the_type_promises() {
        let unknown = BookmarkSystem::default();
        assert_eq!(format_bookmark(&unknown, &hole(), &settings(), None), "ABC");
        let typed = BookmarkSystem {
            wormhole_class_id: Some(5),
            ..unknown
        };
        assert_eq!(
            format_bookmark(&typed, &hole(), &settings(), None),
            "ABC C5"
        );
    }

    #[test]
    fn previews_the_scheme_and_each_format() {
        let shown = super::super::preview(&settings());
        assert_eq!(shown.aliases, ["1", "2", "3", "11"]);
        assert_eq!(shown.wormhole, "1a ABC C5");
        assert_eq!(shown.kspace, "1b HS ABC Jita The Forge");
        assert_eq!(shown.ret, "*1 ABC C5");

        let alphabetical = MapNaming {
            alias_scheme: AliasScheme::Alphabetical,
            ..settings()
        };
        assert_eq!(
            super::super::preview(&alphabetical).aliases,
            ["A", "B", "C", "AA"]
        );
    }
}
