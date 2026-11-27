//! DP02 synthetic mechanisms through the maintained native root and executor.
//! These tests provide no Oracle, ordinary-profile or full-game witness credit.
use super::*;
use crate::{
    activated::{ActivatedAbility, ActivatedAbilityId, SourceCost},
    characteristics::{
        ControlRecipient, PowerToughness, PowerToughnessCounters, ResolvedDuration,
        ResolvedPartSpec, ResolvedPartsSpec,
    },
    mana::ManaPaymentError,
    occurrence::EventCause,
    priority::PriorityPhase,
    program::{PlayerRelation, TriggerGrouping, TriggerSpec, TriggeredAbility, TriggeredAbilityId},
    stack_window::{StackKind, StackWindowError, WindowPhase},
    token::{TokenCreation, TokenDefinition, TokenName},
};

const C: SeatId = SeatId(2);

#[path = "resolving_inspection.rs"]
mod resolving_inspection;

#[path = "depth_completion.rs"]
mod depth_completion;

#[path = "depth_completion_v2.rs"]
mod depth_completion_v2;

#[path = "native_packets.rs"]
pub(super) mod native_packets;

fn seed(id: u64, owner: SeatId, destination: ObjectDestination, program: Program) -> SeedCard {
    SeedCard {
        card: CardId(id),
        owner,
        destination,
        program,
    }
}
fn creature() -> Program {
    Program::creature(None, 2.into(), 8.into(), BTreeSet::new())
}
fn hand(id: u64, owner: SeatId, target: BodyTarget, instructions: Vec<Instruction>) -> SeedCard {
    seed(
        id,
        owner,
        ObjectDestination::Hand,
        Program::instant(ManaCost::free(), ExecutableBody::new(target, instructions)),
    )
}
fn world(seats: Vec<SeatId>, active: SeatId, cards: Vec<SeedCard>) -> GameState {
    let vocabulary = creature_types::selected_vocabulary().unwrap();
    let names = Arc::new(spellwake_contracts::names::NameRules::literal_names());
    let context = GameContext::new(
        "optional-native-mechanisms".into(),
        vocabulary.rules_revision().into(),
        "synthetic-native-only".into(),
        "DP02".into(),
        "unit".into(),
        vocabulary.id(),
        names.id(),
    )
    .unwrap();
    let order = cards
        .iter()
        .filter(|card| matches!(card.destination, ObjectDestination::Battlefield { .. }))
        .map(|card| card.card)
        .collect();
    let seed = GameSeed::with_uniform_life(
        seats,
        active,
        cards,
        order,
        vec![],
        20.into(),
        CombatRules::UnrestrictedFreeForAll,
        vocabulary,
        names,
    );
    let initialized =
        crate::test_support::current_capture::initialize(GameId(7711), context, seed).unwrap();
    native_packets::retain_root(&initialized, "native_mechanisms.world");
    initialized.into_state()
}
fn resolve_top(state: &GameState) -> NativeCandidate {
    let mut state = state.clone();
    let count = state.execution().window().participants().len();
    for index in 0..count {
        let WindowPhase::Priority(PriorityPhase::Ready { holder }) =
            state.execution().window().phase()
        else {
            panic!("actual priority required")
        };
        let result = candidate(&state, holder, GameCommand::Pass);
        if index + 1 == count {
            return result;
        }
        state = result.into_state();
    }
    unreachable!()
}
fn cast(state: &GameState, actor: SeatId, card: u64, target: Option<TargetRef>) -> NativeCandidate {
    let spell = object(state, card);
    candidate(
        state,
        actor,
        match target {
            Some(target) => GameCommand::Cast { spell, target },
            None => GameCommand::CastUntargeted {
                spell,
                payment: None,
            },
        },
    )
}
fn command_reply(state: &GameState, choice: DecisionReply) -> GameCommand {
    GameCommand::Reply {
        decision: state.pending_decision().unwrap().id(),
        choice,
    }
}
fn reject(state: &GameState, actor: SeatId, command: GameCommand, error: GameError) {
    let saved = state.clone();
    let input = GameInput {
        basis: state.basis().clone(),
        actor,
        command,
    };
    assert_eq!(
        crate::test_support::current_capture::advance(state, input),
        Err(error)
    );
    assert_eq!(
        state, &saved,
        "rejection must retain the complete latest state"
    );
}
fn optional_frame(state: &GameState) -> OptionalEffectFrame {
    let RuleFrame::OptionalEffect(frame) = state.pending_decision().unwrap().frame() else {
        panic!("optional expected")
    };
    frame.clone()
}
fn outcomes(result: &NativeCandidate) -> usize {
    result
        .rules_journal()
        .records
        .iter()
        .filter(|row| matches!(row, RulesRecord::OptionalEffectOutcome { .. }))
        .count()
}
fn occurrences(result: &NativeCandidate) -> Vec<&RulesOccurrence> {
    result
        .rules_journal()
        .records
        .iter()
        .filter_map(|row| match row {
            RulesRecord::Event(event) => Some(event.occurrences.iter().map(|row| &row.value)),
            _ => None,
        })
        .flatten()
        .collect()
}

#[test]
fn optional_rejects_wrong_actor_kind_basis_frame_and_repeated_reply_atomically() {
    let state = pending(vec![
        Instruction::OptionalSequence {
            chooser: ResolvingPlayer::Controller,
            instructions: 1,
        },
        gain(1),
        gain(2),
    ]);
    let choice = DecisionReply::OptionalEffect(OptionalEffectChoice::Take);
    reject(
        &state,
        B,
        command_reply(&state, choice.clone()),
        GameError::WrongChooser,
    );
    reject(
        &state,
        A,
        command_reply(
            &state,
            DecisionReply::ResolutionPayment(ResolutionPaymentReply::Decline),
        ),
        GameError::Effect(EffectError::WrongDecisionKind),
    );
    let mut input = GameInput {
        basis: state.basis().clone(),
        actor: A,
        command: command_reply(&state, choice.clone()),
    };
    input.basis.revision -= 1;
    let saved = state.clone();
    assert_eq!(state.advance(input), Err(GameError::StaleRevision));
    assert_eq!(state, saved);
    let frame = optional_frame(&state);
    let mut wrong = frame.clone();
    wrong.instruction += 1;
    assert_eq!(
        state.execution().resume_rule(
            state.execution().revision(),
            &EffectDecision::OptionalEffect(wrong),
            A,
            choice.clone(),
            None
        ),
        Err(EffectError::WrongOptionalEffectFrame)
    );
    assert_eq!(state, saved);
    let command = command_reply(&state, choice.clone());
    let result = reply(&state, choice);
    assert_eq!(outcomes(&result), 1);
    assert_eq!(result.state().execution().life_total(A), Some(&23.into()));
    reject(result.state(), A, command, GameError::NoDecision);
    assert_eq!(
        result.state().execution().resume_rule(
            result.state().execution().revision(),
            &EffectDecision::OptionalEffect(frame),
            A,
            DecisionReply::OptionalEffect(OptionalEffectChoice::Take),
            None
        ),
        Err(EffectError::WrongOptionalEffectFrame)
    );
}

#[test]
fn payment_rejects_wrong_actor_kind_frame_and_repeated_reply_after_accepted_prefix() {
    let state = tap(&pending(offer(3)), 10).into_state();
    let choice = DecisionReply::ResolutionPayment(ResolutionPaymentReply::Decline);
    reject(
        &state,
        B,
        command_reply(&state, choice.clone()),
        GameError::WrongChooser,
    );
    reject(
        &state,
        B,
        GameCommand::ActivateMana {
            source: object(&state, 11),
            ability: ABILITY,
        },
        GameError::WrongChooser,
    );
    reject(
        &state,
        A,
        command_reply(
            &state,
            DecisionReply::OptionalEffect(OptionalEffectChoice::Take),
        ),
        GameError::Effect(EffectError::WrongDecisionKind),
    );
    let mut wrong = payment_frame(&state);
    wrong.cost = ManaCost::free();
    let saved = state.clone();
    assert_eq!(
        state.execution().resume_rule(
            state.execution().revision(),
            &EffectDecision::ResolutionPayment(wrong),
            A,
            choice.clone(),
            None
        ),
        Err(EffectError::WrongResolutionPaymentFrame)
    );
    assert_eq!(state, saved);
    let command = command_reply(&state, choice.clone());
    let result = reply(&state, choice);
    reject(result.state(), A, command, GameError::NoDecision);
    assert_eq!(result.state().execution().mana(A, ManaType::Blue), 1.into());
    assert_eq!(
        result
            .state()
            .execution()
            .window()
            .cards()
            .get(object(&state, 10))
            .unwrap()
            .tapped(),
        Some(true)
    );
}

#[test]
fn payment_and_color_child_deny_every_other_root_action_class() {
    let payment = tap(&pending(offer(3)), 10).into_state();
    let color = tap(&payment, 12).into_state();
    for state in [&payment, &color] {
        let source = object(state, 11);
        for command in [
            GameCommand::Pass,
            GameCommand::CastUntargeted {
                spell: source,
                payment: None,
            },
            GameCommand::Cast {
                spell: source,
                target: TargetRef::Player(B),
            },
            GameCommand::CastPaid {
                spell: source,
                target: TargetRef::Player(B),
                payment: ManaPayment::default(),
            },
            GameCommand::PlayLand { land: source },
            GameCommand::ActivateAbility {
                source,
                ability: ActivatedAbilityId(1),
                target: TargetRef::Player(B),
                payment: None,
            },
        ] {
            reject(state, A, command, GameError::DecisionRequired);
        }
    }
    reject(
        &color,
        A,
        GameCommand::ActivateMana {
            source: object(&color, 11),
            ability: ABILITY,
        },
        GameError::DecisionRequired,
    );
    reject(
        &color,
        B,
        command_reply(&color, DecisionReply::ManaColor(ManaType::Blue)),
        GameError::WrongChooser,
    );
    reject(
        &color,
        A,
        command_reply(
            &color,
            DecisionReply::ResolutionPayment(ResolutionPaymentReply::Decline),
        ),
        GameError::Effect(EffectError::WrongDecisionKind),
    );
    reject(
        &color,
        A,
        command_reply(
            &color,
            DecisionReply::OptionalEffect(OptionalEffectChoice::Take),
        ),
        GameError::Effect(EffectError::WrongDecisionKind),
    );
    for mana_type in [ManaType::Green, ManaType::Colourless] {
        reject(
            &color,
            A,
            command_reply(&color, DecisionReply::ManaColor(mana_type)),
            GameError::Effect(EffectError::IllegalManaColor),
        );
    }
    let old_color = command_reply(&color, DecisionReply::ManaColor(ManaType::Blue));
    let restored = reply(&color, DecisionReply::ManaColor(ManaType::Blue)).into_state();
    reject(&restored, A, old_color, GameError::WrongDecision);
    let result = reply(
        &restored,
        DecisionReply::ResolutionPayment(ResolutionPaymentReply::Decline),
    );
    assert_eq!(result.state().execution().mana(A, ManaType::Blue), 2.into());
    assert_eq!(result.state().execution().life_total(A), Some(&22.into()));
}

#[test]
fn exact_spend_denials_preserve_the_entire_funded_payment_and_valid_continuation() {
    let state = tap(&tap(&pending(offer(3)), 10).into_state(), 11).into_state();
    let color = tap(&state, 12).into_state();
    let funded = reply(&color, DecisionReply::ManaColor(ManaType::Blue)).into_state();
    let spend = |rows: &[(ManaType, i64)]| ManaPayment {
        from_lots: Default::default(),
        spend: rows
            .iter()
            .map(|&(mana_type, amount)| ManaSpend {
                mana_type,
                amount: amount.into(),
            })
            .collect(),
    };
    for (allocation, error) in [
        (
            spend(&[(ManaType::Blue, 1), (ManaType::Blue, 2)]),
            ManaPaymentError::DuplicateType,
        ),
        (
            spend(&[(ManaType::Blue, -3)]),
            ManaPaymentError::NonPositiveSpend,
        ),
        (
            spend(&[(ManaType::Blue, 0)]),
            ManaPaymentError::NonPositiveSpend,
        ),
        (spend(&[(ManaType::Blue, 2)]), ManaPaymentError::WrongTotal),
        (spend(&[(ManaType::Blue, 4)]), ManaPaymentError::WrongTotal),
        (
            spend(&[(ManaType::Red, 3)]),
            ManaPaymentError::InsufficientMana,
        ),
    ] {
        reject(
            &funded,
            A,
            command_reply(
                &funded,
                DecisionReply::ResolutionPayment(ResolutionPaymentReply::Pay(allocation)),
            ),
            GameError::Effect(EffectError::InvalidManaPayment(error)),
        );
        let result = reply(
            &funded,
            DecisionReply::ResolutionPayment(ResolutionPaymentReply::Pay(spend(&[(
                ManaType::Blue,
                3,
            )]))),
        );
        assert_eq!(result.state().execution().mana(A, ManaType::Blue), 0.into());
        assert_eq!(
            result
                .rules_journal()
                .records
                .iter()
                .filter(|row| matches!(
                    row,
                    RulesRecord::ResolutionPaymentOutcome {
                        outcome: ResolutionPaymentOutcome::Paid(_),
                        ..
                    }
                ))
                .count(),
            1
        );
        let declined = reply(
            &funded,
            DecisionReply::ResolutionPayment(ResolutionPaymentReply::Decline),
        );
        assert_eq!(
            declined.state().execution().mana(A, ManaType::Blue),
            3.into()
        );
        assert_eq!(declined.state().execution().life_total(A), Some(&22.into()));
    }
    let zero = pending(offer(0));
    reject(
        &zero,
        A,
        command_reply(
            &zero,
            DecisionReply::ResolutionPayment(ResolutionPaymentReply::Pay(spend(&[(
                ManaType::Blue,
                0,
            )]))),
        ),
        GameError::Effect(EffectError::InvalidManaPayment(
            ManaPaymentError::NonPositiveSpend,
        )),
    );
    assert!(
        reply(
            &zero,
            DecisionReply::ResolutionPayment(ResolutionPaymentReply::Pay(ManaPayment::default()))
        )
        .state()
        .pending_decision()
        .is_none()
    );
}

#[test]
fn positive_counted_production_restores_quote_then_fixed_production_pays_exactly_three() {
    let mut source = Program::artifact(None, vec![]);
    source.mana.push(ManaAbility {
        id: ManaAbilityId(1),
        output: ManaOutput::Counted {
            mana_type: Some(ManaType::Blue),
            permanents: PermanentSelector {
                required_types: BTreeSet::from([CardType::Creature]),
                ..PermanentSelector::default()
            },
        },
    });
    let mut fixed = Program::artifact(None, vec![]);
    fixed.mana.push(ManaAbility {
        id: ManaAbilityId(1),
        output: ManaOutput::Fixed(BTreeMap::from([(ManaType::Blue, 1.into())])),
    });
    let state = world(
        vec![A, B],
        A,
        vec![
            hand(1, A, BodyTarget::None, offer(3)),
            seed(
                10,
                A,
                ObjectDestination::Battlefield { controller: A },
                source,
            ),
            seed(
                11,
                A,
                ObjectDestination::Battlefield { controller: A },
                fixed,
            ),
            seed(
                20,
                A,
                ObjectDestination::Battlefield { controller: A },
                creature(),
            ),
            seed(
                21,
                B,
                ObjectDestination::Battlefield { controller: B },
                creature(),
            ),
        ],
    );
    let state = resolve_top(cast(&state, A, 1, None).state()).into_state();
    let old = payment_frame(&state);
    let old_id = state.pending_decision().unwrap().id();
    let counted = tap(&state, 10);
    assert_eq!(
        counted.mana_production().unwrap().produced,
        BTreeMap::from([(ManaType::Blue, 2.into())])
    );
    assert_eq!(
        counted.state().execution().mana(A, ManaType::Blue),
        2.into()
    );
    assert_old_payment_rejected(counted.state(), &old, old_id);
    let fixed = tap(counted.state(), 11);
    assert_eq!(fixed.state().execution().mana(A, ManaType::Blue), 3.into());
    let paid = reply(
        fixed.state(),
        DecisionReply::ResolutionPayment(ResolutionPaymentReply::Pay(ManaPayment {
            from_lots: Default::default(),
            spend: vec![ManaSpend {
                mana_type: ManaType::Blue,
                amount: 3.into(),
            }],
        })),
    );
    assert_eq!(paid.state().execution().mana(A, ManaType::Blue), 0.into());
    assert_eq!(paid.state().execution().life_total(A), Some(&20.into()));
}

fn wrapped(operation: Instruction) -> Vec<Instruction> {
    vec![
        Instruction::OptionalSequence {
            chooser: ResolvingPlayer::Controller,
            instructions: 1,
        },
        operation,
        gain(2),
    ]
}
fn targeted(operation: Instruction, target: BodyTarget, target_ref: TargetRef) -> GameState {
    let state = world(
        vec![A, B],
        A,
        vec![
            hand(1, A, target, wrapped(operation)),
            seed(
                2,
                B,
                ObjectDestination::Battlefield { controller: B },
                creature(),
            ),
        ],
    );
    let target_ref = match target_ref {
        TargetRef::Object(_) => TargetRef::Object(object(&state, 2)),
        other => other,
    };
    resolve_top(cast(&state, A, 1, Some(target_ref)).state()).into_state()
}
fn creature_optional(operation: Instruction) -> GameState {
    let state = world(
        vec![A, B],
        A,
        vec![
            hand(1, A, BodyTarget::SINGLE_CREATURE, wrapped(operation)),
            seed(
                2,
                B,
                ObjectDestination::Battlefield { controller: B },
                creature(),
            ),
        ],
    );
    resolve_top(cast(&state, A, 1, Some(TargetRef::Object(object(&state, 2)))).state()).into_state()
}
fn take_or_decline(state: &GameState, choice: OptionalEffectChoice) -> NativeCandidate {
    assert_eq!(
        optional_frame(state).options(),
        &BTreeSet::from([OptionalEffectChoice::Take, OptionalEffectChoice::Decline])
    );
    let result = reply(state, DecisionReply::OptionalEffect(choice));
    assert_eq!(outcomes(&result), 1);
    result
}
fn assert_tail(result: &NativeCandidate) {
    assert!(result.state().pending_decision().is_none());
    assert_eq!(result.state().execution().life_total(A), Some(&22.into()));
}

#[test]
fn optional_target_life_runs_once_or_skips_before_the_shared_tail() {
    for choice in [OptionalEffectChoice::Take, OptionalEffectChoice::Decline] {
        let state = targeted(
            Instruction::AdjustLife(Box::new(crate::toolbox::AdjustLifeSpec::lose(
                crate::toolbox::PlayerSelector::Resolving(ResolvingPlayer::TargetPlayer),
                crate::program::ValueExpr::Literal(3.into()),
            ))),
            BodyTarget::SINGLE_PLAYER,
            TargetRef::Player(B),
        );
        let result = take_or_decline(&state, choice);
        assert_tail(&result);
        assert_eq!(
            result.state().execution().life_total(B),
            Some(
                &if choice == OptionalEffectChoice::Take {
                    17
                } else {
                    20
                }
                .into()
            )
        );
    }
}

#[test]
fn optional_damage_uses_real_damage_owner_and_decline_leaves_no_damage() {
    for choice in [OptionalEffectChoice::Take, OptionalEffectChoice::Decline] {
        let state = creature_optional(Instruction::DealDamage(Box::new(
            crate::toolbox::DealDamageSpec {
                source: crate::toolbox::DamageSourceQuery::EntrySource,
                recipients: crate::toolbox::DamageTargetQuery::Targets,
                amount: crate::program::DamageAmountSpec::Expr(crate::program::ValueExpr::Literal(
                    3.into(),
                )),
                exile_if_dies: false,
            },
        )));
        let target = object(&state, 2);
        let result = take_or_decline(&state, choice);
        assert_tail(&result);
        assert_eq!(
            result.state().execution().battlefield_marked_damage(target),
            Some(
                if choice == OptionalEffectChoice::Take {
                    3
                } else {
                    0
                }
                .into()
            )
        );
        assert_eq!(
            result.damage_journal().applied.len(),
            usize::from(choice == OptionalEffectChoice::Take)
        );
    }
}

#[test]
fn optional_prevention_shield_creates_one_owned_effect_or_skips() {
    for choice in [OptionalEffectChoice::Take, OptionalEffectChoice::Decline] {
        let state = creature_optional(Instruction::ApplyContinuous(Box::new(
            crate::toolbox::ApplyContinuousSpec {
                selector: None,
                payload: crate::toolbox::ContinuousPayload::TargetPreventionShield {
                    amount: 3.into(),
                },
                duration: crate::characteristics::ResolvedDuration::UntilEndOfTurn,
                condition: None,
                one_shot: true,
                as_emblem: None,
                emblem_label: None,
            },
        )));
        let result = take_or_decline(&state, choice);
        assert_tail(&result);
        assert_eq!(
            result.state().execution().prevention_shields().count(),
            usize::from(choice == OptionalEffectChoice::Take)
        );
        assert_eq!(
            occurrences(&result)
                .iter()
                .filter(|row| matches!(row, RulesOccurrence::DamageEffectCreated(_)))
                .count(),
            usize::from(choice == OptionalEffectChoice::Take)
        );
    }
}

#[test]
fn optional_multiplier_creates_one_owned_effect_or_skips() {
    for choice in [OptionalEffectChoice::Take, OptionalEffectChoice::Decline] {
        let state = creature_optional(Instruction::ApplyContinuous(Box::new(
            crate::toolbox::ApplyContinuousSpec::damage_multiplier(2.into()),
        )));
        let result = take_or_decline(&state, choice);
        assert_tail(&result);
        assert_eq!(
            result.state().execution().damage_multipliers().count(),
            usize::from(choice == OptionalEffectChoice::Take)
        );
        assert_eq!(
            occurrences(&result)
                .iter()
                .filter(|row| matches!(row, RulesOccurrence::DamageEffectCreated(_)))
                .count(),
            usize::from(choice == OptionalEffectChoice::Take)
        );
    }
}

#[test]
fn optional_bounce_moves_exact_target_once_or_retains_its_incarnation() {
    for choice in [OptionalEffectChoice::Take, OptionalEffectChoice::Decline] {
        let state = creature_optional(crate::program::Instruction::ZoneOp(Box::new(
            crate::toolbox::ZoneOpSpec::move_all(
                crate::toolbox::ObjectQuery::Targets,
                crate::toolbox::ZoneDest::Hand,
            ),
        )));
        let original = object(&state, 2);
        let result = take_or_decline(&state, choice);
        assert_tail(&result);
        let current = object(result.state(), 2);
        assert_eq!(current == original, choice == OptionalEffectChoice::Decline);
        assert_eq!(
            result
                .state()
                .execution()
                .window()
                .cards()
                .get(current)
                .unwrap()
                .zone(),
            if choice == OptionalEffectChoice::Take {
                ObjectZone::Hand(B)
            } else {
                ObjectZone::Battlefield
            }
        );
    }
}

#[test]
fn optional_continuous_effect_uses_real_characteristic_owner_or_skips() {
    for choice in [OptionalEffectChoice::Take, OptionalEffectChoice::Decline] {
        let state = creature_optional({
            struct _Tmp {
                parts: crate::characteristics::ResolvedPartsSpec,
                duration: crate::characteristics::ResolvedDuration,
            }
            let _t = _Tmp {
                parts: ResolvedPartsSpec(vec![ResolvedPartSpec::ModifyPT(PowerToughness {
                    power: 1.into(),
                    toughness: 2.into(),
                })]),
                duration: ResolvedDuration::NoStatedDuration,
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
        let result = take_or_decline(&state, choice);
        assert_tail(&result);
        assert_eq!(
            result.state().execution().continuous_effects().count(),
            usize::from(choice == OptionalEffectChoice::Take)
        );
        let pt = result
            .state()
            .execution()
            .power_toughness(object(&state, 2))
            .unwrap()
            .unwrap()
            .derived;
        assert_eq!(
            pt,
            if choice == OptionalEffectChoice::Take {
                PowerToughness {
                    power: 3.into(),
                    toughness: 10.into(),
                }
            } else {
                PowerToughness {
                    power: 2.into(),
                    toughness: 8.into(),
                }
            }
        );
    }
}

#[test]
fn optional_counters_place_once_or_leave_physical_counters_unchanged() {
    for choice in [OptionalEffectChoice::Take, OptionalEffectChoice::Decline] {
        let state = creature_optional({
            struct _Tmp {
                counters: crate::characteristics::PowerToughnessCounters,
            }
            let _t = _Tmp {
                counters: PowerToughnessCounters {
                    plus_one: 2.into(),
                    minus_one: 0.into(),
                },
            };
            let (_k, _a) = if _t.counters.minus_one.is_positive() {
                (crate::effects::CounterKind::MinusOne, _t.counters.minus_one)
            } else {
                (crate::effects::CounterKind::PlusOne, _t.counters.plus_one)
            };
            crate::program::Instruction::MutateObject(Box::new(
                crate::toolbox::MutateObjectSpec::all(
                    crate::toolbox::ObjectQuery::Targets,
                    crate::toolbox::ObjectMutation::Counters(crate::toolbox::CounterOp::Add {
                        kind: _k,
                        amount: crate::program::ValueExpr::Literal(_a),
                    }),
                ),
            ))
        });
        let result = take_or_decline(&state, choice);
        assert_tail(&result);
        assert_eq!(
            occurrences(&result)
                .iter()
                .filter(|row| matches!(row, RulesOccurrence::CountersPlaced { .. }))
                .count(),
            usize::from(choice == OptionalEffectChoice::Take)
        );
        assert_eq!(
            result
                .state()
                .execution()
                .power_toughness(object(&state, 2))
                .unwrap()
                .unwrap()
                .derived
                .power,
            if choice == OptionalEffectChoice::Take {
                4
            } else {
                2
            }
            .into()
        );
    }
}

#[test]
fn earlier_bounce_removes_take_from_the_real_optional_counter_domain() {
    let state = world(
        vec![A, B],
        A,
        vec![
            hand(
                1,
                A,
                BodyTarget::SINGLE_CREATURE,
                vec![
                    crate::program::Instruction::ZoneOp(Box::new(
                        crate::toolbox::ZoneOpSpec::move_all(
                            crate::toolbox::ObjectQuery::Targets,
                            crate::toolbox::ZoneDest::Hand,
                        ),
                    )),
                    Instruction::OptionalSequence {
                        chooser: ResolvingPlayer::Controller,
                        instructions: 1,
                    },
                    {
                        struct _Tmp {
                            counters: crate::characteristics::PowerToughnessCounters,
                        }
                        let _t = _Tmp {
                            counters: PowerToughnessCounters {
                                plus_one: 1.into(),
                                minus_one: 0.into(),
                            },
                        };
                        let (_k, _a) = if _t.counters.minus_one.is_positive() {
                            (crate::effects::CounterKind::MinusOne, _t.counters.minus_one)
                        } else {
                            (crate::effects::CounterKind::PlusOne, _t.counters.plus_one)
                        };
                        crate::program::Instruction::MutateObject(Box::new(
                            crate::toolbox::MutateObjectSpec::all(
                                crate::toolbox::ObjectQuery::Targets,
                                crate::toolbox::ObjectMutation::Counters(
                                    crate::toolbox::CounterOp::Add {
                                        kind: _k,
                                        amount: crate::program::ValueExpr::Literal(_a),
                                    },
                                ),
                            ),
                        ))
                    },
                    gain(2),
                ],
            ),
            seed(
                2,
                B,
                ObjectDestination::Battlefield { controller: B },
                creature(),
            ),
        ],
    );
    let old = object(&state, 2);
    let state = resolve_top(cast(&state, A, 1, Some(TargetRef::Object(old))).state()).into_state();
    assert_ne!(object(&state, 2), old);
    assert_eq!(optional_frame(&state).instruction(), 1);
    assert_eq!(
        optional_frame(&state).options(),
        &BTreeSet::from([OptionalEffectChoice::Decline])
    );
    reject(
        &state,
        A,
        command_reply(
            &state,
            DecisionReply::OptionalEffect(OptionalEffectChoice::Take),
        ),
        GameError::Effect(EffectError::IllegalOptionalEffect),
    );
    let result = reply(
        &state,
        DecisionReply::OptionalEffect(OptionalEffectChoice::Decline),
    );
    assert_tail(&result);
    assert_eq!(outcomes(&result), 1);
    assert!(
        !occurrences(&result)
            .iter()
            .any(|row| matches!(row, RulesOccurrence::CountersPlaced { .. }))
    );
}

#[test]
fn optional_nonempty_draw_moves_library_top_once_or_skips_all_draw_work() {
    for choice in [OptionalEffectChoice::Take, OptionalEffectChoice::Decline] {
        let state = world(
            vec![A, B],
            A,
            vec![
                hand(
                    1,
                    A,
                    BodyTarget::None,
                    wrapped(crate::program::Instruction::DrawCards(Box::new(
                        crate::toolbox::DrawCardsSpec::simple(
                            crate::toolbox::PlayerSelector::Resolving(
                                crate::program::ResolvingPlayer::Controller,
                            ),
                            crate::program::ValueExpr::Literal(1.into()),
                        ),
                    ))),
                ),
                seed(2, A, ObjectDestination::LibraryBottom, creature()),
            ],
        );
        let state = resolve_top(cast(&state, A, 1, None).state()).into_state();
        let result = take_or_decline(&state, choice);
        assert_tail(&result);
        assert_eq!(
            result.draw_journal().events.len(),
            usize::from(choice == OptionalEffectChoice::Take)
        );
        assert_eq!(
            result
                .state()
                .execution()
                .window()
                .cards()
                .get(object(result.state(), 2))
                .unwrap()
                .zone(),
            if choice == OptionalEffectChoice::Take {
                ObjectZone::Hand(A)
            } else {
                ObjectZone::Library(A)
            }
        );
    }
}

#[test]
fn optional_type_child_retains_cursor_rejects_reused_offer_and_excluded_type_then_finishes_once() {
    let vocabulary = creature_types::selected_vocabulary().unwrap();
    let human = vocabulary.resolve_label("Human").unwrap();
    for choice in [OptionalEffectChoice::Take, OptionalEffectChoice::Decline] {
        let state = pending(wrapped(Instruction::PromptChoice(Box::new(
            crate::toolbox::PromptChoiceSpec {
                chooser: crate::toolbox::PlayerSelector::Resolving(
                    crate::program::ResolvingPlayer::Controller,
                ),
                domain: crate::toolbox::PromptDomain::Subtype {
                    kind: crate::toolbox::SubtypeChoiceKind::Creature,
                    excluding: BTreeSet::from([human]),
                    legacy_slot: Some(CreatureTypeSlot::new(0)),
                },
                category: crate::toolbox::ChoiceCategory::Generic,
                bind_to: crate::toolbox::BindingId(0),
                bind_unchosen: None,
                tag_partition: None,
                auto_resolve_trivial: false,
                option_branches: Vec::new(),
            },
        ))));
        let old = command_reply(&state, DecisionReply::OptionalEffect(choice));
        let result = take_or_decline(&state, choice);
        if choice == OptionalEffectChoice::Decline {
            assert_tail(&result);
            assert!(
                !occurrences(&result)
                    .iter()
                    .any(|row| matches!(row, RulesOccurrence::CreatureTypeChosen { .. }))
            );
            continue;
        }
        let child = result.state();
        assert!(matches!(
            child.pending_decision().unwrap().frame(),
            RuleFrame::PromptChoice(_)
        ));
        assert_eq!(child.execution().instruction_index(), Some(1));
        assert!(
            child
                .execution()
                .resolution_state()
                .unwrap()
                .locals()
                .is_empty()
        );
        reject(child, A, old, GameError::WrongDecision);
        reject(
            child,
            A,
            command_reply(
                child,
                DecisionReply::PromptChoice(crate::toolbox::PromptChoiceReply::CreatureType(human)),
            ),
            GameError::Effect(EffectError::SelectionOutsideDomain),
        );
        let elf = vocabulary.resolve_label("Elf").unwrap();
        let completed = reply(
            child,
            DecisionReply::PromptChoice(crate::toolbox::PromptChoiceReply::CreatureType(elf)),
        );
        assert_tail(&completed);
        assert_eq!(outcomes(&completed), 0);
        assert_eq!(occurrences(&completed).iter().filter(|row|matches!(row,RulesOccurrence::CreatureTypeChosen{slot,value,..} if *slot==CreatureTypeSlot::new(0) && *value==elf)).count(),1);
    }
}

#[test]
fn optional_token_batch_preserves_real_timestamp_child_then_creates_once_or_skips_it() {
    let vocabulary = creature_types::selected_vocabulary().unwrap();
    let definition = TokenDefinition::new(
        creature(),
        TokenName::Specified(
            spellwake_contracts::names::EnglishCardName::new("Optional Native Token".into())
                .unwrap(),
        ),
        &vocabulary,
    )
    .unwrap();
    let mut creation = TokenCreation::one(definition);
    creation.amount = crate::program::ValueExpr::Literal(2.into());
    for choice in [OptionalEffectChoice::Take, OptionalEffectChoice::Decline] {
        let state = pending(wrapped(Instruction::CreateTokens {
            creations: vec![creation.clone()],
        }));
        let result = take_or_decline(&state, choice);
        if choice == OptionalEffectChoice::Decline {
            assert_tail(&result);
            assert_eq!(
                result.state().execution().window().cards().iter().count(),
                5
            );
            continue;
        }
        let child = result.state();
        let RuleFrame::TimestampOrder(frame) = child.pending_decision().unwrap().frame() else {
            panic!("real timestamp child required")
        };
        assert_eq!(frame.instruction(), 1);
        assert_eq!(frame.members().len(), 2);
        let order = frame
            .members()
            .iter()
            .rev()
            .map(|member| member.member)
            .collect();
        let completed = reply(child, DecisionReply::TimestampOrder(order));
        assert_tail(&completed);
        assert_eq!(outcomes(&completed), 0);
        assert_eq!(
            completed
                .state()
                .execution()
                .window()
                .cards()
                .iter()
                .count(),
            7
        );
    }
}

fn watched(mut program: Program, condition: TriggerSpec, body: Vec<Instruction>) -> Program {
    program.triggered.push(TriggeredAbility {
        id: TriggeredAbilityId(1),
        condition,
        grouping: TriggerGrouping::EachOccurrence,
        body: ExecutableBody::new(BodyTarget::None, body),
    });
    program
}
fn captured_after_source_change(control: bool, body: Vec<Instruction>) -> GameState {
    let watcher = watched(
        Program::artifact(None, vec![]),
        crate::program::TriggerSpec::spell_cast(
            PlayerRelation::Opponent,
            crate::program::SpellFilter::of_type(CardType::Kindred),
        ),
        body,
    );
    let mut original = hand(1, B, BodyTarget::None, vec![gain(0)]);
    original.program.card_types.insert(CardType::Kindred);
    let response = if control {
        {
            struct _Tmp {
                parts: crate::characteristics::ResolvedPartsSpec,
                duration: crate::characteristics::ResolvedDuration,
            }
            let _t = _Tmp {
                parts: ResolvedPartsSpec(vec![ResolvedPartSpec::Control(
                    ControlRecipient::ResolvingController,
                )]),
                duration: ResolvedDuration::NoStatedDuration,
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
        }
    } else {
        crate::program::Instruction::ZoneOp(Box::new(crate::toolbox::ZoneOpSpec::move_all(
            crate::toolbox::ObjectQuery::Targets,
            crate::toolbox::ZoneDest::Hand,
        )))
    };
    let mut mana = Program::artifact(None, vec![]);
    mana.mana.push(ManaAbility {
        id: ManaAbilityId(1),
        output: ManaOutput::Fixed(BTreeMap::from([(ManaType::Blue, 1.into())])),
    });
    let state = world(
        vec![C, A, B],
        C,
        vec![
            original,
            hand(2, B, BodyTarget::SINGLE_PERMANENT, vec![response]),
            seed(
                10,
                A,
                ObjectDestination::Battlefield { controller: A },
                watcher,
            ),
            seed(
                11,
                B,
                ObjectDestination::Battlefield { controller: B },
                mana,
            ),
            seed(20, A, ObjectDestination::LibraryBottom, creature()),
        ],
    );
    let state = candidate(&state, C, GameCommand::Pass).into_state();
    let state = candidate(&state, A, GameCommand::Pass).into_state();
    let casted = cast(&state, B, 1, None);
    assert_eq!(casted.state().execution().window().entries().len(), 2);
    let trigger = casted
        .state()
        .execution()
        .window()
        .entries()
        .last()
        .unwrap();
    assert_eq!(trigger.controller(), A);
    let captured = casted
        .state()
        .execution()
        .execution_bindings(trigger.id())
        .unwrap()
        .trigger
        .as_ref()
        .unwrap()
        .clone();
    assert_eq!(captured.causes.members()[0].related_player, Some(B));
    let response = cast(
        casted.state(),
        B,
        2,
        Some(TargetRef::Object(object(casted.state(), 10))),
    );
    let changed = resolve_top(response.state());
    let current = changed
        .state()
        .execution()
        .window()
        .cards()
        .get(object(changed.state(), 10))
        .unwrap();
    if control {
        assert_eq!(current.controller(), Some(B));
        assert_eq!(current.id(), captured.source.object.id());
    } else {
        assert_eq!(current.zone(), ObjectZone::Hand(A));
        assert_ne!(current.id(), captured.source.object.id());
    }
    let trigger = changed
        .state()
        .execution()
        .window()
        .entries()
        .last()
        .unwrap();
    assert_eq!(trigger.controller(), A);
    assert_eq!(
        changed
            .state()
            .execution()
            .execution_bindings(trigger.id())
            .unwrap()
            .trigger
            .as_ref()
            .unwrap(),
        &captured
    );
    resolve_top(changed.state()).into_state()
}

#[test]
fn captured_optional_controller_survives_real_source_departure_and_control_change() {
    for control in [false, true] {
        let state = captured_after_source_change(
            control,
            vec![
                Instruction::OptionalSequence {
                    chooser: ResolvingPlayer::Controller,
                    instructions: 1,
                },
                gain(1),
            ],
        );
        assert_eq!(optional_frame(&state).chooser(), A);
        for actor in [B, C] {
            reject(
                &state,
                actor,
                command_reply(
                    &state,
                    DecisionReply::OptionalEffect(OptionalEffectChoice::Take),
                ),
                GameError::WrongChooser,
            );
        }
        let result = reply(
            &state,
            DecisionReply::OptionalEffect(OptionalEffectChoice::Take),
        );
        assert_eq!(result.state().execution().life_total(A), Some(&21.into()));
        assert_eq!(result.state().execution().life_total(B), Some(&20.into()));
        assert_eq!(result.state().execution().life_total(C), Some(&20.into()));
    }
}

fn captured_offer_body() -> Vec<Instruction> {
    vec![
        Instruction::PromptChoice(Box::new(
            crate::toolbox::PromptChoiceSpec::offer_dynamic_mana_payment(
                ResolvingPlayer::UniqueRelatedPlayer,
                ManaCost::generic(1.into()),
                crate::program::ValueExpr::Literal(0.into()),
                4,
                1,
            ),
        )),
        Instruction::OptionalSequence {
            chooser: ResolvingPlayer::Controller,
            instructions: 1,
        },
        crate::program::Instruction::DrawCards(Box::new(crate::toolbox::DrawCardsSpec::simple(
            crate::toolbox::PlayerSelector::Resolving(crate::program::ResolvingPlayer::Controller),
            crate::program::ValueExpr::Literal(1.into()),
        ))),
        gain(0),
    ]
}

#[test]
fn active_controller_and_caster_keep_three_distinct_roles_after_source_change() {
    for control in [false, true] {
        let state = captured_after_source_change(control, captured_offer_body());
        let frame = payment_frame(&state);
        assert_eq!(frame.payer(), B);
        assert_eq!(state.execution().window().active_player(), C);
        for actor in [A, C] {
            reject(
                &state,
                actor,
                command_reply(
                    &state,
                    DecisionReply::ResolutionPayment(ResolutionPaymentReply::Decline),
                ),
                GameError::WrongChooser,
            );
            reject(
                &state,
                actor,
                GameCommand::ActivateMana {
                    source: object(&state, 11),
                    ability: ABILITY,
                },
                GameError::WrongChooser,
            );
        }
        let funded = candidate(
            &state,
            B,
            GameCommand::ActivateMana {
                source: object(&state, 11),
                ability: ABILITY,
            },
        );
        assert_eq!(funded.state().execution().mana(B, ManaType::Blue), 1.into());
        assert_eq!(payment_frame(funded.state()).payer(), B);
        let may = reply(
            funded.state(),
            DecisionReply::ResolutionPayment(ResolutionPaymentReply::Decline),
        );
        assert_eq!(optional_frame(may.state()).chooser(), A);
        for actor in [B, C] {
            reject(
                may.state(),
                actor,
                command_reply(
                    may.state(),
                    DecisionReply::OptionalEffect(OptionalEffectChoice::Take),
                ),
                GameError::WrongChooser,
            );
        }
        let result = reply(
            may.state(),
            DecisionReply::OptionalEffect(OptionalEffectChoice::Take),
        );
        assert_eq!(
            result
                .state()
                .execution()
                .window()
                .cards()
                .get(object(result.state(), 20))
                .unwrap()
                .zone(),
            ObjectZone::Hand(A)
        );
        assert_eq!(result.state().execution().mana(B, ManaType::Blue), 1.into());
        assert_eq!(result.draw_journal().events.len(), 1);
    }
}

#[test]
fn died_source_owner_graveyard_and_captured_controller_draw_are_independent() {
    let source = watched(
        Program::creature(None, 1.into(), 0.into(), BTreeSet::new()),
        crate::program::TriggerSpec::dies(crate::program::ObjectFilter::self_object()),
        vec![
            Instruction::OptionalSequence {
                chooser: ResolvingPlayer::Controller,
                instructions: 1,
            },
            crate::program::Instruction::DrawCards(Box::new(
                crate::toolbox::DrawCardsSpec::simple(
                    crate::toolbox::PlayerSelector::Resolving(
                        crate::program::ResolvingPlayer::Controller,
                    ),
                    crate::program::ValueExpr::Literal(1.into()),
                ),
            )),
        ],
    );
    let state = world(
        vec![A, B],
        A,
        vec![
            seed(
                1,
                A,
                ObjectDestination::Battlefield { controller: B },
                source,
            ),
            seed(2, B, ObjectDestination::LibraryBottom, creature()),
        ],
    );
    let source = state
        .execution()
        .window()
        .cards()
        .get(object(&state, 1))
        .unwrap();
    assert_eq!(source.zone(), ObjectZone::Graveyard(A));
    let entry = state.execution().window().entries().last().unwrap();
    assert_eq!(entry.controller(), B);
    let binding = state
        .execution()
        .execution_bindings(entry.id())
        .unwrap()
        .trigger
        .as_ref()
        .unwrap();
    assert_eq!(binding.source.object.owner(), A);
    assert_eq!(binding.source.object.controller(), Some(B));
    assert_ne!(binding.source.object.id(), source.id());
    let state = resolve_top(&state).into_state();
    assert_eq!(optional_frame(&state).chooser(), B);
    reject(
        &state,
        A,
        command_reply(
            &state,
            DecisionReply::OptionalEffect(OptionalEffectChoice::Take),
        ),
        GameError::WrongChooser,
    );
    let result = reply(
        &state,
        DecisionReply::OptionalEffect(OptionalEffectChoice::Take),
    );
    assert_eq!(
        result
            .state()
            .execution()
            .window()
            .cards()
            .get(object(result.state(), 2))
            .unwrap()
            .zone(),
        ObjectZone::Hand(B)
    );
    assert_eq!(
        result
            .state()
            .execution()
            .window()
            .cards()
            .get(object(result.state(), 1))
            .unwrap()
            .zone(),
        ObjectZone::Graveyard(A)
    );
}

#[test]
fn malformed_related_player_sets_reject_at_the_actual_role_owner_without_fallback() {
    let state = captured_after_source_change(false, captured_offer_body());
    let actual = state.execution();
    let entry = payment_frame(&state).entry();
    let original = actual
        .execution_bindings(entry)
        .unwrap()
        .trigger
        .as_ref()
        .unwrap()
        .causes
        .members()[0]
        .clone();
    let mut absent = original.clone();
    absent.related_player = None;
    let mut foreign = original.clone();
    foreign.related_player = Some(SeatId(99));
    let mut different = original.clone();
    different.related_player = Some(A);
    different.occurrence.index += 1;
    // Explicit malformed-input seam; these are direct owner fault injections,
    // never admitted publications or ordinary root input coverage.
    for members in [
        vec![],
        vec![absent],
        vec![foreign],
        vec![original.clone(), different],
    ] {
        let mut execution = actual.clone();
        let StackPayload::Instructions(payload) = execution.payloads.get_mut(&entry).unwrap()
        else {
            panic!("instruction payload")
        };
        payload
            .bindings
            .trigger
            .as_mut()
            .unwrap()
            .causes
            .set_test_members(members);
        let Some(PendingWork::ResolutionPayment(work)) = execution.pending_work.take() else {
            panic!("payment owner")
        };
        execution.pending_work = Some(PendingWork::Resolution(work.cursor));
        let saved = execution.clone();
        assert_eq!(
            execution
                .begin_resolution_choice(&captured_offer_body()[0], &mut RulesJournal::default()),
            Err(EffectError::InvalidResolvingPlayer)
        );
        assert_eq!(execution, saved);
    }
    assert_eq!(
        actual.resolving_player(ResolvingPlayer::UniqueRelatedPlayer),
        Ok(B)
    );
}

#[test]
fn tap_watcher_waits_through_real_color_child_and_payment_until_the_rules_boundary() {
    for should_pay in [false, true] {
        let mut source = watched(
            Program::artifact(None, vec![]),
            crate::program::TriggerSpec::becomes_tapped(crate::program::ObjectFilter::self_object()),
            vec![gain(1)],
        );
        source.mana.push(ManaAbility {
            id: ManaAbilityId(1),
            output: ManaOutput::ChooseColor {
                amount: 1.into(),
                colors: BTreeSet::from([ManaType::Blue, ManaType::Red]),
            },
        });
        let state = world(
            vec![A, B],
            A,
            vec![
                hand(1, A, BodyTarget::None, offer(1)),
                seed(
                    10,
                    B,
                    ObjectDestination::Battlefield { controller: A },
                    source,
                ),
            ],
        );
        let state = resolve_top(cast(&state, A, 1, None).state()).into_state();
        let activation = tap(&state, 10);
        let events = activation
            .rules_journal()
            .records
            .iter()
            .filter_map(|row| match row {
                RulesRecord::Event(event) => Some(event),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(events.len(), 2);
        assert!(matches!(
            events[0].cause,
            EventCause::ManaAbilityCost { .. }
        ));
        assert!(matches!(
            events[1].cause,
            EventCause::ManaAbilityActivated { .. }
        ));
        let waiting = activation
            .state()
            .execution()
            .waiting_triggers()
            .collect::<Vec<_>>();
        assert_eq!(waiting.len(), 1);
        assert_eq!(waiting[0].controller, A);
        assert_eq!(waiting[0].source.object.owner(), B);
        assert_eq!(waiting[0].source.object.id(), object(&state, 10));
        assert_eq!(waiting[0].source.object.tapped(), Some(true));
        let captured = (*waiting[0]).clone();
        assert_eq!(activation.state().execution().window().entries().len(), 1);
        let restored = reply(activation.state(), DecisionReply::ManaColor(ManaType::Blue));
        assert_eq!(
            restored
                .state()
                .execution()
                .waiting_triggers()
                .collect::<Vec<_>>(),
            vec![&captured]
        );
        assert_eq!(restored.state().execution().window().entries().len(), 1);
        assert!(
            occurrences(&restored)
                .iter()
                .any(|row| matches!(row, RulesOccurrence::ManaProduced { .. }))
        );
        assert_eq!(
            restored
                .rules_journal()
                .records
                .iter()
                .filter_map(|row| match row {
                    RulesRecord::Event(event) => Some(event),
                    _ => None,
                })
                .count(),
            1
        );
        let paid = reply(
            restored.state(),
            DecisionReply::ResolutionPayment(if should_pay {
                ResolutionPaymentReply::Pay(ManaPayment {
                    from_lots: Default::default(),
                    spend: vec![ManaSpend {
                        mana_type: ManaType::Blue,
                        amount: 1.into(),
                    }],
                })
            } else {
                ResolutionPaymentReply::Decline
            }),
        );
        assert_eq!(paid.state().execution().waiting_triggers().count(), 0);
        let entries = paid.state().execution().window().entries();
        assert_eq!(entries.len(), 1);
        assert!(matches!(
            entries[0].kind(),
            StackKind::TriggeredAbility { .. }
        ));
        assert_eq!(
            paid.state().execution().life_total(A),
            Some(&if should_pay { 20 } else { 22 }.into())
        );
        let finished = resolve_top(paid.state());
        assert_eq!(
            finished.state().execution().life_total(A),
            Some(&if should_pay { 21 } else { 23 }.into())
        );
    }
}

#[test]
fn exhaustion_of_root_revision_decision_identity_executor_revision_and_cursor_is_atomic() {
    let base = pending(offer(3));
    let mut root = base.clone();
    root.set_test_input_revision(u64::MAX);
    reject(
        &root,
        A,
        command_reply(
            &root,
            DecisionReply::ResolutionPayment(ResolutionPaymentReply::Decline),
        ),
        GameError::RevisionExhausted,
    );
    let mut ids = base.clone();
    ids.set_test_next_decision(u64::MAX);
    reject(
        &ids,
        A,
        GameCommand::ActivateMana {
            source: object(&ids, 10),
            ability: ABILITY,
        },
        GameError::DecisionIdentityExhausted,
    );
    let mut execution = base.execution().clone();
    execution.revision = u64::MAX;
    let frame = execution.pending_resolution_decision().unwrap();
    let saved = execution.clone();
    assert_eq!(
        execution.resume_rule(
            u64::MAX,
            &frame,
            A,
            DecisionReply::ResolutionPayment(ResolutionPaymentReply::Decline),
            None
        ),
        Err(EffectError::Window(StackWindowError::RevisionExhausted))
    );
    assert_eq!(execution, saved);
    let state = pending(wrapped(gain(1)));
    let mut execution = state.execution().clone();
    let Some(PendingWork::OptionalEffect(work)) = &mut execution.pending_work else {
        panic!("optional owner")
    };
    work.cursor.instruction = usize::MAX;
    let frame = execution.pending_resolution_decision().unwrap();
    let saved = execution.clone();
    assert_eq!(
        execution.resume_rule(
            execution.revision(),
            &frame,
            A,
            DecisionReply::OptionalEffect(OptionalEffectChoice::Take),
            None
        ),
        Err(EffectError::InconsistentProgram)
    );
    assert_eq!(execution, saved);
    let mut publication = base.execution().clone();
    publication.triggers.set_test_next_event(u64::MAX);
    let saved = publication.clone();
    assert_eq!(
        publication.begin_mana_activation(publication.revision(), A, object(&base, 10), ABILITY),
        Err(EffectError::Trigger(
            crate::occurrence::TriggerError::EventIdentityExhausted
        ))
    );
    assert_eq!(publication, saved);
    assert!(
        reply(
            &base,
            DecisionReply::ResolutionPayment(ResolutionPaymentReply::Decline)
        )
        .state()
        .pending_decision()
        .is_none()
    );
}

fn spell_optional(operation: Instruction) -> (GameState, StackEntryId) {
    let state = world(
        vec![A, B],
        A,
        vec![
            hand(1, A, BodyTarget::None, vec![gain(0)]),
            hand(
                2,
                A,
                BodyTarget::single_spell(crate::program::SpellFilter::any()),
                wrapped(operation),
            ),
        ],
    );
    let original = cast(&state, A, 1, None);
    let donor = original.state().execution().window().entries()[0].id();
    let wrapped = cast(original.state(), A, 2, Some(TargetRef::Stack(donor)));
    (resolve_top(wrapped.state()).into_state(), donor)
}

#[test]
fn optional_counter_removes_the_real_target_spell_once_or_skips() {
    for choice in [OptionalEffectChoice::Take, OptionalEffectChoice::Decline] {
        let (state, donor) = spell_optional(Instruction::StackOp(Box::new(
            crate::toolbox::StackOpSpec::counter_targeted(
                crate::toolbox::CounterDestination::Graveyard,
            ),
        )));
        let result = take_or_decline(&state, choice);
        assert_tail(&result);
        assert_eq!(
            result
                .state()
                .execution()
                .window()
                .entries()
                .iter()
                .any(|entry| entry.id() == donor),
            choice == OptionalEffectChoice::Decline
        );
        assert_eq!(
            result
                .state()
                .execution()
                .window()
                .cards()
                .get(object(result.state(), 1))
                .unwrap()
                .zone(),
            if choice == OptionalEffectChoice::Take {
                ObjectZone::Graveyard(A)
            } else {
                ObjectZone::Stack
            }
        );
    }
}

#[test]
fn optional_targetless_copy_uses_the_real_copy_owner_without_a_target_child() {
    for choice in [OptionalEffectChoice::Take, OptionalEffectChoice::Decline] {
        let (state, donor) = spell_optional(Instruction::StackOp(Box::new(
            crate::toolbox::StackOpSpec::copy_targeted_spell(),
        )));
        let result = take_or_decline(&state, choice);
        assert_tail(&result);
        assert_eq!(
            result.state().execution().window().spell_copies().len(),
            usize::from(choice == OptionalEffectChoice::Take)
        );
        let copies = occurrences(&result)
            .into_iter()
            .filter_map(|row| match row {
                RulesOccurrence::CopyCreated { original, .. } => Some(*original),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(
            copies,
            if choice == OptionalEffectChoice::Take {
                vec![donor]
            } else {
                vec![]
            }
        );
    }
}

#[test]
fn optional_copy_preserves_actual_copy_targets_child_and_rejects_reused_or_foreign_replies() {
    let initial = world(
        vec![A, B],
        A,
        vec![
            hand(1, A, BodyTarget::None, vec![gain(0)]),
            hand(4, A, BodyTarget::None, vec![gain(0)]),
            hand(
                2,
                A,
                BodyTarget::single_spell(crate::program::SpellFilter::any()),
                vec![Instruction::StackOp(Box::new(
                    crate::toolbox::StackOpSpec::counter_targeted(
                        crate::toolbox::CounterDestination::Graveyard,
                    ),
                ))],
            ),
            hand(
                3,
                A,
                BodyTarget::single_spell(crate::program::SpellFilter::copyable_instruction()),
                wrapped(Instruction::StackOp(Box::new(
                    crate::toolbox::StackOpSpec::copy_targeted_spell(),
                ))),
            ),
        ],
    );
    let base = cast(&initial, A, 1, None);
    let first = base.state().execution().window().entries()[0].id();
    let alternative = cast(base.state(), A, 4, None);
    let alternate = alternative.state().execution().window().entries()[1].id();
    let counter = cast(alternative.state(), A, 2, Some(TargetRef::Stack(first)));
    let donor = counter.state().execution().window().entries()[2].id();
    let copier = cast(counter.state(), A, 3, Some(TargetRef::Stack(donor)));
    let state = resolve_top(copier.state()).into_state();
    let declined = take_or_decline(&state, OptionalEffectChoice::Decline);
    assert_tail(&declined);
    assert_eq!(
        declined.state().execution().window().spell_copies().len(),
        0
    );
    let old = command_reply(
        &state,
        DecisionReply::OptionalEffect(OptionalEffectChoice::Take),
    );
    let taken = take_or_decline(&state, OptionalEffectChoice::Take);
    let child = taken.state();
    let RuleFrame::CopyTargets(frame) = child.pending_decision().unwrap().frame() else {
        panic!("actual CopyTargets child")
    };
    assert_eq!(frame.instruction_index(), 1);
    assert_eq!(frame.original(), donor);
    assert_eq!(child.execution().instruction_index(), Some(1));
    reject(child, A, old, GameError::WrongDecision);
    reject(
        child,
        B,
        command_reply(
            child,
            DecisionReply::CopyTargets(CopyTargetChoice::Change(TargetRef::Stack(alternate))),
        ),
        GameError::WrongChooser,
    );
    reject(
        child,
        A,
        command_reply(
            child,
            DecisionReply::CopyTargets(CopyTargetChoice::Change(TargetRef::Player(B))),
        ),
        GameError::Effect(EffectError::IllegalTarget),
    );
    let mut stale = frame.clone();
    stale.instruction = 0;
    let saved = child.clone();
    assert_eq!(
        native_packets::resume_rule(
            child.execution(),
            child.execution().revision(),
            &EffectDecision::CopyTargets(stale),
            A,
            DecisionReply::CopyTargets(CopyTargetChoice::Retain),
            None
        ),
        Err(EffectError::WrongResolutionFrame)
    );
    assert_eq!(child, &saved);
    let command = command_reply(
        child,
        DecisionReply::CopyTargets(CopyTargetChoice::Change(TargetRef::Stack(alternate))),
    );
    let result = candidate(child, A, command.clone());
    assert_tail(&result);
    assert_eq!(outcomes(&result), 0);
    assert_eq!(result.state().execution().window().spell_copies().len(), 1);
    let copy = result
        .state()
        .execution()
        .window()
        .entries()
        .last()
        .unwrap();
    assert!(matches!(copy.kind(),StackKind::SpellCopy{copied_from,..} if *copied_from==donor));
    assert_eq!(
        copy.targets().targets()[0].choice().target,
        TargetRef::Stack(alternate)
    );
    reject(result.state(), A, command, GameError::NoDecision);
}

fn copy_receiver(source_cost: SourceCost) -> Program {
    Program::artifact(
        None,
        vec![ActivatedAbility {
            id: ActivatedAbilityId(1),
            mana_cost: ManaCost::free(),
            source_cost,
            body: ExecutableBody::new(
                BodyTarget::SINGLE_PERMANENT,
                wrapped(Instruction::ApplyContinuous(Box::new(
                    crate::toolbox::ApplyContinuousSpec::become_copy(
                        crate::toolbox::ObjectQuery::Source,
                        crate::toolbox::ObjectQuery::Targets,
                        ResolvedDuration::NoStatedDuration,
                        crate::program::CopyExceptions {
                            override_pt: None,
                            ..Default::default()
                        },
                    ),
                ))),
            ),
        }],
    )
}
fn receiver_optional(cost: SourceCost) -> (GameState, ObjectId, ObjectId) {
    let initial = world(
        vec![A, B],
        A,
        vec![
            seed(
                1,
                A,
                ObjectDestination::Battlefield { controller: A },
                copy_receiver(cost),
            ),
            seed(
                2,
                B,
                ObjectDestination::Battlefield { controller: B },
                creature(),
            ),
        ],
    );
    let receiver = object(&initial, 1);
    let donor = object(&initial, 2);
    let activated = candidate(
        &initial,
        A,
        GameCommand::ActivateAbility {
            source: receiver,
            ability: ActivatedAbilityId(1),
            target: TargetRef::Object(donor),
            payment: None,
        },
    );
    (resolve_top(activated.state()).into_state(), receiver, donor)
}

#[test]
fn optional_permanent_copy_captures_the_real_donor_or_leaves_receiver_unchanged() {
    for choice in [OptionalEffectChoice::Take, OptionalEffectChoice::Decline] {
        let (state, receiver, donor) = receiver_optional(SourceCost::None);
        let result = take_or_decline(&state, choice);
        assert_tail(&result);
        assert_eq!(object(result.state(), 1), receiver);
        assert_eq!(object(result.state(), 2), donor);
        assert_eq!(
            result.state().execution().copiable_program(receiver),
            Some(&if choice == OptionalEffectChoice::Take {
                creature()
            } else {
                copy_receiver(SourceCost::None)
            })
        );
        assert_eq!(
            result
                .state()
                .execution()
                .window()
                .cards()
                .get(receiver)
                .unwrap()
                .controller(),
            Some(A)
        );
    }
}

#[test]
fn optional_copy_receiver_departure_removes_take_while_the_donor_is_still_legal() {
    let (state, receiver, donor) = receiver_optional(SourceCost::Sacrifice);
    assert_eq!(
        state
            .execution()
            .window()
            .cards()
            .get(donor)
            .unwrap()
            .zone(),
        ObjectZone::Battlefield
    );
    assert_ne!(object(&state, 1), receiver);
    assert_eq!(
        state
            .execution()
            .window()
            .cards()
            .get(object(&state, 1))
            .unwrap()
            .zone(),
        ObjectZone::Graveyard(A)
    );
    assert_eq!(
        optional_frame(&state).options(),
        &BTreeSet::from([OptionalEffectChoice::Decline])
    );
    reject(
        &state,
        A,
        command_reply(
            &state,
            DecisionReply::OptionalEffect(OptionalEffectChoice::Take),
        ),
        GameError::Effect(EffectError::IllegalOptionalEffect),
    );
    let result = reply(
        &state,
        DecisionReply::OptionalEffect(OptionalEffectChoice::Decline),
    );
    assert_tail(&result);
    assert_eq!(result.state().execution().continuous_effects().count(), 0);
}

#[test]
fn fresh_receiver_incarnation_cannot_replace_the_exact_captured_copy_source() {
    let (state, receiver, donor) = receiver_optional(SourceCost::Sacrifice);
    // Privileged native setup: the existing complete-world fixture move returns
    // the sacrificed card and assigns its real new battlefield timestamp. No
    // authored resurrection clause or root action is claimed for this prefix.
    let mut execution = state.execution().clone();
    let Some(PendingWork::OptionalEffect(work)) = execution.pending_work.take() else {
        panic!("optional owner")
    };
    let (mut execution, movements, setup_journal) = execution
        .qualification_zone_moves(
            execution.revision(),
            &[(
                object(&state, 1),
                ObjectDestination::Battlefield { controller: A },
            )],
        )
        .unwrap()
        .into_parts();
    assert_eq!(movements.len(), 1);
    assert!(!setup_journal.records.is_empty());
    let fresh = execution
        .window
        .cards()
        .current_identity(CardId(1))
        .unwrap();
    assert_ne!(fresh, receiver);
    assert_eq!(
        execution.window.cards().get(fresh).unwrap().zone(),
        ObjectZone::Battlefield
    );
    assert_eq!(
        execution.window.cards().get(donor).unwrap().zone(),
        ObjectZone::Battlefield
    );
    execution.pending_work = Some(PendingWork::Resolution(work.cursor));
    execution
        .begin_resolution_choice(
            &Instruction::OptionalSequence {
                chooser: ResolvingPlayer::Controller,
                instructions: 1,
            },
            &mut RulesJournal::default(),
        )
        .unwrap();
    native_packets::retain_privileged_setup(&execution, &movements, &setup_journal);
    let EffectDecision::OptionalEffect(frame) = execution.pending_resolution_decision().unwrap()
    else {
        panic!("optional frame")
    };
    assert_eq!(
        frame.options(),
        &BTreeSet::from([OptionalEffectChoice::Decline])
    );
    let saved = execution.clone();
    assert_eq!(
        native_packets::resume_rule(
            &execution,
            execution.revision(),
            &EffectDecision::OptionalEffect(frame.clone()),
            A,
            DecisionReply::OptionalEffect(OptionalEffectChoice::Take),
            None
        ),
        Err(EffectError::IllegalOptionalEffect)
    );
    assert_eq!(execution, saved);
    let (completed, _, append) = native_packets::resume_rule(
        &execution,
        execution.revision(),
        &EffectDecision::OptionalEffect(frame),
        A,
        DecisionReply::OptionalEffect(OptionalEffectChoice::Decline),
        None,
    )
    .unwrap()
    .into_parts();
    assert_eq!(completed.life_total(A), Some(&22.into()));
    assert_eq!(
        completed.copiable_program(fresh),
        Some(&copy_receiver(SourceCost::Sacrifice))
    );
    assert_eq!(
        append
            .records
            .iter()
            .filter(|row| matches!(row, RulesRecord::OptionalEffectOutcome { .. }))
            .count(),
        1
    );
}

#[test]
fn target_spell_controller_role_comes_from_the_actual_card_spell_or_spell_copy() {
    for use_copy in [false, true] {
        let caster = if use_copy { A } else { B };
        let cards = vec![
            hand(1, caster, BodyTarget::None, vec![gain(0)]),
            hand(
                2,
                B,
                BodyTarget::single_spell(crate::program::SpellFilter::copyable_instruction()),
                vec![Instruction::StackOp(Box::new(
                    crate::toolbox::StackOpSpec::copy_targeted_spell(),
                ))],
            ),
            hand(
                3,
                A,
                BodyTarget::single_spell(crate::program::SpellFilter::any()),
                vec![
                    Instruction::PromptChoice(Box::new(
                        crate::toolbox::PromptChoiceSpec::offer_dynamic_mana_payment(
                            ResolvingPlayer::TargetSpellController,
                            ManaCost::free(),
                            crate::program::ValueExpr::Literal(0.into()),
                            2,
                            1,
                        ),
                    )),
                    gain(2),
                ],
            ),
        ];
        // The copy is controlled by B while its original is controlled by A.
        // Reading copied_from instead of the actual target would choose wrongly.
        let state = world(vec![A, B], caster, cards);
        let casted = cast(&state, caster, 1, None);
        let original = casted.state().execution().window().entries()[0].id();
        let state = if use_copy {
            let priority = candidate(casted.state(), A, GameCommand::Pass);
            let copier = cast(priority.state(), B, 2, Some(TargetRef::Stack(original)));
            resolve_top(copier.state()).into_state()
        } else {
            casted.into_state()
        };
        let target = state.execution().window().entries().last().unwrap();
        assert_eq!(
            matches!(target.kind(), StackKind::SpellCopy { .. }),
            use_copy
        );
        assert_eq!(target.controller(), B);
        assert_eq!(state.execution().window().entries()[0].controller(), caster);
        let target = target.id();
        let WindowPhase::Priority(PriorityPhase::Ready { holder }) =
            state.execution().window().phase()
        else {
            panic!("actual priority")
        };
        let state = if holder == A {
            state
        } else {
            candidate(&state, B, GameCommand::Pass).into_state()
        };
        let offered = cast(&state, A, 3, Some(TargetRef::Stack(target)));
        let state = resolve_top(offered.state()).into_state();
        assert_eq!(payment_frame(&state).payer(), B);
        reject(
            &state,
            A,
            command_reply(
                &state,
                DecisionReply::ResolutionPayment(ResolutionPaymentReply::Pay(
                    ManaPayment::default(),
                )),
            ),
            GameError::WrongChooser,
        );
        let result = reply(
            &state,
            DecisionReply::ResolutionPayment(ResolutionPaymentReply::Pay(ManaPayment::default())),
        );
        assert_eq!(result.state().execution().life_total(A), Some(&20.into()));
        assert!(
            result.rules_journal().records.iter().any(
                |row| matches!(row,RulesRecord::ResolutionPaymentOutcome{payer,..} if *payer==B)
            )
        );
    }
}

#[test]
fn payment_mana_source_denials_reach_physical_control_tap_capability_and_age_guards() {
    let mana = |mut program: Program| {
        program.mana.push(ManaAbility {
            id: ManaAbilityId(1),
            output: ManaOutput::Fixed(BTreeMap::from([(ManaType::Blue, 1.into())])),
        });
        program
    };
    let initial = world(
        vec![A, B],
        A,
        vec![
            hand(1, A, BodyTarget::None, offer(3)),
            seed(
                10,
                A,
                ObjectDestination::Battlefield { controller: A },
                mana(Program::artifact(None, vec![])),
            ),
            seed(
                11,
                B,
                ObjectDestination::Battlefield { controller: B },
                mana(Program::artifact(None, vec![])),
            ),
            seed(
                12,
                A,
                ObjectDestination::Battlefield { controller: A },
                mana(creature()),
            ),
            seed(
                13,
                A,
                ObjectDestination::Hand,
                mana(Program::artifact(None, vec![])),
            ),
            seed(
                14,
                A,
                ObjectDestination::Battlefield { controller: A },
                Program::artifact(None, vec![]),
            ),
        ],
    );
    let stale = object(&initial, 1);
    let state = resolve_top(cast(&initial, A, 1, None).state()).into_state();
    assert_eq!(
        state
            .execution()
            .window()
            .cards()
            .get(object(&state, 12))
            .unwrap()
            .has_continuous_control_since_turn_start(),
        Some(false)
    );
    let funded = tap(&state, 10).into_state();
    for (source, ability, error) in [
        (
            object(&funded, 10),
            ABILITY,
            EffectError::Window(StackWindowError::SourceTapped),
        ),
        (
            object(&funded, 11),
            ABILITY,
            EffectError::Window(StackWindowError::WrongController),
        ),
        (
            object(&funded, 12),
            ABILITY,
            EffectError::CreatureTapRestricted,
        ),
        (
            object(&funded, 13),
            ABILITY,
            EffectError::Window(StackWindowError::Objects(
                crate::object_identity::ObjectIdentityError::NotPermanent,
            )),
        ),
        (object(&funded, 14), ABILITY, EffectError::NotManaSource),
        (
            object(&funded, 11),
            ManaAbilityRef::Printed(ManaAbilityId(99)),
            EffectError::UnknownManaAbility,
        ),
        (stale, ABILITY, EffectError::UnknownObject),
    ] {
        reject(
            &funded,
            A,
            GameCommand::ActivateMana { source, ability },
            GameError::Effect(error),
        );
        let result = reply(
            &funded,
            DecisionReply::ResolutionPayment(ResolutionPaymentReply::Decline),
        );
        assert_eq!(result.state().execution().mana(A, ManaType::Blue), 1.into());
        assert_eq!(
            result
                .state()
                .execution()
                .window()
                .cards()
                .get(object(&funded, 10))
                .unwrap()
                .tapped(),
            Some(true)
        );
    }
}

#[test]
fn colored_resolution_cost_rejects_missing_required_color_before_valid_exact_spend() {
    let state = pending(vec![
        Instruction::PromptChoice(Box::new(
            crate::toolbox::PromptChoiceSpec::offer_dynamic_mana_payment(
                ResolvingPlayer::Controller,
                ManaCost::specific(ManaType::Blue, 1.into()),
                crate::program::ValueExpr::Literal(0.into()),
                2,
                1,
            ),
        )),
        gain(2),
    ]);
    let state = tap(&state, 10).into_state();
    reject(
        &state,
        A,
        command_reply(
            &state,
            DecisionReply::ResolutionPayment(ResolutionPaymentReply::Pay(ManaPayment {
                from_lots: Default::default(),
                spend: vec![ManaSpend {
                    mana_type: ManaType::Red,
                    amount: 1.into(),
                }],
            })),
        ),
        GameError::Effect(EffectError::InvalidManaPayment(
            ManaPaymentError::MissingRequired,
        )),
    );
    let result = reply(
        &state,
        DecisionReply::ResolutionPayment(ResolutionPaymentReply::Pay(ManaPayment {
            from_lots: Default::default(),
            spend: vec![ManaSpend {
                mana_type: ManaType::Blue,
                amount: 1.into(),
            }],
        })),
    );
    assert_eq!(result.state().execution().mana(A, ManaType::Blue), 0.into());
    assert_eq!(result.state().execution().life_total(A), Some(&20.into()));
}

#[test]
fn optional_damage_preserves_the_real_order_child_and_consumes_the_offer_once() {
    let body = vec![
        Instruction::ApplyContinuous(Box::new(crate::toolbox::ApplyContinuousSpec {
            selector: None,
            payload: crate::toolbox::ContinuousPayload::TargetPreventionShield { amount: 1.into() },
            duration: crate::characteristics::ResolvedDuration::UntilEndOfTurn,
            condition: None,
            one_shot: true,
            as_emblem: None,
            emblem_label: None,
        })),
        Instruction::ApplyContinuous(Box::new(
            crate::toolbox::ApplyContinuousSpec::damage_multiplier(2.into()),
        )),
        Instruction::OptionalSequence {
            chooser: ResolvingPlayer::Controller,
            instructions: 1,
        },
        Instruction::DealDamage(Box::new(crate::toolbox::DealDamageSpec {
            source: crate::toolbox::DamageSourceQuery::EntrySource,
            recipients: crate::toolbox::DamageTargetQuery::Targets,
            amount: crate::program::DamageAmountSpec::Expr(crate::program::ValueExpr::Literal(
                3.into(),
            )),
            exile_if_dies: false,
        })),
        gain(2),
    ];
    let initial = world(
        vec![A, B],
        A,
        vec![
            hand(1, A, BodyTarget::SINGLE_CREATURE, body),
            seed(
                2,
                B,
                ObjectDestination::Battlefield { controller: B },
                creature(),
            ),
        ],
    );
    let target = object(&initial, 2);
    let state =
        resolve_top(cast(&initial, A, 1, Some(TargetRef::Object(target))).state()).into_state();
    let declined = take_or_decline(&state, OptionalEffectChoice::Decline);
    assert_tail(&declined);
    assert_eq!(
        declined
            .state()
            .execution()
            .battlefield_marked_damage(target),
        Some(0.into())
    );
    assert_eq!(declined.state().execution().prevention_shields().count(), 1);
    assert_eq!(declined.state().execution().damage_multipliers().count(), 1);
    let taken = take_or_decline(&state, OptionalEffectChoice::Take);
    let child = taken.state();
    let RuleFrame::Damage(DamageDecision::Order(frame)) = child.pending_decision().unwrap().frame()
    else {
        panic!("actual damage order child")
    };
    assert_eq!(frame.applications().len(), 2);
    assert_eq!(frame.chooser(), B);
    let Some(DamageParentView::Resolution(cursor)) = child.execution().damage_parent() else {
        panic!("actual moved resolution cursor")
    };
    assert_eq!(cursor.entry(), optional_frame(&state).entry());
    assert_eq!(cursor.instruction(), 3);
    assert_eq!(child.execution().life_total(A), Some(&20.into()));
    let multiplier = frame
        .applications()
        .iter()
        .find(|application| {
            matches!(
                application.effect,
                crate::damage::DamageEffectId::Multiplier(_)
            )
        })
        .copied()
        .unwrap();
    let result = reply(child, DecisionReply::DamageOrder(multiplier));
    assert_tail(&result);
    assert_eq!(outcomes(&result), 0);
    assert_eq!(
        result.state().execution().battlefield_marked_damage(target),
        Some(5.into())
    );
    assert_eq!(result.damage_journal().applied.len(), 1);
    assert_eq!(result.damage_journal().multiplications.len(), 1);
    assert_eq!(result.damage_journal().preventions.len(), 1);
}

#[test]
fn forged_payment_and_color_frames_cannot_replace_current_owned_work() {
    let state = tap(&pending(offer(3)), 10).into_state();
    let actual = payment_frame(&state);
    let mut wrong_instruction = actual.clone();
    wrong_instruction.instruction += 1;
    let mut wrong_revision = actual.clone();
    wrong_revision.revision += 1;
    let mut wrong_payer = actual.clone();
    wrong_payer.payer = B;
    let (foreign, _) = spell_optional(Instruction::StackOp(Box::new(
        crate::toolbox::StackOpSpec::counter_targeted(
            crate::toolbox::CounterDestination::Graveyard,
        ),
    )));
    let mut wrong_entry = actual.clone();
    wrong_entry.entry = optional_frame(&foreign).entry();
    assert_ne!(wrong_entry.entry, actual.entry);
    for frame in [wrong_instruction, wrong_revision, wrong_payer, wrong_entry] {
        let saved = state.clone();
        assert_eq!(
            state.execution().resume_rule(
                state.execution().revision(),
                &EffectDecision::ResolutionPayment(frame),
                A,
                DecisionReply::ResolutionPayment(ResolutionPaymentReply::Decline),
                None
            ),
            Err(EffectError::WrongResolutionPaymentFrame)
        );
        assert_eq!(state, saved);
    }
    let color = tap(&state, 12).into_state();
    let saved = color.clone();
    reject(
        &color,
        A,
        GameCommand::Reply {
            decision: state.pending_decision().unwrap().id(),
            choice: DecisionReply::ResolutionPayment(ResolutionPaymentReply::Decline),
        },
        GameError::WrongDecision,
    );
    assert_eq!(
        color.execution().resume_rule(
            color.execution().revision(),
            &EffectDecision::ResolutionPayment(actual.clone()),
            A,
            DecisionReply::ResolutionPayment(ResolutionPaymentReply::Decline),
            None
        ),
        Err(EffectError::WrongResolutionPaymentFrame)
    );
    let frame = color.execution().pending_mana_color().unwrap().clone();
    let mut other_source = frame.clone();
    other_source.source = object(&color, 11);
    let mut other_revision = frame.clone();
    other_revision.revision += 1;
    let mut other_actor = frame.clone();
    other_actor.chooser = B;
    for frame in [other_source, other_revision, other_actor] {
        assert_eq!(
            color.execution().resume_mana_color(
                color.execution().revision(),
                &frame,
                A,
                ManaType::Blue
            ),
            Err(EffectError::WrongManaColorFrame)
        );
        assert_eq!(color, saved);
    }
    let restored = reply(&color, DecisionReply::ManaColor(ManaType::Blue));
    assert_old_payment_rejected(
        restored.state(),
        &actual,
        state.pending_decision().unwrap().id(),
    );
    assert!(
        reply(
            restored.state(),
            DecisionReply::ResolutionPayment(ResolutionPaymentReply::Decline)
        )
        .state()
        .pending_decision()
        .is_none()
    );
}

#[test]
fn direct_calendar_and_scheduler_work_cannot_interrupt_payment_or_color_child() {
    let payment = tap(&pending(offer(3)), 10).into_state();
    let color = tap(&payment, 12).into_state();
    for state in [&payment, &color] {
        let execution = state.execution();
        let saved = state.clone();
        assert_eq!(
            execution.enter_turn_step(execution.revision(), B, true, true),
            Err(EffectError::WrongPhase)
        );
        assert_eq!(
            execution.enter_turn_step(execution.revision(), A, false, false),
            Err(EffectError::WrongPhase)
        );
        assert_eq!(
            execution.record_turn_start(execution.revision()),
            Err(EffectError::WrongPhase)
        );
        assert_eq!(
            execution.untap_turn(execution.revision()),
            Err(EffectError::WrongPhase)
        );
        assert_eq!(state, &saved);
    }
    let restored = reply(&color, DecisionReply::ManaColor(ManaType::Blue));
    let result = reply(
        restored.state(),
        DecisionReply::ResolutionPayment(ResolutionPaymentReply::Decline),
    );
    assert_eq!(result.state().execution().mana(A, ManaType::Blue), 2.into());
    assert_eq!(result.state().execution().life_total(A), Some(&22.into()));
}

#[test]
fn root_and_decision_exhaustion_during_color_preserve_the_already_paid_tap_prefix() {
    let state = tap(&tap(&pending(offer(3)), 10).into_state(), 12).into_state();
    let mut revision = state.clone();
    revision.set_test_input_revision(u64::MAX);
    reject(
        &revision,
        A,
        command_reply(&revision, DecisionReply::ManaColor(ManaType::Blue)),
        GameError::RevisionExhausted,
    );
    let mut ids = state.clone();
    ids.set_test_next_decision(u64::MAX);
    reject(
        &ids,
        A,
        command_reply(&ids, DecisionReply::ManaColor(ManaType::Blue)),
        GameError::DecisionIdentityExhausted,
    );
    let mut execution = state.execution().clone();
    execution.revision = u64::MAX;
    let frame = execution.pending_mana_color().unwrap().clone();
    let saved = execution.clone();
    assert_eq!(
        execution.resume_mana_color(u64::MAX, &frame, A, ManaType::Blue),
        Err(EffectError::Window(StackWindowError::RevisionExhausted))
    );
    assert_eq!(execution, saved);
    let restored = reply(&state, DecisionReply::ManaColor(ManaType::Blue));
    assert_eq!(
        restored.state().execution().mana(A, ManaType::Blue),
        2.into()
    );
    let result = reply(
        restored.state(),
        DecisionReply::ResolutionPayment(ResolutionPaymentReply::Decline),
    );
    assert_eq!(result.state().execution().mana(A, ManaType::Blue), 2.into());
}

#[test]
fn optional_type_literal_zero_draw_continuation_assigns_only_on_take() {
    for choice in [OptionalEffectChoice::Take, OptionalEffectChoice::Decline] {
        let state = pending(vec![
            Instruction::OptionalSequence {
                chooser: ResolvingPlayer::Controller,
                instructions: 1,
            },
            Instruction::PromptChoice(Box::new(crate::toolbox::PromptChoiceSpec {
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
            })),
            crate::program::Instruction::DrawCards(Box::new(
                crate::toolbox::DrawCardsSpec::simple(
                    crate::toolbox::PlayerSelector::Resolving(
                        crate::program::ResolvingPlayer::Controller,
                    ),
                    crate::program::ValueExpr::Literal(0.into()),
                ),
            )),
        ]);
        let result = take_or_decline(&state, choice);
        if choice == OptionalEffectChoice::Decline {
            assert!(result.state().pending_decision().is_none());
            assert!(
                !occurrences(&result)
                    .iter()
                    .any(|row| matches!(row, RulesOccurrence::CreatureTypeChosen { .. }))
            );
            assert!(result.draw_journal().events.is_empty());
        } else {
            assert!(matches!(
                result.state().pending_decision().unwrap().frame(),
                RuleFrame::PromptChoice(_)
            ));
            let human = result
                .state()
                .execution()
                .creature_vocabulary()
                .resolve_label("Human")
                .unwrap();
            let completed = reply(
                result.state(),
                DecisionReply::PromptChoice(crate::toolbox::PromptChoiceReply::CreatureType(human)),
            );
            assert!(completed.state().pending_decision().is_none());
            assert_eq!(occurrences(&completed).iter().filter(|row|matches!(row,RulesOccurrence::CreatureTypeChosen{slot,value,..} if *slot==CreatureTypeSlot::new(0) && *value==human)).count(),1);
            assert!(completed.draw_journal().events.is_empty());
            assert_eq!(outcomes(&completed), 0);
        }
    }
}

#[test]
fn root_basis_game_epoch_and_revision_are_checked_before_current_payment_authority() {
    let state = tap(&pending(offer(3)), 10).into_state();
    for error in [
        GameError::WrongGame,
        GameError::WrongEpoch,
        GameError::StaleRevision,
    ] {
        let mut basis = state.basis().clone();
        match error {
            GameError::WrongGame => basis.game = GameId(9900),
            GameError::WrongEpoch => basis.epoch += 1,
            GameError::StaleRevision => basis.revision -= 1,
            _ => unreachable!(),
        }
        let saved = state.clone();
        assert_eq!(
            state.advance(GameInput {
                basis,
                actor: A,
                command: command_reply(
                    &state,
                    DecisionReply::ResolutionPayment(ResolutionPaymentReply::Decline)
                )
            }),
            Err(error)
        );
        assert_eq!(state, saved);
        assert_eq!(state.execution().mana(A, ManaType::Blue), 1.into());
    }
    assert!(
        reply(
            &state,
            DecisionReply::ResolutionPayment(ResolutionPaymentReply::Decline)
        )
        .state()
        .pending_decision()
        .is_none()
    );
}

#[test]
fn returned_mana_source_incarnation_rejects_old_identity_before_current_source_succeeds() {
    let state = pending(offer(3));
    let old = object(&state, 10);
    // Explicit DP02 preparation through the existing native complete-world
    // move owner. The current game root is never relabeled as an ordinary run.
    let mut execution = state.execution().clone();
    let parent = execution.pending_work.take().unwrap();
    let (execution, left, first_journal) = execution
        .qualification_zone_moves(execution.revision(), &[(old, ObjectDestination::Hand)])
        .unwrap()
        .into_parts();
    let hand = left[0].after.id();
    let (mut execution, returned, second_journal) = execution
        .qualification_zone_moves(
            execution.revision(),
            &[(hand, ObjectDestination::Battlefield { controller: A })],
        )
        .unwrap()
        .into_parts();
    assert!(!first_journal.records.is_empty());
    assert!(!second_journal.records.is_empty());
    let fresh = returned[0].after.id();
    assert_ne!(fresh, old);
    execution.pending_work = Some(parent);
    let saved = execution.clone();
    assert_eq!(
        execution.begin_mana_activation(execution.revision(), A, old, ABILITY),
        Err(EffectError::UnknownObject)
    );
    assert_eq!(execution, saved);
    let (funded, production, _) = execution
        .begin_mana_activation(execution.revision(), A, fresh, ABILITY)
        .unwrap()
        .into_parts();
    assert!(matches!(production, ManaActivationResult::Complete { .. }));
    assert_eq!(funded.mana(A, ManaType::Blue), 1.into());
    let frame = funded.pending_resolution_decision().unwrap();
    let (completed, _, _) = funded
        .resume_rule(
            funded.revision(),
            &frame,
            A,
            DecisionReply::ResolutionPayment(ResolutionPaymentReply::Decline),
            None,
        )
        .unwrap()
        .into_parts();
    assert_eq!(completed.mana(A, ManaType::Blue), 1.into());
    assert_eq!(
        completed.window().cards().get(fresh).unwrap().tapped(),
        Some(true)
    );
    assert_eq!(completed.life_total(A), Some(&22.into()));
}

#[test]
fn same_original_typed_inputs_reexecute_every_payment_parent_child_journal_and_whole_state() {
    let body = vec![
        Instruction::PromptChoice(Box::new(
            crate::toolbox::PromptChoiceSpec::offer_dynamic_mana_payment(
                ResolvingPlayer::Controller,
                ManaCost::generic(3.into()),
                crate::program::ValueExpr::Literal(0.into()),
                1,
                4,
            ),
        )),
        Instruction::OptionalSequence {
            chooser: ResolvingPlayer::Controller,
            instructions: 1,
        },
        gain(1),
        gain(2),
    ];
    let initial = initialize(body.clone());
    let mut journals = vec![initial.rules_journal().clone()];
    let mut states = vec![initial.state().clone()];
    let mut inputs = Vec::new();
    let mut state = initial.into_state();
    let spell = object(&state, 1);
    {
        let mut accept = |actor: SeatId, command: GameCommand| {
            let input = GameInput {
                basis: state.basis().clone(),
                actor,
                command,
            };
            let result = state.advance(input.clone()).unwrap();
            inputs.push(input);
            journals.push(result.rules_journal().clone());
            states.push(result.state().clone());
            state = result.into_state();
        };
        accept(
            A,
            GameCommand::CastUntargeted {
                spell,
                payment: None,
            },
        );
        accept(A, GameCommand::Pass);
        accept(B, GameCommand::Pass);
    }
    // Subsequent commands bind the actual reached decision identity. The saved
    // complete inputs are then replayed verbatim, without rebinding any field.
    let commands = [10, 12];
    for id in commands {
        let input = GameInput {
            basis: state.basis().clone(),
            actor: A,
            command: GameCommand::ActivateMana {
                source: object(&state, id),
                ability: ABILITY,
            },
        };
        let result = state.advance(input.clone()).unwrap();
        inputs.push(input);
        journals.push(result.rules_journal().clone());
        states.push(result.state().clone());
        state = result.into_state();
    }
    assert!(matches!(
        state.pending_decision().unwrap().frame(),
        RuleFrame::ManaColor(_)
    ));
    {
        let command = command_reply(&state, DecisionReply::ManaColor(ManaType::Blue));
        let input = GameInput {
            basis: state.basis().clone(),
            actor: A,
            command,
        };
        let result = state.advance(input.clone()).unwrap();
        inputs.push(input);
        journals.push(result.rules_journal().clone());
        states.push(result.state().clone());
        state = result.into_state();
    }
    let input = GameInput {
        basis: state.basis().clone(),
        actor: A,
        command: GameCommand::ActivateMana {
            source: object(&state, 11),
            ability: ABILITY,
        },
    };
    let result = state.advance(input.clone()).unwrap();
    inputs.push(input);
    journals.push(result.rules_journal().clone());
    states.push(result.state().clone());
    state = result.into_state();
    for choice in [
        DecisionReply::ResolutionPayment(ResolutionPaymentReply::Pay(ManaPayment {
            from_lots: Default::default(),
            spend: vec![ManaSpend {
                mana_type: ManaType::Blue,
                amount: 3.into(),
            }],
        })),
        DecisionReply::OptionalEffect(OptionalEffectChoice::Take),
    ] {
        let input = GameInput {
            basis: state.basis().clone(),
            actor: A,
            command: command_reply(&state, choice),
        };
        let result = state.advance(input.clone()).unwrap();
        inputs.push(input);
        journals.push(result.rules_journal().clone());
        states.push(result.state().clone());
        state = result.into_state();
    }
    assert_eq!(state.execution().life_total(A), Some(&23.into()));
    assert!(state.pending_decision().is_none());
    assert_eq!(inputs.len(), 9);
    assert_eq!(journals.len(), 10);
    assert_eq!(states.len(), 10);
    let initial = initialize(body);
    assert_eq!(initial.rules_journal(), &journals[0]);
    assert_eq!(initial.state(), &states[0]);
    let mut replay = initial.into_state();
    for (index, input) in inputs.iter().enumerate() {
        assert_eq!(&replay, &states[index]);
        let result = replay.advance(input.clone()).unwrap();
        assert_eq!(
            result.rules_journal(),
            &journals[index + 1],
            "complete accepted journal {index}"
        );
        assert_eq!(
            result.state(),
            &states[index + 1],
            "complete accepted state {index}"
        );
        replay = result.into_state();
    }
    assert_eq!(replay, state);
    let payment_prefix = &states[4];
    let mut tampered = inputs[4].clone();
    tampered.actor = B;
    assert_eq!(
        payment_prefix.advance(tampered),
        Err(GameError::WrongChooser)
    );
    assert_eq!(payment_prefix, &states[4]);
}

#[test]
fn nonempty_resolution_locals_survive_each_mana_prefix_and_both_optional_outcomes() {
    let state = pending(vec![
        Instruction::PromptChoice(Box::new(crate::toolbox::PromptChoiceSpec {
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
        })),
        Instruction::PromptChoice(Box::new(
            crate::toolbox::PromptChoiceSpec::offer_dynamic_mana_payment(
                ResolvingPlayer::Controller,
                ManaCost::generic(3.into()),
                crate::program::ValueExpr::Literal(0.into()),
                2,
                5,
            ),
        )),
        Instruction::OptionalSequence {
            chooser: ResolvingPlayer::Controller,
            instructions: 1,
        },
        gain(1),
        gain(2),
    ]);
    assert!(matches!(
        state.pending_decision().unwrap().frame(),
        RuleFrame::PromptChoice(_)
    ));
    let human = state
        .execution()
        .creature_vocabulary()
        .resolve_label("Human")
        .unwrap();
    let expected = BTreeMap::from([(CreatureTypeSlot::new(0), human)]);
    let state = reply(
        &state,
        DecisionReply::PromptChoice(crate::toolbox::PromptChoiceReply::CreatureType(human)),
    )
    .into_state();
    let entry = payment_frame(&state).entry();
    let assert_parent = |state: &GameState, suspended| {
        let Some(ResolutionChoiceState::Payment {
            cursor,
            suspended_for_mana_color,
            ..
        }) = state.execution().resolution_choice_state()
        else {
            panic!("owned payment parent")
        };
        assert_eq!(cursor.entry(), entry);
        assert_eq!(cursor.instruction(), 1);
        assert_eq!(cursor.locals(), &expected);
        assert_eq!(suspended_for_mana_color, suspended);
    };
    assert_parent(&state, false);
    let fixed = tap(&state, 10).into_state();
    assert_parent(&fixed, false);
    let color = tap(&fixed, 12).into_state();
    assert_parent(&color, true);
    let restored = reply(&color, DecisionReply::ManaColor(ManaType::Blue)).into_state();
    assert_parent(&restored, false);
    let funded = tap(&restored, 11).into_state();
    assert_parent(&funded, false);
    let paid = reply(
        &funded,
        DecisionReply::ResolutionPayment(ResolutionPaymentReply::Pay(ManaPayment {
            from_lots: Default::default(),
            spend: vec![ManaSpend {
                mana_type: ManaType::Blue,
                amount: 3.into(),
            }],
        })),
    )
    .into_state();
    let Some(ResolutionChoiceState::Optional { cursor, .. }) =
        paid.execution().resolution_choice_state()
    else {
        panic!("owned optional cursor")
    };
    assert_eq!(cursor.entry(), entry);
    assert_eq!(cursor.instruction(), 2);
    assert_eq!(cursor.locals(), &expected);
    for (choice, life) in [
        (OptionalEffectChoice::Take, 23),
        (OptionalEffectChoice::Decline, 22),
    ] {
        let result = take_or_decline(&paid, choice);
        assert_eq!(result.state().execution().life_total(A), Some(&life.into()));
        assert!(result.state().pending_decision().is_none());
        assert!(
            !occurrences(&result)
                .iter()
                .any(|row| matches!(row, RulesOccurrence::CreatureTypeChosen { .. }))
        );
    }
}

#[test]
fn zero_count_prefix_rejects_the_complete_original_old_basis_input_before_valid_decline() {
    let state = pending(offer(3));
    let old_frame = payment_frame(&state);
    let old_id = state.pending_decision().unwrap().id();
    let original_input = GameInput {
        basis: state.basis().clone(),
        actor: A,
        command: GameCommand::Reply {
            decision: old_id,
            choice: DecisionReply::ResolutionPayment(ResolutionPaymentReply::Decline),
        },
    };
    let source = object(&state, 13);
    let activated = tap(&state, 13);
    assert!(activated.mana_production().unwrap().produced.is_empty());
    assert_eq!(
        activated.state().execution().mana(A, ManaType::Blue),
        0.into()
    );
    assert_eq!(
        activated
            .state()
            .execution()
            .window()
            .cards()
            .get(source)
            .unwrap()
            .tapped(),
        Some(true)
    );
    assert_ne!(activated.state().pending_decision().unwrap().id(), old_id);
    assert_ne!(activated.state().basis(), &original_input.basis);
    assert_eq!(payment_frame(activated.state()).entry(), old_frame.entry());
    assert_eq!(
        payment_frame(activated.state()).instruction(),
        old_frame.instruction()
    );
    assert_eq!(payment_frame(activated.state()).cost(), old_frame.cost());
    assert!(
        !occurrences(&activated)
            .iter()
            .any(|row| matches!(row, RulesOccurrence::ManaProduced { .. }))
    );

    // This complete input retains BOTH its original basis and DecisionId. The
    // separate current-basis/old-DecisionId test remains unchanged.
    let latest = activated.state().clone();
    let accepted_prefix_journal = activated.rules_journal().clone();
    assert_eq!(
        activated.state().advance(original_input),
        Err(GameError::StaleRevision)
    );
    assert_eq!(activated.state(), &latest);
    assert_eq!(activated.rules_journal(), &accepted_prefix_journal);

    let declined = reply(
        activated.state(),
        DecisionReply::ResolutionPayment(ResolutionPaymentReply::Decline),
    );
    assert!(declined.state().pending_decision().is_none());
    assert_eq!(
        declined.state().execution().mana(A, ManaType::Blue),
        0.into()
    );
    assert_eq!(declined.state().execution().life_total(A), Some(&22.into()));
    assert_eq!(
        declined
            .state()
            .execution()
            .window()
            .cards()
            .get(source)
            .unwrap()
            .tapped(),
        Some(true)
    );
    assert_eq!(
        declined
            .rules_journal()
            .records
            .iter()
            .filter(|row| matches!(
                row,
                RulesRecord::ResolutionPaymentOutcome {
                    outcome: ResolutionPaymentOutcome::Declined,
                    ..
                }
            ))
            .count(),
        1
    );
}

#[test]
fn actual_by_player_group_retains_two_entry_causes_for_distinct_payer_and_optional_controller() {
    use crate::occurrence::{TriggerCauseKey, TriggerRelated};

    let vocabulary = creature_types::selected_vocabulary().unwrap();
    let definition = TokenDefinition::new(
        creature(),
        TokenName::Specified(
            spellwake_contracts::names::EnglishCardName::new("Grouped Native Token".into())
                .unwrap(),
        ),
        &vocabulary,
    )
    .unwrap();
    let mut creation = TokenCreation::one(definition);
    creation.amount = crate::program::ValueExpr::Literal(2.into());
    let mut watcher = Program::artifact(None, vec![]);
    watcher.triggered.push(TriggeredAbility {
        id: TriggeredAbilityId(1),
        condition: crate::program::TriggerSpec::enters(crate::program::ObjectFilter::creature()),
        grouping: TriggerGrouping::OneOrMoreByPlayer,
        body: ExecutableBody::new(
            BodyTarget::None,
            vec![
                Instruction::PromptChoice(Box::new(
                    crate::toolbox::PromptChoiceSpec::offer_dynamic_mana_payment(
                        ResolvingPlayer::UniqueRelatedPlayer,
                        ManaCost::generic(1.into()),
                        crate::program::ValueExpr::Literal(0.into()),
                        1,
                        3,
                    ),
                )),
                Instruction::OptionalSequence {
                    chooser: ResolvingPlayer::Controller,
                    instructions: 1,
                },
                gain(1),
            ],
        ),
    });
    let mut mana = Program::artifact(None, vec![]);
    mana.mana.push(ManaAbility {
        id: ManaAbilityId(1),
        output: ManaOutput::Fixed(BTreeMap::from([(ManaType::Blue, 1.into())])),
    });
    let state = world(
        vec![C, A, B],
        C,
        vec![
            hand(
                1,
                A,
                BodyTarget::None,
                vec![Instruction::CreateTokens {
                    creations: vec![creation],
                }],
            ),
            seed(
                10,
                A,
                ObjectDestination::Battlefield { controller: A },
                mana,
            ),
            seed(
                20,
                B,
                ObjectDestination::Battlefield { controller: B },
                watcher,
            ),
        ],
    );
    let watcher = object(&state, 20);
    let state = candidate(&state, C, GameCommand::Pass).into_state();
    let casted = cast(&state, A, 1, None);
    let timestamp = resolve_top(casted.state());
    let RuleFrame::TimestampOrder(frame) = timestamp.state().pending_decision().unwrap().frame()
    else {
        panic!("real simultaneous token timestamp choice")
    };
    assert_eq!(frame.chooser(), A);
    assert_eq!(frame.members().len(), 2);
    assert_eq!(timestamp.state().execution().waiting_triggers().count(), 0);
    let order = frame
        .members()
        .iter()
        .rev()
        .map(|member| member.member)
        .collect();
    let created = reply(timestamp.state(), DecisionReply::TimestampOrder(order));

    let creation_events = created
        .rules_journal()
        .records
        .iter()
        .filter_map(|row| match row {
            RulesRecord::Event(event)
                if event
                    .occurrences
                    .iter()
                    .any(|row| matches!(row.value, RulesOccurrence::TokenCreated { .. })) =>
            {
                Some(event)
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(creation_events.len(), 1);
    let event = creation_events[0];
    let published = event
        .occurrences
        .iter()
        .filter_map(|row| match &row.value {
            RulesOccurrence::TokenCreated { creator, after } => {
                assert_eq!(*creator, A);
                assert_eq!(after.object.controller(), Some(A));
                assert_eq!(after.object.zone(), ObjectZone::Battlefield);
                assert!(after.is_creature());
                Some((row.id, after.as_ref()))
            }
            _ => None,
        })
        .collect::<BTreeMap<_, _>>();
    assert_eq!(published.len(), 2);
    assert_eq!(
        published
            .values()
            .map(|after| after.object.id())
            .collect::<BTreeSet<_>>()
            .len(),
        2
    );
    let discoveries = created
        .rules_journal()
        .records
        .iter()
        .filter_map(|row| match row {
            RulesRecord::TriggerDiscovered(instance) => Some(instance.as_ref()),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(discoveries.len(), 1);
    let capture = (*discoveries[0]).clone();
    assert_eq!(capture.source.object.id(), watcher);
    assert_eq!(capture.controller, B);
    assert_eq!(capture.causes.key(), TriggerCauseKey::Player(A));
    assert_eq!(capture.causes.event(), event.id);
    assert_eq!(capture.causes.members().len(), 2);
    let published_order = event
        .occurrences
        .iter()
        .filter(|row| matches!(row.value, RulesOccurrence::TokenCreated { .. }))
        .map(|row| row.id)
        .collect::<Vec<_>>();
    assert_eq!(
        capture
            .causes
            .members()
            .iter()
            .map(|member| member.occurrence)
            .collect::<Vec<_>>(),
        published_order
    );
    for member in capture.causes.members() {
        assert_eq!(member.related_player, Some(A));
        assert_eq!(member.occurrence.event, event.id);
        let TriggerRelated::TokenCreated { creator, after } = &member.related else {
            panic!("actual token-entry cause")
        };
        assert_eq!(*creator, A);
        assert_eq!(after, published[&member.occurrence]);
    }
    assert_eq!(created.rules_journal().records.iter().filter(|row| matches!(row, RulesRecord::TriggerPlaced(placement) if placement.instance == capture.id && placement.controller == B)).count(), 1);
    assert_eq!(created.state().execution().waiting_triggers().count(), 0);
    let entries = created.state().execution().window().entries();
    assert_eq!(entries.len(), 1);
    assert!(
        matches!(entries[0].kind(), StackKind::TriggeredAbility { source } if source.id() == watcher)
    );
    assert_eq!(entries[0].controller(), B);
    let trigger_entry = entries[0].id();
    let binding = created
        .state()
        .execution()
        .execution_bindings(trigger_entry)
        .unwrap()
        .trigger
        .as_ref()
        .unwrap();
    assert_eq!(binding.instance, capture.id);
    assert_eq!(binding.causes, capture.causes);

    let queued_payload = projection::stack_payloads(created.state().execution())
        .pop()
        .unwrap();
    let offered = resolve_top(created.state());
    assert_eq!(
        projection::resolving_payload(offered.state().execution()),
        Some(queued_payload)
    );
    assert_eq!(offered.state().execution().window().active_player(), C);
    assert_eq!(payment_frame(offered.state()).entry(), trigger_entry);
    assert_eq!(payment_frame(offered.state()).payer(), A);
    assert_eq!(
        payment_frame(offered.state()).cost(),
        &ManaCost::generic(1.into())
    );
    assert_eq!(
        offered
            .rules_journal()
            .records
            .iter()
            .filter(|row| matches!(row, RulesRecord::ResolutionPaymentOutcome { .. }))
            .count(),
        0
    );
    for actor in [B, C] {
        reject(
            offered.state(),
            actor,
            command_reply(
                offered.state(),
                DecisionReply::ResolutionPayment(ResolutionPaymentReply::Decline),
            ),
            GameError::WrongChooser,
        );
        reject(
            offered.state(),
            actor,
            GameCommand::ActivateMana {
                source: object(offered.state(), 10),
                ability: ABILITY,
            },
            GameError::WrongChooser,
        );
    }
    let funded = tap(offered.state(), 10);
    assert_eq!(funded.state().execution().mana(A, ManaType::Blue), 1.into());
    assert_eq!(payment_frame(funded.state()).payer(), A);
    let paid = reply(
        funded.state(),
        DecisionReply::ResolutionPayment(ResolutionPaymentReply::Pay(ManaPayment {
            from_lots: Default::default(),
            spend: vec![ManaSpend {
                mana_type: ManaType::Blue,
                amount: 1.into(),
            }],
        })),
    );
    assert_eq!(paid.state().execution().mana(A, ManaType::Blue), 0.into());
    assert_eq!(paid.rules_journal().records.iter().filter(|row| matches!(row,
        RulesRecord::ResolutionPaymentOutcome { payer, outcome: ResolutionPaymentOutcome::Paid(payment), next, .. }
            if *payer == A && payment.source == trigger_entry && payment.payer == A && payment.cost == ManaCost::generic(1.into()) && payment.spend == vec![ManaSpend { mana_type: ManaType::Blue, amount: 1.into() }] && *next == 1)).count(), 1);
    assert_eq!(optional_frame(paid.state()).chooser(), B);
    assert_eq!(optional_frame(paid.state()).entry(), trigger_entry);
    assert_eq!(paid.state().execution().life_total(B), Some(&20.into()));
    for actor in [A, C] {
        reject(
            paid.state(),
            actor,
            command_reply(
                paid.state(),
                DecisionReply::OptionalEffect(OptionalEffectChoice::Take),
            ),
            GameError::WrongChooser,
        );
    }
    let taken = reply(
        paid.state(),
        DecisionReply::OptionalEffect(OptionalEffectChoice::Take),
    );
    assert_eq!(outcomes(&taken), 1);
    assert_eq!(taken.state().execution().life_total(B), Some(&21.into()));
    assert_eq!(taken.state().execution().life_total(A), Some(&20.into()));
    assert_eq!(taken.state().execution().life_total(C), Some(&20.into()));
    assert!(taken.state().pending_decision().is_none());
    assert!(taken.state().execution().window().entries().is_empty());
}

#[path = "native_mechanisms/applicability_adapter_tests.rs"]
mod applicability_adapter_tests;
