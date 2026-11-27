//! Shared literal characteristic trajectories and helpers used by public and private tests.
#![allow(dead_code)]
use super::creature_vocabulary as published_vocabulary;

use spellwake_contracts::creature_types::{CreatureTypeId, CreatureTypeVocabulary};
use spellwake_engine::{
    activated::{ActivatedAbility, ActivatedAbilityId, SourceCost},
    characteristics::{
        CharacteristicBucket, ControllerSelector, EffectInstanceKey, EvaluatedCharacteristics,
        InstructionSubtypeOperand, NumericExpression, NumericPair, ObjectCharacteristics,
        PassBinding, PermanentSelector, PowerToughness, PowerToughnessCounters,
        ResolvedContinuousEffect, ResolvedDuration, ResolvedPart, ResolvedPartSpec,
        ResolvedPartsSpec, StaticAbilityId, StaticContinuousAbility, StaticPart, StaticParts,
        StaticSubtypeOperand, StaticTypeChange, SubtypeOperation, TypeChange, TypeChangeSpec,
        TypeOperation,
    },
    combat::{CombatDecision, CombatRules},
    damage::{DamageEffectCreation, DamageEffectId},
    effects::{DamageDecision, EffectError, SeedCard},
    game::{
        DecisionReply, GameCommand, GameContext, GameError, GameId, GameInput, GameState,
        NativeCandidate, RuleFrame,
    },
    mana::{ManaAbility, ManaAbilityId, ManaCost, ManaOutput, ManaPayment, ManaType},
    object_identity::{CardId, ObjectDestination, ObjectId, ObjectZone},
    occurrence::{
        CharacteristicAppearanceChange, DrawOrigin, EntryCompletion, EventCause, RulesOccurrence,
        RulesRecord, ZoneReason,
    },
    priority::{PriorityPhase, SeatId},
    program::{
        BodyTarget, CardType, CreatureTypeSlot, ExecutableBody, Instruction, PermanentDefinition,
        Program, ProgramKind,
    },
    setup::GameSeed,
    stack_window::{StackEntryId, WindowPhase},
    targets::TargetRef,
    turn::{Step, TurnStart},
};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

pub(crate) const P0: SeatId = SeatId(0);
pub(crate) const P1: SeatId = SeatId(1);
const V_ID: &str = "39721c01eef9fd3f869588b097651bb6115f9cdde5d61bf822d65a8e5435c792";

pub(crate) fn vocabulary() -> Arc<CreatureTypeVocabulary> {
    let path = std::env::var_os("SPELLWAKE_CREATURE_VOCABULARY_INPUT")
        .expect("frozen I recipes require an explicitly bound complete vocabulary input");
    let value = published_vocabulary::load_vocabulary(std::path::Path::new(&path)).unwrap();
    assert_eq!(value.id().to_string(), V_ID);
    value
}

pub(crate) fn pt(power: i64, toughness: i64) -> PowerToughness {
    PowerToughness {
        power: power.into(),
        toughness: toughness.into(),
    }
}

pub(crate) fn subtypes(
    vocabulary: &CreatureTypeVocabulary,
    labels: &[&str],
) -> BTreeSet<CreatureTypeId> {
    labels
        .iter()
        .map(|label| vocabulary.resolve_label(label).unwrap())
        .collect()
}

pub(crate) fn permanent(
    vocabulary: &CreatureTypeVocabulary,
    types: &[CardType],
    printed_pt: Option<(i64, i64)>,
    labels: &[&str],
) -> Program {
    Program::new(
        types.iter().copied().collect(),
        subtypes(vocabulary, labels),
        BTreeSet::new(),
        ProgramKind::Permanent(PermanentDefinition {
            entry_tap: None,
            attachment: None,
            printed_pt: printed_pt.map(|(power, toughness)| pt(power, toughness).into()),
            casting_cost: Some(ManaCost::free()),
        }),
    )
}

pub(crate) fn card(id: u64, program: Program, battlefield: bool) -> SeedCard {
    SeedCard {
        card: CardId(id),
        owner: P0,
        destination: if battlefield {
            ObjectDestination::Battlefield { controller: P0 }
        } else {
            ObjectDestination::Hand
        },
        program,
    }
}

pub(crate) fn spell(id: u64, target: BodyTarget, instructions: Vec<Instruction>) -> SeedCard {
    card(
        id,
        Program::instant(ManaCost::free(), ExecutableBody::new(target, instructions)),
        false,
    )
}

pub(crate) fn effect(parts: Vec<ResolvedPartSpec>, duration: ResolvedDuration) -> Instruction {
    {
        struct _Tmp {
            parts: spellwake_engine::characteristics::ResolvedPartsSpec,
            duration: spellwake_engine::characteristics::ResolvedDuration,
        }
        let _t = _Tmp {
            parts: ResolvedPartsSpec(parts),
            duration,
        };
        spellwake_engine::program::Instruction::ApplyContinuous(Box::new(
            spellwake_engine::toolbox::ApplyContinuousSpec {
                selector: Some(spellwake_engine::toolbox::ObjectQuery::Targets),
                payload: spellwake_engine::toolbox::ContinuousPayload::ResolvedParts(Box::new(
                    _t.parts,
                )),
                duration: _t.duration,
                condition: None,
                one_shot: false,
                as_emblem: None,
                emblem_label: None,
            },
        ))
    }
}

pub(crate) fn type_part(
    types: TypeOperation,
    subtypes: SubtypeOperation<InstructionSubtypeOperand>,
) -> ResolvedPartSpec {
    ResolvedPartSpec::Type(TypeChangeSpec {
        land_subtypes: SubtypeOperation::Keep,
        supertypes: TypeOperation::Keep,
        types,
        subtypes,
    })
}

pub(crate) fn submit(game: &GameState, actor: SeatId, command: GameCommand) -> NativeCandidate {
    let saved = game.clone();
    let input = GameInput {
        basis: game.basis().clone(),
        actor,
        command,
    };
    observe_trajectory(TrajectoryObservation::BeforeAdvance {
        source: game,
        input: &input,
    });
    let result = game.advance(input.clone()).unwrap();
    assert_eq!(game, &saved);
    observe_trajectory(TrajectoryObservation::Advance {
        source: game,
        input: &input,
        result: &result,
    });
    observe_state(result.state());
    result
}

/// Observe the one original fallible call and return its actual result unchanged.
/// With no selected observer these borrowed observations clone no owner or input.
pub(crate) fn observe_advance(
    game: &GameState,
    input: GameInput,
) -> Result<NativeCandidate, GameError> {
    observe_trajectory(TrajectoryObservation::BeforeAdvanceAttempt {
        source: game,
        input: &input,
    });
    let result = game.advance(input);
    observe_trajectory(TrajectoryObservation::AdvanceAttemptResult {
        source: game,
        result: &result,
    });
    result
}

pub(crate) fn assert_priority(game: &GameState, actor: SeatId) {
    assert!(game.pending_decision().is_none());
    assert_eq!(
        game.execution().window().phase(),
        WindowPhase::Priority(PriorityPhase::Ready { holder: actor })
    );
    assert!(game.execution().state_based_losses().is_empty());
}

pub(crate) fn pass_pair(game: &GameState) -> NativeCandidate {
    pass_in_order(game, [P0, P1])
}

pub(crate) fn pass_in_order(game: &GameState, actors: [SeatId; 2]) -> NativeCandidate {
    assert_priority(game, actors[0]);
    let first = submit(game, actors[0], GameCommand::Pass);
    assert_priority(first.state(), actors[1]);
    submit(first.state(), actors[1], GameCommand::Pass)
}

pub(crate) fn world(
    vocabulary: Arc<CreatureTypeVocabulary>,
    cards: Vec<SeedCard>,
    order: Vec<CardId>,
) -> GameState {
    world_with_starting_life(vocabulary, cards, order, 20)
}

pub(crate) fn world_with_starting_life(
    vocabulary: Arc<CreatureTypeVocabulary>,
    cards: Vec<SeedCard>,
    order: Vec<CardId>,
    starting_life: i64,
) -> GameState {
    let context = GameContext::new(
        "characteristics-frozen-I-v1".into(),
        vocabulary.rules_revision().into(),
        "literal-frozen-definitions".into(),
        "characteristics-layers-v2".into(),
        "ordinary-first-turn".into(),
        vocabulary.id(),
        spellwake_contracts::names::NameRules::literal_names().id(),
    )
    .unwrap();
    let game_id = GameId(20261101);
    let seed = GameSeed::with_uniform_life(
        vec![P0, P1],
        P0,
        cards,
        order,
        vec![],
        starting_life.into(),
        CombatRules::UnrestrictedFreeForAll,
        vocabulary,
        std::sync::Arc::new(spellwake_contracts::names::NameRules::literal_names()),
    );
    let start = TurnStart {
        number: 1,
        step: Step::Untap,
    };
    observe_trajectory(TrajectoryObservation::BeforeInitialize {
        game: game_id,
        context: &context,
        seed: &seed,
        start,
    });
    let candidate =
        GameState::initialize_scheduled(game_id, context.clone(), seed.clone(), start).unwrap();
    observe_trajectory(TrajectoryObservation::Initialize {
        game: game_id,
        context: &context,
        seed: &seed,
        start,
        result: &candidate,
    });
    assert!(candidate.draw_journal().events.is_empty());
    observe_state(candidate.state());
    let game = candidate.into_state();
    assert_eq!(game.turn().unwrap().position().step, Step::Upkeep);
    for physical in game.execution().window().cards().iter().filter(|physical| {
        physical.zone() == ObjectZone::Battlefield && physical.controller() == Some(P0)
    }) {
        assert_eq!(
            physical.has_continuous_control_since_turn_start(),
            Some(true)
        );
    }
    let main_candidate = pass_pair(&game);
    assert!(main_candidate.draw_journal().events.is_empty());
    let main = main_candidate.into_state();
    assert_eq!(main.turn().unwrap().position().number, 1);
    assert_eq!(main.turn().unwrap().position().step, Step::PrecombatMain);
    assert_priority(&main, P0);
    assert_eq!(main.execution().life_total(P0), Some(&starting_life.into()));
    assert_eq!(main.execution().life_total(P1), Some(&starting_life.into()));
    for player in [P0, P1] {
        for kind in ManaType::ALL {
            assert_eq!(main.execution().mana(player, kind), 0.into());
        }
    }
    main
}

pub(crate) fn object(game: &GameState, id: u64) -> ObjectId {
    game.execution()
        .window()
        .cards()
        .current_identity(CardId(id))
        .unwrap()
}

pub(crate) fn current(game: &GameState, object: ObjectId) -> ObjectCharacteristics {
    observe_trajectory(TrajectoryObservation::BeforeCurrent {
        state: game,
        object,
    });
    let result = game
        .execution()
        .current_characteristics(object)
        .unwrap()
        .unwrap();
    observe_trajectory(TrajectoryObservation::Current {
        state: game,
        result: &result,
    });
    result
}

pub(crate) fn assert_current(
    game: &GameState,
    vocabulary: &CreatureTypeVocabulary,
    object: ObjectId,
    types: &[CardType],
    labels: &[&str],
    values: Option<(i64, i64)>,
) {
    let actual = current(game, object);
    assert_eq!(actual.current_types, types.iter().copied().collect());
    assert_eq!(
        actual.current_creature_subtypes,
        subtypes(vocabulary, labels)
    );
    assert_eq!(
        actual.power_toughness.map(|value| value.derived),
        values.map(|(power, toughness)| pt(power, toughness))
    );
}

pub(crate) fn cast(game: &GameState, id: u64, target: ObjectId) -> (NativeCandidate, StackEntryId) {
    assert_priority(game, P0);
    let candidate = submit(
        game,
        P0,
        GameCommand::CastPaid {
            spell: object(game, id),
            target: TargetRef::Object(target),
            payment: ManaPayment::default(),
        },
    );
    let entry = candidate
        .state()
        .execution()
        .window()
        .entries()
        .last()
        .unwrap()
        .id();
    let paid = candidate.mana_payment().unwrap();
    assert_eq!(paid.source, entry);
    assert_eq!(paid.cost, ManaCost::free());
    assert!(paid.spend.is_empty());
    (candidate, entry)
}

pub(crate) fn assert_completion(
    candidate: &NativeCandidate,
    entry: StackEntryId,
    expected: EntryCompletion,
) {
    let completions = candidate
        .rules_journal()
        .records
        .iter()
        .filter_map(|record| match record {
            RulesRecord::Event(event) => match &event.cause {
                EventCause::ResolutionCompleted {
                    entry: actual,
                    completion,
                } if *actual == entry => Some(*completion),
                _ => None,
            },
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(completions, vec![expected]);
}

pub(crate) fn resolve(
    game: &GameState,
    id: u64,
    target: ObjectId,
) -> (NativeCandidate, StackEntryId) {
    let (cast, entry) = cast(game, id, target);
    let completed = pass_pair(cast.state());
    assert_priority(completed.state(), P0);
    assert_completion(&completed, entry, EntryCompletion::Resolved);
    (completed, entry)
}

pub(crate) fn created(candidate: &NativeCandidate) -> &ResolvedContinuousEffect {
    let records = candidate
        .rules_journal()
        .records
        .iter()
        .filter_map(|record| match record {
            RulesRecord::Event(event) => Some(&event.occurrences),
            _ => None,
        })
        .flatten()
        .filter_map(|row| match &row.value {
            RulesOccurrence::ContinuousEffectCreated { effect, .. } => Some(effect),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(records.len(), 1);
    records[0]
}

pub(crate) fn reach_end(game: &GameState, actors: [SeatId; 2]) -> GameState {
    assert_eq!(game.turn().unwrap().position().step, Step::PrecombatMain);
    let beginning = pass_in_order(game, actors).into_state();
    assert_eq!(
        beginning.turn().unwrap().position().step,
        Step::BeginningCombat
    );
    let mut attackers = pass_in_order(&beginning, actors).into_state();
    assert_eq!(
        attackers.turn().unwrap().position().step,
        Step::DeclareAttackers
    );
    if let Some(decision) = attackers.pending_decision() {
        assert!(matches!(
            decision.frame(),
            RuleFrame::Combat(CombatDecision::Attackers(_))
        ));
        assert_eq!(decision.chooser(), actors[0]);
        attackers = submit(
            &attackers,
            actors[0],
            GameCommand::Reply {
                decision: decision.id(),
                choice: DecisionReply::DeclareAttackers(vec![]),
            },
        )
        .into_state();
    }
    let end_combat = pass_in_order(&attackers, actors).into_state();
    assert_eq!(end_combat.turn().unwrap().position().step, Step::EndCombat);
    let postcombat = pass_in_order(&end_combat, actors).into_state();
    assert_eq!(
        postcombat.turn().unwrap().position().step,
        Step::PostcombatMain
    );
    let end = pass_in_order(&postcombat, actors).into_state();
    assert_eq!(end.turn().unwrap().position().step, Step::End);
    end
}

pub(crate) fn cleanup(game: &GameState) -> NativeCandidate {
    let end = reach_end(game, [P0, P1]);
    let completed = pass_pair(&end);
    let actions = completed
        .rules_journal()
        .records
        .iter()
        .filter_map(|record| match record {
            RulesRecord::Event(event) => match &event.cause {
                EventCause::CleanupCharacteristics { position } => Some(*position),
                _ => None,
            },
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(actions.len(), 1);
    assert_eq!(actions[0].number, 1);
    assert_eq!(actions[0].step, Step::Cleanup);
    assert_eq!(actions[0].cleanup_iteration, 0);
    assert_eq!(completed.state().turn().unwrap().position().number, 2);
    assert_eq!(
        completed.state().turn().unwrap().position().step,
        Step::Upkeep
    );
    assert!(completed.state().pending_decision().is_none());
    assert!(
        completed
            .state()
            .execution()
            .state_based_losses()
            .is_empty()
    );
    completed
}

pub(crate) fn reply(game: &GameState, actor: SeatId, choice: DecisionReply) -> NativeCandidate {
    let decision = game.pending_decision().unwrap();
    assert_eq!(decision.chooser(), actor);
    submit(
        game,
        actor,
        GameCommand::Reply {
            decision: decision.id(),
            choice,
        },
    )
}

pub(crate) fn library(id: u64, owner: SeatId, v: &CreatureTypeVocabulary) -> SeedCard {
    SeedCard {
        card: CardId(id),
        owner,
        destination: ObjectDestination::LibraryBottom,
        program: permanent(v, &[CardType::Artifact], None, &[]),
    }
}

pub(crate) fn controlled(id: u64, owner: SeatId, controller: SeatId, program: Program) -> SeedCard {
    SeedCard {
        card: CardId(id),
        owner,
        destination: ObjectDestination::Battlefield { controller },
        program,
    }
}

pub(crate) fn all_occurrences(candidate: &NativeCandidate) -> Vec<&RulesOccurrence> {
    candidate
        .rules_journal()
        .records
        .iter()
        .filter_map(|record| match record {
            RulesRecord::Event(event) => Some(&event.occurrences),
            _ => None,
        })
        .flatten()
        .map(|row| &row.value)
        .collect()
}

pub(crate) fn zone_change(
    candidate: &NativeCandidate,
    before: ObjectId,
) -> &spellwake_engine::occurrence::ZoneOccurrence {
    all_occurrences(candidate)
        .into_iter()
        .find_map(|value| match value {
            RulesOccurrence::ZoneChange(change) if change.before.object.id() == before => {
                Some(change.as_ref())
            }
            _ => None,
        })
        .unwrap()
}

pub(crate) fn land_source() -> Program {
    let mut source = Program::land(
        std::collections::BTreeSet::new(),
        vec![ManaAbility {
            id: ManaAbilityId(1),
            output: ManaOutput::Fixed(BTreeMap::from([(ManaType::Blue, 1.into())])),
        }],
    );
    source.activated.push(ActivatedAbility {
        id: ActivatedAbilityId(1),
        mana_cost: ManaCost::free(),
        source_cost: SourceCost::Tap,
        body: ExecutableBody::new(
            BodyTarget::SingleCreature,
            vec![effect(
                vec![ResolvedPartSpec::ModifyPT(pt(1, 1))],
                ResolvedDuration::UntilEndOfTurn,
            )],
        ),
    });
    source
}

pub(crate) fn animate_land() -> Instruction {
    effect(
        vec![
            type_part(
                TypeOperation::Add(BTreeSet::from([CardType::Artifact, CardType::Creature])),
                SubtypeOperation::Keep,
            ),
            ResolvedPartSpec::SetPT(pt(2, 2)),
        ],
        ResolvedDuration::UntilEndOfTurn,
    )
}

pub(crate) fn activate(
    game: &GameState,
    source: ObjectId,
    target: ObjectId,
    mana: bool,
) -> NativeCandidate {
    submit(
        game,
        P0,
        if mana {
            GameCommand::ActivateMana {
                source,
                ability: spellwake_engine::mana::ManaAbilityRef::Printed(ManaAbilityId(1)),
            }
        } else {
            GameCommand::ActivateAbility {
                source,
                ability: ActivatedAbilityId(1),
                target: TargetRef::Object(target),
                payment: Some(ManaPayment::default()),
            }
        },
    )
}

pub(crate) fn assert_land_roster(game: &GameState, source: ObjectId) {
    let roster = current(game, source).abilities;
    assert_eq!(roster.mana, vec![ManaAbilityId(1)]);
    assert_eq!(roster.activated, vec![ActivatedAbilityId(1)]);
    assert!(roster.triggered.is_empty());
    assert_eq!(roster.continuous, None);
}

pub(crate) fn new_tap(mana: bool) {
    let v = vocabulary();
    let game = world(
        v.clone(),
        vec![
            card(1, land_source(), false),
            card(
                2,
                permanent(&v, &[CardType::Creature], Some((3, 5)), &["Elf"]),
                true,
            ),
            spell(3, BodyTarget::SinglePermanent, vec![animate_land()]),
            spell(4, BodyTarget::SinglePermanent, vec![animate_land()]),
            library(5, P0, &v),
            library(6, P1, &v),
        ],
        vec![CardId(2)],
    );
    let p0_draw = object(&game, 5);
    let p1_draw = object(&game, 6);
    let played = submit(
        &game,
        P0,
        GameCommand::PlayLand {
            land: object(&game, 1),
        },
    );
    checkpoint("after_real_land_entry", played.state());
    let s = object(played.state(), 1);
    let r = object(played.state(), 2);
    let epoch = played
        .state()
        .execution()
        .window()
        .cards()
        .get(s)
        .unwrap()
        .control_epoch();
    let (animated, _) = resolve(played.state(), 3, s);
    checkpoint("after_animate1", animated.state());
    // CL-TAP-NEW-MANA/a0; CL-TAP-NEW-PUMP/a0
    assert_current(
        animated.state(),
        &v,
        s,
        &[CardType::Land, CardType::Artifact, CardType::Creature],
        &[],
        Some((2, 2)),
    );
    checkpoint("after_animate1_roster", animated.state());
    assert_land_roster(animated.state(), s);
    assert_eq!(
        animated
            .state()
            .execution()
            .window()
            .cards()
            .get(s)
            .unwrap()
            .has_continuous_control_since_turn_start(),
        Some(false)
    );
    checkpoint("before_first_actual_activation_attempt", animated.state());
    // CL-TAP-NEW-MANA/a1; CL-TAP-NEW-PUMP/a1
    let saved = animated.state().clone();
    let command = if mana {
        GameCommand::ActivateMana {
            source: s,
            ability: spellwake_engine::mana::ManaAbilityRef::Printed(ManaAbilityId(1)),
        }
    } else {
        GameCommand::ActivateAbility {
            source: s,
            ability: ActivatedAbilityId(1),
            target: TargetRef::Object(r),
            payment: Some(ManaPayment::default()),
        }
    };
    assert_eq!(
        observe_advance(
            animated.state(),
            GameInput {
                basis: animated.state().basis().clone(),
                actor: P0,
                command
            }
        ),
        Err(GameError::Effect(EffectError::CreatureTapRestricted))
    );
    assert_eq!(animated.state(), &saved);
    checkpoint("first_actual_activation_attempt", animated.state());
    // CL-TAP-NEW-MANA/a2; CL-TAP-NEW-PUMP/a2
    assert_eq!(
        animated
            .state()
            .execution()
            .window()
            .cards()
            .get(s)
            .unwrap()
            .tapped(),
        Some(false)
    );
    assert_eq!(
        animated.state().execution().mana(P0, ManaType::Blue),
        0.into()
    );
    assert_eq!(
        current(animated.state(), r)
            .power_toughness
            .unwrap()
            .derived,
        pt(3, 5)
    );
    assert!(animated.state().execution().window().entries().is_empty());
    checkpoint("after_first_rejected_activation_epoch", animated.state());
    // CL-TAP-NEW-MANA/a3; CL-TAP-NEW-PUMP/a3
    assert_eq!(
        animated
            .state()
            .execution()
            .window()
            .cards()
            .get(s)
            .unwrap()
            .control_epoch(),
        epoch
    );
    let first_cleanup = cleanup(animated.state());
    checkpoint("after_first_cleanup", first_cleanup.state());
    // CL-TAP-NEW-MANA/a4; CL-TAP-NEW-PUMP/a4
    assert_current(first_cleanup.state(), &v, s, &[CardType::Land], &[], None);
    checkpoint("after_first_cleanup_roster", first_cleanup.state());
    assert_land_roster(first_cleanup.state(), s);
    assert_eq!(
        first_cleanup
            .state()
            .execution()
            .window()
            .cards()
            .get(s)
            .unwrap()
            .has_continuous_control_since_turn_start(),
        Some(false)
    );
    assert_eq!(
        first_cleanup
            .state()
            .execution()
            .window()
            .cards()
            .get(s)
            .unwrap()
            .tapped(),
        Some(false)
    );
    assert_priority(first_cleanup.state(), P1);
    let offered = submit(first_cleanup.state(), P1, GameCommand::Pass);
    // CL-TAP-NEW-MANA/a5; CL-TAP-NEW-PUMP/a5
    let postcleanup = activate(offered.state(), s, r, mana);
    checkpoint("postcleanup_activation_accepted", postcleanup.state());
    let postcleanup = if mana {
        postcleanup
    } else {
        pass_pair(postcleanup.state())
    };
    assert_eq!(
        postcleanup
            .state()
            .execution()
            .window()
            .cards()
            .get(s)
            .unwrap()
            .tapped(),
        Some(true)
    );
    assert_eq!(
        postcleanup
            .state()
            .execution()
            .window()
            .cards()
            .get(s)
            .unwrap()
            .has_continuous_control_since_turn_start(),
        Some(false)
    );
    checkpoint("postcleanup_actual_activation", postcleanup.state());
    let p1draw = pass_in_order(postcleanup.state(), if mana { [P0, P1] } else { [P1, P0] });
    assert_eq!(p1draw.state().turn().unwrap().position().step, Step::Draw);
    assert_eq!(p1draw.state().turn().unwrap().position().number, 2);
    checkpoint("after_P1_scheduled_draw", p1draw.state());
    // CL-TAP-NEW-MANA/a8; CL-TAP-NEW-PUMP/a8
    let first_draw = zone_change(&p1draw, p1_draw);
    assert_ne!(first_draw.after.object.id(), p1_draw);
    assert_eq!(first_draw.after.object.zone(), ObjectZone::Hand(P1));
    assert_eq!(first_draw.after.object.owner(), P1);
    assert_eq!(first_draw.after.object.controller(), None);
    assert!(
        p1draw
            .state()
            .execution()
            .window()
            .cards()
            .library(P1)
            .is_empty()
    );
    let p1main = pass_in_order(p1draw.state(), [P1, P0]);
    let end = reach_end(p1main.state(), [P1, P0]);
    let p0upkeep = pass_in_order(&end, [P1, P0]);
    assert_eq!(p0upkeep.state().turn().unwrap().position().number, 3);
    assert_eq!(
        p0upkeep.state().turn().unwrap().position().step,
        Step::Upkeep
    );
    checkpoint("actual_P0_turn3_before_animate2", p0upkeep.state());
    // CL-TAP-NEW-MANA/a6; CL-TAP-NEW-PUMP/a6
    assert_eq!(
        p0upkeep
            .state()
            .execution()
            .window()
            .cards()
            .get(s)
            .unwrap()
            .tapped(),
        Some(false)
    );
    assert_eq!(
        p0upkeep
            .state()
            .execution()
            .window()
            .cards()
            .get(s)
            .unwrap()
            .has_continuous_control_since_turn_start(),
        Some(true)
    );
    let p0draw = pass_pair(p0upkeep.state());
    assert_eq!(p0draw.state().turn().unwrap().position().step, Step::Draw);
    checkpoint("after_P0_scheduled_draw", p0draw.state());
    // CL-TAP-NEW-MANA/a9; CL-TAP-NEW-PUMP/a9
    let second_draw = zone_change(&p0draw, p0_draw);
    assert_ne!(second_draw.after.object.id(), p0_draw);
    assert_eq!(second_draw.after.object.zone(), ObjectZone::Hand(P0));
    assert_eq!(second_draw.after.object.owner(), P0);
    assert_eq!(second_draw.after.object.controller(), None);
    assert!(
        p0draw
            .state()
            .execution()
            .window()
            .cards()
            .library(P0)
            .is_empty()
    );
    let p0main = pass_pair(p0draw.state());
    let (animated2, _) = resolve(p0main.state(), 4, s);
    checkpoint("after_animate2_before_activation", animated2.state());
    // CL-TAP-NEW-MANA/a7; CL-TAP-NEW-PUMP/a7
    assert_current(
        animated2.state(),
        &v,
        s,
        &[CardType::Land, CardType::Artifact, CardType::Creature],
        &[],
        Some((2, 2)),
    );
    let activated2 = activate(animated2.state(), s, r, mana);
    checkpoint("turn3_activation_accepted", activated2.state());
    let done = if mana {
        activated2
    } else {
        pass_pair(activated2.state())
    };
    assert_eq!(
        done.state()
            .execution()
            .window()
            .cards()
            .get(s)
            .unwrap()
            .tapped(),
        Some(true)
    );
    checkpoint("after_animate2_activation", done.state());
    checkpoint("turn3_final", done.state());
    // CL-TAP-NEW-MANA/a10; CL-TAP-NEW-PUMP/a10
    assert_eq!(done.state().turn().unwrap().position().number, 3);
    assert_eq!(done.state().execution().window().active_player(), P0);
    assert_eq!(done.state().outcome(), None);
    for player in [P0, P1] {
        assert_eq!(
            done.state().execution().life_total(player),
            Some(&20.into())
        );
    }
    let hands = done
        .state()
        .execution()
        .window()
        .cards()
        .iter()
        .filter(|card| matches!(card.zone(), ObjectZone::Hand(_)))
        .map(|card| (card.owner(), card.card_id().expect("physical fixture card")))
        .collect::<BTreeSet<_>>();
    assert_eq!(hands, BTreeSet::from([(P0, CardId(5)), (P1, CardId(6))]));
    // CL-TAP-NEW-MANA/a11; CL-TAP-NEW-PUMP/a11
    assert!(done.state().execution().state_based_losses().is_empty());
    assert!(
        p1draw
            .draw_journal()
            .events
            .iter()
            .all(|event| matches!(event, spellwake_engine::draw::DrawEvent::Drawn { .. }))
    );
    assert!(
        p0draw
            .draw_journal()
            .events
            .iter()
            .all(|event| matches!(event, spellwake_engine::draw::DrawEvent::Drawn { .. }))
    );
    assert_eq!(p1draw.draw_journal().events.len(), 1);
    assert_eq!(p0draw.draw_journal().events.len(), 1);
    for (candidate, player, number) in [(&p1draw, P1, 2), (&p0draw, P0, 3)] {
        let origins = candidate
            .rules_journal()
            .records
            .iter()
            .filter_map(|record| match record {
                RulesRecord::Event(event) => match event.cause {
                    EventCause::Draw {
                        player,
                        origin: DrawOrigin::Turn { position },
                    } => Some((player, position.number, position.step)),
                    _ => None,
                },
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(origins, vec![(player, number, Step::Draw)]);
    }
    // Failed-draw private memory is not exposed; actual successful draw receipts and final no-loss remain observable.
}

pub(crate) fn cleanup_record(
    candidate: &NativeCandidate,
) -> (
    &Vec<spellwake_engine::characteristics::ContinuousEffectId>,
    &Vec<spellwake_engine::occurrence::MarkedDamageRemoval>,
    &Vec<CharacteristicAppearanceChange>,
) {
    let records = all_occurrences(candidate)
        .into_iter()
        .filter_map(|value| match value {
            RulesOccurrence::CleanupCharacteristics {
                expired_effects,
                marks_removed,
                appearances,
            } => Some((expired_effects, marks_removed, appearances)),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(records.len(), 1);
    records[0]
}

pub(crate) fn elf_count(game: &GameState, v: &CreatureTypeVocabulary) -> usize {
    let elf = v.resolve_label("Elf").unwrap();
    let evaluated = game.execution().evaluated_characteristics().unwrap();
    game.execution()
        .window()
        .cards()
        .iter()
        .filter(|physical| {
            physical.zone() == ObjectZone::Battlefield && physical.controller() == Some(P0)
        })
        .filter(|physical| {
            evaluated
                .get(physical.id())
                .unwrap()
                .current_creature_subtypes
                .contains(&elf)
        })
        .count()
}

pub(crate) fn type_choice_body() -> Instruction {
    Instruction::PromptChoice(Box::new(
        spellwake_engine::toolbox::PromptChoiceSpec::choose_creature_type(
            CreatureTypeSlot::new(0),
            BTreeSet::new(),
        ),
    ))
}

pub(crate) fn chosen_draw() -> Instruction {
    spellwake_engine::program::Instruction::DrawCards(Box::new(
        spellwake_engine::toolbox::DrawCardsSpec::simple(
            spellwake_engine::toolbox::PlayerSelector::Resolving(
                spellwake_engine::program::ResolvingPlayer::Controller,
            ),
            spellwake_engine::program::ValueExpr::count_battlefield(
                spellwake_engine::program::ObjectFilter::any()
                    .controlled()
                    .of_chosen_slot(CreatureTypeSlot::new(0)),
            ),
        ),
    ))
}

pub(crate) fn assert_type_frame(game: &GameState, entry: StackEntryId) {
    let RuleFrame::PromptChoice(frame) = game.pending_decision().unwrap().frame() else {
        panic!("exact frozen q0 choice")
    };
    assert_eq!(frame.entry, entry);
    assert_eq!(frame.chooser, P0);
    assert!(matches!(
        frame.domain,
        spellwake_engine::toolbox::ResolvedPromptDomain::CreatureType { .. }
    ));
    assert!(
        game.execution()
            .resolution_state()
            .unwrap()
            .locals()
            .is_empty()
    );
}

pub(crate) fn cleanup_simultaneous() {
    let v = vocabulary();
    let mut cards = vec![card(
        1,
        permanent(&v, &[CardType::Creature], Some((2, 2)), &[]),
        true,
    )];
    cards
        .extend((2..=9).map(|id| card(id, permanent(&v, &[CardType::Artifact], None, &[]), false)));
    cards.extend([
        spell(
            10,
            BodyTarget::SinglePermanent,
            vec![effect(
                vec![ResolvedPartSpec::ModifyPT(pt(0, 2))],
                ResolvedDuration::UntilEndOfTurn,
            )],
        ),
        spell(
            11,
            BodyTarget::SinglePermanent,
            vec![effect(
                vec![type_part(
                    TypeOperation::Add(BTreeSet::from([CardType::Artifact])),
                    SubtypeOperation::Keep,
                )],
                ResolvedDuration::UntilEndOfTurn,
            )],
        ),
        spell(
            12,
            BodyTarget::SingleCreature,
            vec![spellwake_engine::program::Instruction::DealDamage(
                Box::new(spellwake_engine::toolbox::DealDamageSpec {
                    source: spellwake_engine::toolbox::DamageSourceQuery::EntrySource,
                    recipients: spellwake_engine::toolbox::DamageTargetQuery::Targets,
                    amount: spellwake_engine::program::DamageAmountSpec::Expr(
                        spellwake_engine::program::ValueExpr::Literal(2.into()),
                    ),
                    exile_if_dies: false,
                }),
            )],
        ),
    ]);
    let game = world(v.clone(), cards, vec![CardId(1)]);
    let x = object(&game, 1);
    let d8 = object(&game, 9);
    let (boost, _) = resolve(&game, 10, x);
    let boost_id = created(&boost).id;
    let (typed, _) = resolve(boost.state(), 11, x);
    let type_id = created(&typed).id;
    let (marked, _) = resolve(typed.state(), 12, x);
    let end = reach_end(marked.state(), [P0, P1]);
    let discard = pass_pair(&end);
    checkpoint("before_cleanup_discard", discard.state());
    // CL-CLEANUP-SIMULTANEOUS/a0
    assert_eq!(
        current(discard.state(), x).power_toughness.unwrap().derived,
        pt(2, 4)
    );
    assert_eq!(
        discard.state().execution().battlefield_marked_damage(x),
        Some(2.into())
    );
    let decision = discard.state().pending_decision().unwrap();
    let RuleFrame::Checks(spellwake_engine::checks::CheckDecision::CleanupDiscard(frame)) =
        decision.frame()
    else {
        panic!("exact excess-hand discard")
    };
    assert_eq!(frame.required(), 1);
    assert_eq!(frame.domain().len(), 8);
    assert!(frame.domain().contains(&d8));
    let completed = reply(discard.state(), P0, DecisionReply::Discard(vec![d8]));
    checkpoint("after_cleanup", completed.state());
    let group = cleanup_record(&completed);
    // CL-CLEANUP-SIMULTANEOUS/a1
    assert_eq!(
        completed
            .state()
            .execution()
            .window()
            .cards()
            .iter()
            .filter(|card| card.zone() == ObjectZone::Hand(P0))
            .count(),
        7
    );
    let discarded = zone_change(&completed, d8);
    assert_eq!(discarded.after.object.zone(), ObjectZone::Graveyard(P0));
    // CL-CLEANUP-SIMULTANEOUS/a2
    let after = &group
        .2
        .iter()
        .find(|change| change.object == x)
        .unwrap()
        .after;
    assert_eq!(after.types, BTreeSet::from([CardType::Creature]));
    assert_eq!(
        after.current_power_toughness.as_ref().unwrap().clone(),
        pt(2, 2)
    );
    assert_eq!(after.battlefield_marked_damage, Some(0.into()));
    assert_eq!(after.object.zone(), ObjectZone::Battlefield);
    // CL-CLEANUP-SIMULTANEOUS/a3
    let cuts = completed
        .rules_journal()
        .records
        .iter()
        .filter_map(|record| match record {
            RulesRecord::Event(event) => match event.cause {
                EventCause::CleanupDiscard { .. } => Some("discard"),
                EventCause::CleanupCharacteristics { .. } => Some("combined"),
                _ => None,
            },
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(cuts, vec!["discard", "combined"]);
    assert!(completed.state().pending_decision().is_none());
    // CL-CLEANUP-SIMULTANEOUS/a4
    assert_eq!(object(completed.state(), 1), x);
    assert_eq!(
        completed
            .state()
            .execution()
            .window()
            .cards()
            .get(x)
            .unwrap()
            .zone(),
        ObjectZone::Battlefield
    );
    assert!(
        !completed
            .check_journal()
            .moves
            .iter()
            .any(|movement| movement.before.id() == x)
    );
    // CL-CLEANUP-SIMULTANEOUS/a5
    assert_eq!(*group.0, vec![boost_id, type_id]);
}

pub(crate) fn chosen_count_draw_cuts() {
    let v = vocabulary();
    let body = vec![
        type_choice_body(),
        effect(
            vec![type_part(
                TypeOperation::Keep,
                SubtypeOperation::Set(InstructionSubtypeOperand::Chosen(CreatureTypeSlot::new(0))),
            )],
            ResolvedDuration::UntilEndOfTurn,
        ),
        chosen_draw(),
        effect(
            vec![type_part(
                TypeOperation::Keep,
                SubtypeOperation::Set(InstructionSubtypeOperand::Literal(subtypes(
                    &v,
                    &["Goblin"],
                ))),
            )],
            ResolvedDuration::NoStatedDuration,
        ),
        chosen_draw(),
    ];
    let game = world(
        v.clone(),
        vec![
            card(
                1,
                permanent(&v, &[CardType::Creature], Some((2, 3)), &["Elf"]),
                true,
            ),
            card(
                2,
                permanent(&v, &[CardType::Creature], Some((1, 4)), &["Goblin"]),
                true,
            ),
            controlled(
                3,
                P1,
                P1,
                permanent(&v, &[CardType::Creature], Some((3, 3)), &["Elf"]),
            ),
            library(4, P0, &v),
            library(5, P0, &v),
            library(6, P0, &v),
            spell(7, BodyTarget::SingleCreature, body),
        ],
        vec![CardId(1), CardId(2), CardId(3)],
    );
    let y = object(&game, 2);
    let d = [object(&game, 4), object(&game, 5), object(&game, 6)];
    assert_eq!(
        game.execution()
            .window()
            .cards()
            .get(object(&game, 3))
            .unwrap()
            .has_continuous_control_since_turn_start(),
        Some(false)
    );
    let (cast, entry) = cast(&game, 7, y);
    let pending = pass_pair(cast.state());
    assert_type_frame(pending.state(), entry);
    // CL-CHOSEN-COUNT-DRAW-CUTS/a0
    assert_eq!(elf_count(pending.state(), &v), 1);
    let retained = pending.state().clone();
    let completed = reply(
        pending.state(),
        P0,
        DecisionReply::PromptChoice(spellwake_engine::toolbox::PromptChoiceReply::CreatureType(
            v.resolve_label("Elf").unwrap(),
        )),
    );
    let occurrences = all_occurrences(&completed);
    let changes = occurrences
        .iter()
        .filter_map(|value| match value {
            RulesOccurrence::ContinuousEffectCreated { appearances, .. } => Some(&appearances[0]),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(changes.len(), 2);
    // CL-CHOSEN-COUNT-DRAW-CUTS/a1
    assert_eq!(changes[0].after.creature_subtypes, subtypes(&v, &["Elf"]));
    let draw_events = completed
        .rules_journal()
        .records
        .iter()
        .filter_map(|record| match record {
            RulesRecord::Event(event) => match event.cause {
                EventCause::Draw {
                    origin:
                        DrawOrigin::Instruction {
                            entry: actual,
                            instruction,
                        },
                    ..
                } if actual == entry => Some((instruction, event)),
                _ => None,
            },
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        draw_events
            .iter()
            .map(|(instruction, _)| *instruction)
            .collect::<Vec<_>>(),
        vec![2, 2, 4]
    );
    // CL-CHOSEN-COUNT-DRAW-CUTS/a2
    for drawn in &d[..2] {
        let change = zone_change(&completed, *drawn);
        assert_eq!(change.before.object.zone(), ObjectZone::Library(P0));
        assert_eq!(change.after.object.zone(), ObjectZone::Hand(P0));
        assert_ne!(change.before.object.id(), change.after.object.id());
    }
    // CL-CHOSEN-COUNT-DRAW-CUTS/a3
    assert_eq!(
        changes[1].after.creature_subtypes,
        subtypes(&v, &["Goblin"])
    );
    assert_eq!(elf_count(completed.state(), &v), 1);
    // CL-CHOSEN-COUNT-DRAW-CUTS/a4
    assert_eq!(
        zone_change(&completed, d[2]).after.object.zone(),
        ObjectZone::Hand(P0)
    );
    // CL-CHOSEN-COUNT-DRAW-CUTS/a5
    assert_eq!(
        retained.execution().window().cards().library(P0),
        d.as_slice()
    );
    assert_eq!(completed.draw_journal().events.len(), 3);
    assert_eq!(
        completed
            .draw_journal()
            .events
            .iter()
            .map(|event| match event {
                spellwake_engine::draw::DrawEvent::Drawn { before, .. } =>
                    before.card_id().expect("physical fixture card"),
                _ => panic!("no failed draw in the frozen body"),
            })
            .collect::<Vec<_>>(),
        vec![CardId(4), CardId(5), CardId(6)]
    );
    assert!(
        completed
            .state()
            .execution()
            .window()
            .cards()
            .library(P0)
            .is_empty()
    );
    // Whole-library intermediate overlays are not present in the public journal; only each actual moved card is captured.
    // CL-CHOSEN-COUNT-DRAW-CUTS/a6
    assert_current(
        completed.state(),
        &v,
        y,
        &[CardType::Creature],
        &["Goblin"],
        Some((1, 4)),
    );
    assert_eq!(
        current(completed.state(), y).defined.creature_subtypes,
        subtypes(&v, &["Goblin"])
    );
    // CL-CHOSEN-COUNT-DRAW-CUTS/a7
    assert_eq!(elf_count(&retained, &v), 1);
    assert_eq!(elf_count(completed.state(), &v), 1);
    let chosen = occurrences
        .iter()
        .filter_map(|value| match value {
            RulesOccurrence::CreatureTypeChosen {
                entry: actual,
                slot,
                value,
                ..
            } => Some((*actual, *slot, *value)),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        chosen,
        vec![(
            entry,
            CreatureTypeSlot::new(0),
            v.resolve_label("Elf").unwrap()
        )]
    );
    // The middle count 2 is the first actual DrawController request (two instruction-2 movements), not a retained full-world snapshot.
}

thread_local! {
    static STATE_OBSERVER: std::cell::Cell<Option<fn(&GameState)>> = const { std::cell::Cell::new(None) };
}

/// Observe each actual accepted return on the shared ordinary route, without changing it.
pub(crate) fn with_state_observer(observe: fn(&GameState), run: impl FnOnce()) {
    struct Reset;
    impl Drop for Reset {
        fn drop(&mut self) {
            STATE_OBSERVER.with(|slot| slot.set(None));
        }
    }
    STATE_OBSERVER.with(|slot| {
        assert!(slot.get().is_none(), "nested trajectory observer");
        slot.set(Some(observe));
    });
    let _reset = Reset;
    run();
}

fn observe_state(state: &GameState) {
    STATE_OBSERVER.with(|slot| {
        if let Some(observe) = slot.get() {
            observe(state);
        }
    });
}

pub(crate) fn add_creature(duration: ResolvedDuration) -> Instruction {
    effect(
        vec![type_part(
            TypeOperation::Add(BTreeSet::from([CardType::Creature])),
            SubtypeOperation::Keep,
        )],
        duration,
    )
}

pub(crate) fn remove_creature(duration: ResolvedDuration) -> Instruction {
    effect(
        vec![type_part(
            TypeOperation::Remove(BTreeSet::from([CardType::Creature])),
            SubtypeOperation::Keep,
        )],
        duration,
    )
}

pub(crate) fn type_restore() {
    let v = vocabulary();
    let game = world(
        v.clone(),
        vec![
            card(
                1,
                permanent(&v, &[CardType::Creature], Some((2, 3)), &["Elf"]),
                true,
            ),
            spell(
                2,
                BodyTarget::SingleCreature,
                vec![remove_creature(ResolvedDuration::UntilEndOfTurn)],
            ),
            spell(
                3,
                BodyTarget::SinglePermanent,
                vec![add_creature(ResolvedDuration::NoStatedDuration)],
            ),
        ],
        vec![CardId(1)],
    );
    let x = object(&game, 1);
    let (removed, _) = resolve(&game, 2, x);
    checkpoint("after_remove_creature", removed.state());
    let removal = created(&removed).id;
    let game = removed.into_state();
    // CL-TYPE-RESTORE/a0
    assert_current(&game, &v, x, &[], &[], None);
    assert_eq!(current(&game, x).defined.printed_pt, Some(pt(2, 3).into()));
    let (added, _) = resolve(&game, 3, x);
    checkpoint("after_add_creature", added.state());
    let addition = created(&added).id;
    let game = added.into_state();
    // CL-TYPE-RESTORE/a1
    assert_current(&game, &v, x, &[CardType::Creature], &[], Some((2, 3)));
    let completed = cleanup(&game);
    checkpoint("after_cleanup", completed.state());
    // CL-TYPE-RESTORE/a2
    assert_current(
        completed.state(),
        &v,
        x,
        &[CardType::Creature],
        &["Elf"],
        Some((2, 3)),
    );
    assert_eq!(
        completed.state().execution().battlefield_marked_damage(x),
        Some(0.into())
    );
    // CL-TYPE-RESTORE/a3
    assert!(
        completed
            .state()
            .execution()
            .continuous_effect(removal)
            .is_none()
    );
    assert_eq!(
        completed
            .state()
            .execution()
            .continuous_effect(addition)
            .unwrap()
            .duration,
        ResolvedDuration::NoStatedDuration
    );
}

pub(crate) fn tap_old(mana: bool) {
    let v = vocabulary();
    let game = world(
        v.clone(),
        vec![
            card(1, land_source(), true),
            card(
                2,
                permanent(&v, &[CardType::Creature], Some((3, 5)), &["Elf"]),
                true,
            ),
            spell(3, BodyTarget::SinglePermanent, vec![animate_land()]),
        ],
        vec![CardId(1), CardId(2)],
    );
    let s = object(&game, 1);
    let r = object(&game, 2);
    let epoch = game
        .execution()
        .window()
        .cards()
        .get(s)
        .unwrap()
        .control_epoch();
    let (animated, _) = resolve(&game, 3, s);
    checkpoint("after_animate", animated.state());
    // CL-TAP-OLD-MANA/a0; CL-TAP-OLD-PUMP/a0
    assert_current(
        animated.state(),
        &v,
        s,
        &[CardType::Land, CardType::Artifact, CardType::Creature],
        &[],
        Some((2, 2)),
    );
    assert_land_roster(animated.state(), s);
    assert_eq!(
        animated
            .state()
            .execution()
            .window()
            .cards()
            .get(s)
            .unwrap()
            .tapped(),
        Some(false)
    );
    assert_eq!(
        animated
            .state()
            .execution()
            .window()
            .cards()
            .get(s)
            .unwrap()
            .has_continuous_control_since_turn_start(),
        Some(true)
    );
    // CL-TAP-OLD-MANA/a1; CL-TAP-OLD-PUMP/a1
    let activated = activate(animated.state(), s, r, mana);
    checkpoint("activation_accepted", activated.state());
    let completed = if mana {
        activated
    } else {
        pass_pair(activated.state())
    };
    checkpoint("after_activation", completed.state());
    // CL-TAP-OLD-MANA/a2; CL-TAP-OLD-PUMP/a2
    assert_eq!(
        completed
            .state()
            .execution()
            .window()
            .cards()
            .get(s)
            .unwrap()
            .tapped(),
        Some(true)
    );
    assert_eq!(
        completed.state().execution().mana(P0, ManaType::Blue),
        if mana { 1.into() } else { 0.into() }
    );
    assert_eq!(
        current(completed.state(), r)
            .power_toughness
            .unwrap()
            .derived,
        if mana { pt(3, 5) } else { pt(4, 6) }
    );
    // CL-TAP-OLD-MANA/a3; CL-TAP-OLD-PUMP/a3
    assert_eq!(
        animated
            .state()
            .execution()
            .window()
            .cards()
            .get(s)
            .unwrap()
            .control_epoch(),
        epoch
    );
    assert_eq!(
        completed
            .state()
            .execution()
            .window()
            .cards()
            .get(s)
            .unwrap()
            .control_epoch(),
        epoch
    );
}

pub(crate) fn latent_damage(shield: bool, multiplier: bool, shield_first: bool) {
    let v = vocabulary();
    let mut body = vec![
        remove_creature(ResolvedDuration::NoStatedDuration),
        effect(
            vec![ResolvedPartSpec::ModifyPT(pt(0, 2))],
            ResolvedDuration::NoStatedDuration,
        ),
    ];
    if shield {
        body.push(Instruction::ApplyContinuous(Box::new(
            spellwake_engine::toolbox::ApplyContinuousSpec {
                selector: None,
                payload: spellwake_engine::toolbox::ContinuousPayload::TargetPreventionShield {
                    amount: 2.into(),
                },
                duration: ResolvedDuration::UntilEndOfTurn,
                condition: None,
                one_shot: true,
                as_emblem: None,
                emblem_label: None,
            },
        )));
    }
    if multiplier {
        body.push(Instruction::ApplyContinuous(Box::new(
            spellwake_engine::toolbox::ApplyContinuousSpec::damage_multiplier(2.into()),
        )));
    }
    let inactive_index = if shield && multiplier { 4 } else { 3 };
    assert_eq!(body.len(), inactive_index);
    body.push(spellwake_engine::program::Instruction::DealDamage(
        Box::new(spellwake_engine::toolbox::DealDamageSpec {
            source: spellwake_engine::toolbox::DamageSourceQuery::EntrySource,
            recipients: spellwake_engine::toolbox::DamageTargetQuery::Targets,
            amount: spellwake_engine::program::DamageAmountSpec::Expr(
                spellwake_engine::program::ValueExpr::Literal(3.into()),
            ),
            exile_if_dies: false,
        }),
    ));
    body.push(add_creature(ResolvedDuration::NoStatedDuration));
    let active_index = if shield && multiplier { 6 } else { 5 };
    assert_eq!(body.len(), active_index);
    body.push(spellwake_engine::program::Instruction::DealDamage(
        Box::new(spellwake_engine::toolbox::DealDamageSpec {
            source: spellwake_engine::toolbox::DamageSourceQuery::EntrySource,
            recipients: spellwake_engine::toolbox::DamageTargetQuery::Targets,
            amount: spellwake_engine::program::DamageAmountSpec::Expr(
                spellwake_engine::program::ValueExpr::Literal(3.into()),
            ),
            exile_if_dies: false,
        }),
    ));
    let game = world(
        v.clone(),
        vec![
            card(
                1,
                permanent(
                    &v,
                    &[CardType::Artifact, CardType::Creature],
                    Some((2, 10)),
                    &[],
                ),
                true,
            ),
            spell(2, BodyTarget::SingleCreature, body),
        ],
        vec![CardId(1)],
    );
    let x = object(&game, 1);
    let (cast, entry) = cast(&game, 2, x);
    let initial = pass_pair(cast.state());
    let mut accepted = vec![initial];
    if shield && multiplier {
        let pending = accepted.last().unwrap().state();
        let RuleFrame::Damage(DamageDecision::Order(frame)) =
            pending.pending_decision().unwrap().frame()
        else {
            panic!("two effects require the frozen application order")
        };
        assert_eq!(frame.chooser(), P0);
        assert_eq!(frame.applications().len(), 2);
        let selected = *frame
            .applications()
            .iter()
            .find(|application| {
                matches!(
                    (application.effect, shield_first),
                    (DamageEffectId::Prevention(_), true) | (DamageEffectId::Multiplier(_), false)
                )
            })
            .unwrap();
        let continued = reply(pending, P0, DecisionReply::DamageOrder(selected));
        accepted.push(continued);
    }
    let completed = accepted.last().unwrap();
    assert_priority(completed.state(), P0);
    assert_completion(completed, entry, EntryCompletion::Resolved);
    let occurrences = accepted
        .iter()
        .flat_map(all_occurrences)
        .collect::<Vec<_>>();
    let reanimation = occurrences
        .iter()
        .find_map(|value| match value {
            RulesOccurrence::ContinuousEffectCreated {
                effect,
                appearances,
            } if effect.parts.0
                == vec![ResolvedPart::Type(TypeChange {
                    land_subtypes: SubtypeOperation::Keep,
                    supertypes: TypeOperation::Keep,
                    types: TypeOperation::Add(BTreeSet::from([CardType::Creature])),
                    subtypes: SubtypeOperation::Keep,
                })] =>
            {
                Some(&appearances[0])
            }
            _ => None,
        })
        .unwrap();
    // CL-DAMAGE-SHIELD/a0; CL-DAMAGE-MULTIPLIER/a0; CL-DAMAGE-SHIELD-FIRST/a0; CL-DAMAGE-MULTIPLIER-FIRST/a0
    assert_eq!(
        reanimation.before.types,
        BTreeSet::from([CardType::Artifact])
    );
    assert!(reanimation.before.current_power_toughness.is_none());
    assert_eq!(reanimation.before.battlefield_marked_damage, Some(0.into()));
    // CL-DAMAGE-SHIELD/a1; CL-DAMAGE-MULTIPLIER/a1; CL-DAMAGE-SHIELD-FIRST/a1; CL-DAMAGE-MULTIPLIER-FIRST/a1
    let journals = accepted
        .iter()
        .map(|candidate| candidate.damage_journal())
        .collect::<Vec<_>>();
    let batches = journals
        .iter()
        .flat_map(|journal| &journal.batches)
        .collect::<Vec<_>>();
    assert_eq!(batches.len(), 1);
    assert_eq!(
        batches[0].origin,
        spellwake_engine::damage::DamageOrigin::Resolution {
            entry,
            instruction: active_index
        }
    );
    assert_ne!(active_index, inactive_index);
    let births = journals
        .iter()
        .flat_map(|journal| &journal.created)
        .collect::<Vec<_>>();
    assert_eq!(births.len(), usize::from(shield) + usize::from(multiplier));
    if shield {
        let birth = births
            .iter()
            .find_map(|birth| match birth {
                DamageEffectCreation::Prevention { shield, .. } => Some(shield),
                _ => None,
            })
            .unwrap();
        assert_eq!(birth.remaining, 2.into());
        let before = occurrences
            .iter()
            .find_map(|value| match value {
                RulesOccurrence::DamagePrevented {
                    application,
                    shield_before,
                    ..
                } if application.shield == birth.id => Some(shield_before),
                _ => None,
            })
            .unwrap();
        assert_eq!(before, &2.into());
    }
    if multiplier {
        let birth = births
            .iter()
            .find_map(|birth| match birth {
                DamageEffectCreation::Multiplier { multiplier, .. } => Some(multiplier),
                _ => None,
            })
            .unwrap();
        assert_eq!(birth.factor, 2.into());
        assert_eq!(
            completed
                .state()
                .execution()
                .damage_multiplier(birth.id)
                .unwrap(),
            birth
        );
    }
    let expected = match (shield, multiplier, shield_first) {
        (true, false, _) => 1,
        (false, true, _) => 6,
        (true, true, true) => 2,
        (true, true, false) => 4,
        _ => unreachable!(),
    };
    checkpoint("after_body", completed.state());
    // CL-DAMAGE-SHIELD/a2; CL-DAMAGE-MULTIPLIER/a2; CL-DAMAGE-SHIELD-FIRST/a2; CL-DAMAGE-MULTIPLIER-FIRST/a2
    assert_current(
        completed.state(),
        &v,
        x,
        &[CardType::Artifact, CardType::Creature],
        &[],
        Some((2, 12)),
    );
    assert_eq!(
        completed.state().execution().battlefield_marked_damage(x),
        Some(expected.into())
    );
    assert_eq!(
        completed
            .state()
            .execution()
            .window()
            .cards()
            .get(x)
            .unwrap()
            .zone(),
        ObjectZone::Battlefield
    );
    // CL-DAMAGE-SHIELD/a3; CL-DAMAGE-MULTIPLIER/a3; CL-DAMAGE-SHIELD-FIRST/a3; CL-DAMAGE-MULTIPLIER-FIRST/a3
    let order = occurrences
        .iter()
        .filter_map(|value| match value {
            RulesOccurrence::DamagePrevented { .. } => Some("shield"),
            RulesOccurrence::DamageMultiplied(_) => Some("multiplier"),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        order,
        match (shield, multiplier, shield_first) {
            (true, false, _) => vec!["shield"],
            (false, true, _) => vec!["multiplier"],
            (true, true, true) => vec!["shield", "multiplier"],
            (true, true, false) => vec!["multiplier", "shield"],
            _ => unreachable!(),
        }
    );
    // CL-DAMAGE-SHIELD/a4; CL-DAMAGE-MULTIPLIER/a4; CL-DAMAGE-SHIELD-FIRST/a4; CL-DAMAGE-MULTIPLIER-FIRST/a4
    let attempts = journals
        .iter()
        .flat_map(|journal| &journal.attempts)
        .collect::<Vec<_>>();
    assert_eq!(attempts.len(), 1);
    assert_eq!(attempts[0].amount, 3.into());
    let applied = journals
        .iter()
        .flat_map(|journal| &journal.applied)
        .collect::<Vec<_>>();
    assert_eq!(applied.len(), 1);
    assert_eq!(applied[0].amount, expected.into());
    let preventions = journals
        .iter()
        .flat_map(|journal| &journal.preventions)
        .collect::<Vec<_>>();
    assert_eq!(preventions.len(), usize::from(shield));
    if shield {
        assert_eq!(preventions[0].amount, 2.into());
    }
    // CL-DAMAGE-SHIELD/a5; CL-DAMAGE-MULTIPLIER/a5; CL-DAMAGE-SHIELD-FIRST/a5; CL-DAMAGE-MULTIPLIER-FIRST/a5
    assert_eq!(
        completed.state().execution().prevention_shields().count(),
        0
    );
    assert_eq!(
        completed.state().execution().damage_multipliers().count(),
        usize::from(multiplier)
    );
    assert!(
        completed
            .state()
            .execution()
            .continuous_effects()
            .any(|record| record.parts.0 == vec![ResolvedPart::ModifyPT(pt(0, 2))])
    );
}

pub(crate) fn cleanup_noncreature_marks() {
    let v = vocabulary();
    let game = world(
        v.clone(),
        vec![
            card(
                1,
                permanent(&v, &[CardType::Creature], Some((2, 4)), &[]),
                true,
            ),
            spell(
                2,
                BodyTarget::SingleCreature,
                vec![spellwake_engine::program::Instruction::DealDamage(
                    Box::new(spellwake_engine::toolbox::DealDamageSpec {
                        source: spellwake_engine::toolbox::DamageSourceQuery::EntrySource,
                        recipients: spellwake_engine::toolbox::DamageTargetQuery::Targets,
                        amount: spellwake_engine::program::DamageAmountSpec::Expr(
                            spellwake_engine::program::ValueExpr::Literal(2.into()),
                        ),
                        exile_if_dies: false,
                    }),
                )],
            ),
            spell(
                3,
                BodyTarget::SinglePermanent,
                vec![remove_creature(ResolvedDuration::NoStatedDuration)],
            ),
            spell(
                4,
                BodyTarget::SinglePermanent,
                vec![effect(
                    vec![ResolvedPartSpec::ModifyPT(pt(0, 2))],
                    ResolvedDuration::UntilEndOfTurn,
                )],
            ),
        ],
        vec![CardId(1)],
    );
    let x = object(&game, 1);
    let (marked, _) = resolve(&game, 2, x);
    let (lost, _) = resolve(marked.state(), 3, x);
    let loss = created(&lost).id;
    let (latent, _) = resolve(lost.state(), 4, x);
    let temporary = created(&latent).id;
    checkpoint("before_cleanup", latent.state());
    // CL-CLEANUP-NONCREATURE-MARKS/a0
    assert_current(latent.state(), &v, x, &[], &[], None);
    assert_eq!(
        latent.state().execution().battlefield_marked_damage(x),
        Some(2.into())
    );
    let completed = cleanup(latent.state());
    checkpoint("after_cleanup", completed.state());
    // CL-CLEANUP-NONCREATURE-MARKS/a1
    assert_current(completed.state(), &v, x, &[], &[], None);
    assert_eq!(
        completed.state().execution().battlefield_marked_damage(x),
        Some(0.into())
    );
    assert_eq!(
        completed
            .state()
            .execution()
            .window()
            .cards()
            .get(x)
            .unwrap()
            .zone(),
        ObjectZone::Battlefield
    );
    // CL-CLEANUP-NONCREATURE-MARKS/a2
    assert!(
        completed
            .state()
            .execution()
            .continuous_effect(loss)
            .is_some()
    );
    assert!(
        completed
            .state()
            .execution()
            .continuous_effect(temporary)
            .is_none()
    );
}

/// Immutable references from the exact shared literal route, never a second execution.
pub(crate) enum TrajectoryObservation<'a> {
    BeforeInitialize {
        game: GameId,
        context: &'a GameContext,
        seed: &'a GameSeed,
        start: TurnStart,
    },
    BeforeAdvance {
        source: &'a GameState,
        input: &'a GameInput,
    },
    Initialize {
        game: GameId,
        context: &'a GameContext,
        seed: &'a GameSeed,
        start: TurnStart,
        result: &'a NativeCandidate,
    },
    Advance {
        source: &'a GameState,
        input: &'a GameInput,
        result: &'a NativeCandidate,
    },
    Evaluated {
        label: &'static str,
        state: &'a GameState,
        result: &'a EvaluatedCharacteristics,
    },
    BeforeAdvanceAttempt {
        source: &'a GameState,
        input: &'a GameInput,
    },
    AdvanceAttemptResult {
        source: &'a GameState,
        result: &'a Result<NativeCandidate, GameError>,
    },
    BeforeCurrent {
        state: &'a GameState,
        object: ObjectId,
    },
    Current {
        state: &'a GameState,
        result: &'a ObjectCharacteristics,
    },
    Checkpoint {
        label: &'static str,
        state: &'a GameState,
    },
}

thread_local! {
    static TRAJECTORY_OBSERVER: std::cell::Cell<Option<for<'a> fn(TrajectoryObservation<'a>)>> = const { std::cell::Cell::new(None) };
}

pub(crate) fn with_trajectory_observer(
    observe: for<'a> fn(TrajectoryObservation<'a>),
    run: impl FnOnce(),
) {
    struct Reset;
    impl Drop for Reset {
        fn drop(&mut self) {
            TRAJECTORY_OBSERVER.with(|slot| slot.set(None));
        }
    }
    TRAJECTORY_OBSERVER.with(|slot| {
        assert!(slot.get().is_none(), "nested immutable trajectory observer");
        slot.set(Some(observe));
    });
    let _reset = Reset;
    run();
}

fn observe_trajectory(value: TrajectoryObservation<'_>) {
    TRAJECTORY_OBSERVER.with(|slot| {
        if let Some(observe) = slot.get() {
            observe(value);
        }
    });
}

fn checkpoint(label: &'static str, state: &GameState) {
    observe_trajectory(TrajectoryObservation::Checkpoint { label, state });
}

// Mechanical shared bodies for the selected static reference routes.
pub(crate) fn started_nonempty() {
    let v = vocabulary();
    let mut source = permanent(&v, &[CardType::Artifact], Some((6, 6)), &[]);
    source.continuous = Some(StaticContinuousAbility::Battlefield {
        id: StaticAbilityId(1),
        selector: PermanentSelector {
            required_types: BTreeSet::from([CardType::Artifact]),
            forbidden_types: BTreeSet::from([CardType::Creature]),
            exclude_source: true,
            ..PermanentSelector::default()
        },
        parts: StaticParts(vec![
            StaticPart::Type(StaticTypeChange {
                land_subtypes: SubtypeOperation::Keep,
                supertypes: TypeOperation::Keep,
                types: TypeOperation::Add(BTreeSet::from([CardType::Creature])),
                subtypes: SubtypeOperation::<StaticSubtypeOperand>::Keep,
            }),
            StaticPart::SetPT(NumericPair {
                power: NumericExpression::Literal(2.into()),
                toughness: NumericExpression::Literal(2.into()),
            }),
        ]),
    });
    let game = world(
        v.clone(),
        vec![
            card(1, source, true),
            card(
                2,
                permanent(&v, &[CardType::Artifact], Some((4, 4)), &[]),
                true,
            ),
            card(
                3,
                permanent(&v, &[CardType::Artifact], Some((1, 1)), &[]),
                false,
            ),
        ],
        vec![CardId(1), CardId(2)],
    );
    let source = object(&game, 1);
    let x = object(&game, 2);
    let key = EffectInstanceKey::Static {
        source,
        ability: StaticAbilityId(1),
    };
    // CL-STARTED-NONEMPTY/a0
    assert_eq!(current(&game, x).power_toughness.unwrap().derived, pt(2, 2));
    // CL-STARTED-NONEMPTY/a1
    let first = game.execution().evaluated_characteristics().unwrap();
    evaluated_checkpoint("initial", &game, &first);
    assert_eq!(
        first.trace.bindings[&key],
        PassBinding::Processed(BTreeSet::from([x]))
    );
    let later = first
        .trace
        .applied
        .iter()
        .filter(|part| part.key == key && part.bucket == CharacteristicBucket::SetPT)
        .collect::<Vec<_>>();
    assert_eq!(later.len(), 1);
    assert_eq!(later[0].affected, BTreeSet::from([x]));
    let cast = submit(
        &game,
        P0,
        GameCommand::CastUntargeted {
            spell: object(&game, 3),
            payment: Some(ManaPayment::default()),
        },
    );
    let entered = pass_pair(cast.state());
    let y = object(entered.state(), 3);
    // CL-STARTED-NONEMPTY/a2
    let fresh = entered
        .state()
        .execution()
        .evaluated_characteristics()
        .unwrap();
    evaluated_checkpoint("final", entered.state(), &fresh);
    assert_eq!(
        fresh
            .get(x)
            .unwrap()
            .power_toughness
            .as_ref()
            .unwrap()
            .derived,
        pt(2, 2)
    );
    assert_eq!(
        fresh
            .get(y)
            .unwrap()
            .power_toughness
            .as_ref()
            .unwrap()
            .derived,
        pt(2, 2)
    );
    assert_eq!(
        fresh.trace.bindings[&key],
        PassBinding::Processed(BTreeSet::from([x, y]))
    );
}

pub(crate) fn assert_generated(
    evaluated: &EvaluatedCharacteristics,
    bucket: CharacteristicBucket,
    key: EffectInstanceKey,
    target: ObjectId,
    operation: ResolvedPart,
) {
    let selection = evaluated
        .trace
        .selections
        .iter()
        .find(|selection| selection.bucket == bucket)
        .unwrap();
    let generated = selection
        .generated
        .iter()
        .find(|generated| generated.key == key)
        .unwrap();
    assert!(generated.present);
    assert_eq!(generated.bucket, bucket);
    assert_eq!(generated.affected, BTreeSet::from([target]));
    assert_eq!(generated.generated, BTreeMap::from([(target, operation)]));
}

pub(crate) fn success_not_intent(grant_first: bool) {
    let v = vocabulary();
    let grant = effect(
        vec![type_part(
            TypeOperation::Keep,
            SubtypeOperation::Add(InstructionSubtypeOperand::Literal(subtypes(&v, &["Elf"]))),
        )],
        ResolvedDuration::NoStatedDuration,
    );
    let creature = add_creature(ResolvedDuration::NoStatedDuration);
    let first = if grant_first {
        grant.clone()
    } else {
        creature.clone()
    };
    let second = if grant_first { creature } else { grant };
    let game = world(
        v.clone(),
        vec![
            card(
                1,
                permanent(&v, &[CardType::Artifact], Some((2, 3)), &[]),
                true,
            ),
            spell(2, BodyTarget::SinglePermanent, vec![first]),
            spell(3, BodyTarget::SinglePermanent, vec![second]),
        ],
        vec![CardId(1)],
    );
    let x = object(&game, 1);
    let (first, _) = resolve(&game, 2, x);
    let first_id = created(&first).id;
    let (second, _) = resolve(first.state(), 3, x);
    let second_id = created(&second).id;
    let (g, c) = if grant_first {
        (first_id, second_id)
    } else {
        (second_id, first_id)
    };
    let evaluated = second
        .state()
        .execution()
        .evaluated_characteristics()
        .unwrap();
    evaluated_checkpoint("final", second.state(), &evaluated);
    // CL-DEP-SUCCESS-NOT-INTENT-GC/a0; CL-DEP-SUCCESS-NOT-INTENT-CG/a0
    assert_generated(
        &evaluated,
        CharacteristicBucket::Type,
        EffectInstanceKey::Resolved(g),
        x,
        ResolvedPart::Type(TypeChange {
            land_subtypes: SubtypeOperation::Keep,
            supertypes: TypeOperation::Keep,
            types: TypeOperation::Keep,
            subtypes: SubtypeOperation::Add(subtypes(&v, &["Elf"])),
        }),
    );
    assert_generated(
        &evaluated,
        CharacteristicBucket::Type,
        EffectInstanceKey::Resolved(c),
        x,
        ResolvedPart::Type(TypeChange {
            land_subtypes: SubtypeOperation::Keep,
            supertypes: TypeOperation::Keep,
            types: TypeOperation::Add(BTreeSet::from([CardType::Creature])),
            subtypes: SubtypeOperation::Keep,
        }),
    );
    let selection = evaluated
        .trace
        .selections
        .iter()
        .find(|selection| selection.bucket == CharacteristicBucket::Type)
        .unwrap();
    assert!(selection.original_waits.is_empty());
    assert_eq!(selection.chosen, EffectInstanceKey::Resolved(first_id));
    // CL-DEP-SUCCESS-NOT-INTENT-GC/a1; CL-DEP-SUCCESS-NOT-INTENT-CG/a1
    assert_eq!(
        evaluated
            .trace
            .applied
            .iter()
            .filter(|part| part.bucket == CharacteristicBucket::Type)
            .map(|part| part.key)
            .collect::<Vec<_>>(),
        vec![
            EffectInstanceKey::Resolved(first_id),
            EffectInstanceKey::Resolved(second_id)
        ]
    );
    // CL-DEP-SUCCESS-NOT-INTENT-GC/a2; CL-DEP-SUCCESS-NOT-INTENT-CG/a2
    assert_current(
        second.state(),
        &v,
        x,
        &[CardType::Artifact, CardType::Creature],
        if grant_first { &[] } else { &["Elf"] },
        Some((2, 3)),
    );
}

pub(crate) fn additive_not_totals() {
    let v = vocabulary();
    let game = world(
        v.clone(),
        vec![
            card(
                1,
                permanent(&v, &[CardType::Creature], Some((2, 3)), &[]),
                true,
            ),
            spell(
                2,
                BodyTarget::SingleCreature,
                vec![effect(
                    vec![ResolvedPartSpec::ModifyPT(pt(1, 2))],
                    ResolvedDuration::NoStatedDuration,
                )],
            ),
            spell(
                3,
                BodyTarget::SingleCreature,
                vec![effect(
                    vec![ResolvedPartSpec::ModifyPT(pt(3, 4))],
                    ResolvedDuration::NoStatedDuration,
                )],
            ),
        ],
        vec![CardId(1)],
    );
    let x = object(&game, 1);
    let (m1, _) = resolve(&game, 2, x);
    let first = created(&m1).id;
    let (m2, _) = resolve(m1.state(), 3, x);
    let second = created(&m2).id;
    let evaluated = m2.state().execution().evaluated_characteristics().unwrap();
    evaluated_checkpoint("final", m2.state(), &evaluated);
    // CL-DEP-ADDITIVE-NOT-TOTALS/a0
    assert_generated(
        &evaluated,
        CharacteristicBucket::ModifyPT,
        EffectInstanceKey::Resolved(first),
        x,
        ResolvedPart::ModifyPT(pt(1, 2)),
    );
    assert_generated(
        &evaluated,
        CharacteristicBucket::ModifyPT,
        EffectInstanceKey::Resolved(second),
        x,
        ResolvedPart::ModifyPT(pt(3, 4)),
    );
    let selection = evaluated
        .trace
        .selections
        .iter()
        .find(|selection| selection.bucket == CharacteristicBucket::ModifyPT)
        .unwrap();
    assert!(selection.original_waits.is_empty());
    assert_eq!(
        evaluated
            .trace
            .applied
            .iter()
            .filter(|part| part.bucket == CharacteristicBucket::ModifyPT)
            .map(|part| part.key)
            .collect::<Vec<_>>(),
        vec![
            EffectInstanceKey::Resolved(first),
            EffectInstanceKey::Resolved(second)
        ]
    );
    // CL-DEP-ADDITIVE-NOT-TOTALS/a1
    assert_eq!(
        current(m2.state(), x).power_toughness.unwrap().derived,
        pt(6, 9)
    );
}

pub(crate) fn cast_permanent(game: &GameState, id: u64) -> NativeCandidate {
    let cast = submit(
        game,
        P0,
        GameCommand::CastUntargeted {
            spell: object(game, id),
            payment: Some(ManaPayment::default()),
        },
    );
    pass_pair(cast.state())
}

pub(crate) fn static_program(
    v: &CreatureTypeVocabulary,
    types: &[CardType],
    values: Option<(i64, i64)>,
    selector: PermanentSelector,
    parts: Vec<StaticPart>,
) -> Program {
    let mut source = permanent(v, types, values, &[]);
    if types.contains(&CardType::Land) {
        let ProgramKind::Permanent(definition) = &mut source.kind else {
            unreachable!()
        };
        definition.casting_cost = None;
    }
    source.continuous = Some(StaticContinuousAbility::Battlefield {
        id: StaticAbilityId(1),
        selector,
        parts: StaticParts(parts),
    });
    source
}

pub(crate) fn static_add_creature() -> StaticPart {
    StaticPart::Type(StaticTypeChange {
        land_subtypes: SubtypeOperation::Keep,
        supertypes: TypeOperation::Keep,
        types: TypeOperation::Add(BTreeSet::from([CardType::Creature])),
        subtypes: SubtypeOperation::Keep,
    })
}

pub(crate) fn static_set(power: i64, toughness: i64) -> StaticPart {
    StaticPart::SetPT(NumericPair {
        power: NumericExpression::Literal(power.into()),
        toughness: NumericExpression::Literal(toughness.into()),
    })
}

pub(crate) fn selected_types(types: &[CardType]) -> PermanentSelector {
    PermanentSelector {
        required_supertypes: std::collections::BTreeSet::new(),
        forbidden_supertypes: std::collections::BTreeSet::new(),
        required_types: types.iter().copied().collect(),
        ..PermanentSelector::default()
    }
}

pub(crate) fn disappearing(first_is_c: bool) {
    let v = vocabulary();
    let a = static_program(
        &v,
        &[CardType::Artifact],
        Some((6, 6)),
        selected_types(&[CardType::Artifact]),
        vec![StaticPart::Type(StaticTypeChange {
            land_subtypes: SubtypeOperation::Keep,
            supertypes: TypeOperation::Keep,
            types: TypeOperation::Remove(BTreeSet::from([CardType::Creature])),
            subtypes: SubtypeOperation::Keep,
        })],
    );
    let add = effect(
        vec![type_part(
            TypeOperation::Add(BTreeSet::from([CardType::Artifact])),
            SubtypeOperation::Keep,
        )],
        ResolvedDuration::NoStatedDuration,
    );
    let game = world(
        v.clone(),
        vec![
            card(
                1,
                permanent(&v, &[CardType::Creature], Some((1, 3)), &[]),
                true,
            ),
            card(2, a, false),
            spell(3, BodyTarget::SinglePermanent, vec![add.clone()]),
            spell(4, BodyTarget::SinglePermanent, vec![add]),
        ],
        vec![CardId(1)],
    );
    let x = object(&game, 1);
    let (first, first_entry) = resolve(&game, 3, x);
    let e1 = created(&first).clone();
    let entered = cast_permanent(first.state(), 2);
    let source = object(entered.state(), 2);
    let a_key = EffectInstanceKey::Static {
        source,
        ability: StaticAbilityId(1),
    };
    let (last, last_entry) = resolve(entered.state(), 4, x);
    let e2 = created(&last).clone();
    let evaluated = last
        .state()
        .execution()
        .evaluated_characteristics()
        .unwrap();
    evaluated_checkpoint("final", last.state(), &evaluated);
    let selections = evaluated
        .trace
        .selections
        .iter()
        .filter(|selection| selection.bucket == CharacteristicBucket::Type)
        .collect::<Vec<_>>();
    let first_key = EffectInstanceKey::Resolved(e1.id);
    let last_key = EffectInstanceKey::Resolved(e2.id);
    // CL-DEP-DISAPPEARING-CAB/a0; CL-DEP-DISAPPEARING-BAC/a0
    assert_eq!(selections[0].chosen, first_key);
    assert_eq!(
        selections[0].original_waits,
        BTreeSet::from([
            spellwake_engine::characteristics::ComponentWait {
                waiting: a_key,
                prerequisite: first_key
            },
            spellwake_engine::characteristics::ComponentWait {
                waiting: a_key,
                prerequisite: last_key
            }
        ])
    );
    assert_generated(
        &evaluated,
        CharacteristicBucket::Type,
        a_key,
        source,
        ResolvedPart::Type(TypeChange {
            land_subtypes: SubtypeOperation::Keep,
            supertypes: TypeOperation::Keep,
            types: TypeOperation::Remove(BTreeSet::from([CardType::Creature])),
            subtypes: SubtypeOperation::Keep,
        }),
    );
    for key in [first_key, last_key] {
        assert_generated(
            &evaluated,
            CharacteristicBucket::Type,
            key,
            x,
            ResolvedPart::Type(TypeChange {
                land_subtypes: SubtypeOperation::Keep,
                supertypes: TypeOperation::Keep,
                types: TypeOperation::Add(BTreeSet::from([CardType::Artifact])),
                subtypes: SubtypeOperation::Keep,
            }),
        );
    }
    let stamps = last
        .state()
        .execution()
        .characteristic_timestamps()
        .collect::<BTreeMap<_, _>>();
    assert!(e1.timestamp < stamps[&source]);
    assert!(stamps[&source] < e2.timestamp);
    // CL-DEP-DISAPPEARING-CAB/a1; CL-DEP-DISAPPEARING-BAC/a1
    assert_eq!(selections[1].chosen, a_key);
    assert!(selections[1].original_waits.is_empty());
    let generated = selections[1]
        .generated
        .iter()
        .find(|value| value.key == a_key)
        .unwrap();
    assert!(generated.present);
    assert_eq!(generated.affected, BTreeSet::from([source, x]));
    assert_eq!(
        generated.generated,
        BTreeMap::from([
            (
                source,
                ResolvedPart::Type(TypeChange {
                    land_subtypes: SubtypeOperation::Keep,
                    supertypes: TypeOperation::Keep,
                    types: TypeOperation::Remove(BTreeSet::from([CardType::Creature])),
                    subtypes: SubtypeOperation::Keep
                })
            ),
            (
                x,
                ResolvedPart::Type(TypeChange {
                    land_subtypes: SubtypeOperation::Keep,
                    supertypes: TypeOperation::Keep,
                    types: TypeOperation::Remove(BTreeSet::from([CardType::Creature])),
                    subtypes: SubtypeOperation::Keep
                })
            )
        ])
    );
    let last_generated = selections[1]
        .generated
        .iter()
        .find(|value| value.key == last_key)
        .unwrap();
    assert!(last_generated.present);
    assert_eq!(last_generated.affected, BTreeSet::from([x]));
    assert_eq!(
        last_generated.generated,
        BTreeMap::from([(
            x,
            ResolvedPart::Type(TypeChange {
                land_subtypes: SubtypeOperation::Keep,
                supertypes: TypeOperation::Keep,
                types: TypeOperation::Add(BTreeSet::from([CardType::Artifact])),
                subtypes: SubtypeOperation::Keep
            })
        )])
    );
    // CL-DEP-DISAPPEARING-CAB/a2; CL-DEP-DISAPPEARING-BAC/a2
    assert_eq!(
        evaluated
            .trace
            .applied
            .iter()
            .filter(|part| part.bucket == CharacteristicBucket::Type)
            .map(|part| part.key)
            .collect::<Vec<_>>(),
        vec![first_key, a_key, last_key]
    );
    // CL-DEP-DISAPPEARING-CAB/a3; CL-DEP-DISAPPEARING-BAC/a3
    assert_current(last.state(), &v, x, &[CardType::Artifact], &[], None);
    // CL-DEP-DISAPPEARING-CAB/a4; CL-DEP-DISAPPEARING-BAC/a4
    assert_eq!(e1.creator, first_entry);
    assert_eq!(entered.permanent_entry().unwrap().after.id(), source);
    assert_eq!(
        zone_change(&entered, entered.permanent_entry().unwrap().before.id())
            .after
            .object
            .id(),
        source
    );
    assert_eq!(e2.creator, last_entry);
    let names = BTreeMap::from([
        (first_key, if first_is_c { "C" } else { "B" }),
        (a_key, "A"),
        (last_key, if first_is_c { "B" } else { "C" }),
    ]);
    assert_eq!(
        evaluated
            .trace
            .applied
            .iter()
            .filter(|part| part.bucket == CharacteristicBucket::Type)
            .map(|part| names[&part.key])
            .collect::<Vec<_>>(),
        if first_is_c {
            vec!["C", "A", "B"]
        } else {
            vec!["B", "A", "C"]
        }
    );
}

pub(crate) fn static_count_controller() {
    let v = vocabulary();
    let elf = v.resolve_label("Elf").unwrap();
    let selector = PermanentSelector {
        creature_subtype: Some(elf),
        controller: ControllerSelector::SourceController,
        ..selected_types(&[CardType::Creature])
    };
    let source = static_program(
        &v,
        &[CardType::Artifact],
        Some((6, 6)),
        selector.clone(),
        vec![StaticPart::ModifyPT(NumericPair {
            power: NumericExpression::count(selector.clone()),
            toughness: NumericExpression::count(selector),
        })],
    );
    let game = world(
        v.clone(),
        vec![
            card(1, source, true),
            card(
                2,
                permanent(&v, &[CardType::Creature], Some((2, 3)), &["Elf"]),
                true,
            ),
            card(
                3,
                permanent(&v, &[CardType::Creature], Some((1, 4)), &["Elf"]),
                true,
            ),
            controlled(
                4,
                P1,
                P1,
                permanent(&v, &[CardType::Creature], Some((3, 3)), &["Elf"]),
            ),
            spell(
                5,
                BodyTarget::SinglePermanent,
                vec![effect(
                    vec![type_part(
                        TypeOperation::Keep,
                        SubtypeOperation::Set(InstructionSubtypeOperand::Literal(subtypes(
                            &v,
                            &["Goblin"],
                        ))),
                    )],
                    ResolvedDuration::NoStatedDuration,
                )],
            ),
        ],
        vec![CardId(1), CardId(2), CardId(3), CardId(4)],
    );
    let s = object(&game, 1);
    let x = object(&game, 2);
    let y = object(&game, 3);
    let z = object(&game, 4);
    let key = EffectInstanceKey::Static {
        source: s,
        ability: StaticAbilityId(1),
    };
    assert_eq!(
        game.execution()
            .window()
            .cards()
            .get(z)
            .unwrap()
            .has_continuous_control_since_turn_start(),
        Some(false)
    );
    // CL-STATIC-COUNT-CONTROLLER/a0
    assert_eq!(current(&game, x).power_toughness.unwrap().derived, pt(4, 5));
    assert_eq!(current(&game, y).power_toughness.unwrap().derived, pt(3, 6));
    assert_eq!(current(&game, z).power_toughness.unwrap().derived, pt(3, 3));
    let before = game.execution().evaluated_characteristics().unwrap();
    evaluated_checkpoint("initial", &game, &before);
    let (changed, _) = resolve(&game, 5, y);
    // CL-STATIC-COUNT-CONTROLLER/a1
    assert_eq!(
        current(changed.state(), x).power_toughness.unwrap().derived,
        pt(3, 4)
    );
    assert_eq!(
        current(changed.state(), y).power_toughness.unwrap().derived,
        pt(1, 4)
    );
    assert_eq!(
        current(changed.state(), z).power_toughness.unwrap().derived,
        pt(3, 3)
    );
    // CL-STATIC-COUNT-CONTROLLER/a2
    let first = before
        .trace
        .applied
        .iter()
        .find(|part| part.key == key)
        .unwrap();
    assert_eq!(first.affected, BTreeSet::from([x, y]));
    assert_eq!(
        first.generated,
        BTreeMap::from([
            (x, ResolvedPart::ModifyPT(pt(2, 2))),
            (y, ResolvedPart::ModifyPT(pt(2, 2)))
        ])
    );
    let after = changed
        .state()
        .execution()
        .evaluated_characteristics()
        .unwrap();
    evaluated_checkpoint("final", changed.state(), &after);
    let later = after
        .trace
        .applied
        .iter()
        .find(|part| part.key == key)
        .unwrap();
    assert_eq!(later.affected, BTreeSet::from([x]));
    assert_eq!(
        later.generated,
        BTreeMap::from([(x, ResolvedPart::ModifyPT(pt(1, 1)))])
    );
    // CL-STATIC-COUNT-CONTROLLER/a3
    for state in [&game, changed.state()] {
        for (id, owner) in [(s, P0), (x, P0), (y, P0), (z, P1)] {
            assert_eq!(
                state
                    .execution()
                    .window()
                    .cards()
                    .get(id)
                    .unwrap()
                    .controller(),
                Some(owner)
            );
        }
    }
}

/// Observe an already returned current-world query, without evaluating again here.
fn evaluated_checkpoint(label: &'static str, state: &GameState, result: &EvaluatedCharacteristics) {
    observe_trajectory(TrajectoryObservation::Evaluated {
        label,
        state,
        result,
    });
}

// Mechanical shared bodies for the six resolved reference routes.
pub(crate) fn counters() -> Instruction {
    {
        struct _Tmp {
            counters: spellwake_engine::characteristics::PowerToughnessCounters,
        }
        let _t = _Tmp {
            counters: PowerToughnessCounters {
                plus_one: 1.into(),
                minus_one: 0.into(),
            },
        };
        let (_k, _a) = if _t.counters.minus_one.is_positive() {
            (
                spellwake_engine::effects::CounterKind::MinusOne,
                _t.counters.minus_one,
            )
        } else {
            (
                spellwake_engine::effects::CounterKind::PlusOne,
                _t.counters.plus_one,
            )
        };
        spellwake_engine::program::Instruction::MutateObject(Box::new(
            spellwake_engine::toolbox::MutateObjectSpec::all(
                spellwake_engine::toolbox::ObjectQuery::Targets,
                spellwake_engine::toolbox::ObjectMutation::Counters(
                    spellwake_engine::toolbox::CounterOp::Add {
                        kind: _k,
                        amount: spellwake_engine::program::ValueExpr::Literal(_a),
                    },
                ),
            ),
        ))
    }
}

pub(crate) fn appearance_change(
    candidate: &NativeCandidate,
    target: ObjectId,
) -> &CharacteristicAppearanceChange {
    candidate
        .rules_journal()
        .records
        .iter()
        .filter_map(|record| match record {
            RulesRecord::Event(event) => Some(&event.occurrences),
            _ => None,
        })
        .flatten()
        .find_map(|row| match &row.value {
            RulesOccurrence::ContinuousEffectCreated { appearances, .. } => {
                appearances.iter().find(|change| change.object == target)
            }
            _ => None,
        })
        .unwrap()
}

pub(crate) fn type_explicit_remove() {
    let v = vocabulary();
    let game = world(
        v.clone(),
        vec![
            card(
                1,
                permanent(&v, &[CardType::Creature], Some((2, 3)), &["Elf"]),
                true,
            ),
            spell(
                2,
                BodyTarget::SinglePermanent,
                vec![effect(
                    vec![type_part(
                        TypeOperation::Keep,
                        SubtypeOperation::Remove(subtypes(&v, &["Elf"])),
                    )],
                    ResolvedDuration::UntilEndOfTurn,
                )],
            ),
            spell(
                3,
                BodyTarget::SinglePermanent,
                vec![add_creature(ResolvedDuration::NoStatedDuration)],
            ),
        ],
        vec![CardId(1)],
    );
    let x = object(&game, 1);
    let (removed, _) = resolve(&game, 2, x);
    let (added, _) = resolve(removed.state(), 3, x);
    checkpoint("after TypeOnly", added.state());
    // CL-TYPE-EXPLICIT-REMOVE/a0
    assert!(
        current(added.state(), x)
            .current_creature_subtypes
            .is_empty()
    );
    let completed = cleanup(added.state());
    checkpoint("after cleanup", completed.state());
    // CL-TYPE-EXPLICIT-REMOVE/a1
    assert_eq!(
        current(completed.state(), x).current_creature_subtypes,
        subtypes(&v, &["Elf"])
    );
}

pub(crate) fn type_replace() {
    let v = vocabulary();
    let game = world(
        v.clone(),
        vec![
            card(
                1,
                permanent(&v, &[CardType::Creature], Some((2, 3)), &["Elf"]),
                true,
            ),
            spell(
                2,
                BodyTarget::SinglePermanent,
                vec![effect(
                    vec![type_part(
                        TypeOperation::Keep,
                        SubtypeOperation::Set(InstructionSubtypeOperand::Literal(subtypes(
                            &v,
                            &["Goblin"],
                        ))),
                    )],
                    ResolvedDuration::NoStatedDuration,
                )],
            ),
            spell(
                3,
                BodyTarget::SinglePermanent,
                vec![add_creature(ResolvedDuration::NoStatedDuration)],
            ),
        ],
        vec![CardId(1)],
    );
    let x = object(&game, 1);
    let (replaced, _) = resolve(&game, 2, x);
    let (added, _) = resolve(replaced.state(), 3, x);
    checkpoint("final", added.state());
    // CL-TYPE-REPLACE/a0
    assert_current(
        added.state(),
        &v,
        x,
        &[CardType::Creature],
        &["Goblin"],
        Some((2, 3)),
    );
    checkpoint("defined", added.state());
    // CL-TYPE-REPLACE/a1
    assert_eq!(
        current(added.state(), x).defined.creature_subtypes,
        subtypes(&v, &["Elf"])
    );
}

pub(crate) fn pt_zero() {
    let v = vocabulary();
    let game = world(
        v.clone(),
        vec![
            card(1, permanent(&v, &[CardType::Artifact], None, &[]), true),
            spell(
                2,
                BodyTarget::SinglePermanent,
                vec![add_creature(ResolvedDuration::NoStatedDuration)],
            ),
        ],
        vec![CardId(1)],
    );
    let x = object(&game, 1);
    let (animated, _) = resolve(&game, 2, x);
    checkpoint("final", animated.state());
    // CL-PT-ZERO/a0
    let appearance = &appearance_change(&animated, x).after;
    assert_eq!(
        appearance.types,
        BTreeSet::from([CardType::Artifact, CardType::Creature])
    );
    assert_eq!(
        appearance.current_power_toughness.as_ref().unwrap().clone(),
        pt(0, 0)
    );
    // CL-PT-ZERO/a1
    let dead = object(animated.state(), 1);
    assert_ne!(dead, x);
    assert_eq!(
        animated
            .state()
            .execution()
            .window()
            .cards()
            .get(dead)
            .unwrap()
            .zone(),
        ObjectZone::Graveyard(P0)
    );
    let causes = animated
        .rules_journal()
        .records
        .iter()
        .filter_map(|record| match record {
            RulesRecord::Event(event) => Some(&event.occurrences),
            _ => None,
        })
        .flatten()
        .filter_map(|row| match &row.value {
            RulesOccurrence::ZoneChange(change) if change.before.object.id() == x => {
                Some(change.reason)
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        causes,
        vec![ZoneReason::StateBased(
            spellwake_engine::occurrence::StateBasedMoveReasons::new(
                spellwake_engine::occurrence::StateBasedMoveReason::ZeroToughness
            )
        )]
    );
    // CL-PT-ZERO/a2
    let action_order = animated
        .rules_journal()
        .records
        .iter()
        .filter_map(|record| match record {
            RulesRecord::Event(event) => match &event.cause {
                EventCause::Instruction { .. }
                    if event.occurrences.iter().any(|row| {
                        matches!(row.value, RulesOccurrence::ContinuousEffectCreated { .. })
                    }) =>
                {
                    Some("animation")
                }
                EventCause::StateBasedActions { .. } => Some("checks"),
                _ => None,
            },
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(action_order, vec!["animation", "checks"]);
}

pub(crate) fn pt_multipart() {
    let v = vocabulary();
    let game = world(
        v.clone(),
        vec![
            card(1, permanent(&v, &[CardType::Artifact], None, &[]), true),
            spell(
                2,
                BodyTarget::SinglePermanent,
                vec![effect(
                    vec![
                        type_part(
                            TypeOperation::Add(BTreeSet::from([CardType::Creature])),
                            SubtypeOperation::Keep,
                        ),
                        ResolvedPartSpec::SetPT(pt(2, 2)),
                    ],
                    ResolvedDuration::UntilEndOfTurn,
                )],
            ),
        ],
        vec![CardId(1)],
    );
    let x = object(&game, 1);
    let (completed, entry) = resolve(&game, 2, x);
    checkpoint("final", completed.state());
    // CL-PT-MULTIPART/a0
    assert_current(
        completed.state(),
        &v,
        x,
        &[CardType::Artifact, CardType::Creature],
        &[],
        Some((2, 2)),
    );
    assert_eq!(
        completed
            .state()
            .execution()
            .window()
            .cards()
            .get(x)
            .unwrap()
            .zone(),
        ObjectZone::Battlefield
    );
    // CL-PT-MULTIPART/a1
    let record = created(&completed);
    assert_eq!(
        completed.state().execution().continuous_effects().count(),
        1
    );
    assert_eq!(record.creator, entry);
    assert_eq!(record.affected, BTreeSet::from([x]));
    assert_eq!(record.duration, ResolvedDuration::UntilEndOfTurn);
    let evaluated = completed
        .state()
        .execution()
        .evaluated_characteristics()
        .unwrap();
    evaluated_checkpoint("multipart trace", completed.state(), &evaluated);
    let parts = evaluated
        .trace
        .applied
        .iter()
        .filter(|part| part.key == EffectInstanceKey::Resolved(record.id))
        .collect::<Vec<_>>();
    assert_eq!(parts.len(), 2);
    for part in &parts {
        assert_eq!(part.affected, BTreeSet::from([x]));
        let EffectInstanceKey::Resolved(id) = part.key else {
            panic!("resolved creation")
        };
        assert_eq!(
            completed
                .state()
                .execution()
                .continuous_effect(id)
                .unwrap()
                .timestamp,
            record.timestamp
        );
    }
    // CL-PT-MULTIPART/a2
    assert_eq!(
        parts.iter().map(|part| part.bucket).collect::<Vec<_>>(),
        vec![CharacteristicBucket::Type, CharacteristicBucket::SetPT]
    );
}

pub(crate) fn pt_core() {
    let v = vocabulary();
    let game = world(
        v.clone(),
        vec![
            card(
                1,
                permanent(&v, &[CardType::Creature], Some((2, 3)), &[]),
                true,
            ),
            spell(
                2,
                BodyTarget::SinglePermanent,
                vec![effect(
                    vec![ResolvedPartSpec::SetPT(pt(4, 5))],
                    ResolvedDuration::NoStatedDuration,
                )],
            ),
            spell(
                3,
                BodyTarget::SinglePermanent,
                vec![effect(
                    vec![ResolvedPartSpec::ModifyPT(pt(1, 2))],
                    ResolvedDuration::NoStatedDuration,
                )],
            ),
            spell(4, BodyTarget::SinglePermanent, vec![counters()]),
            spell(
                5,
                BodyTarget::SinglePermanent,
                vec![effect(
                    vec![ResolvedPartSpec::SwitchPT],
                    ResolvedDuration::NoStatedDuration,
                )],
            ),
        ],
        vec![CardId(1)],
    );
    let x = object(&game, 1);
    let (set, _) = resolve(&game, 2, x);
    let setter = created(&set).id;
    let (modified, _) = resolve(set.state(), 3, x);
    let modifier = created(&modified).id;
    let (counted, _) = resolve(modified.state(), 4, x);
    let (switched, _) = resolve(counted.state(), 5, x);
    let switch = created(&switched).id;
    checkpoint("final", switched.state());
    // CL-PT-CORE/a0
    let actual = current(switched.state(), x);
    assert_eq!(actual.defined.printed_pt, Some(pt(2, 3).into()));
    let values = actual.power_toughness.unwrap();
    assert_eq!(values.base, pt(4, 5));
    assert_eq!(values.derived, pt(8, 6));
    assert_eq!(
        switched.state().execution().power_toughness_counters(x),
        Some(PowerToughnessCounters {
            plus_one: 1.into(),
            minus_one: 0.into()
        })
    );
    // CL-PT-CORE/a1
    let evaluated = switched
        .state()
        .execution()
        .evaluated_characteristics()
        .unwrap();
    evaluated_checkpoint("core trace", switched.state(), &evaluated);
    assert_eq!(
        evaluated
            .trace
            .applied
            .iter()
            .map(|part| (part.key, part.bucket))
            .collect::<Vec<_>>(),
        vec![
            (
                EffectInstanceKey::Resolved(setter),
                CharacteristicBucket::SetPT
            ),
            (
                EffectInstanceKey::Resolved(modifier),
                CharacteristicBucket::ModifyPT
            ),
            (
                EffectInstanceKey::Resolved(switch),
                CharacteristicBucket::SwitchPT
            ),
        ]
    );
    let [counter] = evaluated.trace.counter_applications.as_slice() else {
        panic!("one physical counter application for the frozen X")
    };
    assert_eq!(counter.object, x);
    assert_eq!(counter.after_components, 2);
    assert_eq!(
        counter.counters,
        PowerToughnessCounters {
            plus_one: 1.into(),
            minus_one: 0.into()
        }
    );
    assert_eq!(counter.before, pt(5, 7));
    assert_eq!(counter.after, pt(6, 8));
    assert_eq!(counter.after.power.minus(&counter.before.power), 1.into());
    assert_eq!(
        counter.after.toughness.minus(&counter.before.toughness),
        1.into()
    );
    assert_eq!(
        evaluated.trace.applied[counter.after_components - 1].key,
        EffectInstanceKey::Resolved(modifier)
    );
    assert_eq!(
        evaluated.trace.applied[counter.after_components].key,
        EffectInstanceKey::Resolved(switch)
    );
}

pub(crate) fn target_later_bounce() {
    let v = vocabulary();
    let game = world(
        v.clone(),
        vec![
            card(
                1,
                permanent(&v, &[CardType::Creature], Some((2, 3)), &["Elf"]),
                true,
            ),
            spell(
                2,
                BodyTarget::SingleCreature,
                vec![
                    remove_creature(ResolvedDuration::NoStatedDuration),
                    spellwake_engine::program::Instruction::ZoneOp(Box::new(
                        spellwake_engine::toolbox::ZoneOpSpec::move_all(
                            spellwake_engine::toolbox::ObjectQuery::Targets,
                            spellwake_engine::toolbox::ZoneDest::Hand,
                        ),
                    )),
                ],
            ),
        ],
        vec![CardId(1)],
    );
    let x = object(&game, 1);
    // CL-TARGET-LATER-BOUNCE/a0
    let (completed, entry) = resolve(&game, 2, x);
    assert_completion(&completed, entry, EntryCompletion::Resolved);
    checkpoint("final", completed.state());
    // CL-TARGET-LATER-BOUNCE/a1
    assert!(
        appearance_change(&completed, x)
            .after
            .current_power_toughness
            .is_none()
    );
    // CL-TARGET-LATER-BOUNCE/a2
    let hand = object(completed.state(), 1);
    assert_ne!(hand, x);
    let physical = completed
        .state()
        .execution()
        .window()
        .cards()
        .get(hand)
        .unwrap();
    assert_eq!(physical.zone(), ObjectZone::Hand(P0));
    assert_eq!(physical.owner(), P0);
    assert_eq!(physical.controller(), None);
    // CL-TARGET-LATER-BOUNCE/a3
    assert!(
        completed
            .state()
            .execution()
            .window()
            .cards()
            .get(x)
            .is_none()
    );
}
