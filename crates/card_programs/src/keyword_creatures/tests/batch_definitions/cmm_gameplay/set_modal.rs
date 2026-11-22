//! Confluence cases for selection cardinality, repeated modes and exact outcomes.
use super::*;
use spellwake_engine::mana::CastMode;

fn confluence(
    state: &GameState,
    price: &[(ManaType, i64)],
    modes: &[(u8, Option<TargetRef>)],
) -> GameCommand {
    GameCommand::CastWithResources {
        spell: physical(state, 1).id(),
        target: None,
        payment: CastingPayment {
            modes: modes
                .iter()
                .map(|&(mode, target)| CastMode {
                    mode,
                    target,
                    targets: vec![],
                })
                .collect(),
            mana: Some(spend(price)),
            ..Default::default()
        },
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_fiery_confluence_resolves_all_three_modes_and_permits_repetition() {
    use ManaType::*;
    let price = [(Colourless, 2), (Red, 2)];
    let game = game(
        vec![A, B, C],
        vec![
            hand("Fiery Confluence"),
            creature(2, A, 1, 1, &[]),
            creature(3, B, 2, 2, &[]),
            creature(4, C, 3, 3, &[]),
            named(
                "Mind Stone",
                5,
                B,
                ObjectDestination::Battlefield { controller: B },
            ),
        ],
        &price,
        Step::PrecombatMain,
    );
    let artifact = TargetRef::Object(physical(&game, 5).id());
    let non_artifact = TargetRef::Object(physical(&game, 3).id());
    illegal(&game, A, confluence(&game, &price, &[(0, None), (1, None)]));
    illegal(
        &game,
        A,
        confluence(
            &game,
            &price,
            &[(0, None), (1, None), (2, Some(non_artifact))],
        ),
    );
    let command_all = confluence(&game, &price, &[(0, None), (1, None), (2, Some(artifact))]);
    let result = round(command(&game, A, command_all).into_state()).into_state();
    assert_eq!(physical(&result, 2).zone(), ObjectZone::Graveyard(A));
    assert_eq!(physical(&result, 5).zone(), ObjectZone::Graveyard(B));
    for id in [3, 4] {
        assert_eq!(
            result
                .execution()
                .battlefield_marked_damage(physical(&result, id).id()),
            Some(1.into())
        );
    }
    assert_eq!(result.execution().life_total(A), Some(&20.into()));
    assert_eq!(result.execution().life_total(B), Some(&18.into()));
    assert_eq!(result.execution().life_total(C), Some(&18.into()));

    let thrice = confluence(&game, &price, &[(1, None); 3]);
    let damage = round(command(&game, A, thrice).into_state());
    assert_eq!(damage.state().execution().life_total(B), Some(&14.into()));
    assert_eq!(damage.state().execution().life_total(C), Some(&14.into()));
    assert_eq!(physical(damage.state(), 2).zone(), ObjectZone::Battlefield);
    assert_eq!(physical(damage.state(), 5).zone(), ObjectZone::Battlefield);
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_righteous_confluence_creates_a_vigilant_knight_exiles_and_gains_life() {
    use ManaType::*;
    let price = [(Colourless, 3), (White, 2)];
    let game = game(
        vec![A, B],
        vec![
            hand("Righteous Confluence"),
            seed(
                2,
                B,
                ObjectDestination::Battlefield { controller: B },
                Program::enchantment(None),
            ),
            named(
                "Mind Stone",
                3,
                B,
                ObjectDestination::Battlefield { controller: B },
            ),
        ],
        &price,
        Step::PrecombatMain,
    );
    let target = |id| Some(TargetRef::Object(physical(&game, id).id()));
    illegal(
        &game,
        A,
        confluence(&game, &price, &[(0, None), (1, target(3)), (2, None)]),
    );
    let make_all = confluence(&game, &price, &[(0, None), (1, target(2)), (2, None)]);
    let all = round(command(&game, A, make_all).into_state());
    let knight = tokens(&all, A, 1)[0];
    let view = all
        .state()
        .execution()
        .current_characteristics(knight)
        .unwrap()
        .unwrap();
    assert!(view.has_keyword(Vigilance));
    assert_eq!(
        view.power_toughness.unwrap().derived,
        PowerToughness {
            power: 2.into(),
            toughness: 2.into()
        }
    );
    assert_eq!(view.current_colors, [Color::White].into());
    assert_eq!(
        view.current_creature_subtypes,
        [current_catalogue().1.resolve_label("Knight").unwrap()].into()
    );
    assert_eq!(physical(all.state(), 2).zone(), ObjectZone::Exile);
    assert_eq!(all.state().execution().life_total(A), Some(&25.into()));

    let all_gain = confluence(&game, &price, &[(2, None); 3]);
    let life = round(command(&game, A, all_gain).into_state());
    assert_eq!(life.state().execution().life_total(A), Some(&35.into()));
    assert_eq!(physical(life.state(), 2).zone(), ObjectZone::Battlefield);
    assert!(tokens(&life, A, 0).is_empty());
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_wretched_confluence_draws_and_costs_life_shrinks_and_recovers_a_creature() {
    use ManaType::*;
    let price = [(Colourless, 3), (Black, 2)];
    let game = game(
        vec![A, B],
        vec![
            hand("Wretched Confluence"),
            creature(2, B, 2, 2, &[Indestructible]),
            named("Fencing Ace", 3, A, ObjectDestination::Graveyard),
            named("Mountain", 4, A, ObjectDestination::Graveyard),
            named("Forest", 5, A, ObjectDestination::LibraryBottom),
            named("Island", 6, A, ObjectDestination::LibraryBottom),
            named("Swamp", 7, A, ObjectDestination::LibraryBottom),
        ],
        &price,
        Step::PrecombatMain,
    );
    let target = |id| Some(TargetRef::Object(physical(&game, id).id()));
    let player = Some(TargetRef::Player(A));
    illegal(
        &game,
        A,
        confluence(
            &game,
            &price,
            &[(0, player), (1, target(2)), (2, target(4))],
        ),
    );
    let use_all = confluence(
        &game,
        &price,
        &[(0, player), (1, target(2)), (2, target(3))],
    );
    let result = round(command(&game, A, use_all).into_state()).into_state();
    assert_eq!(result.execution().life_total(A), Some(&19.into()));
    assert_eq!(physical(&result, 2).zone(), ObjectZone::Graveyard(B));
    assert_eq!(physical(&result, 3).zone(), ObjectZone::Hand(A));
    assert_eq!(physical(&result, 5).zone(), ObjectZone::Hand(A));
    assert_eq!(physical(&result, 6).zone(), ObjectZone::Library(A));

    let all_draw = confluence(&game, &price, &[(0, player); 3]);
    let draws = round(command(&game, A, all_draw).into_state()).into_state();
    assert_eq!(draws.execution().life_total(A), Some(&17.into()));
    for id in [5, 6, 7] {
        assert_eq!(physical(&draws, id).zone(), ObjectZone::Hand(A));
    }
    assert_eq!(physical(&draws, 3).zone(), ObjectZone::Graveyard(A));
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_zones_mystic_confluence_taxes_the_spell_controller_bounces_to_owner_and_draws_including_repeated_draws()
 {
    use ManaType::*;
    use spellwake_engine::{effects::ResolutionPaymentReply, land::BasicLandType};
    let price = [(Colourless, 3), (Blue, 2)];
    let mut cards = vec![
        hand("Mystic Confluence"),
        seed(
            2,
            B,
            ObjectDestination::Hand,
            free_instant(BodyTarget::SingleCreature, Instruction::DestroyTarget),
        ),
        creature(3, A, 1, 2, &[]),
        seed(
            4,
            B,
            ObjectDestination::Battlefield { controller: A },
            Program::creature(None, 1.into(), 1.into(), BTreeSet::new()),
        ),
    ];
    for n in 5..8 {
        cards.push(named(
            "Island",
            n,
            B,
            ObjectDestination::Battlefield { controller: B },
        ));
    }
    for n in 8..11 {
        cards.push(named("Forest", n, A, ObjectDestination::LibraryBottom));
    }
    let start = game(vec![A, B], cards, &price, Step::PrecombatMain);
    let draws = round(command(&start, A, confluence(&start, &price, &[(2, None); 3])).into_state());
    for n in 8..11 {
        assert_eq!(physical(draws.state(), n).zone(), ObjectZone::Hand(A));
    }
    let mut funded = pass(&start).into_state();
    for n in 5..8 {
        funded = command(
            &funded,
            B,
            GameCommand::ActivateMana {
                source: physical(&funded, n).id(),
                ability: ManaAbilityRef::BasicLand(BasicLandType::Island),
            },
        )
        .into_state();
    }
    let stacked =
        targeted(&funded, B, 2, TargetRef::Object(physical(&funded, 3).id())).into_state();
    let active = pass(&stacked).into_state();
    let victim = TargetRef::Stack(active.execution().window().entries().last().unwrap().id());
    let bounce = TargetRef::Object(physical(&active, 4).id());
    let all = command(
        &active,
        A,
        confluence(
            &active,
            &price,
            &[(0, Some(victim)), (1, Some(bounce)), (2, None)],
        ),
    )
    .into_state();
    let decision = round(all).into_state();
    assert_eq!(decision.pending_decision().unwrap().chooser(), B);
    for paid in [false, true] {
        let cost = if paid {
            ResolutionPaymentReply::Pay(spend(&[(Blue, 3)]))
        } else {
            ResolutionPaymentReply::Decline
        };
        let done = reply(&decision, DecisionReply::ResolutionPayment(cost)).into_state();
        assert_eq!(physical(&done, 4).zone(), ObjectZone::Hand(B));
        assert_eq!(physical(&done, 8).zone(), ObjectZone::Hand(A));
        assert_eq!(
            done.execution().mana(B, Blue),
            if paid { 0.into() } else { 3.into() }
        );
        let result = if paid { round(done).into_state() } else { done };
        assert_eq!(physical(&result, 2).zone(), ObjectZone::Graveyard(B));
        assert_eq!(
            physical(&result, 3).zone(),
            if paid {
                ObjectZone::Graveyard(A)
            } else {
                ObjectZone::Battlefield
            }
        );
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_zones_verdant_confluence_repeats_its_counter_mode_recovers_own_permanents_and_searches_a_tapped_basic()
 {
    use ManaType::*;
    use spellwake_engine::effects::CounterKind;
    let price = [(Colourless, 4), (Green, 2)];
    let initial = game(
        vec![A, B],
        vec![
            hand("Verdant Confluence"),
            creature(2, B, 1, 1, &[]),
            named("Sol Ring", 3, A, ObjectDestination::Graveyard),
            named("Sol Ring", 4, B, ObjectDestination::Graveyard),
            named("Counterspell", 5, A, ObjectDestination::Graveyard),
            named("Forest", 6, A, ObjectDestination::LibraryTop),
            named("Sol Ring", 7, A, ObjectDestination::LibraryBottom),
        ],
        &price,
        Step::PrecombatMain,
    );
    let obj = |n| Some(TargetRef::Object(physical(&initial, n).id()));
    for n in [4, 5] {
        illegal(
            &initial,
            A,
            confluence(&initial, &price, &[(0, obj(2)), (1, obj(n)), (2, None)]),
        );
    }
    let repeated =
        round(command(&initial, A, confluence(&initial, &price, &[(0, obj(2)); 3])).into_state());
    assert_eq!(
        repeated
            .state()
            .execution()
            .counters_of_kind(physical(repeated.state(), 2).id(), CounterKind::PlusOne),
        6.into()
    );
    let all = command(
        &initial,
        A,
        confluence(&initial, &price, &[(0, obj(2)), (1, obj(3)), (2, None)]),
    )
    .into_state();
    let choice = round(all).into_state();
    assert_eq!(
        choice
            .execution()
            .counters_of_kind(physical(&choice, 2).id(), CounterKind::PlusOne),
        2.into()
    );
    assert_eq!(physical(&choice, 3).zone(), ObjectZone::Hand(A));
    let ready = choice
        .supply_entropy(choice.basis(), vec![0; 16])
        .unwrap()
        .into_state();
    let RuleFrame::LibraryTop(frame) = ready.pending_decision().unwrap().frame() else {
        panic!("expected basic-land search")
    };
    assert_eq!(frame.eligible_results(), [physical(&ready, 6).id()]);
    let found = reply(
        &ready,
        DecisionReply::LibraryTop(LibraryTopReply::Search {
            selected: Some(physical(&ready, 6).id()),
        }),
    );
    assert_eq!(found.shuffle_journal().len(), 1);
    let land = physical(found.state(), 6);
    assert_eq!(
        (land.zone(), land.tapped()),
        (ObjectZone::Battlefield, Some(true))
    );
}

fn order_simultaneous_tokens(candidate: NativeCandidate) -> NativeCandidate {
    let state = candidate.state();
    let Some(RuleFrame::TimestampOrder(frame)) =
        state.pending_decision().map(|decision| decision.frame())
    else {
        return candidate;
    };
    reply(
        state,
        DecisionReply::TimestampOrder(frame.members().iter().map(|member| member.member).collect()),
    )
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_zones_kirtars_wrath_prevents_regeneration_and_creates_two_flying_spirits_only_after_threshold()
 {
    use ManaType::*;
    use spellwake_engine::activated::{ActivatedAbility, SourceCost};
    let price = [(Colourless, 4), (White, 2)];
    for grave_count in [6, 7] {
        let mut fragile = creature(2, A, 1, 1, &[]);
        fragile.program.activated.push(ActivatedAbility {
            id: ActivatedAbilityId(0),
            mana_cost: ManaCost::free(),
            source_cost: SourceCost::None,
            body: ExecutableBody::new(BodyTarget::None, vec![Instruction::RegenerateSource]),
        });
        let mut cards = vec![
            hand("Kirtar's Wrath"),
            fragile,
            creature(3, B, 3, 3, &[]),
            creature(4, B, 2, 2, &[Indestructible]),
        ];
        for n in 0..grave_count {
            cards.push(named("Island", 5 + n, A, ObjectDestination::Graveyard));
        }
        let initial = game(vec![A, B], cards, &price, Step::PrecombatMain);
        let guarded = round(
            command(
                &initial,
                A,
                GameCommand::ActivateUntargetedAbility {
                    source: physical(&initial, 2).id(),
                    ability: ActivatedAbilityId(0),
                    payment: None,
                },
            )
            .into_state(),
        )
        .into_state();
        let done =
            order_simultaneous_tokens(round(cast_untargeted(&guarded, 1, &price).into_state()));
        assert_eq!(physical(done.state(), 2).zone(), ObjectZone::Graveyard(A));
        assert_eq!(physical(done.state(), 3).zone(), ObjectZone::Graveyard(B));
        assert_eq!(physical(done.state(), 4).zone(), ObjectZone::Battlefield);
        for spirit in tokens(&done, A, if grave_count == 7 { 2 } else { 0 }) {
            library_sagas::check_chapter_token(
                done.state(),
                spirit,
                Color::White,
                1,
                &["Spirit"],
                &[Flying],
            );
        }
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_zones_battle_screech_creates_two_flying_birds_and_flashes_back_by_tapping_three_distinct_own_white_creatures()
 {
    use ManaType::*;
    let price = [(Colourless, 2), (White, 2)];
    let initial = game(
        vec![A, B],
        vec![
            hand("Battle Screech"),
            named(
                "Fencing Ace",
                2,
                A,
                ObjectDestination::Battlefield { controller: A },
            ),
            named(
                "Fencing Ace",
                3,
                B,
                ObjectDestination::Battlefield { controller: B },
            ),
            named(
                "Elvish Mystic",
                4,
                A,
                ObjectDestination::Battlefield { controller: A },
            ),
        ],
        &price,
        Step::PrecombatMain,
    );
    let ordinary =
        order_simultaneous_tokens(round(cast_untargeted(&initial, 1, &price).into_state()));
    let birds = tokens(&ordinary, A, 2);
    for &bird in &birds {
        library_sagas::check_chapter_token(
            ordinary.state(),
            bird,
            Color::White,
            1,
            &["Bird"],
            &[Flying],
        );
    }
    let ready = ordinary.into_state();
    let cast = |chosen: &[ObjectId]| GameCommand::CastWithResources {
        spell: physical(&ready, 1).id(),
        target: None,
        payment: CastingPayment {
            objects: chosen
                .iter()
                .copied()
                .map(CastResource::TapForFlashback)
                .collect(),
            ..Default::default()
        },
    };
    illegal(&ready, A, cast(&birds));
    for wrong in [birds[0], physical(&ready, 3).id(), physical(&ready, 4).id()] {
        illegal(&ready, A, cast(&[birds[0], birds[1], wrong]));
    }
    let paid = [birds[0], birds[1], physical(&ready, 2).id()];
    let stack = command(&ready, A, cast(&paid)).into_state();
    for &creature in &paid {
        assert_eq!(
            stack
                .execution()
                .window()
                .cards()
                .get(creature)
                .unwrap()
                .tapped(),
            Some(true)
        );
    }
    let result = order_simultaneous_tokens(round(stack));
    assert_eq!(physical(result.state(), 1).zone(), ObjectZone::Exile);
    let mut cohorts = events(&result).filter_map(|event| {
        let created = event
            .occurrences
            .iter()
            .filter_map(|row| match &row.value {
                RulesOccurrence::TokenCreated { creator, after } => {
                    assert_eq!(*creator, A);
                    Some(after.object.id())
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        (!created.is_empty()).then_some(created)
    });
    let new_birds = cohorts.next().expect("flashback creates a pair of Birds");
    assert_eq!(new_birds.len(), 2);
    assert!(cohorts.next().is_none());
    assert_eq!(
        result
            .state()
            .execution()
            .window()
            .cards()
            .iter()
            .filter(|o| o.is_token() && o.zone() == ObjectZone::Battlefield)
            .count(),
        4
    );
    for bird in new_birds {
        assert!(!birds.contains(&bird));
        library_sagas::check_chapter_token(
            result.state(),
            bird,
            Color::White,
            1,
            &["Bird"],
            &[Flying],
        );
    }
}
