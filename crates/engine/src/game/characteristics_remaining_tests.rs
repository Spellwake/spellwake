//! Remaining frozen root/actual-cut units, authored before source binding.
use super::*;
use crate::{
    characteristics::{
        ControllerSelector, PermanentSelector, PowerToughness, ResolvedDuration, ResolvedPartSpec,
        ResolvedPartsSpec, StaticAbilityId, StaticContinuousAbility, StaticPart, StaticParts,
        StaticTypeChange, SubtypeOperation, TypeChangeSpec, TypeOperation,
    },
    combat::AttackDeclaration,
    mana::ManaCost,
    object_identity::{CardId, ObjectDestination},
    occurrence::{RulesOccurrence, RulesRecord},
    program::{CardType, Instruction, Program, ProgramKind, TriggeredAbility, TriggeredAbilityId},
};

const A: SeatId = SeatId(0);
const B: SeatId = SeatId(1);
fn seed(cards: Vec<SeedCard>, order: Vec<CardId>) -> GameSeed {
    GameSeed::with_uniform_life(
        vec![A, B],
        A,
        cards,
        order,
        vec![],
        20.into(),
        CombatRules::UnrestrictedFreeForAll,
        crate::test_support::creature_types::selected_vocabulary()
            .expect("explicit qualification vocabulary"),
        std::sync::Arc::new(spellwake_contracts::names::NameRules::literal_names()),
    )
}
fn card(id: u64, program: Program, destination: ObjectDestination) -> SeedCard {
    SeedCard {
        card: CardId(id),
        owner: A,
        destination,
        program,
    }
}
fn battlefield() -> ObjectDestination {
    ObjectDestination::Battlefield { controller: A }
}
fn context(seed: &GameSeed) -> GameContext {
    GameContext::new(
        "frozen-owner-next".into(),
        seed.creature_vocabulary.rules_revision().into(),
        "literal-fixtures".into(),
        "characteristic-remaining-v1".into(),
        "individual-turn".into(),
        seed.creature_vocabulary.id(),
        spellwake_contracts::names::NameRules::literal_names().id(),
    )
    .unwrap()
}
fn initialize(seed: GameSeed) -> GameState {
    GameState::initialize_scheduled(
        GameId(6199),
        context(&seed),
        seed,
        TurnStart {
            number: 1,
            step: Step::Untap,
        },
    )
    .unwrap()
    .into_state()
}
fn object(game: &GameState, id: u64) -> ObjectId {
    game.execution
        .window()
        .cards()
        .current_identity(CardId(id))
        .unwrap()
}
fn accept(
    game: &mut GameState,
    actor: SeatId,
    command: GameCommand,
    records: &mut Vec<RulesRecord>,
) -> NativeCandidate {
    let saved = game.clone();
    let next = game
        .advance(GameInput {
            basis: game.basis().clone(),
            actor,
            command,
        })
        .unwrap();
    assert_eq!(*game, saved);
    records.extend(next.rules_journal().records.clone());
    *game = next.state().clone();
    next
}
fn pair(game: &mut GameState, records: &mut Vec<RulesRecord>) {
    for actor in [A, B] {
        accept(game, actor, GameCommand::Pass, records);
    }
}
fn main_phase(game: &mut GameState, records: &mut Vec<RulesRecord>) {
    assert_eq!(game.turn().unwrap().position().step, Step::Upkeep);
    pair(game, records);
    assert_eq!(game.turn().unwrap().position().step, Step::PrecombatMain);
}
fn declare(game: &mut GameState, x: ObjectId, records: &mut Vec<RulesRecord>) {
    pair(game, records);
    assert_eq!(game.turn().unwrap().position().step, Step::BeginningCombat);
    pair(game, records);
    assert_eq!(game.turn().unwrap().position().step, Step::DeclareAttackers);
    let decision = game.pending_decision().unwrap();
    assert!(matches!(
        decision.frame(),
        RuleFrame::Combat(CombatDecision::Attackers(_))
    ));
    let id = decision.id();
    accept(
        game,
        A,
        GameCommand::Reply {
            decision: id,
            choice: DecisionReply::DeclareAttackers(vec![AttackDeclaration {
                attacker: x,
                defender: B,
                defending_planeswalker: None,
            }]),
        },
        records,
    );
    pair(game, records);
    assert_eq!(game.turn().unwrap().position().step, Step::DeclareBlockers);
    assert!(game.pending_decision().is_none());
    assert!(
        matches!(game.execution.window().phase(),WindowPhase::Priority(PriorityPhase::Ready {holder}) if holder==A)
    );
    assert_eq!(
        game.execution
            .combat()
            .unwrap()
            .attackers()
            .map(|row| row.combatant().object())
            .collect::<Vec<_>>(),
        vec![x]
    );
}
fn removed(records: &[RulesRecord]) -> Vec<ObjectId> {
    records
        .iter()
        .filter_map(|record| match record {
            RulesRecord::Event(event) => Some(&event.occurrences),
            _ => None,
        })
        .flatten()
        .filter_map(|row| match &row.value {
            RulesOccurrence::CombatantsRemoved { participants } => Some(
                participants
                    .iter()
                    .map(|participant| participant.object())
                    .collect::<Vec<_>>(),
            ),
            _ => None,
        })
        .flatten()
        .collect()
}
fn assert_current(game: &GameState, x: ObjectId, types: &[CardType], pt: Option<(i64, i64)>) {
    let value = game.execution.current_characteristics(x).unwrap().unwrap();
    assert_eq!(value.current_types, types.iter().copied().collect());
    assert_eq!(
        value.power_toughness.map(|value| value.derived),
        pt.map(|(power, toughness)| PowerToughness {
            power: power.into(),
            toughness: toughness.into()
        })
    );
}

#[test]
fn cl_combat_static_entry() {
    let mut x = Program::creature(Some(ManaCost::free()), 2.into(), 4.into(), BTreeSet::new());
    x.card_types.insert(CardType::Artifact);
    let mut source = Program::artifact(Some(ManaCost::free()), vec![]);
    let ProgramKind::Permanent(d) = &mut source.kind else {
        unreachable!()
    };
    d.printed_pt = Some(
        PowerToughness {
            power: 6.into(),
            toughness: 6.into(),
        }
        .into(),
    );
    source.continuous = Some(StaticContinuousAbility::Battlefield {
        id: StaticAbilityId(1),
        selector: PermanentSelector {
            requires_untapped: false,
            sole_controlled_creature: false,
            token: None,
            combat_role: None,
            requires_flying: false,
            plus_one_counter: false,
            minimum_plus_one_counters: 0,
            any_counter: false,
            source_has_imprinted_creature: false,
            controlled_creatures_and_typed_tokens: None,
            exclude_auras: false,
            artifact_subtype: None,
            matches_source_entry_type: false,
            controller_type_threshold: None,
            source_condition: None,
            equipped_by_at_least: 0,
            devotion_below: None,
            while_controlling_own_commander: false,
            station_at: None,
            attached_to_source: false,
            land_subtype: None,
            required_supertypes: BTreeSet::new(),
            forbidden_supertypes: BTreeSet::new(),
            name: crate::characteristics::NameFilter::Any,
            required_types: BTreeSet::from([CardType::Artifact]),
            forbidden_types: BTreeSet::new(),
            forbidden_creature_subtype: None,
            creature_subtype: None,
            color: Default::default(),
            controller: ControllerSelector::Any,
            exclude_source: false,
        },
        parts: StaticParts(vec![StaticPart::Type(StaticTypeChange {
            land_subtypes: SubtypeOperation::Keep,
            supertypes: TypeOperation::Keep,
            types: TypeOperation::Remove(BTreeSet::from([CardType::Creature])),
            subtypes: SubtypeOperation::Keep,
        })]),
    });
    let mut game = initialize(seed(
        vec![
            card(1, x, battlefield()),
            card(2, source, ObjectDestination::Hand),
        ],
        vec![CardId(1)],
    ));
    let mut records = vec![];
    main_phase(&mut game, &mut records);
    let x = object(&game, 1);
    declare(&mut game, x, &mut records);
    let s = object(&game, 2);
    // Canonical non-BF tapped is None. The frozen v2 S@0 tapped:false premise
    // is flagged in this author packet; it is not forged into physical state.
    assert_eq!(
        game.execution.window().cards().get(s).unwrap().tapped(),
        None
    );
    let entry = game
        .execution
        .qualification_zone_moves(game.execution.revision(), &[(s, battlefield())])
        .unwrap();
    let entered = entry.result()[0].after.id();
    let entry_records = entry.append().records.clone();
    *game.execution = entry.into_state();
    // MAP CL-COMBAT-STATIC-ENTRY assertion 1
    assert_current(&game, x, &[CardType::Artifact], None);
    assert_eq!(game.execution.combat().unwrap().attackers().count(), 0);
    assert_eq!(game.execution.combat().unwrap().blockers().count(), 0);
    assert_eq!(removed(&entry_records), vec![x]);
    assert!(game.execution.combat().unwrap().had_attackers());
    let exit = game
        .execution
        .qualification_zone_moves(
            game.execution.revision(),
            &[(entered, ObjectDestination::Hand)],
        )
        .unwrap();
    *game.execution = exit.into_state();
    // MAP CL-COMBAT-STATIC-ENTRY assertion 2
    assert_current(
        &game,
        x,
        &[CardType::Artifact, CardType::Creature],
        Some((2, 4)),
    );
    assert_eq!(game.execution.combat().unwrap().attackers().count(), 0);
    assert_eq!(game.execution.combat().unwrap().blockers().count(), 0);
    assert!(game.execution.combat().unwrap().had_attackers());
}

#[test]
fn cl_combat_pure_transient() {
    let mut game = initialize(seed(
        vec![card(
            1,
            Program::creature(Some(ManaCost::free()), 2.into(), 4.into(), BTreeSet::new()),
            battlefield(),
        )],
        vec![CardId(1)],
    ));
    let mut records = vec![];
    main_phase(&mut game, &mut records);
    let x = object(&game, 1);
    game.execution.qualification_prepare_frozen_transient(x);
    let effects: Vec<_> = game.execution.continuous_effects().cloned().collect();
    assert_eq!(effects.len(), 2);
    assert!(effects[0].timestamp < effects[1].timestamp);
    assert!(effects.iter().all(
        |effect| effect.duration == ResolvedDuration::NoStatedDuration
            && effect.affected == BTreeSet::from([x])
    ));
    declare(&mut game, x, &mut records);
    let saved = game.clone();
    // MAP CL-COMBAT-PURE-TRANSIENT assertion 1
    let first = game.execution.evaluated_characteristics().unwrap();
    let second = game.execution.evaluated_characteristics().unwrap();
    assert_eq!(first, second);
    assert_eq!(
        first.get(x).unwrap().current_types,
        BTreeSet::from([CardType::Creature])
    );
    assert_eq!(first.trace.selections.len(), 2);
    assert!(first.trace.selections[0].original_waits.is_empty());
    // The evaluator's actual generated-component dependency probes transiently
    // remove/readd Creature in isolated scratch, with no actual-cut finalizer.
    // MAP CL-COMBAT-PURE-TRANSIENT assertion 2
    assert_eq!(game, saved);
    // MAP CL-COMBAT-PURE-TRANSIENT assertion 3
    assert_eq!(
        game.execution
            .combat()
            .unwrap()
            .attackers()
            .map(|row| row.combatant().object())
            .collect::<Vec<_>>(),
        vec![x]
    );
    assert_eq!(game.execution.combat().unwrap().blockers().count(), 0);
    assert!(game.execution.combat().unwrap().had_attackers());
    assert!(removed(&records).is_empty());
}

fn effect_spell(parts: Vec<ResolvedPartSpec>, duration: ResolvedDuration) -> Program {
    Program::instant(
        ManaCost::free(),
        ExecutableBody::new(
            BodyTarget::SinglePermanent,
            vec![Instruction::CreateContinuousEffect {
                parts: ResolvedPartsSpec(parts),
                duration,
            }],
        ),
    )
}
fn resolve(game: &mut GameState, spell: u64, target: ObjectId, records: &mut Vec<RulesRecord>) {
    let spell = object(game, spell);
    accept(
        game,
        A,
        GameCommand::Cast {
            spell,
            target: TargetRef::Object(target),
        },
        records,
    );
    pair(game, records);
}
fn cleanup_repeat_prefix() -> (
    GameState,
    ObjectId,
    crate::characteristics::ContinuousEffectId,
    Vec<RulesRecord>,
) {
    let mut n = Program::artifact(Some(ManaCost::free()), vec![]);
    n.triggered.push(TriggeredAbility {
        grouping: spellwake_engine::program::TriggerGrouping::EachOccurrence,
        id: TriggeredAbilityId(1),
        condition: crate::program::TriggerSpec::dies(crate::program::ObjectFilter::self_object()),
        body: ExecutableBody::new(
            BodyTarget::None,
            vec![Instruction::DrawController {
                amount: crate::program::ValueExpr::Literal(1.into()),
            }],
        ),
    });
    let cards = vec![
        card(1, n, battlefield()),
        card(
            2,
            Program::creature(Some(ManaCost::free()), 2.into(), 2.into(), BTreeSet::new()),
            battlefield(),
        ),
        card(
            3,
            Program::artifact(Some(ManaCost::free()), vec![]),
            ObjectDestination::LibraryBottom,
        ),
        card(
            4,
            effect_spell(
                vec![ResolvedPartSpec::ModifyPT(PowerToughness {
                    power: 1.into(),
                    toughness: 1.into(),
                })],
                ResolvedDuration::UntilEndOfTurn,
            ),
            ObjectDestination::Hand,
        ),
        card(
            5,
            effect_spell(
                vec![ResolvedPartSpec::Type(TypeChangeSpec {
                    land_subtypes: SubtypeOperation::Keep,
                    supertypes: TypeOperation::Keep,
                    types: TypeOperation::Add(BTreeSet::from([CardType::Creature])),
                    subtypes: SubtypeOperation::Keep,
                })],
                ResolvedDuration::NoStatedDuration,
            ),
            ObjectDestination::Hand,
        ),
        card(
            6,
            effect_spell(
                vec![ResolvedPartSpec::ModifyPT(PowerToughness {
                    power: 0.into(),
                    toughness: 3.into(),
                })],
                ResolvedDuration::UntilEndOfTurn,
            ),
            ObjectDestination::Hand,
        ),
    ];
    let mut game = initialize(seed(cards, vec![CardId(1), CardId(2)]));
    let mut records = vec![];
    main_phase(&mut game, &mut records);
    let n = object(&game, 1);
    let x = object(&game, 2);
    assert_eq!(game.execution.window().cards().library(A).len(), 1);
    assert!(game.execution.window().cards().library(B).is_empty());
    resolve(&mut game, 4, n, &mut records);
    resolve(&mut game, 5, n, &mut records);
    assert_current(
        &game,
        n,
        &[CardType::Artifact, CardType::Creature],
        Some((1, 1)),
    );
    // Actual ordinary passes and exact replies reach the first cleanup. There
    // are no attackers, no blocks and no combat damage in this frozen prefix.
    for expected in [Step::BeginningCombat, Step::DeclareAttackers] {
        pair(&mut game, &mut records);
        assert_eq!(game.turn().unwrap().position().step, expected);
    }
    let decision = game.pending_decision().unwrap();
    let id = decision.id();
    assert!(matches!(
        decision.frame(),
        RuleFrame::Combat(CombatDecision::Attackers(_))
    ));
    accept(
        &mut game,
        A,
        GameCommand::Reply {
            decision: id,
            choice: DecisionReply::DeclareAttackers(vec![]),
        },
        &mut records,
    );
    for expected in [
        Step::EndCombat,
        Step::PostcombatMain,
        Step::End,
        Step::Cleanup,
    ] {
        pair(&mut game, &mut records);
        assert_eq!(game.turn().unwrap().position().step, expected);
    }
    assert_eq!(game.turn().unwrap().position().cleanup_iteration, 0);
    assert_eq!(
        game.execution
            .window()
            .cards()
            .get(object(&game, 1))
            .unwrap()
            .zone(),
        ObjectZone::Graveyard(A)
    );
    assert_eq!(game.execution.window().entries().len(), 1);
    assert!(game.pending_decision().is_none());
    pair(&mut game, &mut records);
    assert_eq!(
        game.execution
            .window()
            .cards()
            .get(object(&game, 3))
            .unwrap()
            .zone(),
        ObjectZone::Hand(A)
    );
    resolve(&mut game, 6, x, &mut records);
    assert_current(&game, x, &[CardType::Creature], Some((2, 5)));
    let boost = game
        .execution
        .continuous_effects()
        .find(|effect| {
            effect.duration == ResolvedDuration::UntilEndOfTurn
                && effect.affected == BTreeSet::from([x])
        })
        .unwrap()
        .id;
    assert_eq!(game.turn().unwrap().position().step, Step::Cleanup);
    assert_eq!(game.turn().unwrap().position().cleanup_iteration, 0);
    assert!(
        matches!(game.execution.window().phase(),WindowPhase::Priority(PriorityPhase::Ready {holder}) if holder==A)
    );
    (game, x, boost, records)
}
#[test]
fn cl_fault_repeated_cleanup_clock() {
    let (mut game, x, boost, mut records) = cleanup_repeat_prefix();
    // The only fault is applied to a genuinely reached NewBoost cleanup-priority
    // state. No scheduler, decision, payload, effect or physical world is restored.
    game.turn
        .as_mut()
        .unwrap()
        .qualification_set_cleanup_iteration(u64::MAX)
        .unwrap();
    accept(&mut game, A, GameCommand::Pass, &mut records);
    let saved = game.clone();
    let saved_events = game.execution.operational_counters().next_event;
    let result = game.advance(GameInput {
        basis: game.basis().clone(),
        actor: B,
        command: GameCommand::Pass,
    });
    // MAP CL-FAULT-REPEATED-CLEANUP-CLOCK assertion 1
    assert_eq!(result, Err(GameError::Turn(TurnError::CleanupExhausted)));
    // MAP CL-FAULT-REPEATED-CLEANUP-CLOCK assertion 2
    assert_eq!(game, saved);
    // MAP CL-FAULT-REPEATED-CLEANUP-CLOCK assertion 3
    assert!(game.execution.continuous_effect(boost).is_some());
    assert_current(&game, x, &[CardType::Creature], Some((2, 5)));
    assert_eq!(
        game.execution.operational_counters().next_event,
        saved_events
    );
    assert_eq!(game.turn().unwrap().position().cleanup_iteration, u64::MAX);
    assert!(game.outcome().is_none());
}
