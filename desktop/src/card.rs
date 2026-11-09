//! The card on the system you are in: what it is, what it leads to, what it does to ships,
//! who is there, and what died there lately. It floats over the game like the signatures
//! overlay and changes with every jump.

use std::fmt::Write as _;
use std::time::Instant;

use chrono::{DateTime, Utc};
use gpui_kit::component::{ActiveTheme as _, Theme};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, App, Div, Entity, FontWeight, Hsla, InteractiveElement, IntoElement, MouseButton,
    ParentElement, StatefulInteractiveElement, Styled, div, px, svg,
};

use crate::api::{EffectModifier, Kill, MapSnapshot, Pilot, PlacedSystem, TimeStatus};
use crate::sigs::{Command, Grab, Sigs};
use crate::theme::{self, HEADING};
use crate::ui::rpx;

/// How many kills the card lists.
const KILLS: usize = 5;

/// What the card shows beyond the map, fetched for one system.
#[derive(Debug, Clone)]
pub(crate) struct Intel {
    pub solar_system_id: i64,
    pub kills: Vec<Kill>,
    pub notes: Option<String>,
    pub effect: Vec<EffectModifier>,
    /// Everyone online on the map; the card picks out who is here.
    pub pilots: Vec<Pilot>,
    pub fetched: Instant,
}

/// The card.
pub(crate) fn panel(entity: &Entity<Sigs>, grab: Option<Grab>, cx: &App) -> AnyElement {
    let theme = cx.theme().clone();
    let sigs = entity.read(cx);
    let pad = rpx(8.0);
    let intel = sigs.intel.as_ref().filter(|i| {
        sigs.system()
            .is_some_and(|s| s.solar_system_id == i.solar_system_id)
    });

    let (title, sub) = title(sigs, &theme);
    let header = div()
        .flex_none()
        .flex()
        .items_start()
        .gap_2()
        .px(pad)
        .py_2()
        .border_b_1()
        .border_color(theme.border)
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .gap(rpx(2.0))
                .when_some(grab, |d, grab| {
                    d.cursor_move()
                        .on_mouse_down(MouseButton::Left, move |e, w, cx| grab(e, w, cx))
                })
                .child(
                    title
                        .font_family(HEADING)
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_size(rpx(13.0)),
                )
                .child(
                    div()
                        .text_xs()
                        .truncate()
                        .text_color(theme.muted_foreground)
                        .child(sub),
                ),
        )
        .child(
            div()
                .id("card-hide")
                .flex_none()
                .p_1()
                .cursor_pointer()
                .hover(|s| s.bg(theme.muted))
                .on_click({
                    let entity = entity.clone();
                    move |_, _, cx| entity.update(cx, |_, cx| cx.emit(Command::HideCard))
                })
                .child(
                    svg()
                        .path("icons/x.svg")
                        .size(rpx(12.0))
                        .text_color(theme.muted_foreground),
                ),
        );

    let mut body = div().flex().flex_col().gap_3().px(pad).py_2();
    if let (Some(system), Some(snapshot)) = (sigs.system(), sigs.snapshot.as_ref()) {
        body = body
            .children(tags(system, &theme))
            .children(statics(system, &theme))
            .children(effect(system, intel, &theme))
            .child(scanning(system, snapshot, &theme))
            .children(intel.and_then(|i| pilots(system, &i.pilots, &theme)))
            .children(intel.and_then(|i| notes(i, &theme)))
            .child(kills(intel, &theme));
    } else {
        let text = if sigs.here.is_none() {
            "Log in a character; the chat logs say where you are."
        } else {
            "Not on the map, so there is nothing to show yet."
        };
        body = body.child(
            div()
                .text_xs()
                .text_color(theme.muted_foreground)
                .child(text),
        );
    }

    div()
        .size_full()
        .flex()
        .flex_col()
        .child(header)
        .child(
            div()
                .id("card-body")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .child(body),
        )
        .into_any_element()
}

/// The class, name and alias, and under it where the system is.
fn title(sigs: &Sigs, theme: &Theme) -> (Div, String) {
    let row = div().flex().items_center().gap_2().min_w_0();
    let Some(here) = &sigs.here else {
        return (
            row.child(
                div()
                    .text_color(theme.muted_foreground)
                    .child("No location"),
            ),
            String::new(),
        );
    };
    let Some(system) = sigs.system() else {
        return (row.child(here.system.clone()), here.character.clone());
    };
    let (class, color) = theme::class_meta(system.wormhole_class_id, Some(system.security_status));
    let row = row
        .child(div().flex_none().text_color(color).child(class))
        .when_some(
            system.alias.clone().filter(|a| !a.trim().is_empty()),
            |d, alias| d.child(div().flex_none().child(alias)),
        )
        .child(
            div()
                .truncate()
                .text_color(theme.muted_foreground)
                .child(system.name.clone()),
        );
    let mut sub = vec![system.region.clone()];
    if !system.constellation.is_empty() {
        sub.push(system.constellation.clone());
    }
    // Known space and Pochven: the security status matters there.
    if matches!(system.wormhole_class_id, None | Some(7..=9 | 25)) {
        sub.push(format!(
            "{:.1}",
            crate::classes::ccp_round_security(system.security_status)
        ));
    }
    (row, sub.join(" · "))
}

/// A small heading over a group of lines.
fn heading(text: &'static str, theme: &Theme) -> Div {
    div()
        .text_xs()
        .text_color(theme.muted_foreground)
        .font_weight(FontWeight::SEMIBOLD)
        .child(text)
}

fn group(text: &'static str, theme: &Theme) -> Div {
    div()
        .flex()
        .flex_col()
        .gap_1()
        .text_xs()
        .child(heading(text, theme))
}

/// A word in a frame: the map's intel at a glance.
fn tag(text: String, color: Hsla) -> Div {
    div()
        .px_1()
        .border_1()
        .border_color(color.opacity(0.5))
        .bg(color.opacity(0.12))
        .text_color(color)
        .child(text)
}

/// Home, what the map says about it, its threat, who lives there and who holds it.
fn tags(system: &PlacedSystem, theme: &Theme) -> Option<Div> {
    let mut tags = Vec::new();
    if system.is_home {
        tags.push(tag("Home".into(), theme.primary));
    }
    match system.status.as_deref() {
        Some("friendly") => tags.push(tag("Friendly".into(), theme::healthy())),
        Some("hostile") => tags.push(tag("Hostile".into(), theme::critical())),
        Some("active") => tags.push(tag("Active".into(), theme::reduced())),
        Some("empty") => tags.push(tag("Empty".into(), theme.muted_foreground)),
        _ => {}
    }
    match system.threat_level.as_deref() {
        Some("critical") => tags.push(tag("Critical threat".into(), theme::critical())),
        Some("high") => tags.push(tag("High threat".into(), theme::reduced())),
        _ => {}
    }
    if system.is_shattered {
        tags.push(tag("Shattered".into(), theme.muted_foreground));
    }
    if let Some(group) = system.occupying_group.clone().filter(|g| !g.is_empty()) {
        tags.push(tag(group, theme.foreground));
    }
    if let Some(sov) = &system.sovereignty {
        let text = match &sov.ticker {
            Some(ticker) => format!("[{ticker}] {}", sov.name),
            None => sov.name.clone(),
        };
        tags.push(tag(text, theme.foreground));
    }
    (!tags.is_empty()).then(|| div().flex().flex_wrap().gap_1().text_xs().children(tags))
}

/// Each static, where it leads, and how long and how much it lasts.
fn statics(system: &PlacedSystem, theme: &Theme) -> Option<Div> {
    if system.statics.is_empty() {
        return None;
    }
    let rows = system.statics.iter().map(|s| {
        let (dest, color) = theme::dest_class_meta(s.dest_class);
        let mut facts = Vec::new();
        if let Some(h) = s.lifetime_hours {
            facts.push(format!("{h}h"));
        }
        if let Some(kg) = s.total_mass {
            facts.push(format!("{} kt", kilotons(kg)));
        }
        if let Some(kg) = s.max_jump_mass {
            facts.push(format!("{} kt a jump", kilotons(kg)));
        }
        div()
            .flex()
            .gap_2()
            .child(div().w(rpx(40.0)).child(s.code.clone()))
            .child(div().w(rpx(32.0)).text_color(color).child(dest))
            .child(
                div()
                    .truncate()
                    .text_color(theme.muted_foreground)
                    .child(facts.join(" · ")),
            )
    });
    Some(group("Statics", theme).children(rows))
}

/// The effect and what it changes.
fn effect(system: &PlacedSystem, intel: Option<&Intel>, theme: &Theme) -> Option<Div> {
    let name = system.effect_name.clone()?;
    let rows = intel
        .map(|i| i.effect.as_slice())
        .unwrap_or_default()
        .iter()
        .map(|m| {
            div()
                .flex()
                .gap_2()
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_color(theme.muted_foreground)
                        .child(m.stat.clone()),
                )
                .child(div().flex_none().child(m.value.clone()))
        });
    Some(group("Effect", theme).child(name).children(rows))
}

/// How far the scanning got, and the holes out of here.
fn scanning(system: &PlacedSystem, snapshot: &MapSnapshot, theme: &Theme) -> Div {
    let sigs: Vec<_> = snapshot
        .signatures
        .iter()
        .filter(|s| s.solar_system_id == system.solar_system_id)
        .collect();
    let unknown = sigs
        .iter()
        .filter(|s| s.group.is_empty() || s.group == "unknown")
        .count();
    let mut line = match sigs.len() {
        0 => "No signatures".to_string(),
        1 => "1 signature".to_string(),
        n => format!("{n} signatures"),
    };
    if unknown > 0 {
        let _ = write!(line, ", {unknown} unknown");
    }
    if let Some(at) = system.signatures_pasted_at {
        let _ = write!(line, " · pasted {}", ago(at, Utc::now()));
    }
    let holes: Vec<_> = snapshot
        .connections
        .iter()
        .filter(|c| c.from_system == system.id || c.to_system == system.id)
        .collect();
    let eol = holes
        .iter()
        .filter(|c| matches!(c.time_status, Some(TimeStatus::Eol | TimeStatus::Critical)))
        .count();
    let crit = holes
        .iter()
        .filter(|c| c.mass_status == Some(crate::api::MassStatus::Critical))
        .count();
    let mut holes_line = match holes.len() {
        0 => "No connections".to_string(),
        1 => "1 connection".to_string(),
        n => format!("{n} connections"),
    };
    if eol > 0 {
        let _ = write!(holes_line, ", {eol} end of life");
    }
    if crit > 0 {
        let _ = write!(holes_line, ", {crit} critical");
    }
    group("Scanning", theme)
        .child(line)
        .child(div().text_color(theme.muted_foreground).child(holes_line))
}

/// The map's pilots in this system.
fn pilots(system: &PlacedSystem, pilots: &[Pilot], theme: &Theme) -> Option<Div> {
    let here: Vec<_> = pilots
        .iter()
        .filter(|p| p.solar_system_id == Some(system.solar_system_id))
        .collect();
    if here.is_empty() {
        return None;
    }
    let rows = here.into_iter().map(|p| {
        let ship = match (&p.ship_type, p.is_docked) {
            (Some(ship), true) => format!("{ship}, docked"),
            (Some(ship), false) => ship.clone(),
            (None, true) => "docked".into(),
            (None, false) => String::new(),
        };
        div()
            .flex()
            .gap_2()
            .child(
                div()
                    .flex_none()
                    .when(p.is_mine, |d| d.text_color(theme.primary))
                    .child(p.name.clone()),
            )
            .when(!p.corporation_ticker.is_empty(), |d| {
                d.child(
                    div()
                        .flex_none()
                        .text_color(theme.muted_foreground)
                        .child(format!("[{}]", p.corporation_ticker)),
                )
            })
            .child(
                div()
                    .truncate()
                    .text_color(theme.muted_foreground)
                    .child(ship),
            )
    });
    Some(group("Pilots here", theme).children(rows))
}

fn notes(intel: &Intel, theme: &Theme) -> Option<Div> {
    let notes = intel.notes.clone()?;
    Some(group("Notes", theme).child(div().text_color(theme.foreground).child(notes)))
}

/// The latest kills, or that there were none.
fn kills(intel: Option<&Intel>, theme: &Theme) -> Div {
    let group = group("Recent kills", theme);
    let Some(intel) = intel else {
        return group.child(div().text_color(theme.muted_foreground).child("Loading…"));
    };
    if intel.kills.is_empty() {
        return group.child(
            div()
                .text_color(theme.muted_foreground)
                .child("None lately"),
        );
    }
    let now = Utc::now();
    let rows = intel.kills.iter().take(KILLS).map(|k| {
        let who = victim(k);
        let value = k.total_value.map(isk).unwrap_or_default();
        div()
            .flex()
            .gap_2()
            .when(k.is_npc, |d| d.opacity(0.6))
            .child(
                div()
                    .flex_none()
                    .w(rpx(28.0))
                    .text_color(theme.muted_foreground)
                    .child(ago(k.time, now)),
            )
            .child(
                div()
                    .flex_none()
                    .child(k.victim.ship_name.clone().unwrap_or_else(|| "?".into())),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_color(theme.muted_foreground)
                    .child(who),
            )
            .child(div().flex_none().w(px(36.0)).child(value))
    });
    group.children(rows)
}

/// Who died: the pilot and their ticker, else the corporation or alliance.
fn victim(kill: &Kill) -> String {
    let v = &kill.victim;
    let ticker = v.alliance_ticker.as_ref().or(v.corporation_ticker.as_ref());
    match (&v.character_name, ticker) {
        (Some(name), Some(t)) => format!("{name} [{t}]"),
        (Some(name), None) => name.clone(),
        (None, Some(t)) => format!("[{t}]"),
        (None, None) => String::new(),
    }
}

/// How long ago, at column width: `now`, `47m`, `3h`, `2d`.
pub(crate) fn ago(when: DateTime<Utc>, now: DateTime<Utc>) -> String {
    match (now - when).num_minutes() {
        m if m < 1 => "now".into(),
        m if m < 60 => format!("{m}m"),
        m if m < 24 * 60 => format!("{}h", m / 60),
        m => format!("{}d", m / (24 * 60)),
    }
}

/// Kilograms as kilotons, grouped, one decimal at most: `2,000`, `62.5`.
fn kilotons(kg: f64) -> String {
    let kt = (kg / 100_000.0).round() / 10.0;
    let whole = kt.trunc();
    let tenth = ((kt - whole) * 10.0).round();
    let digits = format!("{whole:.0}");
    let mut grouped = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            grouped.push(',');
        }
        grouped.push(c);
    }
    if tenth > 0.0 {
        format!("{grouped}.{tenth}")
    } else {
        grouped
    }
}

/// ISK as a killboard writes it: `340M`, `1.2B`.
fn isk(value: f64) -> String {
    let (scaled, unit) = match value {
        v if v >= 1e12 => (v / 1e12, "T"),
        v if v >= 1e9 => (v / 1e9, "B"),
        v if v >= 1e6 => (v / 1e6, "M"),
        v if v >= 1e3 => (v / 1e3, "K"),
        v => (v, ""),
    };
    let rounded = (scaled * 10.0).round() / 10.0;
    if rounded.fract() == 0.0 {
        format!("{rounded:.0}{unit}")
    } else {
        format!("{rounded:.1}{unit}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_masses_and_isk_like_the_website() {
        assert_eq!(kilotons(2_000_000_000.0), "2,000");
        assert_eq!(kilotons(62_500_000.0), "62.5");
        assert_eq!(kilotons(375_000_000.0), "375");
        assert_eq!(isk(2_400_000.0), "2.4M");
        assert_eq!(isk(340_000_000.0), "340M");
        assert_eq!(isk(1_250_000_000.0), "1.3B");
        assert_eq!(isk(800.0), "800");
    }

    #[test]
    fn says_how_long_ago() {
        let now = Utc::now();
        assert_eq!(ago(now, now), "now");
        assert_eq!(ago(now - chrono::Duration::minutes(47), now), "47m");
        assert_eq!(ago(now - chrono::Duration::hours(3), now), "3h");
        assert_eq!(ago(now - chrono::Duration::days(2), now), "2d");
    }
}
