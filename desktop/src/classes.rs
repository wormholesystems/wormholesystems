//! Solar system classes as the website numbers them: 1 to 6 and 12 to 18 wormhole space,
//! 7, 8 and 9 high, low and null, 25 Pochven, and the destination groups.

/// Destination class of a K162 that leads to a C1, C2 or C3. Not a real class: the game
/// only tells you that much.
pub(crate) const C1_TO_3: i64 = 101;
/// Destination class of a K162 that leads to a C4 or C5.
pub(crate) const C4_TO_5: i64 = 102;
/// Destination class of Pochven's C729, which leads to high, low or null.
pub(crate) const K_SPACE: i64 = 103;

/// Whether a destination class (possibly a group like `C1-3`) covers a system's class.
pub(crate) fn dest_includes(dest: i64, class: Option<i64>) -> bool {
    match (dest, class) {
        (C1_TO_3, Some(1..=3)) | (C4_TO_5, Some(4..=5)) | (K_SPACE, Some(7..=9)) => true,
        _ => class == Some(dest),
    }
}

/// Security the way the game shows it: 0.0 stays, anything in (0, 0.05) is 0.1, the rest
/// rounds to the nearest tenth, halves away from zero.
fn ccp_round_security(security: f64) -> f64 {
    if security == 0.0 {
        return 0.0;
    }
    if security > 0.0 && security < 0.05 {
        return 0.1;
    }
    let scaled = security * 10.0;
    scaled.signum() * scaled.abs().round() / 10.0
}

pub(crate) fn class_from_security(security: f64) -> i64 {
    let rounded = ccp_round_security(security);
    if rounded >= 0.5 {
        7
    } else if rounded >= 0.1 {
        8
    } else {
        9
    }
}
