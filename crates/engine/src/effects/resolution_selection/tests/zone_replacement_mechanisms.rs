//! Synthetic typed rule definitions through actual native transitions and full replay.
//! No saved card source or complete-card qualification is claimed.
use super::*;
use crate::{
    activated::{ActivatedAbility, ActivatedAbilityId, SourceCost},
    characteristics::{
        AbilityChange, AbilityRemoval, ResolvedDuration, ResolvedPartSpec, ResolvedPartsSpec,
    },
    program::{
        DefinitionError, ObjectRelation, OrdinaryCondition, PlayerRelation,
        PublicZoneDestination as Destination, TriggerGrouping, TriggeredAbility,
        TriggeredAbilityId, ZoneActionKind, ZoneActionPredicate, ZoneObjectKind,
        ZoneReplacementAbility, ZoneReplacementAbilityId, ZoneReplacementChoice,
        ZoneReplacementSubject,
    },
    test_support::{current_profile::applicability::NativeCaptureValue, program as codec},
};

struct Trace {
    initial: GameState,
    state: GameState,
    steps: Vec<(GameInput, NativeCandidate)>,
}
impl Trace {
    fn new(state: GameState) -> Self {
        Self {
            initial: state.clone(),
            state,
            steps: vec![],
        }
    }
    fn step(&mut self, actor: SeatId, command: GameCommand) -> NativeCandidate {
        let input = GameInput {
            basis: self.state.basis().clone(),
            actor,
            command,
        };
        let before = self.state.clone();
        let result = self.state.advance(input.clone()).unwrap();
        assert_eq!(self.state, before);
        assert_eq!(self.state.advance(input.clone()).unwrap(), result);
        self.state = result.state().clone();
        self.steps.push((input, result.clone()));
        result
    }
    fn cast(&mut self, actor: SeatId, card: u64, target: Option<TargetRef>) {
        let spell = id(&self.state, card);
        let command = target.map_or(
            GameCommand::CastUntargeted {
                spell,
                payment: None,
            },
            |target| GameCommand::Cast { spell, target },
        );
        self.step(actor, command);
    }
    fn boundary(&mut self) {
        for _ in 0..self.state.execution().window().participants().len() {
            if self.state.pending_decision().is_some() {
                return;
            }
            let WindowPhase::Priority(PriorityPhase::Ready { holder }) =
                self.state.execution().window().phase()
            else {
                panic!("ready priority")
            };
            self.step(holder, GameCommand::Pass);
        }
    }
    fn reply(&mut self, choice: DecisionReply) -> NativeCandidate {
        let pending = self.state.pending_decision().unwrap();
        self.step(
            pending.chooser(),
            GameCommand::Reply {
                decision: pending.id(),
                choice,
            },
        )
    }
    fn replay(&self) {
        let mut state = self.initial.clone();
        for (input, expected) in &self.steps {
            let actual = state.advance(input.clone()).unwrap();
            assert_eq!(&actual, expected, "whole original native Result replay");
            state = actual.into_state();
        }
        assert_eq!(state, self.state);
    }
}
fn rule(number: u64, from: Destination, to: Destination, optional: bool) -> ZoneReplacementAbility {
    ZoneReplacementAbility {
        id: ZoneReplacementAbilityId(number),
        actions: ZoneActionPredicate::OneOf(BTreeSet::from([
            ZoneActionKind::EffectSacrifice,
            ZoneActionKind::EffectDiscard,
        ])),
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
fn source(abilities: Vec<ZoneReplacementAbility>) -> Program {
    let mut p = Program::enchantment(None);
    p.zone_replacements = abilities;
    p
}
fn bf(card: u64, owner: SeatId, p: Program) -> SeedCard {
    seed(
        card,
        owner,
        ObjectDestination::Battlefield { controller: owner },
        p,
    )
}
fn start(body: Vec<Instruction>, cards: Vec<SeedCard>) -> Trace {
    let mut t = Trace::new(fixture(BodyTarget::None, body, vec![A, B], A, cards));
    t.cast(A, 1, None);
    t.boundary();
    t
}
fn sacrifice_body(count: i64) -> Vec<Instruction> {
    vec![
        sacrifice(
            AffectedPlayers::One(ResolvingPlayer::Controller),
            count.into(),
        ),
        gain(4),
    ]
}
fn replacement(state: &GameState) -> &ZoneReplacementFrame {
    let RuleFrame::ZoneReplacement(frame) = state.pending_decision().unwrap().frame() else {
        panic!("replacement frame")
    };
    frame
}
fn work(state: &GameState) -> &ZoneReplacementBatch {
    let ResolutionSelectionStage::Replacing { batch: work, .. } = state
        .execution()
        .resolution_selection_state()
        .unwrap()
        .stage
    else {
        panic!("replacement work")
    };
    work
}
fn optional(t: &mut Trace, answer: ZoneOptionalAnswer) -> NativeCandidate {
    assert!(matches!(
        replacement(&t.state).request(),
        ZoneReplacementRequest::ConfirmOptional { .. }
    ));
    t.reply(DecisionReply::ZoneReplacement(
        ZoneReplacementReply::Optional(answer),
    ))
}
fn choose(t: &mut Trace, event: ObjectId, ability: u64, source: ObjectId) {
    let ZoneReplacementRequest::Select { applications } = replacement(&t.state).request() else {
        panic!("pair order")
    };
    let application = *applications
        .iter()
        .find(|a| {
            a.event.before() == event
                && a.effect.ability == ZoneReplacementAbilityId(ability)
                && a.effect.source == source
        })
        .unwrap();
    t.reply(DecisionReply::ZoneReplacement(
        ZoneReplacementReply::Select(application),
    ));
}
fn zone(state: &GameState, card: u64) -> ObjectZone {
    state
        .execution()
        .window()
        .cards()
        .get(id(state, card))
        .unwrap()
        .zone()
}
fn selected_changes(result: &NativeCandidate) -> Vec<&crate::occurrence::ZoneOccurrence> {
    events(result)
        .into_iter()
        .flat_map(|e| &e.occurrences)
        .filter_map(|row| match &row.value {
            RulesOccurrence::ZoneChange(c)
                if matches!(
                    c.reason,
                    ZoneReason::Sacrifice { .. } | ZoneReason::Discard { .. }
                ) =>
            {
                Some(c.as_ref())
            }
            _ => None,
        })
        .collect()
}

#[test]
fn mandatory_sacrifice_uses_actual_exile_and_has_no_dies_capture() {
    let mut p = source(vec![rule(
        1,
        Destination::Graveyard,
        Destination::Exile,
        false,
    )]);
    p.triggered.push(TriggeredAbility {
        id: TriggeredAbilityId(1),
        condition: OrdinaryCondition::Dies(ObjectRelation::AnyCreature),
        grouping: TriggerGrouping::EachOccurrence,
        body: ExecutableBody::new(BodyTarget::None, vec![gain(1)]),
    });
    let mut t = start(sacrifice_body(1), vec![bf(50, B, p), bf(10, A, creature())]);
    let before = id(&t.state, 10);
    let result = t.reply(DecisionReply::SelectResolution(vec![before]));
    let changes = selected_changes(&result);
    assert_eq!(changes.len(), 1);
    assert_eq!(changes[0].after.object.zone(), ObjectZone::Exile);
    assert_ne!(changes[0].after.object.id(), before);
    assert!(t.state.execution().window().cards().get(before).is_none());
    assert!(
        !result
            .rules_journal()
            .records
            .iter()
            .any(|r| matches!(r, RulesRecord::TriggerDiscovered(_)))
    );
    assert_eq!(t.state.execution().life_total(A), Some(&24.into()));
    t.replay();
}
#[test]
fn optional_singleton_discard_apply_and_decline_remain_discard_actions() {
    for answer in [ZoneOptionalAnswer::Apply, ZoneOptionalAnswer::Decline] {
        let mut p = source(vec![rule(
            1,
            Destination::Graveyard,
            Destination::Exile,
            true,
        )]);
        p.triggered.push(TriggeredAbility {
            id: TriggeredAbilityId(1),
            condition: OrdinaryCondition::Discarded(PlayerRelation::AnyParticipant),
            grouping: TriggerGrouping::EachOccurrence,
            body: ExecutableBody::new(BodyTarget::None, vec![gain(1)]),
        });
        let mut t = start(
            vec![Instruction::Discard {
                players: AffectedPlayers::One(ResolvingPlayer::Controller),
                amount: InstructionAmount::Literal(1.into()),
            }],
            vec![
                bf(50, B, p),
                seed(10, A, ObjectDestination::Hand, creature()),
            ],
        );
        let before = id(&t.state, 10);
        let selected = t.reply(DecisionReply::SelectResolution(vec![before]));
        assert!(selected.rules_journal().records.is_empty());
        assert_eq!(zone(&t.state, 10), ObjectZone::Hand(A));
        assert_eq!(replacement(&t.state).chooser(), A);
        let result = optional(&mut t, answer);
        let changes = selected_changes(&result);
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].reason, ZoneReason::Discard { player: A });
        assert_eq!(
            changes[0].after.object.zone(),
            if answer == ZoneOptionalAnswer::Apply {
                ObjectZone::Exile
            } else {
                ObjectZone::Graveyard(A)
            }
        );
        assert_eq!(
            result
                .rules_journal()
                .records
                .iter()
                .filter(|r| matches!(r, RulesRecord::TriggerDiscovered(_)))
                .count(),
            1
        );
        t.replay();
    }
}
#[test]
fn overlapping_effect_order_is_affected_player_choice_and_recomputed() {
    let mut t = start(
        sacrifice_body(1),
        vec![
            bf(
                50,
                B,
                source(vec![
                    rule(1, Destination::Graveyard, Destination::Exile, false),
                    rule(2, Destination::Graveyard, Destination::Exile, false),
                ]),
            ),
            bf(10, A, creature()),
        ],
    );
    let object = id(&t.state, 10);
    let source = id(&t.state, 50);
    t.reply(DecisionReply::SelectResolution(vec![object]));
    assert_eq!(replacement(&t.state).chooser(), A);
    assert_eq!(
        work(&t.state).candidates()[&ZoneEventKey::Instruction {
            entry: instruction_context(replacement(&t.state)).0,
            instruction: 0,
            before: object
        }]
            .current,
        Destination::Graveyard
    );
    let mut invalid = match replacement(&t.state).request() {
        ZoneReplacementRequest::Select { applications } => applications[0],
        _ => panic!("select"),
    };
    invalid.effect.source = id(&t.state, 1);
    let state = t.state.clone();
    let pending = state.pending_decision().unwrap();
    assert!(
        state
            .advance(GameInput {
                basis: state.basis().clone(),
                actor: A,
                command: GameCommand::Reply {
                    decision: pending.id(),
                    choice: DecisionReply::ZoneReplacement(ZoneReplacementReply::Select(invalid))
                }
            })
            .is_err()
    );
    assert_eq!(state, t.state);
    choose(&mut t, object, 2, source);
    assert_eq!(zone(&t.state, 10), ObjectZone::Exile);
    assert!(t.state.pending_decision().is_none());
    t.replay();
}
#[test]
fn changed_destination_chain_has_one_incarnation_and_no_intermediate_event() {
    let mut t = start(
        sacrifice_body(1),
        vec![
            bf(
                50,
                B,
                source(vec![
                    rule(1, Destination::Graveyard, Destination::Exile, true),
                    rule(2, Destination::Exile, Destination::Graveyard, false),
                    rule(3, Destination::Graveyard, Destination::Exile, true),
                ]),
            ),
            bf(10, A, creature()),
        ],
    );
    let object = id(&t.state, 10);
    let source = id(&t.state, 50);
    t.reply(DecisionReply::SelectResolution(vec![object]));
    choose(&mut t, object, 1, source);
    optional(&mut t, ZoneOptionalAnswer::Apply);
    let retained = work(&t.state);
    assert_eq!(retained.history().len(), 2);
    assert_eq!(retained.applied().values().next().unwrap().len(), 2);
    assert_eq!(
        retained.candidates().values().next().unwrap().current,
        Destination::Graveyard
    );
    assert_eq!(zone(&t.state, 10), ObjectZone::Battlefield);
    let result = optional(&mut t, ZoneOptionalAnswer::Decline);
    let changes = selected_changes(&result);
    assert_eq!(changes.len(), 1);
    assert_eq!(changes[0].before.object.id(), object);
    assert_eq!(changes[0].after.object.zone(), ObjectZone::Graveyard(A));
    assert_ne!(changes[0].after.object.id(), object);
    t.replay();
}
#[test]
fn declined_opportunity_is_distinct_and_source_departure_keeps_other_lineages() {
    let mut provider = creature();
    provider.zone_replacements = vec![
        rule(1, Destination::Graveyard, Destination::Exile, true),
        rule(2, Destination::Graveyard, Destination::Exile, true),
        rule(3, Destination::Exile, Destination::Graveyard, false),
    ];
    let mut t = start(
        sacrifice_body(2),
        vec![bf(10, A, provider), bf(11, A, creature())],
    );
    let first = id(&t.state, 10);
    let second = id(&t.state, 11);
    t.reply(DecisionReply::SelectResolution(vec![first, second]));
    choose(&mut t, first, 1, first);
    optional(&mut t, ZoneOptionalAnswer::Decline);
    choose(&mut t, first, 2, first);
    optional(&mut t, ZoneOptionalAnswer::Apply);
    // First lineage can be returned to G, but its declined effect is not reoffered.
    choose(&mut t, first, 3, first);
    assert!(
        work(&t.state)
            .optional()
            .iter()
            .any(|(a, v)| a.event.before() == first
                && a.effect.ability == ZoneReplacementAbilityId(1)
                && *v == ZoneOptionalAnswer::Decline)
    );
    assert!(!work(&t.state).applied().iter().any(|(key, used)| {
        key.before() == first
            && used
                .iter()
                .any(|e| e.ability == ZoneReplacementAbilityId(1))
    }));
    choose(&mut t, second, 1, first);
    optional(&mut t, ZoneOptionalAnswer::Apply);
    // Only second-lineage return remains before its other optional opportunity.
    if matches!(
        replacement(&t.state).request(),
        ZoneReplacementRequest::Select { .. }
    ) {
        choose(&mut t, second, 3, first);
    }
    optional(&mut t, ZoneOptionalAnswer::Decline);
    assert_eq!(
        order_frame(&t.state)
            .domain()
            .iter()
            .copied()
            .collect::<BTreeSet<_>>(),
        BTreeSet::from([first, second])
    );
    t.reply(DecisionReply::OrderResolutionGraveyard(vec![second, first]));
    assert_eq!(zone(&t.state, 10), ObjectZone::Graveyard(A));
    assert_eq!(zone(&t.state, 11), ObjectZone::Graveyard(A));
    t.replay();
}
#[test]
fn copied_sources_keep_recipient_instance_identity_after_donor_departure() {
    let mut donor = creature();
    donor.zone_replacements = vec![rule(1, Destination::Graveyard, Destination::Exile, true)];
    let copier = Program::artifact(
        None,
        vec![ActivatedAbility {
            id: ActivatedAbilityId(1),
            mana_cost: ManaCost::free(),
            source_cost: SourceCost::None,
            body: ExecutableBody::new(
                BodyTarget::SinglePermanent,
                vec![Instruction::CopyTargetToSource {
                    duration: ResolvedDuration::NoStatedDuration,
                    power_toughness: None,
                }],
            ),
        }],
    );
    let bounce = Program::instant(
        ManaCost::free(),
        ExecutableBody::new(
            BodyTarget::SinglePermanent,
            vec![Instruction::ReturnToOwnersHand],
        ),
    );
    let mut t = Trace::new(fixture(
        BodyTarget::None,
        sacrifice_body(2),
        vec![A, B],
        A,
        vec![
            bf(10, A, donor),
            bf(20, A, copier.clone()),
            bf(21, A, copier),
            seed(2, A, ObjectDestination::Hand, bounce),
        ],
    ));
    for receiver in [20, 21] {
        t.step(
            A,
            GameCommand::ActivateAbility {
                source: id(&t.state, receiver),
                ability: ActivatedAbilityId(1),
                target: TargetRef::Object(id(&t.state, 10)),
                payment: None,
            },
        );
        t.boundary();
    }
    t.cast(A, 2, Some(TargetRef::Object(id(&t.state, 10))));
    t.boundary();
    assert_eq!(zone(&t.state, 10), ObjectZone::Hand(A));
    t.cast(A, 1, None);
    t.boundary();
    let first = id(&t.state, 20);
    let second = id(&t.state, 21);
    t.reply(DecisionReply::SelectResolution(vec![first, second]));
    assert_eq!(work(&t.state).sources().len(), 2);
    choose(&mut t, first, 1, first);
    optional(&mut t, ZoneOptionalAnswer::Apply);
    choose(&mut t, second, 1, second);
    optional(&mut t, ZoneOptionalAnswer::Apply);
    assert_eq!(zone(&t.state, 20), ObjectZone::Exile);
    assert_eq!(zone(&t.state, 21), ObjectZone::Exile);
    t.replay();
}
#[test]
fn removed_replacement_abilities_do_not_execute_retained_printed_text() {
    for removal in [AbilityRemoval::All, AbilityRemoval::NonMana] {
        let p = source(vec![rule(
            1,
            Destination::Graveyard,
            Destination::Exile,
            false,
        )]);
        let body = vec![
            Instruction::CreateContinuousEffect {
                parts: ResolvedPartsSpec(vec![ResolvedPartSpec::Abilities(AbilityChange::remove(
                    removal,
                ))]),
                duration: ResolvedDuration::UntilEndOfTurn,
            },
            sacrifice(AffectedPlayers::One(ResolvingPlayer::Controller), 1.into()),
        ];
        let mut t = Trace::new(fixture(
            BodyTarget::SinglePermanent,
            body,
            vec![A, B],
            A,
            vec![bf(50, B, p), bf(10, A, creature())],
        ));
        t.cast(A, 1, Some(TargetRef::Object(id(&t.state, 50))));
        t.boundary();
        let current = t
            .state
            .execution()
            .current_characteristics(id(&t.state, 50))
            .unwrap()
            .unwrap();
        assert!(current.abilities.zone_replacements.is_empty());
        assert_eq!(current.printed_text.zone_replacements.len(), 1);
        let object = id(&t.state, 10);
        t.reply(DecisionReply::SelectResolution(vec![object]));
        assert_eq!(zone(&t.state, 10), ObjectZone::Graveyard(A));
        t.replay();
    }
}
#[test]
fn token_owned_source_and_token_filter_use_the_same_program_path() {
    let v = creature_types::selected_vocabulary().unwrap();
    let mut p = creature();
    let mut effect = rule(1, Destination::Graveyard, Destination::Exile, false);
    effect.objects = ZoneObjectKind::Token;
    p.zone_replacements = vec![effect];
    let definition = TokenDefinition::new(
        p,
        TokenName::Specified(EnglishCardName::new("Replacement Probe".into()).unwrap()),
        &v,
    )
    .unwrap();
    let prefix = Program::instant(
        ManaCost::free(),
        ExecutableBody::new(
            BodyTarget::None,
            vec![Instruction::CreateTokens {
                creations: vec![TokenCreation::one(definition)],
            }],
        ),
    );
    let mut body = sacrifice_body(2);
    body.push(Instruction::OptionalEffect {
        chooser: ResolvingPlayer::Controller,
    });
    body.push(gain(1));
    let mut t = Trace::new(fixture(
        BodyTarget::None,
        body,
        vec![A, B],
        A,
        vec![
            seed(2, A, ObjectDestination::Hand, prefix),
            bf(10, A, creature()),
        ],
    ));
    t.cast(A, 2, None);
    t.boundary();
    let token = t
        .state
        .execution()
        .window()
        .cards()
        .iter()
        .find(|o| o.is_token())
        .unwrap()
        .id();
    t.cast(A, 1, None);
    t.boundary();
    let object = id(&t.state, 10);
    let result = t.reply(DecisionReply::SelectResolution(vec![token, object]));
    let changes = selected_changes(&result);
    assert_eq!(changes.len(), 2);
    let departed = changes
        .iter()
        .find(|c| c.before.object.is_token())
        .unwrap()
        .after
        .object
        .id();
    assert_eq!(
        t.state
            .execution()
            .window()
            .cards()
            .get(departed)
            .unwrap()
            .zone(),
        ObjectZone::Exile
    );
    assert_eq!(zone(&t.state, 10), ObjectZone::Graveyard(A));
    let result = t.reply(DecisionReply::OptionalEffect(OptionalEffectChoice::Decline));
    assert!(t.state.execution().window().cards().get(departed).is_none());
    assert!(
        events(&result).iter().flat_map(|e| &e.occurrences).any(
            |r| matches!(&r.value,RulesOccurrence::TokenCeased{object} if object.id()==departed)
        )
    );
    t.replay();
}
#[test]
fn rotated_apnap_uses_stolen_objects_controller_then_final_physical_owner() {
    let mut t = Trace::new(fixture(
        BodyTarget::None,
        vec![sacrifice(AffectedPlayers::EachPlayer, 1.into())],
        vec![A, B, C],
        B,
        vec![
            bf(
                50,
                A,
                source(vec![rule(
                    1,
                    Destination::Graveyard,
                    Destination::Exile,
                    true,
                )]),
            ),
            seed(
                10,
                C,
                ObjectDestination::Battlefield { controller: A },
                creature(),
            ),
            seed(
                11,
                A,
                ObjectDestination::Battlefield { controller: B },
                creature(),
            ),
            seed(
                12,
                B,
                ObjectDestination::Battlefield { controller: C },
                creature(),
            ),
        ],
    ));
    t.cast(B, 1, None);
    t.boundary();
    for card in [11, 12, 10] {
        let object = id(&t.state, card);
        t.reply(DecisionReply::SelectResolution(vec![object]));
    }
    assert_eq!(work(&t.state).apnap(), &[B, C, A]);
    let projected = projection::resolution_selection_state(
        t.state.execution().resolution_selection_state().unwrap(),
    );
    assert_eq!(
        projected["stage"]["work"]["apnap"],
        serde_json::json!([B.0, C.0, A.0])
    );
    for actor in [B, C, A] {
        assert_eq!(replacement(&t.state).chooser(), actor);
        optional(&mut t, ZoneOptionalAnswer::Decline);
    }
    assert_eq!(physical_pile(&t.state, A), vec![CardId(11)]);
    assert_eq!(physical_pile(&t.state, B), vec![CardId(1), CardId(12)]);
    assert_eq!(physical_pile(&t.state, C), vec![CardId(10)]);
    t.replay();
}
#[test]
fn final_arrival_order_excludes_exile_and_rejections_and_exhaustion_are_atomic() {
    let mut provider = creature();
    let mut effect = rule(1, Destination::Graveyard, Destination::Exile, true);
    effect.subject = ZoneReplacementSubject::SourceSelf;
    provider.zone_replacements = vec![effect];
    let mut t = start(
        sacrifice_body(3),
        vec![
            bf(10, A, provider),
            bf(11, A, creature()),
            bf(12, A, creature()),
            seed(90, A, ObjectDestination::Graveyard, creature()),
        ],
    );
    let before = id(&t.state, 10);
    let other = id(&t.state, 11);
    let third = id(&t.state, 12);
    let old = id(&t.state, 90);
    t.reply(DecisionReply::SelectResolution(vec![before, other, third]));
    let snapshot = t.state.execution().clone();
    let frame = EffectDecision::ZoneReplacement(replacement(&t.state).clone());
    let reply =
        DecisionReply::ZoneReplacement(ZoneReplacementReply::Optional(ZoneOptionalAnswer::Apply));
    assert_eq!(
        snapshot
            .resume_rule(snapshot.revision(), &frame, B, reply.clone(), None)
            .unwrap_err(),
        EffectError::WrongChooser
    );
    assert!(
        snapshot
            .resume_rule(snapshot.revision() - 1, &frame, A, reply.clone(), None)
            .is_err()
    );
    assert!(
        snapshot
            .resume_rule(
                snapshot.revision(),
                &frame,
                A,
                DecisionReply::SelectResolution(vec![]),
                None
            )
            .is_err()
    );
    assert_eq!(&snapshot, t.state.execution());
    let bound = replacement(&t.state).clone();
    let wrong = EffectDecision::ZoneReplacement(bound.qualification_with_stage(bound.stage() + 1));
    assert!(
        snapshot
            .resume_rule(snapshot.revision(), &wrong, A, reply.clone(), None)
            .is_err()
    );
    assert_eq!(&snapshot, t.state.execution());
    let mut exhausted_stage = snapshot.clone();
    let Some(PendingWork::ResolutionSelection(selection)) = &mut exhausted_stage.pending_work
    else {
        panic!("selection")
    };
    let ResolutionSelectionStage::Replacing { batch, .. } = &mut selection.stage else {
        panic!("replacing")
    };
    batch.qualification_exhaust_stage();
    let current = exhausted_stage.selection_frame().unwrap();
    let saved = exhausted_stage.clone();
    assert_eq!(
        exhausted_stage
            .resume_rule(exhausted_stage.revision(), &current, A, reply.clone(), None)
            .unwrap_err(),
        EffectError::ZoneReplacement(ZoneReplacementError::StageExhausted)
    );
    assert_eq!(exhausted_stage, saved);
    optional(&mut t, ZoneOptionalAnswer::Apply);
    let prior = t.state.execution().clone();
    assert!(
        prior
            .resume_rule(prior.revision(), &frame, A, reply, None)
            .is_err()
    );
    assert_eq!(&prior, t.state.execution());
    assert_eq!(
        order_frame(&t.state)
            .domain()
            .iter()
            .copied()
            .collect::<BTreeSet<_>>(),
        BTreeSet::from([other, third])
    );
    assert_eq!(zone(&t.state, 10), ObjectZone::Battlefield);
    let mut exhausted = t.state.execution().clone();
    exhausted.window.set_test_next_object_identity(u64::MAX);
    let saved = exhausted.clone();
    let decision = EffectDecision::ResolutionGraveyardOrder(order_frame(&t.state).clone());
    assert!(
        exhausted
            .resume_rule(
                exhausted.revision(),
                &decision,
                A,
                DecisionReply::OrderResolutionGraveyard(vec![third, other]),
                None
            )
            .is_err()
    );
    assert_eq!(exhausted, saved);
    let result = t.reply(DecisionReply::OrderResolutionGraveyard(vec![third, other]));
    assert_eq!(selected_changes(&result).len(), 3);
    assert_eq!(events(&result).iter().filter(|e|e.occurrences.iter().any(|r|matches!(&r.value,RulesOccurrence::ZoneChange(c) if matches!(c.reason,ZoneReason::Sacrifice{..})))).count(),1);
    assert_eq!(zone(&t.state, 10), ObjectZone::Exile);
    assert_eq!(
        physical_pile(&t.state, A),
        vec![CardId(1), CardId(12), CardId(11), CardId(90)]
    );
    assert_eq!(id(&t.state, 90), old);
    t.replay();
}
#[test]
fn admission_codec_and_predecessor_visitor_are_strict_without_source_card_claims() {
    let v = creature_types::selected_vocabulary().unwrap();
    let base = source(vec![]);
    let old = codec::view(&base);
    assert!(old.get("zone_replacements").is_none());
    assert_eq!(codec::view(&codec::decode(old.clone(), &v).unwrap()), old);
    let p = source(vec![
        rule(1, Destination::Graveyard, Destination::Exile, true),
        rule(2, Destination::Exile, Destination::Graveyard, false),
    ]);
    let encoded = codec::view(&p);
    assert_eq!(codec::decode(encoded.clone(), &v).unwrap(), p);
    assert!(p.check_current10().is_err());
    for action in [ZoneActionKind::CleanupDiscard, ZoneActionKind::StateBased] {
        let mut admitted = p.clone();
        admitted.zone_replacements[0].actions =
            ZoneActionPredicate::OneOf(BTreeSet::from([action]));
        assert_eq!(admitted.validate(&v), Ok(()));
        assert_eq!(codec::decode(codec::view(&admitted), &v).unwrap(), admitted);
        assert!(admitted.check_current10().is_err());
    }
    let mut invalids = vec![];
    let mut any = p.clone();
    any.zone_replacements[0].actions = ZoneActionPredicate::Any;
    invalids.push((any, DefinitionError::UnsupportedZoneReplacementScope));
    for action in [
        ZoneActionKind::SacrificeCost,
        ZoneActionKind::Surveil,
        ZoneActionKind::Mill,
        ZoneActionKind::Draw,
        ZoneActionKind::Countered,
        ZoneActionKind::ResolutionExit,
        ZoneActionKind::ReturnToHand,
        ZoneActionKind::LandPlay,
        ZoneActionKind::CastAnnouncement,
    ] {
        let mut mixed = p.clone();
        let ZoneActionPredicate::OneOf(actions) = &mut mixed.zone_replacements[0].actions else {
            unreachable!()
        };
        actions.insert(action);
        invalids.push((mixed, DefinitionError::UnsupportedZoneReplacementScope));
    }
    let mut duplicate = p.clone();
    duplicate.zone_replacements[1].id = ZoneReplacementAbilityId(1);
    invalids.push((duplicate, DefinitionError::DuplicateAbility));
    let mut same = p.clone();
    same.zone_replacements[0].replace_with = Destination::Graveyard;
    invalids.push((same, DefinitionError::InvalidZoneReplacementDefinition));
    let mut empty = p.clone();
    empty.zone_replacements[0].actions = ZoneActionPredicate::OneOf(BTreeSet::new());
    invalids.push((empty, DefinitionError::InvalidZoneReplacementDefinition));
    let mut spell = Program::instant(
        ManaCost::free(),
        ExecutableBody::new(BodyTarget::None, vec![gain(1)]),
    );
    spell.zone_replacements = p.zone_replacements.clone();
    invalids.push((spell, DefinitionError::UnsupportedZoneReplacementScope));
    for (invalid, expected) in invalids {
        assert_eq!(invalid.validate(&v), Err(expected));
        let raw = codec::view(&invalid);
        assert_eq!(codec::decode_for_owner(raw, &v).unwrap(), invalid);
        let names = Arc::new(spellwake_contracts::names::NameRules::literal_names());
        assert_eq!(
            EffectWindow::new(GameSeed::with_uniform_life(
                vec![A, B],
                A,
                vec![bf(50, A, invalid.clone())],
                vec![CardId(50)],
                vec![],
                20.into(),
                CombatRules::UnrestrictedFreeForAll,
                v.clone(),
                names
            ))
            .unwrap_err(),
            EffectError::InvalidSetup
        );
        assert!(
            TokenDefinition::new(
                invalid,
                TokenName::Specified(EnglishCardName::new("Invalid Probe".into()).unwrap()),
                &v
            )
            .is_err()
        );
    }
    for mutate in [0, 1, 2, 3] {
        let mut raw = encoded.clone();
        match mutate {
            0 => raw["zone_replacements"][0]["id"] = serde_json::json!(-1),
            1 => raw["zone_replacements"][0]["unexpected"] = serde_json::json!(true),
            2 => {
                raw["zone_replacements"][0]["actions"]["actions"] =
                    serde_json::json!(["EffectDiscard", "EffectDiscard"])
            }
            _ => raw["zone_replacements"][0]["replace_with"] = serde_json::json!("LibraryTop"),
        };
        assert!(codec::decode_for_owner(raw, &v).is_err());
    }
}

#[test]
fn predecessor_refusal_isolates_retained_text_roster_copy_work_frames_and_replies() {
    use crate::characteristics::{ResolvedPart, ResolvedParts};
    let mut t = start(
        sacrifice_body(1),
        vec![
            bf(
                50,
                B,
                source(vec![rule(
                    1,
                    Destination::Graveyard,
                    Destination::Exile,
                    true,
                )]),
            ),
            bf(10, A, creature()),
        ],
    );
    let object = id(&t.state, 10);
    let source_id = id(&t.state, 50);
    t.reply(DecisionReply::SelectResolution(vec![object]));
    let current = t
        .state
        .execution()
        .current_characteristics(source_id)
        .unwrap()
        .unwrap();
    assert!(current.printed_text.check_current10().is_err());
    let snapshot = t.state.execution().rules_snapshot(None).unwrap();
    let mut appearance = snapshot.view().appearance(source_id).unwrap();
    appearance.printed_text.zone_replacements.clear();
    assert!(appearance.check_current10().is_err());
    appearance.abilities.zone_replacements.clear();
    assert!(appearance.check_current10().is_ok());
    let mut execution = t.state.execution().clone();
    let copied = execution
        .characteristics
        .add_effect(
            instruction_context(replacement(&t.state)).0,
            object,
            ResolvedParts(vec![ResolvedPart::Copy(Box::new(source(vec![rule(
                1,
                Destination::Graveyard,
                Destination::Exile,
                true,
            )])))]),
            ResolvedDuration::NoStatedDuration,
        )
        .unwrap();
    assert!(copied.check_current10().is_err());
    let frame = replacement(&t.state).clone();
    assert!(
        RuleFrame::ZoneReplacement(frame.clone())
            .check_current10()
            .is_err()
    );
    assert!(
        EffectDecision::ZoneReplacement(frame)
            .check_current10()
            .is_err()
    );
    assert!(
        DecisionReply::ZoneReplacement(ZoneReplacementReply::Optional(ZoneOptionalAnswer::Apply))
            .check_current10()
            .is_err()
    );
    // Strip easier authored/captured/copy paths: the pending selected owner alone refuses.
    execution.characteristics = CharacteristicState::default();
    for program in Arc::make_mut(&mut execution.programs).values_mut() {
        *program = creature();
    }
    for payload in execution.payloads.values_mut() {
        if let StackPayload::Instructions(payload) = payload {
            payload.body = ExecutableBody::new(BodyTarget::None, vec![gain(1)]);
            payload.copiable_traits = None;
        }
    }
    assert!(matches!(
        execution.resolution_selection_state().unwrap().stage,
        ResolutionSelectionStage::Replacing { .. }
    ));
    assert!(execution.check_current10().is_err());
    t.replay();
}

#[test]
fn captured_activated_body_replaces_sacrifice_after_its_real_source_cost() {
    let activated = Program::artifact(
        None,
        vec![ActivatedAbility {
            id: ActivatedAbilityId(1),
            mana_cost: ManaCost::free(),
            source_cost: SourceCost::Sacrifice,
            body: ExecutableBody::new(
                BodyTarget::SingleCreature,
                vec![
                    Instruction::PutPowerToughnessCounters {
                        counters: crate::characteristics::PowerToughnessCounters {
                            plus_one: 1.into(),
                            minus_one: 0.into(),
                        },
                    },
                    sacrifice(AffectedPlayers::One(ResolvingPlayer::Controller), 1.into()),
                    gain(4),
                ],
            ),
        }],
    );
    let mut t = Trace::new(fixture(
        BodyTarget::None,
        vec![gain(0)],
        vec![A, B],
        A,
        vec![
            bf(
                50,
                B,
                source(vec![
                    rule(1, Destination::Graveyard, Destination::Exile, false),
                    rule(2, Destination::Graveyard, Destination::Exile, false),
                ]),
            ),
            bf(60, A, activated),
            bf(10, A, creature()),
        ],
    ));
    let source_before = id(&t.state, 60);
    let selected = id(&t.state, 10);
    let replacement_source = id(&t.state, 50);
    let announced = t.step(
        A,
        GameCommand::ActivateAbility {
            source: source_before,
            ability: ActivatedAbilityId(1),
            target: TargetRef::Object(selected),
            payment: None,
        },
    );
    let entry = announced.ability_activation().unwrap().entry;
    assert_eq!(zone(&t.state, 60), ObjectZone::Graveyard(A));
    assert!(
        t.state
            .execution()
            .window()
            .cards()
            .get(source_before)
            .is_none()
    );
    t.boundary();
    assert_eq!(select_frame(&t.state).entry(), entry);
    assert_eq!(select_frame(&t.state).instruction(), 1);
    t.reply(DecisionReply::SelectResolution(vec![selected]));
    let candidate = work(&t.state).candidates().values().next().unwrap();
    assert_eq!(
        t.state
            .execution()
            .window()
            .resolving()
            .unwrap()
            .controller(),
        A
    );
    assert_eq!(
        work(&t.state).cause(),
        &crate::occurrence::EventCause::Instruction {
            entry,
            instruction: 1
        }
    );
    assert!(matches!(candidate.key,ZoneEventKey::Instruction {entry:actual,..} if actual==entry));
    assert_eq!(candidate.action, ZoneActionKind::EffectSacrifice);
    choose(&mut t, selected, 1, replacement_source);
    let result = &t.steps.last().unwrap().1;
    let changed = selected_changes(result);
    assert_eq!(changed.len(), 1);
    assert_eq!(changed[0].after.object.zone(), ObjectZone::Exile);
    assert!(events(result).iter().any(|event| event.cause
        == EventCause::Instruction {
            entry,
            instruction: 1
        }
        && event.occurrences.iter().any(
            |r| matches!(&r.value,RulesOccurrence::ZoneChange(c) if c.before.object.id()==selected)
        )));
    assert_eq!(t.state.execution().life_total(A), Some(&24.into()));
    assert!(t.state.pending_decision().is_none());
    t.replay();
}

fn instruction_context(frame: &ZoneReplacementFrame) -> (StackEntryId, usize) {
    let crate::occurrence::EventCause::Instruction { entry, instruction } = frame.cause() else {
        panic!("real resolving instruction")
    };
    (*entry, *instruction)
}
