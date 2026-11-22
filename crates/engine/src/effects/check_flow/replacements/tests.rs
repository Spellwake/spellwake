//! Real native inputs/full typed replay. Synthetic Programs are mechanism controls,
//! not source-card admission. Private corruption/counter forks are labeled separately.
use super::*;
use crate::{
    activated::{ActivatedAbility, ActivatedAbilityId, SourceCost},
    characteristics::{
        AbilityChange, AbilityRemoval, NumericExpression, NumericPair, PermanentSelector,
        ResolvedDuration, ResolvedPartSpec, ResolvedPartsSpec, StaticAbilityId,
        StaticContinuousAbility, StaticPart, StaticParts,
    },
    effects::private_qualification::QualificationFault,
    game::{
        GameCommand, GameContext, GameError, GameId, GameInput, GameState, NativeCandidate,
        RuleFrame,
    },
    life::LifeAdjustment,
    object_identity::ObjectIdentityError,
    program::{
        AffectedPlayers, BodyTarget, ExecutableBody, PermanentTypeFilter, PlayerRelation,
        PublicZoneDestination as D, ResolvingPlayer, Supertype, TriggerGrouping, TriggeredAbility,
        TriggeredAbilityId, ZoneObjectKind, ZoneReplacementAbility, ZoneReplacementAbilityId,
        ZoneReplacementChoice, ZoneReplacementSubject,
    },
    test_support::{
        check_trace, creature_types, current_profile::applicability::NativeCaptureValue,
        program as codec, projection,
    },
    token::{TokenCreation, TokenDefinition, TokenName, TokenRecipients},
    turn::{Step, TurnStart},
};
use spellwake_contracts::names::{EnglishCardName, NameRules};
const A: SeatId = SeatId(19);
const B: SeatId = SeatId(7);
fn rule(id: u64, action: ZoneActionKind, from: D, to: D, optional: bool) -> ZoneReplacementAbility {
    ZoneReplacementAbility {
        id: ZoneReplacementAbilityId(id),
        actions: ZoneActionPredicate::OneOf(BTreeSet::from([action])),
        objects: ZoneObjectKind::Either,
        subject: ZoneReplacementSubject::AnyObject,
        destination: from,
        replace_with: to,
        choice: if optional {
            ZoneReplacementChoice::OptionalAffectedPlayer
        } else {
            ZoneReplacementChoice::Mandatory
        },
    }
}
fn source(rules: Vec<ZoneReplacementAbility>) -> Program {
    let mut p = Program::enchantment(None);
    p.zone_replacements = rules;
    p
}
fn seed(card: u64, owner: SeatId, destination: ObjectDestination, program: Program) -> SeedCard {
    SeedCard {
        card: CardId(card),
        owner,
        destination,
        program,
    }
}
fn bf(card: u64, owner: SeatId, controller: SeatId, p: Program) -> SeedCard {
    seed(
        card,
        owner,
        ObjectDestination::Battlefield { controller },
        p,
    )
}
fn creature(toughness: i64) -> Program {
    Program::creature(None, 2.into(), toughness.into(), BTreeSet::new())
}
fn gain(n: i64) -> Instruction {
    Instruction::AdjustControllerLife {
        adjustment: LifeAdjustment::Gain(n.into()),
    }
}
fn instant(card: u64, target: BodyTarget, body: Vec<Instruction>) -> SeedCard {
    seed(
        card,
        A,
        ObjectDestination::Hand,
        Program::instant(ManaCost::free(), ExecutableBody::new(target, body)),
    )
}
fn cards_hand(n: u64) -> Vec<SeedCard> {
    (10..10 + n)
        .map(|id| {
            seed(
                id,
                A,
                ObjectDestination::Hand,
                Program::artifact(None, vec![]),
            )
        })
        .collect()
}
fn object(state: &GameState, card: u64) -> ObjectId {
    state
        .execution()
        .window()
        .cards()
        .current_identity(CardId(card))
        .unwrap()
}
fn zone(state: &GameState, card: u64) -> ObjectZone {
    state
        .execution()
        .window()
        .cards()
        .get(object(state, card))
        .unwrap()
        .zone()
}
fn replacement(run: &Run) -> &ZoneReplacementFrame {
    let RuleFrame::ZoneReplacement(f) = run.state.pending_decision().unwrap().frame() else {
        panic!("real replacement")
    };
    f
}
fn check_batch(run: &Run) -> &ZoneReplacementBatch {
    run.state.execution().check_zone_replacement().unwrap()
}
fn check_event(run: &Run) -> CheckEventId {
    match replacement(run).cause() {
        EventCause::CleanupDiscard { event, .. } | EventCause::StateBasedActions { event, .. } => {
            *event
        }
        _ => panic!("real Check cause"),
    }
}
fn select(run: &mut Run, id: ObjectId, ability: u64) {
    let ZoneReplacementRequest::Select { applications } = replacement(run).request() else {
        panic!("real pair choice")
    };
    let offered = *applications
        .iter()
        .find(|a| a.event.before() == id && a.effect.ability == ZoneReplacementAbilityId(ability))
        .unwrap();
    run.reply(DecisionReply::ZoneReplacement(
        ZoneReplacementReply::Select(offered),
    ));
}
fn optional(run: &mut Run, apply: bool) {
    run.reply(DecisionReply::ZoneReplacement(
        ZoneReplacementReply::Optional(if apply {
            ZoneOptionalAnswer::Apply
        } else {
            ZoneOptionalAnswer::Decline
        }),
    ));
}
fn answer_replacements(run: &mut Run, apply: bool) {
    for _ in 0..40 {
        let Some(pending) = run.state.pending_decision() else {
            return;
        };
        let RuleFrame::ZoneReplacement(f) = pending.frame() else {
            return;
        };
        let reply = match f.request() {
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
                ZoneReplacementReply::Optional(if apply {
                    ZoneOptionalAnswer::Apply
                } else {
                    ZoneOptionalAnswer::Decline
                })
            }
        };
        run.reply(DecisionReply::ZoneReplacement(reply));
    }
    panic!("bounded mechanism choices")
}
fn order(run: &mut Run, ids: Vec<ObjectId>) {
    run.reply(DecisionReply::OrderGraveyard(ids));
}
struct Run {
    initial: NativeCandidate,
    state: GameState,
    seed: GameSeed,
    context: GameContext,
    step: Step,
    calls: Vec<(GameInput, Result<NativeCandidate, GameError>)>,
}
impl Run {
    fn new(mut cards: Vec<SeedCard>, active: SeatId, step: Step) -> Self {
        let v = creature_types::selected_vocabulary().unwrap();
        let names = Arc::new(NameRules::literal_names());
        for (owner, start) in [(A, 1000), (B, 2000)] {
            for card in start..start + 5 {
                cards.push(seed(
                    card,
                    owner,
                    ObjectDestination::LibraryBottom,
                    Program::artifact(None, vec![]),
                ));
            }
        }
        cards.push(seed(
            90,
            A,
            ObjectDestination::Graveyard,
            Program::artifact(None, vec![]),
        ));
        let initial_order = cards
            .iter()
            .filter(|c| matches!(c.destination, ObjectDestination::Battlefield { .. }))
            .map(|c| c.card)
            .collect();
        let mut seed = GameSeed::with_uniform_life(
            vec![A, B],
            active,
            cards,
            initial_order,
            vec![],
            20.into(),
            CombatRules::UnrestrictedFreeForAll,
            v.clone(),
            names.clone(),
        );
        // Fixture tenure premise: seeded Worlds are authored singleton cohorts in the supplied BF order.
        seed.initial_world_cohorts = seed
            .cards
            .iter()
            .filter(|c| {
                matches!(c.destination, ObjectDestination::Battlefield { .. })
                    && c.program.supertypes.contains(&Supertype::World)
            })
            .map(|c| vec![c.card])
            .collect();
        let context = GameContext::new(
            "check-zone-unit".into(),
            v.rules_revision().into(),
            "synthetic-mechanism".into(),
            "checks-zone".into(),
            "private-native".into(),
            v.id(),
            names.id(),
        )
        .unwrap();
        let initial = GameState::initialize_scheduled(
            GameId(20261106),
            context.clone(),
            seed.clone(),
            TurnStart { number: 1, step },
        )
        .unwrap();
        Self {
            state: initial.state().clone(),
            initial,
            seed,
            context,
            step,
            calls: vec![],
        }
    }
    fn call(&mut self, input: GameInput) -> Result<NativeCandidate, GameError> {
        let before = self.state.clone();
        let result = self.state.advance(input.clone());
        assert_eq!(self.state, before);
        assert_eq!(self.state.advance(input.clone()), result);
        if let Ok(c) = &result {
            self.state = c.state().clone();
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
    fn reply(&mut self, choice: DecisionReply) -> NativeCandidate {
        let p = self.state.pending_decision().unwrap();
        self.command(
            p.chooser(),
            GameCommand::Reply {
                decision: p.id(),
                choice,
            },
        )
    }
    fn pass_round(&mut self) {
        for _ in 0..2 {
            if self.state.pending_decision().is_some() || self.state.outcome().is_some() {
                return;
            }
            let WindowPhase::Priority(PriorityPhase::Ready { holder }) =
                self.state.execution().window().phase()
            else {
                panic!("actual priority")
            };
            self.command(holder, GameCommand::Pass);
        }
    }
    fn cast(&mut self, actor: SeatId, card: u64, target: Option<TargetRef>) {
        let spell = object(&self.state, card);
        self.command(
            actor,
            target.map_or(
                GameCommand::CastUntargeted {
                    spell,
                    payment: None,
                },
                |target| GameCommand::Cast { spell, target },
            ),
        );
    }
    fn events(&self) -> Vec<&crate::occurrence::RulesEvent> {
        self.calls
            .iter()
            .filter_map(|(_, r)| r.as_ref().ok())
            .flat_map(|c| &c.rules_journal().records)
            .filter_map(|r| match r {
                RulesRecord::Event(e) => Some(e),
                _ => None,
            })
            .collect()
    }
    fn replay(&self) {
        assert_eq!(
            GameState::initialize_scheduled(
                GameId(20261106),
                self.context.clone(),
                self.seed.clone(),
                TurnStart {
                    number: 1,
                    step: self.step
                }
            ),
            Ok(self.initial.clone())
        );
        let mut state = self.initial.state().clone();
        for (input, expected) in &self.calls {
            let before = state.clone();
            let actual = state.advance(input.clone());
            assert_eq!(&actual, expected, "whole original typed input/Result");
            assert_eq!(state, before);
            if let Ok(c) = actual {
                state = c.into_state();
            }
        }
        assert_eq!(state, self.state);
        assert_receipts(self);
    }
}
fn assert_receipts(run: &Run) {
    for c in run.calls.iter().filter_map(|(_, r)| r.as_ref().ok()) {
        for m in &c.check_journal().moves {
            let matching = c
                .rules_journal()
                .records
                .iter()
                .filter_map(|r| match r {
                    RulesRecord::Event(e)
                        if match e.cause {
                            EventCause::CleanupDiscard { event, boundary } => {
                                m.cause == CheckMoveCause::CleanupDiscard
                                    && event == m.event
                                    && boundary == m.boundary
                            }
                            EventCause::StateBasedActions { event, boundary } => {
                                m.cause == CheckMoveCause::StateBasedAction
                                    && event == m.event
                                    && boundary == m.boundary
                            }
                            _ => false,
                        } =>
                    {
                        Some(e)
                    }
                    _ => None,
                })
                .collect::<Vec<_>>();
            assert_eq!(matching.len(), 1);
            let same = matching[0]
                .occurrences
                .iter()
                .filter_map(|o| match &o.value {
                    RulesOccurrence::ZoneChange(z) if z.before.object.id() == m.before.id() => {
                        Some(z)
                    }
                    _ => None,
                })
                .collect::<Vec<_>>();
            assert_eq!(same.len(), 1);
            assert_eq!(same[0].before.object, m.before);
            assert_eq!(same[0].after.object, m.after);
            assert!(matches!(
                m.after.zone(),
                ObjectZone::Graveyard(_) | ObjectZone::Exile
            ));
            assert_ne!(m.before.id(), m.after.id());
        }
    }
}
fn cleanup_source(optional: bool) -> SeedCard {
    bf(
        99,
        A,
        A,
        source(vec![rule(
            0,
            ZoneActionKind::CleanupDiscard,
            D::Graveyard,
            D::Exile,
            optional,
        )]),
    )
}
fn cleanup_run(optional: bool) -> Run {
    let mut cards = cards_hand(10);
    cards.push(cleanup_source(optional));
    let mut run = Run::new(cards, A, Step::End);
    run.pass_round();
    run
}
fn world_spell() -> SeedCard {
    let mut p = Program::creature(Some(ManaCost::free()), 1.into(), 0.into(), BTreeSet::new());
    p.supertypes.insert(Supertype::World);
    seed(1, A, ObjectDestination::Hand, p)
}
fn world_start(rules: Vec<ZoneReplacementAbility>, active: SeatId, stolen: bool) -> Run {
    let mut old = creature(2);
    old.supertypes.insert(Supertype::World);
    let mut spell = world_spell();
    spell.owner = active;
    let owner = if stolen { active } else { A };
    let controller = if stolen {
        if active == B { A } else { B }
    } else {
        A
    };
    let mut run = Run::new(
        vec![
            spell,
            bf(10, owner, controller, old),
            bf(99, A, A, source(rules)),
        ],
        active,
        Step::PrecombatMain,
    );
    run.cast(active, 1, None);
    run.pass_round();
    run
}

#[test]
fn ordinary_cleanup_and_effect_only_definitions_keep_historical_selection_order_and_pass_slots() {
    for count in [7, 10] {
        for effect_only in [false, true] {
            let mut cards = cards_hand(count);
            if effect_only {
                cards.push(bf(
                    99,
                    A,
                    A,
                    source(vec![rule(
                        0,
                        ZoneActionKind::EffectDiscard,
                        D::Graveyard,
                        D::Exile,
                        false,
                    )]),
                ));
            }
            let mut run = Run::new(cards, A, Step::End);
            run.pass_round();
            if count == 10 {
                let RuleFrame::Checks(CheckDecision::CleanupDiscard(f)) =
                    run.state.pending_decision().unwrap().frame()
                else {
                    panic!("ordinary legacy discard")
                };
                assert_eq!(f.event().pass(), 0);
                let chosen = f.domain()[..3].iter().rev().copied().collect::<Vec<_>>();
                assert_eq!(
                    projection::pending_work(run.state.execution()).unwrap(),
                    serde_json::json!({"tag":"Checks"})
                );
                let before_cards = run.state.execution().window().cards().clone();
                let done = run.reply(DecisionReply::Discard(chosen.clone()));
                assert!(
                    matches!(&done.check_journal().choices[0].choice,CheckChoice::CleanupDiscard {ordered} if *ordered==chosen)
                );
                assert_eq!(
                    done.check_journal()
                        .moves
                        .iter()
                        .map(|m| m.before.id())
                        .collect::<Vec<_>>(),
                    chosen
                );
                assert!(
                    done.check_journal()
                        .moves
                        .iter()
                        .all(|m| m.after.zone() == ObjectZone::Graveyard(A))
                );
                let mut journal = done.check_journal().clone();
                journal.moves.clear();
                assert_eq!(journal.check_current10(), Ok(()));
                assert!(
                    check_trace::check_journal_view(&journal, &before_cards, |id| id.0.to_string())
                        .is_ok()
                );
            } else {
                assert!(
                    !run.events()
                        .iter()
                        .any(|e| matches!(e.cause, EventCause::CleanupDiscard { .. }))
                );
            }
            run.replay();
        }
    }
}
#[test]
fn cleanup_selection_and_final_only_order_join_actual_mixed_destinations() {
    let mut run = cleanup_run(true);
    let RuleFrame::Checks(CheckDecision::CleanupSelect(f)) =
        run.state.pending_decision().unwrap().frame()
    else {
        panic!("distinct cleanup selection")
    };
    let event = f.event();
    let ids = [10, 11, 12].map(|id| object(&run.state, id));
    let selected = vec![ids[2], ids[0], ids[1]];
    let accepted = run.reply(DecisionReply::Discard(selected.clone()));
    assert!(
        matches!(&accepted.check_journal().choices[0].choice,CheckChoice::CleanupSelected {selected:s} if *s==selected)
    );
    assert!(accepted.check_journal().moves.is_empty());
    assert_eq!(check_event(&run), event);
    assert_eq!(check_batch(&run).candidates().len(), 3);
    let view = projection::zone_replacement_work(check_batch(&run));
    assert_eq!(view["schema"], "zone-replacement-native-work-v2");
    assert_eq!(view["cause"]["tag"], "CleanupDiscard");
    assert_eq!(view["intents"].as_array().unwrap().len(), 3);
    assert!(projection::pending_work(run.state.execution()).is_err());
    select(&mut run, ids[0], 0);
    optional(&mut run, true);
    select(&mut run, ids[1], 0);
    optional(&mut run, false);
    optional(&mut run, false);
    let RuleFrame::Checks(CheckDecision::GraveyardOrder(f)) =
        run.state.pending_decision().unwrap().frame()
    else {
        panic!("final G owner order")
    };
    assert_eq!(f.event(), event);
    assert_eq!(
        f.domain().iter().copied().collect::<BTreeSet<_>>(),
        BTreeSet::from([ids[1], ids[2]])
    );
    for id in 10..13 {
        assert_eq!(zone(&run.state, id), ObjectZone::Hand(A));
    }
    order(&mut run, vec![ids[2], ids[1]]);
    assert_eq!(zone(&run.state, 10), ObjectZone::Exile);
    let gy = run.state.execution().window().cards().graveyard(A);
    assert_eq!(
        &gy[..3],
        &[
            object(&run.state, 12),
            object(&run.state, 11),
            object(&run.state, 90)
        ]
    );
    run.replay();
    let mut all = cleanup_run(false);
    let selected = [10, 11, 12].map(|id| object(&all.state, id)).to_vec();
    all.reply(DecisionReply::Discard(selected));
    answer_replacements(&mut all, true);
    assert!(all.state.pending_decision().is_none());
    assert!(!all.calls.iter().any(|(_, r)| r.as_ref().is_ok_and(|c| {
        c.check_journal()
            .choices
            .iter()
            .any(|q| matches!(q.choice, CheckChoice::GraveyardOrder { .. }))
    })));
    all.replay();
}

#[test]
fn copied_cleanup_source_functions_before_expiry_then_waiting_only_triggers_repeat_cleanup() {
    let mut cards = cards_hand(9);
    cards.push(cleanup_source(false));
    let mut holder = Program::artifact(None, vec![]);
    holder.activated.push(ActivatedAbility {
        id: ActivatedAbilityId(0),
        mana_cost: ManaCost::free(),
        source_cost: SourceCost::None,
        body: ExecutableBody::new(
            BodyTarget::SinglePermanent,
            vec![Instruction::CopyTargetToSource {
                duration: ResolvedDuration::UntilEndOfTurn,
                power_toughness: None,
            }],
        ),
    });
    cards.push(bf(300, A, A, holder));
    cards.push(instant(
        2,
        BodyTarget::SinglePermanent,
        vec![Instruction::ReturnToOwnersHand],
    ));
    let mut watcher = Program::artifact(None, vec![]);
    watcher.triggered.push(TriggeredAbility {
        id: TriggeredAbilityId(0),
        condition: crate::program::TriggerSpec::player_action(
            PlayerRelation::Controller,
            crate::program::PlayerTriggerAction::Discarded,
        ),
        grouping: TriggerGrouping::EachOccurrence,
        body: ExecutableBody::new(BodyTarget::None, vec![gain(1)]),
    });
    cards.push(bf(400, A, A, watcher));
    let mut run = Run::new(cards, A, Step::End);
    run.command(
        A,
        GameCommand::ActivateAbility {
            source: object(&run.state, 300),
            ability: ActivatedAbilityId(0),
            target: TargetRef::Object(object(&run.state, 99)),
            payment: None,
        },
    );
    run.pass_round();
    run.cast(A, 2, Some(TargetRef::Object(object(&run.state, 99))));
    run.pass_round();
    assert_eq!(zone(&run.state, 99), ObjectZone::Hand(A));
    run.pass_round();
    let selected = [10, 11, 12].map(|id| object(&run.state, id)).to_vec();
    run.reply(DecisionReply::Discard(selected));
    assert!(
        check_batch(&run)
            .sources()
            .keys()
            .all(|source| source.source == object(&run.state, 300))
    );
    answer_replacements(&mut run, true);
    assert_eq!(run.state.turn().unwrap().position().cleanup_iteration, 0);
    assert!(
        run.state
            .execution()
            .current_characteristics(object(&run.state, 300))
            .unwrap()
            .unwrap()
            .abilities
            .zone_replacements
            .is_empty()
    );
    if let Some(p) = run.state.pending_decision() {
        let RuleFrame::Triggers(f) = p.frame() else {
            panic!("actual waiting trigger order")
        };
        run.reply(DecisionReply::OrderTriggers(f.instances().to_vec()));
    }
    assert_eq!(run.state.execution().window().entries().len(), 3);
    for _ in 0..3 {
        run.pass_round();
    }
    assert_eq!(run.state.execution().life_total(A), Some(&23.into()));
    run.pass_round();
    assert!(run.events().iter().any(|e|matches!(&e.cause,EventCause::StepBegan {position} if position.step==Step::Cleanup&&position.cleanup_iteration==1)));
    assert!(
        run.events()
            .iter()
            .any(|e| matches!(e.cause, EventCause::CleanupCharacteristics { .. }))
    );
    run.replay();
}

#[test]
fn represented_sba_reasons_union_and_stolen_apnap_use_real_check_identity_and_owner() {
    let mut run = world_start(
        vec![rule(
            0,
            ZoneActionKind::StateBased,
            D::Graveyard,
            D::Exile,
            true,
        )],
        B,
        true,
    );
    assert_eq!(replacement(&run).chooser(), B);
    assert_eq!(check_event(&run).pass(), 0);
    assert_eq!(check_batch(&run).apnap(), &[B, A]);
    optional(&mut run, false);
    assert_eq!(replacement(&run).chooser(), A);
    optional(&mut run, false);
    let RuleFrame::Checks(CheckDecision::GraveyardOrder(f)) =
        run.state.pending_decision().unwrap().frame()
    else {
        panic!("owner B arranges both controllers' cards")
    };
    assert_eq!(f.chooser(), B);
    let old = [object(&run.state, 10), object(&run.state, 1)];
    order(&mut run, old.to_vec());
    let changes = run
        .events()
        .into_iter()
        .flat_map(|e| &e.occurrences)
        .filter_map(|o| match &o.value {
            RulesOccurrence::ZoneChange(z) if matches!(z.reason, ZoneReason::StateBased(_)) => {
                Some(z)
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(changes.len(), 2);
    assert!(changes.iter().any(|z|matches!(z.reason,ZoneReason::StateBased(reasons) if reasons.contains(StateBasedMoveReason::World))));
    assert!(changes.iter().any(|z|matches!(z.reason,ZoneReason::StateBased(reasons) if reasons.contains(StateBasedMoveReason::ZeroToughness))));
    run.replay();
    let v = creature_types::selected_vocabulary().unwrap();
    let mut p = creature(0);
    p.supertypes = BTreeSet::from([Supertype::World, Supertype::Legendary]);
    let token = TokenDefinition::new(
        p,
        TokenName::Specified(EnglishCardName::new("Union Probe".into()).unwrap()),
        &v,
    )
    .unwrap();
    let mut union = Run::new(
        vec![
            bf(
                99,
                A,
                A,
                source(vec![rule(
                    0,
                    ZoneActionKind::StateBased,
                    D::Graveyard,
                    D::Exile,
                    true,
                )]),
            ),
            instant(
                1,
                BodyTarget::None,
                vec![Instruction::CreateTokens {
                    creations: vec![TokenCreation {
                        definition: token,
                        amount: crate::program::ValueExpr::Literal(2.into()),
                        base_size: None,
                        enters_tapped: false,
                        recipients: TokenRecipients::Controller,
                    }],
                }],
            ),
        ],
        A,
        Step::PrecombatMain,
    );
    union.cast(A, 1, None);
    union.pass_round();
    let RuleFrame::TimestampOrder(f) = union.state.pending_decision().unwrap().frame() else {
        panic!("simultaneous entries")
    };
    union.reply(DecisionReply::TimestampOrder(
        f.members().iter().map(|m| m.member).collect(),
    ));
    let RuleFrame::Checks(CheckDecision::Legend(f)) =
        union.state.pending_decision().unwrap().frame()
    else {
        panic!("actual legend choice")
    };
    let cohort = &f.conflicts()[0];
    union.reply(DecisionReply::KeepLegend {
        conflict: cohort.id(),
        keep: cohort.domain()[0],
    });
    assert_eq!(check_batch(&union).candidates().len(), 2);
    assert!(check_batch(&union).candidates().values().any(|c|matches!(c.reason,ZoneReason::StateBased(r) if r.contains(StateBasedMoveReason::ZeroToughness)&&r.contains(StateBasedMoveReason::World)&&r.contains(StateBasedMoveReason::Legend))));
    answer_replacements(&mut union, true);
    union.replay();
}

#[test]
fn sba_chain_opportunities_are_per_lineage_and_publish_only_terminal_moves() {
    let mut run = world_start(
        vec![
            rule(0, ZoneActionKind::StateBased, D::Graveyard, D::Exile, false),
            rule(1, ZoneActionKind::StateBased, D::Exile, D::Graveyard, true),
        ],
        A,
        false,
    );
    let ids = [object(&run.state, 10), object(&run.state, 1)];
    select(&mut run, ids[0], 0);
    select(&mut run, ids[0], 1);
    optional(&mut run, true);
    assert_eq!(
        check_batch(&run)
            .applied()
            .values()
            .map(BTreeSet::len)
            .sum::<usize>(),
        3
    );
    assert_eq!(check_batch(&run).optional().len(), 1);
    assert_eq!(zone(&run.state, 10), ObjectZone::Battlefield);
    assert_eq!(zone(&run.state, 1), ObjectZone::Battlefield);
    optional(&mut run, false);
    assert_eq!(zone(&run.state, 10), ObjectZone::Graveyard(A));
    assert_eq!(zone(&run.state, 1), ObjectZone::Exile);
    let changes=run.events().iter().flat_map(|e|&e.occurrences).filter(|o|matches!(&o.value,RulesOccurrence::ZoneChange(z) if matches!(z.reason,ZoneReason::StateBased(_)))).count();
    assert_eq!(changes, 2);
    run.replay();
}

#[test]
fn token_owned_copied_and_removed_sba_sources_use_actual_evaluated_abilities() {
    let v = creature_types::selected_vocabulary().unwrap();
    let mut token_source = creature(0);
    let mut definition = rule(0, ZoneActionKind::StateBased, D::Graveyard, D::Exile, true);
    definition.subject = ZoneReplacementSubject::SourceSelf;
    token_source.zone_replacements = vec![definition];
    let token = TokenDefinition::new(
        token_source,
        TokenName::Default {
            ordered_subtypes: vec![],
        },
        &v,
    )
    .unwrap();
    let mut born = Run::new(
        vec![instant(
            1,
            BodyTarget::None,
            vec![Instruction::CreateTokens {
                creations: vec![TokenCreation::one(token)],
            }],
        )],
        A,
        Step::PrecombatMain,
    );
    born.cast(A, 1, None);
    born.pass_round();
    let batch = check_batch(&born);
    let before = batch.candidates().values().next().unwrap().before.id();
    assert!(batch.sources().keys().all(|s| s.source == before));
    optional(&mut born, true);
    let change = born
        .events()
        .into_iter()
        .flat_map(|e| &e.occurrences)
        .find_map(|o| match &o.value {
            RulesOccurrence::ZoneChange(z) if z.before.object.id() == before => Some(z),
            _ => None,
        })
        .unwrap();
    assert_eq!(change.after.object.zone(), ObjectZone::Exile);
    let departed = change.after.object.id();
    assert!(born.events().iter().flat_map(|e| &e.occurrences).any(
        |o| matches!(&o.value,RulesOccurrence::TokenCeased {object} if object.id()==departed)
    ));
    born.replay();
    let token = TokenDefinition::new(
        creature(0),
        TokenName::Default {
            ordered_subtypes: vec![],
        },
        &v,
    )
    .unwrap();
    let mut holder = Program::artifact(None, vec![]);
    holder.activated.push(ActivatedAbility {
        id: ActivatedAbilityId(0),
        mana_cost: ManaCost::free(),
        source_cost: SourceCost::None,
        body: ExecutableBody::new(
            BodyTarget::SinglePermanent,
            vec![
                Instruction::CopyTargetToSource {
                    duration: ResolvedDuration::NoStatedDuration,
                    power_toughness: None,
                },
                Instruction::CreateTokens {
                    creations: vec![TokenCreation::one(token)],
                },
            ],
        ),
    });
    let mut copied = Run::new(
        vec![
            bf(300, A, A, holder),
            bf(
                99,
                A,
                A,
                source(vec![rule(
                    0,
                    ZoneActionKind::StateBased,
                    D::Graveyard,
                    D::Exile,
                    false,
                )]),
            ),
        ],
        A,
        Step::PrecombatMain,
    );
    copied.command(
        A,
        GameCommand::ActivateAbility {
            source: object(&copied.state, 300),
            ability: ActivatedAbilityId(0),
            target: TargetRef::Object(object(&copied.state, 99)),
            payment: None,
        },
    );
    copied.pass_round();
    let recipient = object(&copied.state, 300);
    assert_eq!(check_batch(&copied).sources().len(), 2);
    let ZoneReplacementRequest::Select { applications } = replacement(&copied).request() else {
        panic!("two actual copied source instances")
    };
    let application = *applications
        .iter()
        .find(|a| a.effect.source == recipient)
        .unwrap();
    copied.reply(DecisionReply::ZoneReplacement(
        ZoneReplacementReply::Select(application),
    ));
    assert!(copied.state.pending_decision().is_none());
    copied.replay();
    let mut stripped_source = creature(2);
    stripped_source.zone_replacements = vec![rule(
        0,
        ZoneActionKind::StateBased,
        D::Graveyard,
        D::Exile,
        false,
    )];
    let mut removed = Run::new(
        vec![
            bf(99, A, A, stripped_source),
            instant(
                1,
                BodyTarget::SingleCreature,
                vec![
                    Instruction::CreateContinuousEffect {
                        parts: ResolvedPartsSpec(vec![ResolvedPartSpec::Abilities(
                            AbilityChange::remove(AbilityRemoval::All),
                        )]),
                        duration: ResolvedDuration::NoStatedDuration,
                    },
                    Instruction::OptionalEffect {
                        chooser: ResolvingPlayer::Controller,
                    },
                    Instruction::Damage { amount: 2.into() },
                ],
            ),
        ],
        A,
        Step::PrecombatMain,
    );
    removed.cast(A, 1, Some(TargetRef::Object(object(&removed.state, 99))));
    removed.pass_round();
    assert!(matches!(
        removed.state.pending_decision().unwrap().frame(),
        RuleFrame::OptionalEffect(_)
    ));
    let traits = removed
        .state
        .execution()
        .current_characteristics(object(&removed.state, 99))
        .unwrap()
        .unwrap();
    assert!(traits.abilities.zone_replacements.is_empty());
    assert_eq!(traits.printed_text.zone_replacements.len(), 1);
    removed.reply(DecisionReply::OptionalEffect(OptionalEffectChoice::Take));
    assert_eq!(zone(&removed.state, 99), ObjectZone::Graveyard(A));
    removed.replay();
}

fn complete_pass_run() -> Run {
    let v = creature_types::selected_vocabulary().unwrap();
    let token = TokenDefinition::new(
        creature(1),
        TokenName::Default {
            ordered_subtypes: vec![],
        },
        &v,
    )
    .unwrap();
    let main = instant(
        1,
        BodyTarget::SingleCreature,
        vec![
            Instruction::CreateTokens {
                creations: vec![TokenCreation::one(token)],
            },
            Instruction::Sacrifice {
                players: AffectedPlayers::One(ResolvingPlayer::Controller),
                amount: crate::program::ValueExpr::Literal(1.into()),
                filter: PermanentTypeFilter {
                    any_types: Default::default(),
                    required_types: BTreeSet::from([crate::program::CardType::Creature]),
                    forbidden_types: BTreeSet::new(),
                },
            },
            Instruction::PutPowerToughnessCounters {
                counters: PowerToughnessCounters {
                    plus_one: 1.into(),
                    minus_one: 1.into(),
                },
            },
            Instruction::Damage { amount: 2.into() },
            Instruction::AdjustControllerLife {
                adjustment: LifeAdjustment::Lose(20.into()),
            },
        ],
    );
    let copier = |card| {
        instant(
            card,
            BodyTarget::SingleSpell(crate::program::SpellFilter::copyable_instruction()),
            vec![Instruction::CopySpell],
        )
    };
    let mut run = Run::new(
        vec![
            main,
            copier(2),
            copier(3),
            bf(10, A, A, creature(2)),
            bf(
                99,
                A,
                A,
                source(vec![rule(
                    0,
                    ZoneActionKind::StateBased,
                    D::Graveyard,
                    D::Exile,
                    true,
                )]),
            ),
        ],
        A,
        Step::PrecombatMain,
    );
    run.cast(A, 1, Some(TargetRef::Object(object(&run.state, 10))));
    let original = run.state.execution().window().entries()[0].id();
    for card in [2, 3] {
        run.cast(A, card, Some(TargetRef::Stack(original)));
        run.pass_round();
        run.reply(DecisionReply::CopyTargets(CopyTargetChoice::Retain));
    }
    run.pass_round();
    let token = run
        .state
        .execution()
        .window()
        .cards()
        .iter()
        .find(|o| o.is_token() && o.zone() == ObjectZone::Battlefield)
        .unwrap()
        .id();
    run.reply(DecisionReply::SelectResolution(vec![token]));
    assert!(matches!(
        replacement(&run).cause(),
        EventCause::StateBasedActions { .. }
    ));
    run
}
#[test]
fn complete_sba_pass_holds_exact_copy_token_counter_and_loss_cohorts_until_one_publication() {
    let mut run = complete_pass_run();
    let effect = run.state.execution();
    let process = effect.check_process().unwrap();
    let CheckPhase::Actions(actions) = &process.phase else {
        panic!("actual complete SBA pass")
    };
    assert_eq!(actions.departed_copies.len(), 1);
    assert_eq!(actions.departed_tokens.len(), 1);
    assert_eq!(actions.cancellations.len(), 1);
    assert_eq!(actions.losses, BTreeSet::from([A]));
    let doomed_copy = *actions.departed_copies.keys().next().unwrap();
    let old_token = *actions.departed_tokens.iter().next().unwrap();
    let other = effect
        .window
        .spell_copies()
        .values()
        .find(|c| c.zone == ObjectZone::Stack)
        .unwrap()
        .clone();
    let current_frame = EffectDecision::ZoneReplacement(replacement(&run).clone());
    // Adversarial retained-work fork only: changed exact copy identity, same
    // nonempty boolean and cardinality; no rule input can perform this mutation.
    let mut corrupt = effect.clone();
    let CheckPhase::Actions(actions) = &mut corrupt.check_process_mut().unwrap().phase else {
        unreachable!()
    };
    actions.departed_copies.clear();
    actions.departed_copies.insert(other.id, other.clone());
    let saved = corrupt.clone();
    assert_eq!(
        corrupt.resume_rule(
            corrupt.revision(),
            &current_frame,
            A,
            DecisionReply::ZoneReplacement(ZoneReplacementReply::Optional(
                ZoneOptionalAnswer::Apply
            )),
            run.state.turn()
        ),
        Err(EffectError::Check(CheckError::WrongFrame))
    );
    assert_eq!(corrupt, saved);
    assert_eq!(zone(&run.state, 10), ObjectZone::Battlefield);
    let last = run.calls.len();
    optional(&mut run, true);
    let candidate = run.calls[last].1.as_ref().unwrap();
    let events = candidate
        .rules_journal()
        .records
        .iter()
        .filter_map(|r| match r {
            RulesRecord::Event(e) if matches!(e.cause, EventCause::StateBasedActions { .. }) => {
                Some(e)
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(events.len(), 1);
    let event = events[0];
    assert!(event.occurrences.iter().any(|o|matches!(&o.value,RulesOccurrence::CountersCancelled {object,..} if *object==object_before(candidate,10))));
    assert!(
        event
            .occurrences
            .iter()
            .any(|o| matches!(&o.value,RulesOccurrence::CopyCeased {copy} if copy.id==doomed_copy))
    );
    assert!(event.occurrences.iter().any(
        |o| matches!(&o.value,RulesOccurrence::TokenCeased {object} if object.id()==old_token)
    ));
    assert!(
        event
            .occurrences
            .iter()
            .any(|o| matches!(o.value,RulesOccurrence::PlayerLost {player,..} if player==A))
    );
    assert_eq!(zone(&run.state, 10), ObjectZone::Exile);
    assert!(
        run.state
            .execution()
            .window()
            .spell_copies()
            .contains_key(&other.id)
    );
    assert!(
        !run.state
            .execution()
            .window()
            .spell_copies()
            .contains_key(&doomed_copy)
    );
    assert_eq!(
        run.state.outcome(),
        Some(crate::draw::GameOutcome::Winner(B))
    );
    run.replay();
}
fn object_before(candidate: &NativeCandidate, card: u64) -> ObjectId {
    candidate
        .check_journal()
        .moves
        .iter()
        .find(|m| m.before.card_id() == Some(CardId(card)))
        .unwrap()
        .before
        .id()
}

#[test]
fn later_sba_pass_uses_fresh_roster_after_source_departure() {
    let mut old = creature(2);
    old.supertypes.insert(Supertype::World);
    old.zone_replacements = vec![rule(
        0,
        ZoneActionKind::StateBased,
        D::Graveyard,
        D::Exile,
        true,
    )];
    old.continuous = Some(StaticContinuousAbility::Battlefield {
        id: StaticAbilityId(0),
        selector: PermanentSelector {
            required_types: BTreeSet::from([crate::program::CardType::Creature]),
            exclude_source: true,
            ..Default::default()
        },
        parts: StaticParts(vec![StaticPart::ModifyPT(NumericPair {
            power: NumericExpression::Literal(0.into()),
            toughness: NumericExpression::Literal(1.into()),
        })]),
    });
    let mut run = Run::new(
        vec![bf(10, A, A, old), bf(20, A, A, creature(0)), world_spell()],
        A,
        Step::PrecombatMain,
    );
    run.cast(A, 1, None);
    run.pass_round();
    let first = check_event(&run);
    assert_eq!(check_batch(&run).candidates().len(), 1);
    assert!(
        check_batch(&run)
            .sources()
            .keys()
            .all(|s| s.source == object(&run.state, 10))
    );
    optional(&mut run, true);
    let RuleFrame::Checks(CheckDecision::GraveyardOrder(f)) =
        run.state.pending_decision().unwrap().frame()
    else {
        panic!("fresh next-pass ordinary G ordering")
    };
    assert_eq!(f.event().started_revision(), first.started_revision());
    assert_eq!(f.event().pass(), first.pass() + 1);
    assert!(!run.state.execution().has_check_replacement_work());
    let order_ids = f.domain().iter().rev().copied().collect();
    order(&mut run, order_ids);
    assert_eq!(zone(&run.state, 10), ObjectZone::Exile);
    assert_eq!(zone(&run.state, 1), ObjectZone::Graveyard(A));
    assert_eq!(zone(&run.state, 20), ObjectZone::Graveyard(A));
    run.replay();
}

#[test]
fn cancellation_cessation_and_loss_only_passes_have_no_zone_intents_or_replacement_frames() {
    for mode in 0..3 {
        let body = match mode {
            0 => vec![Instruction::PutPowerToughnessCounters {
                counters: PowerToughnessCounters {
                    plus_one: 1.into(),
                    minus_one: 1.into(),
                },
            }],
            1 => vec![gain(1)],
            _ => vec![Instruction::AdjustControllerLife {
                adjustment: LifeAdjustment::Lose(20.into()),
            }],
        };
        let target = if mode == 0 {
            BodyTarget::SingleCreature
        } else {
            BodyTarget::None
        };
        let mut cards = vec![
            instant(1, target, body),
            bf(10, A, A, creature(3)),
            bf(
                99,
                A,
                A,
                source(vec![rule(
                    0,
                    ZoneActionKind::StateBased,
                    D::Graveyard,
                    D::Exile,
                    true,
                )]),
            ),
        ];
        if mode == 1 {
            cards.push(instant(
                2,
                BodyTarget::SingleSpell(crate::program::SpellFilter::copyable_instruction()),
                vec![Instruction::CopySpell],
            ));
        }
        let mut run = Run::new(cards, A, Step::PrecombatMain);
        let chosen = if mode == 0 {
            Some(TargetRef::Object(object(&run.state, 10)))
        } else {
            None
        };
        run.cast(A, 1, chosen);
        if mode == 1 {
            let original = run.state.execution().window().entries()[0].id();
            run.cast(A, 2, Some(TargetRef::Stack(original)));
            run.pass_round();
            if matches!(
                run.state.pending_decision().map(|p| p.frame()),
                Some(RuleFrame::CopyTargets(_))
            ) {
                run.reply(DecisionReply::CopyTargets(CopyTargetChoice::Retain));
            }
        }
        run.pass_round();
        assert!(run.state.pending_decision().is_none());
        let sbas = run
            .events()
            .into_iter()
            .filter(|e| matches!(e.cause, EventCause::StateBasedActions { .. }))
            .collect::<Vec<_>>();
        assert!(!sbas.is_empty());
        assert!(
            sbas.iter()
                .flat_map(|e| &e.occurrences)
                .all(|o| !matches!(o.value, RulesOccurrence::ZoneChange(_)))
        );
        assert!(
            run.calls
                .iter()
                .filter_map(|(_, r)| r.as_ref().ok())
                .all(|c| c.check_journal().moves.is_empty())
        );
        match mode {
            0 => assert!(
                sbas.iter()
                    .flat_map(|e| &e.occurrences)
                    .any(|o| matches!(o.value, RulesOccurrence::CountersCancelled { .. }))
            ),
            1 => assert!(
                sbas.iter()
                    .flat_map(|e| &e.occurrences)
                    .any(|o| matches!(o.value, RulesOccurrence::CopyCeased { .. }))
            ),
            _ => assert!(run.state.outcome().is_some()),
        }
        run.replay();
    }
}

fn rejection(
    effect: &EffectWindow,
    frame: &EffectDecision,
    actor: SeatId,
    reply: DecisionReply,
    turn: Option<&TurnScheduler>,
    expected: EffectError,
) {
    let saved = effect.clone();
    assert_eq!(
        effect.resume_rule(effect.revision(), frame, actor, reply.clone(), turn),
        Err(expected.clone())
    );
    assert_eq!(*effect, saved);
    assert_eq!(
        effect.resume_rule(effect.revision(), frame, actor, reply, turn),
        Err(expected)
    );
    assert_eq!(*effect, saved);
}
fn root_fault(source: &GameState, choice: DecisionReply, expected: GameError) {
    let p = source.pending_decision().unwrap();
    let input = GameInput {
        basis: source.basis().clone(),
        actor: p.chooser(),
        command: GameCommand::Reply {
            decision: p.id(),
            choice,
        },
    };
    let saved = source.clone();
    assert_eq!(source.advance(input.clone()), Err(expected.clone()));
    assert_eq!(*source, saved);
    assert_eq!(source.advance(input), Err(expected));
    assert_eq!(*source, saved);
}
#[test]
fn real_issued_authority_replay_and_disclosed_late_fault_forks_preserve_whole_source() {
    let mut run = cleanup_run(true);
    let ids = [10, 11, 12].map(|id| object(&run.state, id));
    let old_decision = run.state.pending_decision().unwrap().id();
    let p = run.state.pending_decision().unwrap();
    let wrong = GameInput {
        basis: run.state.basis().clone(),
        actor: B,
        command: GameCommand::Reply {
            decision: p.id(),
            choice: DecisionReply::Discard(ids.to_vec()),
        },
    };
    assert_eq!(run.call(wrong), Err(GameError::WrongChooser));
    run.reply(DecisionReply::Discard(ids.to_vec()));
    let original = replacement(&run).clone();
    let ZoneReplacementRequest::Select { applications } = original.request() else {
        panic!("exact application set")
    };
    let app = applications[0];
    let effect = run.state.execution();
    let mut wrong_app = app;
    wrong_app.event = wrong_app
        .event
        .qualification_with_before(object(&run.state, 90));
    rejection(
        effect,
        &EffectDecision::ZoneReplacement(original.clone()),
        A,
        DecisionReply::ZoneReplacement(ZoneReplacementReply::Select(wrong_app)),
        run.state.turn(),
        EffectError::ZoneReplacement(ZoneReplacementError::InapplicableApplication),
    );
    rejection(
        effect,
        &EffectDecision::ZoneReplacement(original.qualification_with_stage(original.stage() + 1)),
        A,
        DecisionReply::ZoneReplacement(ZoneReplacementReply::Select(app)),
        run.state.turn(),
        EffectError::Check(CheckError::WrongFrame),
    );
    let cause = match original.cause() {
        EventCause::CleanupDiscard { event, .. } => EventCause::StateBasedActions {
            event: *event,
            boundary: CheckBoundary::Priority,
        },
        _ => unreachable!(),
    };
    rejection(
        effect,
        &EffectDecision::ZoneReplacement(original.qualification_with_cause(cause)),
        A,
        DecisionReply::ZoneReplacement(ZoneReplacementReply::Select(app)),
        run.state.turn(),
        EffectError::Check(CheckError::WrongFrame),
    );
    if let EventCause::CleanupDiscard {
        event,
        boundary: CheckBoundary::Cleanup(position),
    } = original.cause()
    {
        let next_event = CheckEventId {
            started_revision: event.started_revision(),
            pass: event.pass() + 1,
        };
        rejection(
            effect,
            &EffectDecision::ZoneReplacement(original.qualification_with_cause(
                EventCause::CleanupDiscard {
                    event: next_event,
                    boundary: CheckBoundary::Cleanup(*position),
                },
            )),
            A,
            DecisionReply::ZoneReplacement(ZoneReplacementReply::Select(app)),
            run.state.turn(),
            EffectError::Check(CheckError::WrongFrame),
        );
        let mut next = *position;
        next.cleanup_iteration += 1;
        rejection(
            effect,
            &EffectDecision::ZoneReplacement(original.qualification_with_cause(
                EventCause::CleanupDiscard {
                    event: *event,
                    boundary: CheckBoundary::Cleanup(next),
                },
            )),
            A,
            DecisionReply::ZoneReplacement(ZoneReplacementReply::Select(app)),
            run.state.turn(),
            EffectError::Check(CheckError::WrongFrame),
        );
    }
    let saved = effect.clone();
    let mut stale = effect.clone();
    stale.window.set_test_revision(stale.window.revision() + 1);
    let invalid = stale.clone();
    assert!(
        stale
            .resume_rule(
                stale.revision(),
                &EffectDecision::ZoneReplacement(original.clone()),
                A,
                DecisionReply::ZoneReplacement(ZoneReplacementReply::Select(app)),
                run.state.turn()
            )
            .is_err()
    );
    assert_eq!(stale, invalid);
    assert_eq!(*effect, saved);
    let mut exhausted = effect.clone();
    let CheckPhase::CleanupMoves { moves, .. } = &mut exhausted.check_process_mut().unwrap().phase
    else {
        panic!("owned cleanup")
    };
    let CheckMoveStage::Replacing(batch) = &mut moves.stage else {
        unreachable!()
    };
    batch.qualification_exhaust_stage();
    let current =
        EffectDecision::ZoneReplacement(exhausted.check_replacement_frame().unwrap().unwrap());
    rejection(
        &exhausted,
        &current,
        A,
        DecisionReply::ZoneReplacement(ZoneReplacementReply::Select(app)),
        run.state.turn(),
        EffectError::ZoneReplacement(ZoneReplacementError::StageExhausted),
    );
    let stale_input = GameInput {
        basis: run.state.basis().clone(),
        actor: A,
        command: GameCommand::Reply {
            decision: old_decision,
            choice: DecisionReply::ZoneReplacement(ZoneReplacementReply::Select(app)),
        },
    };
    assert_eq!(run.call(stale_input), Err(GameError::WrongDecision));
    select(&mut run, ids[0], 0);
    optional(&mut run, true);
    select(&mut run, ids[1], 0);
    optional(&mut run, false);
    optional(&mut run, false);
    let order_ids = vec![ids[2], ids[1]];
    let live = run.state.execution();
    let frame = EffectDecision::Checks(live.current_check_decision().unwrap());
    rejection(
        live,
        &frame,
        A,
        DecisionReply::OrderGraveyard(vec![ids[1], ids[1]]),
        run.state.turn(),
        EffectError::Check(CheckError::InvalidGraveyardOrder),
    );
    // Fault forks of a genuinely issued root cut. Only the named operational
    // counter changes; these forks are not claimed as reachable input prefixes.
    // Actual physical-owner prefix proof for the identity fault below. This
    // returned local prefix is not adopted as a complete cleanup transaction.
    let mut physical = live.window.clone();
    physical.set_test_next_object_identity(u64::MAX - 1);
    let application = [
        (ids[1], D::Graveyard),
        (ids[2], D::Graveyard),
        (ids[0], D::Exile),
    ];
    assert_eq!(
        physical
            .record_cleanup_moves(physical.revision(), A, &application[..1])
            .unwrap()
            .1
            .len(),
        1
    );
    assert_eq!(
        physical
            .record_cleanup_moves(physical.revision(), A, &application)
            .unwrap_err(),
        StackWindowError::Objects(ObjectIdentityError::IdentityExhausted)
    );
    for (fault, error) in [
        (
            QualificationFault::ObjectIdentity(u64::MAX - 1),
            EffectError::Window(StackWindowError::Objects(
                ObjectIdentityError::IdentityExhausted,
            )),
        ),
        (
            QualificationFault::Event(u64::MAX),
            EffectError::Trigger(TriggerError::EventIdentityExhausted),
        ),
    ] {
        let mut faulted = run.state.clone();
        faulted.set_test_check_effect_fault(fault);
        root_fault(
            &faulted,
            DecisionReply::OrderGraveyard(order_ids.clone()),
            GameError::Effect(error),
        );
    }
    // A malformed retained history must fail during the final common replay.
    let mut bad = live.clone();
    let CheckPhase::CleanupMoves { moves, .. } = &mut bad.check_process_mut().unwrap().phase else {
        unreachable!()
    };
    let CheckMoveStage::Ordering {
        replacements: Some(batch),
        ..
    } = &mut moves.stage
    else {
        unreachable!()
    };
    batch.qualification_corrupt_history();
    let saved = bad.clone();
    assert!(
        bad.resume_rule(
            bad.revision(),
            &frame,
            A,
            DecisionReply::OrderGraveyard(order_ids.clone()),
            run.state.turn()
        )
        .is_err()
    );
    assert_eq!(bad, saved);
    order(&mut run, order_ids);
    run.replay();
    // Genuine issued continuation: final cleanup commit creates three waiting
    // triggers, then allocating their root order frame can fail after local moves.
    let mut cards = cards_hand(10);
    cards.push(cleanup_source(true));
    let mut watcher = Program::artifact(None, vec![]);
    watcher.triggered.push(TriggeredAbility {
        id: TriggeredAbilityId(0),
        condition: crate::program::TriggerSpec::player_action(
            PlayerRelation::Controller,
            crate::program::PlayerTriggerAction::Discarded,
        ),
        grouping: TriggerGrouping::EachOccurrence,
        body: ExecutableBody::new(BodyTarget::None, vec![gain(1)]),
    });
    cards.push(bf(400, A, A, watcher));
    let mut continued = Run::new(cards, A, Step::End);
    continued.pass_round();
    let ids = [10, 11, 12].map(|id| object(&continued.state, id));
    continued.reply(DecisionReply::Discard(ids.to_vec()));
    answer_replacements(&mut continued, false);
    let mut fault = continued.state.clone();
    fault.set_test_next_decision(u64::MAX);
    root_fault(
        &fault,
        DecisionReply::OrderGraveyard(ids.to_vec()),
        GameError::DecisionIdentityExhausted,
    );
    order(&mut continued, ids.to_vec());
    assert!(matches!(
        continued.state.pending_decision().unwrap().frame(),
        RuleFrame::Triggers(_)
    ));
    continued.replay();
}

#[test]
fn new_cleanup_journal_and_retained_work_refuse_old_semantics_in_isolation() {
    let mut run = cleanup_run(true);
    let ids = [10, 11, 12].map(|id| object(&run.state, id));
    let accepted = run.reply(DecisionReply::Discard(ids.to_vec()));
    let standalone = accepted.check_journal().clone();
    assert!(standalone.moves.is_empty());
    assert_eq!(standalone.choices.len(), 1);
    assert!(standalone.check_current10().is_err());
    assert!(
        check_trace::check_journal_view(
            &standalone,
            run.state.execution().window().cards(),
            |id| id.0.to_string()
        )
        .is_err()
    );
    let mut isolated = run.state.execution().clone();
    for p in Arc::make_mut(&mut isolated.programs).values_mut() {
        p.zone_replacements.clear();
    }
    isolated.payloads.clear();
    assert!(
        isolated
            .programs()
            .all(|(_, p)| p.zone_replacements.is_empty())
    );
    assert!(isolated.check_current10().is_err());
    assert!(projection::pending_work(&isolated).is_err());
    let encoded = projection::zone_replacement_work(isolated.check_zone_replacement().unwrap());
    assert_eq!(encoded["schema"], "zone-replacement-native-work-v2");
    assert_eq!(encoded["apnap"], serde_json::json!([A.0, B.0]));
    assert_eq!(encoded["candidates"].as_array().unwrap().len(), 3);
    assert_eq!(encoded["candidates"][0]["key"]["tag"], "Check");
    assert!(encoded["cause"].is_object());
    assert!(!encoded["sources"].as_array().unwrap().is_empty());
    answer_replacements(&mut run, false);
    let mut ordered = run.state.execution().clone();
    for p in Arc::make_mut(&mut ordered.programs).values_mut() {
        p.zone_replacements.clear();
    }
    ordered.payloads.clear();
    assert!(ordered.check_current10().is_err());
    assert!(projection::pending_work(&ordered).is_err());
    let RuleFrame::Checks(CheckDecision::GraveyardOrder(f)) =
        run.state.pending_decision().unwrap().frame()
    else {
        panic!("retained settled work")
    };
    let actual_order = f.domain().to_vec();
    order(&mut run, actual_order);
    run.replay();
    let v = creature_types::selected_vocabulary().unwrap();
    for action in [ZoneActionKind::CleanupDiscard, ZoneActionKind::StateBased] {
        let p = source(vec![rule(0, action, D::Graveyard, D::Exile, true)]);
        assert_eq!(codec::decode(codec::view(&p), &v).unwrap(), p);
        let mut any = p.clone();
        any.zone_replacements[0].actions = ZoneActionPredicate::Any;
        assert!(any.validate(&v).is_err());
        let mut mixed = p.clone();
        mixed.zone_replacements[0].actions =
            ZoneActionPredicate::OneOf(BTreeSet::from([action, ZoneActionKind::SacrificeCost]));
        assert_eq!(mixed.validate(&v), Ok(()));
        assert_eq!(codec::decode(codec::view(&mixed), &v).unwrap(), mixed);
        mixed.zone_replacements[0].actions =
            ZoneActionPredicate::OneOf(BTreeSet::from([action, ZoneActionKind::LandPlay]));
        assert_eq!(
            mixed.validate(&v),
            Err(crate::program::DefinitionError::UnsupportedZoneReplacementScope)
        );
    }
}

#[test]
fn already_departed_token_cessation_only_pass_does_not_construct_empty_replacement_work() {
    let v = creature_types::selected_vocabulary().unwrap();
    let token = TokenDefinition::new(
        creature(1),
        TokenName::Default {
            ordered_subtypes: vec![],
        },
        &v,
    )
    .unwrap();
    let mut run = Run::new(
        vec![
            bf(
                99,
                A,
                A,
                source(vec![rule(
                    0,
                    ZoneActionKind::StateBased,
                    D::Graveyard,
                    D::Exile,
                    true,
                )]),
            ),
            instant(
                1,
                BodyTarget::None,
                vec![
                    Instruction::CreateTokens {
                        creations: vec![TokenCreation::one(token)],
                    },
                    Instruction::Sacrifice {
                        players: AffectedPlayers::One(ResolvingPlayer::Controller),
                        amount: crate::program::ValueExpr::Literal(1.into()),
                        filter: PermanentTypeFilter {
                            any_types: Default::default(),
                            required_types: BTreeSet::from([crate::program::CardType::Creature]),
                            forbidden_types: BTreeSet::new(),
                        },
                    },
                    gain(1),
                ],
            ),
        ],
        A,
        Step::PrecombatMain,
    );
    run.cast(A, 1, None);
    run.pass_round();
    let token = run
        .state
        .execution()
        .window()
        .cards()
        .iter()
        .find(|c| c.is_token())
        .unwrap()
        .id();
    run.reply(DecisionReply::SelectResolution(vec![token]));
    assert!(run.state.pending_decision().is_none());
    let events = run
        .events()
        .into_iter()
        .filter(|e| matches!(e.cause, EventCause::StateBasedActions { .. }))
        .collect::<Vec<_>>();
    assert_eq!(events.len(), 1);
    assert!(
        events[0]
            .occurrences
            .iter()
            .any(|o| matches!(o.value, RulesOccurrence::TokenCeased { .. }))
    );
    assert!(
        events[0]
            .occurrences
            .iter()
            .all(|o| !matches!(o.value, RulesOccurrence::ZoneChange(_)))
    );
    assert!(
        run.calls
            .iter()
            .filter_map(|(_, r)| r.as_ref().ok())
            .all(|c| c.check_journal().moves.is_empty())
    );
    run.replay();
}

#[test]
fn cleanup_sba_after_real_expiry_uses_pass_one_and_the_actual_turn_work_parent() {
    let mut run = Run::new(
        vec![
            bf(10, A, A, creature(1)),
            bf(
                99,
                A,
                A,
                source(vec![rule(
                    0,
                    ZoneActionKind::StateBased,
                    D::Graveyard,
                    D::Exile,
                    true,
                )]),
            ),
            instant(
                1,
                BodyTarget::SingleCreature,
                vec![
                    Instruction::CreateContinuousEffect {
                        parts: ResolvedPartsSpec(vec![ResolvedPartSpec::ModifyPT(
                            crate::characteristics::PowerToughness {
                                power: 0.into(),
                                toughness: 1.into(),
                            },
                        )]),
                        duration: ResolvedDuration::UntilEndOfTurn,
                    },
                    Instruction::PutPowerToughnessCounters {
                        counters: PowerToughnessCounters {
                            plus_one: 0.into(),
                            minus_one: 1.into(),
                        },
                    },
                ],
            ),
        ],
        A,
        Step::End,
    );
    run.cast(A, 1, Some(TargetRef::Object(object(&run.state, 10))));
    run.pass_round();
    assert_eq!(zone(&run.state, 10), ObjectZone::Battlefield);
    run.pass_round();
    assert_eq!(check_event(&run).pass(), 1);
    assert_eq!(run.state.pending_turn_work(), Some(TurnWork::Cleanup));
    assert!(run.state.execution().resolution_state().is_none());
    assert!(
        matches!(replacement(&run).cause(),EventCause::StateBasedActions {boundary:CheckBoundary::Cleanup(position),..} if *position==run.state.turn().unwrap().position())
    );
    optional(&mut run, true);
    assert_eq!(zone(&run.state, 10), ObjectZone::Exile);
    assert_eq!(run.state.turn().unwrap().position().step, Step::Cleanup);
    run.pass_round();
    assert!(run.events().iter().any(|e|matches!(e.cause,EventCause::StepBegan {position} if position.step==Step::Cleanup&&position.cleanup_iteration==1)));
    run.replay();
}
#[test]
fn lethal_damage_waits_through_real_resolution_choice_then_enters_the_sba_adapter() {
    let mut run = Run::new(
        vec![
            bf(10, A, A, creature(2)),
            bf(
                99,
                A,
                A,
                source(vec![rule(
                    0,
                    ZoneActionKind::StateBased,
                    D::Graveyard,
                    D::Exile,
                    true,
                )]),
            ),
            instant(
                1,
                BodyTarget::SingleCreature,
                vec![
                    Instruction::Damage { amount: 2.into() },
                    Instruction::OptionalEffect {
                        chooser: ResolvingPlayer::Controller,
                    },
                    gain(1),
                ],
            ),
        ],
        A,
        Step::PrecombatMain,
    );
    run.cast(A, 1, Some(TargetRef::Object(object(&run.state, 10))));
    run.pass_round();
    assert!(matches!(
        run.state.pending_decision().unwrap().frame(),
        RuleFrame::OptionalEffect(_)
    ));
    assert_eq!(zone(&run.state, 10), ObjectZone::Battlefield);
    assert!(!run.state.execution().has_check_replacement_work());
    assert!(
        !run.events()
            .iter()
            .any(|e| matches!(e.cause, EventCause::StateBasedActions { .. }))
    );
    run.reply(DecisionReply::OptionalEffect(OptionalEffectChoice::Take));
    assert!(check_batch(&run).candidates().values().all(|c|matches!(c.reason,ZoneReason::StateBased(reasons) if reasons.contains(StateBasedMoveReason::LethalDestruction))));
    assert_eq!(run.state.execution().life_total(A), Some(&21.into()));
    optional(&mut run, true);
    assert_eq!(zone(&run.state, 10), ObjectZone::Exile);
    run.replay();
}
