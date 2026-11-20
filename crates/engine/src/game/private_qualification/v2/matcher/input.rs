//! The separate frozen matcher schema maps into canonical Program/native seed types.
use super::*;
use crate::setup::StartingLife;
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Case {
    pub id: String,
    pub scope: String,
    pub participants: Vec<String>,
    pub active: String,
    pub cards: Vec<Card>,
    pub initial_battlefield_order: Vec<String>,
    pub initial_world_cohorts: Vec<Vec<String>>,
    pub capture_seed_handles: bool,
    pub operation: Operation,
    pub stop: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Card {
    pub alias: String,
    pub card_id: u64,
    pub owner: String,
    pub controller: Option<String>,
    pub zone: String,
    kind: String,
    names: spellwake_contracts::names::ObjectNames,
    supertypes: Vec<String>,
    creature_subtypes: Vec<Value>,
    power: Option<spellwake_contracts::RulesInteger>,
    toughness: Option<spellwake_contracts::RulesInteger>,
    trigger_id: Option<u64>,
    body: Option<Value>,
    condition: Option<Value>,
    grouping: Option<String>,
}

#[derive(Deserialize)]
#[serde(tag = "method", deny_unknown_fields)]
pub(super) enum Operation {
    #[serde(rename = "actual_canonical_zone_move_then_common_owning_publisher")]
    Zone {
        object_alias: String,
        from: String,
        to: String,
        new_controller: Option<String>,
    },
    #[serde(rename = "actual_set_tapped_then_common_owning_publisher")]
    Tap {
        object_alias: String,
        before_tapped: bool,
        after_tapped: bool,
    },
    #[serde(rename = "actual_hand_to_Graveyard_discard_then_owning_publisher")]
    Discard {
        discarding_player: String,
        discarded_card: DiscardCard,
    },
    #[serde(rename = "actual_turn_scheduler_Untap_to_Upkeep_then_entry_publisher")]
    Step { active: String, turn: u64 },
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct DiscardCard {
    pub alias: String,
    pub card_id: u64,
    pub owner: String,
    pub zone: String,
    pub definition: String,
}

pub(super) fn destination(
    zone: &str,
    controller: Option<&str>,
) -> Result<ObjectDestination, String> {
    match (zone, controller) {
        ("Battlefield", Some(controller)) => Ok(ObjectDestination::Battlefield {
            controller: packet::seat(controller)?,
        }),
        ("Hand", None) => Ok(ObjectDestination::Hand),
        ("Graveyard", None) => Ok(ObjectDestination::Graveyard),
        ("Exile", None) => Ok(ObjectDestination::Exile),
        ("Command", None) => Ok(ObjectDestination::Command),
        _ => Err("unsupported/contradictory matcher destination/controller".into()),
    }
}

pub(super) fn prepare(
    case: &Case,
    shared: &Value,
    definitions: &BTreeMap<String, Value>,
    vocabulary: &std::sync::Arc<spellwake_contracts::creature_types::CreatureTypeVocabulary>,
) -> Result<(GameSeed, packet::AliasContext, BTreeMap<String, CardId>), String> {
    let declared = spellwake_contracts::creature_types::CreatureVocabularyId::try_from(
        shared["creature_vocabulary"]
            .as_str()
            .ok_or("matcher shared seed requires explicit vocabulary identity")?
            .to_owned(),
    )
    .map_err(|error| error.to_string())?;
    if declared != vocabulary.id() {
        return Err("matcher shared seed vocabulary differs from actual supplied V".into());
    }
    if shared["floating_mana"] != json!([])
        || shared["libraries"] != json!({"A":[],"B":[]})
        || shared["initial_stack"] != json!([])
        || shared["combat_rules"] != "UnrestrictedFreeForAll"
    {
        return Err("unsupported matcher resources".into());
    }
    let name_rules = packet::validated_name_rules(
        serde_json::from_value(
            shared
                .get("name_rules")
                .ok_or("matcher requires explicit name relation")?
                .clone(),
        )
        .map_err(|error| error.to_string())?,
        shared["name_rules_id"]
            .as_str()
            .ok_or("matcher requires name rules identity")?,
    )?;
    let mut aliases = Vec::new();
    let mut cards = Vec::new();
    for card in &case.cards {
        let literal = match card.kind.as_str() {
            "Creature" => {
                json!({"power":{"tag":"Number","value":card.power.as_ref().ok_or("creature power missing")?},"toughness":{"tag":"Number","value":card.toughness.as_ref().ok_or("creature toughness missing")?}})
            }
            "Artifact" if card.power.is_none() && card.toughness.is_none() => Value::Null,
            _ => return Err("unsupported matcher type/PT input".into()),
        };
        let triggered = match (card.trigger_id, &card.body, &card.condition, &card.grouping) {
            (Some(id), Some(body), Some(condition), Some(grouping)) => {
                json!([{"id":id,"condition":condition,"grouping":grouping,"body":body}])
            }
            (None, None, None, None) => json!([]),
            _ => return Err("incomplete matcher trigger components".into()),
        };
        let program = program::decode(
            json!({"names":card.names,"supertypes":card.supertypes,"land_subtypes":[],"card_types":[card.kind],"creature_subtypes":card.creature_subtypes,"kind":{"tag":"Permanent","printed_pt":literal,"casting_cost":null},"color_indicator":null,"effect_defined_colors":null,"mana":[],"activated":[],"triggered":triggered,"continuous":null}),
            vocabulary,
        )?;
        let destination = destination(&card.zone, card.controller.as_deref())?;
        aliases.push((CardId(card.card_id), card.alias.clone()));
        cards.push(SeedCard {
            card: CardId(card.card_id),
            owner: packet::seat(&card.owner)?,
            destination,
            program,
        });
    }
    if let Operation::Discard {
        discarded_card: card,
        ..
    } = &case.operation
    {
        if card.zone != "Hand" || card.definition != "creature_1_1" {
            return Err("unsupported matcher discarded-card template".into());
        }
        let raw = definitions
            .get(&card.definition)
            .and_then(|v| v.get("native_program"))
            .ok_or("discarded-card definition missing")?;
        let program = program::decode(raw.clone(), vocabulary)?;
        aliases.push((CardId(card.card_id), card.alias.clone()));
        cards.push(SeedCard {
            card: CardId(card.card_id),
            owner: packet::seat(&card.owner)?,
            destination: ObjectDestination::Hand,
            program,
        });
    }
    let context = packet::AliasContext::from_card_aliases(&aliases)?;
    let ids: BTreeMap<_, _> = aliases.into_iter().map(|(id, name)| (name, id)).collect();
    let initial_battlefield_order = case
        .initial_battlefield_order
        .iter()
        .map(|alias| {
            ids.get(alias)
                .copied()
                .ok_or_else(|| format!("unknown matcher battlefield-order alias {alias}"))
        })
        .collect::<Result<_, String>>()?;
    let initial_world_cohorts = case
        .initial_world_cohorts
        .iter()
        .map(|cohort| {
            cohort
                .iter()
                .map(|alias| {
                    ids.get(alias)
                        .copied()
                        .ok_or_else(|| format!("unknown matcher World cohort alias {alias}"))
                })
                .collect::<Result<Vec<_>, String>>()
        })
        .collect::<Result<Vec<_>, String>>()?;
    let life: BTreeMap<String, spellwake_contracts::RulesInteger> =
        serde_json::from_value(shared["life"].clone()).map_err(|error| error.to_string())?;
    Ok((
        GameSeed {
            name_rules,
            creature_vocabulary: vocabulary.clone(),
            seats: case
                .participants
                .iter()
                .map(|p| packet::seat(p))
                .collect::<Result<_, _>>()?,
            active: packet::seat(&case.active)?,
            combat_rules: CombatRules::UnrestrictedFreeForAll,
            cards,
            initial_battlefield_order,
            initial_world_cohorts,
            mana: vec![],
            life: life
                .into_iter()
                .map(|(player, total)| {
                    Ok(StartingLife {
                        player: packet::seat(&player)?,
                        total,
                    })
                })
                .collect::<Result<_, String>>()?,
        },
        context,
        ids,
    ))
}
