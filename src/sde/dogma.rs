use super::common::LocalizedString;
use serde::Deserialize;

/// `dogmaAttributes.jsonl`
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
#[expect(
    clippy::struct_excessive_bools,
    reason = "mirrors the SDE file, whose flags are independent booleans"
)]
pub struct DogmaAttribute {
    #[serde(rename = "_key")]
    pub id: i64,
    #[serde(rename = "attributeCategoryID")]
    pub attribute_category_id: Option<i64>,
    pub data_type: i32,
    pub default_value: f64,
    pub description: Option<String>,
    pub display_when_zero: bool,
    pub high_is_good: bool,
    pub name: String,
    pub published: bool,
    pub stackable: bool,
    pub display_name: Option<LocalizedString>,
    #[serde(rename = "iconID")]
    pub icon_id: Option<i64>,
    pub tooltip_description: Option<LocalizedString>,
    pub tooltip_title: Option<LocalizedString>,
    #[serde(rename = "unitID")]
    pub unit_id: Option<i64>,
    #[serde(rename = "chargeRechargeTimeID")]
    pub charge_recharge_time_id: Option<i64>,
    #[serde(rename = "maxAttributeID")]
    pub max_attribute_id: Option<i64>,
    #[serde(rename = "minAttributeID")]
    pub min_attribute_id: Option<i64>,
}

/// `dogmaEffects.jsonl`
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
#[expect(
    clippy::struct_excessive_bools,
    reason = "mirrors the SDE file, whose flags are independent booleans"
)]
pub struct DogmaEffect {
    #[serde(rename = "_key")]
    pub id: i64,
    pub disallow_auto_repeat: bool,
    #[serde(rename = "dischargeAttributeID")]
    pub discharge_attribute_id: Option<i64>,
    #[serde(rename = "durationAttributeID")]
    pub duration_attribute_id: Option<i64>,
    #[serde(rename = "effectCategoryID")]
    pub effect_category_id: i64,
    pub electronic_chance: bool,
    pub guid: Option<String>,
    pub is_assistance: bool,
    pub is_offensive: bool,
    pub is_warp_safe: bool,
    pub name: String,
    pub propulsion_chance: bool,
    pub published: bool,
    pub range_chance: bool,
    pub distribution: Option<i32>,
    #[serde(rename = "falloffAttributeID")]
    pub falloff_attribute_id: Option<i64>,
    #[serde(rename = "rangeAttributeID")]
    pub range_attribute_id: Option<i64>,
    #[serde(rename = "trackingSpeedAttributeID")]
    pub tracking_speed_attribute_id: Option<i64>,
    pub description: Option<LocalizedString>,
    pub display_name: Option<LocalizedString>,
    #[serde(rename = "iconID")]
    pub icon_id: Option<i64>,
    pub modifier_info: Option<Vec<DogmaEffectModifier>>,
    #[serde(rename = "npcUsageChanceAttributeID")]
    pub npc_usage_chance_attribute_id: Option<i64>,
    #[serde(rename = "npcActivationChanceAttributeID")]
    pub npc_activation_chance_attribute_id: Option<i64>,
    #[serde(rename = "fittingUsageChanceAttributeID")]
    pub fitting_usage_chance_attribute_id: Option<i64>,
    #[serde(rename = "resistanceAttributeID")]
    pub resistance_attribute_id: Option<i64>,
}

/// An entry of `dogmaEffects.jsonl`'s `modifierInfo` array.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DogmaEffectModifier {
    pub domain: String,
    pub func: String,
    #[serde(rename = "modifiedAttributeID")]
    pub modified_attribute_id: Option<i64>,
    #[serde(rename = "modifyingAttributeID")]
    pub modifying_attribute_id: Option<i64>,
    pub operation: Option<i32>,
    #[serde(rename = "groupID")]
    pub group_id: Option<i64>,
    #[serde(rename = "skillTypeID")]
    pub skill_type_id: Option<i64>,
    #[serde(rename = "effectID")]
    pub effect_id: Option<i64>,
}

/// `dogmaUnits.jsonl`
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DogmaUnit {
    #[serde(rename = "_key")]
    pub id: i64,
    pub description: Option<LocalizedString>,
    pub display_name: Option<LocalizedString>,
    pub name: String,
}

/// `dogmaAttributeCategories.jsonl`
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DogmaAttributeCategory {
    #[serde(rename = "_key")]
    pub id: i64,
    pub description: Option<String>,
    pub name: String,
}

/// `dbuffCollections.jsonl`
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DbuffCollection {
    #[serde(rename = "_key")]
    pub id: i64,
    pub aggregate_mode: String,
    pub developer_description: String,
    pub item_modifiers: Option<Vec<DbuffItemModifier>>,
    pub location_group_modifiers: Option<Vec<DbuffLocationGroupModifier>>,
    pub location_modifiers: Option<Vec<DbuffLocationModifier>>,
    pub location_required_skill_modifiers: Option<Vec<DbuffLocationRequiredSkillModifier>>,
    pub operation_name: String,
    #[serde(rename = "showOutputValueInUI")]
    pub show_output_value_in_ui: String,
    pub display_name: Option<LocalizedString>,
}

/// An entry of `dbuffCollections.jsonl`'s `itemModifiers` array.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DbuffItemModifier {
    #[serde(rename = "dogmaAttributeID")]
    pub dogma_attribute_id: i64,
}

/// An entry of `dbuffCollections.jsonl`'s `locationGroupModifiers` array.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DbuffLocationGroupModifier {
    #[serde(rename = "dogmaAttributeID")]
    pub dogma_attribute_id: i64,
    #[serde(rename = "groupID")]
    pub group_id: i64,
}

/// An entry of `dbuffCollections.jsonl`'s `locationModifiers` array.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DbuffLocationModifier {
    #[serde(rename = "dogmaAttributeID")]
    pub dogma_attribute_id: i64,
}

/// An entry of `dbuffCollections.jsonl`'s `locationRequiredSkillModifiers` array.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DbuffLocationRequiredSkillModifier {
    #[serde(rename = "dogmaAttributeID")]
    pub dogma_attribute_id: i64,
    #[serde(rename = "skillID")]
    pub skill_id: i64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_dogma_attributes() {
        let Some(rows) = crate::sde::parse_sample::<DogmaAttribute>() else {
            return;
        };
        assert!(!rows.is_empty());
    }

    #[test]
    fn parses_dogma_effects() {
        let Some(rows) = crate::sde::parse_sample::<DogmaEffect>() else {
            return;
        };
        assert!(!rows.is_empty());
    }

    #[test]
    fn parses_dogma_units() {
        let Some(rows) = crate::sde::parse_sample::<DogmaUnit>() else {
            return;
        };
        assert!(!rows.is_empty());
    }

    #[test]
    fn parses_dogma_attribute_categories() {
        let Some(rows) = crate::sde::parse_sample::<DogmaAttributeCategory>() else {
            return;
        };
        assert!(!rows.is_empty());
    }

    #[test]
    fn parses_dbuff_collections() {
        let Some(rows) = crate::sde::parse_sample::<DbuffCollection>() else {
            return;
        };
        assert!(!rows.is_empty());
    }
}
