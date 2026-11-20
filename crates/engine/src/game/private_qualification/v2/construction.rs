//! Both explicitly selected constructors receive the identical literal seed.
use super::*;

#[derive(Debug, PartialEq, Eq)]
pub(super) struct ConstructionWitness {
    seed: GameSeed,
    invalid_seed: GameSeed,
    effects: Vec<Result<EffectWindow, EffectError>>,
    roots: Vec<Result<NativeCandidate, GameError>>,
}

pub(super) fn run(selected: &packet::SelectedInput) -> Result<(Value, V2Witness), String> {
    let [construct, duplicate] = rows(&selected.fixture, "actions")? else {
        return Err("construction unit requires two literal actions".into());
    };
    if construct
        != &json!({"op":"construct_from_same_seed_twice","constructors":[{"owner":"EffectWindow","method":"new"},{"owner":"GameState","method":"initialize_scheduled","start":{"turn":1,"step":"PrecombatMain"}}]})
        || duplicate["op"] != "attempt_construction_without_adoption"
    {
        return Err("unsupported construction operations".into());
    }
    let prepared = selected.prepare_seed(
        packet::SeedMode::TrustedUnit,
        packet::DefinitionPolicy::ValidateSelected,
    )?;
    if !matches!(prepared.initial, packet::Initial::TrustedUnitPrecheck {})
        || !prepared.trusted_precheck_overrides.is_empty()
        || prepared.initial_status.iter().any(|(_, value)| *value)
    {
        return Err("construction cannot hide preparation".into());
    }
    let alias = string(
        &duplicate["seed_mutation"],
        "append_duplicate_physical_card_alias",
    )?;
    let physical = rows(&selected.fixture["seed"], "cards")?
        .iter()
        .filter(|value| value["alias"] == alias)
        .collect::<Vec<_>>();
    let [physical] = physical.as_slice() else {
        return Err("duplicate alias is not unique".into());
    };
    let id = CardId(
        physical["card_id"]
            .as_u64()
            .ok_or("missing duplicate physical card")?,
    );
    let matching = prepared
        .seed
        .cards
        .iter()
        .filter(|card| card.card == id)
        .collect::<Vec<_>>();
    let [card] = matching.as_slice() else {
        return Err("duplicate seed card is not unique".into());
    };
    let mut invalid_seed = prepared.seed.clone();
    invalid_seed.cards.push((*card).clone());
    let mut effects = Vec::new();
    let mut roots = Vec::new();
    let mut records = Vec::new();
    for repetition in 1..=2 {
        let (result, writer_point) = crate::test_support::native_calls::effect_initialize_recorded(
            prepared.seed.clone(),
            false,
        );
        records.push(capture::attach_writer_point(json!({"action_ordinal":1,"owner":"EffectWindow","method":"new","repetition":repetition,"result":match &result {Ok(value)=>json!({"tag":"Ok","state":projection::effect(value,&prepared.aliases)?}),Err(error)=>projection::effect_error(error)},"adopt_requested":false}),writer_point));
        effects.push(result);
        let (result, writer_point) =
            crate::test_support::current_capture::initialize_scheduled_recorded(
                prepared.game,
                prepared.context.clone(),
                prepared.seed.clone(),
                TurnStart {
                    number: 1,
                    step: Step::PrecombatMain,
                },
                false,
            );
        records.push(capture::attach_writer_point(json!({"action_ordinal":1,"owner":"GameState","method":"initialize_scheduled","repetition":repetition,"result":match &result {Ok(value)=>json!({"tag":"Ok","candidate":projection::candidate(value,None,&prepared.aliases)?}),Err(error)=>projection::game_error(error)},"adopt_requested":false}),writer_point));
        roots.push(result);
    }
    let (result, writer_point) =
        crate::test_support::native_calls::effect_initialize_recorded(invalid_seed.clone(), false);
    records.push(capture::attach_writer_point(json!({"action_ordinal":2,"owner":"EffectWindow","method":"new","seed_mutation":duplicate["seed_mutation"],"result":match &result {Ok(value)=>json!({"tag":"Ok","state":projection::effect(value,&prepared.aliases)?}),Err(error)=>projection::effect_error(error)},"adopt_requested":false}),writer_point));
    effects.push(result);
    let (result, writer_point) =
        crate::test_support::current_capture::initialize_scheduled_recorded(
            prepared.game,
            prepared.context,
            invalid_seed.clone(),
            TurnStart {
                number: 1,
                step: Step::PrecombatMain,
            },
            false,
        );
    records.push(capture::attach_writer_point(json!({"action_ordinal":2,"owner":"GameState","method":"initialize_scheduled","seed_mutation":duplicate["seed_mutation"],"result":match &result {Ok(value)=>json!({"tag":"Ok","candidate":projection::candidate(value,None,&prepared.aliases)?}),Err(error)=>projection::game_error(error)},"adopt_requested":false}),writer_point));
    roots.push(result);
    Ok((
        json!({"status":"completed_recipe","scope":"trusted_construction","records":records,"native_call_count":6,"action_count":2,"second_action_owner_interpretation":"same two constructors explicitly listed by first action; both actual nonadopted Results retained independently","stop":selected.fixture["stop"]}),
        V2Witness::ConstructionBatch(Box::new(ConstructionWitness {
            seed: prepared.seed,
            invalid_seed,
            effects,
            roots,
        })),
    ))
}
