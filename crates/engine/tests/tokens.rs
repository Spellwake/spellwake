#![allow(dead_code, unused_imports)]
//! Literal-token mechanisms through the existing authoritative native root.
//! Prepared seeds are explicit; these cases do not qualify complete cards.
#[path = "../test_support/creature_vocabulary.rs"]
mod creature_vocabulary;
#[path = "../test_support/receipt_joins.rs"]
mod receipt_joins;
#[path = "../test_support/characteristic_trajectories.rs"]
mod trajectories;
use spellwake_engine::{
    activated::{ActivatedAbility, ActivatedAbilityId, SourceCost},
    characteristics::{
        Color, CreatureTypeTextChangeSpec, CreatureTypeWord, NumericExpression, NumericPair,
        PermanentSelector, PowerToughnessCounters, ResolvedDuration, ResolvedPartSpec,
        ResolvedPartsSpec, StaticAbilityId, StaticContinuousAbility, StaticPart, StaticParts,
    },
    checks::CheckDecision,
    combat::{AttackDeclaration, BlockDeclaration, CombatDamageAssignment, CombatDecision},
    damage::{DamageKind, DamageRecipient, DamageSource},
    game::{DecisionReply, GameCommand, GameInput, GameState, NativeCandidate, RuleFrame},
    land::{BasicLandType, LandType},
    life::LifeAdjustment,
    mana::{
        ManaAbility, ManaAbilityId, ManaAbilityRef, ManaCost, ManaOutput, ManaPayment, ManaType,
    },
    object_identity::{CardId, ObjectId, ObjectOrigin, ObjectZone},
    occurrence::{
        EventCause, RulesOccurrence, RulesRecord, StateBasedMoveReason, SupportedAppearance,
        TriggerRelated, ZoneReason,
    },
    priority::{PriorityPhase, SeatId},
    program::{
        BodyTarget, CardType, ExecutableBody, Instruction, PlayerRelation, Program, Supertype,
        TriggerSpec, TriggeredAbility, TriggeredAbilityId,
    },
    stack_window::{StackEntryId, WindowPhase},
    targets::TargetRef,
    token::{TokenDefinition, TokenName, TokenSubtype},
    turn::Step,
};
use std::collections::BTreeSet;
use trajectories::*;

/// Traverse real priority/calendar work, answering only empty attack declarations.
/// This is a test driver, not a second scheduler or a prepared turn-start stamp.
fn advance_to(game: &GameState, number: u64, step: Step) -> GameState {
    let mut current = game.clone();
    for _ in 0..64 {
        let position = current.turn().unwrap().position();
        if position.number == number && position.step == step {
            return current;
        }
        current = if let Some(decision) = current.pending_decision() {
            assert!(matches!(
                decision.frame(),
                RuleFrame::Combat(CombatDecision::Attackers(_))
            ));
            reply(
                &current,
                decision.chooser(),
                DecisionReply::DeclareAttackers(vec![]),
            )
            .into_state()
        } else {
            let WindowPhase::Priority(PriorityPhase::Ready { holder }) =
                current.execution().window().phase()
            else {
                panic!("expected actual priority while traversing calendar")
            };
            submit(&current, holder, GameCommand::Pass).into_state()
        };
    }
    panic!("actual scheduler did not reach requested boundary")
}

#[test]
fn token_tap_mana_and_attacking_use_actual_control_history_and_generic_choice_source() {
    let vocabulary = vocabulary();
    let template = token(1, 1);
    let mut program = template.program().clone();
    program.mana.push(ManaAbility {
        id: ManaAbilityId(7),
        output: ManaOutput::ChooseColor {
            amount: 2.into(),
            colors: BTreeSet::from([ManaType::Blue, ManaType::Green]),
        },
    });
    let definition = TokenDefinition::new(program, template.name().clone(), &vocabulary).unwrap();
    let game = world(
        vocabulary.clone(),
        vec![
            producer(1, definition, false),
            library(10, P0, &vocabulary),
            library(11, P1, &vocabulary),
            card(
                20,
                Program::creature(None, 1.into(), 1.into(), BTreeSet::new()),
                true,
            ),
        ],
        vec![CardId(20)],
    );
    let cast = cast_untargeted(&game, 1);
    let born = resolve_top(cast.state());
    let source = births(&born)[0].3.object.id();
    let activation = GameCommand::ActivateMana {
        source,
        ability: ManaAbilityRef::Printed(ManaAbilityId(7)),
    };
    let fresh = born.state();
    let saved = fresh.clone();
    assert_eq!(
        fresh.advance(GameInput {
            basis: fresh.basis().clone(),
            actor: P0,
            command: activation.clone()
        }),
        Err(spellwake_engine::game::GameError::Effect(
            spellwake_engine::effects::EffectError::CreatureTapRestricted
        ))
    );
    assert_eq!(fresh, &saved);
    let declaring = advance_to(fresh, 1, Step::DeclareAttackers);
    let pending = declaring.pending_decision().unwrap();
    let saved = declaring.clone();
    assert_eq!(
        declaring.advance(GameInput {
            basis: declaring.basis().clone(),
            actor: P0,
            command: GameCommand::Reply {
                decision: pending.id(),
                choice: DecisionReply::DeclareAttackers(vec![AttackDeclaration {
                    attacker: source,
                    defender: P1,
                    defending_planeswalker: None,
                }])
            },
        }),
        Err(spellwake_engine::game::GameError::Effect(
            spellwake_engine::effects::EffectError::Combat(
                spellwake_engine::combat::CombatError::IllegalAttacker
            )
        ))
    );
    assert_eq!(declaring, saved);
    let after_turn_start = advance_to(&declaring, 3, Step::Upkeep);
    let registry = after_turn_start.execution().window().cards();
    assert_eq!(
        registry
            .get(source)
            .unwrap()
            .has_continuous_control_since_turn_start(),
        Some(true)
    );
    // Two independent branches of the same accepted root: attack eligibility,
    // and choosing mana before taking a tap cost for some other action.
    let ready = advance_to(&after_turn_start, 3, Step::DeclareAttackers);
    let attacking = reply(
        &ready,
        P0,
        DecisionReply::DeclareAttackers(vec![AttackDeclaration {
            attacker: source,
            defender: P1,
            defending_planeswalker: None,
        }]),
    );
    assert_eq!(
        attacking
            .state()
            .execution()
            .window()
            .cards()
            .get(source)
            .unwrap()
            .tapped(),
        Some(true)
    );
    let pending = submit(&after_turn_start, P0, activation);
    let RuleFrame::ManaColor(frame) = pending.state().pending_decision().unwrap().frame() else {
        panic!("actual color choice")
    };
    assert_eq!(frame.source(), source);
    assert_eq!(
        frame.colors(),
        &BTreeSet::from([ManaType::Blue, ManaType::Green])
    );
    assert_eq!(
        pending
            .state()
            .execution()
            .window()
            .cards()
            .get(source)
            .unwrap()
            .card_id(),
        None
    );
    assert_eq!(
        pending
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
        pending.state().execution().mana(P0, ManaType::Green),
        0.into()
    );
    let saved = pending.state().clone();
    let decision = saved.pending_decision().unwrap();
    assert_eq!(
        saved.advance(GameInput {
            basis: saved.basis().clone(),
            actor: P0,
            command: GameCommand::Reply {
                decision: decision.id(),
                choice: DecisionReply::ManaColor(ManaType::Red)
            },
        }),
        Err(spellwake_engine::game::GameError::Effect(
            spellwake_engine::effects::EffectError::IllegalManaColor
        ))
    );
    assert_eq!(pending.state(), &saved);
    let credited = reply(
        pending.state(),
        P0,
        DecisionReply::ManaColor(ManaType::Green),
    );
    assert_eq!(
        credited.state().execution().mana(P0, ManaType::Green),
        2.into()
    );
    assert_eq!(
        credited.state().execution().mana(P0, ManaType::Blue),
        0.into()
    );
}

#[test]
fn newly_created_token_blocks_and_deals_combat_damage_without_physical_card_provenance() {
    // CR 111.6, 302.6, 509.1, 510.1 and 704.5: recent control prevents
    // attacking, not blocking; combat damage and deaths use the shared root.
    let mut opponent_producer = producer(1, token(1, 1), false);
    opponent_producer.owner = P1;
    let game = world(
        vocabulary(),
        vec![
            opponent_producer,
            card(
                20,
                Program::creature(None, 1.into(), 1.into(), BTreeSet::new()),
                true,
            ),
        ],
        vec![CardId(20)],
    );
    let passed = submit(&game, P0, GameCommand::Pass);
    let cast = submit(
        passed.state(),
        P1,
        GameCommand::CastUntargeted {
            spell: object(passed.state(), 1),
            payment: None,
        },
    );
    let born = resolve_top(cast.state());
    let blocker = births(&born)[0].3.object.clone();
    assert_eq!(blocker.controller(), Some(P1));
    assert_eq!(
        blocker.has_continuous_control_since_turn_start(),
        Some(false)
    );
    assert_eq!(blocker.card_id(), None);
    let beginning = pass_pair(born.state());
    let declaring = pass_pair(beginning.state());
    assert_eq!(
        declaring.state().turn().unwrap().position().step,
        Step::DeclareAttackers
    );
    let attacker = object(declaring.state(), 20);
    let attacking = reply(
        declaring.state(),
        P0,
        DecisionReply::DeclareAttackers(vec![AttackDeclaration {
            attacker,
            defender: P1,
            defending_planeswalker: None,
        }]),
    );
    let blocking = pass_pair(attacking.state());
    let blocked = reply(
        blocking.state(),
        P1,
        DecisionReply::DeclareBlockers(vec![BlockDeclaration {
            blocker: blocker.id(),
            attacker,
        }]),
    );
    let assigning = pass_pair(blocked.state());
    // Single-recipient damage can be forced by the kernel; any actual player
    // assignment is still answered at its authoritative frame.
    let mut current = assigning;
    let mut attempts = current.damage_journal().attempts.clone();
    for _ in 0..2 {
        let Some(decision) = current.state().pending_decision() else {
            break;
        };
        let RuleFrame::Combat(CombatDecision::DamageAssignments(frame)) = decision.frame() else {
            panic!("expected only an actual combat damage assignment")
        };
        let assignments = frame
            .domains()
            .iter()
            .map(|domain| {
                assert_eq!(domain.recipients().len(), 1);
                CombatDamageAssignment {
                    source: domain.source(),
                    target: domain.recipients()[0],
                    amount: domain.power().clone(),
                }
            })
            .collect();
        current = reply(
            current.state(),
            decision.chooser(),
            DecisionReply::AssignCombatDamage(assignments),
        );
        attempts.extend(current.damage_journal().attempts.clone());
    }
    assert!(current.state().pending_decision().is_none());
    assert_eq!(attempts.len(), 2);
    assert!(attempts.iter().all(|attempt| attempt.amount == 1.into()));
    let token_damage = attempts.iter().find(|attempt| matches!(&attempt.source,
        DamageSource::Object { object, controller } if object.id() == blocker.id() && *controller == P1
    )).unwrap();
    assert_eq!(token_damage.target, DamageRecipient::Creature(attacker));
    let DamageSource::Object {
        object: captured, ..
    } = &token_damage.source
    else {
        unreachable!()
    };
    assert_eq!(captured, &blocker);
    assert!(
        current
            .damage_journal()
            .batches
            .iter()
            .all(|batch| batch.kind == DamageKind::Combat)
    );
    let registry = current.state().execution().window().cards();
    assert!(registry.iter().all(|object| !object.is_token()));
    assert_eq!(
        registry.get(object(current.state(), 20)).unwrap().zone(),
        ObjectZone::Graveyard(P0)
    );
    assert_eq!(current.state().execution().life_total(P0), Some(&20.into()));
    assert_eq!(current.state().execution().life_total(P1), Some(&20.into()));
}

#[test]
fn token_receipt_joins_reject_ambiguous_appends_and_do_not_invent_missing_births() {
    let game = world(
        vocabulary(),
        vec![
            producer(1, token(1, 1), false),
            card(
                20,
                watcher(spellwake_engine::program::TriggerSpec::player_action(
                    PlayerRelation::Controller,
                    spellwake_engine::program::PlayerTriggerAction::TokenCreated,
                )),
                true,
            ),
        ],
        vec![CardId(20)],
    );
    let cast = cast_untargeted(&game, 1);
    let born = resolve_top(cast.state());
    let (entry, instruction, _, after) = &births(&born)[0];
    let append = born.rules_journal();
    assert_eq!(
        receipt_joins::token_creation(append, *entry, *instruction).unwrap(),
        Some(after.object.id())
    );
    assert_eq!(
        receipt_joins::token_creation(append, *entry, instruction + 1).unwrap(),
        None
    );
    let source = object(born.state(), 20);
    let instance = receipt_joins::birth_discovery(append, source, 7, after.object.id())
        .unwrap()
        .unwrap();
    assert_eq!(
        receipt_joins::birth_discovery(append, source, 8, after.object.id()).unwrap(),
        None
    );
    assert_eq!(
        receipt_joins::birth_discovery(append, source, 7, source).unwrap(),
        None
    );
    let mut duplicated = append.clone();
    duplicated.records.extend(append.records.clone());
    assert!(receipt_joins::token_creation(&duplicated, *entry, *instruction).is_err());
    assert!(receipt_joins::birth_discovery(&duplicated, source, 7, after.object.id()).is_err());
    let empty = spellwake_engine::occurrence::RulesJournal::default();
    assert_eq!(
        receipt_joins::token_creation(&empty, *entry, *instruction).unwrap(),
        None
    );
    assert_eq!(
        receipt_joins::birth_discovery(&empty, source, 7, after.object.id()).unwrap(),
        None
    );
    assert!(append.records.iter().any(|record| matches!(record,
        RulesRecord::TriggerDiscovered(waiting) if waiting.id == instance
    )));
}

fn token(power: i64, toughness: i64) -> TokenDefinition {
    let v = vocabulary();
    let subtype = v.resolve_label("Saproling").unwrap();
    let mut program = Program::creature(
        None,
        power.into(),
        toughness.into(),
        BTreeSet::from([subtype]),
    );
    program.effect_defined_colors = Some(BTreeSet::from([Color::Green]));
    TokenDefinition::new(
        program,
        TokenName::Default {
            ordered_subtypes: vec![TokenSubtype::Creature(subtype)],
        },
        &v,
    )
    .unwrap()
}
fn create(definition: TokenDefinition) -> Instruction {
    Instruction::CreateTokens {
        creations: vec![spellwake_engine::token::TokenCreation::one(definition)],
    }
}
fn producer(
    id: u64,
    definition: TokenDefinition,
    paid: bool,
) -> spellwake_engine::effects::SeedCard {
    card(
        id,
        Program::instant(
            if paid {
                ManaCost::specific(ManaType::Green, 1.into())
            } else {
                ManaCost::free()
            },
            ExecutableBody::new(BodyTarget::None, vec![create(definition)]),
        ),
        false,
    )
}
fn cast_untargeted(game: &GameState, id: u64) -> NativeCandidate {
    submit(
        game,
        P0,
        GameCommand::CastUntargeted {
            spell: object(game, id),
            payment: None,
        },
    )
}
fn resolve_top(game: &GameState) -> NativeCandidate {
    let WindowPhase::Priority(PriorityPhase::Ready { holder }) = game.execution().window().phase()
    else {
        panic!("priority")
    };
    pass_in_order(game, [holder, if holder == P0 { P1 } else { P0 }])
}
fn births(candidate: &NativeCandidate) -> Vec<(StackEntryId, usize, SeatId, SupportedAppearance)> {
    candidate
        .rules_journal()
        .records
        .iter()
        .filter_map(|record| match record {
            RulesRecord::Event(event) => Some(event),
            _ => None,
        })
        .flat_map(|event| {
            event.occurrences.iter().filter_map(|occurrence| {
                match (&event.cause, &occurrence.value) {
                    (
                        EventCause::Instruction { entry, instruction },
                        RulesOccurrence::TokenCreated { creator, after },
                    ) => Some((*entry, *instruction, *creator, after.as_ref().clone())),
                    _ => None,
                }
            })
        })
        .collect()
}
fn watcher(condition: TriggerSpec) -> Program {
    let mut program = Program::artifact(None, vec![]);
    program.triggered.push(TriggeredAbility {
        grouping: spellwake_engine::program::TriggerGrouping::EachOccurrence,
        id: TriggeredAbilityId(7),
        condition,
        body: ExecutableBody::new(
            BodyTarget::None,
            vec![spellwake_engine::program::Instruction::AdjustLife(
                Box::new(spellwake_engine::toolbox::AdjustLifeSpec::gain(
                    spellwake_engine::toolbox::PlayerSelector::Resolving(
                        spellwake_engine::program::ResolvingPlayer::Controller,
                    ),
                    spellwake_engine::program::ValueExpr::Literal(1.into()),
                )),
            )],
        ),
    });
    program
}
#[test]
fn paid_creation_has_an_honest_birth_and_uses_the_shared_permanent_evaluator() {
    let game = world(
        vocabulary(),
        vec![
            producer(1, token(1, 1), true),
            card(
                10,
                Program::land(BTreeSet::from([LandType::Forest]), vec![]),
                true,
            ),
        ],
        vec![CardId(10)],
    );
    let tapped = submit(
        &game,
        P0,
        GameCommand::ActivateMana {
            source: object(&game, 10),
            ability: ManaAbilityRef::BasicLand(BasicLandType::Forest),
        },
    );
    let cast = cast_untargeted(tapped.state(), 1);
    assert_eq!(
        cast.mana_payment().unwrap().cost,
        ManaCost::specific(ManaType::Green, 1.into())
    );
    let entry = cast.state().execution().window().entries()[0].id();
    let next_stack = cast.state().execution().window().next_entry_sequence();
    let completed = resolve_top(cast.state());
    let rows = births(&completed);
    assert_eq!(rows.len(), 1);
    assert_eq!(
        receipt_joins::token_creation_at(completed.rules_journal(), entry, 0, 0).unwrap(),
        Some(rows[0].3.object.id())
    );
    assert!(receipt_joins::token_creation_at(completed.rules_journal(), entry, 0, 1).is_err());
    let (source, index, creator, appearance) = &rows[0];
    assert_eq!((*source, *index, *creator), (entry, 0, P0));
    let execution = completed.state().execution();
    let object = execution
        .window()
        .cards()
        .get(appearance.object.id())
        .unwrap();
    assert!(matches!(object.origin(), ObjectOrigin::Token { .. }));
    assert_eq!(object.card_id(), None);
    assert_eq!(object.owner(), P0);
    assert_eq!(object.controller(), Some(P0));
    assert_eq!(object.tapped(), Some(false));
    assert_eq!(
        object.has_continuous_control_since_turn_start(),
        Some(false)
    );
    let baseline = execution.base_program(object.id()).unwrap();
    assert_eq!(
        baseline.names.iter().next().unwrap().as_str(),
        "Saproling Token"
    );
    assert_eq!(baseline.color_indicator, None);
    assert_eq!(baseline.permanent().unwrap().casting_cost, None);
    assert_eq!(
        baseline.effect_defined_colors,
        Some(BTreeSet::from([Color::Green]))
    );
    let current = current(completed.state(), object.id());
    assert_eq!(current.power_toughness.unwrap().derived, pt(1, 1));
    assert_eq!(current.current_colors, BTreeSet::from([Color::Green]));
    assert_eq!(current.abilities.source, object.id());
    assert_eq!(current.abilities.card, None);
    assert_eq!(execution.programs().count(), 2);
    assert_eq!(execution.window().next_entry_sequence(), next_stack);
    assert!(completed.mana_payment().is_none());
    assert!(!all_occurrences(&completed).iter().any(|value| matches!(
        value,
        RulesOccurrence::CastCompleted(_) | RulesOccurrence::ManaDebited { .. }
    )));
}
#[test]
fn an_owned_spell_copy_creates_a_distinct_token_for_its_own_controller() {
    let mut copier = card(
        2,
        Program::instant(
            ManaCost::specific(ManaType::Blue, 2.into()),
            ExecutableBody::new(
                BodyTarget::SingleSpell(spellwake_engine::program::SpellFilter::any()),
                vec![Instruction::CopySpell],
            ),
        ),
        false,
    );
    copier.owner = P1;
    let game = world(
        vocabulary(),
        vec![
            producer(1, token(1, 1), true),
            copier,
            controlled(
                10,
                P0,
                P0,
                Program::land(BTreeSet::from([LandType::Forest]), vec![]),
            ),
            controlled(
                11,
                P1,
                P1,
                Program::land(BTreeSet::from([LandType::Island]), vec![]),
            ),
            controlled(
                12,
                P1,
                P1,
                Program::land(BTreeSet::from([LandType::Island]), vec![]),
            ),
        ],
        vec![CardId(10), CardId(11), CardId(12)],
    );
    let tapped = submit(
        &game,
        P0,
        GameCommand::ActivateMana {
            source: object(&game, 10),
            ability: ManaAbilityRef::BasicLand(BasicLandType::Forest),
        },
    );
    let cast = cast_untargeted(tapped.state(), 1);
    let original = cast.state().execution().window().entries()[0].id();
    let passed = submit(cast.state(), P0, GameCommand::Pass);
    let first = submit(
        passed.state(),
        P1,
        GameCommand::ActivateMana {
            source: object(passed.state(), 11),
            ability: ManaAbilityRef::BasicLand(BasicLandType::Island),
        },
    );
    let second = submit(
        first.state(),
        P1,
        GameCommand::ActivateMana {
            source: object(first.state(), 12),
            ability: ManaAbilityRef::BasicLand(BasicLandType::Island),
        },
    );
    let cast = submit(
        second.state(),
        P1,
        GameCommand::Cast {
            spell: object(second.state(), 2),
            target: TargetRef::Stack(original),
        },
    );
    assert_eq!(
        cast.mana_payment().unwrap().cost,
        ManaCost::specific(ManaType::Blue, 2.into())
    );
    let copied = resolve_top(cast.state());
    let copy = all_occurrences(&copied)
        .into_iter()
        .find_map(|value| match value {
            RulesOccurrence::CopyCreated { entry, .. } => Some(*entry),
            _ => None,
        })
        .unwrap();
    assert!(copied.state().pending_decision().is_none());
    let resolved_copy = resolve_top(copied.state());
    let copy_birth = births(&resolved_copy);
    assert_eq!(copy_birth.len(), 1);
    assert_eq!((copy_birth[0].0, copy_birth[0].2), (copy, P1));
    let resolved_original = resolve_top(resolved_copy.state());
    let original_birth = births(&resolved_original);
    assert_eq!(original_birth.len(), 1);
    assert_eq!((original_birth[0].0, original_birth[0].2), (original, P0));
    assert_ne!(copy_birth[0].3.object.id(), original_birth[0].3.object.id());
    for appearance in [&copy_birth[0].3, &original_birth[0].3] {
        assert!(appearance.object.is_token());
        assert_eq!(
            appearance.object.owner(),
            appearance.object.controller().unwrap()
        );
        assert_eq!(appearance.object.card_id(), None);
    }
    assert_eq!(
        resolved_original
            .state()
            .execution()
            .window()
            .cards()
            .iter()
            .filter(|object| object.is_token())
            .count(),
        2
    );
}
#[test]
fn creation_and_enters_conditions_have_distinct_honest_birth_related_captures() {
    for condition in [
        spellwake_engine::program::TriggerSpec::enters(
            spellwake_engine::program::ObjectFilter::creature(),
        ),
        spellwake_engine::program::TriggerSpec::player_action(
            PlayerRelation::Controller,
            spellwake_engine::program::PlayerTriggerAction::TokenCreated,
        ),
    ] {
        let game = world(
            vocabulary(),
            vec![
                producer(1, token(1, 1), false),
                card(3, watcher(condition), true),
            ],
            vec![CardId(3)],
        );
        let cast = cast_untargeted(&game, 1);
        let completed = resolve_top(cast.state());
        let births = births(&completed);
        assert_eq!(births.len(), 1);
        let discovered = completed
            .rules_journal()
            .records
            .iter()
            .filter_map(|record| match record {
                RulesRecord::TriggerDiscovered(value) => Some(value),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(discovered.len(), 1);
        let TriggerRelated::TokenCreated { creator, after } =
            &discovered[0].causes.members()[0].related
        else {
            panic!("birth relation")
        };
        assert_eq!(*creator, P0);
        assert_eq!(after.object.id(), births[0].3.object.id());
        assert!(after.object.is_token());
        let triggered = resolve_top(completed.state());
        assert_eq!(
            triggered.state().execution().life_total(P0),
            Some(&21.into())
        );
    }
}
#[test]
fn a_zero_toughness_birth_dies_then_ceases_before_its_owned_trigger_resolves() {
    let game = world(
        vocabulary(),
        vec![
            producer(1, token(1, 1), false),
            card(2, shrink_all(), true),
            card(
                3,
                watcher(spellwake_engine::program::TriggerSpec::dies(
                    spellwake_engine::program::ObjectFilter::creature(),
                )),
                true,
            ),
        ],
        vec![CardId(2), CardId(3)],
    );
    let cast = cast_untargeted(&game, 1);
    let completed = resolve_top(cast.state());
    let appearance = &births(&completed)[0].3;
    let born = appearance.object.id();
    assert_eq!(
        appearance.copiable_power_toughness,
        token(1, 1).program().permanent().unwrap().printed_pt
    );
    let entry_pt = appearance.current_power_toughness.as_ref().unwrap();
    assert_eq!(
        (&entry_pt.power, &entry_pt.toughness),
        (&0.into(), &0.into())
    );
    let death = all_occurrences(&completed)
        .into_iter()
        .find_map(|value| match value {
            RulesOccurrence::ZoneChange(change) if change.before.object.id() == born => {
                Some(change)
            }
            _ => None,
        })
        .unwrap();
    let departed = death.after.object.id();
    assert!(
        matches!(death.reason, ZoneReason::StateBased(reasons) if reasons.contains(StateBasedMoveReason::ZeroToughness))
    );
    assert_ne!(born, departed);
    assert_eq!(death.after.object.zone(), ObjectZone::Graveyard(P0));
    let ceased = all_occurrences(&completed)
        .into_iter()
        .find_map(|value| match value {
            RulesOccurrence::TokenCeased { object } => Some(object),
            _ => None,
        })
        .unwrap();
    assert_eq!(ceased.id(), departed);
    assert!(
        completed
            .state()
            .execution()
            .window()
            .cards()
            .get(departed)
            .is_none()
    );
    assert_eq!(
        completed
            .state()
            .execution()
            .window()
            .cards()
            .graveyard(P0)
            .len(),
        1
    );
    let discovered = completed
        .rules_journal()
        .records
        .iter()
        .find_map(|record| match record {
            RulesRecord::TriggerDiscovered(value) => Some(value),
            _ => None,
        })
        .unwrap();
    assert_eq!(
        completed
            .rules_journal()
            .records
            .iter()
            .filter(|record| matches!(record, RulesRecord::TriggerDiscovered(_)))
            .count(),
        1
    );
    let TriggerRelated::ZoneChange { before, after } = &discovered.causes.members()[0].related
    else {
        panic!("death relation")
    };
    assert_eq!((before.object.id(), after.object.id()), (born, departed));
    assert!(before.object.is_token());
    let resolved = resolve_top(completed.state());
    assert_eq!(
        resolved.state().execution().life_total(P0),
        Some(&21.into())
    );
}
fn shrink_all() -> Program {
    let mut program = Program::enchantment(Some(ManaCost::free()));
    program.continuous = Some(StaticContinuousAbility::Battlefield {
        id: StaticAbilityId(5),
        selector: PermanentSelector {
            required_types: BTreeSet::from([CardType::Creature]),
            ..PermanentSelector::default()
        },
        parts: StaticParts(vec![StaticPart::ModifyPT(NumericPair {
            power: NumericExpression::Literal((-1).into()),
            toughness: NumericExpression::Literal((-1).into()),
        })]),
    });
    program
}
#[test]
fn mixed_deaths_move_every_object_but_only_two_physical_cards_request_ordering() {
    for card_count in 0..=2 {
        let mut cards = vec![
            producer(1, token(1, 1), false),
            card(2, shrink_all(), false),
        ];
        let mut order = vec![];
        for i in 0..card_count {
            cards.push(card(
                20 + i,
                Program::creature(None, 1.into(), 1.into(), BTreeSet::new()),
                true,
            ));
            order.push(CardId(20 + i));
        }
        let game = world(vocabulary(), cards, order);
        let cast = cast_untargeted(&game, 1);
        let born = resolve_top(cast.state());
        let token_id = births(&born)[0].3.object.id();
        let expected_cards = (0..card_count)
            .map(|i| object(born.state(), 20 + i))
            .collect::<Vec<_>>();
        let cast = cast_untargeted(born.state(), 2);
        let result = resolve_top(cast.state());
        let completed = if card_count == 2 {
            let RuleFrame::Checks(CheckDecision::GraveyardOrder(frame)) =
                result.state().pending_decision().unwrap().frame()
            else {
                panic!("physical-card ordering")
            };
            assert_eq!(
                frame.domain().iter().copied().collect::<BTreeSet<_>>(),
                expected_cards.iter().copied().collect()
            );
            assert!(!frame.domain().contains(&token_id));
            let decision = result.state().pending_decision().unwrap();
            let before = result.state().clone();
            assert!(
                result
                    .state()
                    .advance(GameInput {
                        basis: result.state().basis().clone(),
                        actor: P0,
                        command: GameCommand::Reply {
                            decision: decision.id(),
                            choice: DecisionReply::OrderGraveyard(vec![
                                token_id,
                                expected_cards[0]
                            ])
                        }
                    })
                    .is_err()
            );
            assert_eq!(result.state(), &before);
            reply(
                result.state(),
                P0,
                DecisionReply::OrderGraveyard(vec![expected_cards[1], expected_cards[0]]),
            )
        } else {
            assert!(result.state().pending_decision().is_none());
            result
        };
        assert_eq!(
            completed.check_journal().moves.len(),
            usize::try_from(card_count + 1).unwrap()
        );
        assert_eq!(
            all_occurrences(&completed)
                .iter()
                .filter(|value| matches!(value, RulesOccurrence::TokenCeased { .. }))
                .count(),
            1
        );
        let registry = completed.state().execution().window().cards();
        assert!(registry.iter().all(|object| !object.is_token()));
        assert_eq!(
            registry.graveyard(P0).len(),
            usize::try_from(card_count + 1).unwrap()
        );
        if card_count == 2 {
            assert_eq!(
                registry
                    .graveyard(P0)
                    .iter()
                    .map(|id| registry.get(*id).unwrap().card_id().unwrap())
                    .collect::<Vec<_>>(),
                vec![CardId(21), CardId(20), CardId(1)]
            );
        }
    }
}

fn activate_exact(game: &GameState, source: ObjectId, target: ObjectId) -> NativeCandidate {
    submit(
        game,
        P0,
        GameCommand::ActivateAbility {
            source,
            ability: ActivatedAbilityId(1),
            target: TargetRef::Object(target),
            payment: Some(ManaPayment::default()),
        },
    )
}
fn copy_ability() -> ActivatedAbility {
    ActivatedAbility {
        id: ActivatedAbilityId(1),
        mana_cost: ManaCost::free(),
        source_cost: SourceCost::None,
        body: ExecutableBody::new(
            BodyTarget::SinglePermanent,
            vec![Instruction::CopyTargetToSource {
                duration: ResolvedDuration::UntilEndOfTurn,
                power_toughness: None,
            }],
        ),
    }
}
#[test]
fn permanent_copying_preserves_origin_and_owns_token_values_independently() {
    let v = vocabulary();
    let template = token(1, 1);
    let mut program = template.program().clone();
    program.activated.push(copy_ability());
    program.effect_defined_colors = Some(BTreeSet::from([Color::Blue]));
    let template = TokenDefinition::new(program, template.name().clone(), &v).unwrap();
    let mut donor = Program::creature(None, 3.into(), 3.into(), BTreeSet::new());
    donor.effect_defined_colors = Some(BTreeSet::from([Color::Green]));
    donor.names = spellwake_contracts::names::ObjectNames::single(
        spellwake_contracts::names::EnglishCardName::new("Donor".into()).unwrap(),
    );
    let game = world(
        v,
        vec![
            producer(1, template, false),
            card(20, Program::artifact(None, vec![copy_ability()]), true),
            card(21, donor, true),
        ],
        vec![CardId(20), CardId(21)],
    );
    let cast = cast_untargeted(&game, 1);
    let born = resolve_top(cast.state());
    let token_id = births(&born)[0].3.object.id();
    let receiver = object(born.state(), 20);
    let admitted = activate_exact(born.state(), receiver, token_id);
    let copied = resolve_top(admitted.state());
    assert_eq!(
        current(copied.state(), receiver).current_colors,
        BTreeSet::from([Color::Blue])
    );
    assert_eq!(
        current(copied.state(), receiver)
            .power_toughness
            .unwrap()
            .derived,
        pt(1, 1)
    );
    assert_eq!(
        copied
            .state()
            .execution()
            .window()
            .cards()
            .get(receiver)
            .unwrap()
            .card_id(),
        Some(CardId(20))
    );
    let admitted = activate_exact(copied.state(), token_id, object(copied.state(), 21));
    let changed = resolve_top(admitted.state());
    let execution = changed.state().execution();
    assert!(execution.window().cards().get(token_id).unwrap().is_token());
    assert_eq!(
        execution.window().cards().get(token_id).unwrap().card_id(),
        None
    );
    assert_eq!(
        current(changed.state(), token_id)
            .power_toughness
            .unwrap()
            .derived,
        pt(3, 3)
    );
    assert_eq!(
        execution
            .copiable_program(token_id)
            .unwrap()
            .names
            .iter()
            .next()
            .unwrap()
            .as_str(),
        "Donor"
    );
    assert_eq!(
        execution
            .base_program(token_id)
            .unwrap()
            .names
            .iter()
            .next()
            .unwrap()
            .as_str(),
        "Saproling Token"
    );
    assert_eq!(
        current(changed.state(), receiver)
            .power_toughness
            .unwrap()
            .derived,
        pt(1, 1)
    );
    assert_eq!(
        execution
            .copiable_program(receiver)
            .unwrap()
            .names
            .iter()
            .next()
            .unwrap()
            .as_str(),
        "Saproling Token"
    );
    assert_eq!(
        execution
            .copiable_program(receiver)
            .unwrap()
            .color_indicator,
        None
    );
    assert_eq!(
        execution
            .copiable_program(receiver)
            .unwrap()
            .effect_defined_colors,
        Some(BTreeSet::from([Color::Blue]))
    );
}

#[test]
fn two_sequential_singular_creations_die_together_and_cease_on_the_next_observation() {
    // These are two separate creation instructions, not a simultaneous token batch.
    let game = world(
        vocabulary(),
        vec![spell(
            1,
            BodyTarget::None,
            vec![create(token(0, 0)), create(token(0, 0))],
        )],
        vec![],
    );
    let cast = cast_untargeted(&game, 1);
    let completed = resolve_top(cast.state());
    let births = births(&completed);
    assert_eq!(births.len(), 2);
    assert_ne!(births[0].3.object.id(), births[1].3.object.id());
    assert!(completed.state().pending_decision().is_none());
    assert_eq!(completed.check_journal().moves.len(), 2);
    let cohorts=completed.rules_journal().records.iter().filter_map(|record|match record {
        RulesRecord::Event(event) if matches!(event.cause,EventCause::StateBasedActions {..})=>Some((event.occurrences.iter().filter(|row|matches!(&row.value,RulesOccurrence::ZoneChange(change) if change.before.object.is_token())).count(),event.occurrences.iter().filter(|row|matches!(row.value,RulesOccurrence::TokenCeased {..})).count())),_=>None,
    }).filter(|(moves,ceased)|moves+ceased>0).collect::<Vec<_>>();
    assert_eq!(cohorts, vec![(2, 0), (0, 2)]);
    let registry = completed.state().execution().window().cards();
    assert_eq!(registry.graveyard(P0).len(), 1);
    assert!(registry.iter().all(|object| !object.is_token()));
}
#[test]
fn legend_choices_include_both_origins_and_departures_use_the_objects_real_owner() {
    for keep_token in [false, true] {
        let v = vocabulary();
        let template = token(1, 1);
        let mut program = template.program().clone();
        program.supertypes.insert(Supertype::Legendary);
        let definition = TokenDefinition::new(program, template.name().clone(), &v).unwrap();
        let mut physical = Program::creature(None, 2.into(), 2.into(), BTreeSet::new());
        physical.supertypes.insert(Supertype::Legendary);
        physical.names = spellwake_contracts::names::ObjectNames::single(
            spellwake_contracts::names::EnglishCardName::new("Saproling Token".into()).unwrap(),
        );
        let game = world(
            v,
            vec![
                producer(1, definition, false),
                controlled(20, P1, P0, physical),
            ],
            vec![CardId(20)],
        );
        let cast = cast_untargeted(&game, 1);
        let pending = resolve_top(cast.state());
        let token_id = births(&pending)[0].3.object.id();
        let card_id = object(pending.state(), 20);
        let RuleFrame::Checks(CheckDecision::Legend(frame)) =
            pending.state().pending_decision().unwrap().frame()
        else {
            panic!("legend choice")
        };
        assert_eq!(frame.chooser(), P0);
        let [conflict] = frame.conflicts() else {
            panic!("one conflict")
        };
        assert_eq!(
            conflict.domain().iter().copied().collect::<BTreeSet<_>>(),
            BTreeSet::from([card_id, token_id])
        );
        let completed = reply(
            pending.state(),
            P0,
            DecisionReply::KeepLegend {
                conflict: conflict.id(),
                keep: if keep_token { token_id } else { card_id },
            },
        );
        let registry = completed.state().execution().window().cards();
        if keep_token {
            assert!(registry.get(token_id).unwrap().is_token());
            assert_eq!(registry.graveyard(P1).len(), 1);
            assert_eq!(
                registry.get(registry.graveyard(P1)[0]).unwrap().card_id(),
                Some(CardId(20))
            );
        } else {
            assert!(registry.get(token_id).is_none());
            assert!(registry.graveyard(P1).is_empty());
            assert!(registry.get(card_id).is_some());
        }
        assert!(all_occurrences(&completed).iter().any(|value|matches!(value,RulesOccurrence::ZoneChange(change) if matches!(change.reason,ZoneReason::StateBased(reasons) if reasons.contains(StateBasedMoveReason::Legend)))));
    }
}
#[test]
fn a_later_physical_world_eliminates_and_ceases_the_older_world_token() {
    let v = vocabulary();
    let mut program = Program::artifact(None, vec![]);
    program.supertypes.insert(Supertype::World);
    let token = TokenDefinition::new(
        program,
        TokenName::Default {
            ordered_subtypes: vec![],
        },
        &v,
    )
    .unwrap();
    let mut world_card = Program::enchantment(Some(ManaCost::free()));
    world_card.supertypes.insert(Supertype::World);
    let game = world(
        v,
        vec![producer(1, token, false), card(2, world_card, false)],
        vec![],
    );
    let cast = cast_untargeted(&game, 1);
    let first = resolve_top(cast.state());
    let token_id = births(&first)[0].3.object.id();
    assert!(
        first
            .state()
            .execution()
            .window()
            .cards()
            .get(token_id)
            .is_some()
    );
    let cast = cast_untargeted(first.state(), 2);
    let completed = resolve_top(cast.state());
    let registry = completed.state().execution().window().cards();
    assert!(registry.get(token_id).is_none());
    assert_eq!(
        registry.get(object(completed.state(), 2)).unwrap().zone(),
        ObjectZone::Battlefield
    );
    assert!(all_occurrences(&completed).iter().any(|value|matches!(value,RulesOccurrence::ZoneChange(change) if change.before.object.id()==token_id && matches!(change.reason,ZoneReason::StateBased(reasons) if reasons.contains(StateBasedMoveReason::World)))));
    assert_eq!(
        all_occurrences(&completed)
            .iter()
            .filter(|value| matches!(value, RulesOccurrence::TokenCeased { .. }))
            .count(),
        1
    );
}

#[test]
fn actual_plural_birth_selectors_require_event_uniqueness_and_explicit_ordinal() {
    let game = world(
        vocabulary(),
        vec![card(
            1,
            Program::instant(
                ManaCost::free(),
                ExecutableBody::new(
                    BodyTarget::None,
                    vec![Instruction::CreateTokens {
                        creations: vec![spellwake_engine::token::TokenCreation {
                            definition: token(1, 1),
                            amount: spellwake_engine::program::ValueExpr::Literal(2.into()),
                            base_size: None,
                            enters_tapped: false,
                            recipients: spellwake_engine::token::TokenRecipients::Controller,
                        }],
                    }],
                ),
            ),
            false,
        )],
        vec![],
    );
    let cast = cast_untargeted(&game, 1);
    let entry = cast.state().execution().window().entries()[0].id();
    let pending = resolve_top(cast.state());
    let RuleFrame::TimestampOrder(frame) = pending.state().pending_decision().unwrap().frame()
    else {
        panic!()
    };
    let completed = reply(
        pending.state(),
        P0,
        DecisionReply::TimestampOrder(frame.members().iter().map(|m| m.member).collect()),
    );
    let append = completed.rules_journal();
    let ids = receipt_joins::token_creations(append, entry, 0)
        .unwrap()
        .unwrap();
    assert_eq!(ids.len(), 2);
    assert_eq!(
        receipt_joins::token_creation_at(append, entry, 0, 1).unwrap(),
        Some(ids[1])
    );
    assert!(receipt_joins::token_creation(append, entry, 0).is_err());
    assert!(receipt_joins::token_creation_at(append, entry, 0, 2).is_err());
    assert_eq!(
        receipt_joins::token_creation_at(&Default::default(), entry, 0, 0).unwrap(),
        None
    );
    // Synthetic adapter corruption of a real append, not an accepted game event.
    let matching=append.records.iter().find(|r|matches!(r,RulesRecord::Event(e) if matches!(e.cause,EventCause::Instruction{entry:actual,instruction:0} if actual==entry))).unwrap().clone();
    let mut duplicate = append.clone();
    duplicate.records.push(matching.clone());
    assert!(receipt_joins::token_creations(&duplicate, entry, 0).is_err());
    let RulesRecord::Event(mut event) = matching else {
        panic!()
    };
    let birth = event
        .occurrences
        .iter()
        .find(|o| matches!(o.value, RulesOccurrence::TokenCreated { .. }))
        .unwrap()
        .clone();
    event.occurrences.push(birth);
    assert!(
        receipt_joins::token_creations(
            &spellwake_engine::occurrence::RulesJournal {
                records: vec![RulesRecord::Event(event)]
            },
            entry,
            0
        )
        .is_err()
    );
}
#[test]
fn shared_birth_selector_codec_rejects_missing_noncanonical_and_wrong_wire_shapes() {
    use receipt_joins::TokenCreationSelector;
    use serde_json::json;
    let valid = json!({"token_created_by_entry_ref":"producer","instruction":"0","birth_ordinal":"1","field":"after_object","require_unique":true});
    let selected: TokenCreationSelector = serde_json::from_value(valid.clone()).unwrap();
    assert_eq!((selected.instruction, selected.birth_ordinal), (0, 1));
    for (key, value) in [
        ("birth_ordinal", json!(1)),
        ("birth_ordinal", json!("01")),
        ("birth_ordinal", json!("-1")),
        ("birth_ordinal", json!("9999999999999999999999999999")),
        ("instruction", json!("+0")),
        ("require_unique", json!(false)),
        ("field", json!("before_object")),
        ("extra", json!(true)),
    ] {
        let mut bad = valid.clone();
        bad[key] = value;
        assert!(serde_json::from_value::<TokenCreationSelector>(bad).is_err());
    }
    let mut missing = valid.clone();
    missing.as_object_mut().unwrap().remove("birth_ordinal");
    assert!(serde_json::from_value::<TokenCreationSelector>(missing).is_err());
    assert!(
        serde_json::from_value::<TokenCreationSelector>(json!([
            "producer",
            "0",
            "1",
            "after_object",
            true
        ]))
        .is_err()
    );
}
