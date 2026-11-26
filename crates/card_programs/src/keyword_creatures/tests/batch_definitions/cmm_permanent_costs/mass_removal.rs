//! Source-bound mass destruction, exile, filtering and coin-flip outcomes.
use super::*;
use spellwake_engine::{characteristics::KeywordAbility::Indestructible, mana::ManaAbilityRef};

fn finish_owned_order(mut state: GameState) -> GameState {
    while let Some(RuleFrame::ResolutionGraveyardOrder(order)) =
        state.pending_decision().map(|p| p.frame())
    {
        state = reply(
            &state,
            DecisionReply::OrderResolutionGraveyard(order.domain().to_vec()),
        )
        .into_state();
    }
    state
}
#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_boompile_disk_and_perilous_vault_apply_their_distinct_tap_costs_and_removal_to_each_permanent_type()
 {
    for (name, price) in [
        ("Boompile", 0),
        ("Nevinyrral's Disk", 1),
        ("Perilous Vault", 5),
    ] {
        let mut cards = draw_pile();
        cards.extend([
            named(name, 1, A, ObjectDestination::Hand),
            creature(2, A, 1, 3, &[]),
            creature(3, B, 1, 3, &[Indestructible]),
            seed(
                4,
                B,
                ObjectDestination::Battlefield { controller: B },
                Program::enchantment(None),
            ),
            seed(
                5,
                B,
                ObjectDestination::Battlefield { controller: B },
                Program::artifact(None, vec![]),
            ),
            named(
                "Forest",
                6,
                B,
                ObjectDestination::Battlefield { controller: B },
            ),
            named("Elspeth, Sun's Champion", 7, A, ObjectDestination::Hand),
            named(
                "Wastes",
                8,
                A,
                ObjectDestination::Battlefield { controller: A },
            ),
        ]);
        let initial = game(
            vec![A, B],
            cards,
            &[(Colourless, 8 + price), (White, 2)],
            Step::PrecombatMain,
        );
        let walker =
            round(cast_untargeted(&initial, 7, &[(Colourless, 4), (White, 2)]).into_state())
                .into_state();
        let placed =
            round(cast_untargeted(&walker, 1, &[(Colourless, 4)]).into_state()).into_state();
        assert_eq!(
            physical(&placed, 1).tapped(),
            Some(name == "Nevinyrral's Disk")
        );
        let ready = if name == "Nevinyrral's Disk" {
            illegal(&placed, A, activation(&placed, 1, 0, &[(Colourless, 1)]));
            let turn = exile_hand::until(placed, |s| {
                s.turn().unwrap().position().number == 3
                    && s.turn().unwrap().position().step == Step::PrecombatMain
            });
            command(
                &turn,
                A,
                GameCommand::ActivateMana {
                    source: physical(&turn, 8).id(),
                    ability: ManaAbilityRef::Printed(spellwake_engine::mana::ManaAbilityId(0)),
                },
            )
            .into_state()
        } else {
            placed
        };
        let payment = if price == 0 {
            vec![]
        } else {
            vec![(Colourless, price)]
        };
        let paid = command(&ready, A, activation(&ready, 1, 0, &payment)).into_state();
        let exile = name == "Perilous Vault";
        assert_eq!(
            physical(&paid, 1).zone(),
            if exile {
                ObjectZone::Exile
            } else {
                ObjectZone::Battlefield
            }
        );
        for coin in if name == "Boompile" {
            vec![0, 1]
        } else {
            vec![1]
        } {
            let random = if name == "Boompile" {
                paid.supply_entropy(paid.basis(), vec![coin])
                    .unwrap()
                    .into_state()
            } else {
                paid.clone()
            };
            let done = finish_owned_order(round(random).into_state());
            for (number, owner, removed) in [
                (1, A, true),
                (2, A, true),
                (3, B, exile),
                (4, B, true),
                (5, B, true),
                (6, B, false),
                (7, A, name != "Nevinyrral's Disk"),
                (8, A, false),
            ] {
                let zone = if coin == 0 || !removed {
                    ObjectZone::Battlefield
                } else if exile {
                    ObjectZone::Exile
                } else {
                    ObjectZone::Graveyard(owner)
                };
                assert_eq!(
                    physical(&done, number).zone(),
                    zone,
                    "{name}, card {number}, coin {coin}"
                );
            }
        }
    }
}
#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_culling_ritual_destroys_only_nonlands_at_mana_value_two_or_less_and_divides_mana_for_only_the_destroyed_objects()
 {
    let price = [(Colourless, 2), (Black, 1), (Green, 1)];
    let mut protected = Program::artifact(fixed_cost("{2}"), vec![]);
    protected.printed_keywords.insert(Indestructible);
    let cards = vec![
        named("Culling Ritual", 1, A, ObjectDestination::Hand),
        creature(2, A, 2, 2, &[]),
        seed(
            3,
            B,
            ObjectDestination::Battlefield { controller: B },
            Program::artifact(fixed_cost("{2}"), vec![]),
        ),
        seed(
            4,
            B,
            ObjectDestination::Battlefield { controller: B },
            Program::enchantment(fixed_cost("{1}")),
        ),
        seed(
            5,
            A,
            ObjectDestination::Battlefield { controller: A },
            protected,
        ),
        seed(
            6,
            A,
            ObjectDestination::Battlefield { controller: A },
            Program::artifact(fixed_cost("{3}"), vec![]),
        ),
        named(
            "Forest",
            7,
            A,
            ObjectDestination::Battlefield { controller: A },
        ),
    ];
    let start = game(vec![A, B], cards, &price, Step::PrecombatMain);
    let ask =
        finish_owned_order(round(cast_untargeted(&start, 1, &price).into_state()).into_state());
    for (n, owner) in [(2, A), (3, B), (4, B)] {
        assert_eq!(physical(&ask, n).zone(), ObjectZone::Graveyard(owner));
    }
    for n in 5..=7 {
        assert_eq!(physical(&ask, n).zone(), ObjectZone::Battlefield);
    }
    let RuleFrame::ResolutionManaColor(choice) = ask.pending_decision().unwrap().frame() else {
        panic!("split three mana")
    };
    assert_eq!(choice.split(), Some((Black, Green, &3.into())));
    for black in 0..=3 {
        let done = reply(&ask, DecisionReply::SplitMana(black.into())).into_state();
        assert_eq!(done.execution().mana(A, Black), black.into());
        assert_eq!(done.execution().mana(A, Green), (3 - black).into());
    }
}
#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_wakening_suns_avatar_destroys_nondinosaurs_only_when_entering_after_a_hand_cast() {
    for cast in [true, false] {
        let dino = current_catalogue().1.resolve_label("Dinosaur").unwrap();
        let price = if cast {
            vec![(Colourless, 5), (White, 3)]
        } else {
            vec![(Colourless, 2), (Black, 2)]
        };
        let cards = vec![
            named(
                "Wakening Sun's Avatar",
                1,
                A,
                if cast {
                    ObjectDestination::Hand
                } else {
                    ObjectDestination::Graveyard
                },
            ),
            creature(2, A, 1, 3, &[]),
            creature(3, B, 1, 3, &[]),
            seed(
                4,
                B,
                ObjectDestination::Battlefield { controller: B },
                Program::creature(None, 2.into(), 2.into(), [dino].into()),
            ),
            named("Dread Return", 5, A, ObjectDestination::Hand),
        ];
        let opening = game(vec![A, B], cards, &price, Step::PrecombatMain);
        let spell = if cast {
            cast_untargeted(&opening, 1, &price).into_state()
        } else {
            command(
                &opening,
                A,
                GameCommand::CastPaid {
                    spell: physical(&opening, 5).id(),
                    target: TargetRef::Object(physical(&opening, 1).id()),
                    payment: spend(&price),
                },
            )
            .into_state()
        };
        let entered = round(spell).into_state();
        assert_eq!(
            entered.execution().window().entries().len(),
            usize::from(cast)
        );
        let done = if cast {
            finish_owned_order(round(entered).into_state())
        } else {
            entered
        };
        for (n, owner) in [(2, A), (3, B)] {
            assert_eq!(
                physical(&done, n).zone(),
                if cast {
                    ObjectZone::Graveyard(owner)
                } else {
                    ObjectZone::Battlefield
                }
            );
        }
        assert_eq!(physical(&done, 1).zone(), ObjectZone::Battlefield);
        assert_eq!(physical(&done, 4).zone(), ObjectZone::Battlefield);
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_ancient_stone_idol_flashes_in_for_less_as_opponents_creatures_attack_and_dies_into_a_colorless_trampling_construct()
 {
    use spellwake_engine::{
        characteristics::KeywordAbility::{Flash, Haste, Trample},
        mana::{ManaAbility, ManaAbilityId, ManaAbilityRef, ManaOutput},
    };
    let mut rock = Program::artifact(None, vec![]);
    rock.mana.push(ManaAbility {
        id: ManaAbilityId(0),
        output: ManaOutput::Fixed([(Colourless, 10.into())].into()),
    });
    let mut cards = draw_pile();
    cards.extend([
        named("Ancient Stone Idol", 1, A, ObjectDestination::Hand),
        creature(2, B, 1, 1, &[Haste]),
        creature(3, B, 1, 1, &[Haste]),
        seed(4, A, ObjectDestination::Battlefield { controller: A }, rock),
        remove_spell(5, A),
    ]);
    let opening = game(vec![A, B], cards, &[], Step::PrecombatMain);
    let fund = |s: &GameState| {
        command(
            s,
            A,
            GameCommand::ActivateMana {
                source: physical(s, 4).id(),
                ability: ManaAbilityRef::Printed(ManaAbilityId(0)),
            },
        )
        .into_state()
    };
    let funded = fund(&opening);
    illegal(
        &funded,
        A,
        GameCommand::CastUntargeted {
            spell: physical(&funded, 1).id(),
            payment: Some(spend(&[(Colourless, 8)])),
        },
    );
    let full = round(cast_untargeted(&funded, 1, &[(Colourless, 10)]).into_state()).into_state();
    assert_eq!(physical(&full, 1).zone(), ObjectZone::Battlefield);
    let attacks = exile_hand::until(opening, |s| {
        s.turn().unwrap().position().number == 2
            && matches!(
                s.pending_decision().map(|p| p.frame()),
                Some(RuleFrame::Combat(CombatDecision::Attackers(_)))
            )
    });
    let declared = reply(
        &attacks,
        DecisionReply::DeclareAttackers(
            [2, 3]
                .into_iter()
                .map(|n| AttackDeclaration {
                    attacker: physical(&attacks, n).id(),
                    defender: A,
                    defending_planeswalker: None,
                })
                .collect(),
        ),
    )
    .into_state();
    let ours = pass(&declared).into_state();
    let paid = fund(&ours);
    let flashing = cast_untargeted(&paid, 1, &[(Colourless, 8)]).into_state();
    let board = round(flashing).into_state();
    let id = physical(&board, 1).id();
    assert!(sheet(&board, id).has_keyword(Flash));
    assert!(sheet(&board, id).has_keyword(Trample));
    assert_size(&board, id, 12, 12);
    let priority = pass(&board).into_state();
    let died = round(targeted(&priority, A, 5, TargetRef::Object(id)).into_state()).into_state();
    let (made, tokens) = ordered_tokens(died, 1);
    let child = sheet(&made, tokens[0]);
    assert!(child.has_keyword(Trample));
    assert!(child.has_type(CardType::Artifact));
    assert!(child.current_colors.is_empty());
    assert_eq!(
        child.current_creature_subtypes,
        [current_catalogue().1.resolve_label("Construct").unwrap()].into()
    );
    assert_size(&made, tokens[0], 6, 12);
}
#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_avacyn_is_flying_vigilant_and_indestructible_and_protects_other_allied_permanents_until_she_leaves()
 {
    use spellwake_engine::characteristics::KeywordAbility::{Flying, Vigilance};
    let price = [(Colourless, 5), (White, 3)];
    let mut cards = draw_pile();
    cards.extend([
        named("Avacyn, Angel of Hope", 1, A, ObjectDestination::Hand),
        creature(2, A, 1, 1, &[]),
        named(
            "Forest",
            3,
            A,
            ObjectDestination::Battlefield { controller: A },
        ),
        creature(4, B, 2, 3, &[]),
        seed(
            5,
            A,
            ObjectDestination::Hand,
            free_instant(BodyTarget::SinglePermanent, Instruction::DestroyTarget),
        ),
        seed(
            6,
            A,
            ObjectDestination::Hand,
            free_instant(
                BodyTarget::SingleCreature,
                crate::keyword_creatures::oracle_lowering::toolbox_emit::exile_target(),
            ),
        ),
    ]);
    let start = game(vec![A, B], cards, &price, Step::PrecombatMain);
    let field = round(cast_untargeted(&start, 1, &price).into_state()).into_state();
    let source = sheet(&field, physical(&field, 1).id());
    assert!(source.has_keyword(Flying));
    assert!(source.has_keyword(Vigilance));
    assert!(source.has_keyword(Indestructible));
    for n in [1, 2, 3, 4] {
        assert_eq!(
            sheet(&field, physical(&field, n).id()).has_keyword(Indestructible),
            n != 4
        );
        let removed =
            round(targeted(&field, A, 5, TargetRef::Object(physical(&field, n).id())).into_state())
                .into_state();
        assert_eq!(
            physical(&removed, n).zone(),
            if n == 4 {
                ObjectZone::Graveyard(B)
            } else {
                ObjectZone::Battlefield
            }
        );
    }
    let exiled =
        round(targeted(&field, A, 6, TargetRef::Object(physical(&field, 1).id())).into_state())
            .into_state();
    assert_eq!(physical(&exiled, 1).zone(), ObjectZone::Exile);
    for n in [2, 3] {
        assert!(!sheet(&exiled, physical(&exiled, n).id()).has_keyword(Indestructible));
    }
}
#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_spatial_contortion_requires_actual_colorless_mana_modifies_power_and_toughness_and_can_kill_by_zero_toughness()
 {
    let cards = vec![
        named("Spatial Contortion", 1, A, ObjectDestination::Hand),
        creature(2, A, 2, 4, &[]),
        creature(3, B, 4, 3, &[Indestructible]),
    ];
    let field = game(
        vec![A, B],
        cards,
        &[(Blue, 2), (Colourless, 1)],
        Step::PrecombatMain,
    );
    let cast = |n, price: &[(ManaType, i64)]| GameCommand::CastPaid {
        spell: physical(&field, 1).id(),
        target: TargetRef::Object(physical(&field, n).id()),
        payment: spend(price),
    };
    illegal(&field, A, cast(2, &[(Blue, 2)]));
    let alive =
        round(command(&field, A, cast(2, &[(Blue, 1), (Colourless, 1)])).into_state()).into_state();
    assert_size(&alive, physical(&alive, 2).id(), 5, 1);
    let expired = exile_hand::until(alive, |s| s.turn().unwrap().position().number == 2);
    assert_size(&expired, physical(&expired, 2).id(), 2, 4);
    let dead =
        round(command(&field, A, cast(3, &[(Blue, 1), (Colourless, 1)])).into_state()).into_state();
    assert_eq!(physical(&dead, 3).zone(), ObjectZone::Graveyard(B));
}
