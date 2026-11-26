#![allow(dead_code, unused_imports)]
//! Real stack choices and separate entry, move and death information for the counter-transfer abilities.
use super::{keyword_damage::cast_on, keyword_triggers::place, *};
use spellwake_engine::{
    characteristics::{KeywordAbility, PowerToughnessCounters},
    effects::OptionalEffectChoice,
    occurrence::{RulesOccurrence, RulesRecord},
    program::{CardType, TransferKeyword as K, TriggeredAbilityId},
};
fn with(mut card: SeedCard, keys: &[K]) -> SeedCard {
    for (i, &key) in keys.iter().enumerate() {
        card.program.entry_counter_abilities.extend(key.entry());
        card.program
            .triggered
            .push(key.ability(TriggeredAbilityId(i as u64)));
    }
    card
}
fn cast(s: &GameState, n: u64) -> GameState {
    round(
        act(
            s,
            A,
            GameCommand::CastUntargeted {
                spell: object(s, n).id(),
                payment: None,
            },
        )
        .state(),
    )
    .into_state()
}
fn opt(s: &GameState, choice: OptionalEffectChoice) -> NativeCandidate {
    reply(s, DecisionReply::OptionalEffect(choice))
}
fn counters(s: &GameState, n: u64) -> spellwake_contracts::RulesInteger {
    s.execution()
        .power_toughness_counters(object(s, n).id())
        .unwrap()
        .plus_one
}
fn select(s: &GameState, n: u64) -> GameState {
    reply(
        s,
        DecisionReply::TriggerTarget(TargetRef::Object(object(s, n).id())),
    )
    .into_state()
}
fn hand(n: u64) -> SeedCard {
    creature(n, A, ObjectDestination::Hand)
}
fn art(mut seed: SeedCard) -> SeedCard {
    seed.program.card_types.insert(CardType::Artifact);
    seed
}

#[test]
fn soulshift_fixes_a_graveyard_identity_checks_owner_type_value_and_rechecks_after_return() {
    use spellwake_contracts::creature_types::CreatureTypeVocabulary;
    use std::sync::Arc;
    let vocabulary = Arc::new(
        CreatureTypeVocabulary::new("cr-transfer-tests".into(), [0; 32], vec!["Spirit".into()])
            .unwrap(),
    );
    let spirit = vocabulary.resolve_label("Spirit").unwrap();
    let names = Arc::new(spellwake_contracts::names::NameRules::literal_names());
    let context = GameContext::new(
        "transfer-rules".into(),
        "cr-transfer-tests".into(),
        "authored".into(),
        "soulshift".into(),
        "fff".into(),
        vocabulary.id(),
        names.id(),
    )
    .unwrap();
    let grave = |id, owner, cost: i64, is_spirit| {
        let mut seed = creature(id, owner, ObjectDestination::Graveyard);
        seed.program = Program::creature(
            Some(ManaCost::generic(cost.into())),
            1.into(),
            1.into(),
            if is_spirit {
                [spirit].into()
            } else {
                Default::default()
            },
        );
        seed
    };
    let caster = with(battlefield(1, A), &[K::Soulshift(2), K::Soulshift(2)]);
    let cards = vec![
        caster,
        grave(2, A, 2, true),
        grave(3, A, 3, true),
        grave(4, A, 0, false),
        grave(5, B, 1, true),
        instant(6, A, Instruction::DestroyTarget),
    ];
    let seed = GameSeed::with_uniform_life(
        vec![A, B],
        A,
        cards,
        vec![CardId(1)],
        vec![],
        20.into(),
        CombatRules::UnrestrictedFreeForAll,
        vocabulary,
        names,
    );
    let state = GameState::initialize_scheduled(
        GameId(85),
        context,
        seed,
        TurnStart {
            number: 1,
            step: Step::PrecombatMain,
        },
    )
    .unwrap()
    .into_state();
    let mut dead = place(cast_on(&state, 6, 1));
    for _ in 0..2 {
        let p = dead.pending_decision().unwrap();
        let RuleFrame::TriggerTarget(frame) = p.frame() else {
            panic!("soulshift target")
        };
        assert_eq!(frame.domain(), &[TargetRef::Object(object(&dead, 2).id())]);
        dead = select(&dead, 2);
    }
    let first = round(&dead);
    let declined = opt(first.state(), OptionalEffectChoice::Decline);
    assert_eq!(object(declined.state(), 2).zone(), ObjectZone::Graveyard(A));
    let returned = opt(first.state(), OptionalEffectChoice::Take);
    assert_eq!(object(returned.state(), 2).zone(), ObjectZone::Hand(A));
    let second = round(returned.state());
    assert!(second.state().pending_decision().is_none());
    assert!(second.rules_journal().records.iter().any(|record|matches!(record,RulesRecord::Event(event) if event.occurrences.iter().any(|o|matches!(o.value,RulesOccurrence::EntryCompleted{completion:spellwake_engine::occurrence::EntryCompletion::AllTargetsIllegal,..})))));
}

#[test]
fn extort_loses_opponents_life_together_then_gains_actual_total_and_rejects_red_mana() {
    use spellwake_engine::{
        effects::ResolutionPaymentReply,
        life::LifeChangeKind,
        mana::{ManaAbilityId, ManaAbilityRef, ManaPayment, ManaSpend},
        program::TriggeredAbility,
    };
    let mut source = battlefield(1, A);
    source.program.triggered = vec![TriggeredAbility::extort(TriggeredAbilityId(0))];
    let game = setup(
        vec![A, B, C],
        vec![
            source,
            hand(2),
            upkeep_keywords::land(3, ManaType::Red, 1),
            upkeep_keywords::land(4, ManaType::Black, 1),
        ],
        Step::PrecombatMain,
    )
    .into_state();
    let cast = act(
        &game,
        A,
        GameCommand::CastUntargeted {
            spell: object(&game, 2).id(),
            payment: None,
        },
    );
    let offer = round(cast.state()).into_state();
    let tap = |s: &GameState, n| {
        act(
            s,
            A,
            GameCommand::ActivateMana {
                source: object(s, n).id(),
                ability: ManaAbilityRef::Printed(ManaAbilityId(1)),
            },
        )
        .into_state()
    };
    let payment = |kind| {
        DecisionReply::ResolutionPayment(ResolutionPaymentReply::Pay(ManaPayment {
            from_lots: Default::default(),
            spend: vec![ManaSpend {
                mana_type: kind,
                amount: 1.into(),
            }],
        }))
    };
    let red = tap(&offer, 3);
    let wrong = input(
        &red,
        A,
        GameCommand::Reply {
            decision: red.pending_decision().unwrap().id(),
            choice: payment(ManaType::Red),
        },
    );
    assert!(red.advance(wrong).is_err());
    let black = tap(&red, 4);
    let result = reply(&black, payment(ManaType::Black));
    let groups: Vec<Vec<_>> = result
        .rules_journal()
        .records
        .iter()
        .filter_map(|row| match row {
            RulesRecord::Event(e) => {
                let group: Vec<_> = e
                    .occurrences
                    .iter()
                    .filter_map(|o| match &o.value {
                        RulesOccurrence::LifeChanged(change) => {
                            Some((change.kind, change.player, change.amount.clone()))
                        }
                        _ => None,
                    })
                    .collect();
                (!group.is_empty()).then_some(group)
            }
            _ => None,
        })
        .collect();
    assert_eq!(groups.len(), 2);
    assert_eq!(groups[0].len(), 2);
    assert!(
        groups[0].contains(&(LifeChangeKind::Loss, B, 1.into()))
            && groups[0].contains(&(LifeChangeKind::Loss, C, 1.into()))
    );
    assert_eq!(groups[1], [(LifeChangeKind::Gain, A, 2.into())]);
}
