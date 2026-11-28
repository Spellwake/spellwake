//! Named cases for counter payments, sacrifice costs and token mana abilities.
use super::*;

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_khalni_heart_expedition_counts_its_controllers_lands_and_finds_two_tapped_basics() {
    use spellwake_engine::effects::{CounterKind, OptionalEffectChoice};
    let mut cards = draw_pile();
    cards.extend([
        named("Khalni Heart Expedition", 1, A, ObjectDestination::Hand),
        named("Forest", 2, A, ObjectDestination::Hand),
        named("Mountain", 3, A, ObjectDestination::Hand),
        named("Island", 4, A, ObjectDestination::Hand),
        named("Plains", 5, B, ObjectDestination::Hand),
        named("Swamp", 21, A, ObjectDestination::LibraryBottom),
        named("Plains", 22, A, ObjectDestination::LibraryBottom),
        seed(
            23,
            B,
            ObjectDestination::LibraryBottom,
            Program::artifact(None, vec![]),
        ),
        seed(
            24,
            B,
            ObjectDestination::LibraryBottom,
            Program::artifact(None, vec![]),
        ),
    ]);
    let mana = [(Colourless, 1), (Green, 1)];
    let start = game(vec![A, B], cards, &mana, Step::PrecombatMain);
    let mut state = round(cast_untargeted(&start, 1, &mana).into_state()).into_state();
    let quest = physical(&state, 1).id();
    for (id, turn, player) in [(2, 1, A), (5, 2, B), (3, 3, A), (4, 5, A)] {
        state = exile_hand::until(state, |s| {
            let p = s.turn().unwrap().position();
            p.number == turn && p.step == Step::PrecombatMain
        });
        let land = physical(&state, id).id();
        state = command(&state, player, GameCommand::PlayLand { land }).into_state();
        if player == B {
            assert!(state.execution().window().entries().is_empty());
            continue;
        }
        let before = state
            .execution()
            .counters_of_kind(quest, CounterKind::Quest);
        let offered = round(state).into_state();
        let skipped = reply(
            &offered,
            DecisionReply::OptionalEffect(OptionalEffectChoice::Decline),
        )
        .into_state();
        assert_eq!(
            skipped
                .execution()
                .counters_of_kind(quest, CounterKind::Quest),
            before
        );
        state = reply(
            &offered,
            DecisionReply::OptionalEffect(OptionalEffectChoice::Take),
        )
        .into_state();
        assert_eq!(
            state
                .execution()
                .counters_of_kind(quest, CounterKind::Quest),
            before.plus(&1.into())
        );
        if id < 4 {
            illegal(&state, A, activation(&state, 1, 0, &[]));
        }
    }
    let paid = command(&state, A, activation(&state, 1, 0, &[])).into_state();
    assert_eq!(physical(&paid, 1).zone(), ObjectZone::Graveyard(A));
    let search = round(paid).into_state();
    verify_two_tapped_basics(search);
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_burnished_hart_sacrifices_for_a_shuffled_search_of_up_to_two_tapped_basics() {
    let initial = game(
        vec![A, B],
        vec![
            named("Burnished Hart", 1, A, ObjectDestination::Hand),
            named("Swamp", 21, A, ObjectDestination::LibraryBottom),
            named("Plains", 22, A, ObjectDestination::LibraryBottom),
            named("Myriad Landscape", 23, A, ObjectDestination::LibraryBottom),
        ],
        &[(Colourless, 6)],
        Step::PrecombatMain,
    );
    let entered = round(cast_untargeted(&initial, 1, &[(Colourless, 3)]).into_state()).into_state();
    let paid = command(&entered, A, activation(&entered, 1, 0, &[(Colourless, 3)])).into_state();
    assert_eq!(physical(&paid, 1).zone(), ObjectZone::Graveyard(A));
    verify_two_tapped_basics(round(paid).into_state());
}

fn verify_two_tapped_basics(search: GameState) {
    use spellwake_engine::effects::LibraryTopReply;
    let RuleFrame::LibraryTop(frame) = search.pending_decision().unwrap().frame() else {
        panic!("expected basic land search");
    };
    let found = [physical(&search, 21).id(), physical(&search, 22).id()];
    assert_eq!(
        frame
            .eligible_results()
            .iter()
            .copied()
            .collect::<BTreeSet<_>>(),
        found.into()
    );
    let ready = search
        .supply_entropy(search.basis(), vec![0; 16])
        .unwrap()
        .into_state();
    for selections in [vec![], vec![found[0]], found.to_vec()] {
        let result = reply(
            &ready,
            DecisionReply::LibraryTop(LibraryTopReply::SearchBattlefield {
                selected_in_timestamp_order: selections.clone(),
            }),
        );
        for (id, original) in [(21, found[0]), (22, found[1])] {
            let row = physical(result.state(), id);
            if selections.contains(&original) {
                assert_eq!(row.zone(), ObjectZone::Battlefield);
                assert_eq!(row.tapped(), Some(true));
            } else {
                assert_eq!(row.zone(), ObjectZone::Library(A));
            }
        }
        assert_eq!(result.shuffle_journal().len(), 1);
    }
}

fn cash_eldrazi(mut state: GameState, tokens: Vec<ObjectId>, kind: &str, power: i64) -> GameState {
    use spellwake_engine::mana::{ManaAbilityId, ManaAbilityRef};
    let vocab = current_catalogue().1;
    let expected = ["Eldrazi", kind]
        .map(|name| vocab.resolve_label(name).unwrap())
        .into();
    for token in tokens {
        let stats = sheet(&state, token);
        assert_eq!(stats.current_creature_subtypes, expected);
        assert!(stats.current_colors.is_empty());
        assert_size(&state, token, power, 1);
        let before = state.execution().mana(A, Colourless);
        state = command(
            &state,
            A,
            GameCommand::ActivateMana {
                source: token,
                ability: ManaAbilityRef::Printed(ManaAbilityId(0)),
            },
        )
        .into_state();
        assert_eq!(
            state.execution().mana(A, Colourless),
            before.plus(&1.into())
        );
        assert!(state.execution().window().entries().is_empty());
        assert_ne!(
            state
                .execution()
                .window()
                .cards()
                .get(token)
                .map(GameObject::zone),
            Some(ObjectZone::Battlefield)
        );
    }
    state
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_dread_drone_and_kozileks_predator_each_make_two_spawn_with_independent_mana_abilities() {
    for (name, price) in [
        ("Dread Drone", [(Colourless, 4), (Black, 1)]),
        ("Kozilek's Predator", [(Colourless, 3), (Green, 1)]),
    ] {
        let initial = game(
            vec![A, B],
            vec![named(name, 1, A, ObjectDestination::Hand)],
            &price,
            Step::PrecombatMain,
        );
        let entered = round(cast_untargeted(&initial, 1, &price).into_state()).into_state();
        let (formed, spawn) = ordered_tokens(entered, 2);
        let paid = cash_eldrazi(formed, spawn, "Spawn", 0);
        assert_eq!(paid.execution().mana(A, Colourless), 2.into());
        assert_eq!(physical(&paid, 1).zone(), ObjectZone::Battlefield);
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_ashnods_altar_sacrifices_carrier_thrall_and_its_death_trigger_creates_a_mana_scion() {
    use spellwake_engine::mana::{ManaAbilityId, ManaAbilityRef};
    let initial = game(
        vec![A, B],
        vec![
            named(
                "Ashnod's Altar",
                1,
                A,
                ObjectDestination::Battlefield { controller: A },
            ),
            named("Carrier Thrall", 2, A, ObjectDestination::Hand),
            creature(3, B, 2, 2, &[]),
        ],
        &[(Colourless, 1), (Black, 1)],
        Step::PrecombatMain,
    );
    let entered = round(cast_untargeted(&initial, 2, &[(Colourless, 1), (Black, 1)]).into_state())
        .into_state();
    let cash = |id| GameCommand::ActivateManaWithCostObjects {
        source: physical(&entered, 1).id(),
        ability: ManaAbilityRef::Printed(ManaAbilityId(0)),
        objects: vec![physical(&entered, id).id()],
    };
    illegal(&entered, A, cash(3));
    illegal(&entered, A, cash(1));
    let death = command(&entered, A, cash(2)).into_state();
    assert_eq!(physical(&death, 2).zone(), ObjectZone::Graveyard(A));
    assert_eq!(death.execution().mana(A, Colourless), 2.into());
    assert_eq!(death.execution().window().entries().len(), 1);
    let (formed, scion) = ordered_tokens(death, 1);
    let paid = cash_eldrazi(formed, scion, "Scion", 1);
    assert_eq!(paid.execution().mana(A, Colourless), 3.into());
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_investigators_journal_enters_with_the_highest_creature_count_and_pays_each_draw_cost() {
    use spellwake_engine::effects::CounterKind;
    for maximum in [0, 3] {
        let mut cards = draw_pile();
        cards.push(named(
            "Investigator's Journal",
            1,
            A,
            ObjectDestination::Hand,
        ));
        if maximum != 0 {
            cards.push(creature(2, A, 1, 1, &[]));
            cards.extend((3..6).map(|n| creature(n, B, 1, 1, &[])));
        }
        let state = game(vec![A, B], cards, &[(Colourless, 6)], Step::PrecombatMain);
        let entered =
            round(cast_untargeted(&state, 1, &[(Colourless, 2)]).into_state()).into_state();
        let source = physical(&entered, 1).id();
        assert_eq!(
            entered
                .execution()
                .counters_of_kind(source, CounterKind::Suspect),
            maximum.into()
        );
        let tap = activation(&entered, 1, 0, &[(Colourless, 2)]);
        let later = if maximum == 0 {
            illegal(&entered, A, tap);
            entered
        } else {
            let paid = command(&entered, A, tap).into_state();
            assert_eq!(physical(&paid, 1).tapped(), Some(true));
            assert_eq!(
                paid.execution()
                    .counters_of_kind(source, CounterKind::Suspect),
                2.into()
            );
            let resolved = round(paid).into_state();
            assert_eq!(hand_size(&resolved, A), 1);
            resolved
        };
        let before = hand_size(&later, A);
        let sacrificed =
            command(&later, A, activation(&later, 1, 1, &[(Colourless, 2)])).into_state();
        assert_eq!(physical(&sacrificed, 1).zone(), ObjectZone::Graveyard(A));
        let drawn = round(sacrificed).into_state();
        assert_eq!(hand_size(&drawn, A), before + 1);
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_makeshift_munitions_sacrifices_its_controllers_artifact_or_creature_to_damage_any_target() {
    let initial = game(
        vec![A, B],
        vec![
            named("Makeshift Munitions", 1, A, ObjectDestination::Hand),
            seed(
                2,
                A,
                ObjectDestination::Battlefield { controller: A },
                Program::artifact(None, vec![]),
            ),
            creature(3, A, 1, 1, &[]),
            creature(4, B, 1, 1, &[]),
            creature(5, B, 1, 1, &[]),
            named(
                "Forest",
                6,
                A,
                ObjectDestination::Battlefield { controller: A },
            ),
        ],
        &[(Colourless, 3), (Red, 1)],
        Step::PrecombatMain,
    );
    let entered =
        round(cast_untargeted(&initial, 1, &[(Colourless, 1), (Red, 1)]).into_state()).into_state();
    let pay = |s: &GameState, victim: u64, target| GameCommand::ActivateWithCostObjects {
        source: physical(s, 1).id(),
        ability: ActivatedAbilityId(0),
        granting_effect: None,
        target: Some(target),
        objects: vec![physical(s, victim).id()],
        payment: Some(spend(&[(Colourless, 1)])),
    };
    for victim in [4, 6] {
        illegal(&entered, A, pay(&entered, victim, TargetRef::Player(B)));
    }
    let paid = command(&entered, A, pay(&entered, 2, TargetRef::Player(B))).into_state();
    assert_eq!(physical(&paid, 2).zone(), ObjectZone::Graveyard(A));
    let first = round(paid).into_state();
    assert_eq!(first.execution().life_total(B), Some(&19.into()));
    let second = command(
        &first,
        A,
        pay(&first, 3, TargetRef::Object(physical(&first, 4).id())),
    )
    .into_state();
    assert_eq!(physical(&second, 3).zone(), ObjectZone::Graveyard(A));
    let done = round(second).into_state();
    assert_eq!(physical(&done, 4).zone(), ObjectZone::Graveyard(B));
    assert_eq!(physical(&done, 5).zone(), ObjectZone::Battlefield);
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_scavenger_grounds_taps_for_mana_or_sacrifices_a_desert_to_exile_all_graveyards() {
    use spellwake_engine::mana::{ManaAbilityId, ManaAbilityRef};
    let initial = game(
        vec![A, B, C],
        vec![
            named("Scavenger Grounds", 1, A, ObjectDestination::Hand),
            named(
                "Scavenger Grounds",
                2,
                A,
                ObjectDestination::Battlefield { controller: A },
            ),
            named(
                "Scavenger Grounds",
                3,
                B,
                ObjectDestination::Battlefield { controller: B },
            ),
            named(
                "Forest",
                4,
                A,
                ObjectDestination::Battlefield { controller: A },
            ),
            seed(
                5,
                A,
                ObjectDestination::Graveyard,
                Program::artifact(None, vec![]),
            ),
            seed(
                6,
                B,
                ObjectDestination::Graveyard,
                Program::artifact(None, vec![]),
            ),
            seed(
                7,
                C,
                ObjectDestination::Graveyard,
                Program::artifact(None, vec![]),
            ),
        ],
        &[(Colourless, 2)],
        Step::PrecombatMain,
    );
    let played = command(
        &initial,
        A,
        GameCommand::PlayLand {
            land: physical(&initial, 1).id(),
        },
    )
    .into_state();
    let tapped = command(
        &played,
        A,
        GameCommand::ActivateMana {
            source: physical(&played, 1).id(),
            ability: ManaAbilityRef::Printed(ManaAbilityId(0)),
        },
    )
    .into_state();
    assert_eq!(tapped.execution().mana(A, Colourless), 3.into());
    let pay = |id| {
        selected_cost(
            &played,
            1,
            0,
            vec![physical(&played, id).id()],
            None,
            &[(Colourless, 2)],
        )
    };
    illegal(&played, A, pay(3));
    illegal(&played, A, pay(4));
    for victim in [1, 2] {
        let paid = command(&played, A, pay(victim)).into_state();
        assert_eq!(physical(&paid, victim).zone(), ObjectZone::Graveyard(A));
        if victim == 2 {
            assert_eq!(physical(&paid, 1).tapped(), Some(true));
        }
        let done = round(paid).into_state();
        for id in [victim, 5, 6, 7] {
            assert_eq!(physical(&done, id).zone(), ObjectZone::Exile);
        }
        assert_eq!(physical(&done, 3).zone(), ObjectZone::Battlefield);
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_witching_well_scries_two_then_sacrifices_to_draw_the_reordered_cards() {
    use spellwake_engine::effects::LibraryTopReply;
    let mut cards = draw_pile();
    cards.push(named("Witching Well", 1, A, ObjectDestination::Hand));
    let initial = game(
        vec![A, B],
        cards,
        &[(Blue, 2), (Colourless, 3)],
        Step::PrecombatMain,
    );
    let entered = round(cast_untargeted(&initial, 1, &[(Blue, 1)]).into_state()).into_state();
    let scry = round(entered).into_state();
    let RuleFrame::LibraryTop(frame) = scry.pending_decision().unwrap().frame() else {
        panic!("scry two");
    };
    let [first, second] = frame.domain_top_first() else {
        panic!("two available cards");
    };
    let reordered = reply(
        &scry,
        DecisionReply::LibraryTop(LibraryTopReply::Scry {
            top_first: vec![*second],
            bottom_top_first: vec![*first],
        }),
    )
    .into_state();
    let expected = reordered.execution().window().cards().library(A)[..2]
        .iter()
        .map(|id| {
            reordered
                .execution()
                .window()
                .cards()
                .get(*id)
                .unwrap()
                .card_id()
                .unwrap()
        })
        .collect::<Vec<_>>();
    let paid = command(
        &reordered,
        A,
        activation(&reordered, 1, 0, &[(Colourless, 3), (Blue, 1)]),
    )
    .into_state();
    assert_eq!(physical(&paid, 1).zone(), ObjectZone::Graveyard(A));
    let done = round(paid).into_state();
    assert_eq!(hand_size(&done, A), 2);
    for id in expected {
        assert_eq!(physical(&done, id.0).zone(), ObjectZone::Hand(A));
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_rapacious_one_tramples_through_a_blocker_and_creates_spawn_only_for_player_damage() {
    use spellwake_engine::combat::CombatDamageAssignment;
    let initial = game(
        vec![A, B],
        vec![
            named("Rapacious One", 1, A, ObjectDestination::Hand),
            named(
                "Fervor",
                2,
                A,
                ObjectDestination::Battlefield { controller: A },
            ),
            creature(3, B, 1, 2, &[]),
        ],
        &[(Colourless, 5), (Red, 1)],
        Step::PrecombatMain,
    );
    let entered =
        round(cast_untargeted(&initial, 1, &[(Colourless, 5), (Red, 1)]).into_state()).into_state();
    let source = physical(&entered, 1).id();
    let attack = combat_frame(&entered, true);
    let attacking = reply(
        &attack,
        DecisionReply::DeclareAttackers(vec![AttackDeclaration {
            attacker: source,
            defender: B,
            defending_planeswalker: None,
        }]),
    )
    .into_state();
    let blockers = combat_frame(&attacking, false);
    let blocker = physical(&blockers, 3).id();
    let blocked = reply(
        &blockers,
        DecisionReply::DeclareBlockers(vec![BlockDeclaration {
            attacker: source,
            blocker,
        }]),
    )
    .into_state();
    let choosing = round(blocked).into_state();
    let assigned = reply(
        &choosing,
        DecisionReply::AssignCombatDamage(vec![
            CombatDamageAssignment {
                source,
                target: DamageRecipient::Creature(blocker),
                amount: 2.into(),
            },
            CombatDamageAssignment {
                source,
                target: DamageRecipient::Player(B),
                amount: 3.into(),
            },
        ]),
    )
    .into_state();
    assert_eq!(assigned.execution().life_total(B), Some(&17.into()));
    assert_eq!(physical(&assigned, 3).zone(), ObjectZone::Graveyard(B));
    let (made, spawn) = ordered_tokens(assigned, 3);
    assert_eq!(
        cash_eldrazi(made, spawn, "Spawn", 0)
            .execution()
            .mana(A, Colourless),
        3.into()
    );
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_transmogrifying_wand_spends_a_charge_at_sorcery_speed_and_gives_the_controller_a_white_ox() {
    let state = game(
        vec![A, B],
        vec![
            named("Transmogrifying Wand", 1, A, ObjectDestination::Hand),
            creature(2, B, 2, 2, &[]),
            seed(
                3,
                A,
                ObjectDestination::Hand,
                free_instant(
                    BodyTarget::None,
                    crate::keyword_creatures::toolbox_emit::gain_life(
                        spellwake_engine::toolbox::PlayerSelector::Resolving(
                            spellwake_engine::program::ResolvingPlayer::Controller,
                        ),
                        spellwake_engine::program::ValueExpr::literal(1),
                    ),
                ),
            ),
        ],
        &[(Colourless, 4)],
        Step::PrecombatMain,
    );
    let entered = round(cast_untargeted(&state, 1, &[(Colourless, 3)]).into_state()).into_state();
    let source = physical(&entered, 1).id();
    assert_eq!(sheet(&entered, source).charge_counters, 3.into());
    let use_wand = GameCommand::ActivateAbility {
        source,
        ability: ActivatedAbilityId(0),
        target: TargetRef::Object(physical(&entered, 2).id()),
        payment: Some(spend(&[(Colourless, 1)])),
    };
    let spell_on_stack = cast_untargeted(&entered, 3, &[]).into_state();
    illegal(&spell_on_stack, A, use_wand.clone());
    let paid = command(&entered, A, use_wand).into_state();
    assert_eq!(physical(&paid, 1).tapped(), Some(true));
    assert_eq!(sheet(&paid, source).charge_counters, 2.into());
    let done = round(paid);
    let ox = tokens(&done, B, 1)[0];
    assert_eq!(physical(done.state(), 2).zone(), ObjectZone::Graveyard(B));
    let stats = sheet(done.state(), ox);
    assert_eq!(stats.current_colors, [Color::White].into());
    assert_eq!(
        stats.current_creature_subtypes,
        [current_catalogue().1.resolve_label("Ox").unwrap()].into()
    );
    assert_size(done.state(), ox, 2, 4);
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_urzas_exiled_land_uses_a_normal_land_play_and_expires_at_turn_end() {
    let initial = game(
        vec![A, B],
        vec![
            named(
                "Urza, Lord High Artificer",
                1,
                A,
                ObjectDestination::Battlefield { controller: A },
            ),
            named("Forest", 2, A, ObjectDestination::LibraryBottom),
            named("Island", 3, A, ObjectDestination::Hand),
        ],
        &[(Colourless, 5)],
        Step::PrecombatMain,
    );
    let paid = command(&initial, A, activation(&initial, 1, 0, &[(Colourless, 5)])).into_state();
    let ready = paid
        .supply_entropy(paid.basis(), vec![1])
        .unwrap()
        .into_state();
    let done = round(ready);
    assert_eq!(done.shuffle_journal().len(), 1);
    let exiled = done.into_state();
    assert_eq!(physical(&exiled, 2).zone(), ObjectZone::Exile);
    let land = GameCommand::PlayLand {
        land: physical(&exiled, 2).id(),
    };
    let played = command(&exiled, A, land.clone()).into_state();
    assert_eq!(physical(&played, 2).zone(), ObjectZone::Battlefield);
    illegal(
        &played,
        A,
        GameCommand::PlayLand {
            land: physical(&played, 3).id(),
        },
    );
    let next = exile_hand::until(exiled, |s| {
        s.turn().unwrap().position().number == 3
            && s.turn().unwrap().position().step == Step::PrecombatMain
    });
    illegal(&next, A, land);
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_deranged_assistant_mills_and_taps_at_announcement_then_adds_mana_on_the_stack() {
    use spellwake_engine::{
        mana::{ManaAbilityId, ManaAbilityRef},
        stack_window::StackKind,
    };
    let start = game(
        vec![A, B],
        vec![
            named("Deranged Assistant", 1, A, ObjectDestination::Hand),
            named(
                "Fervor",
                2,
                A,
                ObjectDestination::Battlefield { controller: A },
            ),
            remove_spell(3, A),
            named(
                "Deranged Assistant",
                4,
                A,
                ObjectDestination::Battlefield { controller: A },
            ),
        ],
        &[(Colourless, 1), (Blue, 1)],
        Step::PrecombatMain,
    );
    let entered =
        round(cast_untargeted(&start, 1, &[(Colourless, 1), (Blue, 1)]).into_state()).into_state();
    let source = physical(&entered, 1).id();
    assert!(sheet(&entered, source).mana_abilities().is_empty());
    illegal(
        &entered,
        A,
        GameCommand::ActivateMana {
            source,
            ability: ManaAbilityRef::Printed(ManaAbilityId(0)),
        },
    );
    let paid = command(&entered, A, activation(&entered, 1, 0, &[])).into_state();
    let entries = paid.execution().window().entries();
    assert!(
        matches!(entries,[entry] if matches!(entry.kind(),StackKind::ActivatedAbility{source:before} if before.id()==source))
    );
    assert_eq!(physical(&paid, 1).tapped(), Some(true));
    assert_eq!(physical(&paid, 100).zone(), ObjectZone::Graveyard(A));
    assert_eq!(paid.execution().mana(A, Colourless), 0.into());
    illegal(&paid, A, activation(&paid, 4, 0, &[]));
    assert_eq!(physical(&paid, 4).tapped(), Some(false));
    let response = targeted(&paid, A, 3, TargetRef::Object(source)).into_state();
    let removed = round(response).into_state();
    assert_eq!(physical(&removed, 1).zone(), ObjectZone::Graveyard(A));
    assert_eq!(removed.execution().mana(A, Colourless), 0.into());
    let resolved = round(removed).into_state();
    assert_eq!(resolved.execution().mana(A, Colourless), 1.into());
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_sai_creates_a_flying_thopter_on_artifact_cast_then_sacrifices_two_artifacts_to_draw() {
    let start = game(
        vec![A, B],
        vec![
            named("Sai, Master Thopterist", 1, A, ObjectDestination::Hand),
            seed(
                2,
                A,
                ObjectDestination::Hand,
                Program::artifact(Some(ManaCost::free()), vec![]),
            ),
            seed(
                3,
                B,
                ObjectDestination::Battlefield { controller: B },
                Program::artifact(None, vec![]),
            ),
        ],
        &[(Colourless, 3), (Blue, 2)],
        Step::PrecombatMain,
    );
    let entered =
        round(cast_untargeted(&start, 1, &[(Colourless, 2), (Blue, 1)]).into_state()).into_state();
    let cast = cast_untargeted(&entered, 2, &[]).into_state();
    assert_eq!(cast.execution().window().entries().len(), 2);
    let (made, thopters) = ordered_tokens(cast, 1);
    let thopter = thopters[0];
    let piece = sheet(&made, thopter);
    assert!(
        piece.current_colors.is_empty()
            && piece.has_keyword(KeywordAbility::Flying)
            && piece.has_type(CardType::Artifact)
    );
    assert_eq!(
        piece.current_creature_subtypes,
        [current_catalogue().1.resolve_label("Thopter").unwrap()].into()
    );
    assert_size(&made, thopter, 1, 1);
    assert_eq!(physical(&made, 2).zone(), ObjectZone::Stack);
    let landed = round(made).into_state();
    let pay = |ids| selected_cost(&landed, 1, 0, ids, None, &[(Colourless, 1), (Blue, 1)]);
    illegal(&landed, A, pay(vec![thopter, physical(&landed, 3).id()]));
    illegal(&landed, A, pay(vec![thopter, thopter]));
    let paid = command(&landed, A, pay(vec![thopter, physical(&landed, 2).id()])).into_state();
    assert_eq!(physical(&paid, 2).zone(), ObjectZone::Graveyard(A));
    let drawn = round(paid).into_state();
    assert_eq!(hand_size(&drawn, A), 1);
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_blood_aspirant_grows_on_sacrifices_and_taps_to_damage_a_creature_and_prevent_its_block() {
    use spellwake_engine::characteristics::BlockingRestriction;
    let start = game(
        vec![A, B],
        vec![
            named("Blood Aspirant", 1, A, ObjectDestination::Hand),
            named(
                "Fervor",
                2,
                A,
                ObjectDestination::Battlefield { controller: A },
            ),
            creature(3, A, 1, 1, &[]),
            creature(4, A, 2, 2, &[KeywordAbility::Haste]),
            creature(5, B, 1, 4, &[]),
            creature(6, B, 2, 2, &[]),
            seed(
                7,
                A,
                ObjectDestination::Battlefield { controller: A },
                Program::artifact(None, vec![]),
            ),
        ],
        &[(Colourless, 2), (Red, 2)],
        Step::PrecombatMain,
    );
    let entered =
        round(cast_untargeted(&start, 1, &[(Colourless, 1), (Red, 1)]).into_state()).into_state();
    let victim = physical(&entered, 5).id();
    let pay = |n| {
        selected_cost(
            &entered,
            1,
            0,
            vec![physical(&entered, n).id()],
            Some(victim),
            &[(Colourless, 1), (Red, 1)],
        )
    };
    illegal(&entered, A, pay(6));
    illegal(&entered, A, pay(7));
    for selected in [2, 3] {
        let paid = command(&entered, A, pay(selected)).into_state();
        assert_eq!(physical(&paid, selected).zone(), ObjectZone::Graveyard(A));
        assert_eq!(physical(&paid, 1).tapped(), Some(true));
        assert_eq!(paid.execution().window().entries().len(), 2);
        let grown = round(paid).into_state();
        assert_eq!(
            sheet(&grown, physical(&grown, 1).id()).plus_one_counters,
            1.into()
        );
        let done = round(grown).into_state();
        assert_eq!(
            done.execution().battlefield_marked_damage(victim),
            Some(1.into())
        );
        assert!(
            sheet(&done, victim)
                .blocking_restrictions
                .contains(&BlockingRestriction::CannotBlock)
        );
        let attack = combat_frame(&done, true);
        let attacker = physical(&attack, 4).id();
        let attacked = reply(
            &attack,
            DecisionReply::DeclareAttackers(vec![AttackDeclaration {
                attacker,
                defender: B,
                defending_planeswalker: None,
            }]),
        )
        .into_state();
        let blocks = combat_frame(&attacked, false);
        illegal(
            &blocks,
            B,
            GameCommand::Reply {
                decision: blocks.pending_decision().unwrap().id(),
                choice: DecisionReply::DeclareBlockers(vec![BlockDeclaration {
                    attacker,
                    blocker: victim,
                }]),
            },
        );
        let after = exile_hand::until(blocks, |s| {
            s.turn().unwrap().position().number == 2
                && s.turn().unwrap().position().step == Step::PrecombatMain
        });
        assert!(
            !sheet(&after, victim)
                .blocking_restrictions
                .contains(&BlockingRestriction::CannotBlock)
        );
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_demon_of_fates_design_pays_life_for_one_enchantment_per_own_turn_and_pumps_from_a_sacrifice()
{
    let mut flash = Program::enchantment(fixed_cost("{4}"));
    flash.printed_keywords.insert(KeywordAbility::Flash);
    let start = game(
        vec![A, B],
        vec![
            named("Demon of Fate's Design", 1, A, ObjectDestination::Hand),
            seed(
                2,
                A,
                ObjectDestination::Hand,
                Program::enchantment(fixed_cost("{5}")),
            ),
            seed(3, A, ObjectDestination::Hand, flash),
            seed(
                4,
                A,
                ObjectDestination::Hand,
                Program::artifact(fixed_cost("{6}"), vec![]),
            ),
        ],
        &[(Colourless, 6), (Black, 3)],
        Step::PrecombatMain,
    );
    let entered =
        round(cast_untargeted(&start, 1, &[(Colourless, 4), (Black, 2)]).into_state()).into_state();
    let demon = physical(&entered, 1).id();
    let life_cast = |state: &GameState, number| GameCommand::CastWithResources {
        spell: physical(state, number).id(),
        target: None,
        payment: CastingPayment {
            alternative_cost_source: Some(demon),
            ..Default::default()
        },
    };
    illegal(&entered, A, life_cast(&entered, 4));
    let alternate = command(&entered, A, life_cast(&entered, 2)).into_state();
    assert_eq!(alternate.execution().life_total(A), Some(&15.into()));
    let landed = round(alternate).into_state();
    illegal(&landed, A, life_cast(&landed, 3));
    let pay = |number| {
        selected_cost(
            &landed,
            1,
            0,
            vec![physical(&landed, number).id()],
            None,
            &[(Colourless, 2), (Black, 1)],
        )
    };
    illegal(&landed, A, pay(1));
    let paid = command(&landed, A, pay(2)).into_state();
    assert_eq!(physical(&paid, 2).zone(), ObjectZone::Graveyard(A));
    let pumped = round(paid).into_state();
    assert_size(&pumped, demon, 11, 6);
    let opponents_turn = exile_hand::until(pumped, |s| {
        s.turn().unwrap().position().number == 2
            && s.turn().unwrap().position().step == Step::PrecombatMain
    });
    let priority = pass(&opponents_turn).into_state();
    illegal(&priority, A, life_cast(&priority, 3));
    assert_size(&priority, demon, 6, 6);
    let next = exile_hand::until(priority, |s| {
        s.turn().unwrap().position().number == 3
            && s.turn().unwrap().position().step == Step::PrecombatMain
    });
    let another = command(&next, A, life_cast(&next, 3)).into_state();
    assert_eq!(another.execution().life_total(A), Some(&11.into()));
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_nyxborn_behemoth_discounts_only_own_noncreature_enchantments_and_sacrifices_for_indestructible()
 {
    let mut creature_enchantment =
        Program::creature(fixed_cost("{5}"), 1.into(), 1.into(), Default::default());
    creature_enchantment
        .card_types
        .insert(CardType::Enchantment);
    let start = game(
        vec![A, B],
        vec![
            named("Nyxborn Behemoth", 1, A, ObjectDestination::Hand),
            seed(
                2,
                A,
                ObjectDestination::Battlefield { controller: A },
                Program::enchantment(fixed_cost("{4}")),
            ),
            seed(
                3,
                A,
                ObjectDestination::Battlefield { controller: A },
                Program::enchantment(fixed_cost("{2}")),
            ),
            seed(
                4,
                A,
                ObjectDestination::Battlefield { controller: A },
                creature_enchantment,
            ),
            seed(
                5,
                B,
                ObjectDestination::Battlefield { controller: B },
                Program::enchantment(fixed_cost("{8}")),
            ),
            remove_spell(6, A),
        ],
        &[(Colourless, 5), (Green, 3)],
        Step::PrecombatMain,
    );
    illegal(
        &start,
        A,
        GameCommand::CastUntargeted {
            spell: physical(&start, 1).id(),
            payment: Some(spend(&[(Colourless, 3), (Green, 2)])),
        },
    );
    let entered =
        round(cast_untargeted(&start, 1, &[(Colourless, 4), (Green, 2)]).into_state()).into_state();
    let behemoth = physical(&entered, 1).id();
    let pay = |n| {
        selected_cost(
            &entered,
            1,
            0,
            vec![physical(&entered, n).id()],
            None,
            &[(Colourless, 1), (Green, 1)],
        )
    };
    illegal(&entered, A, pay(1));
    illegal(&entered, A, pay(5));
    let paid = command(&entered, A, pay(2)).into_state();
    assert_eq!(physical(&paid, 2).zone(), ObjectZone::Graveyard(A));
    let protected = round(paid).into_state();
    assert!(sheet(&protected, behemoth).has_keyword(KeywordAbility::Indestructible));
    let survived =
        round(targeted(&protected, A, 6, TargetRef::Object(behemoth)).into_state()).into_state();
    assert_eq!(physical(&survived, 1).zone(), ObjectZone::Battlefield);
    let later = exile_hand::until(survived, |s| {
        s.turn().unwrap().position().number == 2
            && s.turn().unwrap().position().step == Step::PrecombatMain
    });
    assert!(!sheet(&later, behemoth).has_keyword(KeywordAbility::Indestructible));
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_loyal_retainers_sacrifice_before_own_attackers_returns_only_an_own_legendary_creature() {
    let start = game(
        vec![A, B],
        vec![
            named("Loyal Retainers", 1, A, ObjectDestination::Hand),
            named("Krenko, Mob Boss", 2, A, ObjectDestination::Graveyard),
            seed(
                3,
                A,
                ObjectDestination::Graveyard,
                Program::creature(None, 1.into(), 1.into(), Default::default()),
            ),
            named("Krenko, Mob Boss", 4, B, ObjectDestination::Graveyard),
            creature(5, A, 1, 2, &[KeywordAbility::Haste]),
        ],
        &[(Colourless, 2), (White, 1)],
        Step::PrecombatMain,
    );
    let main =
        round(cast_untargeted(&start, 1, &[(Colourless, 2), (White, 1)]).into_state()).into_state();
    let activate = |s: &GameState, id| GameCommand::ActivateAbility {
        source: physical(s, 1).id(),
        ability: ActivatedAbilityId(0),
        target: TargetRef::Object(physical(s, id).id()),
        payment: None,
    };
    for invalid in [3, 4] {
        illegal(&main, A, activate(&main, invalid));
    }
    let before = exile_hand::until(main.clone(), |s| {
        s.turn().unwrap().position().step == Step::BeginningCombat
    });
    let paid = command(&before, A, activate(&before, 2)).into_state();
    assert_eq!(physical(&paid, 1).zone(), ObjectZone::Graveyard(A));
    assert_eq!(physical(&paid, 2).zone(), ObjectZone::Graveyard(A));
    let returned = round(paid).into_state();
    assert_eq!(physical(&returned, 2).zone(), ObjectZone::Battlefield);
    let late = exile_hand::until(main, |s| {
        s.turn().unwrap().position().step == Step::PostcombatMain
    });
    illegal(&late, A, activate(&late, 2));
    let theirs = exile_hand::until(late, |s| {
        s.turn().unwrap().position().number == 2
            && s.turn().unwrap().position().step == Step::PrecombatMain
    });
    let own_priority = pass(&theirs).into_state();
    illegal(&own_priority, A, activate(&own_priority, 2));
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_whisper_taps_and_sacrifices_two_creatures_to_return_another_card_from_its_owners_graveyard()
{
    let state = game(
        vec![A, B],
        vec![
            named("Whisper, Blood Liturgist", 1, A, ObjectDestination::Hand),
            named(
                "Fervor",
                2,
                A,
                ObjectDestination::Battlefield { controller: A },
            ),
            creature(3, A, 1, 2, &[]),
            creature(4, B, 1, 2, &[]),
            seed(
                5,
                A,
                ObjectDestination::Graveyard,
                Program::creature(None, 4.into(), 4.into(), Default::default()),
            ),
            seed(
                6,
                B,
                ObjectDestination::Graveyard,
                Program::creature(None, 4.into(), 4.into(), Default::default()),
            ),
        ],
        &[(Colourless, 3), (Black, 1)],
        Step::PrecombatMain,
    );
    let entered =
        round(cast_untargeted(&state, 1, &[(Colourless, 3), (Black, 1)]).into_state()).into_state();
    let pay = |victims: Vec<u64>, target: u64| {
        selected_cost(
            &entered,
            1,
            0,
            victims
                .into_iter()
                .map(|id| physical(&entered, id).id())
                .collect(),
            Some(physical(&entered, target).id()),
            &[],
        )
    };
    illegal(&entered, A, pay(vec![1], 5));
    illegal(&entered, A, pay(vec![1, 4], 5));
    illegal(&entered, A, pay(vec![1, 3], 6));
    let choosing = command(&entered, A, pay(vec![1, 3], 5)).into_state();
    let RuleFrame::LinkedReturnOrder(frame) = choosing.pending_decision().unwrap().frame() else {
        panic!("the owner orders the sacrificed pair");
    };
    let paid = reply(
        &choosing,
        DecisionReply::LinkedReturnOrder(frame.objects().to_vec()),
    )
    .into_state();
    assert_eq!(physical(&paid, 1).zone(), ObjectZone::Graveyard(A));
    assert_eq!(physical(&paid, 3).zone(), ObjectZone::Graveyard(A));
    let resolved = round(paid).into_state();
    assert_eq!(physical(&resolved, 5).zone(), ObjectZone::Battlefield);
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_legion_vanguard_explores_after_sacrificing_another_creature_with_each_library_result() {
    use spellwake_engine::effects::LibraryTopReply;
    for land in [false, true] {
        let top = if land {
            named("Forest", 4, A, ObjectDestination::LibraryBottom)
        } else {
            seed(
                4,
                A,
                ObjectDestination::LibraryBottom,
                Program::artifact(None, vec![]),
            )
        };
        let start = game(
            vec![A, B],
            vec![
                named("Legion Vanguard", 1, A, ObjectDestination::Hand),
                creature(2, A, 1, 1, &[]),
                creature(3, B, 1, 1, &[]),
                top,
            ],
            &[(Colourless, 2), (Black, 1)],
            Step::PrecombatMain,
        );
        let entered =
            round(cast_untargeted(&start, 1, &[(Colourless, 1), (Black, 1)]).into_state())
                .into_state();
        let pay = |n| {
            selected_cost(
                &entered,
                1,
                0,
                vec![physical(&entered, n).id()],
                None,
                &[(Colourless, 1)],
            )
        };
        illegal(&entered, A, pay(1));
        illegal(&entered, A, pay(3));
        let cost = command(&entered, A, pay(2)).into_state();
        assert_eq!(physical(&cost, 2).zone(), ObjectZone::Graveyard(A));
        let resolved = round(cost).into_state();
        assert_eq!(
            sheet(&resolved, physical(&resolved, 1).id()).plus_one_counters,
            if land { 0.into() } else { 1.into() }
        );
        if land {
            assert_eq!(physical(&resolved, 4).zone(), ObjectZone::Hand(A));
        } else {
            let RuleFrame::LibraryTop(frame) = resolved.pending_decision().unwrap().frame() else {
                panic!("choose whether to put revealed nonland in graveyard");
            };
            let top = physical(&resolved, 4).id();
            assert!(frame.may_put_explored_card_in_graveyard());
            for choice in [None, Some(top)] {
                let done = reply(
                    &resolved,
                    DecisionReply::LibraryTop(LibraryTopReply::Search { selected: choice }),
                )
                .into_state();
                assert_eq!(
                    physical(&done, 4).zone(),
                    if choice.is_some() {
                        ObjectZone::Graveyard(A)
                    } else {
                        ObjectZone::Library(A)
                    }
                );
            }
        }
    }
}

fn cast_counter_creature(state: &GameState, x: i64, cost: &[(ManaType, i64)]) -> GameCommand {
    GameCommand::CastWithResources {
        spell: physical(state, 1).id(),
        target: None,
        payment: CastingPayment {
            chosen_x: Some(x.into()),
            mana: (!cost.is_empty()).then(|| spend(cost)),
            ..Default::default()
        },
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_endless_one_and_stonecoil_serpent_enter_with_exactly_their_chosen_x_and_pay_it_once() {
    use spellwake_engine::characteristics::KeywordAbility::{
        ProtectionFromMulticolored, Reach, Trample,
    };
    for (name, serpent) in [("Endless One", false), ("Stonecoil Serpent", true)] {
        let mut colored = free_instant(
            BodyTarget::SINGLE_CREATURE,
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
        );
        colored.color_indicator = Some([Color::Red, Color::Green].into());
        let start = game(
            vec![A, B],
            vec![
                named(name, 1, A, ObjectDestination::Hand),
                seed(2, A, ObjectDestination::Hand, colored),
                remove_spell(3, A),
            ],
            &[(Colourless, 4)],
            Step::PrecombatMain,
        );
        illegal(
            &start,
            A,
            cast_counter_creature(&start, 4, &[(Colourless, 3)]),
        );
        for x in [0, 1, 4] {
            let cost = if x > 0 { vec![(Colourless, x)] } else { vec![] };
            let entered =
                round(command(&start, A, cast_counter_creature(&start, x, &cost)).into_state())
                    .into_state();
            assert_eq!(entered.execution().mana(A, Colourless), (4 - x).into());
            if x == 0 {
                assert_eq!(physical(&entered, 1).zone(), ObjectZone::Graveyard(A));
                continue;
            }
            let source = physical(&entered, 1).id();
            assert_eq!(sheet(&entered, source).plus_one_counters, x.into());
            assert_size(&entered, source, x, x);
            let colored = GameCommand::Cast {
                spell: physical(&entered, 2).id(),
                target: TargetRef::Object(source),
            };
            if serpent {
                for keyword in [ProtectionFromMulticolored, Reach, Trample] {
                    assert!(sheet(&entered, source).has_keyword(keyword));
                }
                illegal(&entered, A, colored);
            } else {
                assert!(sheet(&entered, source).keywords.is_empty());
                let destroyed = round(command(&entered, A, colored).into_state()).into_state();
                assert_eq!(physical(&destroyed, 1).zone(), ObjectZone::Graveyard(A));
            }
            let destroyed = round(targeted(&entered, A, 3, TargetRef::Object(source)).into_state())
                .into_state();
            assert_eq!(physical(&destroyed, 1).zone(), ObjectZone::Graveyard(A));
        }
    }
}
#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_hangarback_walker_pays_double_x_grows_by_activation_and_creates_one_flying_artifact_thopter_per_final_counter()
 {
    use spellwake_engine::characteristics::KeywordAbility::Flying;
    let thopter = current_catalogue().1.resolve_label("Thopter").unwrap();
    let start = game(
        vec![A, B],
        vec![
            named("Hangarback Walker", 1, A, ObjectDestination::Hand),
            remove_spell(2, A),
            named(
                "Fervor",
                3,
                A,
                ObjectDestination::Battlefield { controller: A },
            ),
        ],
        &[(Colourless, 5)],
        Step::PrecombatMain,
    );
    illegal(
        &start,
        A,
        cast_counter_creature(&start, 2, &[(Colourless, 2)]),
    );
    let entered = round(
        command(
            &start,
            A,
            cast_counter_creature(&start, 2, &[(Colourless, 4)]),
        )
        .into_state(),
    )
    .into_state();
    let walker = physical(&entered, 1).id();
    assert_eq!(sheet(&entered, walker).plus_one_counters, 2.into());
    let paid = command(&entered, A, activation(&entered, 1, 0, &[(Colourless, 1)])).into_state();
    assert_eq!(physical(&paid, 1).tapped(), Some(true));
    let grown = round(paid).into_state();
    assert_size(&grown, walker, 3, 3);
    let died = round(targeted(&grown, A, 2, TargetRef::Object(walker)).into_state()).into_state();
    assert_eq!(physical(&died, 1).zone(), ObjectZone::Graveyard(A));
    let (tokens, ids) = ordered_tokens(died, 3);
    for id in ids {
        let token = sheet(&tokens, id);
        assert_eq!(token.current_creature_subtypes, [thopter].into());
        assert!(
            token.current_colors.is_empty()
                && token.has_type(CardType::Artifact)
                && token.has_keyword(Flying)
        );
        assert_size(&tokens, id, 1, 1);
    }
}
#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_lifeblood_hydra_gains_and_draws_using_final_power_including_effects_other_than_its_chosen_x()
{
    use spellwake_engine::characteristics::KeywordAbility::Trample;
    let mut cards = draw_pile();
    cards.extend([
        named("Lifeblood Hydra", 1, A, ObjectDestination::Hand),
        remove_spell(2, A),
        seed(
            3,
            A,
            ObjectDestination::Hand,
            free_instant(
                BodyTarget::SINGLE_CREATURE,
                crate::keyword_creatures::toolbox_emit::modify_group_eot(
                    spellwake_engine::program::EffectSubject::TargetObject,
                    spellwake_engine::program::ValueExpr::literal(2),
                    spellwake_engine::program::ValueExpr::literal(0),
                    [Trample].into(),
                    std::collections::BTreeSet::new(),
                ),
            ),
        ),
    ]);
    let start = game(
        vec![A, B],
        cards,
        &[(Colourless, 2), (Green, 3)],
        Step::PrecombatMain,
    );
    let entered = round(
        command(
            &start,
            A,
            cast_counter_creature(&start, 2, &[(Colourless, 2), (Green, 3)]),
        )
        .into_state(),
    )
    .into_state();
    let hydra = physical(&entered, 1).id();
    assert!(sheet(&entered, hydra).has_keyword(Trample));
    assert_eq!(sheet(&entered, hydra).plus_one_counters, 2.into());
    assert_size(&entered, hydra, 2, 2);
    let pumped =
        round(targeted(&entered, A, 3, TargetRef::Object(hydra)).into_state()).into_state();
    assert_size(&pumped, hydra, 4, 2);
    let died = round(targeted(&pumped, A, 2, TargetRef::Object(hydra)).into_state()).into_state();
    let before = hand_size(&died, A);
    let done = round(died).into_state();
    assert_eq!(done.execution().life_total(A), Some(&24.into()));
    assert_eq!(hand_size(&done, A), before + 4);
}
#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_yargle_casts_for_four_and_black_and_attacks_as_a_nine_three_legendary_frog_spirit() {
    use spellwake_engine::program::Supertype;
    let price = [(Colourless, 4), (Black, 1)];
    let start = game(
        vec![A, B],
        vec![
            named("Yargle, Glutton of Urborg", 1, A, ObjectDestination::Hand),
            named(
                "Fervor",
                2,
                A,
                ObjectDestination::Battlefield { controller: A },
            ),
        ],
        &price,
        Step::PrecombatMain,
    );
    let entered = round(cast_untargeted(&start, 1, &price).into_state()).into_state();
    let yargle = physical(&entered, 1).id();
    let row = sheet(&entered, yargle);
    assert_eq!(row.current_colors, [Color::Black].into());
    assert!(row.current_supertypes.contains(&Supertype::Legendary));
    let vocab = current_catalogue().1;
    assert_eq!(
        row.current_creature_subtypes,
        [
            vocab.resolve_label("Frog").unwrap(),
            vocab.resolve_label("Spirit").unwrap()
        ]
        .into()
    );
    assert_size(&entered, yargle, 9, 3);
    let attack = combat_frame(&entered, true);
    let attacking = reply(
        &attack,
        DecisionReply::DeclareAttackers(vec![AttackDeclaration {
            attacker: yargle,
            defender: B,
            defending_planeswalker: None,
        }]),
    )
    .into_state();
    let ended = exile_hand::until(attacking, |s| {
        s.turn().unwrap().position().step == Step::EndCombat
    });
    assert_eq!(ended.execution().life_total(B), Some(&11.into()));
}

pub(super) fn plus_counters_spell(count: i64) -> Program {
    free_instant(
        BodyTarget::SINGLE_CREATURE,
        crate::keyword_creatures::toolbox_emit::put_counters_on_target(
            spellwake_engine::effects::CounterKind::PlusOne,
            spellwake_engine::program::ValueExpr::Literal(count.into()),
        ),
    )
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_mowu_and_snakeskin_veil_apply_one_extra_counter_per_placement_and_hexproof_until_cleanup() {
    use spellwake_engine::characteristics::KeywordAbility::{Hexproof, Trample, Vigilance};
    let price = [(Colourless, 3), (Green, 1)];
    let start = game(
        vec![A, B],
        vec![
            named("Mowu, Loyal Companion", 1, A, ObjectDestination::Hand),
            named("Snakeskin Veil", 2, A, ObjectDestination::Hand),
            seed(3, A, ObjectDestination::Hand, plus_counters_spell(2)),
            creature(4, B, 1, 1, &[]),
            creature(5, A, 1, 1, &[]),
            remove_spell(6, B),
        ],
        &[(Colourless, 3), (Green, 2)],
        Step::PrecombatMain,
    );
    let field = round(cast_untargeted(&start, 1, &price).into_state()).into_state();
    let mowu = physical(&field, 1).id();
    let row = sheet(&field, mowu);
    assert!(row.has_keyword(Vigilance) && row.has_keyword(Trample));
    let spell = |n| GameCommand::CastPaid {
        spell: physical(&field, 2).id(),
        target: TargetRef::Object(physical(&field, n).id()),
        payment: spend(&[(Green, 1)]),
    };
    illegal(&field, A, spell(4));
    let plain = round(command(&field, A, spell(5)).into_state()).into_state();
    assert_eq!(
        sheet(&plain, physical(&plain, 5).id()).plus_one_counters,
        1.into()
    );
    let guarded = round(command(&field, A, spell(1)).into_state()).into_state();
    assert_eq!(sheet(&guarded, mowu).plus_one_counters, 2.into());
    assert!(sheet(&guarded, mowu).has_keyword(Hexproof));
    let enemy = pass(&guarded).into_state();
    illegal(
        &enemy,
        B,
        GameCommand::Cast {
            spell: physical(&enemy, 6).id(),
            target: TargetRef::Object(mowu),
        },
    );
    let bigger = round(targeted(&guarded, A, 3, TargetRef::Object(mowu)).into_state()).into_state();
    assert_eq!(sheet(&bigger, mowu).plus_one_counters, 5.into());
    let next = exile_hand::until(bigger, |s| {
        s.turn().unwrap().position().number == 2
            && s.turn().unwrap().position().step == Step::PrecombatMain
    });
    assert!(!sheet(&next, mowu).has_keyword(Hexproof));
    assert_size(&next, mowu, 8, 8);
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_ainok_bond_kin_and_tuskguard_captain_outlast_at_sorcery_speed_and_grant_their_keyword_to_controlled_countered_creatures()
 {
    use spellwake_engine::characteristics::KeywordAbility::{FirstStrike, Trample};
    for (name, color, base, outlast, keyword) in [
        ("Ainok Bond-Kin", White, 1, 1, FirstStrike),
        ("Tuskguard Captain", Green, 2, 0, Trample),
    ] {
        let start = game(
            vec![A, B],
            vec![
                named(name, 1, A, ObjectDestination::Hand),
                creature(2, A, 1, 1, &[]),
                creature(3, B, 1, 1, &[]),
                seed(4, A, ObjectDestination::Hand, plus_counters_spell(1)),
                seed(5, A, ObjectDestination::Hand, plus_counters_spell(1)),
                named(
                    "Fervor",
                    6,
                    A,
                    ObjectDestination::Battlefield { controller: A },
                ),
                remove_spell(7, A),
            ],
            &[(Colourless, base + outlast), (color, 2)],
            Step::PrecombatMain,
        );
        let field =
            round(cast_untargeted(&start, 1, &[(Colourless, base), (color, 1)]).into_state())
                .into_state();
        let source = physical(&field, 1).id();
        let us = physical(&field, 2).id();
        let them = physical(&field, 3).id();
        assert!(!sheet(&field, source).has_keyword(keyword));
        let waiting = targeted(&field, A, 4, TargetRef::Object(us)).into_state();
        let price = if outlast == 0 {
            vec![(color, 1)]
        } else {
            vec![(Colourless, outlast), (color, 1)]
        };
        illegal(&waiting, A, activation(&waiting, 1, 1, &price));
        let ours = round(waiting).into_state();
        assert!(sheet(&ours, us).has_keyword(keyword));
        let countered =
            round(targeted(&ours, A, 5, TargetRef::Object(them)).into_state()).into_state();
        assert!(!sheet(&countered, them).has_keyword(keyword));
        let paid = command(&countered, A, activation(&countered, 1, 1, &price)).into_state();
        assert_eq!(physical(&paid, 1).tapped(), Some(true));
        assert_eq!(sheet(&paid, source).plus_one_counters, 0.into());
        let done = round(paid).into_state();
        assert_eq!(sheet(&done, source).plus_one_counters, 1.into());
        assert!(sheet(&done, source).has_keyword(keyword));
        let removed =
            round(targeted(&done, A, 7, TargetRef::Object(source)).into_state()).into_state();
        assert!(!sheet(&removed, us).has_keyword(keyword));
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_wildwood_scourge_enters_with_x_and_grows_once_for_each_placement_on_another_controlled_nonhydra()
 {
    let hydra = current_catalogue().1.resolve_label("Hydra").unwrap();
    let start = game(
        vec![A, B],
        vec![
            named("Wildwood Scourge", 1, A, ObjectDestination::Hand),
            creature(2, A, 1, 1, &[]),
            seed(
                3,
                A,
                ObjectDestination::Battlefield { controller: A },
                Program::creature(None, 3.into(), 3.into(), [hydra].into()),
            ),
            creature(4, B, 2, 2, &[]),
            seed(5, A, ObjectDestination::Hand, plus_counters_spell(2)),
        ],
        &[(Colourless, 2), (Green, 1)],
        Step::PrecombatMain,
    );
    let field = round(
        command(
            &start,
            A,
            cast_counter_creature(&start, 2, &[(Colourless, 2), (Green, 1)]),
        )
        .into_state(),
    )
    .into_state();
    let source = physical(&field, 1).id();
    assert_eq!(sheet(&field, source).plus_one_counters, 2.into());
    for n in [1, 2, 3, 4] {
        let target = physical(&field, n).id();
        let changed =
            round(targeted(&field, A, 5, TargetRef::Object(target)).into_state()).into_state();
        assert_eq!(
            sheet(&changed, target).plus_one_counters,
            if n == 1 { 4.into() } else { 2.into() }
        );
        if n == 2 {
            assert_eq!(changed.execution().window().entries().len(), 1);
            let done = round(changed).into_state();
            assert_eq!(sheet(&done, source).plus_one_counters, 3.into());
        } else {
            assert!(changed.execution().window().entries().is_empty());
        }
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_rishkar_targets_zero_to_two_creatures_and_grants_green_mana_to_each_controlled_creature_with_any_counter()
 {
    use spellwake_engine::mana::ManaAbilityRef;
    let start = game(
        vec![A, B],
        vec![
            named("Rishkar, Peema Renegade", 1, A, ObjectDestination::Hand),
            creature(2, A, 1, 1, &[KeywordAbility::Haste]),
            creature(3, B, 1, 1, &[]),
            creature(4, A, 2, 2, &[KeywordAbility::Haste]),
            seed(
                5,
                A,
                ObjectDestination::Hand,
                free_instant(
                    BodyTarget::SINGLE_CREATURE,
                    crate::keyword_creatures::toolbox_emit::put_counters_on_target(
                        spellwake_engine::effects::CounterKind::Indestructible,
                        spellwake_engine::program::ValueExpr::literal(1),
                    ),
                ),
            ),
            remove_spell(6, A),
            creature(7, A, 1, 1, &[]),
        ],
        &[(Colourless, 2), (Green, 1)],
        Step::PrecombatMain,
    );
    let shield =
        round(targeted(&start, A, 5, TargetRef::Object(physical(&start, 4).id())).into_state())
            .into_state();
    let choice = round(cast_untargeted(&shield, 1, &[(Colourless, 2), (Green, 1)]).into_state())
        .into_state();
    let choose = |ids: &[u64]| {
        DecisionReply::TriggerPermanentTargets(
            ids.iter()
                .map(|&n| TargetRef::Object(physical(&choice, n).id()))
                .collect(),
        )
    };
    illegal(
        &choice,
        A,
        GameCommand::Reply {
            decision: choice.pending_decision().unwrap().id(),
            choice: choose(&[2, 2]),
        },
    );
    for ids in [vec![], vec![1], vec![2, 3]] {
        let ready = round(reply(&choice, choose(&ids)).into_state()).into_state();
        for n in [1, 2, 3, 4, 7] {
            let source = physical(&ready, n).id();
            let view = sheet(&ready, source);
            assert_eq!(view.plus_one_counters, i64::from(ids.contains(&n)).into());
            let granted = n == 4 || (ids.contains(&n) && n != 3);
            assert_eq!(!view.granted_fixed_mana.is_empty(), granted);
            if granted && n != 1 {
                let (&key, &color) = view.granted_fixed_mana.first_key_value().unwrap();
                assert_eq!(color, Green);
                let command_for = GameCommand::ActivateMana {
                    source,
                    ability: ManaAbilityRef::GrantedFixedColor(key, color),
                };
                let mana = command(&ready, A, command_for.clone()).into_state();
                assert_eq!(mana.execution().mana(A, Green), 1.into());
                assert_eq!(physical(&mana, n).tapped(), Some(true));
                let gone = round(
                    targeted(&ready, A, 6, TargetRef::Object(physical(&ready, 1).id()))
                        .into_state(),
                )
                .into_state();
                illegal(&gone, A, command_for);
            }
        }
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_omarthis_optionally_grows_once_per_counter_placement_on_another_colorless_creature_and_manifests_its_final_counter_total()
 {
    use spellwake_engine::{
        effects::OptionalEffectChoice,
        face_down::FaceDownKind,
        mana::{ManaAbilityId, ManaAbilityRef},
    };
    let mut cards = draw_pile();
    cards.extend([
        named(
            "Omarthis, Ghostfire Initiate",
            1,
            A,
            ObjectDestination::Hand,
        ),
        creature(2, B, 1, 1, &[]),
        named(
            "Yargle, Glutton of Urborg",
            3,
            A,
            ObjectDestination::Battlefield { controller: A },
        ),
        seed(4, A, ObjectDestination::Hand, plus_counters_spell(2)),
        seed(
            5,
            A,
            ObjectDestination::Hand,
            free_instant(
                BodyTarget::SINGLE_CREATURE,
                crate::keyword_creatures::toolbox_emit::put_counters_on_target(
                    spellwake_engine::effects::CounterKind::Indestructible,
                    spellwake_engine::program::ValueExpr::literal(1),
                ),
            ),
        ),
        named(
            "Ashnod's Altar",
            6,
            A,
            ObjectDestination::Battlefield { controller: A },
        ),
    ]);
    let start = game(vec![A, B], cards, &[(Colourless, 4)], Step::PrecombatMain);
    illegal(
        &start,
        A,
        cast_counter_creature(&start, 2, &[(Colourless, 2)]),
    );
    let ready = round(
        command(
            &start,
            A,
            cast_counter_creature(&start, 2, &[(Colourless, 4)]),
        )
        .into_state(),
    )
    .into_state();
    let source = physical(&ready, 1).id();
    assert_eq!(sheet(&ready, source).plus_one_counters, 2.into());
    for n in [1, 3] {
        let ignored =
            round(targeted(&ready, A, 4, TargetRef::Object(physical(&ready, n).id())).into_state())
                .into_state();
        assert!(ignored.execution().window().entries().is_empty());
    }
    let changed =
        round(targeted(&ready, A, 4, TargetRef::Object(physical(&ready, 2).id())).into_state())
            .into_state();
    assert_eq!(
        sheet(&changed, physical(&changed, 2).id()).plus_one_counters,
        2.into()
    );
    assert_eq!(changed.execution().window().entries().len(), 1);
    let offer = round(changed).into_state();
    let no = reply(
        &offer,
        DecisionReply::OptionalEffect(OptionalEffectChoice::Decline),
    )
    .into_state();
    assert_eq!(sheet(&no, source).plus_one_counters, 2.into());
    let yes = reply(
        &offer,
        DecisionReply::OptionalEffect(OptionalEffectChoice::Take),
    )
    .into_state();
    assert_eq!(sheet(&yes, source).plus_one_counters, 3.into());
    let shielded = round(targeted(&yes, A, 5, TargetRef::Object(source)).into_state()).into_state();
    let dying = command(
        &shielded,
        A,
        GameCommand::ActivateManaWithCostObjects {
            source: physical(&shielded, 6).id(),
            ability: ManaAbilityRef::Printed(ManaAbilityId(0)),
            objects: vec![source],
        },
    )
    .into_state();
    assert_eq!(physical(&dying, 1).zone(), ObjectZone::Graveyard(A));
    let done = round(dying).into_state();
    for n in 13..17 {
        let card = physical(&done, n);
        assert_eq!(card.zone(), ObjectZone::Battlefield);
        assert_eq!(card.face_down(), Some(FaceDownKind::Manifest));
        assert_size(&done, card.id(), 2, 2);
    }
    assert_eq!(physical(&done, 17).zone(), ObjectZone::Library(A));
}

fn test_token(
    state: &GameState,
    id: ObjectId,
    kind: &str,
    power: i64,
    toughness: i64,
    artifact: bool,
    color: Option<Color>,
) {
    let current = sheet(state, id);
    assert!(current.is_creature());
    assert_eq!(current.has_type(CardType::Artifact), artifact);
    assert_eq!(
        current.current_creature_subtypes,
        [current_catalogue().1.resolve_label(kind).unwrap()].into()
    );
    assert_eq!(current.current_colors, color.into_iter().collect());
    assert_size(state, id, power, toughness);
}
fn seasoned(name: &str, mut rest: Vec<SeedCard>, price: &[(ManaType, i64)]) -> GameState {
    let mut cards = draw_pile();
    cards.extend([
        named(name, 1, A, ObjectDestination::Battlefield { controller: A }),
        named(
            "Fervor",
            91,
            A,
            ObjectDestination::Battlefield { controller: A },
        ),
    ]);
    cards.append(&mut rest);
    game(vec![A, B], cards, price, Step::PrecombatMain)
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_oviya_builds_a_servo_or_a_construct_sized_at_resolution_and_the_idol_draws_only_after_its_controller_created_a_token()
 {
    let field = seasoned(
        "Oviya Pashiri, Sage Lifecrafter",
        vec![
            named(
                "Idol of Oblivion",
                2,
                A,
                ObjectDestination::Battlefield { controller: A },
            ),
            creature(3, A, 1, 2, &[]),
            creature(4, B, 2, 2, &[]),
            remove_spell(5, A),
        ],
        &[(Colourless, 8), (Green, 1)],
    );
    illegal(&field, A, activation(&field, 2, 0, &[]));
    let servo = command(
        &field,
        A,
        activation(&field, 1, 0, &[(Colourless, 2), (Green, 1)]),
    )
    .into_state();
    assert_eq!(physical(&servo, 1).tapped(), Some(true));
    let (made, tokens) = ordered_tokens(servo, 1);
    test_token(&made, tokens[0], "Servo", 1, 1, true, None);
    let before = hand_size(&made, A);
    let drawn = round(command(&made, A, activation(&made, 2, 0, &[])).into_state()).into_state();
    assert_eq!(hand_size(&drawn, A), before + 1);
    let next = exile_hand::until(drawn, |s| {
        s.turn().unwrap().position().number == 3
            && s.turn().unwrap().position().step == Step::PrecombatMain
    });
    illegal(&next, A, activation(&next, 2, 0, &[]));
    let stack = command(
        &field,
        A,
        activation(&field, 1, 1, &[(Colourless, 4), (Green, 1)]),
    )
    .into_state();
    let cut =
        round(targeted(&stack, A, 5, TargetRef::Object(physical(&stack, 3).id())).into_state())
            .into_state();
    for (state, n) in [(stack, 2), (cut, 1)] {
        let (done, tokens) = ordered_tokens(state, 1);
        test_token(&done, tokens[0], "Construct", n, n, true, None);
    }
    let paid = command(&field, A, activation(&field, 2, 1, &[(Colourless, 8)])).into_state();
    assert_eq!(physical(&paid, 2).zone(), ObjectZone::Graveyard(A));
    let (eldrazi, tokens) = ordered_tokens(paid, 1);
    test_token(&eldrazi, tokens[0], "Eldrazi", 10, 10, false, None);
}
#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_krenko_counts_only_controlled_goblins_on_resolution_and_makes_that_many_red_goblin_tokens() {
    let tribe = current_catalogue().1.resolve_label("Goblin").unwrap();
    let ally = |number, owner| {
        seed(
            number,
            owner,
            ObjectDestination::Battlefield { controller: owner },
            Program::creature(None, 1.into(), 1.into(), [tribe].into()),
        )
    };
    let field = seasoned(
        "Krenko, Mob Boss",
        vec![
            ally(2, A),
            ally(3, B),
            creature(4, A, 2, 2, &[]),
            remove_spell(5, A),
        ],
        &[],
    );
    let stack = command(&field, A, activation(&field, 1, 0, &[])).into_state();
    assert_eq!(physical(&stack, 1).tapped(), Some(true));
    let reduced =
        round(targeted(&stack, A, 5, TargetRef::Object(physical(&stack, 2).id())).into_state())
            .into_state();
    for (state, n) in [(stack, 2), (reduced, 1)] {
        let (done, tokens) = ordered_tokens(state, n);
        for id in tokens {
            test_token(&done, id, "Goblin", 1, 1, false, Some(Color::Red));
        }
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_aryel_makes_a_vigilant_white_knight_or_taps_distinct_other_allied_knights_to_destroy_a_creature_with_power_at_most_x()
 {
    use spellwake_engine::characteristics::KeywordAbility::Vigilance;
    let tribe = current_catalogue().1.resolve_label("Knight").unwrap();
    let ally = |n, player| {
        seed(
            n,
            player,
            ObjectDestination::Battlefield { controller: player },
            Program::creature(None, 1.into(), 1.into(), [tribe].into()),
        )
    };
    let field = seasoned(
        "Aryel, Knight of Windgrace",
        vec![
            ally(2, A),
            ally(3, A),
            ally(4, B),
            creature(5, B, 2, 2, &[]),
            creature(6, A, 2, 2, &[]),
            creature(7, B, 0, 3, &[]),
        ],
        &[(White, 1), (Colourless, 2), (Black, 1)],
    );
    assert!(sheet(&field, physical(&field, 1).id()).has_keyword(Vigilance));
    let (made, tokens) = ordered_tokens(
        command(
            &field,
            A,
            activation(&field, 1, 0, &[(Colourless, 2), (White, 1)]),
        )
        .into_state(),
        1,
    );
    test_token(&made, tokens[0], "Knight", 2, 2, false, Some(Color::White));
    assert!(sheet(&made, tokens[0]).has_keyword(Vigilance));
    let attack = |helpers: &[u64], n| {
        selected_cost(
            &field,
            1,
            1,
            helpers
                .iter()
                .map(|id| physical(&field, *id).id())
                .collect(),
            Some(physical(&field, n).id()),
            &[(Black, 1)],
        )
    };
    for bad in [&[2][..], &[2, 2], &[1, 2], &[2, 4], &[2, 6]] {
        illegal(&field, A, attack(bad, 5));
    }
    for (helpers, prey) in [(vec![2, 3], 5), (vec![], 7)] {
        let tap = command(&field, A, attack(&helpers, prey)).into_state();
        assert_eq!(physical(&tap, 1).tapped(), Some(true));
        for id in helpers {
            assert_eq!(physical(&tap, id).tapped(), Some(true));
        }
        let end = round(tap).into_state();
        assert_eq!(physical(&end, prey).zone(), ObjectZone::Graveyard(B));
    }
}
#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_endbringer_can_ping_disable_combat_or_draw_and_untaps_again_during_each_opponents_untap() {
    use spellwake_engine::characteristics::BlockingRestriction::CannotAttackOrBlock;
    let mut cards = draw_pile();
    cards.extend([
        named(
            "Endbringer",
            1,
            A,
            ObjectDestination::Battlefield { controller: A },
        ),
        named(
            "Fervor",
            91,
            A,
            ObjectDestination::Battlefield { controller: A },
        ),
        creature(2, B, 2, 2, &[]),
    ]);
    let field = game(
        vec![A, B, C],
        cards,
        &[(Colourless, 2)],
        Step::PrecombatMain,
    );
    let target = |s: &GameState, id, aim, amount| GameCommand::ActivateAbility {
        source: physical(s, 1).id(),
        ability: ActivatedAbilityId(id),
        target: aim,
        payment: (amount > 0).then(|| spend(&[(Colourless, amount)])),
    };
    let pinged = round(command(&field, A, target(&field, 0, TargetRef::Player(B), 0)).into_state())
        .into_state();
    assert_eq!(pinged.execution().life_total(B), Some(&19.into()));
    assert_eq!(physical(&pinged, 1).tapped(), Some(true));
    let before = hand_size(&field, A);
    let drawn =
        round(command(&field, A, activation(&field, 1, 2, &[(Colourless, 2)])).into_state())
            .into_state();
    assert_eq!(hand_size(&drawn, A), before + 1);
    let disabled = round(
        command(
            &field,
            A,
            target(&field, 1, TargetRef::Object(physical(&field, 2).id()), 1),
        )
        .into_state(),
    )
    .into_state();
    assert!(
        sheet(&disabled, physical(&disabled, 2).id())
            .blocking_restrictions
            .contains(&CannotAttackOrBlock)
    );
    let b = exile_hand::until(disabled, |s| {
        s.turn().unwrap().position().number == 2
            && s.turn().unwrap().position().step == Step::PrecombatMain
    });
    assert_eq!(physical(&b, 1).tapped(), Some(false));
    assert!(
        !sheet(&b, physical(&b, 2).id())
            .blocking_restrictions
            .contains(&CannotAttackOrBlock)
    );
    let a = pass(&pass(&b).into_state()).into_state();
    let use_again =
        round(command(&a, A, target(&a, 0, TargetRef::Player(C), 0)).into_state()).into_state();
    assert_eq!(use_again.execution().life_total(C), Some(&19.into()));
    let c = exile_hand::until(use_again, |s| {
        s.turn().unwrap().position().number == 3
            && s.turn().unwrap().position().step == Step::PrecombatMain
    });
    assert_eq!(physical(&c, 1).tapped(), Some(false));
}
