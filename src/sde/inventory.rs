use super::common::LocalizedString;
use serde::Deserialize;

/// `types.jsonl`
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Type {
    #[serde(rename = "_key")]
    pub id: i64,
    #[serde(rename = "groupID")]
    pub group_id: i64,
    pub mass: Option<f64>,
    pub name: LocalizedString,
    pub portion_size: i32,
    pub published: bool,
    pub volume: Option<f64>,
    pub radius: Option<f64>,
    pub description: Option<LocalizedString>,
    #[serde(rename = "graphicID")]
    pub graphic_id: Option<i64>,
    #[serde(rename = "soundID")]
    pub sound_id: Option<i64>,
    #[serde(rename = "iconID")]
    pub icon_id: Option<i64>,
    #[serde(rename = "raceID")]
    pub race_id: Option<i64>,
    pub base_price: Option<f64>,
    #[serde(rename = "marketGroupID")]
    pub market_group_id: Option<i64>,
    pub capacity: Option<f64>,
    #[serde(rename = "metaGroupID")]
    pub meta_group_id: Option<i64>,
    pub tech_level: Option<i32>,
    pub meta_level: Option<i32>,
    #[serde(rename = "variationParentTypeID")]
    pub variation_parent_type_id: Option<i64>,
    #[serde(rename = "factionID")]
    pub faction_id: Option<i64>,
    #[serde(rename = "shipTreeGroupID")]
    pub ship_tree_group_id: Option<i64>,
}

/// `groups.jsonl`
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
#[expect(
    clippy::struct_excessive_bools,
    reason = "mirrors the SDE file, whose flags are independent booleans"
)]
pub struct Group {
    #[serde(rename = "_key")]
    pub id: i64,
    pub anchorable: bool,
    pub anchored: bool,
    #[serde(rename = "categoryID")]
    pub category_id: i64,
    pub fittable_non_singleton: bool,
    pub name: LocalizedString,
    pub published: bool,
    pub use_base_price: bool,
    #[serde(rename = "iconID")]
    pub icon_id: Option<i64>,
}

/// `categories.jsonl`
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Category {
    #[serde(rename = "_key")]
    pub id: i64,
    pub name: LocalizedString,
    pub published: bool,
    #[serde(rename = "iconID")]
    pub icon_id: Option<i64>,
}

/// `blueprints.jsonl`
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Blueprint {
    #[serde(rename = "_key")]
    pub id: i64,
    pub activities: BlueprintActivities,
    #[serde(rename = "blueprintTypeID")]
    pub blueprint_type_id: i64,
    pub max_production_limit: i32,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BlueprintActivities {
    pub copying: Option<BlueprintActivity>,
    pub invention: Option<BlueprintActivity>,
    pub manufacturing: Option<BlueprintActivity>,
    pub reaction: Option<BlueprintActivity>,
    #[serde(rename = "research_material")]
    pub research_material: Option<BlueprintActivity>,
    #[serde(rename = "research_time")]
    pub research_time: Option<BlueprintActivity>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BlueprintActivity {
    pub materials: Option<Vec<BlueprintMaterial>>,
    pub products: Option<Vec<BlueprintProduct>>,
    pub skills: Option<Vec<BlueprintSkill>>,
    pub time: i32,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BlueprintMaterial {
    pub quantity: i32,
    #[serde(rename = "typeID")]
    pub type_id: i64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BlueprintProduct {
    pub probability: Option<f64>,
    pub quantity: i32,
    #[serde(rename = "typeID")]
    pub type_id: i64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BlueprintSkill {
    pub level: i32,
    #[serde(rename = "typeID")]
    pub type_id: i64,
}

/// `marketGroups.jsonl`
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MarketGroup {
    #[serde(rename = "_key")]
    pub id: i64,
    pub description: Option<LocalizedString>,
    pub has_types: bool,
    #[serde(rename = "iconID")]
    pub icon_id: Option<i64>,
    pub name: LocalizedString,
    #[serde(rename = "parentGroupID")]
    pub parent_group_id: Option<i64>,
}

/// `metaGroups.jsonl`
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MetaGroup {
    #[serde(rename = "_key")]
    pub id: i64,
    pub color: Option<MetaGroupColor>,
    pub name: LocalizedString,
    #[serde(rename = "iconID")]
    pub icon_id: Option<i64>,
    pub icon_suffix: Option<String>,
    pub description: Option<LocalizedString>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MetaGroupColor {
    pub b: f64,
    pub g: f64,
    pub r: f64,
}

/// `typeMaterials.jsonl`
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TypeMaterials {
    #[serde(rename = "_key")]
    pub id: i64,
    pub materials: Option<Vec<TypeMaterial>>,
    pub randomized_materials: Option<Vec<TypeMaterialRandomized>>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TypeMaterial {
    #[serde(rename = "materialTypeID")]
    pub material_type_id: i64,
    pub quantity: i32,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TypeMaterialRandomized {
    #[serde(rename = "materialTypeID")]
    pub material_type_id: i64,
    pub quantity_max: i32,
    pub quantity_min: i32,
}

/// `typeDogma.jsonl`
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TypeDogma {
    #[serde(rename = "_key")]
    pub id: i64,
    pub dogma_attributes: Vec<TypeDogmaAttribute>,
    pub dogma_effects: Option<Vec<TypeDogmaEffect>>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TypeDogmaAttribute {
    #[serde(rename = "attributeID")]
    pub attribute_id: i64,
    pub value: f64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TypeDogmaEffect {
    #[serde(rename = "effectID")]
    pub effect_id: i64,
    pub is_default: bool,
}

/// `typeBonus.jsonl`
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TypeBonus {
    #[serde(rename = "_key")]
    pub id: i64,
    pub role_bonuses: Option<Vec<TypeBonusEntry>>,
    pub types: Option<Vec<TypeBonusTypePair>>,
    #[serde(rename = "iconID")]
    pub icon_id: Option<i64>,
    pub misc_bonuses: Option<Vec<TypeBonusEntry>>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TypeBonusTypePair {
    #[serde(rename = "_key")]
    pub id: i64,
    #[serde(rename = "_value")]
    pub value: Vec<TypeBonusEntry>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TypeBonusEntry {
    pub bonus: Option<f64>,
    pub bonus_text: LocalizedString,
    pub importance: i32,
    #[serde(rename = "unitID")]
    pub unit_id: Option<i64>,
    pub is_positive: Option<bool>,
}

/// `typeLists.jsonl`
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TypeList {
    #[serde(rename = "_key")]
    pub id: i64,
    #[serde(rename = "includedTypeIDs")]
    pub included_type_ids: Option<Vec<i64>>,
    pub name: String,
    #[serde(rename = "includedGroupIDs")]
    pub included_group_ids: Option<Vec<i64>>,
    #[serde(rename = "includedCategoryIDs")]
    pub included_category_ids: Option<Vec<i64>>,
    #[serde(rename = "excludedGroupIDs")]
    pub excluded_group_ids: Option<Vec<i64>>,
    #[serde(rename = "excludedTypeIDs")]
    pub excluded_type_ids: Option<Vec<i64>>,
    #[serde(rename = "excludedCategoryIDs")]
    pub excluded_category_ids: Option<Vec<i64>>,
    pub display_description: Option<LocalizedString>,
    pub display_name: Option<LocalizedString>,
}

/// `typeElements.jsonl`
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TypeElement {
    #[serde(rename = "_key")]
    pub id: i64,
    pub elements: Vec<TypeElementEntry>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TypeElementEntry {
    #[serde(rename = "_key")]
    pub id: i64,
    #[serde(rename = "_value")]
    pub value: i32,
}

/// `compressibleTypes.jsonl`
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompressibleType {
    #[serde(rename = "_key")]
    pub id: i64,
    #[serde(rename = "compressedTypeID")]
    pub compressed_type_id: i64,
}

/// `contrabandTypes.jsonl`
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContrabandType {
    #[serde(rename = "_key")]
    pub id: i64,
    pub factions: Vec<ContrabandFaction>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContrabandFaction {
    #[serde(rename = "_key")]
    pub id: i64,
    pub attack_min_sec: f64,
    pub confiscate_min_sec: f64,
    pub fine_by_value: f64,
    pub standing_loss: f64,
}

/// `dynamicItemAttributes.jsonl`
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DynamicItemAttribute {
    #[serde(rename = "_key")]
    pub id: i64,
    #[serde(rename = "attributeIDs")]
    pub attribute_ids: Vec<DynamicItemAttributeEntry>,
    pub input_output_mapping: Vec<DynamicItemInputOutput>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DynamicItemAttributeEntry {
    #[serde(rename = "_key")]
    pub id: i64,
    pub high_is_good: Option<bool>,
    pub max: f64,
    pub min: f64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DynamicItemInputOutput {
    pub applicable_types: Vec<i32>,
    pub resulting_type: i32,
}

/// `controlTowerResources.jsonl`
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ControlTowerResource {
    #[serde(rename = "_key")]
    pub id: i64,
    pub resources: Vec<ControlTowerResourceEntry>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ControlTowerResourceEntry {
    #[serde(rename = "factionID")]
    pub faction_id: Option<i64>,
    pub min_security_level: Option<f64>,
    pub purpose: i32,
    pub quantity: i32,
    #[serde(rename = "resourceTypeID")]
    pub resource_type_id: i64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_types() {
        let Some(rows) = crate::sde::parse_sample::<Type>() else {
            return;
        };
        assert!(!rows.is_empty());
    }

    #[test]
    fn parses_groups() {
        let Some(rows) = crate::sde::parse_sample::<Group>() else {
            return;
        };
        assert!(!rows.is_empty());
    }

    #[test]
    fn parses_categories() {
        let Some(rows) = crate::sde::parse_sample::<Category>() else {
            return;
        };
        assert!(!rows.is_empty());
    }

    #[test]
    fn parses_blueprints() {
        let Some(rows) = crate::sde::parse_sample::<Blueprint>() else {
            return;
        };
        assert!(!rows.is_empty());
    }

    #[test]
    fn parses_market_groups() {
        let Some(rows) = crate::sde::parse_sample::<MarketGroup>() else {
            return;
        };
        assert!(!rows.is_empty());
    }

    #[test]
    fn parses_meta_groups() {
        let Some(rows) = crate::sde::parse_sample::<MetaGroup>() else {
            return;
        };
        assert!(!rows.is_empty());
    }

    #[test]
    fn parses_type_materials() {
        let Some(rows) = crate::sde::parse_sample::<TypeMaterials>() else {
            return;
        };
        assert!(!rows.is_empty());
    }

    #[test]
    fn parses_type_dogma() {
        let Some(rows) = crate::sde::parse_sample::<TypeDogma>() else {
            return;
        };
        assert!(!rows.is_empty());
    }

    #[test]
    fn parses_type_bonus() {
        let Some(rows) = crate::sde::parse_sample::<TypeBonus>() else {
            return;
        };
        assert!(!rows.is_empty());
    }

    #[test]
    fn parses_type_lists() {
        let Some(rows) = crate::sde::parse_sample::<TypeList>() else {
            return;
        };
        assert!(!rows.is_empty());
    }

    #[test]
    fn parses_type_elements() {
        let Some(rows) = crate::sde::parse_sample::<TypeElement>() else {
            return;
        };
        assert!(!rows.is_empty());
    }

    #[test]
    fn parses_compressible_types() {
        let Some(rows) = crate::sde::parse_sample::<CompressibleType>() else {
            return;
        };
        assert!(!rows.is_empty());
    }

    #[test]
    fn parses_contraband_types() {
        let Some(rows) = crate::sde::parse_sample::<ContrabandType>() else {
            return;
        };
        assert!(!rows.is_empty());
    }

    #[test]
    fn parses_dynamic_item_attributes() {
        let Some(rows) = crate::sde::parse_sample::<DynamicItemAttribute>() else {
            return;
        };
        assert!(!rows.is_empty());
    }

    #[test]
    fn parses_control_tower_resources() {
        let Some(rows) = crate::sde::parse_sample::<ControlTowerResource>() else {
            return;
        };
        assert!(!rows.is_empty());
    }
}
