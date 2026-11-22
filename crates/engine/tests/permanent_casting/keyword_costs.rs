use super::*;
use spellwake_engine::{
    characteristics::{ControllerSelector, PermanentSelector},
    program::{CardType, CastReduction},
};

fn affinity(kind: CardType) -> CastReduction {
    CastReduction::Affinity(Box::new(PermanentSelector {
        controller: ControllerSelector::SourceController,
        required_types: BTreeSet::from([kind]),
        ..Default::default()
    }))
}

#[test]
fn affinity_and_undaunted_stack_and_preserve_colored_costs() {
    let mut cards = seeds();
    let mut cost = ManaCost::one(ManaType::Blue);
    cost.generic = 6.into();
    let mut body = artifact(50, A, Some(cost));
    body.program.cast_reductions = vec![
        affinity(CardType::Artifact),
        affinity(CardType::Artifact),
        CastReduction::Undaunted,
    ];
    cards.push(body);
    for (id, controller) in [(51, A), (52, A), (53, B)] {
        let mut fixture = artifact(id, controller, None);
        fixture.destination = ObjectDestination::Battlefield { controller };
        cards.push(fixture);
    }
    let game = setup(Some(Step::PrecombatMain), cards);
    let request = untargeted(
        &game,
        50,
        Some(payment(&[(ManaType::Blue, 1), (ManaType::Red, 1)])),
    );
    let cast = accept(&game, A, request);
    let receipt = cast.mana_payment().unwrap();
    assert_eq!(receipt.cost.generic, 1.into());
    assert_eq!(
        receipt.cost.required,
        BTreeMap::from([(ManaType::Blue, 1.into())])
    );
    assert_eq!(
        physical(resolve(cast.state()).state(), 50).zone(),
        ObjectZone::Battlefield
    );
}

#[test]
fn flashback_uses_its_own_cost_and_exiles_on_resolution_or_counter() {
    let mut cards = seeds();
    let card = cards.iter_mut().find(|c| c.card == CardId(10)).unwrap();
    card.program.flashback = Some(ManaCost::specific(ManaType::Green, 2.into()));
    for counter in [false, true] {
        let game = setup(Some(Step::PrecombatMain), cards.clone());
        let target = TargetRef::Object(object(&game, 90));
        let first = targeted(&game, A, 10, target);
        assert!(!first.state().execution().window().entries()[0].cast_with_flashback());
        let first_done = resolve(first.state()).into_state();
        assert_eq!(physical(&first_done, 10).zone(), ObjectZone::Graveyard(A));
        let second = targeted(&first_done, A, 10, target);
        let entry = &second.state().execution().window().entries()[0];
        assert!(entry.cast_with_flashback());
        assert_eq!(
            second.mana_payment().unwrap().cost,
            ManaCost::specific(ManaType::Green, 2.into())
        );
        let final_state = if counter {
            let priority = pass(second.state()).into_state();
            let reply = targeted(&priority, B, 11, TargetRef::Stack(entry.id()));
            resolve(reply.state()).into_state()
        } else {
            resolve(second.state()).into_state()
        };
        assert_eq!(physical(&final_state, 10).zone(), ObjectZone::Exile);
        reject(
            &final_state,
            A,
            GameCommand::Cast {
                spell: object(&final_state, 10),
                target,
            },
            EffectError::NotOwnHand,
        );
    }
}

#[test]
fn flashback_sorcery_still_needs_main_phase_and_a_legal_target() {
    let mut cards = seeds();
    let card = cards.iter_mut().find(|c| c.card == CardId(14)).unwrap();
    card.destination = ObjectDestination::Graveyard;
    card.program.flashback = Some(ManaCost::free());
    let game = setup(Some(Step::Upkeep), cards.clone());
    let request = GameCommand::Cast {
        spell: object(&game, 14),
        target: TargetRef::Object(object(&game, 90)),
    };
    reject(
        &game,
        A,
        request,
        EffectError::Timing(TimingError::MainPhaseRequired),
    );
    let main = setup(Some(Step::PrecombatMain), cards);
    reject(
        &main,
        A,
        GameCommand::Cast {
            spell: object(&main, 14),
            target: TargetRef::Player(B),
        },
        EffectError::IllegalTarget,
    );
}

#[test]
fn convoke_improvise_and_delve_pay_actual_symbols_atomically_without_spending_mana() {
    use spellwake_engine::{
        characteristics::{Color, KeywordAbility},
        mana::{CastResource, CastingPayment},
    };
    let mut cards = seeds();
    let mut cost = ManaCost::one(ManaType::Green);
    cost.generic = 2.into();
    let mut spell = artifact(50, A, Some(cost));
    spell.program.printed_keywords.extend([
        KeywordAbility::Convoke,
        KeywordAbility::Improvise,
        KeywordAbility::Delve,
    ]);
    cards.push(spell);
    let mut caster = creature(51, A, ObjectDestination::Hand, Some(ManaCost::free()), 3);
    caster.program.color_indicator = Some(BTreeSet::from([Color::Green]));
    cards.push(caster);
    let mut rock = artifact(52, A, None);
    rock.destination = ObjectDestination::Battlefield { controller: A };
    cards.push(rock);
    for (id, who) in [(53, A), (54, B)] {
        let mut dead = artifact(id, who, None);
        dead.destination = ObjectDestination::Graveyard;
        cards.push(dead);
    }
    let initial = setup(Some(Step::PrecombatMain), cards);
    let creature_cast = cast(&initial, A, 51, None);
    let game = resolve(creature_cast.state()).into_state();
    let caster = object(&game, 51);
    let rock = object(&game, 52);
    let grave = object(&game, 53);
    let request = |objects| GameCommand::CastWithResources {
        spell: object(&game, 50),
        target: None,
        payment: CastingPayment {
            chosen_x: None,
            hybrid_choices: Vec::new(),
            fixed_targets: Vec::new(),
            permanent_targets: Vec::new(),
            target_division: Vec::new(),
            modes: Vec::new(),
            adventure: false,
            split_half: None,
            disturb: false,
            splice: vec![],
            exile_cast_type: None,
            alternative_cost_source: None,
            graveyard_permission: None,
            use_alternative: false,
            use_granted_flashback: false,
            additional: BTreeMap::new(),
            mana: None,
            restricted_mana: Vec::new(),
            objects,
        },
    };
    for bad in [
        vec![CastResource::Improvise(rock), CastResource::Improvise(rock)],
        vec![CastResource::Convoke {
            creature: caster,
            color: Some(Color::Blue),
        }],
        vec![CastResource::Delve(object(&game, 54))],
        vec![CastResource::Improvise(caster)],
    ] {
        reject(&game, A, request(bad), EffectError::InvalidCastResource);
    }
    let mana = balances(&game);
    let paid = accept(
        &game,
        A,
        request(vec![
            CastResource::Convoke {
                creature: caster,
                color: Some(Color::Green),
            },
            CastResource::Improvise(rock),
            CastResource::Delve(grave),
        ]),
    );
    assert_eq!(balances(paid.state()), mana);
    assert!(paid.mana_payment().unwrap().spend.is_empty());
    assert_eq!(paid.mana_payment().unwrap().cost.generic, 2.into());
    assert_eq!(
        paid.mana_payment().unwrap().cost.required[&ManaType::Green],
        1.into()
    );
    assert_eq!(physical(paid.state(), 51).tapped(), Some(true));
    assert_eq!(physical(paid.state(), 52).tapped(), Some(true));
    assert_eq!(physical(paid.state(), 53).zone(), ObjectZone::Exile);
    assert_eq!(
        physical(resolve(paid.state()).state(), 50).zone(),
        ObjectZone::Battlefield
    );
}

#[test]
fn delve_cannot_exile_the_flashback_spell_paying_its_own_cost() {
    use spellwake_engine::{
        characteristics::KeywordAbility,
        mana::{CastResource, CastingPayment},
    };
    let mut cards = seeds();
    let source = cards.iter_mut().find(|c| c.card == CardId(10)).unwrap();
    source.destination = ObjectDestination::Graveyard;
    source
        .program
        .printed_keywords
        .insert(KeywordAbility::Delve);
    source.program.flashback = Some(ManaCost::generic(1.into()));
    let game = setup(Some(Step::PrecombatMain), cards);
    let spell = object(&game, 10);
    reject(
        &game,
        A,
        GameCommand::CastWithResources {
            spell,
            target: Some(TargetRef::Object(object(&game, 90))),
            payment: CastingPayment {
                chosen_x: None,
                hybrid_choices: Vec::new(),
                fixed_targets: Vec::new(),
                permanent_targets: Vec::new(),
                target_division: Vec::new(),
                modes: Vec::new(),
                adventure: false,
                split_half: None,
                disturb: false,
                splice: vec![],
                exile_cast_type: None,
                alternative_cost_source: None,
                graveyard_permission: None,
                use_alternative: false,
                use_granted_flashback: false,
                additional: BTreeMap::new(),
                mana: None,
                restricted_mana: Vec::new(),
                objects: vec![CastResource::Delve(spell)],
            },
        },
        EffectError::InvalidCastResource,
    );
}

fn optional_cast(game: &GameState, choices: &[(usize, i64)]) -> GameCommand {
    let times = choices
        .iter()
        .map(|&(index, n)| (index, n.into()))
        .collect();
    let total = choices.iter().map(|(_, n)| n).sum::<i64>();
    GameCommand::CastWithResources {
        spell: object(game, 10),
        target: Some(TargetRef::Object(object(game, 90))),
        payment: spellwake_engine::mana::CastingPayment {
            chosen_x: None,
            hybrid_choices: Vec::new(),
            fixed_targets: Vec::new(),
            permanent_targets: Vec::new(),
            target_division: Vec::new(),
            modes: Vec::new(),
            adventure: false,
            split_half: None,
            disturb: false,
            splice: vec![],
            exile_cast_type: None,
            alternative_cost_source: None,
            graveyard_permission: None,
            use_alternative: false,
            use_granted_flashback: false,
            additional: times,
            mana: Some(payment(&[(ManaType::Red, 1 + total.max(0))])),
            restricted_mana: Vec::new(),
            objects: vec![],
        },
    }
}
#[test]
fn kicker_copies_choices_and_changes_actual_resolution() {
    use spellwake_engine::program::{AdditionalCastCost, Comparison, ResolutionCondition};
    let mut cards = seeds();
    let seed = cards.iter_mut().find(|c| c.card == CardId(10)).unwrap();
    seed.program = Program::instant(
        ManaCost::one(ManaType::Red),
        ExecutableBody::new(
            BodyTarget::SingleCreature,
            vec![
                Instruction::BranchIfFalse {
                    condition: ResolutionCondition {
                        left: spellwake_engine::program::ValueExpr::event(
                            spellwake_engine::program::EventValueKind::KickerCount(0),
                        ),
                        comparison: Comparison::Greater,
                        right: spellwake_engine::program::ValueExpr::Literal(0.into()),
                    },
                    destination: 3,
                },
                Instruction::Damage { amount: 4.into() },
                Instruction::Jump { destination: 4 },
                Instruction::Damage { amount: 2.into() },
            ],
        ),
    );
    seed.program.additional_cast_costs =
        vec![AdditionalCastCost::Kicker(ManaCost::generic(1.into()))];
    let game = setup(Some(Step::PrecombatMain), cards);
    for choices in [&[(0, 2)][..], &[(0, 0)], &[(1, 1)]] {
        reject(
            &game,
            A,
            optional_cast(&game, choices),
            EffectError::InvalidCastResource,
        );
    }
    for (choices, amount) in [(&[][..], 2), (&[(0, 1)][..], 4)] {
        let cast = accept(&game, A, optional_cast(&game, choices));
        let original = &cast.state().execution().window().entries()[0];
        let copying = targeted(cast.state(), A, 12, TargetRef::Stack(original.id()));
        let pending = resolve(copying.state());
        let copied = accept(
            pending.state(),
            A,
            GameCommand::Reply {
                decision: pending.state().pending_decision().unwrap().id(),
                choice: DecisionReply::CopyTargets(CopyTargetChoice::Retain),
            },
        );
        assert_eq!(
            copied
                .state()
                .execution()
                .window()
                .entries()
                .last()
                .unwrap()
                .kicker_choices(),
            original.kicker_choices()
        );
        let first = resolve(copied.state());
        let second = resolve(first.state());
        assert_eq!(first.damage_journal().applied[0].amount, amount.into());
        assert_eq!(second.damage_journal().applied[0].amount, amount.into());
    }
}
#[test]
fn multikicker_can_repeat_and_buyback_only_returns_a_resolving_spell() {
    use spellwake_engine::program::AdditionalCastCost;
    let mut cards = seeds();
    let seed = cards.iter_mut().find(|c| c.card == CardId(10)).unwrap();
    seed.program.additional_cast_costs = vec![
        AdditionalCastCost::Multikicker(ManaCost::generic(1.into())),
        AdditionalCastCost::Buyback(ManaCost::generic(1.into())),
    ];
    let game = setup(Some(Step::PrecombatMain), cards.clone());
    let cast = accept(&game, A, optional_cast(&game, &[(0, 3), (1, 1)]));
    let entry = &cast.state().execution().window().entries()[0];
    assert_eq!(entry.kicker_choices()[&0], 3.into());
    assert!(entry.bought_back());
    assert_eq!(cast.mana_payment().unwrap().cost.generic, 4.into());
    let done = resolve(cast.state());
    assert_eq!(physical(done.state(), 10).zone(), ObjectZone::Hand(A));
    assert_ne!(object(done.state(), 10), object(&game, 10));
    let reply_priority = pass(cast.state());
    let counter = targeted(reply_priority.state(), B, 11, TargetRef::Stack(entry.id()));
    assert_eq!(
        physical(resolve(counter.state()).state(), 10).zone(),
        ObjectZone::Graveyard(A)
    );
    let bounce = targeted(
        cast.state(),
        A,
        13,
        TargetRef::Object(object(cast.state(), 90)),
    );
    let bounced = resolve(bounce.state());
    assert_eq!(
        physical(resolve(bounced.state()).state(), 10).zone(),
        ObjectZone::Graveyard(A)
    );
    let seed = cards.iter_mut().find(|c| c.card == CardId(10)).unwrap();
    seed.destination = ObjectDestination::Graveyard;
    seed.program.flashback = Some(ManaCost::one(ManaType::Red));
    let game = setup(Some(Step::PrecombatMain), cards);
    let cast = accept(&game, A, optional_cast(&game, &[(1, 1)]));
    assert_eq!(
        physical(resolve(cast.state()).state(), 10).zone(),
        ObjectZone::Exile
    );
}

#[test]
fn graveyard_casts_discard_before_priority_and_enforce_their_distinct_exits() {
    use spellwake_engine::{
        characteristics::KeywordAbility,
        mana::{CastResource, CastingPayment},
        occurrence::{RulesOccurrence, RulesRecord, ZoneReason},
    };
    for jump_start in [false, true] {
        let mut cards = seeds();
        let card = cards.iter_mut().find(|c| c.card == CardId(10)).unwrap();
        card.program.printed_keywords.insert(if jump_start {
            KeywordAbility::JumpStart
        } else {
            KeywordAbility::Retrace
        });
        card.destination = ObjectDestination::Graveyard;
        let game = setup(Some(Step::PrecombatMain), cards);
        let request = |objects| GameCommand::CastWithResources {
            spell: object(&game, 10),
            target: Some(TargetRef::Object(object(&game, 90))),
            payment: CastingPayment {
                objects,
                ..Default::default()
            },
        };
        let select = |id| {
            if jump_start {
                CastResource::JumpStart(object(&game, id))
            } else {
                CastResource::Retrace(object(&game, id))
            }
        };
        reject(
            &game,
            A,
            request(vec![select(8), select(1)]),
            EffectError::InvalidCastResource,
        );
        reject(
            &game,
            A,
            request(vec![select(3)]),
            EffectError::InvalidCastResource,
        );
        reject(
            &game,
            A,
            request(vec![select(10)]),
            EffectError::InvalidCastResource,
        );
        if !jump_start {
            reject(
                &game,
                A,
                request(vec![select(1)]),
                EffectError::InvalidCastResource,
            );
        }
        let id = if jump_start { 1 } else { 8 };
        let action = accept(&game, A, request(vec![select(id)]));
        let entry = &action.state().execution().window().entries()[0];
        assert_eq!(entry.cast_with_jump_start(), jump_start);
        assert_eq!(
            physical(action.state(), id).zone(),
            ObjectZone::Graveyard(A)
        );
        assert!(action.rules_journal().records.iter().any(|record|matches!(record,RulesRecord::Event(e) if e.occurrences.iter().any(|o|matches!(&o.value,RulesOccurrence::ZoneChange(m) if m.reason==ZoneReason::Discard{player:A})))));
        let expected = if jump_start {
            ObjectZone::Exile
        } else {
            ObjectZone::Graveyard(A)
        };
        assert_eq!(
            physical(resolve(action.state()).state(), 10).zone(),
            expected
        );
        let reply = pass(action.state());
        let counter = targeted(reply.state(), B, 11, TargetRef::Stack(entry.id()));
        assert_eq!(
            physical(resolve(counter.state()).state(), 10).zone(),
            expected
        );
    }
}

#[test]
fn surge_and_spectacle_consume_distinct_earlier_turn_facts() {
    use spellwake_engine::{mana::CastingPayment, program::ConditionalCastCost};
    let mut cards = seeds();
    for (id, price) in [
        (50, ConditionalCastCost::Surge(ManaCost::free())),
        (51, ConditionalCastCost::Spectacle(ManaCost::free())),
    ] {
        let mut seed = artifact(id, A, Some(ManaCost::generic(5.into())));
        seed.program.conditional_cost = Some(price);
        cards.push(seed);
    }
    cards.push(spell(
        52,
        A,
        Instruction::Damage { amount: 1.into() },
        ManaCost::free(),
        BodyTarget::SinglePlayer,
    ));
    let game = setup(Some(Step::PrecombatMain), cards);
    let request = |s: &GameState, id| GameCommand::CastWithResources {
        spell: object(s, id),
        target: None,
        payment: CastingPayment {
            chosen_x: None,
            modes: Vec::new(),
            use_alternative: true,
            ..Default::default()
        },
    };
    for id in [50, 51] {
        reject(
            &game,
            A,
            request(&game, id),
            EffectError::InvalidCastResource,
        );
    }
    let first = targeted(&game, A, 52, TargetRef::Player(B));
    let after = resolve(first.state());
    let before = balances(after.state());
    for id in [50, 51] {
        let cheap = accept(after.state(), A, request(after.state(), id));
        assert_eq!(cheap.mana_payment().unwrap().cost, ManaCost::free());
        assert_eq!(before, balances(cheap.state()));
        assert!(
            cheap
                .state()
                .execution()
                .window()
                .entries()
                .last()
                .unwrap()
                .paid_conditional_cost()
        );
    }
    let self_damage = targeted(&game, A, 52, TargetRef::Player(A));
    let done = resolve(self_damage.state());
    assert!(
        done.state()
            .execution()
            .cast_turn_history()
            .unwrap()
            .casters
            .contains(&A)
    );
    reject(
        done.state(),
        A,
        request(done.state(), 51),
        EffectError::InvalidCastResource,
    );
}

#[test]
fn ward_collects_from_the_original_opponent_or_counters_their_spell_or_ability() {
    use spellwake_engine::{
        effects::ResolutionPaymentReply,
        program::{TriggeredAbility, TriggeredAbilityId},
    };
    let mut cards = seeds();
    let warded = cards.iter_mut().find(|c| c.card == CardId(90)).unwrap();
    warded.program.triggered.push(TriggeredAbility::ward(
        TriggeredAbilityId(1),
        ManaCost::generic(2.into()),
    ));
    let game = setup(Some(Step::PrecombatMain), cards);
    let artifact = cast(&game, A, 2, Some(payment(&[(ManaType::Red, 1)])));
    let initial = resolve(artifact.state());
    let state = initial.state();
    let victim = TargetRef::Object(object(state, 90));
    for trigger_source in [
        GameCommand::Cast {
            spell: object(state, 10),
            target: victim,
        },
        GameCommand::ActivateAbility {
            source: object(state, 2),
            ability: ActivatedAbilityId(7),
            payment: None,
            target: victim,
        },
    ] {
        let cast = accept(state, A, trigger_source);
        assert_eq!(cast.state().execution().window().entries().len(), 2);
        let quote = resolve(cast.state());
        let choice = quote.state().pending_decision().unwrap();
        let RuleFrame::ResolutionPayment(frame) = choice.frame() else {
            panic!("expected ward payment")
        };
        assert_eq!(frame.payer(), A);
        assert_eq!(*frame.cost(), ManaCost::generic(2.into()));
        for paid in [false, true] {
            let reply = if paid {
                ResolutionPaymentReply::Pay(payment(&[(ManaType::Red, 2)]))
            } else {
                ResolutionPaymentReply::Decline
            };
            let after = accept(
                quote.state(),
                A,
                GameCommand::Reply {
                    decision: choice.id(),
                    choice: DecisionReply::ResolutionPayment(reply),
                },
            );
            assert_eq!(
                after.state().execution().window().entries().len(),
                usize::from(paid)
            );
            if paid {
                assert_eq!(
                    resolve(after.state()).damage_journal().applied[0].amount,
                    2.into()
                );
            } else {
                assert_eq!(
                    after
                        .state()
                        .execution()
                        .battlefield_marked_damage(object(state, 90)),
                    Some(0.into())
                );
            }
        }
    }
}

#[test]
fn protection_checks_the_source_color_when_choosing_a_target_or_retargeting_a_copy() {
    use spellwake_engine::characteristics::{Color, KeywordAbility};
    let mut cards = seeds();
    cards
        .iter_mut()
        .find(|c| c.card == CardId(90))
        .unwrap()
        .program
        .printed_keywords
        .insert(KeywordAbility::ProtectionFromRed);
    let target = creature(
        91,
        B,
        ObjectDestination::Battlefield { controller: B },
        None,
        8,
    );
    cards.push(target);
    let ability = cards.iter_mut().find(|c| c.card == CardId(2)).unwrap();
    ability.program.color_indicator = Some(BTreeSet::from([Color::Red]));
    let game = setup(Some(Step::PrecombatMain), cards);
    let protected = TargetRef::Object(object(&game, 90));
    reject(
        &game,
        A,
        GameCommand::Cast {
            spell: object(&game, 10),
            target: protected,
        },
        EffectError::IllegalTarget,
    );
    let rock = cast(&game, A, 2, Some(payment(&[(ManaType::Red, 1)])));
    let on_field = resolve(rock.state());
    reject(
        on_field.state(),
        A,
        GameCommand::ActivateAbility {
            source: object(on_field.state(), 2),
            ability: ActivatedAbilityId(7),
            target: protected,
            payment: None,
        },
        EffectError::IllegalTarget,
    );
    let red = targeted(
        on_field.state(),
        A,
        10,
        TargetRef::Object(object(on_field.state(), 91)),
    );
    let copying = targeted(
        red.state(),
        A,
        12,
        TargetRef::Stack(red.state().execution().window().entries()[0].id()),
    );
    let pending = resolve(copying.state());
    let frame = pending.state().pending_decision().unwrap();
    reject(
        pending.state(),
        A,
        GameCommand::Reply {
            decision: frame.id(),
            choice: DecisionReply::CopyTargets(CopyTargetChoice::Change(protected)),
        },
        EffectError::IllegalTarget,
    );
    let blue = targeted(&game, A, 13, protected);
    assert_eq!(
        physical(resolve(blue.state()).state(), 90).zone(),
        ObjectZone::Hand(B)
    );
}

#[test]
fn plot_pays_in_hand_and_only_the_later_main_phase_can_cast_that_exile_object_for_free() {
    use spellwake_engine::combat::CombatDecision;
    let mut cards = seeds();
    cards.retain(|card| matches!(card.card, CardId(1 | 98 | 99)));
    cards
        .iter_mut()
        .find(|s| s.card == CardId(1))
        .unwrap()
        .program
        .plot = Some(ManaCost::one(ManaType::Blue));
    let game = setup(Some(Step::PostcombatMain), cards);
    let hand = object(&game, 1);
    let action = GameCommand::Plot {
        source: hand,
        payment: None,
    };
    let paid = accept(&game, A, action);
    let state = paid.state();
    let exiled = object(state, 1);
    assert_ne!(hand, exiled);
    assert_eq!(physical(state, 1).zone(), ObjectZone::Exile);
    assert_eq!(state.execution().plotted_on(exiled), Some(3));
    assert_eq!(state.execution().mana(A, ManaType::Blue), 19.into());
    assert!(state.execution().window().entries().is_empty());
    assert_eq!(holder(state), A);
    reject(
        state,
        A,
        untargeted(state, 1, None),
        EffectError::InvalidCastResource,
    );
    let mut state = state.clone();
    for _ in 0..130 {
        let turn = state.turn().unwrap();
        if turn.position().number == 5 && turn.position().step == Step::PrecombatMain {
            break;
        }
        if let Some(decision) = state.pending_decision() {
            let choice = match decision.frame() {
                RuleFrame::Combat(CombatDecision::Attackers(_)) => {
                    DecisionReply::DeclareAttackers(vec![])
                }
                RuleFrame::Combat(CombatDecision::Blockers(_)) => {
                    DecisionReply::DeclareBlockers(vec![])
                }
                frame => panic!("unexpected {frame:?}"),
            };
            state = accept(
                &state,
                decision.chooser(),
                GameCommand::Reply {
                    decision: decision.id(),
                    choice,
                },
            )
            .into_state();
        } else {
            state = pass(&state).into_state();
        }
    }
    assert_eq!(state.turn().unwrap().position().number, 5);
    let free = cast(&state, A, 1, None);
    assert_eq!(free.mana_payment().unwrap().cost, ManaCost::free());
    assert!(free.mana_payment().unwrap().spend.is_empty());
    assert_eq!(free.state().execution().plotted_on(exiled), None);
    assert_eq!(
        physical(resolve(free.state()).state(), 1).zone(),
        ObjectZone::Battlefield
    );
}

#[test]
fn bloodthirst_counts_damage_dealt_to_opponents_instead_of_life_lost() {
    use spellwake_engine::{life::LifeAdjustment, program::EntryCounterAbility};
    let mut cards = seeds();
    let mut thirsty = creature(71, A, ObjectDestination::Hand, Some(ManaCost::free()), 4);
    thirsty.program.entry_counter_abilities = vec![
        EntryCounterAbility::Bloodthirst(2.into()),
        EntryCounterAbility::BloodthirstX,
    ];
    cards.push(thirsty);
    cards.push(spell(
        72,
        A,
        Instruction::Damage { amount: 3.into() },
        ManaCost::free(),
        BodyTarget::SinglePlayer,
    ));
    cards.push(spell(
        73,
        A,
        Instruction::AdjustTargetLife {
            adjustment: LifeAdjustment::Lose(3.into()),
        },
        ManaCost::free(),
        BodyTarget::SinglePlayer,
    ));
    let initial = setup(Some(Step::PrecombatMain), cards);
    for (id, target, expected) in [(72, B, 5), (72, A, 0), (73, B, 0)] {
        let spell = targeted(&initial, A, id, TargetRef::Player(target));
        let applied = resolve(spell.state());
        let creature = cast(applied.state(), A, 71, None);
        let entered = resolve(creature.state());
        let state = entered.state();
        assert_eq!(
            state
                .execution()
                .power_toughness_counters(object(state, 71))
                .unwrap()
                .plus_one,
            expected.into()
        );
    }
}

#[test]
fn sunburst_counts_distinct_colors_actually_spent_for_creatures_and_other_permanents() {
    use spellwake_engine::program::EntryCounterAbility;
    let price = Some(ManaCost::generic(4.into()));
    let mut body = creature(71, A, ObjectDestination::Hand, price.clone(), 0);
    let mut item = artifact(72, A, price);
    for seed in [&mut body, &mut item] {
        seed.program.entry_counter_abilities = vec![EntryCounterAbility::Sunburst];
    }
    let mut cards = seeds();
    cards.extend([body, item]);
    let state = setup(Some(Step::PrecombatMain), cards);
    let pay = Some(payment(&[
        (ManaType::Red, 2),
        (ManaType::White, 1),
        (ManaType::Colourless, 1),
    ]));
    for id in [71, 72] {
        let first = cast(&state, A, id, pay.clone());
        assert_eq!(
            first.state().execution().window().entries()[0].sunburst_colors(),
            2
        );
        let enter = resolve(first.state());
        let object = object(enter.state(), id);
        assert_eq!(physical(enter.state(), id).zone(), ObjectZone::Battlefield);
        if id == 71 {
            assert_eq!(
                enter
                    .state()
                    .execution()
                    .power_toughness_counters(object)
                    .unwrap()
                    .plus_one,
                2.into()
            );
        } else {
            assert_eq!(
                enter.state().execution().charge_counters().get(&object),
                Some(&2.into())
            );
        }
    }
}
