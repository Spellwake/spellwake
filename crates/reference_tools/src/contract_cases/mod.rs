//! Selected actual native/codec contract recipes; no expected data is imported.
use crate::root_recipes::input::{Selected, StagePolicy};
use crate::{
    creature_types, current_capture, packet, program, projection,
    resolution_contract_cases as shared, resolution_contract_fixture as fixture,
};
use serde_json::{Value, json};
use spellwake_contracts::creature_types::{
    CreatureTypeId, CreatureTypeVocabulary, CreatureVocabularyError, CreatureVocabularyId,
};
use spellwake_engine::{
    effects::SeedCard,
    effects::{CopyTargetChoice, EffectDecision},
    game::{DecisionId, DecisionReply, RuleFrame},
    object_identity::{CardId, ObjectDestination},
    priority::SeatId,
};
use spellwake_engine::{
    effects::{EffectError, EffectWindow},
    game::{GameCommand, GameContext, GameError, GameId, GameInput, GameState, NativeCandidate},
    program::{CreatureTypeSlot, DefinitionError, ExecutableBody, Program},
};
use std::cell::Cell;
use std::{collections::BTreeSet, sync::Arc};

type PreparedSeedInput = (GameId, GameContext, spellwake_engine::setup::GameSeed);
type PreparedSelection = Option<Result<PreparedSeedInput, String>>;
// Test-only contract case enum kept inline across test fixtures.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, PartialEq, Eq)]
enum Native {
    ReplyLowering {
        public: Box<fixture::PublicNative>,
        result: Result<DecisionReply, String>,
    },
    IsolatedId {
        vocabulary: Result<CreatureTypeVocabulary, CreatureVocabularyError>,
        parsed: Option<Result<Value, String>>,
        result: Option<Result<CreatureTypeId, String>>,
    },
    Id(Result<CreatureTypeId, String>),
    Slot(Result<CreatureTypeSlot, String>),
    Json(Result<Value, String>),
    Programs {
        inputs: Vec<Program>,
        results: Vec<Result<(), DefinitionError>>,
        bodies: Vec<(ExecutableBody, Result<(), DefinitionError>)>,
    },
    Effect(Box<Result<EffectWindow, EffectError>>),
    Game(Box<Result<NativeCandidate, GameError>>),
    Public(Box<fixture::PublicNative>),
    EffectReply {
        public: Box<fixture::PublicNative>,
        frame: EffectDecision,
        result: Box<Result<spellwake_engine::effects::RuleProgress, EffectError>>,
    },
    DecodeProgram {
        decoded: Box<Result<Program, String>>,
        validation: Option<Result<(), DefinitionError>>,
    },
    Raw {
        selected: Box<Result<Selected, String>>,
        prepared: Box<PreparedSelection>,
        initialized: Box<Option<Result<Result<NativeCandidate, GameError>, String>>>,
    },
    SeedDecode(
        Box<
            Result<
                (
                    packet::SelectedInput,
                    GameId,
                    GameContext,
                    spellwake_engine::setup::GameSeed,
                ),
                String,
            >,
        >,
    ),
}
fn text<'a>(value: &'a Value, key: &str) -> Result<&'a str, String> {
    value[key]
        .as_str()
        .ok_or_else(|| format!("literal {key} missing"))
}
fn result<T, E>(
    value: &Result<T, E>,
    ok: impl Fn(&T) -> Value,
    error: impl Fn(&E) -> Value,
) -> Value {
    match value {
        Ok(value) => json!({"tag":"Ok","value":ok(value)}),
        Err(value) => json!({"tag":"Err","error":error(value)}),
    }
}
fn string_error(value: &String) -> Value {
    json!(value)
}
fn validated(value: &Result<(), DefinitionError>) -> Value {
    match value {
        Ok(()) => json!({"tag":"Ok"}),
        Err(error) => projection::definition_error(error),
    }
}
fn add_program(
    calls: &mut Vec<shared::CallRecord>,
    value: &Program,
    vocabulary: &Arc<CreatureTypeVocabulary>,
) -> Result<(), DefinitionError> {
    let (result, record) = shared::borrowed_call(
        "Program::validate",
        value,
        vocabulary,
        |value, vocabulary| value.validate(vocabulary),
        program::view,
        |vocabulary| projection::vocabulary(vocabulary),
        validated,
    );
    calls.push(record);
    result
}
fn add_body(
    calls: &mut Vec<shared::CallRecord>,
    value: &ExecutableBody,
    vocabulary: &Arc<CreatureTypeVocabulary>,
) -> Result<(), DefinitionError> {
    let (result, record) = shared::borrowed_call(
        "ExecutableBody::validate",
        value,
        vocabulary,
        |value, vocabulary| value.validate(vocabulary),
        program::body,
        |vocabulary| projection::vocabulary(vocabulary),
        validated,
    );
    calls.push(record);
    result
}
fn ids(
    value: &Value,
    vocabulary: &Arc<CreatureTypeVocabulary>,
) -> (Result<CreatureTypeId, String>, shared::CallRecord) {
    shared::input_call(
        "creature_types::id",
        &(value.clone(), vocabulary.clone()),
        |(value, vocabulary)| creature_types::id(value, &vocabulary),
        |(value, vocabulary)| json!({"raw":value,"actual_vocabulary":projection::vocabulary(vocabulary)}),
        |value| {
            result(
                value,
                |value| projection::creature_type_id(*value),
                string_error,
            )
        },
    )
}
fn explicit_packet(vocabulary: &CreatureTypeVocabulary) -> Value {
    json!({"schema_version":3,"kind":"deterministic-input-only-producer-recipes","no_expected_outputs":true,"status":"input-only","creature_vocabulary":projection::vocabulary(vocabulary),"definitions":{},"fixtures":[{"id":"explicit-unit","scope":"trusted-unit","seed":{"name_rules":[],"name_rules_id":spellwake_contracts::names::NameRules::literal_names().id().to_string(),"game_id":1,"context":["s2-contract-case-preparation-v1",vocabulary.rules_revision(),"synthetic-contract-inputs-v1","synthetic-s2-contract-bodies-v1","s2-contract-case-preparation-v1"],"creature_vocabulary":vocabulary.id().to_string(),"participants":["A","B"],"active":"A","life":{"A":"20","B":"20"},"combat_rules":"UnrestrictedFreeForAll","initial_boundary":{"kind":"Interaction"},"floating_mana":[],"cards":[],"initial_world_cohorts":[],"initial_battlefield_order":[],"library_order":{"A":[],"B":[]},"trusted_precheck_overrides":[]}}]})
}
fn decode_packet(
    bytes: &[u8],
) -> Result<
    (
        packet::SelectedInput,
        GameId,
        GameContext,
        spellwake_engine::setup::GameSeed,
    ),
    String,
> {
    let selected = packet::Packet::parse(bytes)?.select("explicit-unit")?;
    let prepared = selected.prepare_seed(
        packet::SeedMode::PreparedRoot,
        packet::DefinitionPolicy::ValidateSelected,
    )?;
    Ok((selected, prepared.game, prepared.context, prepared.seed))
}

fn execute(selection: &shared::Selection) -> Result<(Vec<shared::CallRecord>, Native), String> {
    let recipe = &selection.case.recipe;
    let full = &selection.vocabulary;
    let other = fixture::foreign(full)?;
    let mut calls = vec![];
    if recipe["template"] == "PublicSyntheticChoice" || recipe.get("prepare").is_some() {
        return public(selection);
    }
    let op = text(recipe, "op")?;
    if op.contains("contextual CreatureTypeId") {
        let (value, call) = ids(&recipe["raw"], full);
        return Ok((vec![call], Native::Id(value)));
    }
    if op == "canonical slot input decoding" {
        let (value, call) = shared::input_call(
            "creature_types::slot",
            &recipe["raw"],
            creature_types::slot,
            Clone::clone,
            |value| {
                result(
                    value,
                    |slot| json!(slot.ordinal().to_string()),
                    string_error,
                )
            },
        );
        return Ok((vec![call], Native::Slot(value)));
    }
    if op == "CreatureTypeSlot::new" {
        let ordinal = u16::try_from(recipe["u16"].as_u64().ok_or("literal u16 required")?)
            .map_err(|error| error.to_string())?;
        let (value, call) = shared::input_call(
            "CreatureTypeSlot::new",
            &ordinal,
            |value| Ok::<_, String>(CreatureTypeSlot::new(value)),
            |value| json!(value),
            |value| {
                result(
                    value,
                    |slot| json!(slot.ordinal().to_string()),
                    string_error,
                )
            },
        );
        return Ok((vec![call], Native::Slot(value)));
    }
    if op == "decode current explicit seed/context" {
        let mut raw = explicit_packet(full);
        match text(recipe, "remove_field")? {
            "seed.creature_vocabulary" => {
                raw["fixtures"][0]["seed"]
                    .as_object_mut()
                    .unwrap()
                    .remove("creature_vocabulary");
            }
            "packet.creature_vocabulary" => {
                raw.as_object_mut().unwrap().remove("creature_vocabulary");
            }
            _ => return Err("unlisted explicit packet removal".into()),
        };
        let bytes = serde_json::to_vec(&raw).map_err(|error| error.to_string())?;
        let (value, call) = shared::input_call(
            "packet::Packet::parse then select and Seed::prepare",
            &bytes,
            |bytes| decode_packet(&bytes),
            |bytes| json!({"actual_utf8":String::from_utf8_lossy(bytes),"selected_case":"explicit-unit"}),
            |value| {
                result(
                    value,
                    |(value, game, context, seed)| json!({"fixture":value.fixture,"definitions":value.definitions,"metadata":value.metadata,"vocabulary":projection::vocabulary(&value.vocabulary),"game":game.0.to_string(),"context":projection::context(context),"seed":projection::game_seed_input(seed)}),
                    string_error,
                )
            },
        );
        return Ok((vec![call], Native::SeedDecode(Box::new(value))));
    }
    if op == "EffectWindow::new" || op == "GameState::initialize" {
        return constructor(selection);
    }
    if op == "decode PermanentDefinition" {
        let raw = json!({"names":[],"supertypes":[],"land_subtypes":[],"card_types":recipe["types"],"creature_subtypes":recipe["creature_subtypes"],"kind":{"tag":"Permanent","printed_pt":null,"casting_cost":null},"color_indicator":null,"effect_defined_colors":null,"mana":[],"activated":[],"triggered":[],"continuous":null});
        return decode_program(raw, full, false);
    }
    if op == "Program::validate" {
        let value = fixture::definition(recipe, full, &other)?;
        let validation = add_program(&mut calls, &value, full);
        return Ok((
            calls,
            Native::Programs {
                inputs: vec![value],
                results: vec![validation],
                bodies: vec![],
            },
        ));
    }
    if op == "ExecutableBody::validate then enclosing Program::validate"
        || op == "ExecutableBody::validate"
        || op == "validate two separately owned bodies"
    {
        let values = if op == "validate two separately owned bodies" {
            recipe["bodies"]
                .as_array()
                .ok_or("literal bodies missing")?
                .clone()
        } else {
            vec![recipe["body"].clone()]
        };
        let mut programs = vec![];
        let mut results = vec![];
        let mut bodies = vec![];
        for raw in values {
            let value = fixture::body(&raw, full, &other)?;
            let validation = add_body(&mut calls, &value, full);
            bodies.push((value.clone(), validation));
            if op == "ExecutableBody::validate then enclosing Program::validate" {
                let program = Program::instant(spellwake_engine::mana::ManaCost::free(), value);
                let result = add_program(&mut calls, &program, full);
                programs.push(program);
                results.push(result);
            }
        }
        return Ok((
            calls,
            Native::Programs {
                inputs: programs,
                results,
                bodies,
            },
        ));
    }
    if op == "packet::unique_json" {
        let bytes = text(recipe, "raw_utf8")?.as_bytes().to_vec();
        let (value, call) = shared::input_call(
            "packet::unique_json",
            &bytes,
            |bytes| packet::unique_json(&bytes),
            |bytes| json!({"actual_utf8":String::from_utf8_lossy(bytes)}),
            |value| result(value, Clone::clone, string_error),
        );
        return Ok((vec![call], Native::Json(value)));
    }
    if recipe.get("materialized_call").is_some() || selection.case.case_id == "S2C-VM30-068" {
        return raw_selector(selection);
    }
    if op.starts_with("decode current Program syntax/member IDs") {
        let mut raw = program::view(&Program::instant(
            spellwake_engine::mana::ManaCost::free(),
            ExecutableBody::new(
                spellwake_engine::program::BodyTarget::None,
                vec![spellwake_engine::program::Instruction::DrawController {
                    amount: spellwake_engine::program::ValueExpr::Literal(0.into()),
                }],
            ),
        ));
        let delta = &recipe["raw_syntax_delta"];
        if let Some(body) = delta.get("body") {
            let mut body = body.clone();
            if body["instructions"][0]["excluding"]
                .get("all_raw_ID_rows")
                .is_some()
            {
                body["instructions"][0]["excluding"] = json!(
                    full.members()
                        .iter()
                        .map(|label| projection::creature_type_id(
                            full.resolve_label(label).expect("actual full member")
                        ))
                        .collect::<Vec<_>>()
                );
            }
            body["target"] =
                json!({"tag":body["target"].as_str().ok_or("literal raw target missing")?});
            raw["kind"]["body"] = body;
        }
        if let Some(types) = delta.get("card_types") {
            raw["card_types"] = types.clone();
        }
        if let Some(subtypes) = delta.get("creature_subtypes") {
            let mut subtypes = subtypes.clone();
            if subtypes[0].get("lookup").is_some() {
                subtypes = json!([projection::creature_type_id(
                    full.resolve_label("Elf")
                        .ok_or("actual Elf member absent")?
                )]);
            }
            raw["creature_subtypes"] = subtypes;
        }
        if let Some(kind) = delta.get("kind") {
            raw["kind"] = kind.clone();
        }
        return decode_program(raw, full, true);
    }
    if op == "ReplyExactCurrentFrame lowering" {
        return reply_lowering(selection);
    }
    Err(format!(
        "unimplemented exact contract recipe: {} {op}",
        selection.case.case_id
    ))
}
fn decode_program(
    raw: Value,
    vocabulary: &Arc<CreatureTypeVocabulary>,
    validate: bool,
) -> Result<(Vec<shared::CallRecord>, Native), String> {
    let (decoded, call) = shared::input_call(
        "program::decode_for_owner",
        &(raw, vocabulary.clone()),
        |(raw, vocabulary)| program::decode_for_owner(raw, &vocabulary),
        |(raw, vocabulary)| json!({"raw":raw,"actual_vocabulary":projection::vocabulary(vocabulary)}),
        |value| result(value, program::view, string_error),
    );
    let mut calls = vec![call];
    let validation = if validate {
        decoded
            .as_ref()
            .ok()
            .map(|value| add_program(&mut calls, value, vocabulary))
    } else {
        None
    };
    Ok((
        calls,
        Native::DecodeProgram {
            decoded: Box::new(decoded),
            validation,
        },
    ))
}
fn constructor(selection: &shared::Selection) -> Result<(Vec<shared::CallRecord>, Native), String> {
    let recipe = &selection.case.recipe;
    let full = &selection.vocabulary;
    let other = fixture::foreign(full)?;
    let mut seed = fixture::empty_seed(full.clone());
    if recipe["seed"].is_object() {
        if recipe["seed"]["base"] != "EmptyExplicitSeed"
            || recipe["seed"]["add"] != "one Battlefield Creature with legitimate Vforeign Elf ID"
        {
            return Err("unlisted seed constructor delta".into());
        }
        seed.cards.push(SeedCard {
            card: CardId(101),
            owner: SeatId(0),
            destination: ObjectDestination::Battlefield {
                controller: SeatId(0),
            },
            program: Program::creature(
                None,
                2.into(),
                2.into(),
                BTreeSet::from([other
                    .resolve_label("Elf")
                    .ok_or("actual foreign Elf absent")?]),
            ),
        });
        seed.initial_battlefield_order.push(CardId(101));
    }
    let first = Cell::new(true);
    if recipe["op"] == "EffectWindow::new" {
        let (value, record) = shared::input_call(
            "EffectWindow::new",
            &seed,
            |seed| {
                if first.replace(false) {
                    crate::native_calls::effect_initialize_with_adoption(seed, false)
                } else {
                    current_capture::without_capture(|| EffectWindow::new(seed))
                }
            },
            projection::game_seed_input,
            |value| {
                result(
                    value,
                    |value| {
                        projection::effect(value, &packet::AliasContext::default())
                            .expect("actual constructor effect projection")
                    },
                    projection::effect_error,
                )
            },
        );
        return Ok((vec![record], Native::Effect(Box::new(value))));
    }
    let mut context = fixture::context(full);
    if let Some(changes) = recipe["context"]["override"].as_object() {
        let vocabulary = if changes.get("creature_vocabulary") == Some(&json!("Vforeign.id")) {
            other.id()
        } else {
            full.id()
        };
        let rules = changes
            .get("rules")
            .and_then(Value::as_str)
            .unwrap_or(full.rules_revision());
        context = GameContext::new(
            context.release().into(),
            rules.into(),
            context.catalogue().into(),
            context.programs().into(),
            context.profile().into(),
            vocabulary,
            spellwake_contracts::names::NameRules::literal_names().id(),
        )
        .map_err(|error| format!("actual context input constructor rejected: {error:?}"))?;
    }
    let args = (GameId(1), context, seed);
    let (value, record) = shared::input_call(
        "GameState::initialize",
        &args,
        |(game, context, seed)| {
            if first.replace(false) {
                current_capture::initialize(game, context, seed)
            } else {
                current_capture::without_capture(|| GameState::initialize(game, context, seed))
            }
        },
        |(game, context, seed)| json!({"game":game.0.to_string(),"context":projection::context(context),"seed":projection::game_seed_input(seed)}),
        |value| fixture::game_result(value, None),
    );
    Ok((vec![record], Native::Game(Box::new(value))))
}
fn public(selection: &shared::Selection) -> Result<(Vec<shared::CallRecord>, Native), String> {
    let recipe = &selection.case.recipe;
    let default = json!({"template":"PublicSyntheticChoice","board":[],"hidden_cards":[],"body":{"target":"None","instructions":[{"op":"ChooseCreatureType","into":"0","excluding":[]},{"op":"DrawController","amount":{"tag":"ControlledPermanentsOfChosenType","chosen":"0"}}]},"library_for_A":[],"lower_stack_spell":false,"stop":"PendingCreatureTypeExact","max_initialize_calls":1,"max_root_inputs":3});
    let prepared = match recipe.get("prepare") {
        Some(Value::String(value)) if value == "PublicSyntheticChoice real reached Pending" => {
            &default
        }
        Some(value) => value,
        None => recipe,
    };
    let (mut calls, mut native) = fixture::public_pending(prepared, selection.vocabulary.clone())?;
    let other = fixture::foreign(&selection.vocabulary)?;
    if recipe["op"] == "EffectWindow::resume_rule" {
        let frame = match native.state.pending_decision().map(|value| value.frame()) {
            Some(RuleFrame::CreatureType(frame)) => EffectDecision::CreatureType(frame.clone()),
            _ => return Err("actual expected frame missing".into()),
        };
        let source = native.state.execution();
        let mut expected = source.revision();
        if recipe["expected_revision"] == "checked_current_execution_revision+1" {
            expected = expected
                .checked_add(1)
                .ok_or("actual revision addition exhausted")?;
        }
        let actor = packet::seat(text(recipe, "actor")?)?;
        let reply = DecisionReply::CreatureType(fixture::lookup(
            &recipe["reply"],
            &selection.vocabulary,
            &other,
        )?);
        let args = (expected, frame.clone(), actor, reply);
        let first = Cell::new(true);
        let (result, record) = shared::borrowed_call(
            "EffectWindow::resume_rule",
            source,
            &args,
            |source, (expected, frame, actor, reply)| {
                if first.replace(false) && current_capture::is_active() {
                    effect_reply_capture(source, *expected, frame, *actor, reply)
                        .expect("actual native reply capture")
                } else {
                    current_capture::without_capture(|| {
                        source.resume_rule(*expected, frame, *actor, reply.clone(), None)
                    })
                }
            },
            |source| {
                projection::effect(source, &packet::AliasContext::default())
                    .expect("actual effect source projection")
            },
            |(expected, frame, actor, reply)| json!({"expected":expected.to_string(),"frame":projection::effect_frame(frame,source).expect("actual frame projection"),"actor":actor.0,"reply":projection::reply(reply),"turn":null}),
            result_view_progress,
        );
        calls.push(record);
        return Ok((
            calls,
            Native::EffectReply {
                public: Box::new(native),
                frame,
                result: Box::new(result),
            },
        ));
    }
    if recipe["op"] == "GameState::advance Reply"
        || recipe["op"] == "GameState::advance exact current Reply"
    {
        let state = &native.state;
        let pending = state
            .pending_decision()
            .ok_or("actual negative control has no pending decision")?;
        let mut input = GameInput {
            basis: state.basis().clone(),
            actor: SeatId(0),
            command: GameCommand::Reply {
                decision: pending.id(),
                choice: if recipe["reply"].is_string() {
                    DecisionReply::CopyTargets(CopyTargetChoice::Retain)
                } else {
                    DecisionReply::CreatureType(fixture::lookup(
                        &recipe["selection"],
                        &selection.vocabulary,
                        &other,
                    )?)
                },
            },
        };
        if let Some(delta) = recipe["sole_input_mutation"].as_str() {
            match delta {
                "actor=B" => input.actor = SeatId(1),
                "DecisionId=current XOR1" => {
                    let GameCommand::Reply { decision, .. } = &mut input.command else {
                        unreachable!()
                    };
                    *decision = DecisionId(decision.0 ^ 1);
                }
                "basis.game=current XOR1" => input.basis.game = GameId(input.basis.game.0 ^ 1),
                "basis.context.profile=exactProfile+foreignSuffix" => {
                    let context = &input.basis.context;
                    input.basis.context = GameContext::new(
                        context.release().into(),
                        context.rules().into(),
                        context.catalogue().into(),
                        context.programs().into(),
                        format!("{}foreignSuffix", context.profile()),
                        context.creature_vocabulary_id(),
                        spellwake_contracts::names::NameRules::literal_names().id(),
                    )
                    .map_err(|error| format!("mutated explicit input context: {error:?}"))?;
                }
                "basis.epoch=checked_current+1" => {
                    input.basis.epoch = input
                        .basis
                        .epoch
                        .checked_add(1)
                        .ok_or("actual epoch exhausted")?
                }
                "basis.revision=checked_current+1" => {
                    input.basis.revision = input
                        .basis
                        .revision
                        .checked_add(1)
                        .ok_or("actual revision exhausted")?
                }
                _ => return Err("unlisted sole root-input mutation".into()),
            }
        }
        let (result, record) = fixture::advance(state, &input, false);
        calls.push(record);
        native.attempts.push((state.clone(), input, result));
        return Ok((calls, Native::Public(Box::new(native))));
    }
    let replies = if let Some(values) = recipe["reply_sequence"].as_array() {
        values.clone()
    } else if let Some(reply) = recipe.get("reply") {
        vec![reply.clone()]
    } else {
        vec![]
    };
    for reply in replies {
        let id = fixture::lookup(&reply, &selection.vocabulary, &other)?;
        let pending = native
            .state
            .pending_decision()
            .ok_or("literal public reply lacks actual current Pending")?;
        let actor = pending.chooser();
        let input = GameInput {
            basis: native.state.basis().clone(),
            actor,
            command: GameCommand::Reply {
                decision: pending.id(),
                choice: DecisionReply::CreatureType(id),
            },
        };
        let (result, record) = fixture::advance(&native.state, &input, true);
        calls.push(record);
        native
            .attempts
            .push((native.state.clone(), input, result.clone()));
        if let Ok(value) = result {
            native.state = value.into_state();
        }
    }
    if native.attempts.len() as u64
        > prepared["max_root_inputs"]
            .as_u64()
            .ok_or("literal root budget absent")?
    {
        return Err("public template exceeded exact finite root input budget".into());
    }
    Ok((calls, Native::Public(Box::new(native))))
}
fn result_view_progress(
    value: &Result<spellwake_engine::effects::RuleProgress, EffectError>,
) -> Value {
    match value {
        Ok(value) => {
            json!({"tag":"Ok","progress":projection::rule_progress(value,&packet::AliasContext::default()).expect("actual progress projection")})
        }
        Err(error) => projection::effect_error(error),
    }
}
fn effect_reply_capture(
    source: &EffectWindow,
    expected: u64,
    frame: &EffectDecision,
    actor: SeatId,
    reply: &DecisionReply,
) -> Result<Result<spellwake_engine::effects::RuleProgress, EffectError>, String> {
    use crate::current_profile::{
        Append, CallPoint, Phase, ReplayEvidence, SnapshotOwner, SourceCapture,
    };
    let aliases = packet::AliasContext::default();
    let saved = source.clone();
    let id = current_capture::native_call_id()?;
    let before = current_capture::native_snapshot(
        Phase::BeforeCall,
        "EffectWindow::resume_rule",
        SnapshotOwner::EffectWindow(source),
        &aliases,
    )?;
    let result = source.resume_rule(expected, frame, actor, reply.clone(), None);
    let repeated = source.resume_rule(expected, frame, actor, reply.clone(), None);
    let after = result
        .as_ref()
        .ok()
        .map(|value| {
            current_capture::native_snapshot(
                Phase::AfterCall,
                "EffectWindow::resume_rule",
                SnapshotOwner::EffectWindow(value.state()),
                &aliases,
            )
        })
        .transpose()?;
    current_capture::record_native(
        CallPoint {
            call_id: &id,
            before_point: Some(&before),
            after_point: after.as_deref(),
            adopted: false,
        },
        &projection::QualificationCall::EffectCall(projection::EffectCall::ResumeRule {
            expected,
            frame,
            actor,
            reply,
            turn: None,
        }),
        &result,
        &repeated,
        |result| Ok(result_view_progress(result)),
        result
            .as_ref()
            .map(|value| Append::RulesJournal(value.append()))
            .unwrap_or(Append::NotApplicable),
        ReplayEvidence {
            source_eq: Some(*source == saved),
            source_capture: SourceCapture::BeforeInvocation,
        },
        &aliases,
        Some(source),
    )?;
    Ok(result)
}
fn read_pin(value: &Value) -> Result<Vec<u8>, String> {
    let path = current_capture::repository().join(text(value, "path")?);
    let bytes = std::fs::read(&path).map_err(|error| format!("{}: {error}", path.display()))?;
    if shared::hash(&bytes) != text(value, "sha256")?
        || value["bytes"]
            .as_u64()
            .is_some_and(|size| size != bytes.len() as u64)
    {
        return Err("literal materialized input pin differs".into());
    }
    Ok(bytes)
}
fn selected_view(value: &Selected) -> Value {
    json!({"raw_fixture":value.raw_fixture,"definitions":value.definitions,"packet_metadata":value.packet_metadata,"vocabulary":projection::vocabulary(&value.vocabulary)})
}
fn prepared_view(value: &(GameId, GameContext, spellwake_engine::setup::GameSeed)) -> Value {
    json!({"game":value.0.0.to_string(),"context":projection::context(&value.1),"seed":projection::game_seed_input(&value.2)})
}
fn isolated_id(row: &Value) -> Result<(Vec<shared::CallRecord>, Native), String> {
    let baseline = packet::unique_json(&read_pin(
        &row["actual_vocabulary_argument"]["literal_constructor_input"],
    )?)?;
    let raw = &baseline["creature_vocabulary"];
    let arguments = (
        text(raw, "rules_revision")?.to_owned(),
        *CreatureVocabularyId::try_from(text(raw, "rules_sha256")?.to_owned())
            .map_err(|error| error.to_string())?
            .as_bytes(),
        serde_json::from_value::<Vec<String>>(raw["members"].clone())
            .map_err(|error| error.to_string())?,
    );
    let (vocabulary, constructor) = shared::input_call(
        "CreatureTypeVocabulary::new",
        &arguments,
        |(revision, digest, members)| CreatureTypeVocabulary::new(revision, digest, members),
        |(revision, digest, members)| json!({"rules_revision":revision,"rules_sha256":digest.iter().map(|byte|format!("{byte:02x}")).collect::<String>(),"members":members,"literal_constructor_input":row["actual_vocabulary_argument"]["literal_constructor_input"]}),
        |value| result(value, projection::vocabulary, |error| json!(error)),
    );
    let mut calls = vec![constructor];
    let mut parsed = None;
    let mut decoded = None;
    if let Ok(vocabulary) = &vocabulary {
        let bytes = read_pin(&row["isolated_raw_subject"])?;
        let (value, parse) = shared::input_call(
            "packet::unique_json",
            &bytes,
            |bytes| packet::unique_json(&bytes),
            |bytes| json!({"actual_utf8":String::from_utf8_lossy(bytes),"source":row["isolated_raw_subject"]}),
            |value| result(value, Clone::clone, string_error),
        );
        calls.push(parse);
        if let Ok(value) = &value {
            let (value, call) = ids(value, &Arc::new(vocabulary.clone()));
            calls.push(call);
            decoded = Some(value);
        }
        parsed = Some(value);
    }
    Ok((
        calls,
        Native::IsolatedId {
            vocabulary,
            parsed,
            result: decoded,
        },
    ))
}

#[derive(Debug, PartialEq, Eq)]
struct IsolatedSiblingNative {
    negative: Native,
    positive_vocabulary: Option<Arc<CreatureTypeVocabulary>>,
    positive_parsed: Option<Result<Value, String>>,
    positive_result: Option<Result<CreatureTypeId, String>>,
}
impl IsolatedSiblingNative {
    fn completed(&self) -> bool {
        matches!(
            (&self.negative, &self.positive_vocabulary, &self.positive_parsed, &self.positive_result),
            (Native::IsolatedId { vocabulary: Ok(negative_vocabulary), parsed: Some(Ok(_)), result: Some(Err(_)) },
             Some(positive_vocabulary), Some(Ok(_)), Some(Ok(_)))
                if negative_vocabulary == positive_vocabulary.as_ref()
        )
    }
}
fn isolated_positive_sibling(
    selection: &shared::Selection,
) -> Result<(Vec<shared::CallRecord>, IsolatedSiblingNative), String> {
    let materialized =
        packet::unique_json(&read_pin(&selection.case.recipe["materialized_call"])?)?;
    let rows = materialized["cases"]
        .as_array()
        .ok_or("materialized call list absent")?;
    let matches = rows
        .iter()
        .filter(|row| row["case_id"] == selection.case.case_id)
        .collect::<Vec<_>>();
    let [row] = matches.as_slice() else {
        return Err("materialized actual call not unique".into());
    };
    let (mut calls, negative) = raw_selector(selection)?;
    let Native::IsolatedId { vocabulary, .. } = &negative else {
        return Err("positive sibling requires the actual isolated negative recipe".into());
    };
    let mut positive_vocabulary = None;
    let mut positive_parsed = None;
    let mut positive_result = None;
    if let Ok(vocabulary) = vocabulary {
        let vocabulary = Arc::new(vocabulary.clone());
        let bytes = read_pin(&row["positive_isolated_raw_subject"])?;
        let (parsed, parse) = shared::input_call(
            "packet::unique_json",
            &bytes,
            |bytes| packet::unique_json(&bytes),
            |bytes| json!({"actual_utf8":String::from_utf8_lossy(bytes),"source":row["positive_isolated_raw_subject"]}),
            |value| result(value, Clone::clone, string_error),
        );
        calls.push(parse);
        if let Ok(parsed) = &parsed {
            let (value, call) = ids(parsed, &vocabulary);
            calls.push(call);
            positive_result = Some(value);
        }
        positive_vocabulary = Some(vocabulary);
        positive_parsed = Some(parsed);
    }
    Ok((
        calls,
        IsolatedSiblingNative {
            negative,
            positive_vocabulary,
            positive_parsed,
            positive_result,
        },
    ))
}
fn raw_selector(
    selection: &shared::Selection,
) -> Result<(Vec<shared::CallRecord>, Native), String> {
    let recipe = &selection.case.recipe;
    let (bytes, requested, policy_ref, prepare, initialize) =
        if let Some(binding) = recipe.get("materialized_call") {
            let calls = packet::unique_json(&read_pin(binding)?)?;
            let rows = calls["cases"]
                .as_array()
                .ok_or("materialized call list absent")?;
            let matches = rows
                .iter()
                .filter(|row| row["case_id"] == selection.case.case_id)
                .collect::<Vec<_>>();
            let [row] = matches.as_slice() else {
                return Err("materialized actual call not unique".into());
            };
            let _policy = read_pin(&row["sealed_policy"])?;
            if selection.case.case_id == "S2C-VM30-034" {
                return isolated_id(row);
            }
            let bytes = read_pin(&row["raw_packet"])?;
            let boundaries = row["called_boundary"]
                .as_array()
                .ok_or("actual called boundaries absent")?;
            (
                bytes,
                text(row, "requested_case")?.to_owned(),
                row["sealed_policy"].clone(),
                boundaries.iter().any(|value| {
                    value
                        .as_str()
                        .is_some_and(|value| value.contains("::prepare()"))
                }),
                boundaries.iter().any(|value| {
                    value
                        .as_str()
                        .is_some_and(|value| value.contains("::initialize()"))
                }),
            )
        } else {
            let _policy = read_pin(&recipe["policy"])?;
            (
                read_pin(&recipe["packet"])?,
                text(recipe, "fixture")?.to_owned(),
                recipe["policy"].clone(),
                true,
                true,
            )
        };
    let args = (bytes, requested, StagePolicy::ResolutionValues);
    let (selected, parse) = shared::input_call(
        "root_recipes::input::parse",
        &args,
        |(bytes, case, policy)| crate::root_recipes::input::parse(&bytes, &case, policy),
        |(bytes, case, policy)| json!({"actual_utf8":String::from_utf8_lossy(bytes),"requested_case":case,"sealed_policy":policy_ref,"stage_policy":format!("{policy:?}"),"normal_sealed_file_preparse_hash_gate_called":false}),
        |value| result(value, selected_view, string_error),
    );
    let mut records = vec![parse];
    let mut prepared = None;
    let mut initialized = None;
    if let Ok(selected) = &selected {
        if prepare {
            let (value, record) = shared::borrowed_call(
                "root_recipes::input::Selected::prepare",
                selected,
                &(),
                |selected, _| selected.prepare(),
                selected_view,
                |_| json!({}),
                |value| result(value, prepared_view, string_error),
            );
            records.push(record);
            prepared = Some(value);
        }
        if initialize {
            let first = Cell::new(true);
            let (value, record) = shared::borrowed_call(
                "root_recipes::input::Selected::initialize",
                selected,
                &(),
                |selected, _| {
                    if first.replace(false) {
                        selected.initialize()
                    } else {
                        current_capture::without_capture(|| selected.initialize())
                    }
                },
                selected_view,
                |_| json!({}),
                |value| {
                    result(
                        value,
                        |inner| fixture::game_result(inner, None),
                        string_error,
                    )
                },
            );
            records.push(record);
            initialized = Some(value);
        }
    }
    Ok((
        records,
        Native::Raw {
            selected: Box::new(selected),
            prepared: Box::new(prepared),
            initialized: Box::new(initialized),
        },
    ))
}
fn reply_lowering(
    selection: &shared::Selection,
) -> Result<(Vec<shared::CallRecord>, Native), String> {
    let recipe = json!({"template":"PublicSyntheticChoice","board":[],"hidden_cards":[],"body":{"target":"None","instructions":[{"op":"ChooseCreatureType","into":"0","excluding":[]},{"op":"DrawController","amount":{"tag":"ControlledPermanentsOfChosenType","chosen":"0"}}]},"library_for_A":[],"lower_stack_spell":false,"stop":"PendingCreatureTypeExact","max_initialize_calls":1,"max_root_inputs":3});
    let (mut calls, native) = fixture::public_pending(&recipe, selection.vocabulary.clone())?;
    let items = selection.case.recipe["items"]
        .as_array()
        .ok_or("literal reply items missing")?
        .clone();
    let arguments = (items, selection.vocabulary.clone());
    let (value, record) = shared::borrowed_call(
        "root_recipes::ReplyExactCurrentFrame lowering",
        &native.state,
        &arguments,
        |state, (items, vocabulary)| {
            crate::root_recipes::contract_lower_reply(state, items, vocabulary)
        },
        |state| {
            projection::state(state, &packet::AliasContext::default())
                .expect("actual root lowering source")
        },
        |(items, vocabulary)| json!({"kind":"CreatureType","items":items,"actor":"A","actual_vocabulary":projection::vocabulary(vocabulary)}),
        |value| result(value, projection::reply, string_error),
    );
    calls.push(record);
    Ok((
        calls,
        Native::ReplyLowering {
            public: Box::new(native),
            result: value,
        },
    ))
}

const OWNED: &[&str] = &[
    "S2C-VM03-007",
    "S2C-VM03-008",
    "S2C-VM03-009",
    "S2C-VM04-001",
    "S2C-VM04-002",
    "S2C-VM04-003",
    "S2C-VM04-004",
    "S2C-VM04-005",
    "S2C-VM04-006",
    "S2C-VM04-007",
    "S2C-VM04-008",
    "S2C-VM05-001",
    "S2C-VM05-002",
    "S2C-VM05-003",
    "S2C-VM05-004",
    "S2C-VM05-005",
    "S2C-VM05-006",
    "S2C-VM05-007",
    "S2C-VM05-008",
    "S2C-VM05-009",
    "S2C-VM06-001",
    "S2C-VM06-002",
    "S2C-VM06-003",
    "S2C-VM06-004",
    "S2C-VM06-005",
    "S2C-VM06-006",
    "S2C-VM06-007",
    "S2C-VM06-008",
    "S2C-VM06-009",
    "S2C-VM06-010",
    "S2C-VM06-011",
    "S2C-VM06-012",
    "S2C-VM06-013",
    "S2C-VM06-014",
    "S2C-VM06-015",
    "S2C-VM06-016",
    "S2C-VM06-017",
    "S2C-VM06-018",
    "S2C-VM06-019",
    "S2C-VM06-020",
    "S2C-VM07-001",
    "S2C-VM07-002",
    "S2C-VM07-003",
    "S2C-VM07-004",
    "S2C-VM08-009",
    "S2C-VM08-010",
    "S2C-VM08-011",
    "S2C-VM08-012",
    "S2C-VM09-001",
    "S2C-VM09-002",
    "S2C-VM09-003",
    "S2C-VM09-004",
    "S2C-VM09-005",
    "S2C-VM09-006",
    "S2C-VM09-007",
    "S2C-VM09-008",
    "S2C-VM09-009",
    "S2C-VM09-010",
    "S2C-VM09-011",
    "S2C-VM09-012",
    "S2C-VM09-020",
    "S2C-VM10-001",
    "S2C-VM10-002",
    "S2C-VM10-003",
    "S2C-VM29-011",
    "S2C-VM30-001",
    "S2C-VM30-002",
    "S2C-VM30-003",
    "S2C-VM30-004",
    "S2C-VM30-005",
    "S2C-VM30-006",
    "S2C-VM30-007",
    "S2C-VM30-008",
    "S2C-VM30-009",
    "S2C-VM30-010",
    "S2C-VM30-011",
    "S2C-VM30-012",
    "S2C-VM30-013",
    "S2C-VM30-014",
    "S2C-VM30-015",
    "S2C-VM30-016",
    "S2C-VM30-017",
    "S2C-VM30-018",
    "S2C-VM30-019",
    "S2C-VM30-020",
    "S2C-VM30-021",
    "S2C-VM30-022",
    "S2C-VM30-023",
    "S2C-VM30-024",
    "S2C-VM30-025",
    "S2C-VM30-026",
    "S2C-VM30-027",
    "S2C-VM30-028",
    "S2C-VM30-029",
    "S2C-VM30-030",
    "S2C-VM30-031",
    "S2C-VM30-032",
    "S2C-VM30-033",
    "S2C-VM30-034",
    "S2C-VM30-035",
    "S2C-VM30-036",
    "S2C-VM30-037",
    "S2C-VM30-038",
    "S2C-VM30-039",
    "S2C-VM30-040",
    "S2C-VM30-041",
    "S2C-VM30-042",
    "S2C-VM30-048",
    "S2C-VM30-049",
    "S2C-VM30-050",
    "S2C-VM30-051",
    "S2C-VM30-052",
    "S2C-VM30-053",
    "S2C-VM30-054",
    "S2C-VM30-055",
    "S2C-VM30-056",
    "S2C-VM30-057",
    "S2C-VM30-058",
    "S2C-VM30-059",
    "S2C-VM30-060",
    "S2C-VM30-061",
    "S2C-VM30-062",
    "S2C-VM06-021",
    "S2C-VM06-022",
    "S2C-VM06-023",
    "S2C-VM06-024",
    "S2C-VM06-025",
    "S2C-VM06-026",
    "S2C-VM06-027",
    "S2C-VM06-028",
    "S2C-VM06-029",
    "S2C-VM06-030",
    "S2C-VM06-031",
    "S2C-VM30-063",
    "S2C-VM30-064",
    "S2C-VM30-065",
    "S2C-VM30-066",
    "S2C-VM30-067",
    "S2C-VM09-021",
    "S2C-VM09-022",
    "S2C-VM09-023",
    "S2C-VM30-068",
];

fn contains_owner(selection: &shared::Selection) -> bool {
    let recipe = &selection.case.recipe;
    recipe["template"] == "PublicSyntheticChoice"
        || recipe.get("prepare").is_some()
        || matches!(
            recipe["op"].as_str(),
            Some("EffectWindow::new" | "GameState::initialize" | "ReplyExactCurrentFrame lowering")
        )
        || matches!(
            selection.case.case_id.as_str(),
            "S2C-VM30-051" | "S2C-VM30-052" | "S2C-VM30-068"
        )
}
#[test]
#[ignore = "requires exact frozen contract input/case/create-new output and actual D06 vocabulary/publication; captures actual native calls only"]
fn selected_contract_case() {
    let selection = shared::Selection::from_env(OWNED).expect("exact finite contract selection");
    let owner = contains_owner(&selection);
    let profile = "resolution-contract-cases";
    if owner {
        let root = current_capture::repository();
        current_capture::start(
            profile,
            &[
                std::path::PathBuf::from(&selection.input.path),
                root.join("crates/reference_tools/src/contract_cases/mod.rs"),
                root.join("crates/engine/test_support/resolution_contract_fixture.rs"),
                root.join("tests/support/resolution_contract_cases.rs"),
                root.join(
                    "work/resolution-value-choices/contract-case-preparation-v1/recipes.input.json",
                ),
            ],
            None,
        )
        .expect("actual source-bound contract capture");
    }
    let mut invocation = 0;
    let recipe_call = || {
        invocation += 1;
        if invocation == 1 {
            execute(&selection)
        } else {
            current_capture::without_capture(|| execute(&selection))
        }
    };
    let outcome = if owner {
        shared::run_with_finish(&selection, recipe_call, |record| {
            current_capture::finish_record(profile, record)
        })
    } else {
        shared::run(&selection, recipe_call)
    };
    assert!(
        outcome.is_ok(),
        "actual contract producer failed: {outcome:?}"
    );
}

#[test]
#[ignore = "requires frozen VM30-034 input, actual D06 vocabulary/publication and create-new control output; separate from selected221"]
fn isolated_positive_sibling_control() {
    let selection = shared::Selection::from_env(&["S2C-VM30-034"])
        .expect("exact isolated negative case selects its frozen positive sibling");
    let completed = Cell::new(true);
    let outcome = shared::run_with_finish(
        &selection,
        || {
            let (calls, native) = isolated_positive_sibling(&selection)?;
            completed.set(completed.get() && native.completed());
            Ok((calls, native))
        },
        |mut record| {
            record["record_kind"] = json!("ContractCodecSiblingControl");
            record["control_id"] = json!("S2C-VM30-034-positive-isolated-sibling");
            record["counts_toward_selected_221"] = json!(false);
            record["control_completed"] = json!(completed.get());
            Ok(record)
        },
    );
    assert!(
        outcome.is_ok(),
        "actual sibling control failed: {outcome:?}"
    );
    assert!(
        completed.get(),
        "actual positive/negative pair did not complete; control output retained"
    );
}
