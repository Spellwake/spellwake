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
fn modular_counts_last_known_counters_selects_artifact_creatures_and_may_decline() {
    let modular = with(art(hand(1)), &[K::Modular(1), K::Modular(2)]);
    let game = setup(
        vec![A, B],
        vec![
            modular,
            art(battlefield(2, B)),
            battlefield(3, A),
            instant(
                4,
                A,
                Instruction::PutPowerToughnessCounters {
                    counters: PowerToughnessCounters {
                        plus_one: 1.into(),
                        minus_one: 0.into(),
                    },
                },
            ),
            instant(5, A, Instruction::DestroyTarget),
        ],
        Step::PrecombatMain,
    )
    .into_state();
    let entered = cast(&game, 1);
    assert_eq!(counters(&entered, 1), 3.into());
    let larger = cast_on(&entered, 4, 1);
    let mut dead = place(cast_on(&larger, 5, 1));
    for _ in 0..2 {
        let RuleFrame::TriggerTarget(f) = dead.pending_decision().unwrap().frame() else {
            panic!("modular target")
        };
        assert_eq!(f.domain(), &[TargetRef::Object(object(&dead, 2).id())]);
        dead = select(&dead, 2);
    }
    let first = round(&dead).into_state();
    let extra = opt(&first, OptionalEffectChoice::Take).into_state();
    assert_eq!(counters(&extra, 2), 4.into());
    let second = round(&extra).into_state();
    let declined = opt(&second, OptionalEffectChoice::Decline).into_state();
    assert_eq!(counters(&declined, 2), 4.into());
}
#[test]
fn graft_moves_one_counter_atomically_without_targeting_and_rechecks_each_instance() {
    let graft = with(hand(1), &[K::Graft(1), K::Graft(0)]);
    let mut receiver = hand(2);
    receiver
        .program
        .printed_keywords
        .insert(KeywordAbility::Shroud);
    let game = setup(
        vec![A, B],
        vec![
            graft,
            receiver,
            instant(3, A, Instruction::ReturnToOwnersHand),
        ],
        Step::PrecombatMain,
    )
    .into_state();
    let entered = cast(&game, 1);
    assert_eq!(counters(&entered, 1), 1.into());
    let triggers = place(cast(&entered, 2));
    assert_eq!(triggers.execution().window().entries().len(), 2);
    let old = object(&triggers, 1).id();
    let copy = cast_on(&triggers, 3, 1);
    let none = round(&copy).into_state();
    assert!(none.pending_decision().is_none());
    assert_eq!(counters(&none, 2), 0.into());
    let choice = round(&triggers);
    let moved = opt(choice.state(), OptionalEffectChoice::Take);
    assert_eq!(counters(moved.state(), 1), 0.into());
    assert_eq!(counters(moved.state(), 2), 1.into());
    assert!(moved.rules_journal().records.iter().any(|record| matches!(record,RulesRecord::Event(e) if e.occurrences.iter().any(|o|matches!(o.value,RulesOccurrence::CounterRemovedForGraft{object,..} if object==old)) && e.occurrences.iter().any(|o|matches!(o.value,RulesOccurrence::CountersPlaced{object:target,..} if target==object(moved.state(),2).id())))));
    let second = round(moved.state()).into_state();
    assert!(second.pending_decision().is_none());
    assert_eq!(counters(&second, 2), 1.into());
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

#[test]
fn annihilator_uses_the_attacked_opponent_after_its_source_leaves() {
    use spellwake_engine::program::TriggeredAbility;
    let mut source = battlefield(1, A);
    source.program.triggered.push(TriggeredAbility::annihilator(
        TriggeredAbilityId(0),
        2.into(),
    ));
    let game = setup(
        vec![A, B, C],
        vec![
            source,
            battlefield(2, B),
            battlefield(3, C),
            battlefield(4, C),
            battlefield(5, C),
            instant(7, A, Instruction::ReturnToOwnersHand),
        ],
        Step::Untap,
    )
    .into_state();
    let attack = declared(&attackers(&game), &[(1, C)]);
    let left = cast_on(attack.state(), 7, 1);
    let selecting = round(&left).into_state();
    let p = selecting.pending_decision().unwrap();
    let RuleFrame::ResolutionSelect(f) = p.frame() else {
        panic!("sacrifices")
    };
    assert_eq!(p.chooser(), C);
    assert_eq!(f.required(), 2);
    assert_eq!(f.domain().len(), 3);
    let chosen = vec![object(&selecting, 3).id(), object(&selecting, 5).id()];
    let mut done = reply(&selecting, DecisionReply::SelectResolution(chosen)).into_state();
    if let Some(p) = done.pending_decision() {
        let RuleFrame::ResolutionGraveyardOrder(f) = p.frame() else {
            panic!("order sacrificed cards")
        };
        done = reply(
            &done,
            DecisionReply::OrderResolutionGraveyard(f.domain().to_vec()),
        )
        .into_state();
    }
    assert_eq!(object(&done, 3).zone(), ObjectZone::Graveyard(C));
    assert_eq!(object(&done, 5).zone(), ObjectZone::Graveyard(C));
    assert_eq!(object(&done, 2).zone(), ObjectZone::Battlefield);
}
#[test]
fn ingest_exiles_only_the_damaged_players_top_card_or_nothing_from_an_empty_library() {
    use spellwake_engine::program::CombatTriggerKeyword::Ingest;
    let mut seeds = vec![
        battlefield(1, A),
        instant(7, A, Instruction::ReturnToOwnersHand),
    ];
    super::keyword_triggers::add(&mut seeds, 1, &[Ingest, Ingest]);
    let game = setup(vec![A, B, C], seeds, Step::Untap).into_state();
    let attacking = declared(&attackers(&game), &[(1, B)]).into_state();
    let blocked = round(&attacking).into_state();
    let damaged = place(round(&blocked).into_state());
    assert_eq!(damaged.execution().window().entries().len(), 2);
    let top = *damaged
        .execution()
        .window()
        .cards()
        .library(B)
        .first()
        .unwrap();
    let card = damaged
        .execution()
        .window()
        .cards()
        .get(top)
        .unwrap()
        .card_id()
        .unwrap();
    let left = cast_on(&damaged, 7, 1);
    let exiled = round(&left).into_state();
    assert_eq!(object(&exiled, card.0).zone(), ObjectZone::Exile);
    assert!(exiled.execution().window().cards().library(B).is_empty());
    let empty = round(&exiled).into_state();
    assert_eq!(empty.execution().window().cards().library(C).len(), 1);
    assert!(empty.pending_decision().is_none());
}
