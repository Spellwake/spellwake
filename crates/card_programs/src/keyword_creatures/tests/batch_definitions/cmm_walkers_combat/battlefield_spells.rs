//! Commander Masters spells that change the whole battlefield or turn schedule.
use super::declarations::{attacks, combat_on};
use super::equipment::{cast_card, marker, sheet, stats};
use super::*;
mod conditional_spells;

fn main_step(state: GameState, turn: u64) -> GameState {
    exile_hand::until(state, |s| {
        s.turn().unwrap().position().number == turn
            && s.turn().unwrap().position().step == Step::PrecombatMain
    })
}
fn multiple(state: &GameState, spell: u64, ids: &[u64], price: &[(ManaType, i64)]) -> GameCommand {
    GameCommand::CastWithResources {
        spell: physical(state, spell).id(),
        target: None,
        payment: CastingPayment {
            mana: Some(spend(price)),
            permanent_targets: ids.iter().map(|&id| marker(state, id)).collect(),
            ..Default::default()
        },
    }
}
#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_capture_of_jingzhou_inserts_one_controller_turn_before_the_next_opponents_turn_and_normal_order_resumes()
 {
    use ManaType::*;
    let price = [(Colourless, 3), (Blue, 2)];
    let mut cards = vec![named("Capture of Jingzhou", 1, A, ObjectDestination::Hand)];
    for (n, seat) in [(10, A), (11, A), (12, B), (13, C)] {
        cards.push(named("Island", n, seat, ObjectDestination::LibraryBottom));
    }
    let s = game(vec![A, B, C], cards, &price, Step::PrecombatMain);
    let resolved = cast_card(&s, 1, &price);
    let extra = main_step(resolved, 2);
    assert_eq!(extra.execution().window().active_player(), A);
    assert_eq!(physical(&extra, 10).zone(), ObjectZone::Hand(A));
    assert_eq!(physical(&extra, 12).zone(), ObjectZone::Library(B));
    let next = main_step(extra, 3);
    assert_eq!(next.execution().window().active_player(), B);
    assert_eq!(physical(&next, 12).zone(), ObjectZone::Hand(B));
    let last = main_step(next, 4);
    assert_eq!(last.execution().window().active_player(), C);
}
#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_aether_gale_validates_six_distinct_nonland_targets_and_evacuation_returns_all_creatures_to_their_owners()
 {
    use ManaType::*;
    let price = [(Colourless, 3), (Blue, 2)];
    for mass in [false, true] {
        let name = if mass { "Evacuation" } else { "Aether Gale" };
        let mut lent = creature(2, A, 1, 3, &[]);
        lent.destination = battlefield(B);
        let cards = vec![
            named(name, 1, A, ObjectDestination::Hand),
            lent,
            creature(3, B, 2, 2, &[]),
            creature(4, C, 3, 4, &[]),
            seed(5, A, battlefield(A), Program::enchantment(None)),
            named("Sol Ring", 6, B, battlefield(B)),
            creature(7, A, 1, 2, &[]),
            creature(8, C, 1, 1, &[]),
            named("Plains", 9, C, battlefield(C)),
            super::super::cmm_gameplay::remove_spell(10, A),
        ];
        let before = game(vec![A, B, C], cards, &price, Step::PrecombatMain);
        let out = if mass {
            cast_card(&before, 1, &price)
        } else {
            for invalid in [
                &[2, 3, 4, 5, 6][..],
                &[2, 3, 4, 5, 6, 6][..],
                &[2, 3, 4, 5, 6, 9][..],
            ] {
                illegal(&before, A, multiple(&before, 1, invalid, &price));
            }
            let stack = command(
                &before,
                A,
                multiple(&before, 1, &[2, 3, 4, 5, 6, 7], &price),
            )
            .into_state();
            let gone = round(targeted(&stack, A, 10, marker(&stack, 3)).into_state()).into_state();
            let partial = round(gone).into_state();
            assert_eq!(physical(&partial, 3).zone(), ObjectZone::Graveyard(B));
            assert_eq!(physical(&partial, 2).zone(), ObjectZone::Hand(A));
            round(stack).into_state()
        };
        for (n, owner) in [(2, A), (3, B), (4, C), (7, A)] {
            assert_eq!(physical(&out, n).zone(), ObjectZone::Hand(owner));
        }
        for (n, owner) in [(5, A), (6, B)] {
            assert_eq!(
                physical(&out, n).zone(),
                if mass {
                    ObjectZone::Battlefield
                } else {
                    ObjectZone::Hand(owner)
                }
            );
        }
        assert_eq!(
            physical(&out, 8).zone(),
            if mass {
                ObjectZone::Hand(C)
            } else {
                ObjectZone::Battlefield
            }
        );
        assert_eq!(physical(&out, 9).zone(), ObjectZone::Battlefield);
    }
}
#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_heroic_intervention_protects_only_current_controlled_permanents_against_opponents_and_expires_at_cleanup()
 {
    use ManaType::*;
    let price = [(Colourless, 1), (Green, 1)];
    let cards = vec![
        named("Heroic Intervention", 1, A, ObjectDestination::Hand),
        creature(2, A, 2, 2, &[]),
        named("Sol Ring", 3, A, battlefield(A)),
        creature(4, B, 2, 2, &[]),
        super::super::cmm_gameplay::remove_spell(5, B),
        seed(
            6,
            A,
            ObjectDestination::Hand,
            free_instant(
                BodyTarget::SINGLE_PERMANENT,
                crate::keyword_creatures::toolbox_emit::zone_op(
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
        seed(
            7,
            A,
            ObjectDestination::Hand,
            Program::creature(
                Some(ManaCost::free()),
                1.into(),
                1.into(),
                Default::default(),
            ),
        ),
    ];
    let start = game(vec![A, B], cards, &price, Step::PrecombatMain);
    let protected = cast_card(&start, 1, &price);
    for n in [2, 3, 4] {
        let view = sheet(&protected, physical(&protected, n).id());
        for key in [KeywordAbility::Hexproof, KeywordAbility::Indestructible] {
            assert_eq!(view.has_keyword(key), n != 4);
        }
    }
    let opposing = pass(&protected).into_state();
    illegal(
        &opposing,
        B,
        GameCommand::Cast {
            spell: physical(&opposing, 5).id(),
            target: marker(&opposing, 2),
        },
    );
    let own = round(targeted(&protected, A, 6, marker(&protected, 3)).into_state()).into_state();
    assert_eq!(physical(&own, 3).zone(), ObjectZone::Battlefield);
    let late = cast_card(&own, 7, &[]);
    assert!(!sheet(&late, physical(&late, 7).id()).has_keyword(KeywordAbility::Indestructible));
    let next = main_step(late, 2);
    let destroyed = round(targeted(&next, B, 5, marker(&next, 2)).into_state()).into_state();
    assert_eq!(physical(&destroyed, 2).zone(), ObjectZone::Graveyard(A));
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_insurrection_untaps_all_creatures_for_one_attack_and_restores_control_and_haste_on_cleanup()
{
    use ManaType::*;
    let price = [(Colourless, 5), (Red, 3)];
    let mut mana_creature = named("Elvish Mystic", 2, A, battlefield(A));
    mana_creature
        .program
        .printed_keywords
        .insert(KeywordAbility::Haste);
    let mut rival = named("Elvish Mystic", 3, B, battlefield(B));
    rival.program.printed_keywords.insert(KeywordAbility::Haste);
    let cards = vec![
        named("Insurrection", 1, A, ObjectDestination::Hand),
        mana_creature,
        rival,
        creature(4, C, 3, 5, &[]),
        named("Sol Ring", 5, B, battlefield(B)),
    ];
    let initial = game(vec![A, B, C], cards, &price, Step::PrecombatMain);
    let tap = |s: &GameState, n, seat| {
        command(
            s,
            seat,
            GameCommand::ActivateMana {
                source: physical(s, n).id(),
                ability: ManaAbilityRef::Printed(ManaAbilityId(0)),
            },
        )
        .into_state()
    };
    let own = tap(&initial, 2, A);
    let given = pass(&own).into_state();
    let rival = tap(&given, 3, B);
    let priority = pass(&pass(&rival).into_state()).into_state();
    for n in [2, 3] {
        assert_eq!(physical(&priority, n).tapped(), Some(true));
    }
    let changed = cast_card(&priority, 1, &price);
    for n in [2, 3, 4] {
        assert_eq!(physical(&changed, n).tapped(), Some(false));
        let view = sheet(&changed, physical(&changed, n).id());
        assert_eq!(view.controller, Some(A));
        assert!(view.has_keyword(KeywordAbility::Haste));
    }
    assert_eq!(
        sheet(&changed, physical(&changed, 5).id()).controller,
        Some(B)
    );
    let combat = combat_on(changed, 1);
    let swing = reply(
        &combat,
        attacks(&combat, &[(2, B, None), (3, B, None), (4, C, None)]),
    )
    .into_state();
    let cleanup = main_step(swing, 2);
    assert_eq!(cleanup.execution().life_total(B), Some(&18.into()));
    assert_eq!(cleanup.execution().life_total(C), Some(&17.into()));
    for (n, owner) in [(2, A), (3, B), (4, C)] {
        assert_eq!(
            sheet(&cleanup, physical(&cleanup, n).id()).controller,
            Some(owner)
        );
    }
    assert!(!sheet(&cleanup, physical(&cleanup, 4).id()).has_keyword(KeywordAbility::Haste));
}
#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_feast_of_succession_weakens_creatures_across_the_battlefield_and_makes_the_caster_monarch() {
    use ManaType::*;
    let price = [(Colourless, 4), (Black, 2)];
    let start = game(
        vec![A, B, C],
        vec![
            named("Feast of Succession", 1, A, ObjectDestination::Hand),
            creature(2, A, 3, 4, &[KeywordAbility::Indestructible]),
            creature(3, B, 6, 7, &[]),
            creature(4, C, 1, 1, &[]),
            seed(
                5,
                A,
                ObjectDestination::Hand,
                Program::creature(
                    Some(ManaCost::free()),
                    2.into(),
                    2.into(),
                    Default::default(),
                ),
            ),
        ],
        &price,
        Step::PrecombatMain,
    );
    let weakened = cast_card(&start, 1, &price);
    assert_eq!(physical(&weakened, 2).zone(), ObjectZone::Graveyard(A));
    assert_eq!(physical(&weakened, 4).zone(), ObjectZone::Graveyard(C));
    stats(&weakened, physical(&weakened, 3).id(), 2, 3);
    assert_eq!(weakened.execution().monarch(), Some(A));
    let late = cast_card(&weakened, 5, &[]);
    stats(&late, physical(&late, 5).id(), 2, 2);
    let expired = main_step(late, 2);
    stats(&expired, physical(&expired, 3).id(), 6, 7);
    assert_eq!(expired.execution().monarch(), Some(A));
}
#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_blasphemous_act_reduces_only_generic_mana_for_every_creature_and_deals_thirteen_to_each() {
    use ManaType::*;
    for crowded in [false, true] {
        let mut cards = vec![
            named("Blasphemous Act", 1, A, ObjectDestination::Hand),
            creature(2, A, 1, 1, &[]),
            creature(3, B, 1, 13, &[KeywordAbility::Indestructible]),
            creature(4, C, 1, 14, &[]),
            named("Sol Ring", 5, C, battlefield(C)),
        ];
        if crowded {
            for n in 10..16 {
                cards.push(creature(n, B, 1, 16, &[]));
            }
        }
        let n = if crowded { 0 } else { 5 };
        let price = if crowded {
            vec![(Red, 1)]
        } else {
            vec![(Colourless, n), (Red, 1)]
        };
        let start = game(
            vec![A, B, C],
            cards,
            &[(Colourless, 8), (Red, 1)],
            Step::PrecombatMain,
        );
        let missing = GameCommand::CastUntargeted {
            spell: physical(&start, 1).id(),
            payment: Some(spend(&[(Colourless, n + 1)])),
        };
        illegal(&start, A, missing);
        let done = cast_card(&start, 1, &price);
        assert_eq!(done.execution().mana(A, Colourless), (8 - n).into());
        assert_eq!(physical(&done, 2).zone(), ObjectZone::Graveyard(A));
        assert_eq!(physical(&done, 5).zone(), ObjectZone::Battlefield);
        for card in [3, 4]
            .into_iter()
            .chain(if crowded { 10..16 } else { 10..10 })
        {
            assert_eq!(
                done.execution()
                    .battlefield_marked_damage(physical(&done, card).id()),
                Some(13.into())
            );
        }
    }
}
#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_curtains_call_and_sublime_exhalation_reduce_generic_cost_by_opponents_and_destroy_only_their_respective_creatures()
 {
    use ManaType::*;
    for all in [false, true] {
        for count in [1, 2] {
            let (name, printed, color) = if all {
                ("Sublime Exhalation", 6, White)
            } else {
                ("Curtains' Call", 5, Black)
            };
            let seats = if count == 1 {
                vec![A, B]
            } else {
                vec![A, B, C]
            };
            let start = game(
                seats,
                vec![
                    named(name, 1, A, ObjectDestination::Hand),
                    creature(2, A, 2, 2, &[]),
                    creature(3, B, 2, 3, &[]),
                    creature(4, B, 1, 2, &[KeywordAbility::Indestructible]),
                    named("Sol Ring", 5, B, battlefield(B)),
                ],
                &[(Colourless, printed), (color, 1)],
                Step::PrecombatMain,
            );
            let price = [(Colourless, printed - count), (color, 1)];
            let under = [(Colourless, printed - count - 1), (color, 1)];
            let cast = |s: &GameState, cost: &[(ManaType, i64)]| {
                if all {
                    GameCommand::CastUntargeted {
                        spell: physical(s, 1).id(),
                        payment: Some(spend(cost)),
                    }
                } else {
                    multiple(s, 1, &[2, 3], cost)
                }
            };
            illegal(&start, A, cast(&start, &under));
            if !all {
                for ids in [&[2][..], &[2, 2][..], &[2, 5][..]] {
                    illegal(&start, A, multiple(&start, 1, ids, &price));
                }
            }
            let done = round(command(&start, A, cast(&start, &price)).into_state()).into_state();
            assert_eq!(done.execution().mana(A, Colourless), count.into());
            for (id, owner) in [(2, A), (3, B)] {
                assert_eq!(physical(&done, id).zone(), ObjectZone::Graveyard(owner));
            }
            for id in [4, 5] {
                assert_eq!(physical(&done, id).zone(), ObjectZone::Battlefield);
            }
        }
    }
}
