//! The map, drawn the way the website draws it: every system where the map has it (dragged
//! into shape, or laid out as a tree), the same nodes and the same lines and badges, on the
//! same canvas. Drag or scroll to pan, Ctrl and scroll to zoom; it follows you from system
//! to system. A port of `SystemNode.svelte` and its `system-node/` parts, `MapEdge.svelte`
//! and `MapScreen.svelte`; the layout and routing are in [`tree`] and [`edges`]. Change both
//! together. Click a system to set it as your destination.

mod edges;
mod tree;

use std::cell::RefCell;
use std::cmp::Ordering;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use gpui_kit::component::{ActiveTheme as _, Theme};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, App, Bounds, BoxShadow, Div, ElementId, Entity, FontWeight, Hsla, Image,
    ImageFormat, InteractiveElement, IntoElement, MouseButton, ParentElement, PathBuilder, Pixels,
    ScrollDelta, StatefulInteractiveElement, Styled, Window, canvas, div, fill, img, point, px,
    size, svg,
};

use self::edges::{CORNER_RADIUS, Geometry, Link, NODE_W, P, Shape};
use crate::api::{
    Connection, ConnectionKind, MapInfo, MapSnapshot, MassStatus, Sovereignty, Static, TimeStatus,
    WormholeSize,
};
use crate::look;
use crate::overlay::Floating;
use crate::sigs::{Command, Grab, Sigs};
use crate::theme::{self, oklch};

const ZOOM_MIN: f32 = 0.5;
const ZOOM_MAX: f32 = 2.0;
const ZOOM_STEP: f32 = 0.1;
/// How far the cursor moves before a press on the map is a drag rather than a click.
const DRAG_SLOP: f32 = 3.0;

/// How the map places its systems.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Layout {
    /// Where people dragged them.
    Manual,
    /// Drawn as a tree from the connections.
    Tree,
}

/// What the map's settings say about drawing it.
#[derive(Debug, Clone)]
pub(crate) struct Prefs {
    /// Your own placement for the map, when it lets you choose.
    pub layout_override: Option<String>,
    pub show_threat: bool,
    pub grid: crate::jump::Grid,
}

impl Default for Prefs {
    fn default() -> Self {
        Prefs {
            layout_override: None,
            show_threat: true,
            grid: crate::jump::Grid::default(),
        }
    }
}

/// Where the card looks and how close: kept between frames, changed by the mouse.
#[derive(Default)]
pub(crate) struct View {
    /// Where the world's origin sits in the drawing area, in pixels.
    pan: Option<P>,
    zoom: Option<f32>,
    /// The system the view last moved to, so it follows you when you jump.
    centered_on: Option<i64>,
    drag: Option<Drag>,
    /// The drawing area at its last paint, in window coordinates.
    area: Option<Bounds<Pixels>>,
    /// Scroll with Ctrl held, gathered until it makes a zoom step.
    wheel: f32,
    cache: Option<Cached>,
}

struct Drag {
    from: P,
    pan: P,
    moved: bool,
    held: bool,
}

/// Positions and lines for one snapshot, which only change when the map does.
struct Cached {
    key: (usize, Layout, u32),
    positions: Rc<HashMap<i64, P>>,
    edges: Rc<[Geometry]>,
}

impl View {
    fn zoom(&self) -> f32 {
        self.zoom.unwrap_or(1.0)
    }

    /// Zoom by whole steps, keeping `anchor` (in the drawing area) where it is.
    fn zoom_by(&mut self, steps: f32, anchor: P, pan: P) {
        let z = self.zoom();
        let next = (((z + steps * ZOOM_STEP) * 10.0).round() / 10.0).clamp(ZOOM_MIN, ZOOM_MAX);
        if (next - z).abs() < f32::EPSILON {
            return;
        }
        let world = ((anchor.0 - pan.0) / z, (anchor.1 - pan.1) / z);
        self.pan = Some((anchor.0 - world.0 * next, anchor.1 - world.1 * next));
        self.zoom = Some(next);
    }

    fn area_size(&self) -> Option<P> {
        self.area
            .map(|b| (f32::from(b.size.width), f32::from(b.size.height)))
    }

    fn geometry(
        &mut self,
        snapshot: &Arc<MapSnapshot>,
        places: &[Place],
        layout: Layout,
        cell: f32,
    ) -> (Rc<HashMap<i64, P>>, Rc<[Geometry]>) {
        let key = (Arc::as_ptr(snapshot).addr(), layout, cell.to_bits());
        if let Some(c) = self.cache.as_ref().filter(|c| c.key == key) {
            return (Rc::clone(&c.positions), Rc::clone(&c.edges));
        }
        let positions = Rc::new(positions(snapshot, places, layout, cell));
        let links: Vec<Link> = snapshot
            .connections
            .iter()
            .map(|c| Link {
                id: c.id,
                from: c.from_system,
                to: c.to_system,
            })
            .collect();
        let node_h = 2.0 * cell;
        let edges: Rc<[Geometry]> = match layout {
            Layout::Tree => edges::tree(&links, &positions, node_h),
            Layout::Manual => edges::free(&links, &positions, node_h),
        }
        .into();
        self.cache = Some(Cached {
            key,
            positions: Rc::clone(&positions),
            edges: Rc::clone(&edges),
        });
        (positions, edges)
    }
}

/// A system or a ghost, with what its node shows.
#[derive(Debug, Clone, Default)]
#[expect(clippy::struct_excessive_bools, reason = "the map's flags on a system")]
struct Place {
    id: i64,
    /// `None` for a hole nobody has been through.
    solar_system_id: Option<i64>,
    at: P,
    class: Option<i64>,
    security: Option<f64>,
    alias: Option<String>,
    name: Option<String>,
    region: Option<String>,
    statics: Vec<Static>,
    status: Option<String>,
    occupier: Option<String>,
    home: bool,
    rally: bool,
    pinned: bool,
    shattered: bool,
    threat: Option<String>,
    sovereignty: Option<Sovereignty>,
    effect: Option<String>,
}

#[expect(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    reason = "map positions are a few thousand pixels at most"
)]
fn narrow(v: f64) -> f32 {
    v as f32
}

#[expect(
    clippy::as_conversions,
    clippy::cast_precision_loss,
    reason = "a handful of badges and letters"
)]
fn float(i: usize) -> f32 {
    i as f32
}

fn named(a: Option<&String>) -> Option<String> {
    a.filter(|a| !a.is_empty()).cloned()
}

/// Every system and ghost on the map, in the server's order, which is by id.
fn places(snapshot: &MapSnapshot) -> Vec<Place> {
    let systems = snapshot.systems.iter().map(|s| Place {
        id: s.id,
        solar_system_id: Some(s.solar_system_id),
        at: (narrow(s.position_x), narrow(s.position_y)),
        class: s.wormhole_class_id,
        security: Some(s.security_status),
        alias: named(s.alias.as_ref()),
        name: Some(s.name.clone()),
        region: Some(s.region.clone()),
        statics: s.statics.clone(),
        status: s.status.clone(),
        occupier: named(s.occupying_group.as_ref()),
        home: s.is_home,
        rally: s.is_rally,
        pinned: s.is_pinned,
        shattered: s.is_shattered,
        threat: s.threat_level.clone(),
        sovereignty: s.sovereignty.clone(),
        effect: s.effect_name.clone(),
    });
    let ghosts = snapshot.ghosts.iter().map(|g| Place {
        id: g.id,
        at: (narrow(g.position_x), narrow(g.position_y)),
        alias: named(g.alias.as_ref()),
        status: g.status.clone(),
        home: g.is_home,
        rally: g.is_rally,
        pinned: g.is_pinned,
        ..Place::default()
    });
    let mut all: Vec<Place> = systems.chain(ghosts).collect();
    all.sort_by_key(|p| p.id);
    all
}

/// `layoutMode`: the map's placement, unless it lets you pick and you have.
fn layout_mode(map: &MapInfo, own: Option<&str>) -> Layout {
    let mode = match own {
        Some(own @ ("manual" | "tree")) if map.allow_layout_override => own,
        _ => map.layout.as_str(),
    };
    if mode == "tree" {
        Layout::Tree
    } else {
        Layout::Manual
    }
}

/// `compareForTree`: unaliased first, then by alias, class, region and name.
fn compare_for_tree(by_id: &HashMap<i64, &Place>, a: i64, b: i64) -> Ordering {
    let (Some(l), Some(r)) = (by_id.get(&a), by_id.get(&b)) else {
        return a.cmp(&b);
    };
    match (l.alias.is_some(), r.alias.is_some()) {
        (true, false) => return Ordering::Greater,
        (false, true) => return Ordering::Less,
        _ => {}
    }
    let rank = |p: &Place| {
        if p.solar_system_id.is_none() {
            1000.0
        } else {
            tree::class_rank(p.class, p.security)
        }
    };
    tree::locale_cmp(
        l.alias.as_deref().unwrap_or(""),
        r.alias.as_deref().unwrap_or(""),
    )
    .then_with(|| rank(l).total_cmp(&rank(r)))
    .then_with(|| {
        tree::locale_cmp(
            l.region.as_deref().unwrap_or(""),
            r.region.as_deref().unwrap_or(""),
        )
    })
    .then_with(|| {
        tree::locale_cmp(
            l.name.as_deref().unwrap_or(""),
            r.name.as_deref().unwrap_or(""),
        )
    })
    .then(a.cmp(&b))
}

/// Where each node's top-left is, in world units: the tree layout's, or where it was dragged.
fn positions(
    snapshot: &MapSnapshot,
    places: &[Place],
    layout: Layout,
    cell: f32,
) -> HashMap<i64, P> {
    if layout == Layout::Manual {
        return places.iter().map(|p| (p.id, p.at)).collect();
    }
    let by_id: HashMap<i64, &Place> = places.iter().map(|p| (p.id, p)).collect();
    let compare = |a: i64, b: i64| compare_for_tree(&by_id, a, b);
    let input = tree::Input {
        node_ids: places.iter().map(|p| p.id).collect(),
        edges: snapshot
            .connections
            .iter()
            .map(|c| (c.from_system, c.to_system))
            .collect(),
        root_ids: places.iter().filter(|p| p.pinned).map(|p| p.id).collect(),
        home_id: places.iter().find(|p| p.home).map(|p| p.id),
        compare: &compare,
    };
    tree::layout(&input, f64::from(cell))
        .into_iter()
        .map(|(id, (x, y))| (id, (narrow(x), narrow(y))))
        .collect()
}

pub(crate) fn panel(entity: &Entity<Sigs>, grab: Option<Grab>, cx: &App) -> AnyElement {
    let theme = cx.theme().clone();
    let sigs = entity.read(cx);
    let snapshot = sigs.snapshot.clone();
    let places = snapshot.as_deref().map(places).unwrap_or_default();
    let view = Rc::clone(&sigs.chain);
    let zoom = view.borrow().zoom();
    let title = div()
        .flex()
        .items_center()
        .gap_2()
        .child("CHAIN")
        .when(!places.is_empty(), |d| d.child(look::count(places.len())));
    let actions = (!places.is_empty()).then(|| controls(&view, zoom, &theme));
    let header = look::header(entity, Floating::Chain, title, actions, grab, &theme);
    let frame = div()
        .size_full()
        .flex()
        .flex_col()
        .font_family(look::SANS)
        .child(header);
    let Some(snapshot) = snapshot.filter(|_| !places.is_empty()) else {
        return frame
            .child(
                div()
                    .flex_1()
                    .px(px(12.0))
                    .py(px(16.0))
                    .text_size(px(12.0))
                    .text_color(theme.muted_foreground)
                    .child(if sigs.snapshot.is_none() {
                        "No map yet."
                    } else {
                        "Nothing on the map yet."
                    }),
            )
            .into_any_element();
    };
    frame
        .child(map(entity, sigs, &snapshot, &places, &theme))
        .into_any_element()
}

/// Zoom out, how far, zoom in, and back to where you are.
fn controls(view: &Rc<RefCell<View>>, zoom: f32, theme: &Theme) -> AnyElement {
    let button = |id: &'static str, path: &'static str, act: Box<dyn Fn(&mut View)>| {
        let view = Rc::clone(view);
        div()
            .id(id)
            .p(px(2.0))
            .rounded(px(4.0))
            .cursor_pointer()
            .hover(|s| s.bg(theme.muted))
            .on_click(move |_, window, _| {
                act(&mut view.borrow_mut());
                window.refresh();
            })
            .child(
                svg()
                    .path(path)
                    .size(px(12.0))
                    .text_color(theme.muted_foreground),
            )
    };
    let step = |steps: f32| -> Box<dyn Fn(&mut View)> {
        Box::new(move |v: &mut View| {
            let (Some(pan), Some(area)) = (v.pan, v.area_size()) else {
                return;
            };
            v.zoom_by(steps, (area.0 / 2.0, area.1 / 2.0), pan);
        })
    };
    div()
        .flex()
        .items_center()
        .gap_1()
        .child(button("chain-zoom-out", "icons/minus.svg", step(-1.0)))
        .child(
            div()
                .w(px(36.0))
                .flex()
                .justify_center()
                .child(format!("{}%", (zoom * 100.0).round())),
        )
        .child(button("chain-zoom-in", "icons/plus.svg", step(1.0)))
        .child(button(
            "chain-locate",
            "icons/locate-fixed.svg",
            Box::new(|v: &mut View| {
                v.pan = None;
                v.centered_on = None;
            }),
        ))
        .into_any_element()
}

/// What the nodes count: pilots, signatures, the holes nobody has mapped, the scanner ids
/// the ghosts are known by.
struct Tally<'a> {
    pilots: HashMap<i64, Vec<&'a str>>,
    /// Signatures and the uncategorised ones, by solar system.
    signatures: HashMap<i64, (usize, usize)>,
    wormholes: HashMap<i64, usize>,
    /// Connections touching each placement.
    holes: HashMap<i64, usize>,
    ghost_signatures: HashMap<i64, String>,
}

impl<'a> Tally<'a> {
    fn of(snapshot: &'a MapSnapshot, nav: Option<&'a crate::nav::Nav>) -> Tally<'a> {
        let mut pilots: HashMap<i64, Vec<&str>> = HashMap::new();
        for p in nav.iter().flat_map(|n| &n.pilots) {
            if let Some(id) = p.solar_system_id {
                pilots.entry(id).or_default().push(&p.name);
            }
        }
        let mut signatures: HashMap<i64, (usize, usize)> = HashMap::new();
        let mut wormholes: HashMap<i64, usize> = HashMap::new();
        for s in &snapshot.signatures {
            let e = signatures.entry(s.solar_system_id).or_default();
            e.0 += 1;
            if s.group == "unknown" {
                e.1 += 1;
            }
            if s.group == "wormhole" {
                *wormholes.entry(s.solar_system_id).or_default() += 1;
            }
        }
        let mut holes: HashMap<i64, usize> = HashMap::new();
        for c in &snapshot.connections {
            *holes.entry(c.from_system).or_default() += 1;
            *holes.entry(c.to_system).or_default() += 1;
        }
        let ghosts: std::collections::HashSet<i64> = snapshot.ghosts.iter().map(|g| g.id).collect();
        let mut ghost_signatures = HashMap::new();
        for c in &snapshot.connections {
            let Some(sig) = snapshot
                .signatures
                .iter()
                .find(|s| s.connection_id == Some(c.id))
            else {
                continue;
            };
            for end in [c.to_system, c.from_system] {
                if ghosts.contains(&end) {
                    ghost_signatures.insert(end, sig.signature_id.clone());
                }
            }
        }
        Tally {
            pilots,
            signatures,
            wormholes,
            holes,
            ghost_signatures,
        }
    }

    fn marks(&self, p: &Place, here: Option<i64>, prefs: &Prefs, node_h: f32) -> Marks<'_> {
        let id = p.solar_system_id.unwrap_or(0);
        let pilots = self.pilots.get(&id).map_or(&[][..], Vec::as_slice);
        let (signatures, uncategorized) = self.signatures.get(&id).copied().unwrap_or_default();
        let wormholes = self.wormholes.get(&id).copied().unwrap_or(0);
        Marks {
            active: Some(p.id) == here,
            pilots,
            signatures,
            uncategorized,
            unmapped: wormholes.saturating_sub(self.holes.get(&p.id).copied().unwrap_or(0)),
            signature_id: self.ghost_signatures.get(&p.id).map(String::as_str),
            show_threat: prefs.show_threat,
            height: if pilots.is_empty() {
                node_h
            } else {
                node_h + 20.0
            },
        }
    }
}

/// Where the world is drawn in the card: world coordinates times `z`, plus `pan`.
#[derive(Clone, Copy)]
struct Camera {
    pan: P,
    z: f32,
    area: P,
}

impl Camera {
    fn screen(self, p: P) -> P {
        (p.0 * self.z + self.pan.0, p.1 * self.z + self.pan.1)
    }

    /// Whether a box at `p`, `w` by `h` pixels, shows in the area at all.
    fn shows(self, p: P, w: f32, h: f32) -> bool {
        p.0 + w >= -20.0
            && p.1 + h >= -20.0
            && p.0 <= self.area.0 + 20.0
            && p.1 <= self.area.1 + 20.0
    }
}

/// Where the card looks: on you, and with you when you jump; without you, on home, else
/// the first system; otherwise where it was panned.
fn camera(
    view: &mut View,
    sigs: &Sigs,
    places: &[Place],
    positions: &HashMap<i64, P>,
    node_h: f32,
) -> Camera {
    let here = sigs.system().map(|s| s.id);
    let z = view.zoom();
    let area = view.area_size().unwrap_or_else(|| {
        let (w, h) = Floating::Chain.place(sigs).dimensions();
        (w - 2.0, h - 38.0)
    });
    let focus = here
        .or_else(|| places.iter().find(|p| p.home).map(|p| p.id))
        .or_else(|| places.first().map(|p| p.id));
    let recenter = view.pan.is_none() || (here.is_some() && view.centered_on != here);
    let pan = match (recenter, focus.and_then(|f| positions.get(&f))) {
        (true, Some(at)) => {
            let pan = (
                area.0 / 2.0 - (at.0 + NODE_W / 2.0) * z,
                area.1 / 2.0 - (at.1 + node_h / 2.0) * z,
            );
            // Until the area has been measured this is a guess; the next frame centres again.
            if view.area.is_some() {
                view.pan = Some(pan);
                view.centered_on = here;
            }
            pan
        }
        _ => view.pan.unwrap_or((0.0, 0.0)),
    };
    Camera { pan, z, area }
}

/// The connections' lines in pixels, and the grid when the map is dragged into shape.
fn lines(
    geometry: &[Geometry],
    connections: &HashMap<i64, &Connection>,
    cam: Camera,
    grid: Option<&crate::jump::Grid>,
    theme: &Theme,
) -> (Vec<Line>, Option<Grid>) {
    let lines = geometry
        .iter()
        .filter_map(|g| {
            let c = connections.get(&g.id)?;
            let shape = match g.shape {
                Shape::Elbow(points) => Shape::Elbow(points.map(|p| cam.screen(p))),
                Shape::Curve(a, b) => Shape::Curve(cam.screen(a), cam.screen(b)),
            };
            Some(Line {
                shape,
                color: edge_color(c, theme),
                dash: dashed(c).then_some(cam.z),
            })
        })
        .collect();
    let grid = grid.map(|g| Grid {
        origin: cam.pan,
        cell: narrow(g.cell_size) * cam.z,
        world: (
            narrow(g.world_width) * cam.z,
            narrow(g.world_height) * cam.z,
        ),
        color: if theme.mode.is_dark() {
            oklch(0.24, 0.0, 0.0)
        } else {
            oklch(0.90, 0.0, 0.0)
        },
    });
    (lines, grid)
}

/// The canvas: the lines and badges under the nodes, the grid under it all when the map is
/// dragged into shape.
fn map(
    entity: &Entity<Sigs>,
    sigs: &Sigs,
    snapshot: &Arc<MapSnapshot>,
    places: &[Place],
    theme: &Theme,
) -> impl IntoElement {
    let prefs = &sigs.map_look;
    let layout = layout_mode(&snapshot.map, prefs.layout_override.as_deref());
    let cell = narrow(prefs.grid.cell_size).max(1.0);
    let node_h = 2.0 * cell;
    let here = sigs.system().map(|s| s.id);
    let view_rc = Rc::clone(&sigs.chain);
    let mut view = view_rc.borrow_mut();
    let (positions, geometry) = view.geometry(snapshot, places, layout, cell);
    let cam = camera(&mut view, sigs, places, &positions, node_h);
    drop(view);

    let tally = Tally::of(snapshot, sigs.nav.as_ref());
    let connections: HashMap<i64, &Connection> =
        snapshot.connections.iter().map(|c| (c.id, c)).collect();
    let now = chrono::Utc::now();
    let (lines, grid) = lines(
        &geometry,
        &connections,
        cam,
        (layout == Layout::Manual).then_some(&prefs.grid),
        theme,
    );
    let drawing = drawing(lines, grid, cam.z, Rc::clone(&view_rc));
    let badges = geometry.iter().filter_map(|g| {
        let c = connections.get(&g.id)?;
        let at = cam.screen(g.center);
        cam.shows((at.0 - 60.0, at.1 - 10.0), 120.0, 20.0)
            .then(|| badge(c, at, cam.z, now, theme))
            .flatten()
    });
    let nodes = places.iter().filter_map(|p| {
        let at = cam.screen(*positions.get(&p.id)?);
        let marks = tally.marks(p, here, prefs, node_h);
        cam.shows(at, NODE_W * cam.z, marks.height * cam.z)
            .then(|| node(entity, &view_rc, p, at, cam.z, &marks, &sigs.images, theme))
    });

    let canvas_color = if theme.mode.is_dark() {
        oklch(0.145, 0.0, 0.0)
    } else {
        oklch(0.968, 0.0, 0.0)
    };
    gestures(div().id("chain-map"), &view_rc)
        .flex_1()
        .min_h_0()
        .relative()
        .overflow_hidden()
        .bg(canvas_color)
        .child(drawing)
        .children(badges)
        .children(nodes)
}

/// Press and drag to pan, scroll to pan, Ctrl and scroll to zoom where the cursor is.
fn gestures(d: gpui_kit::Stateful<Div>, view: &Rc<RefCell<View>>) -> gpui_kit::Stateful<Div> {
    let down = Rc::clone(view);
    let moving = Rc::clone(view);
    let up = Rc::clone(view);
    let wheel = Rc::clone(view);
    d.on_mouse_down(MouseButton::Left, move |e, _, _| {
        let mut v = down.borrow_mut();
        let pan = v.pan.unwrap_or((0.0, 0.0));
        v.drag = Some(Drag {
            from: (f32::from(e.position.x), f32::from(e.position.y)),
            pan,
            moved: false,
            held: true,
        });
    })
    .on_mouse_move(move |e, window, _| {
        let mut v = moving.borrow_mut();
        let Some(d) = v.drag.as_mut().filter(|d| d.held) else {
            return;
        };
        if e.pressed_button != Some(MouseButton::Left) {
            d.held = false;
            return;
        }
        let dx = f32::from(e.position.x) - d.from.0;
        let dy = f32::from(e.position.y) - d.from.1;
        if dx.hypot(dy) > DRAG_SLOP {
            d.moved = true;
        }
        if d.moved {
            let pan = (d.pan.0 + dx, d.pan.1 + dy);
            v.pan = Some(pan);
            window.refresh();
        }
    })
    .on_mouse_up(MouseButton::Left, move |_, _, _| {
        if let Some(d) = up.borrow_mut().drag.as_mut() {
            d.held = false;
        }
    })
    .on_scroll_wheel(move |e, window, _| {
        let mut v = wheel.borrow_mut();
        let Some(pan) = v.pan else {
            return;
        };
        if e.modifiers.control || e.modifiers.platform {
            let steps = match e.delta {
                ScrollDelta::Lines(l) => l.y.signum(),
                ScrollDelta::Pixels(p) => {
                    v.wheel += f32::from(p.y);
                    if v.wheel.abs() < 40.0 {
                        return;
                    }
                    let steps = v.wheel.signum();
                    v.wheel = 0.0;
                    steps
                }
            };
            let anchor = v.area.map_or((0.0, 0.0), |a| {
                (
                    f32::from(e.position.x - a.origin.x),
                    f32::from(e.position.y - a.origin.y),
                )
            });
            v.zoom_by(steps, anchor, pan);
        } else {
            let delta = e.delta.pixel_delta(px(20.0));
            v.pan = Some((pan.0 + f32::from(delta.x), pan.1 + f32::from(delta.y)));
        }
        window.refresh();
    })
}

/// A connection's line, in pixels within the drawing area.
struct Line {
    shape: Shape,
    color: Hsla,
    /// The dash pattern's unit, for a degraded hole.
    dash: Option<f32>,
}

/// The manual map's grid: lines every cell across the world, in pixels.
struct Grid {
    origin: P,
    cell: f32,
    world: P,
    color: Hsla,
}

/// The grid and the lines, under the nodes. Notes the area it was given, and draws again
/// when that changed.
fn drawing(
    lines: Vec<Line>,
    grid: Option<Grid>,
    z: f32,
    view: Rc<RefCell<View>>,
) -> impl IntoElement {
    canvas(
        |_, _, _| {},
        move |bounds: Bounds<Pixels>, (), window, _| {
            let changed = view.borrow().area != Some(bounds);
            if changed {
                view.borrow_mut().area = Some(bounds);
                window.refresh();
            }
            let at = |(x, y): P| point(bounds.origin.x + px(x), bounds.origin.y + px(y));
            if let Some(g) = &grid {
                paint_grid(g, bounds, z, window);
            }
            for line in &lines {
                match line.shape {
                    Shape::Elbow(points) => {
                        let path = elbow_path(&points, CORNER_RADIUS * z, at, line.dash, 1.5 * z);
                        if let Ok(path) = path.build() {
                            window.paint_path(path, line.color);
                        }
                    }
                    Shape::Curve(from, to) => {
                        let (c1, c2) = edges::curve_controls(from, to);
                        let mut path = stroke(4.0 * z, line.dash);
                        path.move_to(at(from));
                        path.cubic_bezier_to(at(to), at(c1), at(c2));
                        if let Ok(path) = path.build() {
                            window.paint_path(path, line.color);
                        }
                        let r = 4.0 * z;
                        for end in [from, to] {
                            let dot = Bounds::new(
                                at((end.0 - r, end.1 - r)),
                                size(px(2.0 * r), px(2.0 * r)),
                            );
                            window.paint_quad(fill(dot, line.color).corner_radii(px(r)));
                        }
                    }
                }
            }
        },
    )
    .absolute()
    .top_0()
    .left_0()
    .size_full()
}

fn paint_grid(grid: &Grid, bounds: Bounds<Pixels>, zoom: f32, window: &mut Window) {
    if grid.cell < 2.0 {
        return;
    }
    let (origin, cell) = (grid.origin, grid.cell);
    let left = origin.0.max(0.0);
    let top = origin.1.max(0.0);
    let right = (origin.0 + grid.world.0).min(f32::from(bounds.size.width));
    let bottom = (origin.1 + grid.world.1).min(f32::from(bounds.size.height));
    if right <= left || bottom <= top {
        return;
    }
    let thickness = zoom.max(0.5);
    let quad = |x: f32, y: f32, across: f32, down: f32| {
        fill(
            Bounds::new(
                point(bounds.origin.x + px(x), bounds.origin.y + px(y)),
                size(px(across), px(down)),
            ),
            grid.color,
        )
    };
    let mut x = origin.0 + ((left - origin.0) / cell).ceil() * cell;
    while x < right {
        window.paint_quad(quad(x, top, thickness, bottom - top));
        x += cell;
    }
    let mut y = origin.1 + ((top - origin.1) / cell).ceil() * cell;
    while y < bottom {
        window.paint_quad(quad(left, y, right - left, thickness));
        y += cell;
    }
}

/// A stroke, dashed "2 6" in units of `dash` for a degraded hole.
fn stroke(width: f32, dash: Option<f32>) -> PathBuilder {
    let path = PathBuilder::stroke(px(width));
    match dash {
        Some(unit) => path.dash_array(&[px(2.0 * unit), px(6.0 * unit)]),
        None => path,
    }
}

/// `roundedPath`: the elbow through its corners, each rounded.
fn elbow_path(
    points: &[P],
    radius: f32,
    at: impl Fn(P) -> gpui_kit::Point<Pixels>,
    dash: Option<f32>,
    width: f32,
) -> PathBuilder {
    let mut path = stroke(width, dash);
    let mut pts: Vec<P> = Vec::with_capacity(points.len());
    for &p in points {
        if pts
            .last()
            .is_none_or(|q: &P| (p.0 - q.0).hypot(p.1 - q.1) > 0.01)
        {
            pts.push(p);
        }
    }
    let (Some(&first), Some(&last)) = (pts.first(), pts.last()) else {
        return path;
    };
    if pts.len() < 2 {
        return path;
    }
    path.move_to(at(first));
    for i in 1..pts.len() - 1 {
        let (prev, curr, next) = (pts[i - 1], pts[i], pts[i + 1]);
        let len_in = (curr.0 - prev.0).hypot(curr.1 - prev.1);
        let len_out = (next.0 - curr.0).hypot(next.1 - curr.1);
        let r = radius.min(len_in / 2.0).min(len_out / 2.0);
        let start = (
            curr.0 + (prev.0 - curr.0) / len_in * r,
            curr.1 + (prev.1 - curr.1) / len_in * r,
        );
        let end = (
            curr.0 + (next.0 - curr.0) / len_out * r,
            curr.1 + (next.1 - curr.1) / len_out * r,
        );
        path.line_to(at(start));
        path.curve_to(at(end), at(curr));
    }
    path.line_to(at(last));
    path
}

/// `edgeColor`: a stargate, then critical, reduced, end of life, else the plain edge.
fn edge_color(c: &Connection, theme: &Theme) -> Hsla {
    match (c.kind, c.mass_status, c.time_status) {
        (ConnectionKind::Stargate, _, _) => look::sky_500(),
        (_, Some(MassStatus::Critical), _)
        | (_, _, Some(TimeStatus::Critical | TimeStatus::Expired)) => theme::critical(),
        (_, Some(MassStatus::Reduced), _) => theme::reduced(),
        (_, _, Some(TimeStatus::Eol)) => theme::eol(),
        _ if theme.mode.is_dark() => oklch(0.371, 0.0, 0.0),
        _ => oklch(0.87, 0.0, 0.0),
    }
}

/// A degraded wormhole is dashed.
fn dashed(c: &Connection) -> bool {
    c.kind == ConnectionKind::Wormhole
        && (matches!(
            c.mass_status,
            Some(MassStatus::Reduced | MassStatus::Critical)
        ) || c.time_status.is_some_and(|t| t != TimeStatus::Stable))
}

/// `formatRemaining`: the largest unit only.
fn remaining(left: chrono::Duration) -> String {
    let minutes = left.num_minutes();
    if left <= chrono::Duration::zero() {
        "0m".into()
    } else if minutes < 1 {
        "<1m".into()
    } else if minutes < 60 {
        format!("{minutes}m")
    } else {
        format!("{}h", minutes / 60)
    }
}

/// The countdown a lifetime mark earns: from when it was set, never an estimate, and not on a
/// hole that is already expiring.
fn time_left(c: &Connection, now: chrono::DateTime<chrono::Utc>) -> Option<String> {
    let after = match c.time_status? {
        TimeStatus::Eol => chrono::Duration::hours(4),
        TimeStatus::Critical => chrono::Duration::hours(1),
        _ => return None,
    };
    let at = c.time_status_updated_at? + after;
    Some(remaining(at - now))
}

/// The badge pill on a line, `MapEdge.svelte`'s: a stargate's orbit, the size letter, the
/// mass and the lifetime with its countdown.
fn badge(
    c: &Connection,
    at: P,
    z: f32,
    now: chrono::DateTime<chrono::Utc>,
    theme: &Theme,
) -> Option<AnyElement> {
    let s = |v: f32| px(v * z);
    let mass = match c.mass_status {
        Some(MassStatus::Reduced) => Some(theme::reduced()),
        Some(MassStatus::Critical) => Some(theme::critical()),
        _ => None,
    };
    let time = match c.time_status {
        Some(TimeStatus::Eol) => Some(theme::eol()),
        Some(TimeStatus::Critical | TimeStatus::Expired) => Some(theme::critical()),
        _ => None,
    };
    let wormhole = c.kind == ConnectionKind::Wormhole;
    let letter = wormhole.then_some(c.size).flatten().map(|s| match s {
        WormholeSize::Small => "S",
        WormholeSize::Medium => "M",
        WormholeSize::Large => "L",
        WormholeSize::Xl => "XL",
    });
    let gate = c.kind == ConnectionKind::Stargate;
    let count = usize::from(gate)
        + usize::from(letter.is_some())
        + usize::from(mass.is_some())
        + usize::from(time.is_some());
    if count == 0 {
        return None;
    }
    let left = time.and(time_left(c, now));
    let width =
        float(count) * 18.0 + 8.0 + left.as_ref().map_or(0.0, |t| float(t.len()) * 6.0 + 4.0);
    let icon = |path: &'static str, color: Hsla| {
        svg().flex_none().path(path).size(s(14.0)).text_color(color)
    };
    let dark = theme.mode.is_dark();
    let pill = div()
        .absolute()
        .left(px(at.0 - width * z / 2.0))
        .top(px(at.1 - 10.0 * z))
        .w(s(width))
        .h(s(20.0))
        .flex()
        .items_center()
        .justify_center()
        .gap(s(2.0))
        .px(s(4.0))
        .rounded_full()
        .border_1()
        .border_color(oklch(if dark { 0.371 } else { 0.87 }, 0.0, 0.0))
        .bg(oklch(if dark { 0.205 } else { 1.0 }, 0.0, 0.0))
        .when(gate, |d| d.child(icon("icons/orbit.svg", look::sky_500())))
        .when_some(letter, |d, l| {
            d.child(
                div()
                    .text_size(s(13.0))
                    .line_height(s(13.0))
                    .font_weight(FontWeight::BOLD)
                    .text_color(oklch(0.556, 0.0, 0.0))
                    .child(l),
            )
        })
        .when_some(mass, |d, color| d.child(icon("icons/weight.svg", color)))
        .when_some(time, |d, color| {
            d.child(icon("icons/clock.svg", color))
                .when_some(left, |d, left| {
                    d.child(
                        div()
                            .text_size(s(10.0))
                            .line_height(s(10.0))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(color)
                            .child(left),
                    )
                })
        });
    Some(pill.into_any_element())
}

/// What a node shows beyond its system.
struct Marks<'a> {
    /// The system you are in: the amber ring.
    active: bool,
    pilots: &'a [&'a str],
    signatures: usize,
    uncategorized: usize,
    unmapped: usize,
    /// The scanner id a ghost is known by.
    signature_id: Option<&'a str>,
    show_threat: bool,
    /// In world units: two cells, and a row more with pilots.
    height: f32,
}

fn status_icon(status: &str) -> Option<&'static str> {
    match status {
        "friendly" => Some("icons/shield-check.svg"),
        "hostile" => Some("icons/skull.svg"),
        "active" => Some("icons/activity.svg"),
        "unscanned" => Some("icons/radar.svg"),
        "empty" => Some("icons/circle-dashed.svg"),
        _ => None,
    }
}

fn threat_color(threat: Option<&str>) -> Option<Hsla> {
    match threat {
        Some("critical") => Some(oklch(0.577, 0.245, 27.325)),
        Some("high") => Some(oklch(0.705, 0.213, 47.604)),
        _ => None,
    }
}

fn pink_500() -> Hsla {
    oklch(0.656, 0.241, 354.308)
}

fn yellow_500() -> Hsla {
    oklch(0.795, 0.184, 86.047)
}

/// A ring round the node, like Tailwind's `ring`: `inset` pixels outside its border box.
fn ring(inset: f32, width: f32, radius: f32, color: Hsla) -> Div {
    // Absolute children are placed inside the 1px border.
    let out = inset + 1.0;
    div()
        .absolute()
        .top(px(-out))
        .left(px(-out))
        .right(px(-out))
        .bottom(px(-out))
        .rounded(px(radius))
        .border(px(width))
        .border_color(color)
}

/// The rings round a node: amber round yours, the threat level's round a busy one, and
/// yellow under the cursor.
fn rings(p: &Place, marks: &Marks<'_>, z: f32, theme: &Theme) -> Vec<Div> {
    let mut rings = Vec::new();
    if marks.active {
        // `ring-2 ring-amber-500 ring-offset-2 ring-offset-background`.
        let background = oklch(if theme.mode.is_dark() { 0.145 } else { 1.0 }, 0.0, 0.0);
        rings.push(ring(2.0 * z, 2.0 * z, 6.0 * z, background));
        rings.push(ring(4.0 * z, 2.0 * z, 8.0 * z, look::amber_500()));
    } else if let Some(color) = marks
        .show_threat
        .then(|| threat_color(p.threat.as_deref()))
        .flatten()
    {
        rings.push(ring(2.0 * z, 2.0 * z, 6.0 * z, color));
    }
    // `hover:outline-2 hover:outline-yellow-500`.
    rings.push(
        ring(2.0 * z, 2.0 * z, 6.0 * z, gpui_kit::transparent_black())
            .group_hover("chain-node", |s| s.border_color(yellow_500())),
    );
    rings
}

/// One system, `SystemNode.svelte`: the status in its border, the class, name and occupier
/// with the icons on the right, the statics or the region under them, and who is there.
#[expect(clippy::too_many_arguments, reason = "one node, drawn from its parts")]
fn node(
    entity: &Entity<Sigs>,
    view: &Rc<RefCell<View>>,
    p: &Place,
    at: P,
    z: f32,
    marks: &Marks<'_>,
    images: &look::Images,
    theme: &Theme,
) -> AnyElement {
    let s = |v: f32| px(v * z);
    let status = look::status_color(p.status.as_deref(), theme);
    let ghost = p.solar_system_id.is_none();
    let dark = theme.mode.is_dark();
    let bg = if p.rally {
        if dark {
            oklch(0.284, 0.109, 3.907)
        } else {
            oklch(0.971, 0.014, 343.198)
        }
    } else {
        look::card(theme)
    };
    let entity = entity.clone();
    let view = Rc::clone(view);
    let destination = p.solar_system_id;
    div()
        .id(ElementId::from((
            "chain-node",
            u64::try_from(p.id).unwrap_or(0),
        )))
        .group("chain-node")
        .absolute()
        .left(px(at.0))
        .top(px(at.1))
        .w(s(NODE_W))
        .h(s(marks.height))
        .flex()
        .flex_col()
        .justify_center()
        .px(s(8.0))
        .py(s(2.0))
        .rounded(s(4.0))
        .border_1()
        .border_color(status)
        .when(ghost, Styled::border_dashed)
        .bg(bg)
        .map(|d| {
            if p.rally {
                d.shadow(vec![BoxShadow {
                    color: pink_500().opacity(0.6),
                    offset: point(px(0.0), px(0.0)),
                    blur_radius: s(10.0),
                    spread_radius: s(2.0),
                    inset: false,
                }])
            } else {
                d.shadow_sm()
            }
        })
        .text_size(s(11.0))
        .line_height(s(13.75))
        .text_color(theme.foreground)
        .children(rings(p, marks, z, theme))
        .when_some(destination, |d, id| {
            d.cursor_pointer().on_click(move |_, _, cx| {
                // The end of a drag across the map is not a click on the node under it.
                if view.borrow().drag.as_ref().is_some_and(|d| d.moved) {
                    return;
                }
                entity.update(cx, |_, cx| cx.emit(Command::SetDestination(id)));
            })
        })
        .child(
            div()
                .flex()
                .min_w_0()
                .items_center()
                .gap(s(4.0))
                .child(look::class_badge(p.class, p.security).font_weight(FontWeight::MEDIUM))
                .child(name(p, marks.signature_id, theme))
                .when_some(p.occupier.clone(), |d, o| {
                    d.child(
                        div()
                            .min_w_0()
                            .truncate()
                            .text_color(theme.muted_foreground)
                            .child(format!("({o})")),
                    )
                })
                .child(icons(p, marks, status, z, images, theme)),
        )
        .child(second_row(p, z, theme))
        .when(!marks.pilots.is_empty(), |d| {
            d.child(pilots_row(marks.pilots, z, theme))
        })
        .into_any_element()
}

/// The alias with the name after it, else the name, else the scanner id, else "Unmapped".
fn name(p: &Place, signature_id: Option<&str>, theme: &Theme) -> Div {
    let muted = theme.muted_foreground;
    match (&p.alias, &p.name, signature_id) {
        (Some(alias), name, sig) => div()
            .flex()
            .min_w_0()
            .gap_1()
            .child(
                div()
                    .flex_none()
                    .font_weight(FontWeight::MEDIUM)
                    .child(alias.clone()),
            )
            .child(
                div().min_w_0().truncate().text_color(muted).child(
                    name.clone()
                        .or_else(|| sig.map(str::to_owned))
                        .unwrap_or_else(|| "Unmapped".into()),
                ),
            ),
        (None, Some(name), _) => div()
            .min_w_0()
            .truncate()
            .font_weight(FontWeight::MEDIUM)
            .child(name.clone()),
        (None, None, Some(sig)) => div()
            .min_w_0()
            .truncate()
            .font_weight(FontWeight::MEDIUM)
            .text_color(muted)
            .child(sig.to_owned()),
        (None, None, None) => div()
            .min_w_0()
            .truncate()
            .font_weight(FontWeight::MEDIUM)
            .italic()
            .text_color(muted)
            .child("Unmapped"),
    }
}

/// `HeaderIcons.svelte`: status, home, rally, pinned, signatures, unmapped holes,
/// shattered, then who holds it or its effect.
fn icons(
    p: &Place,
    marks: &Marks<'_>,
    status: Hsla,
    z: f32,
    images: &look::Images,
    theme: &Theme,
) -> Div {
    let icon = |path: &'static str, color: Hsla| {
        svg()
            .flex_none()
            .path(path)
            .size(px(14.0 * z))
            .text_color(color)
    };
    let mut row = div()
        .ml_auto()
        .flex_none()
        .flex()
        .items_center()
        .gap(px(4.0 * z));
    if let Some(path) = p.status.as_deref().and_then(status_icon) {
        row = row.child(icon(path, status));
    }
    if p.home {
        row = row.child(icon("icons/house.svg", look::amber_400()));
    }
    if p.rally {
        row = row.child(icon("icons/flag.svg", pink_500()));
    }
    if p.pinned {
        row = row.child(icon("icons/lock.svg", theme.muted_foreground));
    }
    if marks.signatures > 0 {
        let color = if marks.uncategorized > 0 {
            oklch(0.645, 0.246, 16.439)
        } else {
            look::amber_500()
        };
        row = row.child(icon("icons/satellite.svg", color));
    }
    if marks.unmapped > 0 {
        row = row.child(icon("icons/fan.svg", look::sky_500()));
    }
    if p.shattered {
        row = row
            .child(icon("icons/aperture.svg", look::amber_500().opacity(0.9)).size(px(12.0 * z)));
    }
    if let Some(sov) = &p.sovereignty {
        row = row.child(look::picture(
            images,
            look::logo(&sov.kind, sov.id),
            16.0 * z,
            theme,
        ));
    } else if let Some(image) = p.effect.as_deref().and_then(effect_icon) {
        // The HUD's icons glow on black, so they keep a dark disc in the light theme too.
        let size = 16.0 * z;
        row = row.child(
            div()
                .flex_none()
                .relative()
                .size(px(size))
                .rounded_full()
                .overflow_hidden()
                .bg(oklch(0.145, 0.0, 0.0))
                .child(
                    img(image)
                        .absolute()
                        .left(px(-size * 0.125))
                        .top(px(-size * 0.125))
                        .size(px(size * 1.25))
                        .rounded_full(),
                ),
        );
    }
    row
}

/// The game client's icon for a wormhole effect, `effectIcon` on the website.
fn effect_icon(name: &str) -> Option<Arc<Image>> {
    let bytes: &'static [u8] = match name {
        "Black Hole" => include_bytes!("../assets/effects/statuseffects_blackhole.png"),
        "Cataclysmic Variable" => {
            include_bytes!("../assets/effects/statuseffects_cataclysmicvariable.png")
        }
        "Magnetar" => include_bytes!("../assets/effects/statuseffects_magnatar.png"),
        "Pulsar" => include_bytes!("../assets/effects/statuseffects_pulsae.png"),
        "Red Giant" => include_bytes!("../assets/effects/statuseffects_redgiant.png"),
        "Wolf-Rayet Star" => include_bytes!("../assets/effects/statuseffects_wolfrayet.png"),
        _ => return None,
    };
    thread_local! {
        static LOADED: RefCell<HashMap<&'static str, Arc<Image>>> = RefCell::new(HashMap::new());
    }
    LOADED.with(|loaded| {
        let mut loaded = loaded.borrow_mut();
        let key: &'static str = match name {
            "Black Hole" => "Black Hole",
            "Cataclysmic Variable" => "Cataclysmic Variable",
            "Magnetar" => "Magnetar",
            "Pulsar" => "Pulsar",
            "Red Giant" => "Red Giant",
            _ => "Wolf-Rayet Star",
        };
        Some(Arc::clone(loaded.entry(key).or_insert_with(|| {
            Arc::new(Image::from_bytes(ImageFormat::Png, bytes.to_vec()))
        })))
    })
}

/// The statics of a wormhole system, else the region (`RegionRow.svelte`).
fn second_row(p: &Place, z: f32, theme: &Theme) -> Div {
    let wormhole = p.class.is_some_and(|c| matches!(c, 1..=6 | 12..=18));
    if wormhole && !p.statics.is_empty() {
        return statics_row(&p.statics, z);
    }
    div()
        .flex()
        .items_center()
        .text_size(px(10.0 * z))
        .line_height(px(12.5 * z))
        .child(
            div()
                .truncate()
                .text_color(theme.muted_foreground)
                .child(p.region.clone().unwrap_or_default()),
        )
}

/// `StaticsRow.svelte`: each static's destination in its color, on the right.
fn statics_row(statics: &[Static], z: f32) -> Div {
    div()
        .flex()
        .items_center()
        .justify_end()
        .gap(px(6.0 * z))
        .text_size(px(10.0 * z))
        .line_height(px(12.5 * z))
        .children(statics.iter().map(|st| {
            let (dest, color) = theme::dest_class_meta(st.dest_class);
            div()
                .font_weight(FontWeight::MEDIUM)
                .text_color(color)
                .child(dest)
        }))
}

/// `PilotsRow.svelte`: a green dot, the first pilot, and how many more.
fn pilots_row(pilots: &[&str], z: f32, theme: &Theme) -> Div {
    let s = |v: f32| px(v * z);
    div()
        .mt(s(2.0))
        .h(s(18.0))
        .flex()
        .items_center()
        .gap(s(6.0))
        .border_t_1()
        .border_color(theme.border)
        .pt(s(2.0))
        .text_size(s(10.0))
        .line_height(s(12.5))
        .child(
            div()
                .flex_none()
                .size(s(4.0))
                .rounded_full()
                .bg(look::green_500()),
        )
        .child(div().min_w_0().truncate().child(pilots[0].to_owned()))
        .when(pilots.len() > 1, |d| {
            d.child(
                div()
                    .flex_none()
                    .text_color(theme.muted_foreground)
                    .child(format!("and {} more", pilots.len() - 1)),
            )
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info(layout: &str, allow: &str) -> MapInfo {
        MapInfo {
            name: "m".into(),
            layout: layout.into(),
            allow_layout_override: allow == "allows",
        }
    }

    #[test]
    fn your_own_placement_only_where_the_map_allows_it() {
        assert_eq!(
            layout_mode(&info("tree", "fixed"), Some("manual")),
            Layout::Tree
        );
        assert_eq!(
            layout_mode(&info("tree", "allows"), Some("manual")),
            Layout::Manual
        );
        assert_eq!(
            layout_mode(&info("manual", "allows"), Some("tree")),
            Layout::Tree
        );
        assert_eq!(layout_mode(&info("manual", "fixed"), None), Layout::Manual);
    }

    fn place(id: i64, alias: Option<&str>, class: Option<i64>, name: &str) -> Place {
        Place {
            id,
            solar_system_id: Some(id),
            alias: alias.map(str::to_owned),
            class,
            security: Some(-1.0),
            name: Some(name.into()),
            region: Some(String::new()),
            ..Place::default()
        }
    }

    #[test]
    fn siblings_sort_like_the_map() {
        let places = [
            place(1, Some("b"), Some(3), "J1"),
            place(2, None, Some(5), "J2"),
            place(3, None, Some(7), "Jita"),
            place(4, Some("a"), Some(3), "J4"),
            Place {
                id: 5,
                ..Place::default()
            },
        ];
        let by_id: HashMap<i64, &Place> = places.iter().map(|p| (p.id, p)).collect();
        let mut ids = vec![1, 2, 3, 4, 5];
        ids.sort_by(|a, b| compare_for_tree(&by_id, *a, *b));
        // Unaliased by class (high-sec first, the ghost last), then the aliased by alias.
        assert_eq!(ids, vec![3, 2, 5, 4, 1]);
    }

    #[test]
    fn a_countdown_shows_its_largest_unit() {
        assert_eq!(remaining(chrono::Duration::minutes(150)), "2h");
        assert_eq!(remaining(chrono::Duration::minutes(45)), "45m");
        assert_eq!(remaining(chrono::Duration::seconds(30)), "<1m");
        assert_eq!(remaining(chrono::Duration::seconds(-5)), "0m");
    }
}
