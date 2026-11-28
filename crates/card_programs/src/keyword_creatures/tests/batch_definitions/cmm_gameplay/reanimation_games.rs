//! Reanimation, linked cost choices and public-zone target restrictions.
use super::*;
use spellwake_engine::effects::OptionalEffectChoice;

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_zones_dread_return_targets_an_own_creature_and_flashback_sacrifices_three_at_announcement() {
    use ManaType::*;
    let price = [(Colourless, 2), (Black, 2)];
    let mut cards = vec![
        hand("Dread Return"),
        named("Elvish Mystic", 2, A, ObjectDestination::Graveyard),
        named("Scaretiller", 3, A, ObjectDestination::Graveyard),
        named("Elvish Mystic", 4, B, ObjectDestination::Graveyard),
        named("Sol Ring", 5, A, ObjectDestination::Graveyard),
    ];
    for (n, owner) in [(6, A), (7, B), (8, C)] {
        cards.push(seed(
            n,
            owner,
            ObjectDestination::Battlefield { controller: A },
            Program::creature(None, 1.into(), 1.into(), BTreeSet::new()),
        ));
    }
    let initial = game(vec![A, B, C], cards, &price, Step::PrecombatMain);
    let cast = |s: &GameState, n, payment| GameCommand::CastWithResources {
        spell: physical(s, 1).id(),
        target: Some(TargetRef::Object(physical(s, n).id())),
        payment,
    };
    let normal = CastingPayment {
        mana: Some(spend(&price)),
        ..Default::default()
    };
    for n in [4, 5] {
        illegal(&initial, A, cast(&initial, n, normal.clone()));
    }
    let raised = round(command(&initial, A, cast(&initial, 2, normal)).into_state()).into_state();
    assert_eq!(physical(&raised, 2).zone(), ObjectZone::Battlefield);
    assert_eq!(physical(&raised, 1).zone(), ObjectZone::Graveyard(A));
    let pay = |nums: &[u64]| CastingPayment {
        objects: nums
            .iter()
            .map(|&n| CastResource::FlashbackSacrifice(physical(&raised, n).id()))
            .collect(),
        ..Default::default()
    };
    illegal(&raised, A, cast(&raised, 3, pay(&[6, 7])));
    let stacked = command(&raised, A, cast(&raised, 3, pay(&[6, 7, 8]))).into_state();
    for (n, owner) in [(6, A), (7, B), (8, C)] {
        assert_eq!(physical(&stacked, n).zone(), ObjectZone::Graveyard(owner));
    }
    let done = round(stacked);
    assert_eq!(physical(done.state(), 3).zone(), ObjectZone::Battlefield);
    assert_eq!(physical(done.state(), 1).zone(), ObjectZone::Exile);
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_zones_victimize_sacrifices_on_resolution_then_returns_the_chosen_creatures_tapped() {
    let price = [(ManaType::Colourless, 2), (ManaType::Black, 1)];
    for sacrifice in [false, true] {
        let mut cards = vec![
            hand("Victimize"),
            named("Elvish Mystic", 2, A, ObjectDestination::Graveyard),
            named("Scaretiller", 3, A, ObjectDestination::Graveyard),
            creature(4, B, 1, 1, &[]),
        ];
        if sacrifice {
            cards.push(creature(5, A, 1, 1, &[]));
        }
        let initial = game(vec![A, B], cards, &price, Step::PrecombatMain);
        let cast = |ids: &[u64]| GameCommand::CastWithResources {
            spell: physical(&initial, 1).id(),
            target: None,
            payment: CastingPayment {
                mana: Some(spend(&price)),
                permanent_targets: ids
                    .iter()
                    .map(|&n| TargetRef::Object(physical(&initial, n).id()))
                    .collect(),
                ..Default::default()
            },
        };
        illegal(&initial, A, cast(&[2]));
        illegal(&initial, A, cast(&[2, 2]));
        let stack = command(&initial, A, cast(&[2, 3])).into_state();
        if sacrifice {
            assert_eq!(physical(&stack, 5).zone(), ObjectZone::Battlefield);
        }
        let mut result = round(stack).into_state();
        while let Some(d) = result.pending_decision() {
            let choice = match d.frame() {
                RuleFrame::ResolutionSelect(_) => {
                    DecisionReply::SelectResolution(vec![physical(&result, 5).id()])
                }
                RuleFrame::LinkedReturnOrder(frame) => {
                    DecisionReply::LinkedReturnOrder(frame.objects().to_vec())
                }
                RuleFrame::TimestampOrder(frame) => DecisionReply::TimestampOrder(
                    frame.members().iter().map(|member| member.member).collect(),
                ),
                other => panic!("unexpected reanimation decision {other:?}"),
            };
            result = reply(&result, choice).into_state();
        }
        for n in [2, 3] {
            let card = physical(&result, n);
            if sacrifice {
                assert_eq!(
                    (card.zone(), card.tapped()),
                    (ObjectZone::Battlefield, Some(true))
                );
            } else {
                assert_eq!(card.zone(), ObjectZone::Graveyard(A));
            }
        }
        assert_eq!(physical(&result, 4).zone(), ObjectZone::Battlefield);
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_zones_finale_of_devastation_chooses_search_zones_and_applies_a_large_x_bonus_through_cleanup()
 {
    use ManaType::*;
    let initial = game(
        vec![A, B],
        vec![
            hand("Finale of Devastation"),
            named("Elvish Mystic", 2, A, ObjectDestination::LibraryBottom),
            named("Scaretiller", 3, A, ObjectDestination::Graveyard),
            creature(4, A, 2, 2, &[]),
            creature(5, B, 2, 2, &[]),
            named("Forest", 6, A, ObjectDestination::LibraryBottom),
        ],
        &[(Colourless, 10), (Green, 2)],
        Step::PrecombatMain,
    );
    for (x, search_library, target) in [(1_i64, true, 2), (4, false, 3), (10, true, 3)] {
        let stack = command(
            &initial,
            A,
            GameCommand::CastWithResources {
                spell: physical(&initial, 1).id(),
                target: None,
                payment: CastingPayment {
                    chosen_x: Some(x.into()),
                    mana: Some(spend(&[(Colourless, x), (Green, 2)])),
                    ..Default::default()
                },
            },
        )
        .into_state();
        let choice = round(stack).into_state();
        let search = reply(
            &choice,
            DecisionReply::OptionalEffect(if search_library {
                OptionalEffectChoice::Take
            } else {
                OptionalEffectChoice::Decline
            }),
        )
        .into_state();
        let result = if search_library {
            let ready = search
                .supply_entropy(search.basis(), vec![0; 8])
                .unwrap()
                .into_state();
            let RuleFrame::LibraryTop(frame) = ready.pending_decision().unwrap().frame() else {
                panic!("expected joint search");
            };
            assert_eq!(
                frame.eligible_results().contains(&physical(&ready, 3).id()),
                x >= 4
            );
            let result = reply(
                &ready,
                DecisionReply::LibraryTop(LibraryTopReply::Search {
                    selected: Some(physical(&ready, target).id()),
                }),
            );
            assert_eq!(result.shuffle_journal().len(), 1);
            result.into_state()
        } else {
            let result = reply(
                &search,
                DecisionReply::SelectResolution(vec![physical(&search, target).id()]),
            );
            assert!(result.shuffle_journal().is_empty());
            result.into_state()
        };
        assert_eq!(physical(&result, target).zone(), ObjectZone::Battlefield);
        let view = |s: &GameState, id| {
            s.execution()
                .current_characteristics(physical(s, id).id())
                .unwrap()
                .unwrap()
        };
        let own = view(&result, 4);
        assert_eq!(
            own.power_toughness.unwrap().derived.power,
            (if x >= 10 { 12_i64 } else { 2 }).into()
        );
        assert_eq!(own.keywords.contains(&Haste), x >= 10);
        assert_eq!(view(&result, target).keywords.contains(&Haste), x >= 10);
        assert_eq!(
            view(&result, 5).power_toughness.unwrap().derived.power,
            2.into()
        );
        let end = next_main(result, 2);
        assert!(!view(&end, 4).keywords.contains(&Haste));
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_zones_custodi_squire_orders_the_ballot_and_returns_all_tied_graveyard_winners() {
    let price = [(ManaType::Colourless, 4), (ManaType::White, 1)];
    let initial = game(
        vec![A, B, C],
        vec![
            hand("Custodi Squire"),
            named("Sol Ring", 2, A, ObjectDestination::Graveyard),
            named("Elvish Mystic", 3, A, ObjectDestination::Graveyard),
            named("Fervor", 4, A, ObjectDestination::Graveyard),
            named("Forest", 5, A, ObjectDestination::Graveyard),
            named("Sol Ring", 6, B, ObjectDestination::Graveyard),
        ],
        &price,
        Step::PrecombatMain,
    );
    let begin = round(cast_named(&initial, &price)).into_state();
    for ballots in [[2, 3, 2], [2, 3, 4]] {
        let mut state = begin.clone();
        for (voter, pick) in [A, B, C].into_iter().zip(ballots) {
            let RuleFrame::ResolutionSelect(frame) = state.pending_decision().unwrap().frame()
            else {
                panic!("expected a public ballot")
            };
            assert_eq!(frame.chooser(), voter);
            assert_eq!(
                frame.domain().iter().copied().collect::<BTreeSet<_>>(),
                [2, 3, 4].map(|n| physical(&state, n).id()).into()
            );
            state = reply(
                &state,
                DecisionReply::SelectResolution(vec![physical(&state, pick).id()]),
            )
            .into_state();
        }
        for n in [2, 3, 4] {
            let won = n == 2 || ballots[2] == 4;
            assert_eq!(
                physical(&state, n).zone(),
                if won {
                    ObjectZone::Hand(A)
                } else {
                    ObjectZone::Graveyard(A)
                }
            );
        }
        assert_eq!(physical(&state, 5).zone(), ObjectZone::Graveyard(A));
        assert_eq!(physical(&state, 6).zone(), ObjectZone::Graveyard(B));
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_zones_sevinnes_reclamation_checks_the_target_limit_and_offers_a_copy_only_from_the_graveyard()
 {
    use ManaType::*;
    use spellwake_engine::effects::CopyTargetChoice;
    for graveyard in [false, true] {
        let price = [(Colourless, if graveyard { 4 } else { 2 }), (White, 1)];
        let initial = game(
            vec![A, B],
            vec![
                named(
                    "Sevinne's Reclamation",
                    1,
                    A,
                    if graveyard {
                        ObjectDestination::Graveyard
                    } else {
                        ObjectDestination::Hand
                    },
                ),
                named("Forest", 2, A, ObjectDestination::Graveyard),
                named("Fervor", 3, A, ObjectDestination::Graveyard),
                named("Scaretiller", 4, A, ObjectDestination::Graveyard),
                named("Forest", 5, B, ObjectDestination::Graveyard),
                named("Opt", 6, A, ObjectDestination::Graveyard),
            ],
            &price,
            Step::PrecombatMain,
        );
        let cast = |n| GameCommand::CastWithResources {
            spell: physical(&initial, 1).id(),
            target: Some(TargetRef::Object(physical(&initial, n).id())),
            payment: CastingPayment {
                mana: Some(spend(&price)),
                ..Default::default()
            },
        };
        for n in [4, 5, 6] {
            illegal(&initial, A, cast(n));
        }
        let returned = round(command(&initial, A, cast(2)).into_state()).into_state();
        assert_eq!(physical(&returned, 2).zone(), ObjectZone::Battlefield);
        if graveyard {
            let declined = reply(
                &returned,
                DecisionReply::OptionalEffect(OptionalEffectChoice::Decline),
            )
            .into_state();
            assert_eq!(physical(&declined, 1).zone(), ObjectZone::Exile);
            assert_eq!(physical(&declined, 3).zone(), ObjectZone::Graveyard(A));
            let copy = reply(
                &returned,
                DecisionReply::OptionalEffect(OptionalEffectChoice::Take),
            )
            .into_state();
            let original = reply(
                &copy,
                DecisionReply::CopyTargets(CopyTargetChoice::Change(TargetRef::Object(
                    physical(&copy, 3).id(),
                ))),
            )
            .into_state();
            assert_eq!(physical(&original, 1).zone(), ObjectZone::Exile);
            let second = round(original).into_state();
            assert_eq!(physical(&second, 3).zone(), ObjectZone::Battlefield);
            assert!(second.pending_decision().is_none());
        } else {
            assert!(returned.pending_decision().is_none());
            assert_eq!(physical(&returned, 1).zone(), ObjectZone::Graveyard(A));
        }
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_zones_shipwreck_dowser_returns_an_own_instant_or_sorcery_and_gains_prowess_until_cleanup() {
    use ManaType::*;
    let price = [(Colourless, 3), (Blue, 2)];
    let effect = ExecutableBody::new(
        BodyTarget::None,
        vec![crate::keyword_creatures::toolbox_emit::gain_life(
            spellwake_engine::toolbox::PlayerSelector::Resolving(
                spellwake_engine::program::ResolvingPlayer::Controller,
            ),
            spellwake_engine::program::ValueExpr::Literal(1.into()),
        )],
    );
    let initial = game(
        vec![A, B],
        vec![
            hand("Shipwreck Dowser"),
            seed(
                2,
                A,
                ObjectDestination::Graveyard,
                Program::instant(ManaCost::free(), effect.clone()),
            ),
            seed(
                3,
                A,
                ObjectDestination::Graveyard,
                Program::sorcery(ManaCost::free(), effect.clone()),
            ),
            seed(
                4,
                B,
                ObjectDestination::Graveyard,
                Program::instant(ManaCost::free(), effect),
            ),
            named("Elvish Mystic", 5, A, ObjectDestination::Graveyard),
        ],
        &price,
        Step::PrecombatMain,
    );
    let entering = cast_named(&initial, &price);
    let choose = |n| DecisionReply::TriggerTarget(TargetRef::Object(physical(&entering, n).id()));
    for n in [4, 5] {
        illegal(&entering, A, combat_declaration(&entering, choose(n)));
    }
    for n in [2, 3] {
        let stacked = reply(&entering, choose(n)).into_state();
        let returned = round(stacked).into_state();
        assert_eq!(physical(&returned, n).zone(), ObjectZone::Hand(A));
        let power = |s: &GameState| {
            s.execution()
                .current_characteristics(physical(s, 1).id())
                .unwrap()
                .unwrap()
                .power_toughness
                .unwrap()
                .derived
        };
        let spell = cast_untargeted(&returned, n, &[]).into_state();
        let boosted = round(spell).into_state();
        assert_eq!(
            power(&boosted),
            PowerToughness {
                power: 4.into(),
                toughness: 4.into()
            }
        );
        let resolved = round(boosted).into_state();
        assert_eq!(resolved.execution().life_total(A), Some(&21.into()));
        assert_eq!(
            power(&next_main(resolved, 2)),
            PowerToughness {
                power: 3.into(),
                toughness: 3.into()
            }
        );
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_zones_stitcher_geralf_mills_everyone_and_sizes_a_blue_zombie_from_only_the_chosen_creatures()
{
    use ManaType::*;
    let price = [(Colourless, 3), (Blue, 2)];
    let monster = |n, seat, power: i64| {
        seed(
            n,
            seat,
            ObjectDestination::LibraryBottom,
            Program::creature(None, power.into(), 1.into(), BTreeSet::new()),
        )
    };
    let initial = game(
        vec![A, B],
        vec![
            hand("Stitcher Geralf"),
            named(
                "Fervor",
                2,
                A,
                ObjectDestination::Battlefield { controller: A },
            ),
            monster(3, A, 2),
            monster(4, A, 4),
            named("Forest", 5, A, ObjectDestination::LibraryBottom),
            monster(6, B, 5),
            named("Forest", 7, B, ObjectDestination::LibraryBottom),
            named("Island", 8, B, ObjectDestination::LibraryBottom),
            named("Elvish Mystic", 9, A, ObjectDestination::Graveyard),
        ],
        &[(Colourless, 5), (Blue, 3)],
        Step::PrecombatMain,
    );
    let entered = cast_named(&initial, &price);
    let activated = command(
        &entered,
        A,
        GameCommand::ActivateUntargetedAbility {
            source: physical(&entered, 1).id(),
            ability: ActivatedAbilityId(0),
            payment: Some(spend(&[(Colourless, 2), (Blue, 1)])),
        },
    )
    .into_state();
    assert_eq!(physical(&activated, 1).tapped(), Some(true));
    let mut state = round(activated).into_state();
    while let Some(RuleFrame::ResolutionGraveyardOrder(frame)) =
        state.pending_decision().map(|d| d.frame())
    {
        state = reply(
            &state,
            DecisionReply::OrderResolutionGraveyard(frame.domain().to_vec()),
        )
        .into_state();
    }
    let RuleFrame::ResolutionSelect(frame) = state.pending_decision().unwrap().frame() else {
        panic!("expected the newly milled creatures")
    };
    assert_eq!(
        frame.domain().iter().copied().collect::<BTreeSet<_>>(),
        [3, 4, 6].map(|n| physical(&state, n).id()).into()
    );
    for (n, owner) in [(3, A), (4, A), (5, A), (6, B), (7, B), (8, B)] {
        assert_eq!(physical(&state, n).zone(), ObjectZone::Graveyard(owner));
    }
    for (nums, total) in [(vec![], 0_i64), (vec![4], 4), (vec![3, 6], 7)] {
        let done = reply(
            &state,
            DecisionReply::SelectResolution(
                nums.iter().map(|&n| physical(&state, n).id()).collect(),
            ),
        );
        let made = events(&done)
            .flat_map(|e| e.occurrences.iter())
            .filter_map(|r| match &r.value {
                RulesOccurrence::TokenCreated { creator, after } => {
                    Some((*creator, after.object.id()))
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(made.len(), 1);
        assert_eq!(made[0].0, A);
        let field = done
            .state()
            .execution()
            .window()
            .cards()
            .iter()
            .filter(|o| o.zone() == ObjectZone::Battlefield && o.is_token())
            .map(GameObject::id)
            .collect::<Vec<_>>();
        if total == 0 {
            assert!(field.is_empty());
        } else {
            assert_eq!(field, [made[0].1]);
            let sheet = done
                .state()
                .execution()
                .current_characteristics(field[0])
                .unwrap()
                .unwrap();
            assert_eq!(sheet.current_colors, [Color::Blue].into());
            assert_eq!(
                sheet.current_creature_subtypes,
                [current_catalogue().1.resolve_label("Zombie").unwrap()].into()
            );
            assert_eq!(
                sheet.power_toughness.unwrap().derived,
                PowerToughness {
                    power: total.into(),
                    toughness: total.into()
                }
            );
        }
        for (n, owner) in [(3, A), (4, A), (6, B)] {
            assert_eq!(
                physical(done.state(), n).zone(),
                if nums.contains(&n) {
                    ObjectZone::Exile
                } else {
                    ObjectZone::Graveyard(owner)
                }
            );
        }
    }
}
