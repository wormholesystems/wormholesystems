//! What taking something off a map takes with it. Every removal goes through here, so the
//! cascade is worked out once and the same way whether a system, a connection or a scan
//! was asked for.
//!
//! The rules, applied until nothing new turns up:
//!
//! - a system takes its connections, its signatures, and the ghosts hanging off it;
//! - a connection takes every signature linked to it, on both sides: the hole is gone,
//!   and a scan left behind unlinked would only raise a ghost for it again;
//! - a signature takes the ghost it raised, and its connection once no signature on its
//!   side still names it;
//! - a ghost left without a connection goes, and so does a real system when the caller
//!   sweeps orphans ([`Orphans::Sweep`]). Home, pinned and rally nodes never do.
//!
//! Signatures are matched to a connection by its id, never by the systems it joins, so two
//! holes between the same pair of systems keep apart.
//!
//! Everything is snapshotted before anything is deleted, so one undo brings all of it back,
//! and the events come from what actually went rather than from a list kept by hand.

use std::collections::BTreeSet;

use super::command::{MapCommand, Tx};
use super::error::Result;
use super::restore::RestoreSystems;
use super::signatures::RestoreSignatures;
use super::store::{connections, jumps, placements, signatures};
use crate::realtime::MapEvent;

/// Whether a real system left without a connection goes with the removal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Orphans {
    /// It stays: the user asked for something else, and an edgeless system is still a
    /// place they put there.
    Keep,
    /// It goes, unless it holds one of the signatures being removed: that is where the
    /// user is working.
    Sweep,
}

/// What to remove, before the cascade.
pub(super) struct Removal {
    map_id: i64,
    systems: BTreeSet<i64>,
    connections: BTreeSet<i64>,
    signatures: BTreeSet<i64>,
    orphans: Orphans,
}

impl Removal {
    pub(super) fn new(map_id: i64) -> Self {
        Self {
            map_id,
            systems: BTreeSet::new(),
            connections: BTreeSet::new(),
            signatures: BTreeSet::new(),
            orphans: Orphans::Keep,
        }
    }

    /// Placements (`map_solar_systems.id`). Protection is the caller's call: whatever is
    /// named here goes.
    pub(super) fn systems(mut self, ids: &[i64]) -> Self {
        self.systems.extend(ids);
        self
    }

    pub(super) fn connections(mut self, ids: &[i64]) -> Self {
        self.connections.extend(ids);
        self
    }

    /// Signature rows (`signatures.id`).
    pub(super) fn signatures(mut self, pks: &[i64]) -> Self {
        self.signatures.extend(pks);
        self
    }

    pub(super) fn orphans(mut self, orphans: Orphans) -> Self {
        self.orphans = orphans;
        self
    }

    /// Work out the cascade, snapshot it, and delete it.
    pub(super) async fn apply(mut self, tx: &mut Tx<'_>) -> Result<Removed> {
        let map_id = self.map_id;
        // The systems the named scans are in: a sweep never takes those.
        let kept: Vec<i64> = sqlx::query_scalar!(
            r#"select distinct solar_system_id as "id!" from signatures
               where map_id = $1 and id = any($2)"#,
            map_id,
            &ids(&self.signatures),
        )
        .fetch_all(&mut **tx)
        .await?;

        while self.grow(tx, &kept).await? {}

        let systems = ids(&self.systems);
        let connection_ids = ids(&self.connections);
        let signature_ids = ids(&self.signatures);

        let connections = connections::snapshot_many(tx, map_id, &connection_ids).await?;
        // A removal that takes placements carries the far ends of its edges too, so the
        // snapshot stands on its own: an edge cannot go back before both of its ends. Those
        // still on the map are passed over by the restore.
        let mut placed = systems.clone();
        if !systems.is_empty() {
            placed.extend(
                connections
                    .iter()
                    .flat_map(|c| [c.from_system, c.to_system]),
            );
            placed.sort_unstable();
            placed.dedup();
        }
        let snapshot = RestoreSystems {
            map_id,
            systems: placements::snapshot(tx, map_id, &placed).await?,
            jumps: jumps::snapshot_through(tx, map_id, &connection_ids).await?,
            signatures: signatures::snapshot(tx, map_id, &signature_ids).await?,
            connections,
        };

        // Any order would do, the foreign keys only ever cascade into rows in this set; this
        // one deletes the most by hand and leaves the least to them.
        signatures::delete(tx, map_id, &signature_ids).await?;
        connections::delete(tx, map_id, &connection_ids).await?;
        placements::delete(tx, map_id, &systems).await?;

        let present: BTreeSet<i64> = snapshot.systems.iter().map(|s| s.id).collect();
        Ok(Removed {
            systems: systems
                .into_iter()
                .filter(|id| present.contains(id))
                .collect(),
            connections: snapshot.connections.iter().map(|c| c.id).collect(),
            signature_systems: {
                let mut systems: Vec<i64> = snapshot
                    .signatures
                    .iter()
                    .map(|s| s.solar_system_id)
                    .collect();
                systems.sort_unstable();
                systems.dedup();
                systems
            },
            snapshot,
        })
    }

    /// One round of the cascade. Returns whether it found anything new.
    async fn grow(&mut self, tx: &mut Tx<'_>, kept: &[i64]) -> Result<bool> {
        let map_id = self.map_id;
        let systems = ids(&self.systems);
        let connections = ids(&self.connections);
        let signatures = ids(&self.signatures);
        let before = self.systems.len() + self.connections.len() + self.signatures.len();

        // The edges of a system that goes, and the edges whose last scan on one side goes.
        let more_connections = sqlx::query_scalar!(
            r#"select id as "id!" from map_connections
               where map_id = $1 and (from_system = any($2) or to_system = any($2))
               union
               select s.connection_id as "id!" from signatures s
               where s.map_id = $1 and s.id = any($3) and s.connection_id is not null
                 and not exists (
                     select 1 from signatures o
                     where o.map_id = $1
                       and o.connection_id = s.connection_id
                       and o.solar_system_id = s.solar_system_id
                       and not (o.id = any($3))
                 )"#,
            map_id,
            &systems,
            &signatures,
        )
        .fetch_all(&mut **tx)
        .await?;

        // The scans in a system that goes, and both ends' scans of a hole that goes.
        let more_signatures = sqlx::query_scalar!(
            r#"select s.id as "id!" from signatures s
               join map_solar_systems p
                 on p.map_id = s.map_id and p.solar_system_id = s.solar_system_id
               where s.map_id = $1 and p.id = any($2)
               union
               select id from signatures where map_id = $1 and connection_id = any($3)"#,
            map_id,
            &systems,
            &connections,
        )
        .fetch_all(&mut **tx)
        .await?;

        // The nodes those scans raised and the ones hanging off a system that goes, which
        // the foreign keys would take anyway, and the ends left without an edge.
        let more_systems = sqlx::query_scalar!(
            r#"select id as "id!" from map_solar_systems
               where map_id = $1 and (raised_by_signature_id = any($2) or hangs_off_id = any($3))
               union
               select p.id from map_solar_systems p
               where p.map_id = $1
                 and not p.is_home and not p.is_pinned and not p.is_rally
                 and exists (
                     select 1 from map_connections c
                     where c.map_id = $1 and c.id = any($4)
                       and (c.from_system = p.id or c.to_system = p.id)
                 )
                 and not exists (
                     select 1 from map_connections c
                     where c.map_id = $1 and not (c.id = any($4))
                       and (c.from_system = p.id or c.to_system = p.id)
                 )
                 and (p.solar_system_id is null
                      or ($5 and not (p.solar_system_id = any($6))))"#,
            map_id,
            &signatures,
            &systems,
            &connections,
            self.orphans == Orphans::Sweep,
            kept,
        )
        .fetch_all(&mut **tx)
        .await?;

        self.connections.extend(more_connections);
        self.signatures.extend(more_signatures);
        self.systems.extend(more_systems);
        Ok(self.systems.len() + self.connections.len() + self.signatures.len() > before)
    }
}

fn ids(set: &BTreeSet<i64>) -> Vec<i64> {
    set.iter().copied().collect()
}

/// What a removal took.
pub(super) struct Removed {
    /// Placements that went.
    pub systems: Vec<i64>,
    pub connections: Vec<i64>,
    /// Solar systems whose signature list changed.
    pub signature_systems: Vec<i64>,
    pub snapshot: RestoreSystems,
}

impl Removed {
    /// What open clients should refetch.
    pub(super) fn events(&self) -> Vec<MapEvent> {
        let map_id = self.snapshot.map_id;
        let systems = self
            .systems
            .iter()
            .map(|&map_solar_system_id| MapEvent::SystemRemoved {
                map_id,
                map_solar_system_id,
            });
        let connections =
            self.connections
                .iter()
                .map(|&connection_id| MapEvent::ConnectionChanged {
                    map_id,
                    connection_id,
                });
        let signatures =
            self.signature_systems
                .iter()
                .map(|&solar_system_id| MapEvent::SignatureChanged {
                    map_id,
                    solar_system_id,
                });
        systems.chain(connections).chain(signatures).collect()
    }

    /// The command that puts it all back. Scans alone go back as scans, so the history
    /// says so; anything bigger as one snapshot.
    pub(super) fn undo(self) -> MapCommand {
        if self.systems.is_empty() && self.connections.is_empty() {
            return MapCommand::RestoreSignatures(RestoreSignatures {
                map_id: self.snapshot.map_id,
                signatures: self.snapshot.signatures,
                replaces_system: None,
                pasted_at: None,
            });
        }
        MapCommand::RestoreSystems(self.snapshot)
    }
}
