//! What a jump means for the map, worked out the way the website's jump prompt does. Blocking
//! and without GPUI: the app runs it on the background executor.

use std::collections::HashMap;
use std::sync::Arc;

use crate::api::{
    Client, EndBookmark, MapSnapshot, PlacedSystem, ResolveGhost, Signature, SignatureCatalog,
    SignatureType, SystemHit, TrackJump,
};
use crate::jump::{self, Grid};
use crate::sigs::{Ask, Target};

/// The calls planning a jump makes, so it can run against a fake map.
pub(crate) trait MapApi {
    fn snapshot(&self, map_id: i64) -> anyhow::Result<MapSnapshot>;
    fn find_system(&self, name: &str) -> anyhow::Result<Option<SystemHit>>;
    fn ignored(&self, map_id: i64) -> anyhow::Result<Vec<i64>>;
    fn track_jump(&self, cmd: &TrackJump) -> anyhow::Result<()>;
    fn resolve_ghost(&self, cmd: &ResolveGhost) -> anyhow::Result<()>;
    fn suggest_alias(
        &self,
        map_id: i64,
        origin: i64,
        target_system: i64,
    ) -> anyhow::Result<Option<String>>;
    fn connection_bookmarks(
        &self,
        map_id: i64,
        connection_id: i64,
    ) -> anyhow::Result<Vec<EndBookmark>>;
}

impl MapApi for Client {
    fn snapshot(&self, map_id: i64) -> anyhow::Result<MapSnapshot> {
        Client::snapshot(self, map_id)
    }

    fn find_system(&self, name: &str) -> anyhow::Result<Option<SystemHit>> {
        Client::find_system(self, name)
    }

    fn ignored(&self, map_id: i64) -> anyhow::Result<Vec<i64>> {
        Client::ignored(self, map_id)
    }

    fn track_jump(&self, cmd: &TrackJump) -> anyhow::Result<()> {
        Client::track_jump(self, cmd)
    }

    fn resolve_ghost(&self, cmd: &ResolveGhost) -> anyhow::Result<()> {
        Client::resolve_ghost(self, cmd)
    }

    fn suggest_alias(
        &self,
        map_id: i64,
        origin: i64,
        target_system: i64,
    ) -> anyhow::Result<Option<String>> {
        Client::suggest_alias(self, map_id, origin, target_system)
    }

    fn connection_bookmarks(
        &self,
        map_id: i64,
        connection_id: i64,
    ) -> anyhow::Result<Vec<EndBookmark>> {
        Client::connection_bookmarks(self, map_id, connection_id)
    }
}

/// The hole flown last, which the shortcuts act on.
#[derive(Clone, Debug)]
pub(crate) struct LastJump {
    pub(crate) map_id: i64,
    pub(crate) connection_id: i64,
    /// The system the bookmark names: where the hole goes from here.
    pub(crate) back_to: String,
    /// Its placement, which the bookmark is asked for by.
    pub(crate) back_to_node: i64,
    pub(crate) here: String,
}

/// What the app knows that planning a jump needs, fetched once per sign-in.
#[derive(Clone)]
pub(crate) struct JumpContext {
    pub(crate) catalog: Arc<SignatureCatalog>,
    pub(crate) grid: Grid,
    pub(crate) stargates: Arc<HashMap<i64, Vec<i64>>>,
    pub(crate) suggest_alias: bool,
}

/// What a jump means for the map.
#[derive(Debug)]
pub(crate) enum Plan {
    /// Not part of the chain: why, for the log.
    Skip(String),
    /// Already on the map, signature and all; only the way back is news.
    Known(LastJump, String),
    /// Mapped without asking, since no signature on the origin could have been it.
    Mapped(LastJump, String),
    Ask(Box<Ask>),
}

/// The system a jump to `to` landed in: the one on the map, else the one the server knows
/// by that name.
fn target_of(
    client: &impl MapApi,
    existing: Option<&PlacedSystem>,
    to: &str,
) -> anyhow::Result<Option<Target>> {
    Ok(match existing {
        Some(s) => Some(Target {
            solar_system_id: s.solar_system_id,
            name: s.name.clone(),
            class: s.wormhole_class_id,
            security: s.security_status,
        }),
        None => client.find_system(to)?.map(|hit| Target {
            solar_system_id: hit.id,
            name: hit.name,
            class: hit.wormhole_class_id,
            security: hit.security,
        }),
    })
}

/// The names of the ghosts behind `ghosts`, by connection.
fn ghost_aliases(ghosts: &HashMap<i64, i64>, snapshot: &MapSnapshot) -> HashMap<i64, String> {
    ghosts
        .iter()
        .filter_map(|(&connection, ghost)| {
            let alias = snapshot
                .ghosts
                .iter()
                .find(|g| g.id == *ghost)?
                .alias
                .clone()?;
            (!alias.trim().is_empty()).then_some((connection, alias))
        })
        .collect()
}

/// The website's jump prompt, minus the prompting: which system it was, whether it is part
/// of the chain at all, and if so what to ask.
pub(crate) fn plan_jump(
    client: &impl MapApi,
    context: &JumpContext,
    map_id: i64,
    character: &str,
    from: &str,
    to: &str,
) -> anyhow::Result<Plan> {
    let snapshot = client.snapshot(map_id)?;
    // Only a jump out of the chain extends it; flying around elsewhere maps nothing.
    let Some(origin) = snapshot.system_named(from).cloned() else {
        return Ok(Plan::Skip(format!("{from} is not on the map")));
    };
    let existing = snapshot.system_named(to).cloned();
    let Some(target) = target_of(client, existing.as_ref(), to)? else {
        return Ok(Plan::Skip(format!("No system called {to}")));
    };
    if context
        .stargates
        .get(&origin.solar_system_id)
        .is_some_and(|gates| gates.contains(&target.solar_system_id))
    {
        return Ok(Plan::Skip(format!("{from} to {to} was a stargate")));
    }
    // A trade hub the map keeps off stays off; placed by hand, it is somebody's choice.
    if existing.is_none() && client.ignored(map_id)?.contains(&target.solar_system_id) {
        return Ok(Plan::Skip(format!("{to} is on the map's ignore list")));
    }
    let connection = existing
        .as_ref()
        .and_then(|e| snapshot.connection_between(origin.id, e.id))
        .map(|c| c.id);
    if let Some(id) = connection
        && snapshot.signature_on(id).is_some()
        && let Some(jump) = landed_on(&snapshot, map_id, from, to)
    {
        let bookmark = way_back(client, &snapshot, &jump);
        return Ok(Plan::Known(jump, bookmark));
    }

    let types: HashMap<i64, &SignatureType> =
        context.catalog.types.iter().map(|t| (t.id, t)).collect();
    let in_origin: Vec<Signature> = snapshot
        .signatures
        .iter()
        .filter(|s| s.solar_system_id == origin.solar_system_id)
        .cloned()
        .collect();
    let ghosts = jump::ghosts_from(&origin, &snapshot);
    let unflown = ghosts.keys().copied().collect();
    let candidates = jump::candidates(&in_origin, &types, target.class, &unflown);
    let ghost_aliases = ghost_aliases(&ghosts, &snapshot);
    // A system already on the map keeps the name the chain knows it by.
    let alias = match existing.as_ref().and_then(|e| e.alias.clone()) {
        Some(alias) if !alias.trim().is_empty() => Some(alias),
        // A name is a nicety: the jump still goes on the map without one.
        _ if context.suggest_alias => client
            .suggest_alias(map_id, origin.id, target.solar_system_id)
            .unwrap_or(None),
        _ => None,
    };
    let at = jump::free_position(
        &jump::positions(&snapshot.systems, &snapshot.ghosts),
        (origin.position_x, origin.position_y),
        context.grid,
    );
    let ask = Ask {
        character: character.to_string(),
        map_id,
        origin,
        target,
        options: candidates.numbered().cloned().collect(),
        likely: candidates.likely.len(),
        ghosts,
        ghost_aliases,
        alias,
        at,
    };
    if !ask.options.is_empty() {
        return Ok(Plan::Ask(Box::new(ask)));
    }
    // Nothing on the origin could have been it, so there is nothing to ask.
    if connection.is_some() {
        return Ok(match landed_on(&snapshot, map_id, from, to) {
            Some(jump) => {
                let bookmark = way_back(client, &snapshot, &jump);
                Plan::Known(jump, bookmark)
            }
            None => Plan::Skip(format!("{from} to {to} is already on the map")),
        });
    }
    Ok(
        match submit(client, &context.catalog, &ask, None, LoneGhost::Take)? {
            Some((jump, bookmark)) => Plan::Mapped(jump, bookmark),
            None => Plan::Skip(format!("Mapped {to}, but it is not on the map now")),
        },
    )
}

/// What an unscanned jump off a system with exactly one unflown hole drawn went through.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LoneGhost {
    /// That hole.
    Take,
    /// A hole of its own.
    Ignore,
}

/// Map the jump `ask` is about as going through `picked`, or a hole nobody scanned, which
/// `lone_ghost` says what to make of. One command either way, so a wrong pick is one undo.
pub(crate) fn submit(
    client: &impl MapApi,
    catalog: &SignatureCatalog,
    ask: &Ask,
    picked: Option<&Signature>,
    lone_ghost: LoneGhost,
) -> anyhow::Result<Option<(LastJump, String)>> {
    let ty = picked
        .and_then(|s| s.signature_type_id)
        .and_then(|id| catalog.types.iter().find(|t| t.id == id));
    let size = ty
        .and_then(|t| jump::size_for_jump_mass(t.max_jump_mass))
        .or(picked.and_then(|s| s.size));
    let mass_status = picked.and_then(|s| s.mass_status);
    let time_status = picked.and_then(|s| s.time_status);
    let ghost = match picked {
        Some(sig) => sig.connection_id.and_then(|c| ask.ghosts.get(&c).copied()),
        None if lone_ghost == LoneGhost::Take && ask.ghosts.len() == 1 => {
            ask.ghosts.values().next().copied()
        }
        None => None,
    };
    // A hole scanned and named before anyone flew it keeps that name.
    let alias = picked
        .and_then(|s| s.connection_id)
        .and_then(|c| ask.ghost_aliases.get(&c).cloned())
        .or_else(|| ask.alias.clone());

    if let Some(ghost) = ghost {
        client.resolve_ghost(&ResolveGhost {
            map_id: ask.map_id,
            map_solar_system_id: ghost,
            solar_system_id: ask.target.solar_system_id,
            alias,
            size,
            mass_status,
            time_status,
        })?;
    } else {
        client.track_jump(&TrackJump {
            map_id: ask.map_id,
            from_map_solar_system_id: ask.origin.id,
            to_solar_system_id: ask.target.solar_system_id,
            x: ask.at.0,
            y: ask.at.1,
            signature_pk: picked.map(|s| s.id),
            alias,
            size,
            mass_status,
            time_status,
        })?;
    }
    let snapshot = client.snapshot(ask.map_id)?;
    let Some(jump) = landed_on(&snapshot, ask.map_id, &ask.origin.name, &ask.target.name) else {
        return Ok(None);
    };
    let bookmark = way_back(client, &snapshot, &jump);
    Ok(Some((jump, bookmark)))
}

/// The hole between two systems on the map, flown from `from`.
pub(crate) fn landed_on(
    snapshot: &MapSnapshot,
    map_id: i64,
    from: &str,
    to: &str,
) -> Option<LastJump> {
    let origin = snapshot.system_named(from)?;
    let here = snapshot.system_named(to)?;
    let connection = snapshot.connection_between(origin.id, here.id)?;
    Some(LastJump {
        map_id,
        connection_id: connection.id,
        back_to: origin.name.clone(),
        back_to_node: origin.id,
        here: here.name.clone(),
    })
}

/// The bookmark for the way back, blank when it cannot be had: the jump is on the map
/// either way, and a missing name must not make it look as if it were not.
fn way_back(client: &impl MapApi, snapshot: &MapSnapshot, jump: &LastJump) -> String {
    bookmark_for(client, snapshot, jump)
        .ok()
        .flatten()
        .unwrap_or_default()
}

/// The bookmark for the way back across `jump`'s connection, as the map names it now.
/// `None` once the connection is gone from `snapshot`.
pub(crate) fn bookmark_for(
    client: &impl MapApi,
    snapshot: &MapSnapshot,
    jump: &LastJump,
) -> anyhow::Result<Option<String>> {
    if snapshot.connection(jump.connection_id).is_none() {
        return Ok(None);
    }
    Ok(client
        .connection_bookmarks(jump.map_id, jump.connection_id)?
        .into_iter()
        .find(|end| end.node == jump.back_to_node)
        .map(|end| end.text))
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use super::*;
    use crate::api::{Connection, ConnectionKind, Ghost, MapInfo, PlacedSystem};

    const MAP: i64 = 1;
    const AMARR: i64 = 30002187;
    const J100001: i64 = 31000001;

    fn system(id: i64, solar_system_id: i64, name: &str, class: i64, region: &str) -> PlacedSystem {
        PlacedSystem {
            id,
            solar_system_id,
            name: name.into(),
            alias: None,
            region: region.into(),
            wormhole_class_id: Some(class),
            security_status: if class == 7 { 0.9 } else { -1.0 },
            position_x: 0.0,
            position_y: 0.0,
            statics: Vec::new(),
        }
    }

    fn j100001(id: i64) -> PlacedSystem {
        system(id, J100001, "J100001", 3, "B-R00003")
    }

    fn connection(id: i64, from_system: i64, to_system: i64) -> Connection {
        Connection {
            id,
            from_system,
            to_system,
            kind: ConnectionKind::Wormhole,
            mass_status: None,
            time_status: None,
            preserve_mass: false,
            created_at: None,
        }
    }

    fn signature(id: i64, signature_id: &str, solar_system_id: i64, group: &str) -> Signature {
        Signature {
            id,
            signature_id: signature_id.into(),
            solar_system_id,
            group: group.into(),
            ..Signature::default()
        }
    }

    /// Amarr on the map, nothing else.
    fn snapshot() -> MapSnapshot {
        MapSnapshot {
            map: MapInfo {
                name: "Chain".into(),
            },
            systems: vec![system(10, AMARR, "Amarr", 7, "Domain")],
            ghosts: Vec::new(),
            connections: Vec::new(),
            signatures: Vec::new(),
        }
    }

    fn context() -> JumpContext {
        JumpContext {
            catalog: Arc::default(),
            grid: Grid::default(),
            stargates: Arc::default(),
            suggest_alias: false,
        }
    }

    /// A map that knows one system, J100001, and places it the way the server would.
    struct FakeMap {
        snapshot: RefCell<MapSnapshot>,
        ignored: Vec<i64>,
        tracked: RefCell<Vec<TrackJump>>,
        resolved: RefCell<Vec<ResolveGhost>>,
    }

    impl FakeMap {
        fn new(snapshot: MapSnapshot) -> Self {
            FakeMap {
                snapshot: RefCell::new(snapshot),
                ignored: Vec::new(),
                tracked: RefCell::default(),
                resolved: RefCell::default(),
            }
        }
    }

    impl MapApi for FakeMap {
        fn snapshot(&self, _: i64) -> anyhow::Result<MapSnapshot> {
            Ok(self.snapshot.borrow().clone())
        }

        fn find_system(&self, name: &str) -> anyhow::Result<Option<SystemHit>> {
            Ok(name.eq_ignore_ascii_case("J100001").then(|| SystemHit {
                id: J100001,
                name: "J100001".into(),
                security: -1.0,
                wormhole_class_id: Some(3),
            }))
        }

        fn ignored(&self, _: i64) -> anyhow::Result<Vec<i64>> {
            Ok(self.ignored.clone())
        }

        fn track_jump(&self, cmd: &TrackJump) -> anyhow::Result<()> {
            let mut snapshot = self.snapshot.borrow_mut();
            snapshot.systems.push(j100001(20));
            snapshot
                .connections
                .push(connection(50, cmd.from_map_solar_system_id, 20));
            self.tracked.borrow_mut().push(cmd.clone());
            Ok(())
        }

        fn resolve_ghost(&self, cmd: &ResolveGhost) -> anyhow::Result<()> {
            let mut snapshot = self.snapshot.borrow_mut();
            snapshot.ghosts.retain(|g| g.id != cmd.map_solar_system_id);
            snapshot.systems.push(j100001(cmd.map_solar_system_id));
            self.resolved.borrow_mut().push(cmd.clone());
            Ok(())
        }

        fn suggest_alias(&self, _: i64, origin: i64, _: i64) -> anyhow::Result<Option<String>> {
            Ok(Some(format!("after {origin}")))
        }

        /// Names each end after the system it leads to, which is all the planning needs:
        /// what the name says is the server's to decide.
        fn connection_bookmarks(
            &self,
            _: i64,
            connection_id: i64,
        ) -> anyhow::Result<Vec<EndBookmark>> {
            let snapshot = self.snapshot.borrow();
            let Some(c) = snapshot.connection(connection_id) else {
                anyhow::bail!("not found");
            };
            Ok([c.from_system, c.to_system]
                .into_iter()
                .filter_map(|node| {
                    let end = snapshot.systems.iter().find(|s| s.id == node)?;
                    Some(EndBookmark {
                        node,
                        text: format!("to {}", end.name),
                    })
                })
                .collect())
        }
    }

    fn plan(map: &FakeMap, from: &str, to: &str) -> Plan {
        plan_jump(map, &context(), MAP, "Pilot", from, to).unwrap()
    }

    #[test]
    fn a_jump_off_the_map_maps_nothing() {
        let map = FakeMap::new(snapshot());
        assert!(matches!(plan(&map, "Jita", "J100001"), Plan::Skip(_)));
        assert!(matches!(plan(&map, "Amarr", "Nowhere"), Plan::Skip(_)));
        assert!(map.tracked.borrow().is_empty());
    }

    #[test]
    fn stargates_and_ignored_systems_are_skipped() {
        let mut map = FakeMap::new(snapshot());
        map.ignored = vec![J100001];
        let plan = plan(&map, "Amarr", "J100001");
        assert!(matches!(plan, Plan::Skip(why) if why.contains("ignore")));

        let map = FakeMap::new(snapshot());
        let context = JumpContext {
            stargates: Arc::new(HashMap::from([(AMARR, vec![J100001])])),
            ..context()
        };
        let plan = plan_jump(&map, &context, MAP, "Pilot", "Amarr", "J100001").unwrap();
        assert!(matches!(plan, Plan::Skip(why) if why.contains("stargate")));
        assert!(map.tracked.borrow().is_empty());
    }

    #[test]
    fn an_unmapped_target_with_nothing_to_ask_is_mapped() {
        let map = FakeMap::new(snapshot());
        let Plan::Mapped(jump, bookmark) = plan(&map, "Amarr", "J100001") else {
            panic!("not mapped");
        };
        let tracked = map.tracked.borrow();
        assert_eq!(tracked.len(), 1);
        assert_eq!(tracked[0].from_map_solar_system_id, 10);
        assert_eq!(tracked[0].to_solar_system_id, J100001);
        assert_eq!(tracked[0].signature_pk, None);
        assert_eq!((jump.map_id, jump.connection_id), (MAP, 50));
        assert_eq!(
            (jump.back_to.as_str(), jump.here.as_str()),
            ("Amarr", "J100001")
        );
        // The bookmark for the way back, to Amarr.
        assert_eq!(bookmark, "to Amarr");
    }

    #[test]
    fn an_unmapped_target_asks_which_signature_it_was() {
        let mut snapshot = snapshot();
        snapshot.signatures = vec![
            signature(1, "ABC-123", AMARR, "wormhole"),
            signature(2, "DEF-456", AMARR, "data"),
            signature(3, "GHI-789", 30000142, "wormhole"),
        ];
        let map = FakeMap::new(snapshot);
        let Plan::Ask(ask) = plan(&map, "Amarr", "J100001") else {
            panic!("not asked");
        };
        let options: Vec<&str> = ask
            .options
            .iter()
            .map(|s| s.signature_id.as_str())
            .collect();
        assert_eq!(options, ["ABC-123"]);
        assert_eq!(ask.origin.id, 10);
        // Nobody asked for a name.
        assert_eq!(ask.alias, None);
        assert_eq!(ask.target.solar_system_id, J100001);
        assert!(map.tracked.borrow().is_empty());

        // The pick goes on the jump.
        let picked = ask.options[0].clone();
        let catalog = SignatureCatalog::default();
        let landed = submit(&map, &catalog, &ask, Some(&picked), LoneGhost::Ignore).unwrap();
        assert_eq!(map.tracked.borrow()[0].signature_pk, Some(1));
        assert_eq!(landed.map(|(jump, _)| jump.connection_id), Some(50));
    }

    #[test]
    fn the_name_for_a_new_system_comes_from_the_map() {
        let mut snapshot = snapshot();
        snapshot.signatures = vec![signature(1, "ABC-123", AMARR, "wormhole")];
        let map = FakeMap::new(snapshot);
        let context = JumpContext {
            suggest_alias: true,
            ..context()
        };
        let Ok(Plan::Ask(ask)) = plan_jump(&map, &context, MAP, "Pilot", "Amarr", "J100001") else {
            panic!("not asked");
        };
        assert_eq!(ask.alias.as_deref(), Some("after 10"));
    }

    #[test]
    fn an_already_mapped_jump_is_known() {
        let mut snapshot = snapshot();
        snapshot.systems.push(j100001(20));
        snapshot.connections.push(connection(50, 10, 20));
        let mut sig = signature(1, "XYZ-789", J100001, "wormhole");
        sig.connection_id = Some(50);
        snapshot.signatures.push(sig);
        let map = FakeMap::new(snapshot);
        let Plan::Known(jump, bookmark) = plan(&map, "Amarr", "J100001") else {
            panic!("not known");
        };
        assert_eq!(jump.connection_id, 50);
        assert_eq!(bookmark, "to Amarr");
        assert!(map.tracked.borrow().is_empty());
    }

    #[test]
    fn a_lone_unflown_hole_is_taken_to_be_the_jump() {
        let mut snapshot = snapshot();
        snapshot.ghosts.push(Ghost {
            id: 7,
            alias: Some("B1".into()),
            position_x: 0.0,
            position_y: 0.0,
        });
        snapshot.connections.push(connection(60, 10, 7));
        let map = FakeMap::new(snapshot);
        let Plan::Mapped(jump, _) = plan(&map, "Amarr", "J100001") else {
            panic!("not mapped");
        };
        let resolved = map.resolved.borrow();
        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].map_solar_system_id, 7);
        assert_eq!(resolved[0].solar_system_id, J100001);
        assert!(map.tracked.borrow().is_empty());
        assert_eq!(jump.connection_id, 60);
    }

    #[test]
    fn the_bookmark_follows_the_map_as_it_is_now() {
        let mut snapshot = snapshot();
        snapshot.systems.push(j100001(20));
        snapshot.connections.push(connection(50, 10, 20));
        let jump = LastJump {
            map_id: MAP,
            connection_id: 50,
            back_to: "Amarr".into(),
            back_to_node: 10,
            here: "J100001".into(),
        };
        let map = FakeMap::new(snapshot.clone());
        assert_eq!(
            bookmark_for(&map, &snapshot, &jump).unwrap().as_deref(),
            Some("to Amarr")
        );
        snapshot.connections.clear();
        assert_eq!(bookmark_for(&map, &snapshot, &jump).unwrap(), None);
    }
}
