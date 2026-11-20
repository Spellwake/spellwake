//! Input-only JSON mutations followed by the actual Program/EffectWindow owners.
use super::*;
use crate::setup::StartingLife;
use serde::Deserialize;

#[cfg(test)]
mod epoch_tests {
    use super::*;

    #[test]
    fn definition_matrix_requires_both_current_named_and_numeric_epochs() {
        // Empty input-only matrix checks the envelope, not definition execution.
        let current = json!({"schema":"spellwake.definition-admission-inputs.v9","schema_version":9,
            "creature_vocabulary":projection::vocabulary(&creature_types::selected_vocabulary().unwrap()),"cases":[]});
        assert_eq!(preflight(&current, &BTreeMap::new()), Ok(vec![]));
        assert!(
            matches!(run(&current,"unit",&BTreeMap::new()),Err(error) if error == "definition case not found")
        );
        for named in 1..=9 {
            for numeric in 1..=9 {
                if named == 9 && numeric == 9 {
                    continue;
                }
                let old = json!({"schema":format!("spellwake.definition-admission-inputs.v{named}"),"schema_version":numeric});
                assert_eq!(
                    preflight(&old, &BTreeMap::new()),
                    Err("wrong definition matrix schema".into())
                );
                assert!(
                    matches!(run(&old,"unit",&BTreeMap::new()),Err(error) if error == "unsupported definition matrix schema")
                );
            }
        }
    }
}

pub(super) const INPUT_SHA256: &str =
    "fcfc624f7ed05bda8de97630ec1d8a4daf0e050f04641dd94c730435ae7cb49c";

pub(super) fn preflight(
    input: &Value,
    definitions: &BTreeMap<String, Value>,
) -> Result<Vec<String>, String> {
    if input["schema"] != "spellwake.definition-admission-inputs.v9" || input["schema_version"] != 9
    {
        return Err("wrong definition matrix schema".into());
    }
    let vocabulary = creature_types::vocabulary(input["creature_vocabulary"].clone())?;
    rows(input, "cases")?
        .iter()
        .map(|raw| {
            let case: DefinitionCase =
                serde_json::from_value(raw.clone()).map_err(|error| error.to_string())?;
            program::decode_for_owner(
                mutate(definitions, &case.base_native_definition, &case.mutations)?,
                &vocabulary,
            )?;
            Ok(case.id)
        })
        .collect()
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DefinitionCase {
    id: String,
    base_native_definition: String,
    mutations: Vec<Value>,
    operation: String,
    seed: Seed,
    scope: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Seed {
    participants: Vec<String>,
    active: String,
    card_id: u64,
    owner: String,
    destination: Destination,
    life: BTreeMap<String, spellwake_contracts::RulesInteger>,
    creature_vocabulary: String,
    name_rules: Vec<spellwake_contracts::names::ObjectNames>,
    name_rules_id: String,
    initial_battlefield_order: Vec<u64>,
    initial_world_cohorts: Vec<Vec<u64>>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Destination {
    zone: String,
    player: String,
}

fn definition<'a>(
    definitions: &'a BTreeMap<String, Value>,
    name: &str,
) -> Result<&'a Value, String> {
    definitions
        .get(name)
        .and_then(|value| value.get("native_program"))
        .ok_or_else(|| format!("missing native definition {name}"))
}

fn mutate(
    definitions: &BTreeMap<String, Value>,
    name: &str,
    mutations: &[Value],
) -> Result<Value, String> {
    let mut result = definition(definitions, name)?.clone();
    for mutation in mutations {
        let pointer = string(mutation, "json_pointer")?;
        let object = mutation.as_object().ok_or("mutation is not an object")?;
        let replacement = if let Some(value) = object.get("replace_with") {
            if object.len() != 2 {
                return Err("ambiguous replacement mutation".into());
            }
            value.clone()
        } else if let Some(source) = object.get("copy_from_definition") {
            if object.len() != 3 {
                return Err("ambiguous copied mutation".into());
            }
            let source = definition(
                definitions,
                source.as_str().ok_or("invalid copied definition")?,
            )?;
            source
                .pointer(string(mutation, "copy_pointer")?)
                .ok_or("copy pointer missing")?
                .clone()
        } else if let Some(index) = object.get("duplicate_element") {
            if object.len() != 2 {
                return Err("ambiguous duplicate mutation".into());
            }
            let index = usize::try_from(index.as_u64().ok_or("noninteger duplicate index")?)
                .map_err(|error| error.to_string())?;
            let mut values = result
                .pointer(pointer)
                .and_then(Value::as_array)
                .ok_or("duplicate pointer is not an array")?
                .clone();
            values.push(
                values
                    .get(index)
                    .ok_or("duplicate index out of range")?
                    .clone(),
            );
            json!(values)
        } else {
            return Err("unsupported definition mutation".into());
        };
        *result
            .pointer_mut(pointer)
            .ok_or("replacement pointer missing")? = replacement;
    }
    Ok(result)
}

pub(super) fn run(
    input: &Value,
    case: &str,
    definitions: &BTreeMap<String, Value>,
) -> Result<(Value, V2Witness), String> {
    if input["schema"] != "spellwake.definition-admission-inputs.v9" || input["schema_version"] != 9
    {
        return Err("unsupported definition matrix schema".into());
    }
    let cases = rows(input, "cases")?;
    let mut ids = BTreeSet::new();
    for row in cases {
        if !ids.insert(string(row, "id")?) {
            return Err("duplicate definition case ID".into());
        }
    }
    let raw = cases
        .iter()
        .find(|value| value["id"] == case)
        .ok_or("definition case not found")?;
    let selected: DefinitionCase =
        serde_json::from_value(raw.clone()).map_err(|error| error.to_string())?;
    if selected.operation != "validate_authored_definition_then_seed_if_valid"
        || selected.id != case
    {
        return Err("unsupported definition matrix operation".into());
    }
    let shared = &input["shared_seed"];
    if shared["floating_mana"] != json!([])
        || shared["initial_stack"] != json!([])
        || shared["additional_cards"] != json!([])
        || shared["libraries"] != json!({"A":[],"B":[]})
        || shared["combat_rules"] != "UnrestrictedFreeForAll"
    {
        return Err("unsupported definition matrix resources".into());
    }
    if selected.seed.destination.zone != "Hand"
        || selected.seed.destination.player != selected.seed.owner
    {
        return Err("definition matrix destination contradicts explicit owner Hand".into());
    }
    let native_input = mutate(
        definitions,
        &selected.base_native_definition,
        &selected.mutations,
    )?;
    let vocabulary = std::sync::Arc::new(creature_types::vocabulary(
        input["creature_vocabulary"].clone(),
    )?);
    if selected.seed.creature_vocabulary != vocabulary.id().to_string() {
        return Err("definition seed vocabulary differs from supplied actual V".into());
    }
    let program = program::decode_for_owner(native_input.clone(), &vocabulary)?;
    let authored = program.clone();
    let validation = capture::validate_program(&program, &vocabulary)?;
    let validation_view = match &validation {
        Ok(()) => json!({"tag":"Ok"}),
        Err(error) => projection::definition_error(error),
    };
    // GameSeed is an unaccepted constructor input. The actual EffectWindow owner
    // validates every definition before it can register any physical card.
    let seed = GameSeed {
        name_rules: packet::validated_name_rules(
            selected.seed.name_rules,
            &selected.seed.name_rules_id,
        )?,
        creature_vocabulary: vocabulary,
        seats: selected
            .seed
            .participants
            .iter()
            .map(|name| packet::seat(name))
            .collect::<Result<_, _>>()?,
        active: packet::seat(&selected.seed.active)?,
        combat_rules: CombatRules::UnrestrictedFreeForAll,
        cards: vec![SeedCard {
            card: CardId(selected.seed.card_id),
            owner: packet::seat(&selected.seed.owner)?,
            destination: ObjectDestination::Hand,
            program,
        }],
        initial_world_cohorts: selected
            .seed
            .initial_world_cohorts
            .into_iter()
            .map(|cohort| cohort.into_iter().map(CardId).collect())
            .collect(),
        initial_battlefield_order: selected
            .seed
            .initial_battlefield_order
            .into_iter()
            .map(CardId)
            .collect(),
        mana: vec![],
        life: selected
            .seed
            .life
            .into_iter()
            .map(|(name, total)| {
                Ok(StartingLife {
                    player: packet::seat(&name)?,
                    total,
                })
            })
            .collect::<Result<_, String>>()?,
    };
    let result = crate::test_support::native_calls::effect_initialize_with_adoption(seed, false);
    let aliases = packet::AliasContext::from_card_aliases(&[])?;
    let actual = match &result {
        Ok(state) => json!({"tag":"Ok","state":projection::effect(state,&aliases)?}),
        Err(error) => projection::effect_error(error),
    };
    let observation = json!({"status":"completed_recipe","scope":selected.scope,"selected_input":raw,"shared_seed":shared,"mutated_native_definition":native_input,"source_state":null,"authored_validation":validation_view,"constructor_result":actual,"checks_or_body_execution":false,"adopt_requested":false});
    if crate::test_support::current_capture::is_active() {
        let point = crate::test_support::current_capture::native_snapshot(
            crate::test_support::current_profile::Phase::PureInput,
            "definitions::run actual authored validation input",
            crate::test_support::current_profile::SnapshotOwner::ProgramInput {
                program: &authored,
                source_ref: "actual selected/mutated native Program input",
            },
            &aliases,
        )?;
        crate::test_support::current_capture::bind_observation(&point, "/observation")?;
    }
    Ok((
        observation,
        V2Witness::Definition {
            validation,
            construction: Box::new(result),
        },
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        mana::{ManaAbility, ManaOutput},
        program::Program,
    };

    #[test]
    fn duplicate_recipe_retains_input_and_reaches_actual_duplicate_ability_owner_check() {
        let forest = Program::land(
            std::collections::BTreeSet::new(),
            vec![ManaAbility {
                id: ManaAbilityId(1),
                output: ManaOutput::Fixed(BTreeMap::from([(
                    crate::mana::ManaType::Green,
                    1.into(),
                )])),
            }],
        );
        let definitions = BTreeMap::from([(
            "forest".into(),
            json!({"native_program":program::view(&forest)}),
        )]);
        let before = definitions.clone();
        let changed = mutate(
            &definitions,
            "forest",
            &[json!({"json_pointer":"/mana","duplicate_element":0})],
        )
        .unwrap();
        let value = program::decode_for_owner(
            changed,
            &crate::test_support::creature_types::synthetic_vocabulary(),
        )
        .unwrap();
        assert_eq!(
            value.validate(&crate::test_support::creature_types::synthetic_vocabulary()),
            Err(crate::program::DefinitionError::DuplicateAbility)
        );
        assert_eq!(
            EffectWindow::new(GameSeed::with_uniform_life(
                vec![SeatId(0), SeatId(1)],
                SeatId(0),
                vec![SeedCard {
                    card: CardId(1),
                    owner: SeatId(0),
                    destination: ObjectDestination::Hand,
                    program: value
                }],
                vec![],
                vec![],
                20.into(),
                CombatRules::UnrestrictedFreeForAll,
                crate::test_support::creature_types::synthetic_vocabulary(),
                std::sync::Arc::new(spellwake_contracts::names::NameRules::literal_names()),
            )),
            Err(EffectError::InvalidSetup)
        );
        assert_eq!(definitions, before);
        assert!(
            mutate(
                &definitions,
                "forest",
                &[json!({"json_pointer":"/missing","replace_with":null})]
            )
            .is_err()
        );
    }
}
