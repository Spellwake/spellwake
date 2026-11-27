//! Genuine issued root cuts; no fabricated decision is used to concede.
mod check_replacements;
mod departure_continuation;
mod departure_damage;
use super::*;
use crate::{
    effects::{
        LibraryTopReply, SeedCard, ZoneOptionalAnswer, ZoneReplacementReply, ZoneReplacementRequest,
    },
    mana::{ManaAbility, ManaAbilityRef, ManaCost, ManaOutput, ManaType},
    object_identity::{CardId, ObjectDestination},
    occurrence::RulesRecord,
    program::{
        AffectedPlayers, CreatureTypeSlot, Instruction, PlayerRelation, Program,
        PublicZoneDestination, ResolvingPlayer, Supertype, TriggerGrouping, TriggeredAbility,
        TriggeredAbilityId, ZoneActionKind, ZoneActionPredicate, ZoneObjectKind,
        ZoneReplacementAbility, ZoneReplacementAbilityId, ZoneReplacementChoice,
        ZoneReplacementSubject,
    },
};
use std::sync::Arc;
const A: SeatId = SeatId(41);
const B: SeatId = SeatId(3);
const C: SeatId = SeatId(9);

fn card(id: u64, owner: SeatId, destination: ObjectDestination, program: Program) -> SeedCard {
    SeedCard {
        card: CardId(id),
        owner,
        destination,
        program,
    }
}
fn seed(cards: Vec<SeedCard>, seats: Vec<SeatId>, lives: &[i64]) -> (GameContext, GameSeed) {
    let vocabulary = crate::test_support::creature_types::selected_vocabulary().unwrap();
    let names = Arc::new(spellwake_contracts::names::NameRules::literal_names());
    let context = GameContext::new(
        "concession-mechanisms".into(),
        vocabulary.rules_revision().into(),
        "synthetic".into(),
        "native-concession".into(),
        "private-only".into(),
        vocabulary.id(),
        names.id(),
    )
    .unwrap();
    let order = cards
        .iter()
        .filter(|c| matches!(c.destination, ObjectDestination::Battlefield { .. }))
        .map(|c| c.card)
        .collect();
    let mut seed = GameSeed::with_uniform_life(
        seats,
        A,
        cards,
        order,
        vec![],
        20.into(),
        CombatRules::UnrestrictedFreeForAll,
        vocabulary,
        names,
    );
    for (row, total) in seed.life.iter_mut().zip(lives) {
        row.total = (*total).into();
    }
    (context, seed)
}
fn root(cards: Vec<SeedCard>, lives: &[i64], start: Option<Step>, seats: Vec<SeatId>) -> GameState {
    let (context, seed) = seed(cards, seats, lives);
    let build = || match start {
        Some(step) => GameState::initialize_scheduled(
            GameId(1043),
            context.clone(),
            seed.clone(),
            TurnStart { number: 1, step },
        ),
        None => GameState::initialize(GameId(1043), context.clone(), seed.clone()),
    };
    let actual = build();
    assert_eq!(actual, build());
    actual.unwrap().into_state()
}
fn fixture(body: Vec<Instruction>, target: BodyTarget, mut cards: Vec<SeedCard>) -> GameState {
    cards.insert(
        0,
        card(
            1,
            A,
            ObjectDestination::Hand,
            Program::instant(ManaCost::free(), ExecutableBody::new(target, body)),
        ),
    );
    root(cards, &[20, 20], Some(Step::PrecombatMain), vec![A, B])
}
fn object(state: &GameState, id: u64) -> ObjectId {
    state
        .execution()
        .window()
        .cards()
        .current_identity(CardId(id))
        .unwrap()
}
fn input(state: &GameState, actor: SeatId, command: GameCommand) -> GameInput {
    GameInput {
        basis: state.basis().clone(),
        actor,
        command,
    }
}
fn step(state: &GameState, actor: SeatId, command: GameCommand) -> GameState {
    let before = state.clone();
    let issued = input(state, actor, command);
    let actual = state.advance(issued.clone());
    assert_eq!(actual, state.advance(issued));
    assert_eq!(*state, before);
    let next = actual.unwrap().into_state();
    if next.outcome().is_none() && next.execution().window().original_participants() == [A, B] {
        both(&next);
    }
    next
}
fn reply(state: &GameState, choice: DecisionReply) -> GameState {
    let pending = state.pending_decision().unwrap();
    step(
        state,
        pending.chooser(),
        GameCommand::Reply {
            decision: pending.id(),
            choice,
        },
    )
}
fn resolve(state: GameState, target: Option<TargetRef>) -> GameState {
    both(&state);
    let spell = object(&state, 1);
    let command = target.map_or(
        GameCommand::CastUntargeted {
            spell,
            payment: None,
        },
        |target| GameCommand::Cast { spell, target },
    );
    let state = step(&state, A, command);
    let state = step(&state, A, GameCommand::Pass);
    step(&state, B, GameCommand::Pass)
}
fn conclude(state: &GameState, actor: SeatId) -> NativeCandidate {
    let original = state.clone();
    let issued = input(state, actor, GameCommand::Concede);
    let actual = state.advance(issued.clone());
    assert_eq!(actual, state.advance(issued));
    assert_eq!(*state, original);
    let accepted = actual.unwrap();
    let terminal = accepted.state();
    assert_eq!(
        accepted.base(),
        &CandidateBase::Advance(state.basis().clone())
    );
    assert_eq!(terminal.basis.revision, state.basis.revision + 1);
    assert_eq!(
        terminal.outcome(),
        Some(GameOutcome::Winner(if actor == A { B } else { A }))
    );
    let conclusion = terminal.conclusion().unwrap();
    assert_eq!(
        conclusion.cause(),
        &GameConclusionCause::Conceded { player: actor }
    );
    assert_eq!(conclusion.losing_players().collect::<Vec<_>>(), vec![actor]);
    assert_eq!(
        accepted.rules_journal().records,
        vec![RulesRecord::GameConcluded {
            conclusion: conclusion.clone()
        }]
    );
    let mut no_terminal_fact = accepted.facts.clone();
    no_terminal_fact.rules_journal.records.clear();
    assert_eq!(no_terminal_fact, TransitionFacts::default());
    let mut frozen = terminal.clone();
    frozen.basis = state.basis.clone();
    frozen.conclusion = state.conclusion.clone();
    assert_eq!(
        frozen, *state,
        "all owned work/resources and accepted prefixes frozen"
    );
    assert!(terminal.pending_decision().is_none());
    assert!(terminal.pending_turn_work().is_none());
    assert_eq!(
        terminal.retained_terminal_decision(),
        state.pending.as_ref()
    );
    if let Some(pending) = state.pending.as_ref() {
        assert_ne!(pending.basis(), terminal.basis());
    }
    for command in [
        GameCommand::Pass,
        GameCommand::Reply {
            decision: state
                .pending
                .as_ref()
                .map_or(DecisionId(0), DecisionState::id),
            choice: DecisionReply::Discard(vec![]),
        },
    ] {
        assert_eq!(
            terminal.advance(input(terminal, actor, command)),
            Err(GameError::GameEnded)
        );
    }
    if let Some(card) = state.execution().window().cards().iter().next() {
        assert_eq!(
            terminal.advance(input(
                terminal,
                actor,
                GameCommand::CastUntargeted {
                    spell: card.id(),
                    payment: None
                }
            )),
            Err(GameError::GameEnded)
        );
    }
    assert_eq!(
        terminal.supply_entropy(terminal.basis(), vec![0]),
        Err(GameError::GameEnded)
    );
    accepted
}
fn both(state: &GameState) {
    for actor in [A, B] {
        conclude(state, actor);
    }
}

#[test]
fn concession_own_opponent_priority_and_serialized_concessions() {
    let state = root(vec![], &[20, 20], None, vec![A, B]);
    both(&state);
    let passed = step(&state, A, GameCommand::Pass);
    both(&passed);
    let terminal = conclude(&passed, B).into_state();
    for actor in [A, B] {
        assert_eq!(
            terminal.advance(input(&terminal, actor, GameCommand::Concede)),
            Err(GameError::GameEnded)
        );
    }
    assert_eq!(
        terminal.advance(input(&passed, A, GameCommand::Concede)),
        Err(GameError::StaleRevision)
    );
}

#[test]
fn concession_basis_actor_terminal_scope_and_capacity_error_order() {
    let state = root(vec![], &[20, 20], None, vec![A, B]);
    let reject = |state: &GameState, issued: GameInput, expected: GameError| {
        let before = state.clone();
        assert_eq!(state.advance(issued.clone()), Err(expected.clone()));
        assert_eq!(state.advance(issued), Err(expected));
        assert_eq!(*state, before);
    };
    let mut bad = input(&state, C, GameCommand::Concede);
    bad.basis.game = GameId(0);
    bad.basis.epoch = 1;
    bad.basis.revision = 1;
    reject(&state, bad, GameError::WrongGame);
    let mut bad = input(&state, C, GameCommand::Concede);
    bad.basis.context = GameContext::new(
        "other".into(),
        state.basis.context.rules().into(),
        "c".into(),
        "p".into(),
        "p".into(),
        state.basis.context.creature_vocabulary_id(),
        state.basis.context.name_rules_id(),
    )
    .unwrap();
    reject(&state, bad, GameError::WrongContext);
    let mut bad = input(&state, C, GameCommand::Concede);
    bad.basis.epoch = 1;
    bad.basis.revision = 1;
    reject(&state, bad, GameError::WrongEpoch);
    let mut bad = input(&state, C, GameCommand::Concede);
    bad.basis.revision = 1;
    reject(&state, bad, GameError::StaleRevision);
    reject(
        &state,
        input(&state, C, GameCommand::Concede),
        GameError::UnknownActor,
    );
    let terminal = conclude(&state, A).into_state();
    reject(
        &terminal,
        input(&terminal, C, GameCommand::Concede),
        GameError::UnknownActor,
    );
    for command in [
        GameCommand::Concede,
        GameCommand::Pass,
        GameCommand::Reply {
            decision: DecisionId(0),
            choice: DecisionReply::Discard(vec![]),
        },
    ] {
        reject(
            &terminal,
            input(&terminal, B, command),
            GameError::GameEnded,
        );
    }
    assert_eq!(
        terminal.supply_entropy(terminal.basis(), vec![1, 2]),
        Err(GameError::GameEnded)
    );
    assert_eq!(
        terminal.supply_entropy(state.basis(), vec![1]),
        Err(GameError::StaleRevision)
    );
    let mut exhausted = state.clone();
    exhausted.basis.revision = u64::MAX;
    reject(
        &exhausted,
        input(&exhausted, A, GameCommand::Concede),
        GameError::RevisionExhausted,
    );
    let mut multi = root(vec![], &[20, 20, 20], None, vec![A, B, C]);
    let conceded = step(&multi, A, GameCommand::Concede);
    assert_eq!(conceded.execution().window().participants(), &[B, C]);
    assert_eq!(conceded.outcome(), None);
    multi.basis.revision = u64::MAX;
    reject(
        &multi,
        input(&multi, A, GameCommand::Concede),
        GameError::RevisionExhausted,
    );
}

#[test]
fn concession_bypasses_unrelated_exhausted_child_allocators() {
    use crate::effects::private_qualification::QualificationFault as F;
    let state = root(vec![], &[20, 20], None, vec![A, B]);
    for fault in [
        F::Revision(u64::MAX),
        F::WindowRevision(u64::MAX),
        F::PriorityRevision(u64::MAX),
        F::ObjectRevision(u64::MAX),
        F::ObjectIdentity(u64::MAX),
        F::StackEntry(u64::MAX),
        F::Event(u64::MAX),
        F::Instance(u64::MAX),
        F::Wave(u64::MAX),
    ] {
        let mut accepted = state.clone();
        accepted.execution.qualification_set_fault(fault);
        accepted.next_decision = u64::MAX;
        both(&accepted);
    }
}

#[test]
fn concession_does_not_promote_prospective_losses_in_real_legend_choice() {
    let mut legendary = Program::artifact(None, vec![]);
    legendary.supertypes.insert(Supertype::Legendary);
    legendary.names = spellwake_contracts::names::ObjectNames::new(vec![
        spellwake_contracts::names::EnglishCardName::new("Same".into()).unwrap(),
    ])
    .unwrap();
    let state = root(
        (1..=2)
            .map(|id| {
                card(
                    id,
                    A,
                    ObjectDestination::Battlefield { controller: A },
                    legendary.clone(),
                )
            })
            .collect(),
        &[0, 0],
        None,
        vec![A, B],
    );
    assert!(matches!(
        state.pending_decision().unwrap().frame(),
        RuleFrame::Checks(CheckDecision::Legend(_))
    ));
    assert!(state.execution().state_based_losses().is_empty());
    assert!(state.conclusion().is_none());
    both(&state);
    assert_eq!(
        conclude(&state, A).state().outcome(),
        Some(GameOutcome::Winner(B))
    );
}

#[test]
fn simultaneous_departures_create_independent_surviving_triggers() {
    let d = SeatId(19);
    let mut watcher = Program::creature(None, 1.into(), 1.into(), BTreeSet::new());
    watcher.triggered.push(TriggeredAbility {
        id: TriggeredAbilityId(0),
        condition: crate::program::TriggerSpec::new(crate::program::TriggerEvent::PlayerLosesGame(
            PlayerRelation::AnyParticipant,
        )),
        grouping: TriggerGrouping::EachOccurrence,
        body: ExecutableBody::new(
            BodyTarget::None,
            vec![Instruction::MutateObject(Box::new(
                crate::toolbox::MutateObjectSpec::all(
                    crate::toolbox::ObjectQuery::Source,
                    crate::toolbox::ObjectMutation::Counters(crate::toolbox::CounterOp::Add {
                        kind: crate::effects::CounterKind::PlusOne,
                        amount: crate::program::ValueExpr::literal(13),
                    }),
                ),
            ))],
        ),
    });
    let state = root(
        vec![
            card(
                1,
                A,
                ObjectDestination::Battlefield { controller: A },
                watcher.clone(),
            ),
            card(
                2,
                B,
                ObjectDestination::Battlefield { controller: B },
                watcher,
            ),
        ],
        &[20, 0, 0, 20],
        None,
        vec![A, B, C, d],
    );
    assert_eq!(state.execution().window().participants(), &[A, d]);
    let RuleFrame::Triggers(frame) = state.pending_decision().unwrap().frame() else {
        panic!("two separate losses must reach the surviving controller's trigger ordering");
    };
    assert_eq!(frame.chooser(), A);
    assert_eq!(frame.instances().len(), 2);
    let mut state = reply(
        &state,
        DecisionReply::OrderTriggers(frame.instances().to_vec()),
    );
    for total in [13, 26] {
        state = step(&step(&state, A, GameCommand::Pass), d, GameCommand::Pass);
        let counters = state
            .execution()
            .power_toughness_counters(object(&state, 1))
            .unwrap();
        assert_eq!(counters.plus_one, total.into());
    }
}

#[test]
fn departure_ends_temporary_control_before_exiling_remaining_objects() {
    use crate::characteristics::{
        ControlRecipient, ResolvedDuration, ResolvedPartSpec, ResolvedPartsSpec,
    };
    let creature = Program::creature(None, 2.into(), 2.into(), BTreeSet::new());
    let control = Program::instant(
        ManaCost::free(),
        ExecutableBody::new(
            BodyTarget::SingleCreature,
            vec![{
                struct _Tmp {
                    parts: crate::characteristics::ResolvedPartsSpec,
                    duration: crate::characteristics::ResolvedDuration,
                }
                let _t = _Tmp {
                    parts: ResolvedPartsSpec(vec![ResolvedPartSpec::Control(
                        ControlRecipient::ResolvingController,
                    )]),
                    duration: ResolvedDuration::UntilEndOfTurn,
                };
                crate::program::Instruction::ApplyContinuous(Box::new(
                    crate::toolbox::ApplyContinuousSpec {
                        selector: Some(crate::toolbox::ObjectQuery::Targets),
                        payload: crate::toolbox::ContinuousPayload::ResolvedParts(Box::new(
                            _t.parts,
                        )),
                        duration: _t.duration,
                        condition: None,
                        one_shot: false,
                        as_emblem: None,
                        emblem_label: None,
                    },
                ))
            }],
        ),
    );
    let state = root(
        vec![
            card(1, A, ObjectDestination::Hand, control),
            card(
                2,
                B,
                ObjectDestination::Battlefield { controller: B },
                creature,
            ),
        ],
        &[20, 20, 20],
        None,
        vec![A, B, C],
    );
    let target = object(&state, 2);
    let state = step(
        &state,
        A,
        GameCommand::Cast {
            spell: object(&state, 1),
            target: TargetRef::Object(target),
        },
    );
    let state = [A, B, C]
        .into_iter()
        .fold(state, |state, actor| step(&state, actor, GameCommand::Pass));
    assert_eq!(
        state
            .execution()
            .window()
            .cards()
            .get(target)
            .unwrap()
            .controller(),
        Some(A)
    );
    let state = step(&state, A, GameCommand::Concede);
    let restored = state.execution().window().cards().get(target).unwrap();
    assert_eq!(restored.zone(), ObjectZone::Battlefield);
    assert_eq!(restored.controller(), Some(B));
}

#[test]
fn a_permanent_is_exiled_when_its_default_controller_left_before_the_effect_expired() {
    use crate::characteristics::{
        ControlRecipient, ResolvedDuration, ResolvedPartSpec, ResolvedPartsSpec,
    };
    let control = Program::instant(
        ManaCost::free(),
        ExecutableBody::new(
            BodyTarget::SingleCreature,
            vec![{
                struct _Tmp {
                    parts: crate::characteristics::ResolvedPartsSpec,
                    duration: crate::characteristics::ResolvedDuration,
                }
                let _t = _Tmp {
                    parts: ResolvedPartsSpec(vec![ResolvedPartSpec::Control(
                        ControlRecipient::ResolvingController,
                    )]),
                    duration: ResolvedDuration::UntilEndOfTurn,
                };
                crate::program::Instruction::ApplyContinuous(Box::new(
                    crate::toolbox::ApplyContinuousSpec {
                        selector: Some(crate::toolbox::ObjectQuery::Targets),
                        payload: crate::toolbox::ContinuousPayload::ResolvedParts(Box::new(
                            _t.parts,
                        )),
                        duration: _t.duration,
                        condition: None,
                        one_shot: false,
                        as_emblem: None,
                        emblem_label: None,
                    },
                ))
            }],
        ),
    );
    let creature = Program::creature(None, 2.into(), 2.into(), BTreeSet::new());
    let state = root(
        vec![
            card(1, B, ObjectDestination::Hand, control),
            card(
                2,
                C,
                ObjectDestination::Battlefield { controller: A },
                creature,
            ),
        ],
        &[20, 20, 20],
        Some(Step::End),
        vec![A, B, C],
    );
    let state = step(&state, A, GameCommand::Pass);
    let state = step(
        &state,
        B,
        GameCommand::Cast {
            spell: object(&state, 1),
            target: TargetRef::Object(object(&state, 2)),
        },
    );
    let state = [B, C, A]
        .into_iter()
        .fold(state, |state, actor| step(&state, actor, GameCommand::Pass));
    let state = step(&state, A, GameCommand::Concede);
    assert_eq!(
        state
            .execution()
            .window()
            .cards()
            .get(object(&state, 2))
            .unwrap()
            .controller(),
        Some(B)
    );
    let state = step(&step(&state, B, GameCommand::Pass), C, GameCommand::Pass);
    assert_eq!(
        state
            .execution()
            .window()
            .cards()
            .get(object(&state, 2))
            .unwrap()
            .zone(),
        ObjectZone::Exile
    );
}

#[test]
fn skipped_departed_turn_expires_its_effects_and_returns_phased_permanents() {
    use crate::characteristics::{
        ControlRecipient, PowerToughness, ResolvedDuration, ResolvedPartSpec, ResolvedPartsSpec,
    };

    let d = SeatId(19);
    let instant = |instruction| {
        Program::instant(
            ManaCost::free(),
            ExecutableBody::new(BodyTarget::SingleCreature, vec![instruction]),
        )
    };
    let control = instant({
        struct _Tmp {
            parts: crate::characteristics::ResolvedPartsSpec,
            duration: crate::characteristics::ResolvedDuration,
        }
        let _t = _Tmp {
            parts: ResolvedPartsSpec(vec![ResolvedPartSpec::Control(
                ControlRecipient::ResolvingController,
            )]),
            duration: ResolvedDuration::UntilEndOfTurn,
        };
        crate::program::Instruction::ApplyContinuous(Box::new(
            crate::toolbox::ApplyContinuousSpec {
                selector: Some(crate::toolbox::ObjectQuery::Targets),
                payload: crate::toolbox::ContinuousPayload::ResolvedParts(Box::new(_t.parts)),
                duration: _t.duration,
                condition: None,
                one_shot: false,
                as_emblem: None,
                emblem_label: None,
            },
        ))
    });
    let phase = instant(crate::program::Instruction::MutateObject(Box::new(
        crate::toolbox::MutateObjectSpec::all(
            crate::toolbox::ObjectQuery::Targets,
            crate::toolbox::ObjectMutation::Orientation(crate::toolbox::OrientationOp::PhaseOut),
        ),
    )));
    let boost = instant({
        struct _Tmp {
            parts: crate::characteristics::ResolvedPartsSpec,
            duration: crate::characteristics::ResolvedDuration,
        }
        let _t = _Tmp {
            parts: ResolvedPartsSpec(vec![ResolvedPartSpec::ModifyPT(PowerToughness {
                power: 3.into(),
                toughness: 3.into(),
            })]),
            duration: ResolvedDuration::ThroughNextTurn {
                player: B,
                created_in_turn: 1,
            },
        };
        crate::program::Instruction::ApplyContinuous(Box::new(
            crate::toolbox::ApplyContinuousSpec {
                selector: Some(crate::toolbox::ObjectQuery::Targets),
                payload: crate::toolbox::ContinuousPayload::ResolvedParts(Box::new(_t.parts)),
                duration: _t.duration,
                condition: None,
                one_shot: false,
                as_emblem: None,
                emblem_label: None,
            },
        ))
    });
    let creature = Program::creature(None, 2.into(), 2.into(), BTreeSet::new());
    let mut state = root(
        vec![
            card(1, B, ObjectDestination::Hand, control),
            card(2, B, ObjectDestination::Hand, phase),
            card(3, A, ObjectDestination::Hand, boost),
            card(
                4,
                d,
                ObjectDestination::Battlefield { controller: d },
                creature.clone(),
            ),
            card(
                5,
                A,
                ObjectDestination::Battlefield { controller: A },
                creature,
            ),
        ],
        &[20, 20, 20, 20],
        Some(Step::End),
        vec![A, B, C, d],
    );
    let phased = object(&state, 4);
    let boosted = object(&state, 5);
    for (actor, card, target) in [(B, 1, phased), (B, 2, phased), (A, 3, boosted)] {
        if actor == B {
            state = step(&state, A, GameCommand::Pass);
        }
        state = step(
            &state,
            actor,
            GameCommand::Cast {
                spell: object(&state, card),
                target: TargetRef::Object(target),
            },
        );
        for _ in 0..4 {
            let WindowPhase::Priority(PriorityPhase::Ready { holder }) =
                state.execution().window().phase()
            else {
                panic!("spell resolution advances through ordinary priority");
            };
            state = step(&state, holder, GameCommand::Pass);
        }
    }
    let state = step(&state, B, GameCommand::Concede);
    assert_eq!(
        state.execution().window().cards().phased_out_under(phased),
        Some(B)
    );
    assert_eq!(
        state.execution().creature(boosted).unwrap().unwrap().power,
        5.into()
    );
    let state = step(&step(&state, A, GameCommand::Pass), C, GameCommand::Pass);
    let candidate = state.advance(input(&state, d, GameCommand::Pass)).unwrap();
    assert!(candidate.rules_journal().records.iter().any(|record| {
        matches!(record, RulesRecord::Event(event) if event.occurrences.iter().any(|row|
            matches!(&row.value, crate::occurrence::RulesOccurrence::TurnsSkipped { players, expired_effects }
                if players == &[B].into() && expired_effects.len() == 1)))
    }));
    let state = candidate.state();
    assert_eq!(state.execution().window().active_participant(), Some(C));
    assert_eq!(
        state
            .execution()
            .window()
            .cards()
            .get(phased)
            .unwrap()
            .controller(),
        Some(d)
    );
    assert_eq!(
        state.execution().creature(boosted).unwrap().unwrap().power,
        2.into()
    );
}

#[test]
fn concession_library_partition_replacement_optional_and_final_order_cuts() {
    let library = (10..=13)
        .map(|id| {
            card(
                id,
                B,
                ObjectDestination::LibraryBottom,
                Program::artifact(None, vec![]),
            )
        })
        .collect::<Vec<_>>();
    let state = fixture(
        vec![Instruction::MutatePlayer(Box::new(
            crate::toolbox::MutatePlayerSpec::surveil(
                ResolvingPlayer::TargetPlayer,
                crate::program::ValueExpr::Literal(3.into()),
            ),
        ))],
        BodyTarget::SinglePlayer,
        library.clone(),
    );
    let state = resolve(state, Some(TargetRef::Player(B)));
    let RuleFrame::LibraryTop(frame) = state.pending_decision().unwrap().frame() else {
        panic!()
    };
    let state = reply(
        &state,
        DecisionReply::LibraryTop(LibraryTopReply::Surveil {
            top_first: vec![],
            to_graveyard: frame.domain_top_first().to_vec(),
        }),
    );
    assert!(matches!(
        state.pending_decision().unwrap().frame(),
        RuleFrame::ResolutionGraveyardOrder(_)
    ));
    both(&state);
    let mut p = Program::enchantment(None);
    p.zone_replacements.push(ZoneReplacementAbility {
        id: ZoneReplacementAbilityId(0),
        actions: ZoneActionPredicate::OneOf(BTreeSet::from([ZoneActionKind::EffectMill])),
        objects: ZoneObjectKind::Card,
        subject: ZoneReplacementSubject::AnyObject,
        destination: PublicZoneDestination::Graveyard,
        replace_with: PublicZoneDestination::Exile,
        choice: ZoneReplacementChoice::OptionalAffectedPlayer,
    });
    let mut cards = library;
    cards.push(card(
        90,
        A,
        ObjectDestination::Battlefield { controller: A },
        p,
    ));
    let state = fixture(
        vec![Instruction::MutatePlayer(Box::new(
            crate::toolbox::MutatePlayerSpec::mill(
                ResolvingPlayer::TargetPlayer,
                crate::program::ValueExpr::Literal(3.into()),
            ),
        ))],
        BodyTarget::SinglePlayer,
        cards,
    );
    let state = resolve(state, Some(TargetRef::Player(B)));
    let RuleFrame::ZoneReplacement(frame) = state.pending_decision().unwrap().frame() else {
        panic!()
    };
    let ZoneReplacementRequest::Select { applications } = frame.request() else {
        panic!()
    };
    let chosen = applications[0];
    let state = reply(
        &state,
        DecisionReply::ZoneReplacement(ZoneReplacementReply::Select(chosen)),
    );
    assert!(matches!(
        state.pending_decision().unwrap().frame(),
        RuleFrame::ZoneReplacement(_)
    ));
    let mut state = reply(
        &state,
        DecisionReply::ZoneReplacement(ZoneReplacementReply::Optional(ZoneOptionalAnswer::Decline)),
    );
    both(&state);
    while let RuleFrame::ZoneReplacement(frame) = state.pending_decision().unwrap().frame() {
        let choice = match frame.request() {
            ZoneReplacementRequest::Select { applications } => {
                ZoneReplacementReply::Select(applications[0])
            }
            ZoneReplacementRequest::Devour { .. }
            | ZoneReplacementRequest::DevourGraveyardOrder { .. }
            | ZoneReplacementRequest::EntryCopy { .. }
            | ZoneReplacementRequest::EntryController { .. } => {
                panic!("fixture has no controller-changing entrants")
            }
            ZoneReplacementRequest::ConfirmOptional { .. } => {
                ZoneReplacementReply::Optional(ZoneOptionalAnswer::Decline)
            }
        };
        state = reply(&state, DecisionReply::ZoneReplacement(choice));
    }
    assert!(matches!(
        state.pending_decision().unwrap().frame(),
        RuleFrame::ResolutionGraveyardOrder(_)
    ));
    both(&state);
}

#[test]
fn concession_other_player_selection_retains_accepted_prefix() {
    let cards = [(A, 10), (A, 11), (B, 20), (B, 21)]
        .into_iter()
        .map(|(owner, id)| {
            card(
                id,
                owner,
                ObjectDestination::Hand,
                Program::artifact(None, vec![]),
            )
        })
        .collect();
    let state = resolve(
        fixture(
            vec![Instruction::ZoneOp(Box::new(
                crate::toolbox::ZoneOpSpec::discard(
                    AffectedPlayers::EachPlayer,
                    crate::program::ValueExpr::Literal(1.into()),
                ),
            ))],
            BodyTarget::None,
            cards,
        ),
        None,
    );
    let RuleFrame::ResolutionSelect(frame) = state.pending_decision().unwrap().frame() else {
        panic!()
    };
    assert_eq!(frame.chooser(), A);
    let state = reply(
        &state,
        DecisionReply::SelectResolution(vec![frame.domain()[0]]),
    );
    assert_eq!(state.pending_decision().unwrap().chooser(), B);
    both(&state);
    let accepted = conclude(&state, A);
    assert_eq!(
        accepted.state().retained_terminal_decision(),
        state.pending_decision()
    );
    assert!(state.execution().resolution_selection_state().is_some());
}

#[test]
fn concession_opening_declare_bottom_entropy_shortage_and_host_append() {
    let cards = [A, B]
        .into_iter()
        .flat_map(|owner| {
            (0..12).map(move |n| {
                card(
                    100 + u64::from(owner.0) * 100 + n,
                    owner,
                    ObjectDestination::LibraryBottom,
                    Program::artifact(None, vec![]),
                )
            })
        })
        .collect();
    let (context, seed) = seed(cards, vec![A, B], &[20, 20]);
    let setup = OpeningSetup {
        seed,
        entropy: EntropyTape::new(vec![0; 22]),
    };
    let initial = GameState::initialize_opening(GameId(1044), context.clone(), setup.clone());
    assert_eq!(
        initial,
        GameState::initialize_opening(GameId(1044), context, setup)
    );
    let state = initial.unwrap().into_state();
    assert_eq!(state.entropy().unwrap().remaining(), 0);
    both(&state);
    let state = reply(&state, DecisionReply::Mulligan(MulliganChoice::Mulligan));
    let pending = state.pending_decision().unwrap();
    let failed = input(
        &state,
        pending.chooser(),
        GameCommand::Reply {
            decision: pending.id(),
            choice: DecisionReply::Mulligan(MulliganChoice::Keep),
        },
    );
    let before = state.clone();
    assert_eq!(
        state.advance(failed.clone()),
        Err(GameError::Effect(EffectError::Window(
            crate::stack_window::StackWindowError::Objects(
                crate::object_identity::ObjectIdentityError::Entropy(EntropyError::Exhausted)
            )
        )))
    );
    assert_eq!(state.advance(failed.clone()), state.advance(failed));
    assert_eq!(state, before);
    both(&state);
    let supplied = state.supply_entropy(state.basis(), vec![0; 11]).unwrap();
    assert_eq!(
        state.supply_entropy(state.basis(), vec![0; 11]),
        Ok(supplied.clone())
    );
    let state = supplied.into_state();
    let state = reply(&state, DecisionReply::Mulligan(MulliganChoice::Keep));
    assert!(matches!(
        state.pending_decision().unwrap().frame(),
        RuleFrame::Opening(OpeningDecision::Bottom { .. })
    ));
    both(&state);
}

#[test]
fn concession_isolated_input_root_record_journal_and_candidate_legacy_refusals() {
    use crate::test_support::{
        current_profile::applicability::{ApplicabilityError, NativeCaptureValue},
        packet::AliasContext,
        projection,
    };
    let source = root(vec![], &[20, 20], None, vec![A, B]);
    let candidate = conclude(&source, A);
    let aliases = AliasContext::default();
    let error = projection::game_error(&GameError::UnsupportedConcessionScope);
    assert_eq!(
        error["error_path"],
        serde_json::json!(["UnsupportedConcessionScope"])
    );
    assert_eq!(error["error_leaf"]["domain"], "GameError");
    assert_eq!(error["error_leaf"]["variant"], "UnsupportedConcessionScope");
    assert!(error["error_leaf"]["data"].is_null());
    for actor in [A, C] {
        let issued = input(&source, actor, GameCommand::Concede);
        assert_eq!(issued.check_current10(), Err(ApplicabilityError));
        assert_eq!(projection::input(&issued)["command"]["tag"], "Concede");
    }
    source.check_current10().unwrap();
    assert_eq!(candidate.state().check_current10(), Err(ApplicabilityError));
    assert!(projection::state(candidate.state(), &aliases).is_err());
    assert!(projection::candidate(&candidate, Some(&source), &aliases).is_err());
    let record = &candidate.rules_journal().records[0];
    assert_eq!(record.check_current10(), Err(ApplicabilityError));
    assert_eq!(
        candidate.rules_journal().check_current10(),
        Err(ApplicabilityError)
    );
    assert_eq!(candidate.check_current10(), Err(ApplicabilityError));
    let mut isolated_record = candidate.clone();
    isolated_record.next = source.clone();
    assert_eq!(isolated_record.check_current10(), Err(ApplicabilityError));
    assert!(projection::candidate(&isolated_record, Some(&source), &aliases).is_err());
    let mut isolated_state = candidate.clone();
    isolated_state.facts = TransitionFacts::default();
    assert_eq!(isolated_state.check_current10(), Err(ApplicabilityError));
    assert!(projection::candidate(&isolated_state, Some(&source), &aliases).is_err());
    let checked = root(vec![], &[0, 20], None, vec![A, B]);
    checked.check_current10().unwrap();
    projection::state(&checked, &aliases).unwrap();
}

fn round(mut state: GameState) -> GameState {
    for _ in 0..2 {
        let WindowPhase::Priority(PriorityPhase::Ready { holder }) =
            state.execution().window().phase()
        else {
            panic!("actual ready priority");
        };
        state = step(&state, holder, GameCommand::Pass);
    }
    state
}

#[test]
fn concession_real_combat_attack_block_and_damage_assignment_cuts() {
    use crate::combat::{AttackDeclaration, BlockDeclaration};
    let mut cards = (1..=4)
        .map(|id| {
            let owner = if id <= 2 { A } else { B };
            card(
                id,
                owner,
                ObjectDestination::Battlefield { controller: owner },
                Program::creature(None, 2.into(), 4.into(), BTreeSet::new()),
            )
        })
        .collect::<Vec<_>>();
    cards.push(card(
        100,
        A,
        ObjectDestination::LibraryBottom,
        Program::artifact(None, vec![]),
    ));
    let mut state = root(cards, &[20, 20], Some(Step::Untap), vec![A, B]);
    while state.turn().unwrap().position().step != Step::BeginningCombat {
        state = round(state);
    }
    let state = round(state);
    assert!(matches!(
        state.pending_decision().unwrap().frame(),
        RuleFrame::Combat(CombatDecision::Attackers(_))
    ));
    both(&state);
    let state = reply(
        &state,
        DecisionReply::DeclareAttackers(vec![
            AttackDeclaration {
                attacker: object(&state, 1),
                defender: B,
                defending_planeswalker: None,
            },
            AttackDeclaration {
                attacker: object(&state, 2),
                defender: B,
                defending_planeswalker: None,
            },
        ]),
    );
    let state = round(state);
    assert!(matches!(
        state.pending_decision().unwrap().frame(),
        RuleFrame::Combat(CombatDecision::Blockers(_))
    ));
    both(&state);
    let state = reply(
        &state,
        DecisionReply::DeclareBlockers(vec![
            BlockDeclaration {
                blocker: object(&state, 3),
                attacker: object(&state, 1),
            },
            BlockDeclaration {
                blocker: object(&state, 4),
                attacker: object(&state, 1),
            },
        ]),
    );
    let state = round(state);
    assert!(matches!(
        state.pending_decision().unwrap().frame(),
        RuleFrame::Combat(CombatDecision::DamageAssignments(_))
    ));
    both(&state);
}

#[test]
fn concession_keeps_actual_cast_payment_and_tap_at_a_later_choice() {
    let mana = Program::land(
        BTreeSet::new(),
        vec![ManaAbility {
            id: ManaAbilityId(7),
            output: ManaOutput::Fixed(BTreeMap::from([(ManaType::Red, 1.into())])),
        }],
    );
    let spell = Program::instant(
        ManaCost::one(ManaType::Red),
        ExecutableBody::new(
            BodyTarget::None,
            vec![Instruction::PromptChoice(Box::new(
                crate::toolbox::PromptChoiceSpec {
                    chooser: crate::toolbox::PlayerSelector::Resolving(
                        crate::program::ResolvingPlayer::Controller,
                    ),
                    domain: crate::toolbox::PromptDomain::Subtype {
                        kind: crate::toolbox::SubtypeChoiceKind::Creature,
                        excluding: BTreeSet::new(),
                        legacy_slot: Some(CreatureTypeSlot::new(0)),
                    },
                    category: crate::toolbox::ChoiceCategory::Generic,
                    bind_to: crate::toolbox::BindingId(0),
                    bind_unchosen: None,
                    tag_partition: None,
                    auto_resolve_trivial: false,
                    option_branches: Vec::new(),
                },
            ))],
        ),
    );
    let state = root(
        vec![
            card(1, A, ObjectDestination::Hand, spell),
            card(
                70,
                A,
                ObjectDestination::Battlefield { controller: A },
                mana,
            ),
        ],
        &[20, 20],
        Some(Step::PrecombatMain),
        vec![A, B],
    );
    let state = step(
        &state,
        A,
        GameCommand::ActivateMana {
            source: object(&state, 70),
            ability: ManaAbilityRef::Printed(ManaAbilityId(7)),
        },
    );
    assert_eq!(state.execution().mana(A, ManaType::Red), 1.into());
    let issued = input(
        &state,
        A,
        GameCommand::CastUntargeted {
            spell: object(&state, 1),
            payment: Some(ManaPayment {
                from_lots: Default::default(),
                spend: vec![crate::mana::ManaSpend {
                    mana_type: ManaType::Red,
                    amount: 1.into(),
                }],
            }),
        },
    );
    let candidate = state.advance(issued.clone()).unwrap();
    assert_eq!(state.advance(issued), Ok(candidate.clone()));
    assert_eq!(
        candidate.mana_payment().unwrap().cost,
        ManaCost::one(ManaType::Red)
    );
    let state = candidate.into_state();
    let state = step(&state, A, GameCommand::Pass);
    let state = step(&state, B, GameCommand::Pass);
    assert!(matches!(
        state.pending_decision().unwrap().frame(),
        RuleFrame::PromptChoice(_)
    ));
    assert_eq!(state.execution().mana(A, ManaType::Red), 0.into());
    assert_eq!(
        state
            .execution()
            .window()
            .cards()
            .get(object(&state, 70))
            .unwrap()
            .tapped(),
        Some(true)
    );
    both(&state);
}
