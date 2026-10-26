//! The pages of the main window, and the sidebar and header around them.

use gpui_kit::component::input::Input;
use gpui_kit::component::switch::Switch;
use gpui_kit::component::{ActiveTheme as _, Theme};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, App, Context, Div, ElementId, FontWeight, Hsla, InteractiveElement, IntoElement,
    KeyDownEvent, ParentElement, Render, SharedString, StatefulInteractiveElement, Styled, Svg,
    Window, div, img, px, svg,
};

use super::{DesktopApp, Level, Page, Updater};
use crate::config::{Corner, Overlay, OverlaySize};
use crate::focus::{self, Focus};
use crate::hotkeys::{self, Action, PICK_MODIFIERS};
use crate::sigs;
use crate::theme::{self, Appearance, HEADING};
use crate::ui::{self, ButtonState, Kind, rpx};

const SIDEBAR: f32 = 220.0;
/// The text size choices, in percent.
const TEXT_SIZES: &[(u8, &str)] = &[
    (90, "Small"),
    (100, "Normal"),
    (115, "Large"),
    (130, "Larger"),
];

/// A page on the website at `server`, which may be typed without its scheme.
fn website(server: &str, path: &str) -> String {
    let server = server.trim().trim_end_matches('/');
    let server = if server.is_empty() {
        crate::config::DEFAULT_SERVER
    } else {
        server
    };
    if server.contains("://") {
        format!("{server}{path}")
    } else {
        format!("https://{server}{path}")
    }
}

/// A framed section with a header band.
fn panel(title: &'static str, theme: &Theme) -> Div {
    ui::section(title, theme)
}

fn icon(path: &'static str, size: f32, color: Hsla) -> Svg {
    svg()
        .path(path)
        .size(rpx(size))
        .flex_none()
        .text_color(color)
}

fn muted(text: impl Into<SharedString>, theme: &Theme) -> Div {
    div()
        .text_sm()
        .text_color(theme.muted_foreground)
        .child(text.into())
}

/// A row of toggle buttons, one per choice, the chosen one filled.
fn segmented<T: Copy + PartialEq + 'static>(
    id: &'static str,
    choices: &[(T, &'static str)],
    current: T,
    cx: &mut Context<'_, DesktopApp>,
    pick: impl Fn(&mut DesktopApp, T, &mut Window, &mut Context<'_, DesktopApp>) + Clone + 'static,
) -> Div {
    let mut row = div().flex().flex_wrap().gap_1();
    for (i, &(value, label)) in choices.iter().enumerate() {
        let pick = pick.clone();
        let kind = if value == current {
            Kind::Selected
        } else {
            Kind::Normal
        };
        let theme = cx.theme().clone();
        row = row.child(
            ui::button(
                ElementId::from((id, i)),
                label,
                kind,
                ButtonState::Enabled,
                &theme,
            )
            .on_click(cx.listener(move |this, _, window, cx| pick(this, value, window, cx))),
        );
    }
    row
}

/// A settings row: the label, and its control at the far end.
fn setting_row(label: &'static str, control: AnyElement) -> Div {
    div()
        .flex()
        .items_center()
        .justify_between()
        .gap_4()
        .text_sm()
        .child(label)
        .child(control)
}

impl DesktopApp {
    /// The sidebar card that says a new version is out.
    fn update_card(&self, theme: &Theme, cx: &mut Context<'_, Self>) -> Option<Div> {
        let available = self.update.as_ref()?;
        let installing = self.updater == Updater::Installing;
        let label = if installing {
            "Updating…"
        } else if available.installer.is_some() {
            "Update and restart"
        } else {
            "Download"
        };
        Some(
            div()
                .mx_3()
                .mb_3()
                .p_3()
                .flex()
                .flex_col()
                .gap_2()
                .rounded(px(4.0))
                .border_1()
                .border_color(theme.primary.opacity(0.5))
                .bg(theme.primary.opacity(0.08))
                .text_xs()
                .child(
                    div()
                        .font_family(HEADING)
                        .font_weight(FontWeight::MEDIUM)
                        .text_sm()
                        .child(format!("Version {} is out", available.version)),
                )
                .child(
                    ui::button(
                        "update-install",
                        label,
                        Kind::Primary,
                        ButtonState::from(!installing),
                        theme,
                    )
                    .on_click(cx.listener(|this, _, _, cx| this.install_update(cx))),
                ),
        )
    }

    fn sidebar(&self, cx: &mut Context<'_, Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let update_card = self.update_card(&theme, cx);
        let nav = self.nav(&theme, cx);
        let status = self.status(&theme, cx);
        let tools = self.tools(&theme, cx);

        div()
            .w(rpx(SIDEBAR))
            .h_full()
            .flex_none()
            .flex()
            .flex_col()
            .bg(theme.sidebar)
            .border_r_1()
            .border_color(theme.sidebar_border)
            .text_color(theme.sidebar_foreground)
            .child(Self::brand(&theme))
            .child(nav)
            .child(div().flex_1())
            .children(update_card)
            .child(status)
            .child(tools)
    }

    fn brand(theme: &Theme) -> Div {
        div()
            .h(rpx(56.0))
            .px_4()
            .flex()
            .items_center()
            .gap_2()
            .border_b_1()
            .border_color(theme.sidebar_border)
            .child(
                svg()
                    .path("logo.svg")
                    .size(rpx(18.0))
                    .text_color(theme.sidebar_foreground),
            )
            .child(
                div()
                    .font_family(HEADING)
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_sm()
                    .child("WormholeSystems"),
            )
            .child(
                div()
                    .px(rpx(5.0))
                    .py(px(1.0))
                    .border_1()
                    .border_color(theme.primary.opacity(0.5))
                    .text_color(theme.primary)
                    .text_size(rpx(9.0))
                    .child(stage()),
            )
    }

    fn nav(&self, theme: &Theme, cx: &mut Context<'_, Self>) -> Div {
        let mut nav = div().flex().flex_col().py_3();
        for page in Page::ALL {
            let active = self.page == page;
            nav = nav.child(
                div()
                    .id(page.label())
                    .relative()
                    .h(rpx(38.0))
                    .px_4()
                    .flex()
                    .items_center()
                    .gap_3()
                    .cursor_pointer()
                    .text_color(if active {
                        theme.sidebar_foreground
                    } else {
                        theme.muted_foreground
                    })
                    .when(active, |d| d.bg(theme.sidebar_accent))
                    .hover(|s| {
                        s.bg(theme.sidebar_accent.opacity(0.6))
                            .text_color(theme.sidebar_foreground)
                    })
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.page = page;
                        cx.notify();
                    }))
                    .when(active, |d| {
                        d.child(
                            div()
                                .absolute()
                                .left_0()
                                .top_0()
                                .bottom_0()
                                .w(px(2.0))
                                .bg(theme.primary),
                        )
                    })
                    .child(icon(
                        page.icon(),
                        16.0,
                        if active {
                            theme.primary
                        } else {
                            theme.muted_foreground
                        },
                    ))
                    .child(
                        div()
                            .font_family(HEADING)
                            .font_weight(FontWeight::MEDIUM)
                            .text_sm()
                            .child(page.label()),
                    ),
            );
        }
        nav
    }

    /// Who pastes and jumps go to: the tracked character, else whoever moved last.
    fn pilot(&self, theme: &Theme) -> Option<Div> {
        self.config
            .tracked
            .clone()
            .or_else(|| self.active.clone())
            .map(|name| {
                let system = self
                    .characters
                    .iter()
                    .find(|(c, _, _)| *c == name)
                    .map(|(_, s, _)| s.clone());
                let face = rpx(32.0);
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .pb_2()
                    .child(match self.portrait(&name) {
                        Some(portrait) => img(portrait)
                            .size(face)
                            .flex_none()
                            .rounded(px(2.0))
                            .into_any_element(),
                        None => div()
                            .size(face)
                            .flex_none()
                            .rounded(px(2.0))
                            .bg(theme.foreground.opacity(0.08))
                            .into_any_element(),
                    })
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .min_w_0()
                            .child(
                                div()
                                    .font_family(HEADING)
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_sm()
                                    .truncate()
                                    .child(name),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .truncate()
                                    .text_color(theme.muted_foreground)
                                    .child(system.unwrap_or_else(|| "Not logged in".into())),
                            ),
                    )
            })
    }

    /// The account itself is under Settings; here only whether it is signed in, and the map.
    fn status(&self, theme: &Theme, cx: &App) -> Div {
        let map_name = self
            .sigs
            .read(cx)
            .snapshot
            .as_ref()
            .map(|s| s.map.name.clone());
        let pilot = self.pilot(theme);
        let signed_in = self.account.is_some();
        div()
            .px_4()
            .py_3()
            .flex()
            .flex_col()
            .gap_1()
            .border_t_1()
            .border_color(theme.sidebar_border)
            .children(pilot)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .text_xs()
                    .child(
                        div()
                            .size(rpx(7.0))
                            .flex_none()
                            .rounded_full()
                            .bg(if signed_in {
                                theme.success
                            } else {
                                theme.muted_foreground
                            }),
                    )
                    .text_color(theme.muted_foreground)
                    .child(div().truncate().child(if signed_in {
                        map_name.unwrap_or_else(|| "No map".into())
                    } else {
                        "Signed out".into()
                    })),
            )
    }

    fn tools(&self, theme: &Theme, cx: &mut Context<'_, Self>) -> Div {
        let overlay_open = self.overlay.is_some();
        let appearance = self.config.appearance;
        let tool = |id: &'static str, path: &'static str, label: &'static str, on: bool| {
            let color = if on {
                theme.primary
            } else {
                theme.muted_foreground
            };
            div()
                .id(id)
                .flex_1()
                .h(rpx(40.0))
                .flex()
                .items_center()
                .justify_center()
                .gap_2()
                .cursor_pointer()
                .text_xs()
                .text_color(color)
                .hover(|s| s.bg(theme.sidebar_accent))
                .child(icon(path, 14.0, color))
                .child(label)
        };
        div()
            .flex()
            .border_t_1()
            .border_color(theme.sidebar_border)
            .child(
                tool(
                    "toggle-overlay",
                    "icons/picture-in-picture-2.svg",
                    "Overlay",
                    overlay_open,
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    if overlay_open {
                        this.close_overlay(cx);
                    } else {
                        this.open_overlay(cx);
                    }
                })),
            )
            .child(div().w(px(1.0)).bg(theme.sidebar_border))
            .child(
                tool("toggle-theme", appearance.icon(), appearance.label(), false).on_click(
                    cx.listener(move |this, _, window, cx| {
                        this.set_appearance(appearance.next(), window, cx);
                    }),
                ),
            )
    }

    fn header(&self, theme: &Theme, cx: &App) -> Div {
        div()
            .flex_none()
            .h(rpx(56.0))
            .px_6()
            .flex()
            .items_center()
            .gap_3()
            .border_b_1()
            .border_color(theme.border)
            .child(
                div()
                    .font_family(HEADING)
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_xl()
                    .child(self.page.label()),
            )
            .child(div().w(px(1.0)).h(rpx(16.0)).bg(theme.border))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .child(muted(self.page.blurb(), theme)),
            )
            .when(self.page != Page::Signatures, |d| {
                d.when_some(self.sigs.read(cx).notice.as_ref(), |d, n| {
                    d.child(
                        div()
                            .flex_none()
                            .max_w(rpx(420.0))
                            .child(ui::notice(n, theme)),
                    )
                })
            })
    }

    fn signatures_page(&self, cx: &mut Context<'_, Self>) -> AnyElement {
        let theme = cx.theme().clone();
        if self.account.is_none() {
            return div()
                .flex_1()
                .flex()
                .items_center()
                .justify_center()
                .child(muted("Sign in under Settings to paste scans.", &theme))
                .into_any_element();
        }
        let keys = |action: Action| {
            self.config
                .hotkeys
                .get(&action)
                .filter(|k| !k.is_empty())
                .map(|k| hotkeys::display(k))
        };
        let hint = match (keys(Action::PasteScan), keys(Action::DeleteMissing)) {
            (Some(paste), Some(sweep)) => format!(
                "In the probe scanner, select all and copy, then press {paste}. {sweep} deletes what the scan no longer sees."
            ),
            (Some(paste), None) => {
                format!("In the probe scanner, select all and copy, then press {paste}.")
            }
            _ => "In the probe scanner, select all and copy, then paste here.".into(),
        };
        div()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .child(
                div()
                    .flex_none()
                    .px_6()
                    .py_2()
                    .border_b_1()
                    .border_color(theme.border)
                    .child(muted(hint, &theme)),
            )
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .child(sigs::panel(&self.sigs, sigs::Place::Page, cx)),
            )
            .into_any_element()
    }

    fn activity_page(&self, cx: &mut Context<'_, Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let last = div()
            .flex()
            .flex_col()
            .gap_2()
            .child(match &self.last_bookmark {
                Some(b) => div()
                    .font_family(HEADING)
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_xl()
                    .text_color(theme.primary)
                    .child(b.clone()),
                None => muted("Nothing copied yet. Jump through a mapped hole.", &theme),
            })
            .when_some(self.last.as_ref(), |d, last| {
                d.child(muted(
                    format!("The way back from {} to {}", last.here, last.back_to),
                    &theme,
                ))
            });

        let mut lines = panel("Log", &theme);
        if self.log.is_empty() {
            lines = lines.child(muted("Nothing yet.", &theme));
        }
        for line in &self.log {
            let (color, mark) = match line.level {
                Level::Info => (theme.muted_foreground, theme.border),
                Level::Done => (theme.foreground, theme.success),
                Level::Problem => (theme.danger, theme.danger),
            };
            lines = lines.child(
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .text_sm()
                    .child(div().size(rpx(6.0)).flex_none().rounded_full().bg(mark))
                    .child(
                        div()
                            .flex_none()
                            .font_family(HEADING)
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .child(line.at.clone()),
                    )
                    .child(div().text_color(color).child(line.text.clone())),
            );
        }

        div()
            .flex()
            .flex_col()
            .gap_4()
            .p_6()
            .child(panel("Last bookmark", &theme).child(last))
            .child(lines)
            .into_any_element()
    }

    fn shortcuts_page(&self, cx: &mut Context<'_, Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let target = match &self.last {
            Some(last) => format!(
                "The hole keys act on the last hole you jumped: {} to {}.",
                last.back_to, last.here
            ),
            None => "The hole keys act on the last hole you jumped.".to_string(),
        };
        let mut list = panel("Keys", &theme).child(muted(
            "Paste and delete act on the system the tracked character is in (Settings, EVE chat logs).",
            &theme,
        ));
        for action in Action::ALL {
            if action == Action::CopyBookmark {
                list = list.child(div().pt_2().child(muted(target.clone(), &theme)));
            }
            let capturing = self.capturing == Some(action);
            let label = if capturing {
                "Press keys…".to_string()
            } else {
                self.config
                    .hotkeys
                    .get(&action)
                    .filter(|h| !h.is_empty())
                    .map_or_else(|| "Not set".into(), |h| hotkeys::display(h))
            };
            let button = ui::button(
                SharedString::from(format!("hotkey-{action:?}")),
                label,
                if capturing {
                    Kind::Selected
                } else {
                    Kind::Normal
                },
                ButtonState::Enabled,
                &theme,
            )
            .on_click(
                cx.listener(move |this, _, window, cx| this.start_capture(action, window, cx)),
            );
            list = list.child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .py_1()
                    .border_b_1()
                    .border_color(theme.border.opacity(0.5))
                    .child(div().text_sm().child(action.label()))
                    .child(button),
            );
        }
        list = list.child(muted(
            "Click a key and press the new one. Esc cancels, Backspace clears.",
            &theme,
        ));
        let picks = panel("Picking the signature", &theme)
            .child(muted(
                "After a jump out of the chain, the overlay numbers the signatures it could have been. Press the number to map the jump through that one, or 0 for a hole nobody scanned. The keys are only taken while a jump waits for its answer.",
                &theme,
            ))
            .child(segmented(
                "pick-modifiers",
                &PICK_MODIFIERS,
                PICK_MODIFIERS
                    .iter()
                    .map(|(m, _)| *m)
                    .find(|m| *m == self.config.pick_modifiers)
                    .unwrap_or(PICK_MODIFIERS[0].0),
                cx,
                |this, modifiers, _, cx| this.set_pick_modifiers(modifiers, cx),
            ));
        if let Some(problem) = &self.hotkeys.problem {
            list = list.child(
                div()
                    .text_sm()
                    .text_color(theme.danger)
                    .child(problem.clone()),
            );
        }
        div()
            .flex()
            .flex_col()
            .gap_4()
            .p_6()
            .max_w(rpx(720.0))
            .child(list)
            .child(picks)
            .into_any_element()
    }

    fn settings_page(&self, cx: &mut Context<'_, Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let account = self.account_panel(&theme, cx);
        let maps = self.maps_panel(&theme, cx);
        let logs = self.logs_panel(&theme, cx);
        let overlay = self.overlay_panel(&theme, cx);
        let looks = self.looks_panel(&theme, cx);
        let about = self.about_panel(&theme, cx);

        div()
            .flex()
            .flex_col()
            .gap_4()
            .p_6()
            .max_w(rpx(720.0))
            .child(account)
            .children(maps)
            .child(logs)
            .child(overlay)
            .child(looks)
            .child(about)
            .into_any_element()
    }

    fn account_panel(&self, theme: &Theme, cx: &mut Context<'_, Self>) -> Div {
        if let Some(name) = &self.account {
            panel("Account", theme).child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(div().text_sm().child(format!(
                        "{name} on {}",
                        self.config.server.trim_start_matches("https://")
                    )))
                    .child(
                        div()
                            .flex()
                            .gap_1()
                            .child(
                                ui::button(
                                    "settings",
                                    "Manage tokens",
                                    Kind::Normal,
                                    ButtonState::Enabled,
                                    theme,
                                )
                                .on_click(cx.listener(
                                    |this, _, _, cx| {
                                        cx.open_url(&website(
                                            &this.config.server,
                                            "/settings/tokens",
                                        ));
                                    },
                                )),
                            )
                            .child(
                                ui::button(
                                    "sign-out",
                                    "Sign out",
                                    Kind::Normal,
                                    ButtonState::Enabled,
                                    theme,
                                )
                                .on_click(cx.listener(|this, _, _, cx| this.sign_out(cx))),
                            ),
                    ),
            )
        } else {
            panel("Account", theme)
                .child(muted(
                    "Make a token on the website under Settings, Tokens, and paste it here.",
                    theme,
                ))
                .child(Input::new(&self.server))
                .child(Input::new(&self.token).mask_toggle())
                .child(
                    div()
                        .flex()
                        .gap_1()
                        .child(
                            ui::button(
                                "connect",
                                if self.connecting {
                                    "Signing in…"
                                } else {
                                    "Sign in"
                                },
                                Kind::Primary,
                                ButtonState::from(!self.connecting),
                                theme,
                            )
                            .on_click(cx.listener(|this, _, _, cx| this.connect(cx))),
                        )
                        .child(
                            ui::button(
                                "get-token",
                                "Make a token",
                                Kind::Normal,
                                ButtonState::Enabled,
                                theme,
                            )
                            .on_click(cx.listener(|this, _, _, cx| {
                                let server = this.server.read(cx).value().to_string();
                                cx.open_url(&website(&server, "/settings/tokens"));
                            })),
                        ),
                )
        }
    }

    fn maps_panel(&self, theme: &Theme, cx: &mut Context<'_, Self>) -> Option<Div> {
        (!self.maps.is_empty()).then(|| {
            let mut row = div().flex().flex_wrap().gap_1();
            for map in &self.maps {
                let id = map.id;
                let kind = if self.config.map_id == Some(id) {
                    Kind::Selected
                } else {
                    Kind::Normal
                };
                row = row.child(
                    ui::button(
                        ElementId::Integer(id.cast_unsigned()),
                        map.name.clone(),
                        kind,
                        ButtonState::Enabled,
                        theme,
                    )
                    .on_click(cx.listener(move |this, _, _, cx| this.pick_map(id, cx))),
                );
            }
            panel("Map", theme).child(row)
        })
    }

    fn logs_panel(&self, theme: &Theme, cx: &mut Context<'_, Self>) -> Div {
        let mut logs = panel("EVE chat logs", theme).child(Input::new(&self.chatlog_dir));
        if self.watcher.is_some() && self.characters.is_empty() {
            logs = logs.child(muted(
                "No Local logs yet. Log in a character, and keep \"Log Chat to File\" on in EVE's chat settings.",
                theme,
            ));
        }
        if !self.characters.is_empty() || self.config.tracked.is_some() {
            // Everyone the logs know, and the tracked one even while logged out.
            let mut names: Vec<String> =
                self.characters.iter().map(|(c, _, _)| c.clone()).collect();
            if let Some(tracked) = &self.config.tracked
                && !names.contains(tracked)
            {
                names.push(tracked.clone());
                names.sort();
            }
            let mut chips = div().flex().flex_wrap().gap_1().child(
                ui::button(
                    "track-any",
                    "Whoever moved last",
                    if self.config.tracked.is_none() {
                        Kind::Selected
                    } else {
                        Kind::Normal
                    },
                    ButtonState::Enabled,
                    theme,
                )
                .on_click(cx.listener(|this, _, _, cx| this.track(None, cx))),
            );
            for (i, name) in names.into_iter().enumerate() {
                let picked = self.config.tracked.as_ref() == Some(&name);
                chips = chips.child(
                    ui::character(
                        ElementId::from(("track-char", i)),
                        name.clone(),
                        self.portrait(&name),
                        if picked { Kind::Selected } else { Kind::Normal },
                        theme,
                    )
                    .on_click(
                        cx.listener(move |this, _, _, cx| this.track(Some(name.clone()), cx)),
                    ),
                );
            }
            logs = logs.child(
                div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .text_sm()
                    .child("Track")
                    .child(chips)
                    .child(muted(
                        "Pastes go to this character's system, and only their jumps are mapped.",
                        theme,
                    )),
            );
        }
        logs = logs.child(setting_row(
            "Copy the bookmark back on every jump",
            Switch::new("copy-on-jump")
                .checked(self.config.copy_on_jump)
                .on_click(cx.listener(|this, checked: &bool, _, cx| {
                    this.config.copy_on_jump = *checked;
                    this.save();
                    cx.notify();
                }))
                .into_any_element(),
        ));
        logs
    }

    fn overlay_panel(&self, theme: &Theme, cx: &mut Context<'_, Self>) -> Div {
        let o = self.config.overlay.clone();
        let overlay = panel("Overlay", theme)
            .child(muted(
                "The signatures in a panel that stays above the game. Drag it by its title and size it by the grip in its corner. It takes clicks but never the keyboard.",
                theme,
            ))
            .child(setting_row(
                "Show the overlay",
                Switch::new("overlay-open")
                    .checked(self.overlay.is_some())
                    .on_click(
                        cx.listener(|this, checked: &bool, _, cx| {
                            if *checked {
                                this.open_overlay(cx);
                            } else {
                                this.close_overlay(cx);
                            }
                        }),
                    )
                    .into_any_element(),
            ))
            .child(setting_row(
                "Position and size",
                ui::button("overlay-reset", "Reset", Kind::Normal, ButtonState::Enabled, theme)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.edit_overlay(cx, |o| {
                            o.corner = Corner::TopRight;
                            o.size = OverlaySize::Medium;
                            (o.x, o.y, o.width, o.height) = (None, None, None, None);
                        });
                    }))
                    .into_any_element(),
            ))
            .child(setting_row(
                "Text size",
                segmented("overlay-text-size", TEXT_SIZES, o.text_size, cx, |this, v, _, cx| {
                    this.edit_overlay(cx, |o| o.text_size = v);
                })
                .into_any_element(),
            ))
            .child(setting_row(
                "Background",
                segmented(
                    "opacity",
                    &[(60u8, "60%"), (75, "75%"), (85, "85%"), (100, "100%")],
                    o.opacity,
                    cx,
                    |this, v, _, cx| this.edit_overlay(cx, |o| o.opacity = v),
                )
                .into_any_element(),
            ))
            .child(setting_row(
                "Show it when a jump needs its signature",
                Switch::new("overlay-on-jump")
                    .checked(o.show_on_jump)
                    .on_click(cx.listener(|this, checked: &bool, _, cx| {
                        let checked = *checked;
                        this.edit_overlay(cx, |o| o.show_on_jump = checked);
                    }))
                    .into_any_element(),
            ))
            .child(setting_row(
                "Show it when a scan is pasted",
                Switch::new("overlay-on-paste")
                    .checked(o.show_on_paste)
                    .on_click(cx.listener(|this, checked: &bool, _, cx| {
                        let checked = *checked;
                        this.edit_overlay(cx, |o| o.show_on_paste = checked);
                    }))
                    .into_any_element(),
            ));

        let mut overlay = overlay.child(setting_row(
            "Only while EVE or this app has focus",
            Switch::new("overlay-focused")
                .checked(o.only_when_focused)
                .on_click(cx.listener(|this, checked: &bool, _, cx| {
                    let checked = *checked;
                    this.edit_overlay(cx, |o| o.only_when_focused = checked);
                }))
                .into_any_element(),
        ));
        if o.only_when_focused && !focus::supported() {
            overlay = overlay.child(muted(
                "This desktop does not say which window has focus, so the overlay stays up. It works on Hyprland and Windows.",
                theme,
            ));
        }
        if o.only_when_focused {
            overlay = self.overlay_characters(overlay, &o, theme, cx);
        }
        overlay
    }

    /// Which characters' clients the overlay shows for, added to its `panel`.
    fn overlay_characters(
        &self,
        panel: Div,
        o: &Overlay,
        theme: &Theme,
        cx: &mut Context<'_, Self>,
    ) -> Div {
        // Everyone the chat logs know, and anyone picked before who is not logged in now.
        let mut names: Vec<String> = self.characters.iter().map(|(c, _, _)| c.clone()).collect();
        for c in &o.characters {
            if !names.contains(c) {
                names.push(c.clone());
            }
        }
        names.sort();
        let mut chips = div().flex().flex_wrap().gap_1().child(
            ui::button(
                "overlay-any",
                "Any character",
                if o.characters.is_empty() {
                    Kind::Selected
                } else {
                    Kind::Normal
                },
                ButtonState::Enabled,
                theme,
            )
            .on_click(
                cx.listener(|this, _, _, cx| this.edit_overlay(cx, |o| o.characters.clear())),
            ),
        );
        for (i, name) in names.into_iter().enumerate() {
            let picked = o.characters.contains(&name);
            chips = chips.child(
                ui::character(
                    ElementId::from(("overlay-char", i)),
                    name.clone(),
                    self.portrait(&name),
                    if picked { Kind::Selected } else { Kind::Normal },
                    theme,
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    let name = name.clone();
                    this.edit_overlay(cx, |o| {
                        if let Some(i) = o.characters.iter().position(|c| *c == name) {
                            o.characters.remove(i);
                        } else {
                            o.characters.push(name);
                        }
                    });
                })),
            );
        }
        let now = match &self.focus {
            Focus::Eve(Some(name)) => format!("In front now: {name}'s client."),
            Focus::Eve(None) => "In front now: EVE, no character yet.".into(),
            Focus::Us => "In front now: this app.".into(),
            Focus::Other => "In front now: another app, so the overlay is hidden.".into(),
            Focus::Unknown => String::new(),
        };
        panel
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .text_sm()
                    .child("For these characters")
                    .child(chips),
            )
            .when(!now.is_empty(), |d| d.child(muted(now, theme)))
    }

    fn looks_panel(&self, theme: &Theme, cx: &mut Context<'_, Self>) -> Div {
        panel("Appearance", theme)
            .child(setting_row(
                "Theme",
                segmented(
                    "appearance",
                    &[
                        (Appearance::System, "System"),
                        (Appearance::Light, "Light"),
                        (Appearance::Dark, "Dark"),
                    ],
                    self.config.appearance,
                    cx,
                    super::DesktopApp::set_appearance,
                )
                .into_any_element(),
            ))
            .child(setting_row(
                "Text size",
                segmented(
                    "text-size",
                    TEXT_SIZES,
                    self.config.text_size,
                    cx,
                    |this, v, _, cx| {
                        this.config.text_size = v;
                        this.save();
                        theme::set_text_size(v, cx);
                        cx.notify();
                    },
                )
                .into_any_element(),
            ))
    }

    fn about_panel(&self, theme: &Theme, cx: &mut Context<'_, Self>) -> Div {
        let status = if self.updater == Updater::Checking {
            "Checking…".to_string()
        } else if let Some(available) = &self.update {
            format!("Version {} is out.", available.version)
        } else {
            self.update_note.clone().unwrap_or_default()
        };
        panel("Version", theme).child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .gap_4()
                .text_sm()
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .child(format!("WormholeSystems {}", crate::update::VERSION))
                        .when(!status.is_empty(), |d| d.child(muted(status, theme))),
                )
                .child(
                    div()
                        .flex()
                        .gap_1()
                        .child(
                            ui::button(
                                "source",
                                "Source code",
                                Kind::Normal,
                                ButtonState::Enabled,
                                theme,
                            )
                            .on_click(|_, _, cx| {
                                cx.open_url(&format!("https://github.com/{}", crate::update::REPO));
                            }),
                        )
                        .child(
                            ui::button(
                                "update-check",
                                "Check now",
                                Kind::Normal,
                                ButtonState::from(self.updater == Updater::Idle),
                                theme,
                            )
                            .on_click(cx.listener(|this, _, _, cx| this.check_for_update(cx))),
                        ),
                ),
        )
    }
}

impl Render for DesktopApp {
    fn render(&mut self, _: &mut Window, cx: &mut Context<'_, Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let page = match self.page {
            Page::Signatures => self.signatures_page(cx),
            Page::Activity => self.activity_page(cx),
            Page::Shortcuts => self.shortcuts_page(cx),
            Page::Settings => self.settings_page(cx),
        };
        let body = if self.page == Page::Signatures {
            page
        } else {
            div()
                .id("page-scroll")
                .flex_1()
                .overflow_y_scroll()
                .child(page)
                .into_any_element()
        };

        div()
            .id("root")
            .track_focus(&self.capture_focus)
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| this.captured(event, cx)))
            .size_full()
            .flex()
            .bg(theme.background)
            .text_color(theme.foreground)
            .font_family(theme::UI)
            .child(self.sidebar(cx))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .flex()
                    .flex_col()
                    .child(self.header(&theme, cx))
                    .child(body),
            )
    }
}

/// The sidebar badge: the release stage while there is one (`0.1.0-alpha.1` is an alpha),
/// else just that this is the desktop app.
fn stage() -> &'static str {
    let version = env!("CARGO_PKG_VERSION");
    if version.contains("alpha") {
        "ALPHA"
    } else if version.contains("beta") {
        "BETA"
    } else {
        "DESKTOP"
    }
}
