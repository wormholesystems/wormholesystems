//! The main window: a sidebar and four pages. Signatures shows the system you are in and
//! what the last paste did, Activity what happened, Shortcuts the global keys and Settings
//! the rest. Where you are comes from the chat logs; the shortcuts paste scans, sweep what a
//! scan no longer sees, say which signature a jump went through, name systems and mark the
//! hole flown last.

mod pages;

use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::*;

use crate::alias;
use crate::api::{Client, MapEntry, MapSnapshot, SignatureCatalog, SignatureEdit};
use crate::chatlog::{self, ChatEvent, Watcher};
use crate::config::Config;
use crate::focus::{self, Focus};
use crate::fresh::Freshness;
use crate::hotkeys::{self, Action, Press, Registry};
use crate::jump::{self, AliasScheme, Grid};
use crate::naming::{BookmarkContext, BookmarkFormats, BookmarkSystem, format_bookmark};
use crate::overlay;
use crate::plan::{
    JumpContext, LastJump, Plan, aliases, bookmark_for, landed_on, plan_jump, submit,
};
use crate::scan;
use crate::sigs::{Ask, Command, Here, Review, Sigs, Tone};
use crate::theme::{self, Appearance};
use crate::ui::{self, Mood, Notice};

/// How often the map and its signatures are fetched again.
const REFRESH: Duration = Duration::from_secs(5);
const LOG_LINES: usize = 60;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Page {
    Signatures,
    Activity,
    Shortcuts,
    Settings,
}

impl Page {
    const ALL: [Page; 4] = [
        Page::Signatures,
        Page::Activity,
        Page::Shortcuts,
        Page::Settings,
    ];

    fn label(self) -> &'static str {
        match self {
            Page::Signatures => "Signatures",
            Page::Activity => "Activity",
            Page::Shortcuts => "Shortcuts",
            Page::Settings => "Settings",
        }
    }

    fn icon(self) -> &'static str {
        match self {
            Page::Signatures => "icons/radar.svg",
            Page::Activity => "icons/activity.svg",
            Page::Shortcuts => "icons/keyboard.svg",
            Page::Settings => "icons/settings.svg",
        }
    }

    fn blurb(self) -> &'static str {
        match self {
            Page::Signatures => "The system you are in, and what the last paste did.",
            Page::Activity => "Jumps, bookmarks and marks, newest first.",
            Page::Shortcuts => "Keys that work while the game has focus.",
            Page::Settings => "Account, map, chat logs, overlay and looks.",
        }
    }
}

enum Level {
    Info,
    Done,
    Problem,
}

struct LogLine {
    at: String,
    text: String,
    level: Level,
}

pub struct DesktopApp {
    config: Config,
    page: Page,
    sigs: Entity<Sigs>,
    server: Entity<InputState>,
    token: Entity<InputState>,
    chatlog_dir: Entity<InputState>,
    account: Option<String>,
    /// Built on connect and dropped on sign-out, so requests share its connections.
    client: Option<Client>,
    /// Which snapshot is newest, so a slow refresh never puts an older map back.
    fresh: Freshness,
    maps: Vec<MapEntry>,
    connecting: bool,
    /// Wormhole codes by signature type, for the `{wh}` token.
    codes: Arc<HashMap<i64, String>>,
    catalog: Arc<SignatureCatalog>,
    grid: Grid,
    /// Which systems each system has a stargate to, so gate travel is never mapped.
    stargates: Arc<HashMap<i64, Vec<i64>>>,
    /// The website's "suggest an alias" setting for this map.
    suggest_alias: bool,
    watcher: Option<Arc<Mutex<Watcher>>>,
    /// This machine's characters: name, system, id.
    characters: Vec<(String, String, Option<i64>)>,
    /// Portraits by character name, from EVE's image server; `None` while one loads or
    /// when it could not be had.
    portraits: HashMap<String, Option<Arc<Image>>>,
    /// The character whose system a paste goes to: the tracked one, or whoever moved last.
    active: Option<String>,
    last: Option<LastJump>,
    last_bookmark: Option<String>,
    /// A jump is being looked into or mapped; the next waits its turn.
    thinking: bool,
    /// Jumps that landed while another was being asked about, oldest first.
    queued: VecDeque<(String, String, String)>,
    alias_window: Option<AnyWindowHandle>,
    log: VecDeque<LogLine>,
    capturing: Option<Action>,
    notice_seq: u64,
    focus: Focus,
    focus_rx: Option<std::sync::mpsc::Receiver<Focus>>,
    capture_focus: FocusHandle,
    hotkeys: Registry,
    clipboard: Option<arboard::Clipboard>,
    overlay: Option<AnyWindowHandle>,
    /// A newer release, from the last check.
    update: Option<crate::update::Available>,
    checking_update: bool,
    installing_update: bool,
    /// What the last check said when it found nothing or failed, for Settings.
    update_note: Option<String>,
    _tasks: Vec<Task<()>>,
    _subscriptions: Vec<Subscription>,
}

impl DesktopApp {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let (config, config_problem) = Config::load();
        let tracked = config.tracked.clone();
        theme::set_text_size(config.text_size, cx);
        theme::apply(config.appearance, window, cx);

        let server = cx.new(|cx| InputState::new(window, cx).default_value(config.server.clone()));
        let token = cx.new(|cx| {
            InputState::new(window, cx)
                .masked(true)
                .placeholder("wst_…")
                .default_value(config.token.clone())
        });
        let found = config.chatlog_dir.clone().or_else(chatlog::default_dir);
        let chatlog_dir = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Documents/EVE/logs/Chatlogs")
                .default_value(
                    found
                        .as_ref()
                        .map(|p| p.display().to_string())
                        .unwrap_or_default(),
                )
        });
        let sigs = cx.new(|_| Sigs {
            overlay: config.overlay.clone(),
            list: config.list.clone(),
            pick_prefix: hotkeys::pick_prefix(&config.pick_modifiers),
            ..Default::default()
        });

        let subscriptions = vec![
            cx.subscribe_in(
                &chatlog_dir,
                window,
                |this, _, event: &InputEvent, _, cx| {
                    if matches!(event, InputEvent::PressEnter { .. } | InputEvent::Blur) {
                        this.use_chatlog_dir(cx);
                    }
                },
            ),
            cx.observe(&sigs, |_, _, cx| cx.notify()),
            cx.subscribe(&sigs, |this, _, command: &Command, cx| {
                this.command(command.clone(), cx)
            }),
            // Follow the system's light or dark when asked to.
            cx.observe_window_appearance(window, |this, window, cx| {
                if this.config.appearance == Appearance::System {
                    theme::apply(Appearance::System, window, cx);
                }
            }),
        ];

        let mut hotkeys = Registry::new();
        let refused = hotkeys.apply(&config.hotkeys);
        let clipboard = arboard::Clipboard::new().ok();
        let open_overlay = config.overlay.open;

        let mut this = DesktopApp {
            config,
            page: Page::Signatures,
            sigs,
            server,
            token,
            chatlog_dir,
            account: None,
            client: None,
            fresh: Freshness::default(),
            maps: Vec::new(),
            connecting: false,
            codes: Arc::default(),
            catalog: Arc::default(),
            grid: Grid::default(),
            stargates: Arc::default(),
            suggest_alias: false,
            watcher: None,
            characters: Vec::new(),
            portraits: HashMap::new(),
            active: tracked,
            last: None,
            last_bookmark: None,
            thinking: false,
            queued: VecDeque::new(),
            alias_window: None,
            log: VecDeque::new(),
            capturing: None,
            notice_seq: 0,
            focus: Focus::Unknown,
            focus_rx: Some(focus::watch()),
            capture_focus: cx.focus_handle(),
            hotkeys,
            clipboard,
            overlay: None,
            update: None,
            checking_update: false,
            installing_update: false,
            update_note: None,
            _tasks: Vec::new(),
            _subscriptions: subscriptions,
        };
        if let Some(problem) = config_problem {
            this.note(Level::Problem, problem);
        }
        if let Some(problem) = this.hotkeys.problem.clone() {
            this.note(Level::Problem, problem);
        }
        for (action, why) in refused {
            this.note(Level::Problem, format!("{}: {why}", action.label()));
        }
        if this.clipboard.is_none() {
            this.note(
                Level::Problem,
                "No clipboard to copy to on this system".into(),
            );
        }
        if let Some(dir) = found {
            this.watch(dir);
        } else {
            this.note(
                Level::Problem,
                "Could not find EVE's chat logs; set the folder in Settings".into(),
            );
        }
        this._tasks.push(this.chatlog_loop(cx));
        this._tasks.push(this.hotkey_loop(cx));
        this._tasks.push(this.focus_loop(cx));
        this._tasks.push(this.refresh_loop(cx));
        this._tasks.push(this.update_loop(cx));
        if !this.config.token.is_empty() {
            this.connect(cx);
        } else {
            this.page = Page::Settings;
        }
        if open_overlay {
            this.set_overlay(true, cx);
        }
        this
    }

    fn note(&mut self, level: Level, text: String) {
        let at = chrono::Utc::now().format("%H:%M:%S").to_string();
        self.log.push_front(LogLine { at, text, level });
        self.log.truncate(LOG_LINES);
    }

    fn save(&mut self) {
        if let Err(err) = self.config.save() {
            self.note(
                Level::Problem,
                format!("Could not save the settings: {err}"),
            );
        }
    }

    fn client(&self) -> Option<Client> {
        self.client.clone()
    }

    /// A map that came back from a change: newer than any refresh still out.
    fn show_changed(&mut self, snapshot: MapSnapshot, cx: &mut Context<Self>) {
        self.fresh.changed();
        self.sigs.update(cx, |sigs, cx| {
            sigs.snapshot = Some(Arc::new(snapshot));
            cx.notify();
        });
    }

    fn copy(&mut self, text: &str) -> bool {
        match self
            .clipboard
            .as_mut()
            .map(|c| c.set_text(text.to_string()))
        {
            Some(Ok(())) => true,
            Some(Err(err)) => {
                self.note(Level::Problem, format!("Could not copy: {err}"));
                false
            }
            None => false,
        }
    }

    // Account and map.

    fn connect(&mut self, cx: &mut Context<Self>) {
        self.config.server = self.server.read(cx).value().trim().to_string();
        self.config.token = self.token.read(cx).value().trim().to_string();
        self.save();
        self.client = (!self.config.token.is_empty())
            .then(|| Client::new(&self.config.server, &self.config.token));
        let Some(client) = self.client() else {
            self.note(Level::Problem, "Paste a token first".into());
            return;
        };
        self.connecting = true;
        cx.notify();
        let fetch = cx.background_spawn(async move {
            let me = client.me()?;
            let maps = client.maps()?;
            let catalog = client.catalog()?;
            let grid = client.grid()?;
            let stargates = client.stargates()?;
            anyhow::Ok((me, maps, catalog, grid, stargates))
        });
        cx.spawn(async move |this, cx| {
            let result = fetch.await;
            this.update(cx, |this, cx| {
                this.connecting = false;
                match result {
                    Ok((me, maps, catalog, grid, stargates)) => {
                        this.note(Level::Done, format!("Signed in as {}", me.name));
                        this.account = Some(me.name);
                        this.grid = grid;
                        this.stargates = Arc::new(stargates);
                        this.codes = Arc::new(
                            catalog
                                .types
                                .iter()
                                .filter_map(|t| Some((t.id, t.signature.clone()?)))
                                .collect(),
                        );
                        this.catalog = Arc::new(catalog);
                        let catalog = this.catalog.clone();
                        this.sigs.update(cx, |sigs, cx| {
                            sigs.catalog = catalog;
                            cx.notify();
                        });
                        if this
                            .config
                            .map_id
                            .is_none_or(|id| !maps.iter().any(|m| m.id == id))
                        {
                            this.config.map_id = maps.first().map(|m| m.id);
                            this.save();
                        }
                        this.maps = maps;
                        if this.page == Page::Settings && this.config.map_id.is_some() {
                            this.page = Page::Signatures;
                        }
                        this.load_map_settings(cx);
                        this.refresh(cx);
                    }
                    Err(err) => {
                        this.account = None;
                        this.note(Level::Problem, format!("Could not sign in: {err}"));
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn sign_out(&mut self, cx: &mut Context<Self>) {
        self.config.token.clear();
        self.client = None;
        self.account = None;
        self.maps.clear();
        self.last = None;
        self.save();
        self.clear_map(cx);
        self.drop_jumps(cx);
        cx.notify();
    }

    fn clear_map(&mut self, cx: &mut Context<Self>) {
        // Refreshes still out are for the map that was.
        self.fresh.changed();
        self.sigs.update(cx, |sigs, cx| {
            sigs.snapshot = None;
            sigs.review = None;
            sigs.notice = None;
            cx.notify();
        });
    }

    fn pick_map(&mut self, id: i64, cx: &mut Context<Self>) {
        self.config.map_id = Some(id);
        self.last = None;
        self.save();
        self.clear_map(cx);
        self.drop_jumps(cx);
        self.load_map_settings(cx);
        self.refresh(cx);
        cx.notify();
    }

    /// Fetch the map and its signatures now.
    fn refresh(&mut self, cx: &mut Context<Self>) {
        let (Some(client), Some(map_id)) = (self.client(), self.config.map_id) else {
            return;
        };
        if self.account.is_none() {
            return;
        }
        let n = self.fresh.start();
        let fetch = cx.background_spawn(async move { client.snapshot(map_id) });
        cx.spawn(async move |this, cx| match fetch.await {
            Ok(snapshot) => {
                this.update(cx, |this, cx| {
                    if !this.fresh.show(n) {
                        return;
                    }
                    let snapshot = Arc::new(snapshot);
                    this.mapped_elsewhere(&snapshot, cx);
                    this.sigs.update(cx, |sigs, cx| {
                        sigs.snapshot = Some(snapshot);
                        cx.notify();
                    });
                })
                .ok();
            }
            Err(err) => {
                this.update(cx, |this, cx| {
                    this.note(Level::Problem, format!("Could not read the map: {err}"));
                    cx.notify();
                })
                .ok();
            }
        })
        .detach();
    }

    /// [`Self::refresh`] after changing the map: a refresh already out may have read it
    /// before the change.
    fn refresh_changed(&mut self, cx: &mut Context<Self>) {
        self.fresh.changed();
        self.refresh(cx);
    }

    /// The website's per-user settings for the map that the app follows.
    fn load_map_settings(&mut self, cx: &mut Context<Self>) {
        let (Some(client), Some(map_id)) = (self.client(), self.config.map_id) else {
            return;
        };
        let fetch = cx.background_spawn(async move { client.user_settings(map_id) });
        cx.spawn(async move |this, cx| {
            let result = fetch.await;
            this.update(cx, |this, cx| match result {
                Ok(settings) => {
                    this.suggest_alias = settings.suggest_alias;
                    this.sigs.update(cx, |sigs, cx| {
                        sigs.statics_first = settings.show_statics_first;
                        cx.notify();
                    });
                }
                Err(err) => this.note(
                    Level::Problem,
                    format!("Could not read your map settings: {err}"),
                ),
            })
            .ok();
        })
        .detach();
    }

    fn refresh_loop(&self, cx: &mut Context<Self>) -> Task<()> {
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(REFRESH).await;
                if this.update(cx, |this, cx| this.refresh(cx)).is_err() {
                    return;
                }
            }
        })
    }

    // Chat logs.

    fn watch(&mut self, dir: PathBuf) {
        self.note(Level::Info, format!("Watching {}", dir.display()));
        self.watcher = Some(Arc::new(Mutex::new(Watcher::new(dir))));
        self.characters.clear();
    }

    fn use_chatlog_dir(&mut self, cx: &mut Context<Self>) {
        let text = self.chatlog_dir.read(cx).value().trim().to_string();
        let dir = PathBuf::from(&text);
        if self
            .watcher
            .as_ref()
            .is_some_and(|w| w.lock().unwrap().dir() == dir)
        {
            return;
        }
        if !dir.is_dir() {
            self.note(Level::Problem, format!("{text} is not a folder"));
            cx.notify();
            return;
        }
        self.config.chatlog_dir = Some(dir.clone());
        self.save();
        self.watch(dir);
        cx.notify();
    }

    fn chatlog_loop(&self, cx: &mut Context<Self>) -> Task<()> {
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(500))
                    .await;
                let Ok(watcher) = this.read_with(cx, |this, _| this.watcher.clone()) else {
                    return;
                };
                let Some(watcher) = watcher else { continue };
                let polled = cx
                    .background_spawn(async move {
                        let mut watcher = watcher.lock().unwrap();
                        let events = watcher.poll();
                        let mut seen: Vec<(String, String, Option<i64>)> = watcher
                            .locations()
                            .map(|(c, s, id)| (c.to_string(), s.to_string(), id))
                            .collect();
                        seen.sort();
                        events.map(|events| (events, seen))
                    })
                    .await;
                let alive = this.update(cx, |this, cx| match polled {
                    Ok((events, seen)) => {
                        let moved = seen != this.characters;
                        if moved {
                            this.characters = seen;
                            this.fetch_portraits(cx);
                            cx.notify();
                        }
                        // Oldest first, so the last one is whoever moved last.
                        if this.config.tracked.is_none()
                            && let Some(event) = events.last()
                        {
                            let character = match event {
                                ChatEvent::Located { character, .. }
                                | ChatEvent::Jumped { character, .. } => character.clone(),
                            };
                            this.active = Some(character);
                        }
                        if moved {
                            this.locate(cx);
                        }
                        for event in events {
                            if let ChatEvent::Jumped {
                                character,
                                from,
                                to,
                            } = event
                                && this
                                    .config
                                    .tracked
                                    .as_ref()
                                    .is_none_or(|tracked| *tracked == character)
                            {
                                this.jumped(character, from, to, cx);
                            }
                        }
                        if moved {
                            this.refresh(cx);
                        }
                    }
                    Err(err) => {
                        this.note(
                            Level::Problem,
                            format!("Could not read the chat logs: {err}"),
                        );
                        this.watcher = None;
                        cx.notify();
                    }
                });
                if alive.is_err() {
                    return;
                }
            }
        })
    }

    /// Follow one character, or `None` for whoever moves.
    fn track(&mut self, character: Option<String>, cx: &mut Context<Self>) {
        if self.config.tracked == character {
            return;
        }
        self.config.tracked = character.clone();
        self.save();
        if character.is_some() {
            self.active = character;
        }
        self.locate(cx);
        self.refresh(cx);
        cx.notify();
    }

    /// Tell the signatures where you are now.
    fn locate(&mut self, cx: &mut Context<Self>) {
        let here = self.active.as_ref().and_then(|active| {
            self.characters
                .iter()
                .find(|(name, _, _)| name == active)
                .map(|(name, system, _)| Here {
                    character: name.clone(),
                    system: system.clone(),
                })
        });
        self.sigs.update(cx, |sigs, cx| {
            if sigs.here != here {
                sigs.here = here;
                sigs.notice = None;
                cx.notify();
            }
        });
    }

    // Jumps.

    fn jumped(&mut self, character: String, from: String, to: String, cx: &mut Context<Self>) {
        self.note(Level::Info, format!("{character}: {from} → {to}"));
        cx.notify();
        // One at a time, in order: the next jump usually starts where this one lands, which
        // is only on the map once this one is answered.
        if self.thinking || self.sigs.read(cx).ask.is_some() {
            self.queued.push_back((character, from, to));
            return;
        }
        self.consider(character, from, to, cx);
    }

    /// Work out what a jump means for the map, then map it, ask about it or let it be.
    fn consider(&mut self, character: String, from: String, to: String, cx: &mut Context<Self>) {
        let (Some(client), Some(map_id)) = (self.client(), self.config.map_id) else {
            return;
        };
        if self.account.is_none() {
            return;
        }
        self.thinking = true;
        let context = self.jump_context();
        let work = cx.background_spawn(async move {
            plan_jump(&client, &context, map_id, &character, &from, &to)
        });
        cx.spawn(async move |this, cx| {
            let plan = work.await;
            this.update(cx, |this, cx| {
                this.thinking = false;
                match plan {
                    Ok(Plan::Skip(why)) => this.note(Level::Info, why),
                    Ok(Plan::Known(jump, bookmark)) => this.landed(jump, bookmark, None, cx),
                    Ok(Plan::Mapped(jump, bookmark)) => {
                        let what = format!("Mapped {} from {}", jump.here, jump.back_to);
                        this.landed(jump, bookmark, Some(what), cx)
                    }
                    Ok(Plan::Ask(ask)) => return this.ask(*ask, cx),
                    Err(err) => this.note(Level::Problem, format!("Could not map the jump: {err}")),
                }
                this.refresh_changed(cx);
                this.next_jump(cx);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn jump_context(&self) -> JumpContext {
        JumpContext {
            catalog: self.catalog.clone(),
            codes: self.codes.clone(),
            grid: self.grid,
            stargates: self.stargates.clone(),
            suggest_alias: self.suggest_alias,
        }
    }

    fn next_jump(&mut self, cx: &mut Context<Self>) {
        if self.thinking || self.sigs.read(cx).ask.is_some() {
            return;
        }
        if let Some((character, from, to)) = self.queued.pop_front() {
            self.consider(character, from, to, cx);
        }
    }

    /// Ask which signature the jump went through, in the panels and with the pick keys.
    fn ask(&mut self, ask: Ask, cx: &mut Context<Self>) {
        if let Some(why) = self.hotkeys.set_picks(Some(&self.config.pick_modifiers)) {
            self.note(Level::Problem, format!("Pick keys: {why}"));
        }
        let text = format!(
            "{} jumped from {} to {}: which signature was it?",
            ask.character, ask.origin.name, ask.target.name
        );
        self.sigs.update(cx, |sigs, cx| {
            sigs.ask = Some(ask);
            cx.notify();
        });
        if self.config.overlay.show_on_jump && self.overlay.is_none() {
            self.set_overlay(true, cx);
        }
        self.note(Level::Info, text);
        cx.notify();
    }

    fn clear_ask(&mut self, cx: &mut Context<Self>) -> Option<Ask> {
        self.hotkeys.set_picks(None);
        self.sigs.update(cx, |sigs, cx| {
            cx.notify();
            sigs.ask.take()
        })
    }

    /// Forget every jump not yet mapped: the map they were for is gone.
    fn drop_jumps(&mut self, cx: &mut Context<Self>) {
        self.clear_ask(cx);
        self.queued.clear();
    }

    /// The answer: the jump went through the option at `index`, or through a hole nobody
    /// scanned.
    fn pick(&mut self, index: Option<usize>, cx: &mut Context<Self>) {
        let Some(ask) = self.sigs.read(cx).ask.clone() else {
            return;
        };
        let picked = match index {
            Some(i) => match ask.options.get(i) {
                Some(sig) => Some(sig.clone()),
                None => return,
            },
            None => None,
        };
        let Some(client) = self.client() else { return };
        self.clear_ask(cx);
        self.thinking = true;
        let catalog = self.catalog.clone();
        let codes = self.codes.clone();
        let work = cx.background_spawn(async move {
            submit(&client, &catalog, &codes, &ask, picked.as_ref(), false)
        });
        cx.spawn(async move |this, cx| {
            let result = work.await;
            this.update(cx, |this, cx| {
                this.thinking = false;
                match result {
                    Ok(Some((jump, bookmark))) => {
                        let what = format!("Mapped {} from {}", jump.here, jump.back_to);
                        this.landed(jump, bookmark, Some(what), cx);
                    }
                    Ok(None) => this.note(
                        Level::Problem,
                        "Mapped, but the hole is not on the map now".into(),
                    ),
                    Err(err) => this.note(Level::Problem, format!("Could not map the jump: {err}")),
                }
                this.refresh_changed(cx);
                this.next_jump(cx);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn skip_jump(&mut self, cx: &mut Context<Self>) {
        if let Some(ask) = self.clear_ask(cx) {
            self.note(
                Level::Info,
                format!(
                    "Left {} to {} off the map",
                    ask.origin.name, ask.target.name
                ),
            );
        }
        self.next_jump(cx);
        cx.notify();
    }

    /// A jump now on the map: the hole keys act on it, and its way back is copied.
    fn landed(
        &mut self,
        jump: LastJump,
        bookmark: String,
        what: Option<String>,
        cx: &mut Context<Self>,
    ) {
        let copied = self.config.copy_on_jump && !bookmark.is_empty() && self.copy(&bookmark);
        let mut text = what.map(|w| format!("{w}. ")).unwrap_or_default();
        text += &if copied {
            format!("Copied “{bookmark}”, the way back to {}", jump.back_to)
        } else {
            format!("The way back to {} is “{bookmark}”", jump.back_to)
        };
        self.tell(Tone::Done, text, cx);
        self.last_bookmark = Some(bookmark);
        self.last = Some(jump);
    }

    /// Someone mapped the jump being asked about (the website's own prompt, or a fleet
    /// mate): the question is answered.
    fn mapped_elsewhere(&mut self, snapshot: &MapSnapshot, cx: &mut Context<Self>) {
        let Some(ask) = self.sigs.read(cx).ask.clone() else {
            return;
        };
        let Some(found) = landed_on(
            snapshot,
            ask.map_id,
            &ask.origin.name,
            &ask.target.name,
            &self.codes,
        ) else {
            return;
        };
        if snapshot.signature_on(found.0.connection_id).is_none() {
            return;
        }
        self.clear_ask(cx);
        let (jump, bookmark) = found;
        let what = format!("{} was mapped on the website", jump.here);
        self.landed(jump, bookmark, Some(what), cx);
        self.next_jump(cx);
    }

    // Focus.

    /// Follow which window has focus, hiding the overlay while it should not be up.
    fn focus_loop(&self, cx: &mut Context<Self>) -> Task<()> {
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(100))
                    .await;
                let alive = this.update(cx, |this, cx| {
                    let latest = this.focus_rx.as_ref().and_then(|rx| rx.try_iter().last());
                    if let Some(focus) = latest.filter(|f| *f != this.focus) {
                        this.focus = focus;
                        this.apply_focus(cx);
                    }
                });
                if alive.is_err() {
                    return;
                }
            }
        })
    }

    // Updates.

    /// Look for a new release now, and again every few hours.
    fn update_loop(&self, cx: &mut Context<Self>) -> Task<()> {
        cx.spawn(async move |this, cx| {
            loop {
                if this
                    .update(cx, |this, cx| this.check_for_update(cx))
                    .is_err()
                {
                    return;
                }
                cx.background_executor()
                    .timer(Duration::from_secs(6 * 60 * 60))
                    .await;
            }
        })
    }

    fn check_for_update(&mut self, cx: &mut Context<Self>) {
        if self.checking_update || self.installing_update {
            return;
        }
        self.checking_update = true;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let found = cx.background_spawn(async { crate::update::check() }).await;
            this.update(cx, |this, cx| {
                this.checking_update = false;
                match found {
                    Ok(Some(available)) => {
                        this.update = Some(available);
                        this.update_note = None;
                    }
                    Ok(None) => {
                        this.update = None;
                        this.update_note = Some("This is the newest version.".into());
                    }
                    Err(err) => this.update_note = Some(err),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Install in place where this copy can be, else open the release page.
    fn install_update(&mut self, cx: &mut Context<Self>) {
        let Some(available) = self.update.clone() else {
            return;
        };
        let Some(installer) = available.installer else {
            cx.open_url(&available.page);
            return;
        };
        if self.installing_update {
            return;
        }
        self.installing_update = true;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let done = cx
                .background_spawn(async move { crate::update::install(&installer) })
                .await;
            this.update(cx, |this, cx| match done {
                Ok(()) => crate::update::restart(cx),
                Err(err) => {
                    this.installing_update = false;
                    this.tell(Tone::Problem, format!("Could not update: {err}"), cx);
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }

    fn apply_focus(&mut self, cx: &mut Context<Self>) {
        let o = &self.config.overlay;
        let hidden = o.only_when_focused && !self.focus.shows_overlay(&o.characters);
        self.sigs.update(cx, |sigs, cx| {
            if sigs.overlay_hidden != hidden {
                sigs.overlay_hidden = hidden;
                cx.notify();
            }
        });
        cx.notify();
    }

    // Shortcuts.

    fn hotkey_loop(&self, cx: &mut Context<Self>) -> Task<()> {
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(50))
                    .await;
                let alive = this.update(cx, |this, cx| {
                    for press in this.hotkeys.pressed() {
                        match press {
                            Press::Action(action) => this.run(action, cx),
                            Press::Pick(0) => this.pick(None, cx),
                            Press::Pick(n) => this.pick(Some(n as usize - 1), cx),
                        }
                    }
                });
                if alive.is_err() {
                    return;
                }
            }
        })
    }

    fn run(&mut self, action: Action, cx: &mut Context<Self>) {
        match action {
            Action::PasteScan => return self.paste(cx),
            Action::DeleteMissing => return self.delete_missing(cx),
            Action::ToggleOverlay => return self.set_overlay(self.overlay.is_none(), cx),
            Action::SetAlias => return self.open_alias(cx),
            _ => {}
        }
        if self.sigs.read(cx).ask.is_some() {
            let keys = format!("{0}1 to {0}9", self.sigs.read(cx).pick_prefix);
            return self.tell(
                Tone::Problem,
                format!(
                    "{}: say which signature the jump was first ({keys})",
                    action.label()
                ),
                cx,
            );
        }
        let (Some(client), Some(last)) = (self.client(), self.last.clone()) else {
            self.note(
                Level::Problem,
                format!("{}: no jump to act on yet", action.label()),
            );
            cx.notify();
            return;
        };
        let codes = self.codes.clone();
        let change = action.change();
        let work = cx.background_spawn(async move {
            if let Some(change) = &change {
                client.set_status(last.map_id, last.connection_id, change)?;
            }
            let snapshot = client.snapshot(last.map_id)?;
            anyhow::Ok(bookmark_for(&snapshot, &last, &codes))
        });
        cx.spawn(async move |this, cx| {
            let result = work.await;
            this.update(cx, |this, cx| {
                match result {
                    Ok(bookmark) => {
                        let copied = bookmark.as_deref().is_some_and(|b| this.copy(b));
                        let what = match (action, copied, &bookmark) {
                            (Action::CopyBookmark, true, Some(b)) => format!("Copied “{b}”"),
                            (_, true, Some(b)) => format!("{}, copied “{b}”", action.label()),
                            _ => action.label().to_string(),
                        };
                        if let Some(b) = bookmark {
                            this.last_bookmark = Some(b);
                        }
                        this.note(Level::Done, what);
                    }
                    Err(err) => this.note(Level::Problem, format!("{}: {err}", action.label())),
                }
                this.refresh_changed(cx);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn start_capture(&mut self, action: Action, window: &mut Window, cx: &mut Context<Self>) {
        self.capturing = Some(action);
        window.focus(&self.capture_focus, cx);
        cx.notify();
    }

    fn captured(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) {
        let Some(action) = self.capturing else { return };
        let key = event.keystroke.key.as_str();
        if key == "escape" {
            self.capturing = None;
        } else if key == "backspace" || key == "delete" {
            // Empty rather than gone, so the default does not come back on the next start.
            self.config.hotkeys.insert(action, String::new());
            self.capturing = None;
            self.rebind();
        } else if let Some(hotkey) = hotkeys::from_keystroke(&event.keystroke) {
            // A shortcut does one thing: taking it away from another action is the point.
            for h in self.config.hotkeys.values_mut() {
                if *h == hotkey {
                    h.clear();
                }
            }
            self.config.hotkeys.insert(action, hotkey);
            self.capturing = None;
            self.rebind();
        } else {
            return;
        }
        cx.stop_propagation();
        cx.notify();
    }

    fn set_pick_modifiers(&mut self, modifiers: &str, cx: &mut Context<Self>) {
        self.config.pick_modifiers = modifiers.to_string();
        self.save();
        let prefix = hotkeys::pick_prefix(modifiers);
        let asking = self.sigs.update(cx, |sigs, cx| {
            sigs.pick_prefix = prefix;
            cx.notify();
            sigs.ask.is_some()
        });
        if asking && let Some(why) = self.hotkeys.set_picks(Some(modifiers)) {
            self.note(Level::Problem, format!("Pick keys: {why}"));
        }
        cx.notify();
    }

    fn rebind(&mut self) {
        for (action, why) in self.hotkeys.apply(&self.config.hotkeys) {
            self.note(Level::Problem, format!("{}: {why}", action.label()));
        }
        self.save();
    }

    // Signatures.

    fn command(&mut self, command: Command, cx: &mut Context<Self>) {
        match command {
            Command::Paste => self.paste(cx),
            Command::DeleteMissing => self.delete_missing(cx),
            Command::Delete(key) => self.delete(vec![key], "Deleted the signature".into(), cx),
            Command::Edit(key, edit) => self.edit_signature(key, edit, cx),
            Command::SaveList => {
                self.config.list = self.sigs.read(cx).list.clone();
                self.save();
            }
            Command::CopyBookmark(key) => self.copy_signature_bookmark(key, cx),
            Command::ClearReview => self.sigs.update(cx, |sigs, cx| {
                sigs.review = None;
                cx.notify();
            }),
            Command::HideOverlay => self.set_overlay(false, cx),
            Command::Pick(index) => self.pick(index, cx),
            Command::SkipJump => self.skip_jump(cx),
            Command::PlaceOverlay {
                x,
                y,
                width,
                height,
            } => {
                let o = &mut self.config.overlay;
                (o.x, o.y) = (Some(x), Some(y));
                if o.dimensions() != (width, height) {
                    (o.width, o.height) = (Some(width), Some(height));
                }
                self.save();
            }
        }
    }

    /// Show the outcome in the panels and the log.
    fn tell(&mut self, tone: Tone, text: String, cx: &mut Context<Self>) {
        let level = match tone {
            Tone::Info => Level::Info,
            Tone::Done => Level::Done,
            Tone::Problem => Level::Problem,
        };
        self.note(level, text.clone());
        self.notice_seq += 1;
        let notice = Notice {
            mood: match tone {
                Tone::Info => Mood::Info,
                Tone::Done => Mood::Done,
                Tone::Problem => Mood::Problem,
            },
            text,
            id: self.notice_seq,
            leaving: false,
        };
        let (id, lasts) = (notice.id, notice.lasts());
        self.sigs.update(cx, |sigs, cx| {
            sigs.notice = Some(notice);
            cx.notify();
        });
        cx.notify();
        // Fade it out, then drop it, unless a newer one took its place.
        let sigs = self.sigs.downgrade();
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(lasts).await;
            let still = |cx: &mut App, leave: bool| {
                sigs.update(cx, |sigs, cx| {
                    if sigs.notice.as_ref().is_some_and(|n| n.id == id) {
                        if leave {
                            if let Some(n) = sigs.notice.as_mut() {
                                n.leaving = true;
                            }
                        } else {
                            sigs.notice = None;
                        }
                        cx.notify();
                    }
                })
                .ok();
            };
            cx.update(|cx| still(cx, true));
            this.update(cx, |_, cx| cx.notify()).ok();
            cx.background_executor().timer(ui::FADE).await;
            cx.update(|cx| still(cx, false));
            this.update(cx, |_, cx| cx.notify()).ok();
        })
        .detach();
    }

    fn set_busy(&mut self, busy: bool, cx: &mut Context<Self>) {
        self.sigs.update(cx, |sigs, cx| {
            sigs.busy = busy;
            cx.notify();
        });
    }

    /// Paste the probe scanner's clipboard into the system you are in.
    fn paste(&mut self, cx: &mut Context<Self>) {
        if self.config.overlay.show_on_paste && self.overlay.is_none() {
            self.set_overlay(true, cx);
        }
        let (Some(client), Some(map_id)) = (self.client(), self.config.map_id) else {
            return self.tell(Tone::Problem, "Sign in and pick a map first".into(), cx);
        };
        let sigs = self.sigs.read(cx);
        if sigs.busy {
            return;
        }
        let Some(here) = sigs.here.clone() else {
            return self.tell(
                Tone::Problem,
                "No idea where you are yet: the chat logs have no character".into(),
                cx,
            );
        };
        let Some(system) = sigs.system().cloned() else {
            return self.tell(
                Tone::Problem,
                format!("{} is not on the map, so it has nowhere to go", here.system),
                cx,
            );
        };
        let text = match self.clipboard.as_mut().map(|c| c.get_text()) {
            Some(Ok(text)) => text,
            _ => return self.tell(Tone::Problem, "The clipboard holds no text".into(), cx),
        };
        let rows = scan::parse(&text, &self.catalog);
        if rows.is_empty() {
            return self.tell(
                Tone::Problem,
                "Nothing in the clipboard looked like a scan. Select the rows in the probe scanner and copy them first.".into(),
                cx,
            );
        }

        self.set_busy(true, cx);
        let solar_system_id = system.solar_system_id;
        let work = cx.background_spawn(async move {
            // Fresh, so a teammate's paste a moment ago counts as there before this one.
            let before: std::collections::HashSet<String> = client
                .snapshot(map_id)?
                .signatures
                .into_iter()
                .filter(|s| s.solar_system_id == solar_system_id)
                .map(|s| s.signature_id)
                .collect();
            client.paste(map_id, solar_system_id, &rows)?;
            let after = client.snapshot(map_id)?;
            let pasted = rows.into_iter().map(|r| r.signature_id).collect();
            anyhow::Ok((before, pasted, after))
        });
        let name = here.system.clone();
        cx.spawn(async move |this, cx| {
            let result = work.await;
            this.update(cx, |this, cx| {
                this.set_busy(false, cx);
                match result {
                    Ok((before, pasted, after)) => {
                        let review = Review {
                            solar_system_id,
                            before,
                            pasted,
                        };
                        let new = review.pasted.difference(&review.before).count();
                        let updated = review.pasted.intersection(&review.before).count();
                        let missing = review.before.difference(&review.pasted).count();
                        this.show_changed(after, cx);
                        this.sigs.update(cx, |sigs, cx| {
                            sigs.review = Some(review);
                            cx.notify();
                        });
                        let mut text = format!("Pasted into {name}: {new} new, {updated} updated");
                        if missing > 0 {
                            text += &format!(", {missing} missing");
                        }
                        this.tell(Tone::Done, text, cx);
                    }
                    Err(err) => this.tell(Tone::Problem, format!("Could not paste: {err}"), cx),
                }
            })
            .ok();
        })
        .detach();
    }

    /// Delete what the last paste no longer saw.
    fn delete_missing(&mut self, cx: &mut Context<Self>) {
        let missing = self.sigs.read(cx).missing();
        if missing.is_empty() {
            return self.tell(Tone::Info, "Nothing missing from the last paste".into(), cx);
        }
        let n = missing.len();
        let text = format!(
            "Deleted {n} signature{} the scan no longer sees",
            if n == 1 { "" } else { "s" }
        );
        self.delete(missing, text, cx);
    }

    fn delete(&mut self, keys: Vec<i64>, done: String, cx: &mut Context<Self>) {
        let (Some(client), Some(map_id)) = (self.client(), self.config.map_id) else {
            return;
        };
        if self.sigs.read(cx).busy {
            return;
        }
        self.set_busy(true, cx);
        let work = cx.background_spawn(async move {
            client.remove_signatures(map_id, &keys)?;
            client.snapshot(map_id)
        });
        cx.spawn(async move |this, cx| {
            let result = work.await;
            this.update(cx, |this, cx| {
                this.set_busy(false, cx);
                match result {
                    Ok(after) => {
                        this.show_changed(after, cx);
                        this.tell(Tone::Done, done, cx);
                    }
                    Err(err) => this.tell(Tone::Problem, format!("Could not delete: {err}"), cx),
                }
            })
            .ok();
        })
        .detach();
    }

    /// Change one signature the way the website's row does, then show the map as it is now.
    fn edit_signature(&mut self, key: i64, edit: SignatureEdit, cx: &mut Context<Self>) {
        let (Some(client), Some(map_id)) = (self.client(), self.config.map_id) else {
            return;
        };
        if self.sigs.read(cx).busy {
            return;
        }
        let done = match &edit {
            SignatureEdit::Group(_) => "Category set",
            SignatureEdit::Type(_) => "Type set",
            SignatureEdit::Link(_) => "Linked to the connection",
            SignatureEdit::Unlink => "Unlinked from its connection",
            SignatureEdit::Mass(_) => "Mass set",
            SignatureEdit::Time(_) => "Lifetime set",
            SignatureEdit::PreserveMass { on: true, .. } => "Preserving mass on the connection",
            SignatureEdit::PreserveMass { on: false, .. } => "No longer preserving mass",
        };
        self.set_busy(true, cx);
        let work = cx.background_spawn(async move {
            client.edit_signature(map_id, key, &edit)?;
            client.snapshot(map_id)
        });
        cx.spawn(async move |this, cx| {
            let result = work.await;
            this.update(cx, |this, cx| {
                this.set_busy(false, cx);
                match result {
                    Ok(after) => {
                        this.show_changed(after, cx);
                        this.tell(Tone::Done, done.into(), cx);
                    }
                    Err(err) => this.tell(Tone::Problem, format!("Could not change it: {err}"), cx),
                }
            })
            .ok();
        })
        .detach();
    }

    /// Copy the bookmark for where a signature's hole goes, the way the website's copy
    /// button does: the far system once someone has been, else the class its type promises.
    /// A hole with no name yet gets the next one in the chain, written onto its far end so
    /// the next copy takes the one after.
    fn copy_signature_bookmark(&mut self, key: i64, cx: &mut Context<Self>) {
        let sigs = self.sigs.read(cx);
        let (Some(snapshot), Some(here)) = (sigs.snapshot.clone(), sigs.system().cloned()) else {
            return;
        };
        let Some(sig) = snapshot.signatures.iter().find(|s| s.id == key).cloned() else {
            return;
        };
        let ty = sig
            .signature_type_id
            .and_then(|id| sigs.catalog.types.iter().find(|t| t.id == id))
            .cloned();
        // The connection's other end: a placed system, or a ghost nobody has been through.
        let end = sig
            .connection_id
            .and_then(|c| snapshot.connection(c))
            .map(|c| {
                if c.from_system == here.id {
                    c.to_system
                } else {
                    c.from_system
                }
            });
        let far = end.and_then(|id| snapshot.systems.iter().find(|s| s.id == id));
        let end_alias = end.and_then(|id| {
            far.map(|s| s.alias.clone())
                .or_else(|| {
                    snapshot
                        .ghosts
                        .iter()
                        .find(|g| g.id == id)
                        .map(|g| g.alias.clone())
                })
                .flatten()
                .filter(|a| !a.trim().is_empty())
        });
        let class = far
            .and_then(|s| s.wormhole_class_id)
            .or(ty.as_ref().and_then(|t| t.target_class));
        let mut guessed = None;
        let alias = end_alias.or_else(|| {
            let alias = jump::suggest_alias(
                &here,
                class,
                far.map_or(-1.0, |s| s.security_status),
                &aliases(&snapshot),
                AliasScheme::parse(snapshot.map.naming.alias_scheme.as_deref()),
                &snapshot.map.naming.ignored_alias,
            );
            guessed = alias.clone().zip(end);
            alias
        });
        let text = format_bookmark(
            &BookmarkSystem {
                alias,
                name: far.map(|s| s.name.clone()).unwrap_or_default(),
                region: far.map(|s| s.region.clone()),
                wormhole_class_id: class,
                security: far.map(|s| s.security_status),
                occupier: far.and_then(|s| s.occupying_group.clone()),
                is_home: far.is_some_and(|s| s.is_home),
            },
            &BookmarkContext {
                current_alias: None,
                signature_id: Some(sig.signature_id.clone()),
                size: sig.size,
                mass_status: sig.mass_status,
                time_status: sig.time_status,
                wormhole_code: ty.and_then(|t| t.signature),
            },
            &BookmarkFormats::from(&snapshot.map.naming),
            here.alias.as_deref(),
        );
        if !self.copy(&text) {
            return self.tell(Tone::Problem, "Could not copy the bookmark".into(), cx);
        }
        self.tell(Tone::Done, format!("Copied “{text}”"), cx);
        if let (Some((alias, placement)), Some(client), Some(map_id)) =
            (guessed, self.client(), self.config.map_id)
        {
            let work = cx.background_spawn(async move {
                client.set_alias(map_id, placement, &alias)?;
                client.snapshot(map_id)
            });
            cx.spawn(async move |this, cx| {
                let result = work.await;
                this.update(cx, |this, cx| match result {
                    Ok(after) => this.show_changed(after, cx),
                    Err(err) => {
                        this.tell(Tone::Problem, format!("Could not name the hole: {err}"), cx)
                    }
                })
                .ok();
            })
            .detach();
        }
    }

    // Aliases.

    /// Open the dialog that names the system you are in.
    fn open_alias(&mut self, cx: &mut Context<Self>) {
        if self.alias_window.is_some() {
            return;
        }
        let sigs = self.sigs.read(cx);
        let Some(system) = sigs.system().cloned() else {
            let why = match &sigs.here {
                Some(here) => format!("{} is not on the map, so it cannot be named", here.system),
                None => "No idea where you are yet: the chat logs have no character".into(),
            };
            return self.tell(Tone::Problem, why, cx);
        };
        // Nothing yet: offer what the chain's scheme would call it, from where it was
        // jumped to from.
        let current = system
            .alias
            .clone()
            .filter(|a| !a.trim().is_empty())
            .or_else(|| {
                let snapshot = sigs.snapshot.as_ref()?;
                let last = self.last.as_ref().filter(|l| l.here == system.name)?;
                let origin = snapshot.system_named(&last.back_to)?;
                jump::suggest_alias(
                    origin,
                    system.wormhole_class_id,
                    system.security_status,
                    &aliases(snapshot),
                    AliasScheme::parse(snapshot.map.naming.alias_scheme.as_deref()),
                    &snapshot.map.naming.ignored_alias,
                )
            })
            .unwrap_or_default();
        let this = cx.entity().downgrade();
        let (placement, name) = (system.id, system.name.clone());
        let done: alias::Done = Box::new(move |value, cx| {
            this.update(cx, |this, cx| {
                this.alias_window = None;
                if let Some(value) = value {
                    this.save_alias(placement, name.clone(), value, cx);
                }
            })
            .ok();
        });
        match alias::open(system.name.clone(), current, done, cx) {
            Ok(handle) => self.alias_window = Some(handle),
            Err(err) => self.note(
                Level::Problem,
                format!("Could not open the alias dialog: {err}"),
            ),
        }
    }

    fn save_alias(&mut self, placement: i64, name: String, alias: String, cx: &mut Context<Self>) {
        let (Some(client), Some(map_id)) = (self.client(), self.config.map_id) else {
            return;
        };
        let work = cx.background_spawn({
            let alias = alias.clone();
            async move { client.set_alias(map_id, placement, &alias) }
        });
        cx.spawn(async move |this, cx| {
            let result = work.await;
            this.update(cx, |this, cx| {
                match result {
                    Ok(()) if alias.is_empty() => {
                        this.tell(Tone::Done, format!("Cleared the alias of {name}"), cx)
                    }
                    Ok(()) => this.tell(Tone::Done, format!("Named {name} {alias}"), cx),
                    Err(err) => {
                        this.tell(Tone::Problem, format!("Could not name {name}: {err}"), cx)
                    }
                }
                this.refresh_changed(cx);
            })
            .ok();
        })
        .detach();
    }

    /// Fetch the portraits of characters the logs know an id for, once each.
    fn fetch_portraits(&mut self, cx: &mut Context<Self>) {
        let mut agent: Option<ureq::Agent> = None;
        for (name, _, id) in &self.characters {
            let Some(id) = *id else { continue };
            if self.portraits.contains_key(name) {
                continue;
            }
            self.portraits.insert(name.clone(), None);
            let name = name.clone();
            // With a timeout, so a stalled download never holds a thread for good.
            let agent = agent
                .get_or_insert_with(|| {
                    ureq::Agent::config_builder()
                        .timeout_global(Some(Duration::from_secs(15)))
                        .build()
                        .new_agent()
                })
                .clone();
            let fetch = cx.background_spawn(async move {
                let mut response = agent
                    .get(format!(
                        "https://images.evetech.net/characters/{id}/portrait?size=64"
                    ))
                    .call()
                    .ok()?;
                let bytes = response.body_mut().read_to_vec().ok()?;
                Some(Arc::new(Image::from_bytes(ImageFormat::Jpeg, bytes)))
            });
            cx.spawn(async move |this, cx| {
                if let Some(portrait) = fetch.await {
                    this.update(cx, |this, cx| {
                        this.portraits.insert(name, Some(portrait));
                        cx.notify();
                    })
                    .ok();
                }
            })
            .detach();
        }
    }

    fn portrait(&self, name: &str) -> Option<Arc<Image>> {
        self.portraits.get(name).cloned().flatten()
    }

    // Looks and the overlay.

    fn set_appearance(
        &mut self,
        appearance: Appearance,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.config.appearance = appearance;
        self.save();
        theme::apply(appearance, window, cx);
        cx.notify();
    }

    pub fn toggle_overlay(&mut self, cx: &mut Context<Self>) {
        self.set_overlay(self.overlay.is_none(), cx);
    }

    fn set_overlay(&mut self, open: bool, cx: &mut Context<Self>) {
        if let Some(handle) = self.overlay.take() {
            handle
                .update(cx, |_, window, _| window.remove_window())
                .ok();
        }
        self.config.overlay.open = open;
        self.save();
        if open {
            match overlay::open(self.sigs.clone(), &self.config.overlay, cx) {
                Ok(handle) => self.overlay = Some(handle),
                Err(err) => {
                    self.config.overlay.open = false;
                    self.note(Level::Problem, format!("Could not open the overlay: {err}"));
                }
            }
        }
        cx.notify();
    }

    /// Change an overlay setting; an open overlay is reopened to take it.
    fn edit_overlay(
        &mut self,
        cx: &mut Context<Self>,
        edit: impl FnOnce(&mut crate::config::Overlay),
    ) {
        edit(&mut self.config.overlay);
        let settings = self.config.overlay.clone();
        self.sigs.update(cx, |sigs, cx| {
            sigs.overlay = settings;
            cx.notify();
        });
        self.apply_focus(cx);
        if self.config.overlay.open {
            self.set_overlay(true, cx);
        } else {
            self.save();
        }
        cx.notify();
    }
}
