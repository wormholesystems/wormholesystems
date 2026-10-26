use super::common::{LocalizedString, Position3D};
use serde::Deserialize;

/// `npcCorporations.jsonl`
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
#[expect(
    clippy::struct_excessive_bools,
    reason = "mirrors the SDE file, whose flags are independent booleans"
)]
pub struct NpcCorporation {
    #[serde(rename = "_key")]
    pub id: i64,
    #[serde(rename = "ceoID")]
    pub ceo_id: Option<i64>,
    pub deleted: bool,
    pub description: Option<LocalizedString>,
    pub extent: String,
    pub has_player_personnel_manager: bool,
    pub initial_price: i32,
    pub member_limit: i32,
    pub min_security: f64,
    pub minimum_join_standing: i32,
    pub name: LocalizedString,
    pub send_char_termination_message: bool,
    pub shares: i64,
    pub size: String,
    #[serde(rename = "stationID")]
    pub station_id: Option<i64>,
    pub tax_rate: f64,
    pub ticker_name: String,
    pub unique_name: bool,
    pub allowed_member_races: Option<Vec<i32>>,
    pub corporation_trades: Option<Vec<NpcCorporationCorporationTrade>>,
    pub divisions: Option<Vec<NpcCorporationDivisionEntry>>,
    #[serde(rename = "enemyID")]
    pub enemy_id: Option<i64>,
    #[serde(rename = "factionID")]
    pub faction_id: Option<i64>,
    #[serde(rename = "friendID")]
    pub friend_id: Option<i64>,
    #[serde(rename = "iconID")]
    pub icon_id: Option<i64>,
    pub investors: Option<Vec<NpcCorporationInvestor>>,
    pub lp_offer_tables: Option<Vec<i32>>,
    #[serde(rename = "mainActivityID")]
    pub main_activity_id: Option<i64>,
    #[serde(rename = "raceID")]
    pub race_id: Option<i64>,
    pub size_factor: Option<f64>,
    #[serde(rename = "solarSystemID")]
    pub solar_system_id: Option<i64>,
    #[serde(rename = "secondaryActivityID")]
    pub secondary_activity_id: Option<i64>,
    pub exchange_rates: Option<Vec<NpcCorporationExchangeRate>>,
}

/// Nested `corporationTrades` entry of `npcCorporations.jsonl`.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NpcCorporationCorporationTrade {
    #[serde(rename = "_key")]
    pub id: i64,
    #[serde(rename = "_value")]
    pub value: f64,
}

/// Nested `divisions` entry of `npcCorporations.jsonl`.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NpcCorporationDivisionEntry {
    #[serde(rename = "_key")]
    pub id: i64,
    pub division_number: i32,
    #[serde(rename = "leaderID")]
    pub leader_id: i64,
    pub size: i32,
}

/// Nested `investors` entry of `npcCorporations.jsonl`.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NpcCorporationInvestor {
    #[serde(rename = "_key")]
    pub id: i64,
    #[serde(rename = "_value")]
    pub value: i32,
}

/// Nested `exchangeRates` entry of `npcCorporations.jsonl`.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NpcCorporationExchangeRate {
    #[serde(rename = "_key")]
    pub id: i64,
    #[serde(rename = "_value")]
    pub value: f64,
}

/// `npcStations.jsonl`
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NpcStation {
    #[serde(rename = "_key")]
    pub id: i64,
    pub celestial_index: Option<i32>,
    #[serde(rename = "operationID")]
    pub operation_id: i64,
    #[serde(rename = "orbitID")]
    pub orbit_id: i64,
    pub orbit_index: Option<i32>,
    #[serde(rename = "ownerID")]
    pub owner_id: i64,
    pub position: Position3D,
    pub reprocessing_efficiency: f64,
    pub reprocessing_hangar_flag: i32,
    pub reprocessing_stations_take: f64,
    #[serde(rename = "solarSystemID")]
    pub solar_system_id: i64,
    #[serde(rename = "typeID")]
    pub type_id: i64,
    pub use_operation_name: bool,
}

/// `npcCharacters.jsonl`
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NpcCharacter {
    #[serde(rename = "_key")]
    pub id: i64,
    #[serde(rename = "bloodlineID")]
    pub bloodline_id: i64,
    pub ceo: bool,
    #[serde(rename = "corporationID")]
    pub corporation_id: i64,
    pub gender: bool,
    #[serde(rename = "locationID")]
    pub location_id: Option<i64>,
    pub name: LocalizedString,
    #[serde(rename = "raceID")]
    pub race_id: i64,
    pub start_date: Option<String>,
    pub unique_name: bool,
    pub skills: Option<Vec<NpcCharacterSkill>>,
    #[serde(rename = "ancestryID")]
    pub ancestry_id: Option<i64>,
    #[serde(rename = "careerID")]
    pub career_id: Option<i64>,
    #[serde(rename = "schoolID")]
    pub school_id: Option<i64>,
    #[serde(rename = "specialityID")]
    pub speciality_id: Option<i64>,
    pub agent: Option<NpcCharacterAgent>,
    pub description: Option<String>,
}

/// Nested `skills` entry of `npcCharacters.jsonl`.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NpcCharacterSkill {
    #[serde(rename = "typeID")]
    pub type_id: i64,
}

/// Nested `agent` object of `npcCharacters.jsonl`.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NpcCharacterAgent {
    #[serde(rename = "agentTypeID")]
    pub agent_type_id: i64,
    #[serde(rename = "divisionID")]
    pub division_id: i64,
    pub is_locator: bool,
    pub level: i32,
}

/// `npcCorporationDivisions.jsonl`
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NpcCorporationDivision {
    #[serde(rename = "_key")]
    pub id: i64,
    pub display_name: Option<String>,
    pub internal_name: String,
    pub leader_type_name: LocalizedString,
    pub name: LocalizedString,
    pub description: Option<LocalizedString>,
}

/// `factions.jsonl`
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Faction {
    #[serde(rename = "_key")]
    pub id: i64,
    #[serde(rename = "corporationID")]
    pub corporation_id: Option<i64>,
    pub description: LocalizedString,
    pub flat_logo: Option<String>,
    pub flat_logo_with_name: Option<String>,
    #[serde(rename = "iconID")]
    pub icon_id: i64,
    pub member_races: Vec<i32>,
    #[serde(rename = "militiaCorporationID")]
    pub militia_corporation_id: Option<i64>,
    pub name: LocalizedString,
    pub short_description: Option<LocalizedString>,
    pub size_factor: f64,
    #[serde(rename = "solarSystemID")]
    pub solar_system_id: i64,
    pub unique_name: bool,
}

/// `agentTypes.jsonl`
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentType {
    #[serde(rename = "_key")]
    pub id: i64,
    pub name: String,
}

/// `agentsInSpace.jsonl`
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentInSpace {
    #[serde(rename = "_key")]
    pub id: i64,
    #[serde(rename = "dungeonID")]
    pub dungeon_id: i64,
    #[serde(rename = "solarSystemID")]
    pub solar_system_id: i64,
    #[serde(rename = "spawnPointID")]
    pub spawn_point_id: i64,
    #[serde(rename = "typeID")]
    pub type_id: i64,
}

/// `corporationActivities.jsonl`
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CorporationActivity {
    #[serde(rename = "_key")]
    pub id: i64,
    pub name: LocalizedString,
}

/// `stationOperations.jsonl`
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StationOperation {
    #[serde(rename = "_key")]
    pub id: i64,
    #[serde(rename = "activityID")]
    pub activity_id: i64,
    pub border: f64,
    pub corridor: f64,
    pub description: Option<LocalizedString>,
    pub fringe: f64,
    pub hub: f64,
    pub manufacturing_factor: f64,
    pub operation_name: LocalizedString,
    pub ratio: f64,
    pub research_factor: f64,
    pub services: Vec<i32>,
    pub station_types: Option<Vec<StationOperationStationType>>,
}

/// Nested `stationTypes` entry of `stationOperations.jsonl`.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StationOperationStationType {
    #[serde(rename = "_key")]
    pub id: i64,
    #[serde(rename = "_value")]
    pub value: i32,
}

/// `stationServices.jsonl`
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StationService {
    #[serde(rename = "_key")]
    pub id: i64,
    pub service_name: LocalizedString,
    pub description: Option<LocalizedString>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_npc_corporations() {
        let Some(rows) = crate::sde::parse_sample::<NpcCorporation>() else {
            return;
        };
        assert!(!rows.is_empty());
    }

    #[test]
    fn parses_npc_stations() {
        let Some(rows) = crate::sde::parse_sample::<NpcStation>() else {
            return;
        };
        assert!(!rows.is_empty());
    }

    #[test]
    fn parses_npc_characters() {
        let Some(rows) = crate::sde::parse_sample::<NpcCharacter>() else {
            return;
        };
        assert!(!rows.is_empty());
    }

    #[test]
    fn parses_npc_corporation_divisions() {
        let Some(rows) = crate::sde::parse_sample::<NpcCorporationDivision>() else {
            return;
        };
        assert!(!rows.is_empty());
    }

    #[test]
    fn parses_factions() {
        let Some(rows) = crate::sde::parse_sample::<Faction>() else {
            return;
        };
        assert!(!rows.is_empty());
    }

    #[test]
    fn parses_agent_types() {
        let Some(rows) = crate::sde::parse_sample::<AgentType>() else {
            return;
        };
        assert!(!rows.is_empty());
    }

    #[test]
    fn parses_agents_in_space() {
        let Some(rows) = crate::sde::parse_sample::<AgentInSpace>() else {
            return;
        };
        assert!(!rows.is_empty());
    }

    #[test]
    fn parses_corporation_activities() {
        let Some(rows) = crate::sde::parse_sample::<CorporationActivity>() else {
            return;
        };
        assert!(!rows.is_empty());
    }

    #[test]
    fn parses_station_operations() {
        let Some(rows) = crate::sde::parse_sample::<StationOperation>() else {
            return;
        };
        assert!(!rows.is_empty());
    }

    #[test]
    fn parses_station_services() {
        let Some(rows) = crate::sde::parse_sample::<StationService>() else {
            return;
        };
        assert!(!rows.is_empty());
    }
}
