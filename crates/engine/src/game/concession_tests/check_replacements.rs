//! Combined-lane controls at real Checks-owned decisions, with full typed replay.
use super::*;
use crate::{checks::CheckChoice, occurrence::EventCause};

struct ChecksRun {
    initial: GameState,
    state: GameState,
    calls: Vec<(GameInput, NativeCandidate)>,
}
impl ChecksRun {
    fn new(state: GameState) -> Self {
        both(&state);
        Self {
            initial: state.clone(),
            state,
            calls: vec![],
        }
    }
    fn command(&mut self, actor: SeatId, command: GameCommand) -> NativeCandidate {
        let original = self.state.clone();
        let issued = input(&self.state, actor, command);
        let actual = self.state.advance(issued.clone());
        assert_eq!(actual, self.state.advance(issued.clone()));
        assert_eq!(self.state, original);
        let actual = actual.unwrap();
        self.state = actual.state().clone();
        both(&self.state);
        self.calls.push((issued, actual.clone()));
        actual
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
    fn cast(&mut self, card: u64, target: TargetRef) {
        self.command(
            A,
            GameCommand::Cast {
                spell: object(&self.state, card),
                target,
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
                panic!("real ready holder")
            };
            self.command(holder, GameCommand::Pass);
        }
    }
    fn replay(&self) {
        let mut state = self.initial.clone();
        for (issued, expected) in &self.calls {
            let before = state.clone();
            let actual = state.advance(issued.clone());
            assert_eq!(
                actual,
                Ok(expected.clone()),
                "complete original input/result replay"
            );
            assert_eq!(state, before);
            state = actual.unwrap().into_state();
        }
        assert_eq!(state, self.state);
    }
}

fn replacement_source(action: ZoneActionKind) -> Program {
    let mut program = Program::enchantment(None);
    program.zone_replacements.push(ZoneReplacementAbility {
        id: ZoneReplacementAbilityId(0),
        actions: ZoneActionPredicate::OneOf(BTreeSet::from([action])),
        objects: ZoneObjectKind::Either,
        subject: ZoneReplacementSubject::AnyObject,
        destination: PublicZoneDestination::Graveyard,
        replace_with: PublicZoneDestination::Exile,
        choice: ZoneReplacementChoice::OptionalAffectedPlayer,
    });
    program
}

fn assert_frame_v2(state: &GameState, expected_cause: &str) {
    use crate::{
        effects::{EffectDecision, ZoneEventKey},
        test_support::projection,
    };
    let rule = state.pending_decision().unwrap().frame();
    let RuleFrame::ZoneReplacement(frame) = rule else {
        panic!("issued replacement")
    };
    let encoded = projection::frame(rule, state).unwrap();
    assert_eq!(
        encoded,
        projection::effect_frame(
            &EffectDecision::ZoneReplacement(frame.clone()),
            state.execution()
        )
        .unwrap()
    );
    assert_eq!(encoded["tag"], "ZoneReplacement");
    assert_eq!(encoded["schema"], "zone-replacement-native-frame-v2");
    assert_eq!(encoded["cause"]["tag"], expected_cause);
    assert_eq!(encoded["revision"], frame.revision().to_string());
    assert_eq!(encoded["stage"], frame.stage().to_string());
    assert_eq!(encoded["chooser"], frame.chooser().0);
    assert!(encoded.get("entry").is_none() && encoded.get("instruction").is_none());
    let (offered, values) = match frame.request() {
        ZoneReplacementRequest::Select { applications } => {
            assert_eq!(encoded["request"]["tag"], "Select");
            (
                applications.clone(),
                encoded["request"]["applications"]
                    .as_array()
                    .unwrap()
                    .clone(),
            )
        }
        ZoneReplacementRequest::Devour { .. }
        | ZoneReplacementRequest::DevourGraveyardOrder { .. }
        | ZoneReplacementRequest::EntryCopy { .. }
        | ZoneReplacementRequest::EntryController { .. } => {
            panic!("fixture has no controller-changing entrants")
        }
        ZoneReplacementRequest::ConfirmOptional { application } => {
            assert_eq!(encoded["request"]["tag"], "ConfirmOptional");
            (
                vec![*application],
                vec![encoded["request"]["application"].clone()],
            )
        }
    };
    assert_eq!(offered.len(), values.len());
    for (application, value) in offered.iter().zip(values) {
        assert_eq!(
            value["effect"]["source"],
            format!("{:?}", application.effect.source)
        );
        assert_eq!(value["effect"]["ability"], application.effect.ability.0);
        assert_eq!(
            value["event"]["before"],
            format!("{:?}", application.event.before())
        );
        match application.event {
            ZoneEventKey::Instruction {
                entry, instruction, ..
            } => {
                assert_eq!(value["event"]["tag"], "Instruction");
                assert_eq!(value["event"]["entry"], format!("{entry:?}"));
                assert_eq!(value["event"]["instruction"], instruction.to_string());
                assert!(value["event"].get("event").is_none());
            }
            ZoneEventKey::Check { event, .. } => {
                assert_eq!(value["event"]["tag"], "Check");
                assert_eq!(
                    value["event"]["event"]["started_revision"],
                    event.started_revision()
                );
                assert_eq!(value["event"]["event"]["pass"], event.pass());
                assert!(
                    value["event"].get("entry").is_none()
                        && value["event"].get("instruction").is_none()
                );
            }
            ZoneEventKey::LinkedReturn { .. } => {
                panic!("unexpected linked return in concession fixture")
            }
            ZoneEventKey::Cost { .. } => panic!("unexpected cost event in concession fixture"),
            ZoneEventKey::ManaCost { .. } => panic!("unexpected mana cost in concession fixture"),
            ZoneEventKey::PermanentEntry { .. } => {
                panic!("unexpected permanent entry in concession fixture")
            }
            ZoneEventKey::DrawReveal { .. } => {
                panic!("unexpected replaced draw in concession fixture")
            }
            ZoneEventKey::DamageFollowup { .. } | ZoneEventKey::LandPlay { .. } => {
                panic!("unexpected land play in concession fixture")
            }
        }
    }
    if let Some(batch) = state.execution().check_zone_replacement() {
        let work = projection::zone_replacement_work(batch);
        assert_eq!(work["schema"], "zone-replacement-native-work-v2");
        assert_eq!(work["cause"], encoded["cause"]);
        assert_eq!(
            work["candidates"].as_array().unwrap().len(),
            batch.candidates().len()
        );
        assert_eq!(
            work["intents"].as_array().unwrap().len(),
            batch.intents().len()
        );
        assert!(
            work.get("selections").is_none(),
            "neutral reducer does not invent selecting players"
        );
    }
}

/// Every accepted reduction prefix remains frozen when either player concedes.
fn reduce_to_order(run: &mut ChecksRun, expected_cleanup: bool) {
    for _ in 0..32 {
        let pending = run.state.pending_decision().unwrap();
        let RuleFrame::ZoneReplacement(frame) = pending.frame() else {
            return;
        };
        assert_frame_v2(
            &run.state,
            if expected_cleanup {
                "CleanupDiscard"
            } else {
                "StateBasedActions"
            },
        );
        let batch = run.state.execution().check_zone_replacement().unwrap();
        assert!(run.state.execution().has_check_replacement_work());
        assert_eq!(frame.cause(), batch.cause());
        assert_eq!(
            matches!(frame.cause(), EventCause::CleanupDiscard { .. }),
            expected_cleanup,
        );
        if !expected_cleanup {
            assert!(matches!(
                frame.cause(),
                EventCause::StateBasedActions { .. }
            ));
        }
        for intent in batch.intents() {
            let before = run
                .state
                .execution()
                .window()
                .cards()
                .get(intent.before)
                .unwrap();
            assert!(matches!(
                before.zone(),
                ObjectZone::Hand(_) | ObjectZone::Battlefield
            ));
        }
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
            ZoneReplacementRequest::ConfirmOptional { application } => {
                let before = run
                    .state
                    .execution()
                    .window()
                    .cards()
                    .get(application.event.before())
                    .unwrap();
                ZoneReplacementReply::Optional(if before.card_id() == Some(CardId(10)) {
                    ZoneOptionalAnswer::Apply
                } else {
                    ZoneOptionalAnswer::Decline
                })
            }
        };
        run.reply(DecisionReply::ZoneReplacement(choice));
    }
    panic!("bounded replacement script did not reach final owner order");
}

#[test]
fn concession_combined_cleanup_selection_reduction_and_final_order_freeze() {
    let mut cards = (10..20)
        .map(|id| {
            card(
                id,
                A,
                ObjectDestination::Hand,
                Program::artifact(None, vec![]),
            )
        })
        .collect::<Vec<_>>();
    cards.push(card(
        90,
        A,
        ObjectDestination::Battlefield { controller: A },
        replacement_source(ZoneActionKind::CleanupDiscard),
    ));
    let state = root(cards, &[20, 20], Some(Step::Cleanup), vec![A, B]);
    use crate::test_support::{current_profile::applicability::NativeCaptureValue, projection};
    let initial_frame = state.pending_decision().unwrap().frame();
    assert!(
        initial_frame.check_current10().is_err(),
        "isolated new cleanup frame refuses predecessor profile"
    );
    assert!(
        projection::frame(initial_frame, &state).is_err(),
        "historical check frame codec refuses new selection semantics"
    );
    let RuleFrame::Checks(CheckDecision::CleanupSelect(frame)) =
        state.pending_decision().unwrap().frame()
    else {
        panic!("new issued cleanup selection")
    };
    assert_eq!(frame.required(), 3);
    let selected = [10, 11, 12].map(|id| object(&state, id));
    let mut run = ChecksRun::new(state);
    let candidate = run.reply(DecisionReply::Discard(selected.to_vec()));
    assert!(
        matches!(&candidate.check_journal().choices[0].choice,CheckChoice::CleanupSelected { selected:actual } if actual==&selected)
    );
    reduce_to_order(&mut run, true);
    let RuleFrame::Checks(CheckDecision::GraveyardOrder(frame)) =
        run.state.pending_decision().unwrap().frame()
    else {
        panic!("new issued final-G cleanup order")
    };
    assert_eq!(
        frame.domain().iter().copied().collect::<BTreeSet<_>>(),
        BTreeSet::from([selected[1], selected[2]])
    );
    assert!(run.state.execution().has_check_replacement_work());
    assert_eq!(run.state.pending_turn_work(), Some(TurnWork::Cleanup));
    for id in selected {
        assert_eq!(
            run.state
                .execution()
                .window()
                .cards()
                .get(id)
                .unwrap()
                .zone(),
            ObjectZone::Hand(A)
        );
    }
    both(&run.state);
    run.replay();
}

#[test]
fn concession_combined_sba_prefix_and_final_order_keep_prospective_loss_inactive() {
    let mut cards = (10..13)
        .map(|id| {
            card(
                id,
                A,
                ObjectDestination::Battlefield { controller: A },
                Program::creature(None, 1.into(), 0.into(), BTreeSet::new()),
            )
        })
        .collect::<Vec<_>>();
    cards.push(card(
        90,
        A,
        ObjectDestination::Battlefield { controller: A },
        replacement_source(ZoneActionKind::StateBased),
    ));
    let state = root(cards, &[0, 20], None, vec![A, B]);
    assert!(state.execution().state_based_losses().is_empty());
    assert!(state.conclusion().is_none());
    let mut run = ChecksRun::new(state);
    reduce_to_order(&mut run, false);
    let RuleFrame::Checks(CheckDecision::GraveyardOrder(frame)) =
        run.state.pending_decision().unwrap().frame()
    else {
        panic!("new issued final-G SBA order")
    };
    assert_eq!(frame.domain().len(), 2);
    assert!(run.state.execution().state_based_losses().is_empty());
    assert!(run.state.conclusion().is_none());
    assert_eq!(
        conclude(&run.state, B).state().outcome(),
        Some(GameOutcome::Winner(A))
    );
    run.replay();
}

#[test]
fn concession_combined_instruction_frame_uses_v2_real_cause_and_key() {
    let mut cards = (10..12)
        .map(|id| {
            card(
                id,
                B,
                ObjectDestination::LibraryBottom,
                Program::artifact(None, vec![]),
            )
        })
        .collect::<Vec<_>>();
    cards.push(card(
        90,
        A,
        ObjectDestination::Battlefield { controller: A },
        replacement_source(ZoneActionKind::EffectMill),
    ));
    let state = fixture(
        vec![Instruction::MutatePlayer(Box::new(
            crate::toolbox::MutatePlayerSpec::mill(
                ResolvingPlayer::TargetPlayer,
                crate::program::ValueExpr::Literal(2.into()),
            ),
        ))],
        BodyTarget::SinglePlayer,
        cards,
    );
    let mut run = ChecksRun::new(state);
    run.cast(1, TargetRef::Player(B));
    run.pass_round();
    assert_frame_v2(&run.state, "Instruction");
    assert!(run.state.execution().check_zone_replacement().is_none());
    both(&run.state);
    run.replay();
}
