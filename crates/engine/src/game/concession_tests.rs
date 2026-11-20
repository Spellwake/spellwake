//! Genuine issued root cuts; no fabricated decision is used to concede.
mod bank_expiry;
mod check_replacements;
mod multiplayer_departure;
mod original_roster;
use super::*;
use crate::{
    effects::{
        LibraryTopReply, OptionalEffectChoice, SeedCard, ZoneOptionalAnswer, ZoneReplacementReply,
        ZoneReplacementRequest,
    },
    life::LifeAdjustment,
    mana::{ManaAbility, ManaAbilityRef, ManaCost, ManaOutput, ManaType},
    object_identity::{CardId, ObjectDestination},
    occurrence::RulesRecord,
    program::{
        AffectedPlayers, CreatureTypeSlot, Instruction, InstructionAmount, OrdinaryCondition,
        PlayerRelation, Program, PublicZoneDestination, ResolvingPlayer, Supertype,
        TriggerGrouping, TriggeredAbility, TriggeredAbilityId, ZoneActionKind, ZoneActionPredicate,
        ZoneObjectKind, ZoneReplacementAbility, ZoneReplacementAbilityId, ZoneReplacementChoice,
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
    if next.outcome().is_none() {
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
    let departed = multi
        .advance(input(&multi, A, GameCommand::Concede))
        .unwrap();
    assert_eq!(departed.state().execution().window().active_player(), None);
    assert_eq!(
        departed.state().execution().window().participants(),
        &[B, C]
    );
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
fn concession_common_conclusion_preserves_checked_winner_draw_and_multiplayer_continuation() {
    for (lives, expected, losers) in [
        (vec![0, 20], Some(GameOutcome::Winner(B)), vec![A]),
        (vec![0, 0], Some(GameOutcome::Draw), vec![B, A]),
    ] {
        let state = root(vec![], &lives, None, vec![A, B]);
        assert_eq!(state.outcome(), expected);
        assert_eq!(
            state.conclusion().unwrap().cause(),
            &GameConclusionCause::CheckedLosses {
                players: losers.into_iter().collect()
            }
        );
        assert!(state.pending_decision().is_none());
        assert!(state.pending_turn_work().is_none());
        assert_eq!(
            state.advance(input(&state, A, GameCommand::Concede)),
            Err(GameError::GameEnded)
        );
    }
    let state = root(vec![], &[0, 20, 20], None, vec![A, B, C]);
    assert!(state.conclusion().is_none());
    assert!(state.pending_eliminations().is_empty());
    assert_eq!(state.execution().window().participants(), &[B, C]);
    assert_eq!(
        state
            .advance(input(&state, B, GameCommand::Concede))
            .unwrap()
            .state()
            .outcome(),
        Some(GameOutcome::Winner(C))
    );
    for (lives, outcome) in [
        ([0, 0, 20], GameOutcome::Winner(C)),
        ([0, 0, 0], GameOutcome::Draw),
    ] {
        assert_eq!(
            root(vec![], &lives, None, vec![A, B, C]).outcome(),
            Some(outcome)
        );
    }
    for operation in [
        Instruction::DrawController {
            amount: InstructionAmount::Literal(1.into()),
        },
        Instruction::AdjustControllerLife {
            adjustment: LifeAdjustment::Lose(20.into()),
        },
    ] {
        let state = fixture(vec![operation], BodyTarget::None, vec![]);
        let terminal = resolve(state, None);
        assert_eq!(terminal.outcome(), Some(GameOutcome::Winner(B)));
        assert_eq!(
            terminal
                .conclusion()
                .unwrap()
                .losing_players()
                .collect::<Vec<_>>(),
            vec![A]
        );
    }
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
        vec![Instruction::Surveil {
            player: ResolvingPlayer::TargetPlayer,
            amount: InstructionAmount::Literal(3.into()),
        }],
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
        vec![Instruction::Mill {
            player: ResolvingPlayer::TargetPlayer,
            amount: InstructionAmount::Literal(3.into()),
        }],
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
            vec![Instruction::Discard {
                players: AffectedPlayers::EachPlayer,
                amount: InstructionAmount::Literal(1.into()),
            }],
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
fn concession_optional_payment_and_nested_color_preserve_paid_prefix() {
    let state = resolve(
        fixture(
            vec![
                Instruction::OptionalEffect {
                    chooser: ResolvingPlayer::TargetPlayer,
                },
                Instruction::AdjustControllerLife {
                    adjustment: LifeAdjustment::Gain(1.into()),
                },
            ],
            BodyTarget::SinglePlayer,
            vec![],
        ),
        Some(TargetRef::Player(B)),
    );
    assert!(matches!(
        state.pending_decision().unwrap().frame(),
        RuleFrame::OptionalEffect(_)
    ));
    both(&state);
    let _ = reply(
        &state,
        DecisionReply::OptionalEffect(OptionalEffectChoice::Decline),
    );
    let mana = Program::land(
        BTreeSet::new(),
        vec![ManaAbility {
            id: ManaAbilityId(7),
            output: ManaOutput::ChooseColor {
                amount: 2.into(),
                colors: BTreeSet::from([ManaType::Red, ManaType::Green]),
            },
        }],
    );
    let state = resolve(
        fixture(
            vec![Instruction::OfferManaPayment {
                payer: ResolvingPlayer::TargetPlayer,
                cost: ManaCost::one(ManaType::Red),
                paid_destination: 1,
                declined_destination: 1,
            }],
            BodyTarget::SinglePlayer,
            vec![card(
                70,
                B,
                ObjectDestination::Battlefield { controller: B },
                mana,
            )],
        ),
        Some(TargetRef::Player(B)),
    );
    assert!(matches!(
        state.pending_decision().unwrap().frame(),
        RuleFrame::ResolutionPayment(_)
    ));
    let state = step(
        &state,
        B,
        GameCommand::ActivateMana {
            source: object(&state, 70),
            ability: ManaAbilityRef::Printed(ManaAbilityId(7)),
        },
    );
    assert!(matches!(
        state.pending_decision().unwrap().frame(),
        RuleFrame::ManaColor(_)
    ));
    let state = reply(&state, DecisionReply::ManaColor(ManaType::Red));
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

#[test]
fn concession_creature_type_and_actual_trigger_order_cuts() {
    let state = resolve(
        fixture(
            vec![Instruction::ChooseCreatureType {
                into: CreatureTypeSlot::new(0),
                excluding: BTreeSet::new(),
            }],
            BodyTarget::None,
            vec![],
        ),
        None,
    );
    assert!(matches!(
        state.pending_decision().unwrap().frame(),
        RuleFrame::CreatureType(_)
    ));
    both(&state);
    let mut watcher = Program::artifact(None, vec![]);
    for id in [0, 1] {
        watcher.triggered.push(TriggeredAbility {
            id: TriggeredAbilityId(id),
            condition: OrdinaryCondition::Surveilled(PlayerRelation::Controller),
            grouping: TriggerGrouping::EachOccurrence,
            body: ExecutableBody::new(
                BodyTarget::None,
                vec![Instruction::AdjustControllerLife {
                    adjustment: LifeAdjustment::Gain(1.into()),
                }],
            ),
        });
    }
    let state = resolve(
        fixture(
            vec![Instruction::Surveil {
                player: ResolvingPlayer::Controller,
                amount: InstructionAmount::Literal(1.into()),
            }],
            BodyTarget::None,
            vec![card(
                90,
                A,
                ObjectDestination::Battlefield { controller: A },
                watcher,
            )],
        ),
        None,
    );
    assert!(matches!(
        state.pending_decision().unwrap().frame(),
        RuleFrame::Triggers(_)
    ));
    both(&state);
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

#[test]
fn concession_real_timestamp_copy_target_and_trigger_target_cuts() {
    use crate::token::{TokenCreation, TokenDefinition, TokenName, TokenRecipients};
    let vocabulary = crate::test_support::creature_types::selected_vocabulary().unwrap();
    let token = TokenDefinition::new(
        Program::creature(None, 1.into(), 1.into(), BTreeSet::new()),
        TokenName::Default {
            ordered_subtypes: vec![],
        },
        &vocabulary,
    )
    .unwrap();
    let state = resolve(
        fixture(
            vec![Instruction::CreateTokens {
                creations: vec![TokenCreation {
                    definition: token,
                    amount: InstructionAmount::Literal(2.into()),
                    recipients: TokenRecipients::Controller,
                }],
            }],
            BodyTarget::None,
            vec![],
        ),
        None,
    );
    assert!(matches!(
        state.pending_decision().unwrap().frame(),
        RuleFrame::TimestampOrder(_)
    ));
    both(&state);
    let copier = Program::instant(
        ManaCost::free(),
        ExecutableBody::new(
            BodyTarget::SingleSpell(SpellTargetFilter::CopyableInstructionSpell),
            vec![Instruction::CopySpell],
        ),
    );
    let state = fixture(
        vec![Instruction::AdjustTargetLife {
            adjustment: LifeAdjustment::Gain(1.into()),
        }],
        BodyTarget::SinglePlayer,
        vec![card(2, B, ObjectDestination::Hand, copier)],
    );
    let state = step(
        &state,
        A,
        GameCommand::Cast {
            spell: object(&state, 1),
            target: TargetRef::Player(B),
        },
    );
    let state = step(&state, A, GameCommand::Pass);
    let original = state.execution().window().entries()[0].id();
    let state = step(
        &state,
        B,
        GameCommand::Cast {
            spell: object(&state, 2),
            target: TargetRef::Stack(original),
        },
    );
    let state = step(&state, B, GameCommand::Pass);
    let state = step(&state, A, GameCommand::Pass);
    assert!(matches!(
        state.pending_decision().unwrap().frame(),
        RuleFrame::CopyTargets(_)
    ));
    both(&state);
    let mut watcher = Program::artifact(None, vec![]);
    watcher.triggered.push(TriggeredAbility {
        id: TriggeredAbilityId(0),
        condition: OrdinaryCondition::Surveilled(PlayerRelation::Controller),
        grouping: TriggerGrouping::EachOccurrence,
        body: ExecutableBody::new(
            BodyTarget::SinglePlayer,
            vec![Instruction::AdjustTargetLife {
                adjustment: LifeAdjustment::Gain(1.into()),
            }],
        ),
    });
    let state = resolve(
        fixture(
            vec![Instruction::Surveil {
                player: ResolvingPlayer::Controller,
                amount: InstructionAmount::Literal(1.into()),
            }],
            BodyTarget::None,
            vec![card(
                90,
                A,
                ObjectDestination::Battlefield { controller: A },
                watcher,
            )],
        ),
        None,
    );
    assert!(matches!(
        state.pending_decision().unwrap().frame(),
        RuleFrame::TriggerTarget(_)
    ));
    both(&state);
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
            },
            AttackDeclaration {
                attacker: object(&state, 2),
                defender: B,
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
fn concession_damage_replacements_and_cleanup_work_are_frozen() {
    let target = card(
        10,
        B,
        ObjectDestination::Battlefield { controller: B },
        Program::creature(None, 1.into(), 100.into(), BTreeSet::new()),
    );
    let spells = [
        Instruction::CreatePreventionShield { amount: 1.into() },
        Instruction::CreateDamageMultiplier { factor: 2.into() },
        Instruction::Damage { amount: 5.into() },
    ];
    let cards = spells
        .into_iter()
        .enumerate()
        .map(|(i, op)| {
            card(
                1 + u64::try_from(i).unwrap(),
                A,
                ObjectDestination::Hand,
                Program::instant(
                    ManaCost::free(),
                    ExecutableBody::new(BodyTarget::SingleCreature, vec![op]),
                ),
            )
        })
        .chain([target])
        .collect();
    let mut state = root(cards, &[20, 20], Some(Step::PrecombatMain), vec![A, B]);
    for id in 1..=3 {
        state = step(
            &state,
            A,
            GameCommand::Cast {
                spell: object(&state, id),
                target: TargetRef::Object(object(&state, 10)),
            },
        );
        state = step(&state, A, GameCommand::Pass);
        state = step(&state, B, GameCommand::Pass);
    }
    assert!(matches!(
        state.pending_decision().unwrap().frame(),
        RuleFrame::Damage(_)
    ));
    both(&state);
    let cards = (1..=8)
        .map(|id| {
            card(
                id,
                A,
                ObjectDestination::Hand,
                Program::artifact(None, vec![]),
            )
        })
        .collect();
    let state = root(cards, &[20, 20], Some(Step::Cleanup), vec![A, B]);
    assert_eq!(state.pending_turn_work(), Some(TurnWork::Cleanup));
    assert!(matches!(
        state.pending_decision().unwrap().frame(),
        RuleFrame::Checks(CheckDecision::CleanupDiscard(_))
    ));
    both(&state);
    let terminal = conclude(&state, B).into_state();
    assert_eq!(
        terminal.clone().drive(&mut TransitionFacts::default()),
        Err(GameError::GameEnded)
    );
    let mut other = terminal.clone();
    assert_eq!(
        other.bind_decision(state.pending_decision().unwrap().frame().clone()),
        Err(GameError::GameEnded)
    );
    assert_eq!(other, terminal);
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
            vec![Instruction::ChooseCreatureType {
                into: CreatureTypeSlot::new(0),
                excluding: BTreeSet::new(),
            }],
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
        RuleFrame::CreatureType(_)
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
