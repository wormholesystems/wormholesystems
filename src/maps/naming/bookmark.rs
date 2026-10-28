//! The pieces any naming strategy builds a bookmark name from: what a bookmark says about a
//! system and a hole, and the `{token}` templates the map's formats are written in.

use crate::maps::{MassStatus, TimeStatus, WormholeSize};

/// Every token a bookmark format may use.
pub const BOOKMARK_TOKENS: [&str; 11] = [
    "alias", "current", "sig", "class", "name", "region", "occupier", "size", "wh", "mass", "life",
];

/// A system as a bookmark describes it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct BookmarkSystem {
    pub alias: Option<String>,
    pub name: String,
    pub region: Option<String>,
    pub wormhole_class_id: Option<i32>,
    /// `None` when the far side is unknown, which blanks the class rather than guessing.
    pub security: Option<f64>,
    pub occupier: Option<String>,
    /// The map's home system: a bookmark pointing there is the way home, alias or not.
    pub is_home: bool,
}

/// What a bookmark says about the hole itself.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Hole {
    pub signature_id: Option<String>,
    pub size: Option<WormholeSize>,
    pub mass_status: Option<MassStatus>,
    pub time_status: Option<TimeStatus>,
    /// The wormhole code, e.g. `H296`.
    pub wormhole_code: Option<String>,
}

/// Destination class of a K162 that leads to a C1, C2 or C3. Not a real class: the game
/// only tells you that much.
pub const C1_TO_3: i32 = 101;
/// Destination class of a K162 that leads to a C4 or C5.
pub const C4_TO_5: i32 = 102;

/// Classes 1 to 6 and 12 to 18 are wormhole space, and so are the K162 groups.
pub fn is_wormhole_class(id: Option<i32>) -> bool {
    matches!(id, Some(1..=6 | 12..=18 | C1_TO_3 | C4_TO_5))
}

/// A class's short label: `C5`, `H`, `L`, `N`, `P`.
pub fn class_short(id: i32) -> Option<&'static str> {
    Some(match id {
        1 => "C1",
        2 => "C2",
        3 => "C3",
        4 => "C4",
        5 => "C5",
        6 => "C6",
        7 => "H",
        8 => "L",
        9 => "N",
        12 => "C12",
        13 => "C13",
        14 => "C14",
        15 => "C15",
        16 => "C16",
        17 => "C17",
        18 => "C18",
        19 => "C19",
        20 => "C20",
        21 => "C21",
        22 => "C22",
        23 => "C23",
        25 => "P",
        C1_TO_3 => "C1-3",
        C4_TO_5 => "C4-5",
        _ => return None,
    })
}

/// The class a system without one falls in by its security band.
pub fn class_from_security(security: f64) -> i32 {
    let rounded = crate::util::security::ccp_round_security(security);
    if rounded >= 0.5 {
        7
    } else if rounded >= 0.1 {
        8
    } else {
        9
    }
}

/// `C3` for wormhole space, otherwise `HS` / `LS` / `NS` / `P`. Blank when the destination is
/// unknown, since a security of zero would label an unscanned hole as lowsec.
pub fn bookmark_class(wormhole_class_id: Option<i32>, security: Option<f64>) -> String {
    if wormhole_class_id.is_none() && security.is_none() {
        return String::new();
    }
    let id = wormhole_class_id.unwrap_or_else(|| class_from_security(security.unwrap_or(0.0)));
    match class_short(id).unwrap_or("?") {
        "H" => "HS".into(),
        "L" => "LS".into(),
        "N" => "NS".into(),
        short => short.into(),
    }
}

/// The scanner id's first three characters: `ABC-123` is bookmarked as `ABC`.
pub fn short_signature_id(signature_id: Option<&str>) -> String {
    signature_id
        .map(|s| s.chars().take(3).collect())
        .unwrap_or_default()
}

/// The value of one token. `current` is the alias of the system the bookmark is made in.
fn token_value(
    token: &str,
    system: &BookmarkSystem,
    hole: &Hole,
    current: Option<&str>,
) -> Option<String> {
    let text = |value: Option<&str>| value.unwrap_or("").to_string();
    Some(match token {
        "alias" => text(system.alias.as_deref()),
        "current" => text(current.map(str::trim)),
        "sig" => short_signature_id(hole.signature_id.as_deref()),
        "class" => bookmark_class(system.wormhole_class_id, system.security),
        "name" => system.name.clone(),
        "region" => text(system.region.as_deref()),
        "occupier" => text(system.occupier.as_deref()),
        // Large is the common case and deliberately blank.
        "size" => match hole.size {
            Some(WormholeSize::Small) => "SM",
            Some(WormholeSize::Medium) => "MD",
            Some(WormholeSize::Xl) => "XM",
            _ => "",
        }
        .into(),
        "wh" => text(hole.wormhole_code.as_deref()),
        // Stable says nothing, so mass only shows once the hole has degraded.
        "mass" => match hole.mass_status {
            Some(MassStatus::Reduced) => "reduced",
            Some(MassStatus::Critical) => "crit",
            _ => "",
        }
        .into(),
        // Kept in the EOL vocabulary so it never reads as mass "crit".
        "life" => match hole.time_status {
            Some(TimeStatus::Eol) => "EOL",
            Some(TimeStatus::Critical) => "EOL!",
            Some(TimeStatus::Expired) => "EOL!!",
            _ => "",
        }
        .into(),
        _ => return None,
    })
}

/// Substitute `{token}` placeholders and collapse the gaps empty ones leave. An unknown
/// placeholder stays as written, so a typo in the format is visible.
pub fn render_bookmark(
    template: &str,
    system: &BookmarkSystem,
    hole: &Hole,
    current: Option<&str>,
) -> String {
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        match after.find('}') {
            Some(close)
                if !after[..close].is_empty()
                    && after[..close]
                        .chars()
                        .all(|c| c.is_alphanumeric() || c == '_') =>
            {
                let token = &after[..close];
                if let Some(value) = token_value(token, system, hole, current) {
                    out.push_str(&value);
                } else {
                    out.push('{');
                    out.push_str(token);
                    out.push('}');
                }
                rest = &after[close + 1..];
            }
            _ => {
                out.push('{');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wormhole() -> BookmarkSystem {
        BookmarkSystem {
            alias: Some("D2".into()),
            name: "J122515".into(),
            wormhole_class_id: Some(5),
            security: Some(-0.99),
            ..Default::default()
        }
    }

    fn hole() -> Hole {
        Hole {
            signature_id: Some("ABC-123".into()),
            ..Default::default()
        }
    }

    #[test]
    fn class_uses_the_wormhole_class_and_two_letters_for_kspace() {
        assert_eq!(bookmark_class(Some(5), Some(-0.99)), "C5");
        assert_eq!(bookmark_class(None, Some(0.95)), "HS");
        assert_eq!(bookmark_class(None, Some(0.3)), "LS");
        assert_eq!(bookmark_class(None, Some(-0.5)), "NS");
        assert_eq!(bookmark_class(Some(C4_TO_5), None), "C4-5");
    }

    #[test]
    fn keeps_the_three_characters_read_in_the_scanner() {
        assert_eq!(short_signature_id(Some("ABC-123")), "ABC");
        assert_eq!(short_signature_id(None), "");
    }

    #[test]
    fn drops_empty_tokens_and_closes_the_gap() {
        let text = render_bookmark("{alias} {sig} {class} {region}", &wormhole(), &hole(), None);
        assert_eq!(text, "D2 ABC C5");
    }

    #[test]
    fn leaves_an_unknown_placeholder_alone() {
        let text = render_bookmark("{alias} {nope}", &wormhole(), &hole(), None);
        assert_eq!(text, "D2 {nope}");
    }

    #[test]
    fn names_only_the_restrictive_sizes() {
        let with = |size| Hole {
            size: Some(size),
            ..hole()
        };
        let render = |h: &Hole| render_bookmark("{alias} {sig} {size}", &wormhole(), h, None);
        assert_eq!(render(&with(WormholeSize::Small)), "D2 ABC SM");
        assert_eq!(render(&with(WormholeSize::Medium)), "D2 ABC MD");
        assert_eq!(render(&with(WormholeSize::Xl)), "D2 ABC XM");
        assert_eq!(render(&with(WormholeSize::Large)), "D2 ABC");
    }

    #[test]
    fn only_shows_mass_and_lifetime_once_degraded() {
        let render = |mass, time| {
            let h = Hole {
                mass_status: Some(mass),
                time_status: Some(time),
                ..hole()
            };
            render_bookmark("{alias} {sig} {mass} {life}", &wormhole(), &h, None)
        };
        assert_eq!(render(MassStatus::Stable, TimeStatus::Stable), "D2 ABC");
        assert_eq!(
            render(MassStatus::Critical, TimeStatus::Eol),
            "D2 ABC crit EOL"
        );
        assert_eq!(
            render(MassStatus::Stable, TimeStatus::Critical),
            "D2 ABC EOL!"
        );
        assert_eq!(
            render(MassStatus::Stable, TimeStatus::Expired),
            "D2 ABC EOL!!"
        );
    }
}
