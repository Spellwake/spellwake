//! Synthetic native actions and full original Result replay, not card qualification.
use super::*;
use crate::{
    game::{
        GameCommand, GameContext, GameError, GameId, GameInput, GameState, NativeCandidate,
        RuleFrame,
    },
    occurrence::{RulesOccurrence, RulesRecord},
    program::{
        BodyTarget, ExecutableBody, PlayerRelation, ResolvingPlayer, TriggerGrouping, TriggerSpec,
        TriggeredAbility, TriggeredAbilityId,
    },
    test_support::creature_types,
};
const A: SeatId = SeatId(19);
const B: SeatId = SeatId(7);

fn operation(
    kind: LibraryActionKind,
    player: ResolvingPlayer,
    amount: RulesInteger,
) -> Instruction {
    let amount = crate::program::ValueExpr::Literal(amount);
    match kind {
        LibraryActionKind::Scry => Instruction::Scry { player, amount },
        LibraryActionKind::Surveil => Instruction::Surveil { player, amount },
        LibraryActionKind::Mill => Instruction::Mill { player, amount },
    }
}
fn gain(amount: i64) -> Instruction {
    crate::program::Instruction::AdjustLife(Box::new(crate::toolbox::AdjustLifeSpec::gain(
        crate::toolbox::PlayerSelector::Resolving(crate::program::ResolvingPlayer::Controller),
        crate::program::ValueExpr::Literal(amount.into()),
    )))
}
fn seed(card: u64, owner: SeatId, destination: ObjectDestination, program: Program) -> SeedCard {
    SeedCard {
        card: CardId(card),
        owner,
        destination,
        program,
    }
}
fn fixture(
    body: Vec<Instruction>,
    target: BodyTarget,
    owner: SeatId,
    count: u64,
    watcher: Option<TriggerSpec>,
    copy: bool,
) -> GameState {
    fixture_with_sources(body, target, owner, count, watcher, copy, vec![])
}
fn fixture_with_sources(
    body: Vec<Instruction>,
    target: BodyTarget,
    owner: SeatId,
    count: u64,
    watcher: Option<TriggerSpec>,
    copy: bool,
    sources: Vec<Program>,
) -> GameState {
    let vocabulary = creature_types::selected_vocabulary().unwrap();
    let names = Arc::new(spellwake_contracts::names::NameRules::literal_names());
    let context = GameContext::new(
        "library-top-unit".into(),
        vocabulary.rules_revision().into(),
        "synthetic".into(),
        "library-top".into(),
        "private-native".into(),
        vocabulary.id(),
        names.id(),
    )
    .unwrap();
    let mut cards = vec![seed(
        1,
        A,
        ObjectDestination::Hand,
        Program::instant(ManaCost::free(), ExecutableBody::new(target, body)),
    )];
    for id in (10..10 + count).rev() {
        cards.push(seed(
            id,
            owner,
            ObjectDestination::LibraryTop,
            Program::artifact(None, vec![]),
        ));
    }
    cards.push(seed(
        90,
        owner,
        ObjectDestination::Graveyard,
        Program::artifact(None, vec![]),
    ));
    let mut battlefield = vec![];
    for (i, source) in sources.into_iter().enumerate() {
        let card = 200 + u64::try_from(i).unwrap();
        cards.push(seed(
            card,
            A,
            ObjectDestination::Battlefield { controller: A },
            source,
        ));
        battlefield.push(CardId(card));
    }
    if let Some(condition) = watcher {
        let mut program = Program::artifact(None, vec![]);
        program.triggered.push(TriggeredAbility {
            id: TriggeredAbilityId(0),
            condition,
            grouping: TriggerGrouping::EachOccurrence,
            body: ExecutableBody::new(BodyTarget::None, vec![gain(1)]),
        });
        cards.push(seed(
            100,
            A,
            ObjectDestination::Battlefield { controller: A },
            program,
        ));
        battlefield.push(CardId(100));
    }
    if copy {
        cards.push(seed(
            2,
            A,
            ObjectDestination::Hand,
            Program::instant(
                ManaCost::free(),
                ExecutableBody::new(
                    BodyTarget::SingleSpell(crate::program::SpellFilter::copyable_instruction()),
                    vec![Instruction::StackOp(Box::new(
                        crate::toolbox::StackOpSpec::copy_targeted_spell(),
                    ))],
                ),
            ),
        ));
    }
    GameState::initialize(
        GameId(88411),
        context,
        GameSeed::with_uniform_life(
            vec![A, B],
            A,
            cards,
            battlefield,
            vec![],
            20.into(),
            CombatRules::UnrestrictedFreeForAll,
            vocabulary,
            names,
        ),
    )
    .unwrap()
    .into_state()
}
struct Run {
    initial: GameState,
    state: GameState,
    calls: Vec<(GameInput, Result<NativeCandidate, GameError>)>,
}
impl Run {
    fn new(state: GameState) -> Self {
        Self {
            initial: state.clone(),
            state,
            calls: vec![],
        }
    }
    fn call(&mut self, input: GameInput) -> Result<NativeCandidate, GameError> {
        let before = self.state.clone();
        let result = self.state.advance(input.clone());
        assert_eq!(
            self.state, before,
            "candidate preparation/rejection preserves source"
        );
        assert_eq!(
            self.state.advance(input.clone()),
            result,
            "whole Result deterministic at original cut"
        );
        if let Ok(candidate) = &result {
            self.state = candidate.state().clone();
        }
        self.calls.push((input, result.clone()));
        result
    }
    fn command(&mut self, actor: SeatId, command: GameCommand) -> NativeCandidate {
        self.call(GameInput {
            basis: self.state.basis().clone(),
            actor,
            command,
        })
        .unwrap()
    }
    fn cast(&mut self, card: u64, target: Option<TargetRef>) {
        let spell = object(&self.state, card);
        self.command(
            A,
            match target {
                Some(target) => GameCommand::Cast { spell, target },
                None => GameCommand::CastUntargeted {
                    spell,
                    payment: None,
                },
            },
        );
    }
    fn pass_round(&mut self) {
        for _ in 0..2 {
            if self.state.pending_decision().is_some() {
                break;
            }
            let WindowPhase::Priority(PriorityPhase::Ready { holder }) =
                self.state.execution().window().phase()
            else {
                panic!("actual ready priority")
            };
            self.command(holder, GameCommand::Pass);
        }
    }
    fn reply(&mut self, choice: DecisionReply) -> NativeCandidate {
        let pending = self.state.pending_decision().unwrap();
        self.command(
            pending.chooser(),
            GameCommand::Reply {
                decision: pending.id(),
                choice,
            },
        )
    }
    fn reject(&mut self, actor: SeatId, choice: DecisionReply, error: GameError) {
        let pending = self.state.pending_decision().unwrap();
        let input = GameInput {
            basis: self.state.basis().clone(),
            actor,
            command: GameCommand::Reply {
                decision: pending.id(),
                choice,
            },
        };
        assert_eq!(self.call(input), Err(error));
    }
    fn facts(&self) -> Vec<&RulesOccurrence> {
        self.calls
            .iter()
            .filter_map(|(_, r)| r.as_ref().ok())
            .flat_map(|c| &c.rules_journal().records)
            .filter_map(|r| match r {
                RulesRecord::Event(e) => Some(e),
                _ => None,
            })
            .flat_map(|e| &e.occurrences)
            .map(|o| &o.value)
            .collect()
    }
    fn replay(&self) {
        let mut state = self.initial.clone();
        for (input, expected) in &self.calls {
            let before = state.clone();
            let actual = state.advance(input.clone());
            assert_eq!(
                &actual, expected,
                "complete original typed input/result replay"
            );
            assert_eq!(state, before);
            if let Ok(candidate) = actual {
                state = candidate.into_state();
            }
        }
        assert_eq!(state, self.state);
        replacements::assert_publication_receipts(self);
    }
}
fn object(state: &GameState, card: u64) -> ObjectId {
    state
        .execution()
        .window()
        .cards()
        .current_identity(CardId(card))
        .unwrap()
}
fn library(state: &GameState, owner: SeatId) -> Vec<CardId> {
    let cards = state.execution().window().cards();
    cards
        .library(owner)
        .iter()
        .map(|id| cards.get(*id).unwrap().card_id().unwrap())
        .collect()
}
fn graveyard(state: &GameState, owner: SeatId) -> Vec<CardId> {
    let cards = state.execution().window().cards();
    cards
        .graveyard(owner)
        .iter()
        .map(|id| cards.get(*id).unwrap().card_id().unwrap())
        .collect()
}
fn frame(state: &GameState) -> &LibraryTopFrame {
    let RuleFrame::LibraryTop(frame) = state.pending_decision().unwrap().frame() else {
        panic!("actual library frame")
    };
    frame
}

#[test]
fn huge_targeted_mill_clamps_before_conversion_and_empty_library_is_not_failed_draw() {
    let huge = RulesInteger::try_from("999999999999999999999999999999999999".to_owned()).unwrap();
    let mut run = Run::new(fixture(
        vec![
            operation(
                LibraryActionKind::Mill,
                ResolvingPlayer::TargetPlayer,
                huge.clone(),
            ),
            gain(2),
        ],
        BodyTarget::SinglePlayer,
        B,
        3,
        None,
        false,
    ));
    let old: Vec<_> = (10..13).map(|n| object(&run.state, n)).collect();
    run.cast(1, Some(TargetRef::Player(B)));
    run.pass_round();
    assert!(matches!(
        run.state.pending_decision().unwrap().frame(),
        RuleFrame::ResolutionGraveyardOrder(_)
    ));
    run.reply(DecisionReply::OrderResolutionGraveyard(
        old.iter().rev().copied().collect(),
    ));
    assert!(library(&run.state, B).is_empty());
    assert_eq!(
        graveyard(&run.state, B),
        vec![CardId(12), CardId(11), CardId(10), CardId(90)]
    );
    assert!(run.state.outcome().is_none());
    assert!(!run.facts().iter().any(|f| matches!(
        f,
        RulesOccurrence::FailedDraw { .. } | RulesOccurrence::PlayerLost { .. }
    )));
    assert!(run.facts().iter().any(|f| matches!(f, RulesOccurrence::LibraryAction(a) if a.kind==LibraryActionKind::Mill && a.requested==huge && a.looked_top_first.len()==3)));
    run.replay();
}

#[test]
fn zero_and_positive_empty_actions_have_distinct_completed_trigger_semantics() {
    for kind in [LibraryActionKind::Scry, LibraryActionKind::Surveil] {
        for amount in [0, 4] {
            let condition = if kind == LibraryActionKind::Scry {
                crate::program::TriggerSpec::player_action(
                    PlayerRelation::Controller,
                    crate::program::PlayerTriggerAction::Scried,
                )
            } else {
                crate::program::TriggerSpec::player_action(
                    PlayerRelation::Controller,
                    crate::program::PlayerTriggerAction::Surveilled,
                )
            };
            let mut run = Run::new(fixture(
                vec![
                    operation(kind, ResolvingPlayer::Controller, amount.into()),
                    gain(5),
                ],
                BodyTarget::None,
                A,
                0,
                Some(condition),
                false,
            ));
            run.cast(1, None);
            run.pass_round();
            assert!(run.state.pending_decision().is_none());
            assert_eq!(run.state.execution().life_total(A), Some(&25.into()));
            let actions = run
                .facts()
                .iter()
                .filter(|f| matches!(f, RulesOccurrence::LibraryAction(_)))
                .count();
            assert_eq!(actions, usize::from(amount > 0));
            assert_eq!(
                run.state.execution().window().entries().len(),
                usize::from(amount > 0),
                "trigger only placed after the main action and life tail complete"
            );
            if amount > 0 {
                run.pass_round();
                assert_eq!(run.state.execution().life_total(A), Some(&26.into()));
            }
            run.replay();
        }
    }
}

#[test]
fn singleton_scry_and_surveil_retain_real_partition_choices() {
    for kind in [LibraryActionKind::Scry, LibraryActionKind::Surveil] {
        let mut run = Run::new(fixture(
            vec![operation(kind, ResolvingPlayer::Controller, 1.into())],
            BodyTarget::None,
            A,
            1,
            None,
            false,
        ));
        let id = object(&run.state, 10);
        run.cast(1, None);
        run.pass_round();
        assert_eq!(frame(&run.state).domain_top_first(), &[id]);
        let reply = if kind == LibraryActionKind::Scry {
            LibraryTopReply::Scry {
                top_first: vec![],
                bottom_top_first: vec![id],
            }
        } else {
            LibraryTopReply::Surveil {
                top_first: vec![],
                to_graveyard: vec![id],
            }
        };
        run.reply(DecisionReply::LibraryTop(reply));
        assert_eq!(
            library(&run.state, A).len(),
            usize::from(kind == LibraryActionKind::Scry)
        );
        run.replay();
    }
}

#[test]
fn invalid_partitions_actor_family_and_stale_basis_leave_whole_source_and_cursor_unchanged() {
    let mut run = Run::new(fixture(
        vec![
            operation(
                LibraryActionKind::Scry,
                ResolvingPlayer::Controller,
                3.into(),
            ),
            gain(2),
        ],
        BodyTarget::None,
        A,
        4,
        None,
        false,
    ));
    run.cast(1, None);
    run.pass_round();
    let d = frame(&run.state).domain_top_first().to_vec();
    let valid = LibraryTopReply::Scry {
        top_first: d.clone(),
        bottom_top_first: vec![],
    };
    run.reject(
        B,
        DecisionReply::LibraryTop(valid.clone()),
        GameError::WrongChooser,
    );
    for (top_first, bottom_top_first) in [
        (vec![d[0], d[0]], vec![d[1]]),
        (vec![d[0]], vec![d[1]]),
        (d.clone(), vec![object(&run.state, 13)]),
        (vec![object(&run.state, 90)], vec![d[0], d[1]]),
    ] {
        run.reject(
            A,
            DecisionReply::LibraryTop(LibraryTopReply::Scry {
                top_first,
                bottom_top_first,
            }),
            GameError::Effect(EffectError::InvalidLibraryPartition),
        );
    }
    run.reject(
        A,
        DecisionReply::LibraryTop(LibraryTopReply::Surveil {
            top_first: d.clone(),
            to_graveyard: vec![],
        }),
        GameError::Effect(EffectError::WrongDecisionKind),
    );
    let pending = run.state.pending_decision().unwrap();
    let mut basis = run.state.basis().clone();
    basis.revision -= 1;
    assert_eq!(
        run.call(GameInput {
            basis,
            actor: A,
            command: GameCommand::Reply {
                decision: pending.id(),
                choice: DecisionReply::LibraryTop(valid.clone())
            }
        }),
        Err(GameError::StaleRevision)
    );
    run.reply(DecisionReply::LibraryTop(valid));
    run.replay();
}

#[test]
fn owned_targetless_copy_and_original_each_run_the_same_library_operator() {
    for kind in [
        LibraryActionKind::Scry,
        LibraryActionKind::Surveil,
        LibraryActionKind::Mill,
    ] {
        let mut run = Run::new(fixture(
            vec![operation(kind, ResolvingPlayer::Controller, 1.into())],
            BodyTarget::None,
            A,
            3,
            None,
            true,
        ));
        run.cast(1, None);
        let original = run.state.execution().window().entries()[0].id();
        run.cast(2, Some(TargetRef::Stack(original)));
        run.pass_round();
        for _ in 0..2 {
            run.pass_round();
            if let Some(pending) = run.state.pending_decision() {
                let RuleFrame::LibraryTop(f) = pending.frame() else {
                    panic!("library copy frame")
                };
                let domain = f.domain_top_first().to_vec();
                let reply = if kind == LibraryActionKind::Scry {
                    LibraryTopReply::Scry {
                        top_first: vec![],
                        bottom_top_first: domain,
                    }
                } else {
                    LibraryTopReply::Surveil {
                        top_first: vec![],
                        to_graveyard: domain,
                    }
                };
                run.reply(DecisionReply::LibraryTop(reply));
            }
        }
        assert!(run.state.execution().window().entries().is_empty());
        assert_eq!(
            run.facts()
                .iter()
                .filter(|f| matches!(f,RulesOccurrence::LibraryAction(a) if a.kind==kind))
                .count(),
            2
        );
        run.replay();
    }
}

#[test]
fn admission_rejects_negative_undefined_amount_and_unavailable_recipient_capabilities() {
    let v = creature_types::selected_vocabulary().unwrap();
    for kind in [
        LibraryActionKind::Scry,
        LibraryActionKind::Surveil,
        LibraryActionKind::Mill,
    ] {
        let mut undefined = operation(kind, ResolvingPlayer::Controller, 1.into());
        match &mut undefined {
            Instruction::Scry { amount, .. }
            | Instruction::Surveil { amount, .. }
            | Instruction::Mill { amount, .. } => {
                *amount = crate::program::ValueExpr::count_battlefield(
                    crate::program::ObjectFilter::any()
                        .controlled()
                        .of_chosen_slot(CreatureTypeSlot::new(9)),
                )
            }
            _ => unreachable!("test constructs only library instructions"),
        }
        assert!(
            ExecutableBody::new(BodyTarget::None, vec![undefined])
                .validate(&v)
                .is_err()
        );
        assert!(
            ExecutableBody::new(
                BodyTarget::None,
                vec![operation(kind, ResolvingPlayer::Controller, (-1).into())]
            )
            .validate(&v)
            .is_err()
        );
        assert!(
            ExecutableBody::new(
                BodyTarget::None,
                vec![operation(kind, ResolvingPlayer::TargetPlayer, 1.into())]
            )
            .validate(&v)
            .is_err()
        );
        assert!(
            ExecutableBody::new(
                BodyTarget::None,
                vec![operation(kind, ResolvingPlayer::Controller, 0.into())]
            )
            .validate(&v)
            .is_ok()
        );
    }
}

#[test]
fn zero_actions_with_nonempty_library_preserve_all_cards_and_do_not_emit_actions_or_losses() {
    let mut body = vec![];
    for kind in [
        LibraryActionKind::Scry,
        LibraryActionKind::Surveil,
        LibraryActionKind::Mill,
    ] {
        body.push(operation(kind, ResolvingPlayer::Controller, 0.into()));
    }
    body.push(gain(3));
    let mut run = Run::new(fixture(body, BodyTarget::None, A, 3, None, false));
    let original = run.state.execution().window().cards().library(A).to_vec();
    run.cast(1, None);
    run.pass_round();
    assert!(run.state.pending_decision().is_none());
    assert_eq!(run.state.execution().window().cards().library(A), original);
    assert!(!run.facts().iter().any(|f| matches!(
        f,
        RulesOccurrence::LibraryAction(_)
            | RulesOccurrence::FailedDraw { .. }
            | RulesOccurrence::PlayerLost { .. }
    )));
    assert_eq!(run.state.execution().life_total(A), Some(&23.into()));
    run.replay();
}

mod replacements;
