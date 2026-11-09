//! The card on the system you are in, the website's System card with its Pilots, Killmails
//! and Notes beneath: what it is, what it leads to, what it does to ships, who is there,
//! and what died there lately. It floats over the game and changes with every jump.

use std::fmt::Write as _;
use std::time::Instant;

use chrono::{DateTime, Utc};
use gpui_kit::component::{ActiveTheme as _, Theme};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, App, Div, ElementId, Entity, FontWeight, Hsla, InteractiveElement, IntoElement,
    ParentElement, StatefulInteractiveElement, Styled, div, px,
};

use crate::api::{EffectModifier, Kill, MapSnapshot, Pilot, PlacedSystem};
use crate::look::{self, Images, MONO};
use crate::overlay::Floating;
use crate::sigs::{Grab, Sigs};
use crate::ui::rpx;

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
    let intel = sigs.intel.as_ref().filter(|i| {
        sigs.system()
            .is_some_and(|s| s.solar_system_id == i.solar_system_id)
    });
    let header = look::header(entity, Floating::System, "SYSTEM", None, grab, &theme);
    // How wide the card is, at the text size it is drawn in, for what fits in a kill's row.
    let place = Floating::System.place(sigs);
    let width = place.dimensions().0 * 100.0 / f32::from(sigs.overlay.text_size.max(1));

    let mut body = div().flex().flex_col();
    if let (Some(system), Some(snapshot)) = (sigs.system(), sigs.snapshot.as_ref()) {
        let here: Vec<&Pilot> = intel
            .map(|i| i.pilots.as_slice())
            .unwrap_or_default()
            .iter()
            .filter(|p| p.solar_system_id == Some(system.solar_system_id))
            .collect();
        body = body
            .child(summary(system, snapshot, &theme))
            .children(statics(system, &theme))
            .children(effect(intel, &theme))
            .children(sovereignty(system, &sigs.images, &theme))
            .child(pilots(&here, &sigs.images, &theme))
            .child(kills(intel, &sigs.images, width, &theme))
            .children(intel.and_then(|i| notes(i, &theme)));
    } else {
        let text = if sigs.here.is_none() {
            "Log in a character; the chat logs say where you are."
        } else {
            "Not on the map, so there is nothing to show yet."
        };
        body = body.child(empty(text, &theme));
    }

    div()
        .size_full()
        .flex()
        .flex_col()
        .font_family(look::SANS)
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

fn empty(text: &'static str, theme: &Theme) -> Div {
    div()
        .px(rpx(12.0))
        .py(rpx(16.0))
        .text_size(rpx(12.0))
        .text_color(theme.muted_foreground)
        .child(text)
}

/// A block of the card with the hairline under it.
fn block(theme: &Theme) -> Div {
    div()
        .px(rpx(12.0))
        .py(rpx(8.0))
        .border_b_1()
        .border_color(theme.border.opacity(0.5))
}

/// Links out for a system: zKillboard, Dotlan, and Anoikis for a wormhole.
fn links(system: &PlacedSystem) -> Vec<(&'static str, String)> {
    let wormhole = system
        .wormhole_class_id
        .is_some_and(|c| !matches!(c, 7..=9 | 25));
    let under = |s: &str| s.replace(' ', "_");
    let dotlan = if wormhole {
        format!("https://evemaps.dotlan.net/system/{}", under(&system.name))
    } else {
        format!(
            "https://evemaps.dotlan.net/map/{}/{}",
            under(&system.region),
            under(&system.name)
        )
    };
    let mut links = vec![
        (
            "zKill",
            format!("https://zkillboard.com/system/{}/", system.solar_system_id),
        ),
        ("Dotlan", dotlan),
    ];
    if wormhole {
        links.push(("Anoik", format!("https://anoik.is/systems/{}", system.name)));
    }
    links
}

/// Class and name, the effect, who lives there, where it is and where to read more; then
/// how far the scanning got.
fn summary(system: &PlacedSystem, snapshot: &MapSnapshot, theme: &Theme) -> Div {
    let name = match system.alias.clone().filter(|a| !a.trim().is_empty()) {
        Some(alias) => look::strong(alias).child(
            div()
                .text_color(theme.muted_foreground)
                .child(format!(" ({})", system.name)),
        ),
        None => look::strong(system.name.clone()),
    };
    let small = || {
        div()
            .text_size(rpx(11.0))
            .text_color(theme.muted_foreground)
    };
    let side = |text: String, color: Hsla| {
        div()
            .flex_none()
            .text_size(rpx(10.0))
            .text_color(color)
            .child(text)
    };
    block(theme)
        .py(rpx(12.0))
        .child(
            div()
                .flex()
                .items_center()
                .gap_2()
                .min_w_0()
                .child(look::class_badge(
                    system.wormhole_class_id,
                    Some(system.security_status),
                ))
                .child(name.flex().min_w_0().truncate())
                .when_some(system.effect_name.clone(), |d, effect| {
                    let color = look::effect_color(&effect, theme);
                    d.child(side(effect, color))
                })
                .when(system.is_shattered, |d| {
                    d.child(side("Shattered".into(), look::amber_500()))
                }),
        )
        .when_some(
            system.occupying_group.clone().filter(|g| !g.is_empty()),
            |d, group| {
                d.child(
                    small().mt_1().flex().gap_1().child("Occupied by").child(
                        div()
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(theme.foreground)
                            .child(group),
                    ),
                )
            },
        )
        .child(
            small()
                .mt_1()
                .flex()
                .items_center()
                .gap_1()
                .child(system.region.clone())
                .when(!system.constellation.is_empty(), |d| {
                    d.child(format!("· {}", system.constellation))
                })
                .child(div().text_color(theme.border).child("·"))
                .children(links(system).into_iter().map(|(text, url)| {
                    div()
                        .id(ElementId::from(text))
                        .cursor_pointer()
                        .hover(|s| s.text_color(theme.foreground))
                        .on_click(move |_, _, cx| cx.open_url(&url))
                        .child(text)
                })),
        )
        .child(small().mt_1().child(scanning(system, snapshot)))
}

/// Signatures and connections in a line.
fn scanning(system: &PlacedSystem, snapshot: &MapSnapshot) -> String {
    let sigs: Vec<_> = snapshot
        .signatures
        .iter()
        .filter(|s| s.solar_system_id == system.solar_system_id)
        .collect();
    let unknown = sigs
        .iter()
        .filter(|s| s.group.is_empty() || s.group == "unknown")
        .count();
    let holes = snapshot
        .connections
        .iter()
        .filter(|c| c.from_system == system.id || c.to_system == system.id)
        .count();
    let mut line = match sigs.len() {
        1 => "1 signature".to_string(),
        n => format!("{n} signatures"),
    };
    if unknown > 0 {
        let _ = write!(line, ", {unknown} unknown");
    }
    if holes == 1 {
        line.push_str(" · 1 connection");
    } else {
        let _ = write!(line, " · {holes} connections");
    }
    if let Some(at) = system.signatures_pasted_at {
        let _ = write!(line, " · pasted {} ago", ago(at, Utc::now()));
    }
    line
}

/// Each static's code in its destination's color, the destination after it.
fn statics(system: &PlacedSystem, theme: &Theme) -> Option<Div> {
    if system.statics.is_empty() {
        return None;
    }
    let codes = system.statics.iter().map(|s| {
        let (dest, color) = crate::theme::dest_class_meta(s.dest_class);
        div()
            .flex()
            .gap_1()
            .font_family(MONO)
            .text_size(rpx(12.0))
            .font_weight(FontWeight::MEDIUM)
            .text_color(color)
            .child(s.code.clone())
            .child(div().opacity(0.6).child(dest.to_uppercase()))
    });
    Some(
        block(theme).child(
            div()
                .flex()
                .items_center()
                .gap_2()
                .child(look::label("Statics", theme))
                .child(div().flex().gap(rpx(6.0)).children(codes)),
        ),
    )
}

/// One group of an effect's modifiers, two to a line.
fn modifiers(label: &'static str, rows: &[&EffectModifier], color: Hsla, theme: &Theme) -> Div {
    let mut grid = div().flex().flex_wrap().gap_y(rpx(2.0));
    for m in rows {
        grid = grid.child(
            div()
                .w(gpui_kit::relative(0.5))
                .pr(rpx(8.0))
                .flex()
                .items_center()
                .gap_2()
                .text_size(rpx(11.0))
                .child(div().flex_1().min_w_0().truncate().child(m.stat.clone()))
                .child(
                    div()
                        .flex_none()
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(color)
                        .child(m.value.clone()),
                ),
        );
    }
    div()
        .flex()
        .flex_col()
        .gap(rpx(2.0))
        .child(look::label(label, theme))
        .child(grid)
}

/// The effect's buffs and debuffs.
fn effect(intel: Option<&Intel>, theme: &Theme) -> Option<Div> {
    let mods = &intel?.effect;
    if mods.is_empty() {
        return None;
    }
    let buffs: Vec<_> = mods.iter().filter(|m| m.kind == "buff").collect();
    let debuffs: Vec<_> = mods.iter().filter(|m| m.kind != "buff").collect();
    Some(
        block(theme).child(
            div()
                .flex()
                .flex_col()
                .gap_1()
                .child(look::label("Effect", theme))
                .when(!buffs.is_empty(), |d| {
                    d.child(modifiers("Buffs", &buffs, look::green_500(), theme))
                })
                .when(!debuffs.is_empty(), |d| {
                    d.child(modifiers("Debuffs", &debuffs, look::red_500(), theme))
                }),
        ),
    )
}

fn sovereignty(system: &PlacedSystem, images: &Images, theme: &Theme) -> Option<Div> {
    let sov = system.sovereignty.as_ref()?;
    let logo = (!sov.kind.is_empty() && sov.id > 0)
        .then(|| look::picture(images, look::logo(&sov.kind, sov.id), 20.0, theme));
    Some(
        block(theme).child(
            div()
                .flex()
                .flex_col()
                .gap(rpx(6.0))
                .child(look::label("Sovereignty", theme))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .text_size(rpx(12.0))
                        .children(logo)
                        .when_some(sov.ticker.clone(), |d, t| {
                            d.child(
                                div()
                                    .font_weight(FontWeight::MEDIUM)
                                    .child(format!("[{t}]")),
                            )
                        })
                        .child(sov.name.clone()),
                ),
        ),
    )
}

/// A pilot's row, as the website's Pilots card has it, without where: that is here.
fn pilot(p: &Pilot, images: &Images, theme: &Theme) -> impl IntoElement {
    look::row(
        ElementId::from(("pilot", u64::try_from(p.character_id).unwrap_or(0))),
        theme,
    )
    .child(look::picture(
        images,
        look::portrait(p.character_id),
        20.0,
        theme,
    ))
    .child(
        div()
            .flex_none()
            .w(rpx(112.0))
            .flex()
            .items_center()
            .gap_1()
            .child(
                div()
                    .truncate()
                    .when(p.is_mine, |d| {
                        d.font_weight(FontWeight::MEDIUM)
                            .text_color(theme.foreground)
                    })
                    .child(p.name.clone()),
            )
            .when(p.is_docked, |d| {
                d.child(
                    div()
                        .flex_none()
                        .text_size(rpx(10.0))
                        .text_color(theme.muted_foreground)
                        .child("(D)"),
                )
            }),
    )
    .child(
        div()
            .flex_1()
            .min_w_0()
            .flex()
            .items_center()
            .gap(rpx(6.0))
            .when_some(p.ship_type_id, |d, id| {
                d.child(look::picture(images, look::ship(id), 16.0, theme))
            })
            .child(
                div()
                    .min_w_0()
                    .truncate()
                    .font_family(MONO)
                    .text_size(rpx(10.0))
                    .text_color(theme.muted_foreground)
                    .child(p.ship_type.clone().unwrap_or_else(|| "Unknown ship".into())),
            ),
    )
}

/// The map's pilots here.
fn pilots(here: &[&Pilot], images: &Images, theme: &Theme) -> Div {
    let title = div()
        .flex()
        .items_center()
        .gap_2()
        .child(div().size(px(6.0)).rounded_full().bg(look::green_500()))
        .child("PILOTS HERE")
        .child(look::count(here.len()));
    let list = div().flex().flex_col().child(look::band(title, theme));
    if here.is_empty() {
        return list.child(empty("Nobody from the map is here.", theme));
    }
    list.children(here.iter().map(|p| pilot(p, images, theme)))
}

/// A kill's row, as the website's Killmails card has it. The value and the time only where
/// the card is wide enough, as there.
fn kill(k: &Kill, images: &Images, width: f32, theme: &Theme) -> impl IntoElement {
    let mono = |d: Div| d.font_family(MONO).text_size(rpx(10.0));
    let v = &k.victim;
    let ticker = v
        .alliance_ticker
        .clone()
        .or_else(|| v.corporation_ticker.clone());
    let crowd = if k.is_npc {
        theme.muted_foreground.opacity(0.5)
    } else if k.is_solo {
        look::amber_400()
    } else {
        theme.muted_foreground
    };
    let (value, tone, weight) = isk_tone(k.total_value, theme);
    let url = format!("https://zkillboard.com/kill/{}/", k.id);
    look::row(
        ElementId::from(("kill", u64::try_from(k.id).unwrap_or(0))),
        theme,
    )
    .cursor_pointer()
    .on_click(move |_, _, cx| cx.open_url(&url))
    .child(
        div()
            .flex_none()
            .flex()
            .items_center()
            .gap_1()
            .when_some(v.ship_type_id, |d, id| {
                d.child(look::picture(images, look::ship(id), 20.0, theme))
            })
            .when_some(v.character_id, |d, id| {
                d.child(look::picture(images, look::portrait(id), 20.0, theme))
            }),
    )
    .child(
        div()
            .flex_1()
            .min_w_0()
            .flex()
            .items_center()
            .gap_1()
            .child(
                div()
                    .truncate()
                    .child(v.ship_name.clone().unwrap_or_else(|| "Unknown ship".into())),
            )
            .when_some(ticker, |d, t| {
                d.child(
                    mono(div())
                        .flex_none()
                        .text_color(theme.muted_foreground)
                        .child(format!("[{t}]")),
                )
            }),
    )
    .child(
        div()
            .flex_none()
            .flex()
            .items_center()
            .gap(rpx(6.0))
            .when_some(k.final_blow.ship_type_id, |d, id| {
                d.child(look::picture(images, look::ship(id), 16.0, theme))
            })
            .child(
                mono(div())
                    .text_color(crowd)
                    .child(k.attacker_count.to_string()),
            ),
    )
    .when(width >= 380.0, |d| {
        d.child(
            mono(div())
                .flex_none()
                .w(rpx(56.0))
                .flex()
                .justify_end()
                .text_color(tone)
                .font_weight(weight)
                .child(value),
        )
    })
    .when(width >= 480.0, |d| {
        d.child(
            mono(div())
                .flex_none()
                .w(rpx(56.0))
                .flex()
                .justify_end()
                .text_color(theme.muted_foreground.opacity(0.6))
                .child(format!("{} ago", ago(k.time, Utc::now()))),
        )
    })
}

/// The killmails here in the last week.
fn kills(intel: Option<&Intel>, images: &Images, width: f32, theme: &Theme) -> Div {
    let n = intel.map_or(0, |i| i.kills.len());
    let title = div()
        .flex()
        .items_center()
        .gap_2()
        .child("KILLMAILS")
        .when(n > 0, |d| d.child(look::count(n)));
    let list = div().flex().flex_col().child(look::band(title, theme));
    let Some(intel) = intel else {
        return list.child(empty("Loading…", theme));
    };
    if intel.kills.is_empty() {
        return list.child(empty(
            "Nothing has died in this system in the last week.",
            theme,
        ));
    }
    list.children(intel.kills.iter().map(|k| kill(k, images, width, theme)))
}

/// How loudly a loss reads: routine, a good ship, a capital.
fn isk_tone(value: Option<f64>, theme: &Theme) -> (String, Hsla, FontWeight) {
    match value {
        None => (
            "--".into(),
            theme.muted_foreground.opacity(0.6),
            FontWeight::NORMAL,
        ),
        Some(v) if v >= 10e9 => (isk(v), look::red_400(), FontWeight::SEMIBOLD),
        Some(v) if v >= 1e9 => (isk(v), look::amber_400(), FontWeight::SEMIBOLD),
        Some(v) => (isk(v), theme.muted_foreground, FontWeight::NORMAL),
    }
}

fn notes(intel: &Intel, theme: &Theme) -> Option<Div> {
    let notes = intel.notes.clone()?;
    Some(
        div()
            .flex()
            .flex_col()
            .child(look::band("NOTES", theme))
            .child(
                div()
                    .px(rpx(12.0))
                    .py(rpx(8.0))
                    .text_size(rpx(12.0))
                    .child(notes),
            ),
    )
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
    fn writes_isk_like_the_website() {
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
