//! The signatures in the system you are in, what the last paste did to them, and after a
//! jump, which signature it was. One entity, shown by the main window and the overlay alike;
//! their buttons emit [`Command`]s, which the app carries out.

use std::cell::RefCell;
use std::cmp::Reverse;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::{ActiveTheme as _, Theme, ThemeStyled as _};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    Anchor, Animation, AnimationExt, AnyElement, App, Bounds, ClickEvent, DisplayId, Div,
    ElementId, Entity, EventEmitter, FontWeight, Hsla, Image, InteractiveElement, IntoElement,
    MouseButton, MouseDownEvent, ParentElement, Pixels, Rems, Stateful, StatefulInteractiveElement,
    Styled, Window, anchored, canvas, deferred, div, ease_out_quint, img, px, svg,
};

use crate::api::{
    Connection, MapSnapshot, MassStatus, PlacedSystem, Signature, SignatureCatalog, SignatureEdit,
    SignatureType, TimeStatus,
};
use crate::classes;
use crate::config::{ListView, Overlay, OverlaySetting, SortColumn};
use crate::theme::{self, HEADING};
use crate::ui::{self, ButtonState, Kind, Notice, OPACITIES, TEXT_SIZES, rpx};

/// What a button in a signatures panel asks the app to do.
#[derive(Debug, Clone)]
pub(crate) enum Command {
    Paste,
    DeleteMissing,
    Delete(i64),
    ClearReview,
    HideOverlay,
    /// The list's filter or sort changed, to keep.
    SaveList,
    /// Change a signature: its category, type, or the connection it is.
    Edit(i64, SignatureEdit),
    /// The jump was the option at this index; `None` for a hole nobody had scanned.
    Pick(Option<usize>),
    /// Copy the bookmark for where this signature's hole goes.
    CopyBookmark(i64),
    /// Leave the jump off the map.
    SkipJump,
    /// Open this popup in a window of its own, by the pointer at `x`, `y` on `display`, the
    /// screen the overlay is on.
    Popover {
        popup: Popup,
        x: f32,
        y: f32,
        display: Option<DisplayId>,
    },
    /// From the overlay's quick settings: follow this character, or whoever moves.
    Track(Option<String>),
    /// Map onto this map.
    PickMap(i64),
    /// Work on this tab of it.
    PickTab(i64),
    ToggleCopyOnJump,
    Overlay(OverlaySetting),
    /// The overlay was dragged or resized to this: top-left corner and size.
    PlaceOverlay {
        x: f32,
        y: f32,
        width: f32,
        height: f32,
    },
}

/// The system a jump landed in.
#[derive(Debug, Clone)]
pub(crate) struct Target {
    pub solar_system_id: i64,
    pub name: String,
    pub class: Option<i64>,
    pub security: f64,
}

/// A jump waiting to be told which of the origin's signatures it went through.
#[derive(Debug, Clone)]
pub(crate) struct Ask {
    pub character: String,
    pub map_id: i64,
    pub origin: PlacedSystem,
    pub target: Target,
    /// Likely ones first, then the unlikely; picked by position, numbered from 1.
    pub options: Vec<Signature>,
    pub likely: usize,
    /// Holes off the origin nobody has flown, connection to ghost placement, with their
    /// names. Flying one names the ghost rather than placing the system twice.
    pub ghosts: HashMap<i64, i64>,
    pub ghost_aliases: HashMap<i64, String>,
    /// What the new system will be called, if anything.
    pub alias: Option<String>,
    /// Where it goes on the map.
    pub at: (f64, f64),
}

/// Which of a row's cells has its choices open under it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Picker {
    pub sig: i64,
    pub kind: PickerKind,
    /// Opened in the overlay rather than the main window; only that one shows it.
    pub overlay: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PickerKind {
    Category,
    Type,
    Connection,
    /// The row's menu: the hole's mass and lifetime, preserve mass, delete.
    Menu,
}

/// Something hung open off a button: a row's picker, or the overlay's quick settings. Only
/// one is open at a time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Popup {
    Picker(Picker),
    Quick,
}

/// Where the overlay's popups are drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum Popups {
    /// In the overlay itself, under their buttons. It only hears of a click that lands on
    /// it, and cannot be typed into.
    #[default]
    Inline,
    /// Each in a window of its own over the screen (see `popover`), which a click anywhere
    /// else closes and which takes the keyboard when there is something to type.
    Windowed,
}

impl Popups {
    /// Windowed where there may be a layer shell to open them on. Whether there is one is
    /// only known by trying: the first that fails makes them inline.
    pub(crate) fn detect() -> Self {
        if cfg!(target_os = "linux") && std::env::var_os("WAYLAND_DISPLAY").is_some() {
            Popups::Windowed
        } else {
            Popups::Inline
        }
    }
}

/// The types that can lead into a system of `class`. A group like `C1-3` is what a hole says
/// from outside; with the class known, the type of the same code that names it is the
/// answer, so the group is left out wherever that one is on offer.
fn leading_to(types: Vec<&SignatureType>, class: i64) -> Vec<&SignatureType> {
    let exact: HashSet<Option<&str>> = types
        .iter()
        .filter(|t| t.target_class == Some(class))
        .map(|t| t.signature.as_deref())
        .collect();
    types
        .into_iter()
        .filter(|t| match t.target_class {
            Some(d) if d == class => true,
            Some(d) => {
                classes::dest_includes(d, Some(class)) && !exact.contains(&t.signature.as_deref())
            }
            None => false,
        })
        .collect()
}

/// Whether a type matches what has been typed: its wormhole code, its name, where it leads
/// and whatever the catalog notes about it. Every term has to match something, not all the
/// same thing, so `c6 frig` finds the frigate hole to class 6. A port of `matchesTypeQuery`
/// in `frontend/src/lib/signatures/type-search.ts`.
fn matches_query(t: &SignatureType, query: &str) -> bool {
    let dest = t
        .target_class
        .map_or("", |c| theme::dest_class_meta(Some(c)).0);
    let haystack = format!(
        "{} {} {} {dest}",
        t.signature.as_deref().unwrap_or_default(),
        t.name,
        t.extra.as_deref().unwrap_or_default(),
    )
    .to_lowercase();
    query
        .split_whitespace()
        .all(|term| haystack.contains(&term.to_lowercase()))
}

/// Which of a category's types to look at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Spawning {
    /// The ones known to spawn in the system you are in.
    Here,
    Anywhere,
}

/// What a type picker lists for a signature and a search.
pub(crate) struct TypeChoices<'a> {
    /// The signature has a category, so there are types to pick from.
    pub category: bool,
    /// For wormholes, statics (when wanted), then K162, then the rest; one unnamed section
    /// otherwise. Without the empty ones.
    pub sections: Vec<(Option<&'static str>, Vec<&'a SignatureType>)>,
    /// Matches not known to spawn here, offered when nothing here matches: the spawn data
    /// lags the game.
    pub elsewhere: Vec<&'a SignatureType>,
}

impl TypeChoices<'_> {
    /// The first one listed, which Enter picks.
    pub(crate) fn first(&self) -> Option<i64> {
        self.sections
            .iter()
            .flat_map(|(_, types)| types)
            .chain(&self.elsewhere)
            .next()
            .map(|t| t.id)
    }
}

/// How a row names its type: the wormhole code and where it leads, else the site.
pub(crate) struct TypeLabel {
    pub text: String,
    pub dest: Option<(&'static str, Hsla)>,
    pub extra: Option<String>,
    /// Nothing picked yet, or only the scanner's raw name.
    pub muted: bool,
}

/// The far end of one of the system's connections, as a picker lists it.
pub(crate) struct Far {
    pub connection: i64,
    pub class: (&'static str, Hsla),
    pub alias: Option<String>,
    /// `None` for a hole nobody has been through.
    pub name: Option<String>,
    pub region: Option<String>,
}

/// Where you are: the character who moved last, by the chat logs.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Here {
    pub character: String,
    pub system: String,
}

/// What a paste did, kept until the next paste, a sweep, or leaving the system. Only rows
/// there when the scan was pasted can be missing from it: one added since is news the scan
/// never saw.
#[derive(Debug, Clone)]
pub(crate) struct Review {
    pub solar_system_id: i64,
    pub before: HashSet<String>,
    pub pasted: HashSet<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PasteStatus {
    New,
    Updated,
    Missing,
}

#[derive(Clone, Copy)]
pub(crate) enum Tone {
    Info,
    Done,
    Problem,
}

/// What the overlay's quick settings show, kept up to date by the app.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct Quick {
    /// The characters that can be followed.
    pub characters: Vec<String>,
    /// The one followed; `None` for whoever moved last.
    pub tracked: Option<String>,
    /// The account's maps, id and name.
    pub maps: Vec<(i64, String)>,
    pub map_id: Option<i64>,
    pub copy_on_jump: bool,
    /// They are open. They share the pickers' popup, so only one of the two is open at a
    /// time.
    pub open: bool,
}

#[derive(Default)]
pub(crate) struct Sigs {
    pub here: Option<Here>,
    pub snapshot: Option<Arc<MapSnapshot>>,
    pub catalog: Arc<SignatureCatalog>,
    pub review: Option<Review>,
    pub busy: bool,
    /// The outcome of the last thing done here, shown under the list for a moment.
    pub notice: Option<Notice>,
    pub overlay: Overlay,
    pub list: ListView,
    pub ask: Option<Ask>,
    /// How the pick keys read before the digit, `Ctrl+Alt+`.
    pub pick_prefix: String,
    pub picker: Option<Picker>,
    /// Where the open picker's cell is, and where its popup landed, both in window
    /// coordinates: the popup hangs off the one, and the overlay takes clicks on the other.
    pub anchor: Rc<RefCell<Option<Bounds<Pixels>>>>,
    pub popup: Rc<RefCell<Option<Bounds<Pixels>>>>,
    /// List the system's statics first when picking a wormhole's type.
    pub statics_first: bool,
    /// The overlay is out of the way: EVE (or the right character) is not in front.
    pub overlay_hidden: bool,
    pub quick: Quick,
    pub portraits: HashMap<String, Arc<Image>>,
    /// The main window's box to search a type picker with.
    pub search: Option<Entity<InputState>>,
    pub popups: Popups,
}

impl EventEmitter<Command> for Sigs {}

impl Sigs {
    pub(crate) fn is_open(&self, popup: Popup) -> bool {
        match popup {
            Popup::Picker(picker) => self.picker == Some(picker),
            Popup::Quick => self.quick.open,
        }
    }

    /// Close `popup` if it is the one open, and say whether it was.
    pub(crate) fn close(&mut self, popup: Popup) -> bool {
        let open = self.is_open(popup);
        if open {
            self.picker = None;
            self.quick.open = false;
        }
        open
    }

    /// Whether `popup` is drawn in a window of its own rather than under its button.
    pub(crate) fn detached(&self, popup: Popup) -> bool {
        let overlay = match popup {
            Popup::Picker(picker) => picker.overlay,
            Popup::Quick => true,
        };
        overlay && self.popups == Popups::Windowed
    }

    /// Where the popup open in the overlay landed, for its input region.
    pub(crate) fn overlay_popup(&self) -> Option<Bounds<Pixels>> {
        (self.quick.open || self.picker.is_some_and(|p| p.overlay))
            .then(|| *self.popup.borrow())
            .flatten()
    }

    /// The signature with that row key.
    pub(crate) fn signature(&self, key: i64) -> Option<&Signature> {
        self.snapshot
            .as_ref()?
            .signatures
            .iter()
            .find(|s| s.id == key)
    }

    /// The system you are in, when it is on the map.
    pub(crate) fn system(&self) -> Option<&PlacedSystem> {
        let here = self.here.as_ref()?;
        self.snapshot.as_ref()?.system_named(&here.system)
    }

    /// The signatures in the system you are in, by scanner id.
    pub(crate) fn rows(&self) -> Vec<&Signature> {
        let (Some(system), Some(snapshot)) = (self.system(), &self.snapshot) else {
            return Vec::new();
        };
        let mut rows: Vec<&Signature> = snapshot
            .signatures
            .iter()
            .filter(|s| s.solar_system_id == system.solar_system_id)
            .collect();
        rows.sort_by(|a, b| a.signature_id.cmp(&b.signature_id));
        rows
    }

    /// What the list shows: [`Sigs::rows`] without the hidden categories, sorted the way the
    /// website's panel sorts, ties by scanner id so rows never swap on a refresh.
    /// `rows` is what [`Sigs::rows`] gave, so a render that wants both looks them up once.
    pub(crate) fn shown<'a>(&self, rows: &[&'a Signature]) -> Vec<&'a Signature> {
        let mut shown: Vec<&Signature> = rows
            .iter()
            .copied()
            .filter(|s| !self.list.hidden.contains(&s.group))
            .collect();
        let column = self.list.sort;
        let key = |s: &&Signature| {
            let name = (column == SortColumn::Type)
                .then(|| self.type_of(s).map(|t| t.name.as_str()))
                .flatten();
            sort_key(column, s, name)
        };
        // The rows come in by scanner id and the sort is stable, so ties stay in that order
        // both ways and rows never swap on a refresh.
        if self.list.descending {
            shown.sort_by_cached_key(|s| Reverse(key(s)));
        } else {
            shown.sort_by_cached_key(key);
        }
        shown
    }

    /// The review, when it is about the system you are in.
    fn current_review(&self) -> Option<&Review> {
        let system = self.system()?;
        self.review
            .as_ref()
            .filter(|r| r.solar_system_id == system.solar_system_id)
    }

    pub(crate) fn paste_status(&self, sig: &Signature) -> Option<PasteStatus> {
        let review = self.current_review()?;
        let before = review.before.contains(&sig.signature_id);
        if !review.pasted.contains(&sig.signature_id) {
            return before.then_some(PasteStatus::Missing);
        }
        Some(if before {
            PasteStatus::Updated
        } else {
            PasteStatus::New
        })
    }

    /// Row keys of what the last paste no longer saw.
    pub(crate) fn missing(&self) -> Vec<i64> {
        self.missing_in(&self.rows())
    }

    /// [`Sigs::missing`], among `rows` from [`Sigs::rows`].
    fn missing_in(&self, rows: &[&Signature]) -> Vec<i64> {
        rows.iter()
            .filter(|s| self.paste_status(s) == Some(PasteStatus::Missing))
            .map(|s| s.id)
            .collect()
    }

    fn type_of(&self, sig: &Signature) -> Option<&SignatureType> {
        let id = sig.signature_type_id?;
        self.catalog.types.iter().find(|t| t.id == id)
    }

    /// What a signature is, the way the website's row says it.
    pub(crate) fn type_label(&self, sig: &Signature) -> TypeLabel {
        match self.type_of(sig) {
            Some(t) if sig.group == "wormhole" && t.signature.is_some() => TypeLabel {
                text: t.signature.clone().unwrap_or_default(),
                dest: Some(theme::dest_class_meta(t.target_class)),
                extra: t.extra.clone(),
                muted: false,
            },
            Some(t) => TypeLabel {
                text: t.name.clone(),
                dest: None,
                extra: None,
                muted: false,
            },
            None => TypeLabel {
                text: sig.name.clone().unwrap_or_else(|| "Type".into()),
                dest: None,
                extra: None,
                muted: true,
            },
        }
    }

    /// The far end of a connection off the system you are in.
    fn far(&self, connection: i64) -> Option<Far> {
        let snapshot = self.snapshot.as_ref()?;
        let here = self.system()?;
        let c = snapshot.connection(connection)?;
        let other = if c.from_system == here.id {
            c.to_system
        } else {
            c.from_system
        };
        let alias = |a: &Option<String>| a.clone().filter(|a| !a.trim().is_empty());
        if let Some(s) = snapshot.systems.iter().find(|s| s.id == other) {
            return Some(Far {
                connection,
                class: theme::class_meta(s.wormhole_class_id, Some(s.security_status)),
                alias: alias(&s.alias),
                name: Some(s.name.clone()),
                region: Some(s.region.clone()).filter(|r| !r.is_empty()),
            });
        }
        let ghost = snapshot.ghosts.iter().find(|g| g.id == other)?;
        Some(Far {
            connection,
            class: theme::class_meta(None, None),
            alias: alias(&ghost.alias),
            name: None,
            region: None,
        })
    }

    /// The class of the system a signature's connection leads to, once someone has been.
    fn linked_class(&self, sig: &Signature) -> Option<i64> {
        let snapshot = self.snapshot.as_ref()?;
        let far = self.far(sig.connection_id?)?;
        let name = far.name?;
        snapshot.system_named(&name)?.wormhole_class_id
    }

    /// The types of a signature's category, narrowed to the far end's class once linked, by
    /// destination.
    fn types_of(&self, sig: &Signature, spawning: Spawning) -> Vec<&SignatureType> {
        let Some(&(_, category, _)) = theme::CATEGORIES.iter().find(|c| c.0 == sig.group) else {
            return Vec::new();
        };
        let system = self.system().filter(|_| spawning == Spawning::Here);
        // Types named for this system replace the rest of their category here.
        let named = system.is_some_and(|s| {
            self.catalog
                .types
                .iter()
                .any(|t| t.signature_category_id == category && t.names(s))
        });
        let mut types: Vec<&SignatureType> = self
            .catalog
            .types
            .iter()
            .filter(|t| t.signature_category_id == category)
            .filter(|t| match system {
                Some(s) if named => t.names(s),
                Some(s) => t.spawns_in(s),
                None => true,
            })
            .collect();
        if let Some(class) = self.linked_class(sig) {
            types = leading_to(types, class);
        }
        types.sort_by_key(|t| theme::dest_sort_weight(t.target_class));
        types
    }

    /// The types a signature can be, less what `query` rules out, grouped the way the
    /// website's picker lists them.
    pub(crate) fn type_choices(&self, sig: &Signature, query: &str) -> TypeChoices<'_> {
        let available = self.types_of(sig, Spawning::Here);
        let here: HashSet<i64> = available.iter().map(|t| t.id).collect();
        let types: Vec<&SignatureType> = available
            .into_iter()
            .filter(|t| matches_query(t, query))
            .collect();
        let sections = if sig.group == "wormhole" {
            let statics: HashSet<&str> = match self.system() {
                Some(s) if self.statics_first => {
                    s.statics.iter().map(|s| s.code.as_str()).collect()
                }
                _ => HashSet::new(),
            };
            let code = |t: &SignatureType| t.signature.clone().unwrap_or_default();
            let (first, rest): (Vec<_>, Vec<_>) = types
                .into_iter()
                .partition(|t| statics.contains(code(t).as_str()));
            let (k162, rest): (Vec<_>, Vec<_>) = rest.into_iter().partition(|t| code(t) == "K162");
            vec![
                (Some("Statics"), first),
                (Some("K162"), k162),
                (Some("Wormholes"), rest),
            ]
        } else {
            vec![(None, types)]
        };
        let sections: Vec<_> = sections
            .into_iter()
            .filter(|(_, v)| !v.is_empty())
            .collect();
        let elsewhere = if sections.is_empty() && !query.trim().is_empty() {
            self.types_of(sig, Spawning::Anywhere)
                .into_iter()
                .filter(|t| !here.contains(&t.id) && matches_query(t, query))
                .collect()
        } else {
            Vec::new()
        };
        TypeChoices {
            category: theme::CATEGORIES.iter().any(|c| c.0 == sig.group),
            sections,
            elsewhere,
        }
    }

    /// The connections a wormhole signature could be: the system's, narrowed to the type's
    /// destination class once it has one, aliased ones first. Split into the ones no other
    /// signature here claims, and the ones another does.
    pub(crate) fn link_options(&self, sig: &Signature) -> (Vec<Far>, Vec<Far>) {
        let (Some(snapshot), Some(here)) = (&self.snapshot, self.system()) else {
            return (Vec::new(), Vec::new());
        };
        let class = self.type_of(sig).and_then(|t| t.target_class);
        let mut options: Vec<Far> = snapshot
            .connections
            .iter()
            .filter(|c| c.from_system == here.id || c.to_system == here.id)
            .filter_map(|c| self.far(c.id))
            .filter(|f| {
                class.is_none()
                    || f.name
                        .as_ref()
                        .and_then(|n| snapshot.system_named(n))
                        .is_some_and(|s| {
                            class.is_some_and(|d| classes::dest_includes(d, s.wormhole_class_id))
                        })
            })
            .collect();
        options.sort_by(|a, b| match (&a.alias, &b.alias) {
            (Some(x), Some(y)) => x.cmp(y),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => a.name.cmp(&b.name),
        });
        let claimed: HashSet<i64> = snapshot
            .signatures
            .iter()
            .filter(|s| s.solar_system_id == here.solar_system_id && s.id != sig.id)
            .filter_map(|s| s.connection_id)
            .collect();
        options
            .into_iter()
            .partition(|f| !claimed.contains(&f.connection))
    }
}

/// `now`, `47m`, `3h`, `2d`: how old a row is, like the website's age column. Wormholes
/// count from when they were found (the signature or its connection, whichever is older),
/// sites from their last update.
/// Where a row goes when the list is sorted by one column, going up.
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
enum SortKey {
    /// A missing value sorts last going up, first going down, like the website.
    Text { missing: bool, text: String },
    /// Newest first going up.
    Age(Reverse<DateTime<Utc>>),
}

/// The key [`Sigs::shown`] sorts by. `type_name` is the name of the signature's catalog
/// type, which only the type column looks at.
fn sort_key(column: SortColumn, sig: &Signature, type_name: Option<&str>) -> SortKey {
    let text = |value: Option<String>| SortKey::Text {
        missing: value.is_none(),
        text: value.unwrap_or_default(),
    };
    match column {
        SortColumn::Id => text(Some(sig.signature_id.clone())),
        SortColumn::Category => text(Some(sig.group.clone()).filter(|g| g != "unknown")),
        SortColumn::Type => text(type_name.map(str::to_string).or_else(|| sig.name.clone())),
        SortColumn::Age => SortKey::Age(Reverse(if sig.group == "wormhole" {
            sig.created_at
        } else {
            sig.updated_at
        })),
    }
}

fn age(sig: &Signature, connection: Option<&Connection>, now: DateTime<Utc>) -> String {
    let since = if sig.group == "wormhole" {
        connection
            .and_then(|c| c.created_at)
            .map_or(sig.created_at, |c| c.min(sig.created_at))
    } else {
        sig.updated_at
    };
    let minutes = (now - since).num_minutes();
    match minutes {
        m if m < 1 => "now".into(),
        m if m < 60 => format!("{m}m"),
        m if m < 24 * 60 => format!("{}h", m / 60),
        m => format!("{}d", m / (24 * 60)),
    }
}

/// The age's color: the hole's mass first, then its lifetime, where the signature's own
/// lifetime wins over the connection's unless it is healthy.
fn age_color(sig: &Signature, connection: Option<&Connection>) -> Option<Hsla> {
    let mass = connection.and_then(|c| c.mass_status).or(sig.mass_status);
    let time = match sig.time_status {
        Some(t) if t != TimeStatus::Stable => Some(t),
        _ => connection.and_then(|c| c.time_status).or(sig.time_status),
    };
    match (mass, time) {
        (Some(MassStatus::Reduced), _) => Some(theme::reduced()),
        (Some(MassStatus::Critical), _) | (_, Some(TimeStatus::Critical | TimeStatus::Expired)) => {
            Some(theme::critical())
        }
        (_, Some(TimeStatus::Eol)) => Some(theme::eol()),
        _ => None,
    }
}

fn emit(
    entity: &Entity<Sigs>,
    command: Command,
) -> impl Fn(&ClickEvent, &mut Window, &mut App) + use<> {
    let entity = entity.clone();
    move |_, _, cx| {
        let command = command.clone();
        entity.update(cx, |_, cx| cx.emit(command));
    }
}

/// Where a signatures panel is shown. The overlay's is the compact one: a close button in
/// the header, and less room around everything.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Place {
    Page,
    Overlay,
}

/// The signatures panel.
pub(crate) fn panel(entity: &Entity<Sigs>, place: Place, cx: &App) -> AnyElement {
    panel_with(entity, place, None, None, cx)
}

/// What pressing on the panel's title does: the overlay moves by it.
pub(crate) type Grab = Box<dyn Fn(&MouseDownEvent, &mut Window, &mut App)>;

/// [`panel`], with `grab` making the title a handle to drag the overlay by.
pub(crate) fn panel_with(
    entity: &Entity<Sigs>,
    place: Place,
    grab: Option<Grab>,
    width: Option<f32>,
    cx: &App,
) -> AnyElement {
    let theme = cx.theme().clone();
    let sigs = entity.read(cx);
    let compact = place == Place::Overlay;
    let panel = Panel {
        entity,
        sigs,
        compact,
        pad: if compact { rpx(8.0) } else { rpx(16.0) },
        columns: Columns::fitting(width),
        theme: &theme,
    };
    let now = Utc::now();
    // Once per render: each is a lookup, a filter and a sort.
    let all = sigs.rows();
    let rows = sigs.shown(&all);
    let header = panel.header(all.len(), rows.len(), grab, cx);

    // The rows.
    let mut list = div().flex().flex_col();
    if let Some(empty) = panel.empty_note(all.len(), rows.len()) {
        list = list.child(empty);
    }
    let mut shown = false;
    for sig in rows {
        let (row, open) = panel.row(sig, now, cx);
        list = list.child(row);
        shown |= open;
    }
    let mine = sigs.picker.is_some_and(|p| p.overlay == compact);
    if (sigs.picker.is_none() && !sigs.quick.open) || (mine && !shown) {
        *sigs.anchor.borrow_mut() = None;
        *sigs.popup.borrow_mut() = None;
    }

    let ask = sigs
        .ask
        .as_ref()
        .map(|ask| ask_section(entity, sigs, ask, panel.pad, &theme));
    let footer = panel.footer(sigs.missing_in(&all).len());
    let columns = sigs
        .system()
        .is_some()
        .then(|| column_header(entity, &sigs.list, panel.pad, panel.columns, &theme));

    div()
        .size_full()
        .flex()
        .flex_col()
        .child(header)
        .children(ask)
        .children(columns)
        .child(
            div()
                .flex_1()
                .min_h_0()
                .relative()
                .child(
                    div()
                        .id(if compact { "overlay-rows" } else { "page-rows" })
                        .size_full()
                        .overflow_y_scroll()
                        .child(list),
                )
                .children(panel.status()),
        )
        .child(footer)
        .into_any_element()
}

/// Which of the list's columns a panel has room for. The id, the type, the connection and
/// the row's buttons always show; the rest give way as it gets narrow, so nothing is ever
/// cut off at the right edge.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Columns {
    All,
    /// The category as its icon alone.
    Tight,
    /// And no age.
    Tightest,
}

impl Columns {
    /// For a panel `width` wide at the normal text size; `None` for one with room to spare.
    fn fitting(width: Option<f32>) -> Self {
        match width {
            Some(w) if w < 390.0 => Columns::Tightest,
            Some(w) if w < 480.0 => Columns::Tight,
            _ => Columns::All,
        }
    }

    fn category(self) -> Rems {
        rpx(if self == Columns::All { 72.0 } else { 22.0 })
    }

    fn age(self) -> bool {
        self != Columns::Tightest
    }
}

/// What one render of a signatures panel draws with.
#[derive(Clone, Copy)]
struct Panel<'a> {
    entity: &'a Entity<Sigs>,
    sigs: &'a Sigs,
    /// The overlay's version.
    compact: bool,
    pad: Rems,
    columns: Columns,
    theme: &'a Theme,
}

impl Panel<'_> {
    /// Where you are: the system's class, alias and name.
    fn title(&self) -> Div {
        let Self { sigs, theme, .. } = *self;
        let title = div().flex().items_center().gap(rpx(6.0)).min_w_0();
        match (&sigs.here, sigs.system()) {
            (None, _) => title.child(
                div()
                    .text_color(theme.muted_foreground)
                    .child("No location"),
            ),
            (Some(here), None) => title.child(div().truncate().child(here.system.clone())),
            (Some(_), Some(system)) => {
                let (class, color) =
                    theme::class_meta(system.wormhole_class_id, Some(system.security_status));
                title
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
                    )
            }
        }
    }

    /// Under the title, small: who is mapped from, onto which map and tab, and how many
    /// signatures (`shown` of `all` when some are filtered out). The names give way before
    /// the count does.
    fn meta(&self, all: usize, shown: usize) -> Div {
        let Self { sigs, theme, .. } = *self;
        let quiet = theme.muted_foreground;
        let row = div()
            .flex()
            .items_center()
            .gap(rpx(8.0))
            .min_w_0()
            .text_xs()
            .text_color(quiet);
        let Some(here) = &sigs.here else {
            return row.child(
                div()
                    .truncate()
                    .child("Log in a character; the chat logs say where you are."),
            );
        };
        let part = |lead: AnyElement, text: String| {
            div()
                .flex()
                .items_center()
                .gap(rpx(4.0))
                .min_w_0()
                .child(lead)
                .child(div().truncate().child(text))
        };
        let glyph = |path: &'static str| {
            svg()
                .flex_none()
                .path(path)
                .size(rpx(11.0))
                .text_color(quiet.opacity(0.7))
                .into_any_element()
        };
        let face = match sigs.portraits.get(&here.character) {
            Some(portrait) => img(Arc::clone(portrait))
                .flex_none()
                .size(rpx(14.0))
                .rounded(px(2.0))
                .into_any_element(),
            None => glyph("icons/user.svg"),
        };
        let on_map = sigs.system().is_some();
        row.child(part(face, here.character.clone()))
            .when_some(sigs.snapshot.as_ref(), |d, snapshot| {
                let label = snapshot.label();
                d.child(part(
                    glyph("icons/map.svg"),
                    if on_map {
                        label
                    } else {
                        format!("not on {label}")
                    },
                ))
            })
            .when(on_map, |d| {
                d.child(
                    div()
                        .flex_none()
                        .ml_auto()
                        .font_family(theme.mono_font_family.clone())
                        .child(if shown == all {
                            format!("{all} sig{}", if all == 1 { "" } else { "s" })
                        } else {
                            format!("{shown}/{all} sigs")
                        }),
                )
            })
    }

    /// One of the overlay's header buttons.
    fn header_button(&self, id: &'static str, icon: &'static str) -> Stateful<Div> {
        let theme = self.theme;
        div()
            .id(id)
            .relative()
            .flex_none()
            .size(rpx(22.0))
            .rounded(px(3.0))
            .flex()
            .items_center()
            .justify_center()
            .cursor_pointer()
            .hover(|s| s.bg(theme.muted))
            .child(
                svg()
                    .path(icon)
                    .size(rpx(13.0))
                    .text_color(theme.muted_foreground),
            )
    }

    /// The title with the overlay's quick settings and close button beside it, and the meta
    /// line under both. The overlay is dragged by all of it but the buttons.
    fn header(&self, all: usize, shown: usize, grab: Option<Grab>, cx: &App) -> Div {
        let Self {
            entity,
            compact,
            pad,
            theme,
            ..
        } = *self;
        div()
            .flex_none()
            .flex()
            .flex_col()
            .gap(rpx(3.0))
            .px(pad)
            .pt(rpx(6.0))
            .pb(rpx(7.0))
            .border_b_1()
            .border_color(theme.border)
            .when_some(grab, |d, grab| {
                d.cursor_move()
                    .on_mouse_down(MouseButton::Left, move |e, w, cx| grab(e, w, cx))
            })
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(rpx(2.0))
                    .h(rpx(22.0))
                    .child(
                        self.title()
                            .flex_1()
                            .font_family(HEADING)
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_size(if compact { rpx(14.0) } else { rpx(16.0) }),
                    )
                    .when(compact, |d| {
                        d.child(self.quick_button(cx)).child(
                            self.header_button("overlay-hide", "icons/x.svg")
                                // Not a drag.
                                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                                .on_click(emit(entity, Command::HideOverlay)),
                        )
                    }),
            )
            .child(self.meta(all, shown))
    }

    /// The overlay's button for its quick settings, with their popup while that is open.
    fn quick_button(&self, cx: &App) -> Stateful<Div> {
        let Self {
            entity,
            sigs,
            theme,
            ..
        } = *self;
        let open = sigs.quick.open;
        let popup = (open && !sigs.detached(Popup::Quick)).then(|| {
            let content = quick_settings(entity, sigs, theme);
            float(entity, sigs, Popup::Quick, content, cx)
        });
        let toggle = toggle(entity, Popup::Quick);
        self.header_button("overlay-quick", "icons/settings.svg")
            .when(open, |d| d.bg(theme.muted).child(probe(&sigs.anchor)))
            .on_mouse_down(MouseButton::Left, move |event, window, cx| {
                // Not a drag.
                cx.stop_propagation();
                toggle(event, window, cx);
            })
            .children(popup)
    }

    /// What stands in for the rows when none are shown.
    fn empty_note(&self, all: usize, shown: usize) -> Option<Div> {
        let text = if shown > 0 {
            None
        } else if all > 0 {
            Some(format!("{all} hidden by the filters."))
        } else if self.sigs.system().is_some() {
            Some("No signatures yet. Copy the scan in the probe scanner and paste it.".to_string())
        } else {
            None
        }?;
        Some(
            div()
                .px(self.pad)
                .py_3()
                .text_xs()
                .text_color(self.theme.muted_foreground)
                .child(text),
        )
    }

    /// One signature's row, and whether one of its pickers is open.
    fn row(&self, sig: &Signature, now: DateTime<Utc>, cx: &App) -> (Stateful<Div>, bool) {
        let Self {
            entity,
            sigs,
            compact,
            pad,
            theme,
            ..
        } = *self;
        let status = sigs.paste_status(sig);
        let wormhole = sig.group == "wormhole";
        let connection = sig
            .connection_id
            .and_then(|c| sigs.snapshot.as_ref()?.connection(c));
        let far = sig.connection_id.and_then(|c| sigs.far(c));
        let mark = match status {
            Some(PasteStatus::New) => Some(theme::added()),
            Some(PasteStatus::Updated) => Some(theme::refreshed()),
            Some(PasteStatus::Missing) => Some(theme::missing()),
            None => None,
        };
        let missing = status == Some(PasteStatus::Missing);
        let key = sig.id;
        let open = sigs
            .picker
            .filter(|p| p.sig == key && p.overlay == compact)
            .map(|p| p.kind);
        // The open picker's popup, built once and handed to the cell it hangs off.
        let float_el = RefCell::new(open.and_then(|kind| {
            let popup = Popup::Picker(Picker {
                sig: key,
                kind,
                overlay: compact,
            });
            if sigs.detached(popup) {
                return None;
            }
            let search = sigs.search.as_ref().filter(|_| !compact);
            let content = picker(entity, sigs, sig, connection, kind, theme, search, cx);
            Some((kind, float(entity, sigs, popup, content, cx)))
        }));
        let float_for = |kind: PickerKind| {
            let mut slot = float_el.borrow_mut();
            match slot.take() {
                Some((k, el)) if k == kind => Some(el),
                other => {
                    *slot = other;
                    None
                }
            }
        };
        let cell = |kind: PickerKind| self.cell(key, kind, open).children(float_for(kind));
        let row = div()
            .id(ElementId::NamedInteger("sig".into(), key.cast_unsigned()))
            .relative()
            .flex()
            .items_center()
            .gap_2()
            .px(pad)
            .h(rpx(26.0))
            .text_xs()
            .border_b_1()
            .border_color(theme.border.opacity(0.3))
            .hover(|s| s.bg(theme.muted.opacity(0.3)))
            .when_some(mark, |d, color| d.bg(color.opacity(0.1)))
            .when(missing, |d| d.opacity(0.6))
            .child(
                // The paste's mark down the left edge.
                div()
                    .absolute()
                    .left_0()
                    .top_0()
                    .bottom_0()
                    .w(px(2.0))
                    .when_some(mark, gpui_kit::Styled::bg),
            )
            .child(
                div()
                    .w(rpx(56.0))
                    .flex_none()
                    .font_family(theme.mono_font_family.clone())
                    .when(missing, gpui_kit::Styled::line_through)
                    .child(if sig.signature_id.is_empty() {
                        "---".to_string()
                    } else {
                        sig.signature_id.clone()
                    }),
            )
            .child(self.category(cell(PickerKind::Category), sig))
            .child(self.type_cell(cell(PickerKind::Type), sig))
            .when(wormhole, |d| {
                d.child(self.connection_cell(cell(PickerKind::Connection), far))
            })
            .when(self.columns.age(), |d| {
                d.child(self.age_cell(sig, connection, now))
            })
            .child(self.actions(sig, open, float_for(PickerKind::Menu)));
        (row, open.is_some())
    }

    /// A row's cell that opens its picker, outlined while that is open.
    fn cell(&self, key: i64, kind: PickerKind, open: Option<PickerKind>) -> Stateful<Div> {
        let Self {
            entity,
            sigs,
            compact,
            theme,
            ..
        } = *self;
        div()
            .id(ElementId::Name(format!("cell-{key}-{kind:?}").into()))
            .h(rpx(20.0))
            .px_1()
            .rounded(px(3.0))
            .flex()
            .items_center()
            .gap_1()
            .min_w_0()
            .overflow_hidden()
            .cursor_pointer()
            .border_1()
            .border_color(if open == Some(kind) {
                theme.primary.opacity(0.7)
            } else {
                theme.transparent
            })
            .hover(|s| s.bg(theme.muted).border_color(theme.input))
            .relative()
            .on_mouse_down(
                MouseButton::Left,
                toggle(
                    entity,
                    Popup::Picker(Picker {
                        sig: key,
                        kind,
                        overlay: compact,
                    }),
                ),
            )
            .when(open == Some(kind), |d| d.child(probe(&sigs.anchor)))
    }

    /// Where a wormhole's connection leads, or a placeholder for one not linked.
    fn connection_cell(&self, cell: Stateful<Div>, far: Option<Far>) -> Stateful<Div> {
        let theme = self.theme;
        cell.flex_1().map(|d| match far {
            Some(far) => far_label(d, far, None, theme),
            None => d.child(
                div()
                    .truncate()
                    .text_color(theme.muted_foreground)
                    .child("Connection"),
            ),
        })
    }

    fn age_cell(
        &self,
        sig: &Signature,
        connection: Option<&Connection>,
        now: DateTime<Utc>,
    ) -> Div {
        let theme = self.theme;
        div()
            .w(rpx(32.0))
            .flex_none()
            .flex()
            .justify_end()
            .font_family(theme.mono_font_family.clone())
            .text_color(age_color(sig, connection).unwrap_or(theme.muted_foreground))
            .child(age(sig, connection, now))
    }

    /// The category cell's icon and name, or a placeholder for one nobody has set.
    fn category(&self, cell: Stateful<Div>, sig: &Signature) -> Stateful<Div> {
        let theme = self.theme;
        let (group, group_color) = theme::group_meta(&sig.group);
        let named = self.columns == Columns::All;
        cell.w(self.columns.category()).flex_none().map(|d| {
            let Some(color) = group_color.filter(|_| sig.group != "unknown") else {
                return d.child(div().text_color(theme.muted_foreground).child(if named {
                    "Category"
                } else {
                    "?"
                }));
            };
            d.when(!named, |d| d.px_0().justify_center())
                .child(
                    svg()
                        .flex_none()
                        .path(theme::group_icon(&sig.group))
                        .size(rpx(12.0))
                        .text_color(color),
                )
                .when(named, |d| d.child(div().truncate().child(group)))
        })
    }

    fn type_cell(&self, cell: Stateful<Div>, sig: &Signature) -> Stateful<Div> {
        let theme = self.theme;
        let label = self.sigs.type_label(sig);
        let wormhole = sig.group == "wormhole";
        cell
            // A wormhole's type is a code and a class, `K162 C3`: it needs little room, and
            // the connection beside it gets the rest.
            .when(wormhole, |d| d.w(rpx(84.0)).flex_none())
            .when(!wormhole, gpui_kit::Styled::flex_1)
            .map(|d| match label.dest {
                Some((short, color)) => d
                    .font_family(theme.mono_font_family.clone())
                    .child(div().flex_none().child(label.text))
                    .child(div().flex_none().text_color(color).child(short))
                    .when_some(label.extra, |d, extra| {
                        d.child(
                            div()
                                .truncate()
                                .text_color(theme.muted_foreground)
                                .child(format!("({extra})")),
                        )
                    }),
                None => d.child(
                    div()
                        .truncate()
                        .when(label.muted, |d| d.text_color(theme.muted_foreground))
                        .child(label.text),
                ),
            })
    }

    /// The row's buttons: copy the bookmark for a wormhole, and the menu with `menu` its
    /// popup when that is open.
    fn actions(&self, sig: &Signature, open: Option<PickerKind>, menu: Option<AnyElement>) -> Div {
        let Self {
            entity,
            sigs,
            compact,
            theme,
            ..
        } = *self;
        let key = sig.id;
        let wormhole = sig.group == "wormhole";
        let icon_button = |id: &str, path: &'static str| {
            div()
                .id(ElementId::Name(format!("{id}-{key}").into()))
                .flex_none()
                .size(rpx(20.0))
                .rounded(px(3.0))
                .flex()
                .items_center()
                .justify_center()
                .cursor_pointer()
                .hover(|s| s.bg(theme.muted))
                .child(
                    svg()
                        .path(path)
                        .size(rpx(13.0))
                        .text_color(theme.muted_foreground),
                )
        };
        div()
            .w(rpx(44.0))
            .flex_none()
            .flex()
            .items_center()
            .justify_end()
            .gap(rpx(2.0))
            .when(wormhole, |d| {
                d.child(
                    icon_button("sig-copy", "icons/copy.svg")
                        .on_click(emit(entity, Command::CopyBookmark(key))),
                )
            })
            .child(
                icon_button("sig-menu", "icons/ellipsis-vertical.svg")
                    .relative()
                    .when(open == Some(PickerKind::Menu), |d| {
                        d.bg(theme.muted).child(probe(&sigs.anchor))
                    })
                    .on_mouse_down(
                        MouseButton::Left,
                        toggle(
                            entity,
                            Popup::Picker(Picker {
                                sig: key,
                                kind: PickerKind::Menu,
                                overlay: compact,
                            }),
                        ),
                    )
                    .children(menu),
            )
    }

    /// The last outcome and what can be done next, with `missing` signatures the last
    /// paste no longer saw.
    fn footer(&self, missing: usize) -> Div {
        let Self {
            entity,
            sigs,
            pad,
            theme,
            ..
        } = *self;
        let can_paste = sigs.system().is_some() && !sigs.busy;
        let idle = ButtonState::from(!sigs.busy);
        div()
            .flex_none()
            .flex()
            .flex_col()
            .gap_2()
            .px(pad)
            .py_2()
            .border_t_1()
            .border_color(theme.border)
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap_1()
                    .child(
                        ui::button(
                            "sig-paste",
                            if sigs.busy {
                                "Working…"
                            } else {
                                "Paste scan"
                            },
                            Kind::Primary,
                            ButtonState::from(can_paste),
                            theme,
                        )
                        .when(can_paste, |d| d.on_click(emit(entity, Command::Paste))),
                    )
                    .when(missing > 0, |d| {
                        d.child(
                            ui::button(
                                "sig-sweep",
                                format!("Delete {missing} missing"),
                                Kind::Danger,
                                idle,
                                theme,
                            )
                            .when(!sigs.busy, |d| {
                                d.on_click(emit(entity, Command::DeleteMissing))
                            }),
                        )
                    })
                    .when(sigs.current_review().is_some(), |d| {
                        d.child(
                            ui::button(
                                "sig-clear",
                                "Clear marks",
                                Kind::Normal,
                                ButtonState::Enabled,
                                theme,
                            )
                            .on_click(emit(entity, Command::ClearReview)),
                        )
                    })
                    .child(filters(entity, &sigs.list, theme)),
            )
    }

    /// What just happened, for as long as it is news: in full, on as many lines as it needs.
    /// It lies over the bottom of the list, just above the buttons, so nothing moves when it
    /// comes and goes.
    fn status(&self) -> Option<AnyElement> {
        let Self {
            sigs, pad, theme, ..
        } = *self;
        let notice = sigs.notice.as_ref()?;
        let (icon, color) = ui::mood_look(notice.mood, theme);
        let line = div()
            .flex()
            .items_start()
            .gap(rpx(6.0))
            .px(pad)
            .py(rpx(5.0))
            .border_t_1()
            .border_color(color.opacity(0.45))
            .bg(color.opacity(0.1))
            .text_xs()
            .child(
                svg()
                    .flex_none()
                    .mt(rpx(1.5))
                    .path(icon)
                    .size(rpx(13.0))
                    .text_color(color),
            )
            .child(div().flex_1().min_w_0().child(notice.text.clone()));
        // Solid underneath, so the rows it covers do not show through the tint.
        let line = div()
            .absolute()
            .bottom_0()
            .left_0()
            .right_0()
            .bg(theme.background)
            .child(line);
        let (id, leaving) = (notice.id, notice.leaving);
        Some(
            line.with_animation(
                ElementId::NamedInteger(
                    if leaving { "status-out" } else { "status-in" }.into(),
                    id,
                ),
                Animation::new(if leaving {
                    ui::FADE
                } else {
                    std::time::Duration::from_millis(160)
                }),
                move |d, t| d.opacity(if leaving { 1.0 - t } else { t }),
            )
            .into_any_element(),
        )
    }
}

/// Change the list's filter or sort, and keep it.
fn relist(
    entity: &Entity<Sigs>,
    change: impl Fn(&mut ListView) + 'static,
) -> impl Fn(&ClickEvent, &mut Window, &mut App) + 'static {
    let entity = entity.clone();
    move |_, _, cx| {
        entity.update(cx, |sigs, cx| {
            change(&mut sigs.list);
            cx.emit(Command::SaveList);
            cx.notify();
        });
    }
}

/// The list's column names, the sortable ones clickable: the same column again flips the
/// direction, another starts it going up. Widths follow the rows'.
fn column_header(
    entity: &Entity<Sigs>,
    list: &ListView,
    pad: Rems,
    columns: Columns,
    theme: &Theme,
) -> Div {
    let head = |label: &'static str, column: Option<SortColumn>| {
        let active = column.is_some_and(|c| c == list.sort);
        let arrow = if list.descending {
            "icons/chevron-down.svg"
        } else {
            "icons/chevron-up.svg"
        };
        div()
            .id(ElementId::Name(format!("col-{label}").into()))
            .flex()
            .items_center()
            .gap(rpx(2.0))
            .min_w_0()
            .text_color(if active {
                theme.foreground
            } else {
                theme.muted_foreground
            })
            .child(label)
            .when(active, |d| {
                d.child(
                    svg()
                        .flex_none()
                        .path(arrow)
                        .size(rpx(11.0))
                        .text_color(theme.primary),
                )
            })
            .when_some(column, |d, column| {
                d.cursor_pointer()
                    .hover(|s| s.text_color(theme.foreground))
                    .on_click(relist(entity, move |l| {
                        if l.sort == column {
                            l.descending = !l.descending;
                        } else {
                            l.sort = column;
                            l.descending = false;
                        }
                    }))
            })
    };
    div()
        .flex_none()
        .flex()
        .items_center()
        .gap_2()
        .px(pad)
        .h(rpx(22.0))
        .text_size(rpx(11.0))
        .border_b_1()
        .border_color(theme.border.opacity(0.5))
        .child(head("ID", Some(SortColumn::Id)).w(rpx(56.0)).flex_none())
        .child(
            head(
                if columns == Columns::All {
                    "Category"
                } else {
                    ""
                },
                Some(SortColumn::Category),
            )
            .w(columns.category())
            .flex_none()
            .when(columns == Columns::All, gpui_kit::Styled::pl_1),
        )
        .child(
            head("Type", Some(SortColumn::Type))
                .w(rpx(84.0))
                .flex_none()
                .pl_1(),
        )
        .child(head("Connection", None).flex_1().pl_1())
        .when(columns.age(), |d| {
            d.child(
                head("Age", Some(SortColumn::Age))
                    .w(rpx(32.0))
                    .flex_none()
                    .justify_end(),
            )
        })
        .child(div().w(rpx(44.0)).flex_none())
}

/// One toggle per category, lit while it shows, like the website's panel header.
fn filters(entity: &Entity<Sigs>, list: &ListView, theme: &Theme) -> Div {
    let mut row = div().flex().items_center().gap(rpx(2.0)).ml_auto();
    for (group, _, name) in theme::CATEGORIES {
        let shown = !list.hidden.iter().any(|g| g == group);
        let (_, color) = theme::group_meta(group);
        row = row.child(
            div()
                .id(ElementId::Name(format!("filter-{group}").into()))
                .size(rpx(22.0))
                .rounded(px(3.0))
                .flex()
                .items_center()
                .justify_center()
                .cursor_pointer()
                .hover(|s| s.bg(theme.muted))
                .tooltip(move |window, cx| {
                    gpui_kit::component::tooltip::Tooltip::new(if shown {
                        format!("Hide {name}")
                    } else {
                        format!("Show {name}")
                    })
                    .build(window, cx)
                })
                .on_click(relist(entity, move |l| {
                    if let Some(i) = l.hidden.iter().position(|g| g == group) {
                        l.hidden.remove(i);
                    } else {
                        l.hidden.push(group.to_string());
                    }
                }))
                .child(
                    svg()
                        .path(theme::group_icon(group))
                        .size(rpx(13.0))
                        .text_color(if shown {
                            color.unwrap_or(theme.foreground)
                        } else {
                            theme.muted_foreground.opacity(0.35)
                        }),
                ),
        );
    }
    row
}

/// Write an element's bounds into `slot` as it is laid out, redrawing when they move.
fn probe(slot: &Rc<RefCell<Option<Bounds<Pixels>>>>) -> impl IntoElement {
    let slot = Rc::clone(slot);
    canvas(
        move |bounds, window, _| {
            if *slot.borrow() != Some(bounds) {
                *slot.borrow_mut() = Some(bounds);
                window.refresh();
            }
        },
        |_, (), _, _| {},
    )
    .absolute()
    .top_0()
    .left_0()
    .size_full()
}

/// Which edge of its button a popup lines up with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Side {
    Left,
    Right,
}

/// How a popup hangs off the button that opens it.
pub(crate) struct Hang {
    /// Tells it from the other popups.
    pub id: String,
    pub width: Rems,
    pub side: Side,
}

impl Popup {
    pub(crate) fn hang(self) -> Hang {
        let Popup::Picker(picker) = self else {
            return Hang {
                id: "quick".into(),
                width: rpx(250.0),
                side: Side::Right,
            };
        };
        Hang {
            id: format!("{}-{:?}", picker.sig, picker.kind),
            width: match picker.kind {
                PickerKind::Category => rpx(200.0),
                PickerKind::Menu => rpx(210.0),
                PickerKind::Type => rpx(240.0),
                PickerKind::Connection => rpx(300.0),
            },
            side: if picker.kind == PickerKind::Menu {
                Side::Right
            } else {
                Side::Left
            },
        }
    }
}

/// The box a popup's choices are in: as wide as `hang` says, scrolling past a height.
pub(crate) fn popup_surface(hang: &Hang, content: Div, cx: &App) -> Stateful<Div> {
    div()
        .id(ElementId::Name(format!("popup-{}", hang.id).into()))
        .occlude()
        .relative()
        .w(hang.width)
        .max_h(rpx(340.0))
        .overflow_y_scroll()
        .rounded(px(4.0))
        .popover_style(cx)
        .border_1()
        .border_color(cx.theme().border)
        .p_1()
        .child(content)
}

/// `popup`'s choices hung under the cell or button that opens it, over the rows rather than
/// between them. Laid out as that one's child, so it is placed in the same frame it opens.
/// It fades in, and a press anywhere else in the window closes it.
fn float(entity: &Entity<Sigs>, sigs: &Sigs, popup: Popup, content: Div, cx: &App) -> AnyElement {
    let hang = popup.hang();
    let close = {
        let entity = entity.clone();
        move |event: &MouseDownEvent, _: &mut Window, cx: &mut App| {
            entity.update(cx, |sigs, cx| {
                // A press on the button itself toggles it, which closes it.
                let on_button = sigs
                    .anchor
                    .borrow()
                    .is_some_and(|a| a.contains(&event.position));
                if !on_button && sigs.close(popup) {
                    cx.notify();
                }
            });
        }
    };
    let (id, side) = (hang.id.clone(), hang.side);
    let surface = popup_surface(&hang, content, cx)
        .on_mouse_down_out(close)
        .with_animation(
            ElementId::Name(format!("popup-in-{id}").into()),
            Animation::new(std::time::Duration::from_millis(120)).with_easing(ease_out_quint()),
            |d, t| d.opacity(t).mt(px((1.0 - t) * -4.0)),
        );
    let right = side == Side::Right;
    div()
        .absolute()
        .top_full()
        .mt(rpx(4.0))
        .map(|d| if right { d.right_0() } else { d.left_0() })
        .child(
            deferred(
                anchored()
                    .anchor(if right {
                        Anchor::TopRight
                    } else {
                        Anchor::TopLeft
                    })
                    .snap_to_window_with_margin(px(8.0))
                    // Measured around the scrolling surface, not inside it: in there the
                    // probe would scroll away with the choices, and the overlay would stop
                    // taking clicks on what it left behind.
                    .child(div().relative().child(surface).child(probe(&sigs.popup))),
            )
            .with_priority(1),
        )
        .into_any_element()
}

/// Open `popup`, or close it when it is the one open. One the overlay cannot draw itself
/// goes to a window of its own; the main window's type picker gets its search emptied and
/// focused.
fn toggle(
    entity: &Entity<Sigs>,
    popup: Popup,
) -> impl Fn(&MouseDownEvent, &mut Window, &mut App) + use<> {
    let entity = entity.clone();
    move |event, window, cx| {
        let display = window.display(cx).map(|d| d.id());
        let search = entity.update(cx, |sigs, cx| {
            cx.notify();
            if sigs.close(popup) {
                return None;
            }
            sigs.quick.open = popup == Popup::Quick;
            sigs.picker = match popup {
                Popup::Picker(picker) => Some(picker),
                Popup::Quick => None,
            };
            if sigs.detached(popup) {
                cx.emit(Command::Popover {
                    popup,
                    x: f32::from(event.position.x),
                    y: f32::from(event.position.y),
                    display,
                });
                return None;
            }
            match popup {
                Popup::Picker(picker) if picker.kind == PickerKind::Type && !picker.overlay => {
                    sigs.search.clone()
                }
                _ => None,
            }
        });
        if let Some(search) = search {
            search.update(cx, |input, cx| {
                input.set_value("", window, cx);
                input.focus(window, cx);
            });
        }
    }
}

/// Close the picker and make the change.
fn choose(
    entity: &Entity<Sigs>,
    key: i64,
    edit: SignatureEdit,
) -> impl Fn(&ClickEvent, &mut Window, &mut App) + use<> {
    let entity = entity.clone();
    move |_, _, cx| {
        let edit = edit.clone();
        entity.update(cx, |sigs, cx| {
            sigs.picker = None;
            cx.emit(Command::Edit(key, edit));
            cx.notify();
        });
    }
}

/// A connection's far end: its class, alias and name, the alias tinted by the hole's state.
fn far_label(d: Stateful<Div>, far: Far, hole: Option<Hsla>, theme: &Theme) -> Stateful<Div> {
    let (class, color) = far.class;
    d.child(div().flex_none().text_color(color).child(class))
        .when_some(far.alias.clone(), |d, alias| {
            d.child(
                div()
                    .flex_none()
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(hole.unwrap_or(theme.foreground))
                    .child(alias),
            )
        })
        // The system keeps its room; the region gives way first when it is tight.
        .child(
            div()
                .flex_shrink_0()
                .text_color(if far.alias.is_some() {
                    theme.foreground.opacity(0.8)
                } else {
                    hole.unwrap_or(theme.foreground)
                })
                .child(far.name.unwrap_or_else(|| "Unmapped".into())),
        )
        .when_some(far.region, |d, region| {
            d.child(
                div()
                    .min_w_0()
                    .truncate()
                    .text_color(theme.muted_foreground.opacity(0.6))
                    .child(region),
            )
        })
}

/// One choice in a picker's popup; [`chosen`] marks the one that is.
fn option(id: String, theme: &Theme) -> Stateful<Div> {
    div()
        .id(ElementId::Name(id.into()))
        .relative()
        .flex()
        .items_center()
        .gap(rpx(6.0))
        .h(rpx(26.0))
        .pl_2()
        .pr(rpx(26.0))
        .rounded(px(3.0))
        .text_xs()
        .cursor_pointer()
        .hover(|s| s.bg(theme.muted))
}

/// The chosen [`option`], tinted and ticked.
fn chosen(option: Stateful<Div>, theme: &Theme) -> Stateful<Div> {
    option.bg(theme.primary.opacity(0.14)).child(
        svg()
            .absolute()
            .right(rpx(8.0))
            .path("icons/check.svg")
            .size(rpx(12.0))
            .text_color(theme.primary),
    )
}

fn heading(text: &'static str, theme: &Theme) -> Div {
    div()
        .px_2()
        .pt_2()
        .pb_1()
        .text_size(rpx(11.0))
        .text_color(theme.muted_foreground)
        .child(text)
}

fn separator(theme: &Theme) -> Div {
    div().my_1().h(px(1.0)).bg(theme.border)
}

/// The choices for one of a row's cells: what the website's dropdowns offer. A type picker
/// is narrowed by what `search` holds, and shows it.
#[expect(
    clippy::too_many_arguments,
    reason = "one more than the limit, and each a different thing"
)]
pub(crate) fn picker(
    entity: &Entity<Sigs>,
    sigs: &Sigs,
    sig: &Signature,
    connection: Option<&Connection>,
    kind: PickerKind,
    theme: &Theme,
    search: Option<&Entity<InputState>>,
    cx: &App,
) -> Div {
    let body = div().flex().flex_col();
    match kind {
        PickerKind::Category => category_picker(body, entity, sig, theme),
        PickerKind::Type => {
            let key = sig.id;
            let query = search.map_or_default(|s| s.read(cx).value().to_string());
            let pick = |t| -> PickHandler { Box::new(choose(entity, key, SignatureEdit::Type(t))) };
            body.children(search.map(|s| div().p_1().child(Input::new(s))))
                .child(type_picker(sigs, sig, &query, &pick, theme))
        }
        PickerKind::Menu => menu_picker(body, entity, sig, connection, theme),
        PickerKind::Connection => connection_picker(body, entity, sigs, sig, theme),
    }
}

fn category_picker(mut body: Div, entity: &Entity<Sigs>, sig: &Signature, theme: &Theme) -> Div {
    let key = sig.id;
    for (group, _, name) in theme::CATEGORIES {
        let (_, color) = theme::group_meta(group);
        body = body.child(
            option(format!("pick-{key}-cat-{group}"), theme)
                .when(sig.group == group, |d| chosen(d, theme))
                .text_color(color.unwrap_or(theme.foreground))
                .on_click(choose(entity, key, SignatureEdit::Group(group.into())))
                .child(name),
        );
    }
    body
}

/// What a click on one of a type picker's choices does.
pub(crate) type PickHandler = Box<dyn Fn(&ClickEvent, &mut Window, &mut App)>;

/// What the search box of a type picker says before anything is typed. Not "Code, ...":
/// the website avoids the word for password managers' sake, and the two should read alike.
pub(crate) const TYPE_SEARCH: &str = "K162, name or where it leads…";

/// The types a signature can be, less what `query` rules out. `pick` makes the handler for
/// a choice: a type's id, or `None` for unknown.
pub(crate) fn type_picker(
    sigs: &Sigs,
    sig: &Signature,
    query: &str,
    pick: &dyn Fn(Option<i64>) -> PickHandler,
    theme: &Theme,
) -> Div {
    let key = sig.id;
    let mut body = div().flex().flex_col();
    let choices = sigs.type_choices(sig, query);
    if !choices.category {
        return body.child(heading("Pick a category first.", theme));
    }
    if choices.sections.is_empty() && choices.elsewhere.is_empty() {
        body = body.child(heading("No type matches", theme));
    }
    body = body.child(
        option(format!("pick-{key}-type-none"), theme)
            .when(sig.signature_type_id.is_none(), |d| chosen(d, theme))
            .text_color(theme.muted_foreground)
            .on_click(pick(None))
            .child("Unknown"),
    );
    let elsewhere = (!choices.elsewhere.is_empty())
        .then_some((Some("Not known to spawn here"), choices.elsewhere));
    for (label, types) in choices.sections.into_iter().chain(elsewhere) {
        if let Some(label) = label {
            body = body.child(heading(label, theme));
        }
        for t in types {
            let mut chip = option(format!("pick-{key}-type-{}", t.id), theme)
                .when(sig.signature_type_id == Some(t.id), |d| chosen(d, theme))
                .on_click(pick(Some(t.id)));
            chip = match &t.signature {
                Some(code) if sig.group == "wormhole" => {
                    let (short, color) = theme::dest_class_meta(t.target_class);
                    chip.child(code.clone())
                        .child(div().text_color(color).child(short))
                        .when_some(t.extra.clone(), |d, extra| {
                            d.child(
                                div()
                                    .text_color(theme.muted_foreground)
                                    .child(format!("({extra})")),
                            )
                        })
                }
                _ => chip.child(t.name.clone()),
            };
            body = body.child(chip);
        }
    }
    body
}

/// The row's menu: a wormhole's mass and lifetime and its connection's preserve mass,
/// then delete.
fn menu_picker(
    mut body: Div,
    entity: &Entity<Sigs>,
    sig: &Signature,
    connection: Option<&Connection>,
    theme: &Theme,
) -> Div {
    let key = sig.id;
    let dot = |color: Hsla| div().size(rpx(8.0)).rounded_full().bg(color);
    if sig.group == "wormhole" {
        body = body.child(heading("Mass", theme));
        for (value, name, color) in [
            (None, "Fresh Mass", theme::healthy()),
            (Some(MassStatus::Reduced), "Reduced Mass", theme::reduced()),
            (
                Some(MassStatus::Critical),
                "Critical Mass",
                theme::critical(),
            ),
        ] {
            let current = sig.mass_status.filter(|m| *m != MassStatus::Stable);
            body = body.child(
                option(format!("pick-{key}-mass-{name}"), theme)
                    .when(current == value, |d| chosen(d, theme))
                    .on_click(choose(entity, key, SignatureEdit::Mass(value)))
                    .child(dot(color))
                    .child(name),
            );
        }
        body = body.child(heading("Lifetime", theme));
        for (value, name, color) in [
            (None, "Healthy", theme::healthy()),
            (Some(TimeStatus::Eol), "End of Life", theme::eol()),
            (Some(TimeStatus::Critical), "Critical", theme::critical()),
            (Some(TimeStatus::Expired), "Expired", theme::critical()),
        ] {
            let current = sig.time_status.filter(|t| *t != TimeStatus::Stable);
            body = body.child(
                option(format!("pick-{key}-time-{name}"), theme)
                    .when(current == value, |d| chosen(d, theme))
                    .on_click(choose(entity, key, SignatureEdit::Time(value)))
                    .child(dot(color))
                    .child(name),
            );
        }
        if let Some(c) = connection {
            body = body.child(separator(theme)).child(
                option(format!("pick-{key}-preserve"), theme)
                    .when(c.preserve_mass, |d| chosen(d, theme))
                    .on_click(choose(
                        entity,
                        key,
                        SignatureEdit::PreserveMass {
                            connection: c.id,
                            on: !c.preserve_mass,
                        },
                    ))
                    .child("Preserve mass"),
            );
        }
        body = body.child(separator(theme));
    }
    let entity = entity.clone();
    body.child(
        option(format!("pick-{key}-delete"), theme)
            .text_color(theme.danger)
            .on_click(move |_, _, cx| {
                entity.update(cx, |sigs, cx| {
                    sigs.picker = None;
                    cx.emit(Command::Delete(key));
                    cx.notify();
                });
            })
            .child(
                svg()
                    .path("icons/trash-2.svg")
                    .size(rpx(12.0))
                    .text_color(theme.danger),
            )
            .child("Delete Signature"),
    )
}

fn connection_picker(
    mut body: Div,
    entity: &Entity<Sigs>,
    sigs: &Sigs,
    sig: &Signature,
    theme: &Theme,
) -> Div {
    let key = sig.id;
    let (free, taken) = sigs.link_options(sig);
    body = body.child(
        option(format!("pick-{key}-conn-none"), theme)
            .when(sig.connection_id.is_none(), |d| chosen(d, theme))
            .text_color(theme.muted_foreground)
            .on_click(choose(entity, key, SignatureEdit::Unlink))
            .child("Unknown"),
    );
    if free.is_empty() && taken.is_empty() {
        body = body.child(heading("No connection here fits this type yet.", theme));
    }
    for (label, list) in [("Connections", free), ("Connected", taken)] {
        if list.is_empty() {
            continue;
        }
        body = body.child(heading(label, theme));
        for far in list {
            let id = far.connection;
            let chip = option(format!("pick-{key}-conn-{id}"), theme)
                .when(sig.connection_id == Some(id), |d| chosen(d, theme))
                .on_click(choose(entity, key, SignatureEdit::Link(id)));
            body = body.child(far_label(chip, far, None, theme));
        }
    }
    body
}

/// What is worth changing without leaving the game: who is mapped from and onto which map,
/// whether jumps copy their bookmark, and how the overlay shows. It stays open while you
/// change things; a press anywhere else closes it.
pub(crate) fn quick_settings(entity: &Entity<Sigs>, sigs: &Sigs, theme: &Theme) -> Div {
    let quick = &sigs.quick;
    let mut body = div()
        .flex()
        .flex_col()
        .child(heading("Map from", theme))
        .child(
            option("quick-track-any".into(), theme)
                .when(quick.tracked.is_none(), |d| chosen(d, theme))
                .on_click(emit(entity, Command::Track(None)))
                .child("Whoever moved last"),
        );
    for (i, name) in quick.characters.iter().enumerate() {
        let face = rpx(16.0);
        let portrait = match sigs.portraits.get(name) {
            Some(portrait) => img(Arc::clone(portrait)).size(face).into_any_element(),
            None => div()
                .size(face)
                .bg(theme.foreground.opacity(0.08))
                .into_any_element(),
        };
        body = body.child(
            option(format!("quick-track-{i}"), theme)
                .when(quick.tracked.as_ref() == Some(name), |d| chosen(d, theme))
                .on_click(emit(entity, Command::Track(Some(name.clone()))))
                .child(div().flex_none().child(portrait))
                .child(div().truncate().child(name.clone())),
        );
    }
    if quick.maps.len() > 1 {
        body = body.child(heading("Map onto", theme));
        for (id, name) in &quick.maps {
            body = body.child(
                option(format!("quick-map-{id}"), theme)
                    .when(quick.map_id == Some(*id), |d| chosen(d, theme))
                    .on_click(emit(entity, Command::PickMap(*id)))
                    .child(div().truncate().child(name.clone())),
            );
        }
    }
    // Where you are decides the tab; picking one here holds until you move.
    if let Some(snapshot) = sigs.snapshot.as_ref().filter(|s| s.tabs.len() > 1) {
        body = body.child(heading("Tab", theme));
        for tab in &snapshot.tabs {
            body = body.child(
                option(format!("quick-tab-{}", tab.id), theme)
                    .when(snapshot.map.id == tab.id, |d| chosen(d, theme))
                    .on_click(emit(entity, Command::PickTab(tab.id)))
                    .child(div().truncate().child(tab.name.clone())),
            );
        }
    }
    let switch = |id: &'static str, label: &'static str, on: bool, command: Command| {
        option(id.into(), theme)
            .when(on, |d| chosen(d, theme))
            .on_click(emit(entity, command))
            .child(label)
    };
    let focused = sigs.overlay.only_when_focused;
    body.child(separator(theme))
        .child(switch(
            "quick-copy",
            "Copy the bookmark on every jump",
            quick.copy_on_jump,
            Command::ToggleCopyOnJump,
        ))
        .child(switch(
            "quick-focused",
            "Only while EVE has focus",
            focused,
            Command::Overlay(OverlaySetting::OnlyWhenFocused(!focused)),
        ))
        .child(heading("Background", theme))
        .child(choices(
            entity,
            "quick-opacity",
            OPACITIES,
            sigs.overlay.opacity,
            OverlaySetting::Opacity,
            theme,
        ))
        .child(heading("Text size", theme))
        .child(choices(
            entity,
            "quick-text",
            TEXT_SIZES,
            sigs.overlay.text_size,
            OverlaySetting::TextSize,
            theme,
        ))
}

/// A row of small buttons for one overlay setting, the current value lit.
fn choices(
    entity: &Entity<Sigs>,
    id: &'static str,
    values: &[(u8, &'static str)],
    current: u8,
    setting: fn(u8) -> OverlaySetting,
    theme: &Theme,
) -> Div {
    let mut row = div().flex().gap(rpx(2.0)).px_1().pb_1();
    for &(value, label) in values {
        let lit = value == current;
        row = row.child(
            div()
                .id(ElementId::NamedInteger(id.into(), u64::from(value)))
                .flex_1()
                .h(rpx(22.0))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(3.0))
                .border_1()
                .border_color(if lit {
                    theme.primary.opacity(0.7)
                } else {
                    theme.border
                })
                .when(lit, |d| d.bg(theme.primary.opacity(0.14)))
                .text_xs()
                .cursor_pointer()
                .hover(|s| s.bg(theme.muted))
                .on_click(emit(entity, Command::Overlay(setting(value))))
                .child(label),
        );
    }
    row
}

/// The question after a jump: which of the origin's signatures was it? One row per option,
/// numbered for the pick keys, and a last row for a hole nobody had scanned.
fn ask_section(entity: &Entity<Sigs>, sigs: &Sigs, ask: &Ask, pad: Rems, theme: &Theme) -> Div {
    let mut section = ask_header(entity, ask, pad, theme);
    for i in 0..ask.options.len() {
        section = section.child(ask_option(entity, sigs, ask, i, pad, theme));
    }
    section.child(
        ask_row(
            "ask-none".into(),
            Some(format!("{}0", sigs.pick_prefix)),
            pad,
            theme,
        )
        .on_click(emit(entity, Command::Pick(None)))
        .child(
            div()
                .text_color(theme.muted_foreground)
                .child("None of these: a hole nobody scanned"),
        ),
    )
}

/// Where the jump went, and the question.
fn ask_header(entity: &Entity<Sigs>, ask: &Ask, pad: Rems, theme: &Theme) -> Div {
    let origin = ask
        .origin
        .alias
        .clone()
        .filter(|a| !a.trim().is_empty())
        .unwrap_or_else(|| ask.origin.name.clone());
    let (class, class_color) = theme::class_meta(ask.target.class, Some(ask.target.security));
    div()
        .flex_none()
        .flex()
        .flex_col()
        .border_b_1()
        .border_color(theme.primary.opacity(0.6))
        .bg(theme.primary.opacity(0.06))
        .child(
            div()
                .flex()
                .items_center()
                .gap_2()
                .px(pad)
                .pt_2()
                .pb_1()
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .items_center()
                        .gap_1()
                        .font_family(HEADING)
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_xs()
                        .child(div().truncate().child(format!("{origin} →")))
                        .child(div().flex_none().text_color(class_color).child(class))
                        .child(div().flex_none().child(ask.target.name.clone()))
                        .when_some(ask.alias.clone(), |d, alias| {
                            d.child(
                                div()
                                    .flex_none()
                                    .text_color(theme.muted_foreground)
                                    .child(format!("as {alias}")),
                            )
                        }),
                )
                .child(
                    div()
                        .id("ask-skip")
                        .flex_none()
                        .px_1()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .cursor_pointer()
                        .hover(|s| s.text_color(theme.foreground))
                        .on_click(emit(entity, Command::SkipJump))
                        .child("Skip"),
                ),
        )
        .child(
            div()
                .px(pad)
                .pb_1()
                .text_xs()
                .text_color(theme.muted_foreground)
                .child(format!("Which signature did {} jump?", ask.character)),
        )
}

/// A row to pick, led by its pick key.
fn ask_row(id: ElementId, number: Option<String>, pad: Rems, theme: &Theme) -> Stateful<Div> {
    div()
        .id(id)
        .flex()
        .items_center()
        .gap_2()
        .px(pad)
        .h(rpx(22.0))
        .text_xs()
        .cursor_pointer()
        .hover(|s| s.bg(theme.primary.opacity(0.12)))
        .child(
            div()
                .w(rpx(64.0))
                .flex_none()
                .font_family(HEADING)
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(theme.primary)
                .child(number.unwrap_or_default()),
        )
}

/// The `i`th of the jump's options, dimmed past the likely ones.
fn ask_option(
    entity: &Entity<Sigs>,
    sigs: &Sigs,
    ask: &Ask,
    i: usize,
    pad: Rems,
    theme: &Theme,
) -> Stateful<Div> {
    let sig = &ask.options[i];
    let label = sigs.type_label(sig);
    let ghost = sig
        .connection_id
        .and_then(|c| ask.ghost_aliases.get(&c).cloned());
    let (group, group_color) = theme::group_meta(&sig.group);
    let number = (i < 9).then(|| format!("{}{}", sigs.pick_prefix, i + 1));
    ask_row(ElementId::from(("ask", i)), number, pad, theme)
        .when(i >= ask.likely, |d| d.opacity(0.55))
        .on_click(emit(entity, Command::Pick(Some(i))))
        .child(
            div()
                .w(rpx(54.0))
                .flex_none()
                .font_family(HEADING)
                .font_weight(FontWeight::SEMIBOLD)
                .child(sig.signature_id.clone()),
        )
        .child(
            div()
                .w(rpx(40.0))
                .flex_none()
                .text_color(group_color.unwrap_or(theme.muted_foreground))
                .child(group),
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
                        .when(label.muted, |d| d.text_color(theme.muted_foreground))
                        .child(label.text),
                )
                .when_some(label.dest, |d, (short, color)| {
                    d.child(div().flex_none().text_color(color).child(short))
                })
                .when_some(ghost, |d, alias| {
                    d.child(
                        div()
                            .flex_none()
                            .text_color(theme.muted_foreground)
                            .child(format!("→ {alias}")),
                    )
                }),
        )
}

#[cfg(test)]
mod tests {
    // Not a glob: GPUI has a `test` attribute of its own.
    use super::{
        DateTime, ListView, Signature, SignatureType, Sigs, SortColumn, Utc, classes, leading_to,
        matches_query,
    };

    fn hole(id: i64, code: &str, target_class: i64) -> SignatureType {
        SignatureType {
            id,
            signature: Some(code.into()),
            name: code.into(),
            signature_category_id: 1,
            target_class: Some(target_class),
            max_jump_mass: None,
            extra: None,
            spawn_areas: Vec::new(),
            regions: Vec::new(),
            constellations: Vec::new(),
            systems: Vec::new(),
        }
    }

    #[test]
    fn a_known_class_replaces_the_group_of_the_same_code() {
        let types = [
            hole(1, "K162", classes::C1_TO_3),
            hole(2, "K162", 3),
            hole(3, "K162", 5),
            hole(4, "X877", classes::C1_TO_3),
            hole(5, "O477", 3),
        ];
        let ids = |class| -> Vec<i64> {
            leading_to(types.iter().collect(), class)
                .iter()
                .map(|t| t.id)
                .collect()
        };
        assert_eq!(ids(3), [2, 4, 5]);
        // No K162 names a C2 here, so the group stays.
        assert_eq!(ids(2), [1, 4]);
    }

    #[test]
    fn a_search_matches_code_name_note_and_destination_in_any_order() {
        let mut frigate = hole(1, "Q003", 9);
        frigate.name = "Wormhole Q003".into();
        frigate.extra = Some("Frigate".into());
        assert!(matches_query(&frigate, ""));
        assert!(matches_query(&frigate, "q0"));
        assert!(matches_query(&frigate, "frig N"));
        assert!(matches_query(&frigate, "  wormhole  "));
        assert!(!matches_query(&frigate, "frig c6"));
    }

    fn sig(id: &str, name: Option<&str>) -> Signature {
        Signature {
            signature_id: id.into(),
            group: "data".into(),
            name: name.map(str::to_string),
            ..Default::default()
        }
    }

    fn sorted(sigs: &Sigs, rows: &[Signature]) -> Vec<String> {
        let rows: Vec<&Signature> = rows.iter().collect();
        sigs.shown(&rows)
            .into_iter()
            .map(|s| s.signature_id.clone())
            .collect()
    }

    fn going_up(column: SortColumn) -> Sigs {
        Sigs {
            list: ListView {
                hidden: Vec::new(),
                sort: column,
                descending: false,
            },
            ..Default::default()
        }
    }

    fn going_down(column: SortColumn) -> Sigs {
        let mut sigs = going_up(column);
        sigs.list.descending = true;
        sigs
    }

    #[test]
    fn sorts_by_type_with_missing_names_last_going_up_and_first_going_down() {
        let rows = [
            sig("AAA-100", None),
            sig("BBB-100", Some("Relic Site")),
            sig("CCC-100", Some("Data Site")),
            sig("DDD-100", Some("Data Site")),
        ];
        assert_eq!(
            sorted(&going_up(SortColumn::Type), &rows),
            ["CCC-100", "DDD-100", "BBB-100", "AAA-100"]
        );
        // Ties stay by scanner id either way.
        assert_eq!(
            sorted(&going_down(SortColumn::Type), &rows),
            ["AAA-100", "BBB-100", "CCC-100", "DDD-100"]
        );
    }

    #[test]
    fn sorts_by_id_both_ways() {
        let rows = [sig("AAA-100", None), sig("BBB-100", None)];
        assert_eq!(
            sorted(&going_up(SortColumn::Id), &rows),
            ["AAA-100", "BBB-100"]
        );
        assert_eq!(
            sorted(&going_down(SortColumn::Id), &rows),
            ["BBB-100", "AAA-100"]
        );
    }

    #[test]
    fn sorts_by_age_newest_first_going_up() {
        let at = |s: &str| s.parse::<DateTime<Utc>>().unwrap();
        let mut old = sig("AAA-100", None);
        old.updated_at = at("2026-10-01T00:00:00Z");
        let mut new = sig("BBB-100", None);
        new.updated_at = at("2026-10-02T00:00:00Z");
        // A wormhole's age is when it was found, not when it was last touched.
        let mut hole = sig("CCC-100", None);
        hole.group = "wormhole".into();
        hole.created_at = at("2026-09-01T00:00:00Z");
        hole.updated_at = at("2026-10-03T00:00:00Z");
        assert_eq!(
            sorted(&going_up(SortColumn::Age), &[old, new, hole]),
            ["BBB-100", "AAA-100", "CCC-100"]
        );
    }

    #[test]
    fn leaves_out_hidden_categories() {
        let mut hole = sig("BBB-100", None);
        hole.group = "wormhole".into();
        let mut sigs = going_up(SortColumn::Id);
        sigs.list.hidden = vec!["data".into()];
        assert_eq!(sorted(&sigs, &[sig("AAA-100", None), hole]), ["BBB-100"]);
    }
}
