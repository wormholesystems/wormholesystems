use serde::Deserialize;

use super::{EsiClient, Result};

#[derive(Debug, Clone, Deserialize)]
pub struct Corporation {
    pub name: String,
    pub ticker: String,
    pub alliance_id: Option<i64>,
    pub member_count: i64,
    pub ceo_id: i64,
    pub creator_id: i64,
    pub tax_rate: f64,
    pub date_founded: Option<String>,
    pub faction_id: Option<i64>,
    pub home_station_id: Option<i64>,
    pub url: Option<String>,
    pub war_eligible: Option<bool>,
    pub description: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Alliance {
    pub name: String,
    pub ticker: String,
    pub creator_corporation_id: i64,
    pub creator_id: i64,
    pub date_founded: String,
    pub executor_corporation_id: Option<i64>,
    pub faction_id: Option<i64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Affiliation {
    pub character_id: i64,
    pub corporation_id: i64,
    pub alliance_id: Option<i64>,
    pub faction_id: Option<i64>,
}

/// A player-owned structure, as one character with docking access sees it.
#[derive(Debug, Clone, Deserialize)]
pub struct Structure {
    pub name: String,
    pub owner_id: i64,
    pub solar_system_id: i64,
    pub type_id: Option<i64>,
}

/// One id resolved by the bulk name endpoint. `category` says which kind it turned out to
/// be: `character`, `corporation`, `alliance`, `solar_system`, and so on.
#[derive(Debug, Clone, Deserialize)]
pub struct UniverseName {
    pub id: i64,
    pub name: String,
    pub category: String,
}

impl EsiClient {
    /// Names for up to 1000 ids of any kind, in one call.
    ///
    /// The per-entity endpoints return far more (a corporation's ticker, a character's
    /// corp), so this is only worth reaching for when a name is all that is needed and the
    /// list is long, as when importing a year of killmails.
    pub async fn universe_names(&self, ids: &[i64]) -> Result<Vec<UniverseName>> {
        self.post_json("/universe/names", &ids, None).await
    }

    pub async fn corporation(&self, corporation_id: i64) -> Result<Corporation> {
        self.get_json(&format!("/corporations/{corporation_id}"), None)
            .await
    }

    pub async fn alliance(&self, alliance_id: i64) -> Result<Alliance> {
        self.get_json(&format!("/alliances/{alliance_id}"), None)
            .await
    }

    /// Every public structure's id, without a token. Naming one still needs one.
    pub async fn public_structures(&self) -> Result<Vec<i64>> {
        self.get_json("/universe/structures", None).await
    }

    /// Needs `esi-universe.read_structures.v1`, and access to the structure: a 403 means
    /// this character is not on its docking list.
    pub async fn structure(&self, token: &str, structure_id: i64) -> Result<Structure> {
        self.get_json(&format!("/universe/structures/{structure_id}"), Some(token))
            .await
    }

    /// ESI accepts at most 1000 ids per call; callers batch larger sets.
    pub async fn affiliation(&self, character_ids: &[i64]) -> Result<Vec<Affiliation>> {
        self.post_json("/characters/affiliation", &character_ids, None)
            .await
    }
}
