//! Library, graveyard and spell permissions from named planeswalker abilities.
use super::super::declarations::invalid;
use super::additional_walkers::permanent;
use super::*;

pub(super) fn entropy(state: GameState) -> GameState {
    state
        .supply_entropy(state.basis(), (1_u64..=128).collect())
        .unwrap()
        .into_state()
}
fn at_main(state: GameState, turn: u64) -> GameState {
    exile_hand::until(state, |s| {
        s.turn().unwrap().position().number == turn
            && s.turn().unwrap().position().step == Step::PrecombatMain
    })
}
fn card_zone(s: &GameState, n: u64) -> ObjectZone {
    physical(s, n).zone()
}
fn look_four(s: &GameState, op: u64, eligible: u64, ineligible: u64) {
    let look = round(command(s, A, activate(s, 1, op, &[])).into_state()).into_state();
    let look = entropy(look);
    let RuleFrame::LibraryTop(frame) = look.pending_decision().unwrap().frame() else {
        panic!("look four");
    };
    assert_eq!(frame.domain_top_first().len(), 4);
    assert_eq!(frame.eligible_results(), &[physical(&look, eligible).id()]);
    let choose = |n: Option<u64>| {
        DecisionReply::LibraryTop(spellwake_engine::effects::LibraryTopReply::Search {
            selected: n.map(|n| physical(&look, n).id()),
        })
    };
    invalid(&look, A, choose(Some(ineligible)));
    for take in [false, true] {
        let new = reply(&look, choose(take.then_some(eligible)));
        assert_eq!(
            card_zone(new.state(), eligible),
            if take {
                ObjectZone::Hand(A)
            } else {
                ObjectZone::Library(A)
            }
        );
        let library = new.state().execution().window().cards().library(A);
        assert_eq!(library.first(), Some(&physical(new.state(), 14).id()));
        assert!(
            events(&new)
                .flat_map(|e| &e.occurrences)
                .any(|row| matches!(row.value, RulesOccurrence::LibraryCardRevealed { .. }))
                == take
        );
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_calix_filters_enchantments_links_exile_to_one_enchantment_and_returns_only_its_owners_enchantments()
 {
    use ManaType::*;
    let mut cards = vec![
        seed(2, A, battlefield(A), Program::enchantment(None)),
        creature(3, B, 2, 3, &[]),
        seed(4, B, battlefield(B), Program::enchantment(None)),
        seed(
            5,
            A,
            ObjectDestination::Graveyard,
            Program::enchantment(None),
        ),
        seed(
            6,
            A,
            ObjectDestination::Graveyard,
            Program::artifact(None, vec![]),
        ),
        seed(
            7,
            B,
            ObjectDestination::Graveyard,
            Program::enchantment(None),
        ),
        seed(
            8,
            A,
            ObjectDestination::Hand,
            free_instant(
                BodyTarget::SinglePermanent,
                crate::keyword_creatures::oracle_lowering::toolbox_emit::zone_op(
                    spellwake_engine::toolbox::ZoneOpSpec::move_all(
                        spellwake_engine::toolbox::ObjectQuery::Targets,
                        spellwake_engine::toolbox::ZoneDest::Graveyard {
                            semantics: spellwake_engine::toolbox::GraveyardSemantics::Destroy {
                                allow_regenerate: true,
                            },
                        },
                    ),
                ),
            ),
        ),
    ];
    for n in 10..=14 {
        cards.push(seed(
            n,
            A,
            ObjectDestination::LibraryBottom,
            if n == 11 {
                Program::enchantment(None)
            } else {
                Program::artifact(None, vec![])
            },
        ));
    }
    let ready = with_loyalty_for_ultimate(
        "Calix, Destiny's Hand",
        &[(Colourless, 2), (Green, 1), (White, 1)],
        4,
        7,
        cards,
    );
    look_four(&ready, 0, 11, 10);
    for targets in [&[2, 2][..], &[3, 4][..], &[3, 3][..]] {
        illegal(&ready, A, selected_from(&ready, 1, targets));
    }
    for target in [3, 4] {
        let exile = round(command(&ready, A, selected_from(&ready, 1, &[target, 2])).into_state())
            .into_state();
        let old = physical(&ready, target).id();
        assert_eq!(card_zone(&exile, target), ObjectZone::Exile);
        let broken = round(targeted(&exile, A, 8, permanent(&exile, 2)).into_state()).into_state();
        assert_eq!(card_zone(&broken, target), ObjectZone::Battlefield);
        assert_ne!(physical(&broken, target).id(), old);
        assert_eq!(physical(&broken, target).controller(), Some(B));
    }
    let returned = round(command(&ready, A, activate(&ready, 1, 2, &[])).into_state()).into_state();
    assert_eq!(card_zone(&returned, 5), ObjectZone::Battlefield);
    assert_eq!(card_zone(&returned, 6), ObjectZone::Graveyard(A));
    assert_eq!(card_zone(&returned, 7), ObjectZone::Graveyard(B));
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_narset_parter_limits_each_opponents_draws_by_turn_and_finds_only_noncreature_nonland_cards()
{
    use ManaType::*;
    let price = [(Colourless, 1), (Blue, 2)];
    let cards = vec![
        named("Narset, Parter of Veils", 1, A, ObjectDestination::Hand),
        seed(
            2,
            A,
            ObjectDestination::Hand,
            free_instant(
                BodyTarget::None,
                crate::keyword_creatures::oracle_lowering::toolbox_emit::draw_controller(
                    spellwake_engine::program::ValueExpr::Literal(2.into()),
                ),
            ),
        ),
        seed(
            3,
            B,
            ObjectDestination::Hand,
            free_instant(
                BodyTarget::None,
                crate::keyword_creatures::oracle_lowering::toolbox_emit::draw_controller(
                    spellwake_engine::program::ValueExpr::Literal(2.into()),
                ),
            ),
        ),
        seed(
            4,
            C,
            ObjectDestination::Hand,
            free_instant(
                BodyTarget::None,
                crate::keyword_creatures::oracle_lowering::toolbox_emit::draw_controller(
                    spellwake_engine::program::ValueExpr::Literal(2.into()),
                ),
            ),
        ),
        seed(
            5,
            B,
            ObjectDestination::Hand,
            free_instant(
                BodyTarget::None,
                crate::keyword_creatures::oracle_lowering::toolbox_emit::draw_controller(
                    spellwake_engine::program::ValueExpr::Literal(2.into()),
                ),
            ),
        ),
        named("Plains", 10, A, ObjectDestination::LibraryBottom),
        named("Sol Ring", 11, A, ObjectDestination::LibraryBottom),
        named("Elvish Mystic", 12, A, ObjectDestination::LibraryBottom),
        named("Island", 13, A, ObjectDestination::LibraryBottom),
        named("Forest", 14, A, ObjectDestination::LibraryBottom),
        named("Island", 20, B, ObjectDestination::LibraryBottom),
        named("Island", 21, B, ObjectDestination::LibraryBottom),
        named("Island", 22, B, ObjectDestination::LibraryBottom),
        named("Island", 30, C, ObjectDestination::LibraryBottom),
        named("Island", 31, C, ObjectDestination::LibraryBottom),
    ];
    let initial = game(vec![A, B, C], cards, &price, Step::PrecombatMain);
    let ready = round(cast_untargeted(&initial, 1, &price).into_state()).into_state();
    look_four(&ready, 1, 11, 10);
    let own = round(cast_untargeted(&ready, 2, &[]).into_state()).into_state();
    assert_eq!(card_zone(&own, 10), ObjectZone::Hand(A));
    assert_eq!(card_zone(&own, 11), ObjectZone::Hand(A));
    let mut s = own;
    for (seat, id, first, second) in [(B, 3, 20, 21), (C, 4, 30, 31)] {
        while !matches!(s.execution().window().phase(),WindowPhase::Priority(PriorityPhase::Ready {holder}) if holder==seat)
        {
            s = pass(&s).into_state();
        }
        let cast = command(
            &s,
            seat,
            GameCommand::CastUntargeted {
                spell: physical(&s, id).id(),
                payment: None,
            },
        )
        .into_state();
        s = round(cast).into_state();
        assert_eq!(card_zone(&s, first), ObjectZone::Hand(seat));
        assert_eq!(card_zone(&s, second), ObjectZone::Library(seat));
    }
    let next = at_main(s, 2);
    assert_eq!(card_zone(&next, 21), ObjectZone::Hand(B));
    let capped = round(
        command(
            &next,
            B,
            GameCommand::CastUntargeted {
                spell: physical(&next, 5).id(),
                payment: None,
            },
        )
        .into_state(),
    )
    .into_state();
    assert_eq!(card_zone(&capped, 22), ObjectZone::Library(B));
}

fn choose(s: &GameState, ids: &[u64]) -> DecisionReply {
    DecisionReply::SelectResolution(ids.iter().map(|&n| physical(s, n).id()).collect())
}
fn target_ability(s: &GameState, index: u64, target: TargetRef) -> GameCommand {
    GameCommand::ActivateAbility {
        source: physical(s, 1).id(),
        ability: ActivatedAbilityId(index),
        target,
        payment: None,
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_daretti_discards_the_chosen_number_trades_an_artifact_and_delays_returns_until_the_next_end_step()
 {
    use ManaType::*;
    verify_commander_deck("Daretti, Scrap Savant", "Mountain");
    let mut cards = vec![
        seed(2, A, battlefield(A), Program::artifact(None, vec![])),
        seed(3, A, battlefield(A), Program::artifact(None, vec![])),
        seed(
            4,
            A,
            ObjectDestination::Graveyard,
            Program::artifact(None, vec![]),
        ),
        seed(
            5,
            B,
            ObjectDestination::Graveyard,
            Program::artifact(None, vec![]),
        ),
        seed(
            6,
            A,
            ObjectDestination::Hand,
            Program::artifact(None, vec![]),
        ),
        seed(
            7,
            A,
            ObjectDestination::Hand,
            Program::artifact(None, vec![]),
        ),
        seed(
            8,
            A,
            ObjectDestination::Hand,
            free_instant(
                BodyTarget::SinglePermanent,
                crate::keyword_creatures::oracle_lowering::toolbox_emit::zone_op(
                    spellwake_engine::toolbox::ZoneOpSpec::move_all(
                        spellwake_engine::toolbox::ObjectQuery::Targets,
                        spellwake_engine::toolbox::ZoneDest::Graveyard {
                            semantics: spellwake_engine::toolbox::GraveyardSemantics::Destroy {
                                allow_regenerate: true,
                            },
                        },
                    ),
                ),
            ),
        ),
        seed(9, B, battlefield(B), Program::artifact(None, vec![])),
    ];
    for n in 10..13 {
        cards.push(seed(
            n,
            A,
            ObjectDestination::LibraryBottom,
            Program::artifact(None, vec![]),
        ));
    }
    let ready = with_loyalty_for_ultimate(
        "Daretti, Scrap Savant",
        &[(Colourless, 3), (Red, 1)],
        3,
        10,
        cards,
    );
    let offer = round(command(&ready, A, activate(&ready, 1, 0, &[])).into_state()).into_state();
    for selected in [&[][..], &[6][..], &[6, 7][..]] {
        let mut done = reply(&offer, choose(&offer, selected)).into_state();
        if matches!(
            done.pending_decision().map(|p| p.frame()),
            Some(RuleFrame::ResolutionGraveyardOrder(_))
        ) {
            done = reply(
                &done,
                DecisionReply::OrderResolutionGraveyard(
                    selected
                        .iter()
                        .map(|&id| physical(&done, id).id())
                        .collect(),
                ),
            )
            .into_state();
        }
        for (i, n) in [6, 7].into_iter().enumerate() {
            assert_eq!(
                card_zone(&done, n),
                if selected.contains(&n) {
                    ObjectZone::Graveyard(A)
                } else {
                    ObjectZone::Hand(A)
                }
            );
            assert_eq!(
                card_zone(&done, 10 + i as u64),
                if i < selected.len() {
                    ObjectZone::Hand(A)
                } else {
                    ObjectZone::Library(A)
                }
            );
        }
    }
    illegal(&ready, A, target_ability(&ready, 1, permanent(&ready, 5)));
    let sac =
        round(command(&ready, A, target_ability(&ready, 1, permanent(&ready, 4))).into_state())
            .into_state();
    let back = reply(&sac, choose(&sac, &[2])).into_state();
    assert_eq!(card_zone(&back, 2), ObjectZone::Graveyard(A));
    assert_eq!(card_zone(&back, 4), ObjectZone::Battlefield);
    let emblem = round(command(&ready, A, activate(&ready, 1, 2, &[])).into_state()).into_state();
    assert_eq!(card_zone(&emblem, 1), ObjectZone::Graveyard(A));
    for (id, owner) in [(2, A), (9, B)] {
        let dead = round(targeted(&emblem, A, 8, permanent(&emblem, id)).into_state()).into_state();
        assert_eq!(card_zone(&dead, id), ObjectZone::Graveyard(owner));
        let before_end = exile_hand::until(dead, |s| {
            s.turn().unwrap().position().step == Step::PostcombatMain
        });
        assert_eq!(card_zone(&before_end, id), ObjectZone::Graveyard(owner));
        let after = at_main(before_end, 2);
        assert_eq!(
            card_zone(&after, id),
            if owner == A {
                ObjectZone::Battlefield
            } else {
                ObjectZone::Graveyard(B)
            }
        );
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_nahiri_harbinger_loots_exiles_only_printed_targets_and_returns_its_hasty_library_permanent()
{
    use ManaType::*;
    use spellwake_engine::effects::OptionalEffectChoice;
    let cards = vec![
        named("Sol Ring", 2, A, battlefield(A)),
        creature(3, B, 2, 3, &[]),
        named("Elvish Mystic", 8, A, battlefield(A)),
        seed(4, B, battlefield(B), Program::enchantment(None)),
        seed(
            5,
            A,
            ObjectDestination::Hand,
            Program::artifact(None, vec![]),
        ),
        seed(
            6,
            A,
            ObjectDestination::Hand,
            Program::artifact(None, vec![]),
        ),
        named("Elvish Mystic", 10, A, ObjectDestination::LibraryBottom),
        named("Sol Ring", 11, A, ObjectDestination::LibraryBottom),
        named("Island", 12, A, ObjectDestination::LibraryBottom),
    ];
    let ready = with_loyalty_for_ultimate(
        "Nahiri, the Harbinger",
        &[(Colourless, 2), (Red, 1), (White, 1)],
        4,
        8,
        cards,
    );
    let loot = round(command(&ready, A, activate(&ready, 1, 0, &[])).into_state()).into_state();
    let skipped = reply(
        &loot,
        DecisionReply::OptionalEffect(OptionalEffectChoice::Decline),
    )
    .into_state();
    assert_eq!(card_zone(&skipped, 10), ObjectZone::Library(A));
    let take = reply(
        &loot,
        DecisionReply::OptionalEffect(OptionalEffectChoice::Take),
    )
    .into_state();
    let drew = reply(&take, choose(&take, &[5])).into_state();
    assert_eq!(card_zone(&drew, 5), ObjectZone::Graveyard(A));
    assert_eq!(card_zone(&drew, 10), ObjectZone::Hand(A));
    for n in [2, 3] {
        illegal(&ready, A, target_ability(&ready, 1, permanent(&ready, n)));
    }
    let tapped = tap_source(&ready, 2);
    for n in [2, 4] {
        let out = round(
            command(
                &tapped,
                A,
                target_ability(&tapped, 1, permanent(&tapped, n)),
            )
            .into_state(),
        )
        .into_state();
        assert_eq!(card_zone(&out, n), ObjectZone::Exile);
    }
    let next = at_main(ready.clone(), 4);
    let tapped_creature = tap_source(&next, 8);
    let exiled = round(
        command(
            &tapped_creature,
            A,
            target_ability(&tapped_creature, 1, permanent(&tapped_creature, 8)),
        )
        .into_state(),
    )
    .into_state();
    assert_eq!(card_zone(&exiled, 8), ObjectZone::Exile);
    let search = round(command(&ready, A, activate(&ready, 1, 2, &[])).into_state()).into_state();
    let search = entropy(search);
    let offer = |id: Option<u64>| {
        DecisionReply::LibraryTop(spellwake_engine::effects::LibraryTopReply::Search {
            selected: id.map(|n| physical(&search, n).id()),
        })
    };
    invalid(&search, A, offer(Some(12)));
    let failed = reply(&search, offer(None));
    assert_eq!(card_zone(failed.state(), 10), ObjectZone::Library(A));
    assert_eq!(failed.shuffle_journal().len(), 1);
    for n in [10, 11] {
        let found = reply(&search, offer(Some(n)));
        assert_eq!(found.shuffle_journal().len(), 1);
        assert_eq!(card_zone(found.state(), n), ObjectZone::Battlefield);
        let view = found
            .state()
            .execution()
            .current_characteristics(physical(found.state(), n).id())
            .unwrap()
            .unwrap();
        assert!(view.has_keyword(KeywordAbility::Haste));
        let after = at_main(found.into_state(), 2);
        assert_eq!(card_zone(&after, n), ObjectZone::Hand(A));
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_narset_ancient_way_uses_restricted_mana_creates_reflexive_nonland_damage_and_rewards_only_noncreature_spells()
 {
    use ManaType::*;
    use spellwake_engine::effects::OptionalEffectChoice;
    use spellwake_engine::mana::{ManaRestriction, RestrictedManaSpend};
    let ready = with_loyalty_for_ultimate(
        "Narset of the Ancient Way",
        &[(Colourless, 1), (Blue, 1), (Red, 1), (White, 1)],
        4,
        6,
        vec![
            named("Sol Ring", 2, A, ObjectDestination::Hand),
            named("Elvish Mystic", 3, A, ObjectDestination::Hand),
            named("Plains", 4, A, ObjectDestination::Hand),
            creature(5, B, 1, 1, &[]),
            seed(
                6,
                A,
                ObjectDestination::Hand,
                Program::artifact(Some(ManaCost::free()), vec![]),
            ),
            named("Ornithopter", 7, A, ObjectDestination::Hand),
            named("Island", 8, A, ObjectDestination::LibraryBottom),
        ],
    );
    let colors = round(command(&ready, A, activate(&ready, 1, 0, &[])).into_state()).into_state();
    assert_eq!(colors.execution().life_total(A), Some(&22.into()));
    invalid(&colors, A, DecisionReply::ManaColor(Green));
    for c in [Blue, Red, White] {
        let mana = reply(&colors, DecisionReply::ManaColor(c)).into_state();
        let restriction = ManaRestriction::NoncreatureSpell;
        assert_eq!(
            mana.execution()
                .restricted_mana()
                .get(&(A, c, restriction.clone())),
            Some(&1.into())
        );
        let cast = |id| GameCommand::CastWithResources {
            spell: physical(&mana, id).id(),
            target: None,
            payment: CastingPayment {
                mana: Some(spend(&[(c, 1)])),
                restricted_mana: vec![RestrictedManaSpend {
                    mana_type: c,
                    restriction: restriction.clone(),
                    amount: 1.into(),
                }],
                ..Default::default()
            },
        };
        let ok = round(command(&mana, A, cast(2)).into_state()).into_state();
        assert_eq!(card_zone(&ok, 2), ObjectZone::Battlefield);
    }
    let loot = round(command(&ready, A, activate(&ready, 1, 1, &[])).into_state()).into_state();
    assert_eq!(card_zone(&loot, 8), ObjectZone::Hand(A));
    let no = reply(
        &loot,
        DecisionReply::OptionalEffect(OptionalEffectChoice::Decline),
    )
    .into_state();
    assert!(no.execution().window().entries().is_empty());
    let take = reply(
        &loot,
        DecisionReply::OptionalEffect(OptionalEffectChoice::Take),
    )
    .into_state();
    let land = reply(&take, choose(&take, &[4])).into_state();
    assert_eq!(card_zone(&land, 4), ObjectZone::Graveyard(A));
    assert!(land.execution().window().entries().is_empty());
    let aim = reply(&take, choose(&take, &[2])).into_state();
    invalid(&aim, A, DecisionReply::TriggerTarget(TargetRef::Player(B)));
    let stacked = reply(&aim, DecisionReply::TriggerTarget(permanent(&aim, 5))).into_state();
    let damaged = round(stacked).into_state();
    assert_eq!(card_zone(&damaged, 5), ObjectZone::Graveyard(B));
    let emblem = round(command(&ready, A, activate(&ready, 1, 2, &[])).into_state()).into_state();
    assert_eq!(card_zone(&emblem, 1), ObjectZone::Graveyard(A));
    let creature = cast_untargeted(&emblem, 7, &[]).into_state();
    assert_eq!(creature.execution().window().entries().len(), 1);
    assert!(creature.pending_decision().is_none());
    let offer = cast_untargeted(&emblem, 6, &[]).into_state();
    let trigger = reply(&offer, DecisionReply::TriggerTarget(TargetRef::Player(C))).into_state();
    let damage = round(trigger).into_state();
    assert_eq!(damage.execution().life_total(C), Some(&18.into()));
    assert_eq!(card_zone(&damage, 6), ObjectZone::Stack);
}
