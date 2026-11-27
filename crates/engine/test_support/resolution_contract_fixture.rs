//! Explicit native fixture construction for the sealed finite contract recipes.
//! This constructs typed inputs; all validation, choices and work stay in native owners.
use super::{creature_types, packet};
use serde_json::Value;
use spellwake_contracts::{
    RulesInteger,
    creature_types::{CreatureTypeId, CreatureTypeVocabulary},
};
use spellwake_engine::{
    activated::{ActivatedAbility, ActivatedAbilityId, SourceCost},
    combat::CombatRules,
    effects::SeedCard,
    game::{GameContext, GameId},
    mana::{ManaAbility, ManaAbilityId, ManaCost, ManaOutput, ManaType},
    object_identity::{CardId, ObjectDestination},
    priority::SeatId,
    program::{
        BodyTarget, CardType, ExecutableBody, Instruction, Program, ProgramKind, TriggeredAbility,
        TriggeredAbilityId, ValueExpr,
    },
    setup::GameSeed,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

fn text<'a>(value: &'a Value, key: &str) -> Result<&'a str, String> {
    value[key]
        .as_str()
        .ok_or_else(|| format!("missing literal {key}"))
}
fn rows(value: &Value) -> Result<&[Value], String> {
    value
        .as_array()
        .map(Vec::as_slice)
        .ok_or("literal array required".into())
}
fn quantity(value: &Value) -> Result<RulesInteger, String> {
    serde_json::from_value(value.clone()).map_err(|error| error.to_string())
}
fn number(value: &Value) -> Result<u64, String> {
    value
        .as_u64()
        .or_else(|| value.as_str().and_then(|value| value.parse().ok()))
        .ok_or("literal unsigned number required".into())
}
pub fn foreign(full: &CreatureTypeVocabulary) -> Result<Arc<CreatureTypeVocabulary>, String> {
    CreatureTypeVocabulary::new(
        "SYNTHETIC-SHAPE-FOREIGN".into(),
        [0; 32],
        full.members().to_vec(),
    )
    .map(Arc::new)
    .map_err(|error| format!("actual foreign vocabulary input: {error:?}"))
}
pub fn lookup(
    value: &Value,
    full: &CreatureTypeVocabulary,
    other: &CreatureTypeVocabulary,
) -> Result<CreatureTypeId, String> {
    let label = value
        .as_str()
        .or_else(|| value["label"].as_str())
        .ok_or("literal type label required")?;
    let vocabulary = if value["from"] == "Vforeign.resolve_label" {
        other
    } else {
        full
    };
    vocabulary
        .resolve_label(label)
        .ok_or_else(|| format!("literal type label absent: {label}"))
}
pub fn type_set(
    value: &Value,
    full: &CreatureTypeVocabulary,
    other: &CreatureTypeVocabulary,
) -> Result<BTreeSet<CreatureTypeId>, String> {
    if let Some(values) = value.as_array() {
        return values
            .iter()
            .map(|value| lookup(value, full, other))
            .collect();
    }
    if value.get("label").is_some() {
        return lookup(value, full, other).map(|value| BTreeSet::from([value]));
    }
    let mut result = if value.get("all_members_from").is_some()
        || value.get("all_actual_Vfull_IDs_except").is_some()
    {
        full.members()
            .iter()
            .map(|label| {
                full.resolve_label(label)
                    .ok_or("actual V member lookup failed".into())
            })
            .collect::<Result<BTreeSet<_>, String>>()?
    } else if let Some(values) = value["union"].as_array() {
        let mut union = BTreeSet::new();
        for value in values {
            union.extend(type_set(value, full, other)?);
        }
        union
    } else {
        return Err("unlisted typed set construction".into());
    };
    if let Some(label) = value["all_actual_Vfull_IDs_except"].as_str() {
        result.remove(
            &full
                .resolve_label(label)
                .ok_or("excepted actual member missing")?,
        );
    }
    Ok(result)
}
pub fn body(
    value: &Value,
    full: &CreatureTypeVocabulary,
    other: &CreatureTypeVocabulary,
) -> Result<ExecutableBody, String> {
    let target = match text(value, "target")? {
        "None" => BodyTarget::None,
        "SingleCreature" => BodyTarget::SingleCreature,
        "SingleSpell(CopyableInstructionSpell)" => {
            BodyTarget::SingleSpell(spellwake_engine::program::SpellFilter::copyable_instruction())
        }
        "SingleSpell(AnySpell)" => {
            BodyTarget::SingleSpell(spellwake_engine::program::SpellFilter::any())
        }
        _ => return Err("unlisted finite body target".into()),
    };
    let instructions = rows(&value["instructions"])?
        .iter()
        .map(|row| {
            Ok(match text(row, "op")? {
                "ChooseCreatureType" => Instruction::PromptChoice(Box::new(
                    spellwake_engine::toolbox::PromptChoiceSpec::choose_creature_type(
                        creature_types::slot(row["into"].clone())?,
                        type_set(&row["excluding"], full, other)?,
                    ),
                )),
                "DrawController" => Instruction::DrawCards(Box::new(
                    spellwake_engine::toolbox::DrawCardsSpec::simple(
                        spellwake_engine::toolbox::PlayerSelector::Resolving(
                            spellwake_engine::program::ResolvingPlayer::Controller,
                        ),
                        match text(&row["amount"], "tag")? {
                            "Literal" => ValueExpr::Literal(quantity(&row["amount"]["value"])?),
                            "ControlledPermanentsOfChosenType" => {
                                spellwake_engine::program::ValueExpr::count_battlefield(
                                    spellwake_engine::program::ObjectFilter::any()
                                        .controlled()
                                        .of_chosen_slot(creature_types::slot(
                                            row["amount"]["chosen"].clone(),
                                        )?),
                                )
                            }
                            _ => return Err("unlisted finite draw amount".into()),
                        },
                    ),
                )),
                "Damage" => {
                    Instruction::DealDamage(Box::new(spellwake_engine::toolbox::DealDamageSpec {
                        source: spellwake_engine::toolbox::DamageSourceQuery::EntrySource,
                        recipients: spellwake_engine::toolbox::DamageTargetQuery::Targets,
                        amount: spellwake_engine::program::DamageAmountSpec::Expr(
                            ValueExpr::Literal(quantity(&row["amount"])?),
                        ),
                        exile_if_dies: false,
                    }))
                }
                "CopySpell" => Instruction::StackOp(Box::new(
                    spellwake_engine::toolbox::StackOpSpec::copy_targeted_spell(),
                )),
                "CounterSpell" => Instruction::StackOp(Box::new(
                    spellwake_engine::toolbox::StackOpSpec::counter_targeted(
                        spellwake_engine::toolbox::CounterDestination::Graveyard,
                    ),
                )),
                _ => return Err("unlisted finite body instruction".into()),
            })
        })
        .collect::<Result<_, String>>()?;
    Ok(ExecutableBody::new(target, instructions))
}
pub fn permanent(name: &str, subtypes: BTreeSet<CreatureTypeId>) -> Result<Program, String> {
    let mut value = match name {
        "Creature" | "Creature2/2" => Program::creature(None, 2.into(), 2.into(), subtypes.clone()),
        "Artifact" => Program::artifact(None, vec![]),
        "Enchantment" => Program::enchantment(None),
        "Land" | "LandWithFixedTapAbility" => Program::land(
            std::collections::BTreeSet::new(),
            vec![ManaAbility {
                id: ManaAbilityId(1),
                output: ManaOutput::Fixed(BTreeMap::from([(ManaType::Green, 1.into())])),
            }],
        ),
        _ => return Err("unlisted finite permanent input".into()),
    };
    value.creature_subtypes = subtypes;
    Ok(value)
}
pub fn definition(
    value: &Value,
    full: &CreatureTypeVocabulary,
    other: &CreatureTypeVocabulary,
) -> Result<Program, String> {
    let definition = value.get("definition").unwrap_or(value);
    let mut result = if definition["kind"] == "Instant" {
        Program::instant(ManaCost::free(), body(&definition["body"], full, other)?)
    } else {
        let name = definition["permanent"]
            .as_str()
            .or_else(|| definition["permanent_template"].as_str())
            .or_else(|| definition["program"].as_str())
            .ok_or("finite permanent template missing")?;
        permanent(
            name,
            definition
                .get("subtypes")
                .map(|value| type_set(value, full, other))
                .transpose()?
                .unwrap_or_default(),
        )?
    };
    if let Some(types) = value["types"].as_array() {
        let ProgramKind::Permanent(_) = &result.kind else {
            return Err("types override requires actual permanent input".into());
        };
        result.card_types = types
            .iter()
            .map(|value| match value.as_str() {
                Some("Creature") => Ok(CardType::Creature),
                Some("Artifact") => Ok(CardType::Artifact),
                Some("Land") => Ok(CardType::Land),
                Some("Enchantment") => Ok(CardType::Enchantment),
                Some("Kindred") => Ok(CardType::Kindred),
                _ => Err("unlisted native permanent type".into()),
            })
            .collect::<Result<_, String>>()?;
    }
    for row in definition
        .get("activated")
        .map(rows)
        .transpose()?
        .unwrap_or(&[])
    {
        result.activated.push(ActivatedAbility {
            id: ActivatedAbilityId(number(&row["id"])?),
            mana_cost: ManaCost {
                generic: quantity(&row["mana_cost"]["generic"])?,
                required: row["mana_cost"]["required"]
                    .as_object()
                    .ok_or("literal required mana missing")?
                    .iter()
                    .map(|(kind, value)| Ok((super::program::mana(kind)?, quantity(value)?)))
                    .collect::<Result<_, String>>()?,
            },
            source_cost: match text(row, "source_cost")? {
                "Tap" => SourceCost::Tap,
                "None" => SourceCost::None,
                "Sacrifice" => SourceCost::Sacrifice,
                _ => return Err("unlisted finite source cost".into()),
            },
            body: body(&row["body"], full, other)?,
        });
    }
    for row in definition
        .get("triggered")
        .map(rows)
        .transpose()?
        .unwrap_or(&[])
    {
        if row["condition"] != serde_json::json!({"tag":"Enters","relation":"SelfObject"}) {
            return Err("unlisted finite trigger condition".into());
        }
        result.triggered.push(TriggeredAbility {
            grouping: spellwake_engine::program::TriggerGrouping::EachOccurrence,
            id: TriggeredAbilityId(number(&row["id"])?),
            condition: spellwake_engine::program::TriggerSpec::enters(
                spellwake_engine::program::ObjectFilter::self_object(),
            ),
            body: body(&row["body"], full, other)?,
        });
    }
    Ok(result)
}
pub fn empty_seed(vocabulary: Arc<CreatureTypeVocabulary>) -> GameSeed {
    GameSeed::with_uniform_life(
        vec![SeatId(0), SeatId(1)],
        SeatId(0),
        vec![],
        vec![],
        vec![],
        20.into(),
        CombatRules::UnrestrictedFreeForAll,
        vocabulary,
        std::sync::Arc::new(spellwake_contracts::names::NameRules::literal_names()),
    )
}
pub fn context(vocabulary: &CreatureTypeVocabulary) -> GameContext {
    GameContext::new(
        "s2-contract-case-preparation-v1".into(),
        vocabulary.rules_revision().into(),
        "synthetic-contract-inputs-v1".into(),
        "synthetic-s2-contract-bodies-v1".into(),
        "s2-contract-case-preparation-v1".into(),
        vocabulary.id(),
        spellwake_contracts::names::NameRules::literal_names().id(),
    )
    .expect("explicit finite matching context")
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublicInput {
    pub game: GameId,
    pub context: GameContext,
    pub seed: GameSeed,
    pub aliases: BTreeMap<String, CardId>,
}
pub fn public_input(
    recipe: &Value,
    vocabulary: Arc<CreatureTypeVocabulary>,
) -> Result<PublicInput, String> {
    if recipe["template"] != "PublicSyntheticChoice" {
        return Err("unlisted public native fixture template".into());
    }
    let other = foreign(&vocabulary)?;
    let mut seed = empty_seed(vocabulary.clone());
    let mut aliases = BTreeMap::new();
    let mut add = |alias: &str,
                   id: u64,
                   owner: SeatId,
                   destination: ObjectDestination,
                   definition: Program|
     -> Result<(), String> {
        if aliases.insert(alias.into(), CardId(id)).is_some() {
            return Err("duplicate finite fixture alias".into());
        }
        // Fixture premise: the authored board additions below supply setup timestamp order.
        if matches!(destination, ObjectDestination::Battlefield { .. }) {
            seed.initial_battlefield_order.push(CardId(id));
        }
        seed.cards.push(SeedCard {
            card: CardId(id),
            owner,
            destination,
            program: definition,
        });
        Ok(())
    };
    add(
        "S",
        100,
        SeatId(0),
        ObjectDestination::Hand,
        Program::instant(
            ManaCost::free(),
            body(&recipe["body"], &vocabulary, &other)?,
        ),
    )?;
    if recipe["lower_stack_spell"] == true {
        add(
            "LOW",
            99,
            SeatId(0),
            ObjectDestination::Hand,
            Program::instant(
                ManaCost::free(),
                ExecutableBody::new(
                    BodyTarget::None,
                    vec![Instruction::DrawCards(Box::new(
                        spellwake_engine::toolbox::DrawCardsSpec::simple(
                            spellwake_engine::toolbox::PlayerSelector::Resolving(
                                spellwake_engine::program::ResolvingPlayer::Controller,
                            ),
                            ValueExpr::Literal(0.into()),
                        ),
                    ))],
                ),
            ),
        )?;
    }
    for (index, row) in recipe
        .get("board")
        .map(rows)
        .transpose()?
        .unwrap_or(&[])
        .iter()
        .enumerate()
    {
        let owner = packet::seat(text(row, "owner")?)?;
        let controller = packet::seat(text(row, "controller")?)?;
        add(
            text(row, "alias")?,
            101 + index as u64,
            owner,
            ObjectDestination::Battlefield { controller },
            permanent(
                text(row, "kind")?,
                type_set(&row["subtypes"], &vocabulary, &other)?,
            )?,
        )?;
    }
    let hidden = recipe
        .get("hidden_cards")
        .map(rows)
        .transpose()?
        .unwrap_or(&[]);
    let mut hidden_by_alias = BTreeMap::new();
    for row in hidden {
        if row["owner"] != "A" || row["zone"] != "Library" {
            return Err("unlisted finite hidden placement".into());
        }
        if hidden_by_alias.insert(text(row, "alias")?, row).is_some() {
            return Err("duplicate hidden alias".into());
        }
    }
    let libraries = recipe
        .get("library_for_A")
        .map(rows)
        .transpose()?
        .unwrap_or(&[]);
    let mut consumed = BTreeSet::new();
    for (index, alias) in libraries.iter().enumerate() {
        let alias = alias.as_str().ok_or("literal library alias required")?;
        let definition = if let Some(row) = hidden_by_alias.get(alias) {
            consumed.insert(alias);
            permanent(
                text(row, "kind")?,
                type_set(&row["subtypes"], &vocabulary, &other)?,
            )?
        } else {
            permanent("Creature2/2", BTreeSet::new())?
        };
        add(
            alias,
            200 + index as u64,
            SeatId(0),
            ObjectDestination::LibraryBottom,
            definition,
        )?;
    }
    if consumed.len() != hidden_by_alias.len() {
        return Err(
            "hidden cards must occur exactly once in complete explicit library order".into(),
        );
    }
    Ok(PublicInput {
        game: GameId(1),
        context: context(&vocabulary),
        seed,
        aliases,
    })
}

use super::{current_capture, projection, resolution_contract_cases as calls};
use spellwake_engine::{
    game::{GameCommand, GameError, GameInput, GameState, NativeCandidate, RuleFrame},
    stack_window::StackEntryId,
};
use std::cell::Cell;
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublicNative {
    pub input: PublicInput,
    pub initialization: Result<NativeCandidate, GameError>,
    pub attempts: Vec<(GameState, GameInput, Result<NativeCandidate, GameError>)>,
    pub state: GameState,
    pub entry: StackEntryId,
    pub lower_entry: Option<StackEntryId>,
}
pub fn game_result(
    result: &Result<NativeCandidate, GameError>,
    source: Option<&GameState>,
) -> Value {
    match result {
        Ok(value) => {
            serde_json::json!({"tag":"Ok","candidate":projection::candidate(value,source,&packet::AliasContext::default()).expect("actual native candidate projection")})
        }
        Err(error) => projection::game_error(error),
    }
}
pub fn advance(
    source: &GameState,
    input: &GameInput,
    adopt: bool,
) -> (Result<NativeCandidate, GameError>, calls::CallRecord) {
    let first = Cell::new(true);
    calls::borrowed_call(
        "GameState::advance",
        source,
        input,
        |source, input| {
            if first.replace(false) {
                current_capture::advance_with_adoption(source, input.clone(), adopt)
            } else {
                current_capture::without_capture(|| source.advance(input.clone()))
            }
        },
        |source| {
            projection::state(source, &packet::AliasContext::default())
                .expect("actual native source projection")
        },
        projection::input,
        |result| game_result(result, Some(source)),
    )
}
pub fn public_pending(
    recipe: &Value,
    vocabulary: Arc<CreatureTypeVocabulary>,
) -> Result<(Vec<calls::CallRecord>, PublicNative), String> {
    let input = public_input(recipe, vocabulary)?;
    if recipe["max_initialize_calls"] != 1
        || !matches!(recipe["max_root_inputs"].as_u64(), Some(3..=5))
    {
        return Err("finite public root call budget absent".into());
    }
    let first = Cell::new(true);
    let (initialization, initial_call) = calls::input_call(
        "GameState::initialize",
        &input,
        |input| {
            if first.replace(false) {
                current_capture::initialize(input.game, input.context, input.seed)
            } else {
                current_capture::without_capture(|| {
                    GameState::initialize(input.game, input.context, input.seed)
                })
            }
        },
        |input| serde_json::json!({"game":input.game.0.to_string(),"context":projection::context(&input.context),"seed":projection::game_seed_input(&input.seed)}),
        |result| game_result(result, None),
    );
    let mut state = initialization
        .as_ref()
        .map_err(|error| format!("literal public fixture initialization rejected: {error:?}"))?
        .state()
        .clone();
    let mut records = vec![initial_call];
    let mut attempts = vec![];
    let mut entry = None;
    let mut lower_entry = None;
    let mut sequence = Vec::new();
    if recipe["lower_stack_spell"] == true {
        sequence.push(Some("LOW"));
    }
    sequence.extend([Some("S"), None, None]);
    for (index, alias) in sequence.into_iter().enumerate() {
        let actor = if alias.is_none()
            && index
                == if recipe["lower_stack_spell"] == true {
                    3
                } else {
                    2
                } {
            SeatId(1)
        } else {
            SeatId(0)
        };
        let command = if let Some(alias) = alias {
            let card = *input
                .aliases
                .get(alias)
                .ok_or("literal alias absent from constructed typed seed")?;
            let spell = state
                .execution()
                .window()
                .cards()
                .current_identity(card)
                .ok_or("actual current hand incarnation missing")?;
            GameCommand::CastUntargeted {
                spell,
                payment: None,
            }
        } else {
            GameCommand::Pass
        };
        let native_input = GameInput {
            basis: state.basis().clone(),
            actor,
            command,
        };
        let (result, record) = advance(&state, &native_input, true);
        records.push(record);
        attempts.push((state.clone(), native_input, result.clone()));
        let value = result
            .map_err(|error| format!("literal public preparation call rejected: {error:?}"))?;
        if let Some(alias) = alias {
            let receipt = value
                .mana_payment()
                .ok_or("actual cast receipt missing")?
                .source;
            if alias == "S" {
                entry = Some(receipt);
            } else {
                lower_entry = Some(receipt);
            }
        }
        state = value.into_state();
    }
    let entry = entry.ok_or("actual S cast entry missing")?;
    let Some(RuleFrame::PromptChoice(frame)) = state.pending_decision().map(|value| value.frame())
    else {
        return Err("literal public preparation did not reach actual PromptChoice Pending".into());
    };
    if frame.entry != entry
        || frame.instruction != 0
        || frame.chooser != SeatId(0)
        || !matches!(
            frame.domain,
            spellwake_engine::toolbox::ResolvedPromptDomain::CreatureType { .. }
        )
    {
        return Err("actual first frame differs from explicit public template boundary".into());
    }
    Ok((
        records,
        PublicNative {
            input,
            initialization,
            attempts,
            state,
            entry,
            lower_entry,
        },
    ))
}
