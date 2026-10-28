//! How a map names things: the alias a newly reached system gets, and the in-game bookmark
//! name for a hole. Every client asks here rather than working it out itself, so a naming
//! convention is written once.
//!
//! A convention is a [`NamingStrategy`]. It is handed the whole map ([`NamingMap`]) and
//! decides on its own, so a new one needs nothing more than an implementation and a line in
//! [`strategy`]. [`chain::Chain`] is the convention every map had before there were others.

pub mod bookmark;
pub mod chain;
pub mod siblings;

use std::collections::hash_map::Entry;
use std::collections::{HashMap, VecDeque};
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};
use sqlx::PgPool;

pub use bookmark::{BookmarkSystem, Hole, render_bookmark};

use super::access::Reader;
use super::error::{MapError, Result};
use super::map::MapNaming;
use super::solar_system::SetAlias;
use super::{Actor, MassStatus, NamingStrategyKind, TimeStatus, WormholeSize};

/// A naming convention.
pub trait NamingStrategy: Sync {
    /// The alias for a system reached from `origin`, or `None` to leave it unnamed.
    fn alias(&self, map: &NamingMap, origin: &Node, target: &Target) -> Option<String>;

    /// The bookmark name for `destination`, made while standing in `near` at the other end of
    /// the hole. `near` is `None` when there is no such system to stand in.
    fn bookmark(
        &self,
        map: &NamingMap,
        destination: &Destination,
        hole: &Hole,
        near: Option<&Node>,
    ) -> String;

    /// The first few aliases it hands out on a fresh map, for the settings page to show.
    fn example_aliases(&self, settings: &MapNaming) -> Vec<String>;
}

/// The strategy a map names itself with.
pub fn strategy(settings: &MapNaming) -> &'static dyn NamingStrategy {
    match settings.strategy {
        NamingStrategyKind::Chain => &chain::Chain,
        NamingStrategyKind::Siblings => &siblings::Siblings,
    }
}

/// Everything a strategy may look at: the map's naming settings and its graph.
#[derive(Debug, Clone)]
pub struct NamingMap {
    pub settings: MapNaming,
    nodes: Vec<Node>,
    connections: Vec<Edge>,
    /// Node index by id, and each node's neighbours, built once.
    index: HashMap<i64, usize>,
    adjacent: HashMap<i64, Vec<i64>>,
    /// Worked out the first time a strategy asks.
    distances: OnceLock<HashMap<i64, u32>>,
}

impl NamingMap {
    pub fn new(settings: MapNaming, nodes: Vec<Node>, connections: Vec<Edge>) -> Self {
        let index = nodes.iter().enumerate().map(|(i, n)| (n.id, i)).collect();
        let mut adjacent: HashMap<i64, Vec<i64>> = HashMap::new();
        for c in &connections {
            adjacent.entry(c.from).or_default().push(c.to);
            adjacent.entry(c.to).or_default().push(c.from);
        }
        NamingMap {
            settings,
            nodes,
            connections,
            index,
            adjacent,
            distances: OnceLock::new(),
        }
    }

    pub fn nodes(&self) -> &[Node] {
        &self.nodes
    }

    pub fn node(&self, id: i64) -> Option<&Node> {
        self.index.get(&id).map(|&i| &self.nodes[i])
    }

    pub fn edge(&self, id: i64) -> Option<&Edge> {
        self.connections.iter().find(|c| c.id == id)
    }

    /// The node a solar system is placed as, if it is on the map.
    pub fn node_of_system(&self, solar_system_id: i64) -> Option<&Node> {
        self.nodes.iter().find(|n| {
            n.system
                .as_ref()
                .is_some_and(|s| s.solar_system_id == solar_system_id)
        })
    }

    /// Every alias on the map, ghosts' included.
    pub fn aliases(&self) -> Vec<String> {
        self.nodes.iter().filter_map(|n| n.alias.clone()).collect()
    }

    /// How many connections each node is from home: the map's home system, or a node that
    /// carries the ignored alias. Nodes that cannot reach home are left out, and so is
    /// everything on a map without one.
    pub fn distances_from_home(&self) -> &HashMap<i64, u32> {
        self.distances.get_or_init(|| {
            let mut distance = HashMap::new();
            let mut queue = VecDeque::new();
            for node in &self.nodes {
                if node.is_home
                    || chain::is_ignored_alias(node.alias.as_deref(), &self.settings.ignored_alias)
                {
                    distance.insert(node.id, 0);
                    queue.push_back(node.id);
                }
            }
            while let Some(id) = queue.pop_front() {
                let next = distance[&id] + 1;
                for &neighbour in self.adjacent.get(&id).into_iter().flatten() {
                    if let Entry::Vacant(slot) = distance.entry(neighbour) {
                        slot.insert(next);
                        queue.push_back(neighbour);
                    }
                }
            }
            distance
        })
    }

    /// The nodes one connection away from `id`.
    pub fn neighbours(&self, id: i64) -> impl Iterator<Item = &Node> {
        self.adjacent
            .get(&id)
            .into_iter()
            .flatten()
            .filter_map(|&other| self.node(other))
    }
}

/// A node on the map: a placed system, or a ghost (the far side of a hole nobody has been
/// through yet), which has no system.
#[derive(Debug, Clone)]
pub struct Node {
    pub id: i64,
    pub alias: Option<String>,
    pub is_home: bool,
    pub system: Option<NodeSystem>,
}

#[derive(Debug, Clone)]
pub struct NodeSystem {
    pub solar_system_id: i64,
    pub name: String,
    pub region: String,
    pub class: Option<i32>,
    pub security: f64,
    pub occupier: Option<String>,
}

impl Node {
    /// The node as a bookmark describes it. A ghost is only its alias.
    pub fn bookmark_system(&self) -> BookmarkSystem {
        match &self.system {
            Some(s) => BookmarkSystem {
                alias: self.alias.clone(),
                name: s.name.clone(),
                region: Some(s.region.clone()),
                wormhole_class_id: s.class,
                security: Some(s.security),
                occupier: s.occupier.clone(),
                is_home: self.is_home,
            },
            None => BookmarkSystem {
                alias: self.alias.clone(),
                ..BookmarkSystem::default()
            },
        }
    }
}

#[derive(Debug, Clone)]
pub struct Edge {
    pub id: i64,
    pub from: i64,
    pub to: i64,
    pub size: Option<WormholeSize>,
    pub mass_status: Option<MassStatus>,
    pub time_status: Option<TimeStatus>,
}

/// What is known of a system about to be named: its class, or failing that its security.
/// Both `None` is a hole whose far side nobody knows.
#[derive(Debug, Clone, Copy, Default)]
pub struct Target {
    pub class: Option<i32>,
    pub security: Option<f64>,
}

/// The system a bookmark names, and its node when it is on the map.
#[derive(Debug, Clone)]
pub struct Destination {
    pub node: Option<i64>,
    pub system: BookmarkSystem,
}

/// What a strategy needs of the map and nothing more: the full map view also carries
/// statics, sovereignty and jump statistics, none of which names anything. `_reader` is the
/// proof that whoever asked may see the map.
async fn load(pool: &PgPool, _reader: Reader, map_id: i64) -> Result<NamingMap> {
    let settings = sqlx::query!(
        r#"select alias_scheme, naming_strategy, ignored_alias, bookmark_wormhole,
                  bookmark_kspace, bookmark_return
           from maps where id = $1"#,
        map_id,
    )
    .fetch_optional(pool)
    .await?
    .ok_or(MapError::NotFound)?;
    let settings = MapNaming {
        strategy: settings.naming_strategy,
        alias_scheme: settings.alias_scheme,
        ignored_alias: settings.ignored_alias,
        bookmark_wormhole: settings.bookmark_wormhole,
        bookmark_kspace: settings.bookmark_kspace,
        bookmark_return: settings.bookmark_return,
    };

    let nodes = sqlx::query!(
        r#"select mss.id, mss.alias, mss.is_home, mss.solar_system_id,
                  ss.name as "name?", ss.security_status as "security?", ss.wormhole_class_id,
                  r.name as "region?", d.occupying_group
           from map_solar_systems mss
           -- Left joins: a ghost has no system to join to.
           left join solar_systems ss on ss.id = mss.solar_system_id
           left join regions r on r.id = ss.region_id
           left join map_solar_system_details d
               on d.map_id = mss.map_id and d.solar_system_id = mss.solar_system_id
           where mss.map_id = $1
           order by mss.id"#,
        map_id,
    )
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(|row| Node {
        id: row.id,
        alias: row.alias,
        is_home: row.is_home,
        system: row.solar_system_id.map(|solar_system_id| NodeSystem {
            solar_system_id,
            name: row.name.unwrap_or_default(),
            region: row.region.unwrap_or_default(),
            class: row.wormhole_class_id,
            security: row.security.unwrap_or_default(),
            occupier: row.occupying_group,
        }),
    })
    .collect();

    let connections = sqlx::query!(
        r#"select id, from_system, to_system, size, mass_status, time_status
           from map_connections where map_id = $1"#,
        map_id,
    )
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(|row| Edge {
        id: row.id,
        from: row.from_system,
        to: row.to_system,
        size: row.size,
        mass_status: row.mass_status,
        time_status: row.time_status,
    })
    .collect();

    Ok(NamingMap::new(settings, nodes, connections))
}

/// A solar system that may not be on the map, as a bookmark describes it.
async fn solar_system(pool: &PgPool, id: i64) -> Result<BookmarkSystem> {
    let row = sqlx::query!(
        r#"select ss.name, ss.security_status, ss.wormhole_class_id, r.name as region
           from solar_systems ss join regions r on r.id = ss.region_id
           where ss.id = $1"#,
        id,
    )
    .fetch_optional(pool)
    .await?
    .ok_or(MapError::NotFound)?;
    Ok(BookmarkSystem {
        alias: None,
        name: row.name,
        region: Some(row.region),
        wormhole_class_id: row.wormhole_class_id,
        security: Some(row.security_status),
        occupier: None,
        is_home: false,
    })
}

/// A signature type's wormhole code and the class it leads to.
async fn signature_type(pool: &PgPool, id: Option<i64>) -> Result<(Option<String>, Option<i32>)> {
    let Some(id) = id else {
        return Ok((None, None));
    };
    let row = sqlx::query!(
        "select signature, target_class from signature_types where id = $1",
        id,
    )
    .fetch_optional(pool)
    .await?;
    Ok(row.map_or((None, None), |r| (r.signature, r.target_class)))
}

/// Ask for the alias of a system reached from `origin`.
#[derive(Debug, Clone, Deserialize, ts_rs::TS, utoipa::ToSchema, utoipa::IntoParams)]
#[ts(export)]
#[into_params(parameter_in = Query)]
pub struct SuggestAlias {
    /// The placement the system is reached from.
    pub origin: i64,
    /// The solar system reached, when it is known.
    #[serde(default)]
    #[ts(optional)]
    pub target_system: Option<i64>,
    /// The class it leads to, when only that is known (a typed hole not yet flown).
    #[serde(default)]
    #[ts(optional)]
    pub target_class: Option<i32>,
}

#[derive(Debug, Clone, Serialize, ts_rs::TS, utoipa::ToSchema)]
#[ts(export)]
pub struct AliasSuggestion {
    pub alias: Option<String>,
}

/// The alias the map's strategy gives a system reached from `origin`.
pub async fn suggest_alias(
    pool: &PgPool,
    reader: Reader,
    map_id: i64,
    cmd: SuggestAlias,
) -> Result<AliasSuggestion> {
    let map = load(pool, reader, map_id).await?;
    let origin = map.node(cmd.origin).ok_or(MapError::NotFound)?;
    let target = match cmd.target_system {
        Some(id) => {
            let system = solar_system(pool, id).await?;
            Target {
                class: system.wormhole_class_id,
                security: system.security,
            }
        }
        None => Target {
            class: cmd.target_class,
            security: None,
        },
    };
    Ok(AliasSuggestion {
        alias: strategy(&map.settings).alias(&map, origin, &target),
    })
}

/// Ask for the bookmark name of a hole, made while standing in `near`.
#[derive(Debug, Clone, Deserialize, ts_rs::TS, utoipa::ToSchema)]
#[ts(export)]
pub struct NameBookmark {
    /// The placement the bookmark is made in.
    pub near: i64,
    /// The node the hole leads to, when it is on the map (a ghost counts).
    #[serde(default)]
    #[ts(optional)]
    pub destination: Option<i64>,
    /// The solar system it leads to, when that is known but not on the map.
    #[serde(default)]
    #[ts(optional)]
    pub destination_system: Option<i64>,
    /// The alias to name it by. Absent takes the destination's own, and failing that the
    /// strategy's suggestion when `suggest_alias` is set; `null` names it without one.
    #[serde(default, deserialize_with = "super::double_option")]
    #[ts(optional, type = "string | null")]
    pub alias: Option<Option<String>>,
    #[serde(default)]
    #[ts(as = "Option<bool>", optional)]
    pub suggest_alias: bool,
    /// Write a suggested alias onto the destination node in the same step, so a quick
    /// second bookmark sees it taken and gets the next one. Needs a signed-in member; for
    /// anyone else the suggestion is only named, not written.
    #[serde(default)]
    #[ts(as = "Option<bool>", optional)]
    pub claim_alias: bool,
    #[serde(default)]
    #[ts(optional)]
    pub signature_id: Option<String>,
    /// The signature's type: the wormhole code, and the class it leads to while the far
    /// side is unknown.
    #[serde(default)]
    #[ts(optional)]
    pub signature_type_id: Option<i64>,
    #[serde(default)]
    #[ts(optional)]
    pub size: Option<WormholeSize>,
    #[serde(default)]
    #[ts(optional)]
    pub mass_status: Option<MassStatus>,
    #[serde(default)]
    #[ts(optional)]
    pub time_status: Option<TimeStatus>,
}

#[derive(Debug, Clone, Serialize, ts_rs::TS, utoipa::ToSchema)]
#[ts(export)]
pub struct BookmarkName {
    pub text: String,
    /// The alias the strategy suggested for a destination that had none.
    pub suggested_alias: Option<String>,
}

/// Claims run one at a time, so two copies a moment apart cannot both read the map before
/// either has written its alias and hand out the same name. Naming a hole is rare enough
/// that one lock for the whole server costs nothing.
static CLAIMS: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// The bookmark name the map's strategy gives a hole. With `claim_alias`, a suggested alias
/// is written onto the destination node as `claimer`, when there is one.
pub async fn name_bookmark(
    pool: &PgPool,
    reader: Reader,
    map_id: i64,
    cmd: NameBookmark,
    claimer: Option<Actor>,
) -> Result<BookmarkName> {
    let _claiming = if cmd.claim_alias {
        Some(CLAIMS.lock().await)
    } else {
        None
    };
    let map = load(pool, reader, map_id).await?;
    let strategy = strategy(&map.settings);
    let near = map.node(cmd.near).ok_or(MapError::NotFound)?;
    let (wormhole_code, type_class) = signature_type(pool, cmd.signature_type_id).await?;

    let node = match (cmd.destination, cmd.destination_system) {
        (Some(id), _) => Some(map.node(id).ok_or(MapError::NotFound)?),
        (None, Some(system)) => map.node_of_system(system),
        (None, None) => None,
    };
    let mut system = match (node, cmd.destination_system) {
        (Some(node), _) if node.system.is_some() => node.bookmark_system(),
        (_, Some(id)) => solar_system(pool, id).await?,
        // A ghost, or no node at all: until the hole is flown, its type's promised class
        // is all there is, and the name stays blank rather than borrowing the near side's.
        _ => BookmarkSystem {
            wormhole_class_id: type_class,
            ..BookmarkSystem::default()
        },
    };

    let own = node
        .and_then(|n| n.alias.clone())
        .filter(|a| !a.trim().is_empty());
    let mut suggested_alias = None;
    system.alias = match cmd.alias {
        Some(given) => given.filter(|a| !a.trim().is_empty()),
        None => own.or_else(|| {
            if !cmd.suggest_alias {
                return None;
            }
            let target = Target {
                class: system.wormhole_class_id,
                security: system.security,
            };
            suggested_alias = strategy.alias(&map, near, &target);
            suggested_alias.clone()
        }),
    };

    let hole = Hole {
        signature_id: cmd.signature_id,
        size: cmd.size,
        mass_status: cmd.mass_status,
        time_status: cmd.time_status,
        wormhole_code,
    };
    let destination = Destination {
        node: node.map(|n| n.id),
        system,
    };
    let text = strategy.bookmark(&map, &destination, &hole, Some(near));
    if let (true, Some(actor), Some(alias), Some(node)) =
        (cmd.claim_alias, claimer, &suggested_alias, destination.node)
    {
        let written = super::run(
            pool,
            actor,
            SetAlias {
                map_id,
                map_solar_system_id: node,
                alias: Some(alias.clone()),
            },
        )
        .await;
        match written {
            // A viewer can still copy the name; it just is not written.
            Ok(()) | Err(MapError::Forbidden) => {}
            Err(err) => return Err(err),
        }
    }
    Ok(BookmarkName {
        text,
        suggested_alias,
    })
}

/// The bookmark for one end of a connection, as it would be written in game.
#[derive(Debug, Clone, Serialize, ts_rs::TS, utoipa::ToSchema)]
#[ts(export)]
pub struct EndBookmark {
    /// The end the bookmark leads to.
    pub node: i64,
    pub text: String,
}

/// The bookmark for each end of a connection. A bookmark is made while standing at the
/// hole, so the name for one end carries the signature read on the other: the far side is
/// what the name has to describe, the near side is where the scanner was. A ghost end has
/// no system to name, so it gets none.
pub async fn connection_bookmarks(
    pool: &PgPool,
    reader: Reader,
    map_id: i64,
    connection_id: i64,
) -> Result<Vec<EndBookmark>> {
    let map = load(pool, reader, map_id).await?;
    let strategy = strategy(&map.settings);
    let edge = map.edge(connection_id).ok_or(MapError::NotFound)?;
    let signatures = sqlx::query!(
        r#"select s.solar_system_id, s.signature_id, st.signature as "code?"
           from signatures s
           left join signature_types st on st.id = s.signature_type_id
           where s.map_id = $1 and s.connection_id = $2
           order by s.id"#,
        map_id,
        connection_id,
    )
    .fetch_all(pool)
    .await?;

    let (Some(from), Some(to)) = (map.node(edge.from), map.node(edge.to)) else {
        return Ok(Vec::new());
    };
    let name = |end: &Node, near: &Node| -> Option<EndBookmark> {
        end.system.as_ref()?;
        let sig = near.system.as_ref().and_then(|n| {
            signatures
                .iter()
                .find(|s| s.solar_system_id == n.solar_system_id)
        });
        let hole = Hole {
            signature_id: sig.map(|s| s.signature_id.clone()),
            size: edge.size,
            mass_status: edge.mass_status,
            time_status: edge.time_status,
            wormhole_code: sig.and_then(|s| s.code.clone()),
        };
        let destination = Destination {
            node: Some(end.id),
            system: end.bookmark_system(),
        };
        Some(EndBookmark {
            node: end.id,
            text: strategy.bookmark(&map, &destination, &hole, Some(near)),
        })
    };
    Ok([name(from, to), name(to, from)]
        .into_iter()
        .flatten()
        .collect())
}

/// What the settings page shows while naming is being edited: the first aliases the strategy
/// hands out, and each format rendered against a worked example.
#[derive(Debug, Clone, Serialize, ts_rs::TS, utoipa::ToSchema)]
#[ts(export)]
pub struct NamingPreview {
    /// The first few aliases the map's strategy hands out, so switching strategy or
    /// scheme shows the difference.
    pub aliases: Vec<String>,
    pub wormhole: String,
    pub kspace: String,
    #[serde(rename = "return")]
    #[ts(rename = "return")]
    pub ret: String,
    /// Every token a format may use.
    pub tokens: Vec<String>,
    /// What each blank setting falls back to, for the page's placeholders.
    pub defaults: NamingDefaults,
}

#[derive(Debug, Clone, Serialize, ts_rs::TS, utoipa::ToSchema)]
#[ts(export)]
pub struct NamingDefaults {
    pub wormhole: String,
    pub kspace: String,
    #[serde(rename = "return")]
    #[ts(rename = "return")]
    pub ret: String,
    pub ignored_alias: String,
}

pub fn preview(settings: &MapNaming) -> NamingPreview {
    let aliases = strategy(settings).example_aliases(settings);

    let example = BookmarkSystem {
        alias: Some("1a".into()),
        name: "J155207".into(),
        region: Some("E-R00028".into()),
        wormhole_class_id: Some(5),
        security: Some(-1.0),
        occupier: Some("Hard Knocks".into()),
        is_home: false,
    };
    let hole = Hole {
        signature_id: Some("ABC-123".into()),
        size: Some(WormholeSize::Medium),
        mass_status: Some(MassStatus::Critical),
        time_status: Some(TimeStatus::Eol),
        wormhole_code: Some("H296".into()),
    };
    let kspace = BookmarkSystem {
        alias: Some("1b".into()),
        name: "Jita".into(),
        region: Some("The Forge".into()),
        wormhole_class_id: Some(7),
        security: Some(0.95),
        occupier: None,
        is_home: false,
    };
    let kspace_hole = Hole {
        wormhole_code: Some("B041".into()),
        ..hole.clone()
    };
    // Back up the chain, so the two aliases swap.
    let back = BookmarkSystem {
        alias: Some("1".into()),
        ..example.clone()
    };
    let format = |f: &str, fallback: &str| if f.is_empty() { fallback } else { f }.to_string();
    NamingPreview {
        tokens: bookmark::BOOKMARK_TOKENS.map(String::from).to_vec(),
        defaults: NamingDefaults {
            wormhole: chain::DEFAULT_FORMAT_WORMHOLE.into(),
            kspace: chain::DEFAULT_FORMAT_KSPACE.into(),
            ret: chain::DEFAULT_FORMAT_RETURN.into(),
            ignored_alias: chain::DEFAULT_IGNORED_ALIAS.into(),
        },
        aliases,
        wormhole: render_bookmark(
            &format(&settings.bookmark_wormhole, chain::DEFAULT_FORMAT_WORMHOLE),
            &example,
            &hole,
            Some("1"),
        ),
        kspace: render_bookmark(
            &format(&settings.bookmark_kspace, chain::DEFAULT_FORMAT_KSPACE),
            &kspace,
            &kspace_hole,
            Some("1"),
        ),
        ret: render_bookmark(
            &format(&settings.bookmark_return, chain::DEFAULT_FORMAT_RETURN),
            &back,
            &hole,
            Some("1a"),
        ),
    }
}
