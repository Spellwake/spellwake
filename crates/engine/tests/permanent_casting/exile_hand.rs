#![allow(dead_code, unused_imports)]
use super::*;
use spellwake_engine::{
    characteristics::KeywordAbility, combat::CombatDecision, effects::OptionalEffectChoice,
    mana::CastingPayment, program::ExileAction,
};

fn advance_to(mut game: GameState, until: impl Fn(&GameState) -> bool) -> GameState {
    for _ in 0..120 {
        if until(&game) {
            return game;
        }
        game = if let Some(p) = game.pending_decision() {
            let answer = match p.frame() {
                RuleFrame::Combat(CombatDecision::Attackers(_)) => {
                    DecisionReply::DeclareAttackers(vec![])
                }
                RuleFrame::Combat(CombatDecision::Blockers(_)) => {
                    DecisionReply::DeclareBlockers(vec![])
                }
                other => panic!("unexpected {other:?}"),
            };
            accept(
                &game,
                p.chooser(),
                GameCommand::Reply {
                    decision: p.id(),
                    choice: answer,
                },
            )
            .into_state()
        } else {
            pass(&game).into_state()
        };
    }
    panic!("requested game state was not reached")
}

#[test]
fn foretell_is_private_and_characteristicless_and_can_cast_an_instant_on_a_later_opposing_turn() {
    let mut cards = seeds();
    cards.retain(|c| !matches!(c.card.0, 2 | 6 | 7 | 8 | 12 | 13 | 14));
    let spell = cards.iter_mut().find(|c| c.card == CardId(10)).unwrap();
    spell.program.exile_action = Some(ExileAction::Foretell(ManaCost::free()));
    cards
        .iter_mut()
        .find(|c| c.card == CardId(5))
        .unwrap()
        .program
        .printed_keywords
        .insert(KeywordAbility::SplitSecond);
    let game = setup(Some(Step::PostcombatMain), cards);
    let blocking = cast(&game, A, 5, None);
    let game = blocking.into_state();
    let hand = object(&game, 10);
    let action = GameCommand::Foretell {
        source: hand,
        payment: Some(payment(&[(ManaType::Green, 2)])),
    };
    let paid = accept(&game, A, action).into_state();
    assert_eq!(paid.execution().window().entries().len(), 1);
    let exile = object(&paid, 10);
    assert_eq!(paid.execution().foretold_on(exile), Some(3));
    let characteristics = paid
        .execution()
        .current_characteristics(exile)
        .unwrap()
        .unwrap();
    assert!(characteristics.current_types.is_empty() && characteristics.current_colors.is_empty());
    reject(
        &paid,
        A,
        GameCommand::Cast {
            spell: exile,
            target: TargetRef::Object(object(&paid, 90)),
        },
        EffectError::InvalidCastResource,
    );
    let game = advance_to(paid, |s| {
        s.turn().unwrap().position().number == 4
            && s.turn().unwrap().position().step == Step::Upkeep
            && holder(s) == A
    });
    let result = targeted(&game, A, 10, TargetRef::Object(object(&game, 90)));
    assert_eq!(result.mana_payment().unwrap().cost, ManaCost::free());
    assert_eq!(result.state().execution().foretold_on(exile), None);
    assert!(result.rules_journal().records.iter().any(|row|matches!(row,spellwake_engine::occurrence::RulesRecord::Event(e) if e.occurrences.iter().any(|v|matches!(&v.value,spellwake_engine::occurrence::RulesOccurrence::FaceDownRevealed{object} if object.id()==exile)))));
    assert_eq!(
        physical(resolve(result.state()).state(), 10).zone(),
        ObjectZone::Graveyard(A)
    );
}

#[test]
fn suspend_waits_for_owner_upkeep_and_offers_real_cast_with_target_or_decline() {
    let mut cards = seeds();
    cards.retain(|c| !matches!(c.card.0, 2 | 6 | 7 | 8 | 12 | 13 | 14));
    cards
        .iter_mut()
        .find(|c| c.card == CardId(10))
        .unwrap()
        .program
        .exile_action = Some(ExileAction::Suspend {
        price: ManaCost::one(ManaType::Green),
        counters: 1,
    });
    let game = setup(Some(Step::PostcombatMain), cards);
    let mut blocked_cards = seeds();
    blocked_cards
        .iter_mut()
        .find(|c| c.card == CardId(5))
        .unwrap()
        .program
        .printed_keywords
        .insert(KeywordAbility::SplitSecond);
    blocked_cards
        .iter_mut()
        .find(|c| c.card == CardId(10))
        .unwrap()
        .program
        .exile_action = Some(ExileAction::Suspend {
        price: ManaCost::free(),
        counters: 1,
    });
    let before_block = setup(Some(Step::PostcombatMain), blocked_cards);
    let blocked = cast(&before_block, A, 5, None).into_state();
    reject(
        &blocked,
        A,
        GameCommand::Suspend {
            source: object(&blocked, 10),
            payment: None,
        },
        EffectError::SplitSecond,
    );
    let suspended = accept(
        &game,
        A,
        GameCommand::Suspend {
            source: object(&game, 10),
            payment: None,
        },
    )
    .into_state();
    let exiled = object(&suspended, 10);
    assert_eq!(
        suspended.execution().time_counters().get(&exiled),
        Some(&1.into())
    );
    let upkeep = advance_to(suspended, |s| {
        s.turn().unwrap().position().number == 5 && !s.execution().window().entries().is_empty()
    });
    assert_eq!(upkeep.turn().unwrap().position().step, Step::Upkeep);
    let removed = resolve(&upkeep).into_state();
    assert_eq!(removed.execution().time_counters().get(&exiled), None);
    assert_eq!(removed.execution().window().entries().len(), 1);
    let choice = resolve(&removed).into_state();
    let pending = choice.pending_decision().unwrap();
    let RuleFrame::OptionalEffect(frame) = pending.frame() else {
        panic!("expected optional suspend cast")
    };
    let target = TargetRef::Object(object(&choice, 90));
    assert_eq!(frame.suspended_cast().unwrap().spell, exiled);
    assert!(
        frame
            .suspended_cast()
            .unwrap()
            .targets
            .as_ref()
            .unwrap()
            .contains(&target)
    );
    let declined = accept(
        &choice,
        A,
        GameCommand::Reply {
            decision: pending.id(),
            choice: DecisionReply::OptionalEffect(OptionalEffectChoice::Decline),
        },
    )
    .into_state();
    assert_eq!(physical(&declined, 10).zone(), ObjectZone::Exile);
    assert!(declined.execution().window().entries().is_empty());
    let cast = accept(
        &choice,
        A,
        GameCommand::Reply {
            decision: pending.id(),
            choice: DecisionReply::SuspendCast {
                target: Some(target),
                payment: CastingPayment::default(),
            },
        },
    )
    .into_state();
    assert!(matches!(
        cast.execution().window().entries()[0].kind(),
        StackKind::CardSpell(_)
    ));
    assert_eq!(
        physical(resolve(&cast).state(), 10).zone(),
        ObjectZone::Graveyard(A)
    );
}

#[test]
fn paid_buyback_and_rebound_offer_both_actual_resolution_destinations() {
    let mut cards = seeds();
    let program = &mut cards
        .iter_mut()
        .find(|c| c.card == CardId(10))
        .unwrap()
        .program;
    program.printed_keywords.insert(KeywordAbility::Rebound);
    program
        .additional_cast_costs
        .push(spellwake_engine::program::AdditionalCastCost::Buyback(
            ManaCost::free(),
        ));
    let game = setup(Some(Step::PrecombatMain), cards);
    let cast = accept(
        &game,
        A,
        GameCommand::CastWithResources {
            spell: object(&game, 10),
            target: Some(TargetRef::Object(object(&game, 90))),
            payment: CastingPayment {
                additional: BTreeMap::from([(0, 1.into())]),
                ..Default::default()
            },
        },
    );
    let ready = resolve(cast.state()).into_state();
    let choice = ready.pending_decision().unwrap();
    assert!(matches!(choice.frame(),RuleFrame::OptionalEffect(f) if f.rebound_or_buyback()));
    for (branch, zone) in [
        (OptionalEffectChoice::Take, ObjectZone::Exile),
        (OptionalEffectChoice::Decline, ObjectZone::Hand(A)),
    ] {
        let next = accept(
            &ready,
            A,
            GameCommand::Reply {
                decision: choice.id(),
                choice: DecisionReply::OptionalEffect(branch),
            },
        )
        .into_state();
        assert_eq!(physical(&next, 10).zone(), zone);
        assert_eq!(
            next.execution()
                .battlefield_marked_damage(object(&next, 90)),
            Some(2.into())
        );
    }
}

#[test]
fn rebound_with_no_legal_target_finishes_the_upkeep_trigger_and_leaves_the_card_exiled() {
    let mut cards = seeds();
    cards.retain(|c| matches!(c.card.0, 10 | 90 | 98 | 99));
    let source = cards.iter_mut().find(|c| c.card == CardId(10)).unwrap();
    source.program = Program::instant(
        ManaCost::free(),
        ExecutableBody::new(BodyTarget::SingleCreature, vec![Instruction::DestroyTarget]),
    )
    .with_printed_keywords([KeywordAbility::Rebound]);
    let start = setup(Some(Step::PostcombatMain), cards);
    let cast = targeted(&start, A, 10, TargetRef::Object(object(&start, 90)));
    let exiled = resolve(cast.state()).into_state();
    assert_eq!(physical(&exiled, 90).zone(), ObjectZone::Graveyard(B));
    let upkeep = advance_to(exiled, |g| {
        g.turn().unwrap().position().number == 5 && !g.execution().window().entries().is_empty()
    });
    let done = resolve(&upkeep).into_state();
    assert!(done.pending_decision().is_none());
    assert_eq!(physical(&done, 10).zone(), ObjectZone::Exile);
    assert!(done.execution().window().entries().is_empty());
}
