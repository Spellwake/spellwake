//! Linked exile, zone ownership and multiple copies of the same imprint ability.
use super::equipment::{cast_card, marker, optional, sheet, stats};
use super::*;

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_duplicant_and_phyrexian_ingester_keep_distinct_imprint_values_when_the_entry_ability_is_copied()
 {
    use ManaType::*;
    use spellwake_engine::activated::{ActivatedAbility, SourceCost};
    use spellwake_engine::effects::CopyTargetChoice;
    for (name, price, ingester) in [
        ("Duplicant", vec![(Colourless, 6)], false),
        ("Phyrexian Ingester", vec![(Colourless, 6), (Blue, 1)], true),
    ] {
        let mut copier = Program::artifact(None, vec![]);
        copier.activated.push(ActivatedAbility {
            id: ActivatedAbilityId(0),
            mana_cost: ManaCost::free(),
            source_cost: SourceCost::None,
            body: ExecutableBody::new(
                BodyTarget::ControlledAbility {
                    colorless_source: false,
                },
                vec![Instruction::CopyAbility],
            ),
        });
        let mut maker = named("Dragon Fodder", 7, A, ObjectDestination::Hand);
        let ProgramKind::Spell(def) = &maker.program.kind else {
            panic!("token maker");
        };
        maker.program = Program::instant(ManaCost::free(), def.body.clone());
        let cards = vec![
            named(name, 1, A, ObjectDestination::Hand),
            named("Goliath Sphinx", 2, B, battlefield(B)),
            named("Elvish Mystic", 3, B, battlefield(B)),
            seed(4, A, battlefield(A), copier),
            maker,
        ];
        let init = game(vec![A, B], cards, &price, Step::PrecombatMain);
        let pre =
            super::equipment::order_tokens(round(cast_untargeted(&init, 7, &[]).into_state()))
                .into_state();
        let token = pre
            .execution()
            .window()
            .cards()
            .iter()
            .find(|o| o.is_token())
            .unwrap()
            .id();
        let offer = cast_card(&pre, 1, &price);
        super::declarations::invalid(
            &offer,
            A,
            DecisionReply::TriggerTarget(TargetRef::Object(token)),
        );
        let target = reply(&offer, DecisionReply::TriggerTarget(marker(&offer, 2))).into_state();
        let source = physical(&target, 1).id();
        let may = round(target.clone()).into_state();
        let no = optional(&may, false);
        stats(
            &no,
            source,
            if ingester { 3 } else { 2 },
            if ingester { 3 } else { 4 },
        );
        let single = optional(&may, true);
        assert_eq!(physical(&single, 2).zone(), ObjectZone::Exile);
        stats(
            &single,
            source,
            if ingester { 11 } else { 8 },
            if ingester { 10 } else { 7 },
        );
        let original = target.execution().window().entries().last().unwrap().id();
        let copy = round(
            command(
                &target,
                A,
                GameCommand::ActivateAbility {
                    source: physical(&target, 4).id(),
                    ability: ActivatedAbilityId(0),
                    target: TargetRef::Stack(original),
                    payment: None,
                },
            )
            .into_state(),
        )
        .into_state();
        let retarget = reply(
            &copy,
            DecisionReply::CopyTargets(CopyTargetChoice::Change(marker(&copy, 3))),
        )
        .into_state();
        let first = optional(&round(retarget).into_state(), true);
        assert_eq!(physical(&first, 3).zone(), ObjectZone::Exile);
        stats(
            &first,
            source,
            if ingester { 4 } else { 1 },
            if ingester { 4 } else { 1 },
        );
        let second = optional(&round(first).into_state(), true);
        assert_eq!(physical(&second, 2).zone(), ObjectZone::Exile);
        stats(
            &second,
            source,
            if ingester { 12 } else { 8 },
            if ingester { 11 } else { 7 },
        );
        if !ingester {
            assert_eq!(
                sheet(&second, source).current_creature_subtypes,
                ["Sphinx", "Shapeshifter"]
                    .map(|k| current_catalogue().1.resolve_label(k).unwrap())
                    .into()
            );
        }
        assert!(!sheet(&second, source).has_keyword(KeywordAbility::Flying));
    }
}
#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_oblivion_sower_exiles_exactly_four_on_cast_and_can_take_any_of_that_opponents_preexisting_and_newly_exiled_lands()
 {
    use ManaType::*;
    let price = [(Colourless, 6)];
    let initial = game(
        vec![A, B, C],
        vec![
            named("Oblivion Sower", 1, A, ObjectDestination::Hand),
            named("Island", 2, B, ObjectDestination::Exile),
            named("Plains", 3, C, ObjectDestination::Exile),
            named("Sol Ring", 4, B, ObjectDestination::Exile),
            named("Island", 10, B, ObjectDestination::LibraryBottom),
            named("Sol Ring", 11, B, ObjectDestination::LibraryBottom),
            named("Forest", 12, B, ObjectDestination::LibraryBottom),
            named("Elvish Mystic", 13, B, ObjectDestination::LibraryBottom),
            named("Swamp", 14, B, ObjectDestination::LibraryBottom),
        ],
        &price,
        Step::PrecombatMain,
    );
    let choose = cast_untargeted(&initial, 1, &price).into_state();
    super::declarations::invalid(
        &choose,
        A,
        DecisionReply::TriggerTarget(TargetRef::Player(A)),
    );
    let stack = reply(&choose, DecisionReply::TriggerTarget(TargetRef::Player(B))).into_state();
    let offer = round(stack).into_state();
    assert_eq!(physical(&offer, 1).zone(), ObjectZone::Stack);
    for n in 10..=13 {
        assert_eq!(physical(&offer, n).zone(), ObjectZone::Exile);
    }
    assert_eq!(
        offer.execution().window().cards().library(B)[0],
        physical(&offer, 14).id()
    );
    for n in [3, 4, 11, 13] {
        super::declarations::invalid(
            &offer,
            A,
            DecisionReply::SelectResolution(vec![physical(&offer, n).id()]),
        );
    }
    for selected in [vec![], vec![2], vec![2, 10, 12]] {
        let mut chosen = reply(
            &offer,
            DecisionReply::SelectResolution(
                selected.iter().map(|&n| physical(&offer, n).id()).collect(),
            ),
        );
        if selected.len() > 1 {
            let Some(RuleFrame::LinkedReturnOrder(frame)) =
                chosen.state().pending_decision().map(|p| p.frame())
            else {
                panic!("the entering lands require a timestamp order");
            };
            let lands = selected
                .iter()
                .map(|&n| physical(&offer, n).id())
                .collect::<Vec<_>>();
            assert_eq!(frame.objects(), lands);
            chosen = reply(chosen.state(), DecisionReply::LinkedReturnOrder(lands));
        }
        let changed = super::equipment::order_tokens(chosen).into_state();
        for n in [2, 10, 12] {
            let card = physical(&changed, n);
            assert_eq!(
                card.zone(),
                if selected.contains(&n) {
                    ObjectZone::Battlefield
                } else {
                    ObjectZone::Exile
                }
            );
            if selected.contains(&n) {
                assert_eq!(card.owner(), B);
                assert_eq!(card.controller(), Some(A));
            }
        }
        let result = round(changed).into_state();
        assert_eq!(physical(&result, 1).zone(), ObjectZone::Battlefield);
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_crashing_drawbridge_taps_to_grant_only_existing_controlled_creatures_haste_until_cleanup() {
    use ManaType::*;
    let bridge = named("Crashing Drawbridge", 1, A, ObjectDestination::Hand);
    let ability = bridge.program.activated[0].id;
    let initial = game(
        vec![A, B],
        vec![
            bridge,
            named("Island", 8, B, ObjectDestination::LibraryBottom),
            seed(
                2,
                A,
                ObjectDestination::Hand,
                Program::creature(
                    Some(ManaCost::free()),
                    2.into(),
                    3.into(),
                    Default::default(),
                ),
            ),
            seed(
                3,
                A,
                ObjectDestination::Hand,
                Program::creature(
                    Some(ManaCost::free()),
                    1.into(),
                    1.into(),
                    Default::default(),
                ),
            ),
            creature(4, B, 1, 2, &[]),
        ],
        &[(Colourless, 2)],
        Step::PrecombatMain,
    );
    let cast = cast_card(&initial, 1, &[(Colourless, 2)]);
    let span = physical(&cast, 1).id();
    assert!(sheet(&cast, span).has_keyword(KeywordAbility::Defender));
    let tap = GameCommand::ActivateUntargetedAbility {
        source: span,
        ability,
        payment: None,
    };
    illegal(&cast, A, tap.clone());
    let next = super::copy_permanents::main_on(cast, 3);
    let ready = cast_card(&next, 2, &[]);
    let haste = round(command(&ready, A, tap).into_state()).into_state();
    assert_eq!(physical(&haste, 1).tapped(), Some(true));
    for n in [1, 2, 4] {
        assert_eq!(
            sheet(&haste, physical(&haste, n).id()).has_keyword(KeywordAbility::Haste),
            n != 4
        );
    }
    let late = cast_card(&haste, 3, &[]);
    assert!(!sheet(&late, physical(&late, 3).id()).has_keyword(KeywordAbility::Haste));
    let step = super::declarations::combat_on(late, 3);
    super::declarations::invalid(
        &step,
        A,
        super::declarations::attacks(&step, &[(3, B, None)]),
    );
    let swing = reply(&step, super::declarations::attacks(&step, &[(2, B, None)])).into_state();
    let expired = super::copy_permanents::main_on(swing, 4);
    assert!(!sheet(&expired, physical(&expired, 2).id()).has_keyword(KeywordAbility::Haste));
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_warping_wail_exiles_either_low_stat_counters_only_a_sorcery_or_makes_a_scion_that_sacrifices_for_colorless()
 {
    use ManaType::*;
    let price = [(Colourless, 2)];
    let cards = vec![
        named("Warping Wail", 1, A, ObjectDestination::Hand),
        creature(2, B, 1, 4, &[]),
        creature(3, B, 4, 1, &[]),
        creature(4, B, 2, 2, &[]),
        seed(
            5,
            A,
            ObjectDestination::Hand,
            Program::sorcery(
                ManaCost::free(),
                ExecutableBody::new(
                    BodyTarget::None,
                    vec![Instruction::DrawController {
                        amount: spellwake_engine::program::ValueExpr::Literal(1.into()),
                    }],
                ),
            ),
        ),
        seed(
            6,
            A,
            ObjectDestination::Hand,
            free_instant(
                BodyTarget::None,
                Instruction::DrawController {
                    amount: spellwake_engine::program::ValueExpr::Literal(1.into()),
                },
            ),
        ),
    ];
    let ready = game(vec![A, B], cards, &price, Step::PrecombatMain);
    let cast = |s: &GameState, mode: u8, target: Option<TargetRef>, mana: &[(ManaType, i64)]| {
        GameCommand::CastWithResources {
            spell: physical(s, 1).id(),
            target: None,
            payment: CastingPayment {
                modes: vec![spellwake_engine::mana::CastMode {
                    mode,
                    target,
                    targets: vec![],
                }],
                mana: Some(spend(mana)),
                ..Default::default()
            },
        }
    };
    illegal(&ready, A, cast(&ready, 0, Some(marker(&ready, 4)), &price));
    for n in [2, 3] {
        let out = round(
            command(&ready, A, cast(&ready, 0, Some(marker(&ready, n)), &price)).into_state(),
        )
        .into_state();
        assert_eq!(physical(&out, n).zone(), ObjectZone::Exile);
    }
    for (id, legal) in [(5, true), (6, false)] {
        let priority = cast_untargeted(&ready, id, &[]).into_state();
        let entry = priority.execution().window().entries().last().unwrap().id();
        let cmd = cast(&priority, 1, Some(TargetRef::Stack(entry)), &price);
        if legal {
            let stopped = round(command(&priority, A, cmd).into_state()).into_state();
            assert_eq!(physical(&stopped, id).zone(), ObjectZone::Graveyard(A));
            assert!(stopped.execution().window().entries().is_empty());
        } else {
            illegal(&priority, A, cmd);
        }
    }
    let made = round(command(&ready, A, cast(&ready, 2, None, &price)).into_state());
    let spawn = tokens(&made, A, 1)[0];
    let s = made.into_state();
    let card = sheet(&s, spawn);
    assert!(card.current_colors.is_empty());
    assert_eq!(
        card.current_creature_subtypes,
        ["Eldrazi", "Scion"]
            .map(|n| current_catalogue().1.resolve_label(n).unwrap())
            .into()
    );
    stats(&s, spawn, 1, 1);
    let sacrificed = command(
        &s,
        A,
        GameCommand::ActivateMana {
            source: spawn,
            ability: ManaAbilityRef::Printed(ManaAbilityId(0)),
        },
    )
    .into_state();
    assert_eq!(sacrificed.execution().mana(A, Colourless), 1.into());
    assert!(sacrificed.execution().window().cards().get(spawn).is_none());
}
#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_zacama_untaps_only_its_controllers_lands_if_cast_and_has_three_separate_colored_activations()
{
    use ManaType::*;
    let mut other = super::loyalty::mana_source(4, &[(Blue, 1)]);
    other.owner = B;
    other.destination = battlefield(B);
    let enter = Instruction::PutHandPermanentsOntoBattlefield {
        player: spellwake_engine::program::ResolvingPlayer::Controller,
        types: [CardType::Creature].into(),
        maximum: 1,
        tapped: false,
    };
    let cards = vec![
        named("Zacama, Primal Calamity", 1, A, ObjectDestination::Hand),
        super::loyalty::mana_source(2, &[(Green, 1)]),
        named("Sol Ring", 3, A, battlefield(A)),
        other,
        creature(5, B, 1, 4, &[]),
        seed(6, B, battlefield(B), Program::enchantment(None)),
        seed(
            7,
            A,
            ObjectDestination::Hand,
            free_instant(BodyTarget::None, enter),
        ),
    ];
    let initial = game(
        vec![A, B],
        cards,
        &[(Colourless, 8), (Red, 2), (Green, 2), (White, 2)],
        Step::PrecombatMain,
    );
    let tap = |s: &GameState, n, owner| {
        command(
            s,
            owner,
            GameCommand::ActivateMana {
                source: physical(s, n).id(),
                ability: ManaAbilityRef::Printed(ManaAbilityId(0)),
            },
        )
        .into_state()
    };
    let land = tap(&initial, 2, A);
    let artifact = tap(&land, 3, A);
    let opp = pass(&artifact).into_state();
    let tapped = tap(&opp, 4, B);
    let setup = pass(&tapped).into_state();
    for n in [2, 3, 4] {
        assert_eq!(physical(&setup, n).tapped(), Some(true));
    }
    let free = round(cast_untargeted(&setup, 7, &[]).into_state()).into_state();
    let uncast = reply(
        &free,
        DecisionReply::SelectResolution(vec![physical(&free, 1).id()]),
    )
    .into_state();
    assert_eq!(physical(&uncast, 1).zone(), ObjectZone::Battlefield);
    assert_eq!(physical(&uncast, 2).tapped(), Some(true));
    assert!(uncast.execution().window().entries().is_empty());
    let entered = cast_card(
        &setup,
        1,
        &[(Colourless, 6), (Red, 1), (Green, 1), (White, 1)],
    );
    let ready = round(entered).into_state();
    let dino = physical(&ready, 1).id();
    for key in [
        KeywordAbility::Reach,
        KeywordAbility::Vigilance,
        KeywordAbility::Trample,
    ] {
        assert!(sheet(&ready, dino).has_keyword(key));
    }
    assert_eq!(physical(&ready, 2).tapped(), Some(false));
    assert_eq!(physical(&ready, 3).tapped(), Some(true));
    assert_eq!(physical(&ready, 4).tapped(), Some(true));
    let abilities = &named("Zacama, Primal Calamity", 8, A, ObjectDestination::Hand)
        .program
        .activated;
    let aimed = |index: usize, n, color| GameCommand::ActivateAbility {
        source: dino,
        ability: abilities[index].id,
        target: marker(&ready, n),
        payment: Some(spend(&[(Colourless, 2), (color, 1)])),
    };
    illegal(&ready, A, aimed(0, 6, Red));
    let burn = round(command(&ready, A, aimed(0, 5, Red)).into_state());
    assert_eq!(
        burn.damage_journal().applied[0].source.id(),
        DamageSourceId::Object(dino)
    );
    assert_eq!(burn.damage_journal().applied[0].amount, 3.into());
    illegal(&ready, A, aimed(1, 5, Green));
    for n in [3, 6] {
        let lost = round(command(&ready, A, aimed(1, n, Green)).into_state()).into_state();
        assert_eq!(
            physical(&lost, n).zone(),
            ObjectZone::Graveyard(if n == 3 { A } else { B })
        );
    }
    let gain = round(
        command(
            &ready,
            A,
            GameCommand::ActivateUntargetedAbility {
                source: dino,
                ability: abilities[2].id,
                payment: Some(spend(&[(Colourless, 2), (White, 1)])),
            },
        )
        .into_state(),
    )
    .into_state();
    assert_eq!(gain.execution().life_total(A), Some(&23.into()));
}
#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_grasp_of_fate_exiles_up_to_one_nonland_per_opponent_and_returns_the_exact_cards_under_their_owners()
 {
    use ManaType::*;
    let price = [(Colourless, 1), (White, 2)];
    let mut lent = creature(2, A, 2, 2, &[]);
    lent.destination = battlefield(B);
    let cards = vec![
        named("Grasp of Fate", 1, A, ObjectDestination::Hand),
        lent,
        creature(3, B, 1, 3, &[]),
        named("Sol Ring", 4, C, battlefield(C)),
        named("Island", 5, C, battlefield(C)),
        seed(
            6,
            A,
            ObjectDestination::Hand,
            free_instant(BodyTarget::SinglePermanent, Instruction::DestroyTarget),
        ),
    ];
    let initial = game(vec![A, B, C], cards, &price, Step::PrecombatMain);
    let entry = cast_card(&initial, 1, &price);
    let choose = |ids: &[u64]| {
        DecisionReply::TriggerPermanentTargets(ids.iter().map(|&n| marker(&entry, n)).collect())
    };
    for ids in [&[2, 3][..], &[1][..], &[5][..]] {
        super::declarations::invalid(&entry, A, choose(ids));
    }
    for ids in [vec![], vec![2], vec![2, 4]] {
        let stack = reply(&entry, choose(&ids)).into_state();
        let gone = round(targeted(&stack, A, 6, marker(&stack, 1)).into_state()).into_state();
        let no = round(gone).into_state();
        for n in [2, 4] {
            assert_eq!(physical(&no, n).zone(), ObjectZone::Battlefield);
        }
        let hidden = round(stack).into_state();
        for n in [2, 4] {
            assert_eq!(
                physical(&hidden, n).zone(),
                if ids.contains(&n) {
                    ObjectZone::Exile
                } else {
                    ObjectZone::Battlefield
                }
            );
        }
        let freed = round(targeted(&hidden, A, 6, marker(&hidden, 1)).into_state()).into_state();
        for &n in &ids {
            let card = physical(&freed, n);
            assert_eq!(card.zone(), ObjectZone::Battlefield);
            assert_eq!(card.controller(), Some(card.owner()));
            assert_ne!(card.id(), physical(&entry, n).id());
        }
    }
}

fn modal_selections(mut state: GameState, preferred: &[u64]) -> GameState {
    while let Some(p) = state.pending_decision() {
        let choice = match p.frame() {
            RuleFrame::ResolutionSelect(f) => {
                let selected = preferred
                    .iter()
                    .map(|&id| physical(&state, id).id())
                    .filter(|id| f.domain().contains(id))
                    .take(f.required())
                    .collect::<Vec<_>>();
                assert_eq!(selected.len(), f.required());
                DecisionReply::SelectResolution(selected)
            }
            RuleFrame::ResolutionGraveyardOrder(f) => {
                DecisionReply::OrderResolutionGraveyard(f.domain().to_vec())
            }
            frame => panic!("unexpected resolving modal choice: {frame:?}"),
        };
        state = reply(&state, choice).into_state();
    }
    state
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_rankle_flies_with_haste_and_executes_each_subset_of_its_combat_damage_modes_in_printed_order()
 {
    use ManaType::*;
    let price = [(Colourless, 2), (Black, 2)];
    let mut cards = vec![
        named("Rankle, Master of Pranks", 1, A, ObjectDestination::Hand),
        creature(2, A, 1, 1, &[]),
        creature(3, B, 1, 3, &[]),
        creature(4, C, 1, 2, &[]),
        creature(5, B, 4, 5, &[KeywordAbility::Flying]),
    ];
    for (n, owner) in [(10, A), (20, B), (30, C)] {
        cards.push(named("Forest", n, owner, ObjectDestination::Hand));
    }
    let before = game(vec![A, B, C], cards, &price, Step::PrecombatMain);
    let field = cast_card(&before, 1, &price);
    for k in [KeywordAbility::Haste, KeywordAbility::Flying] {
        assert!(sheet(&field, physical(&field, 1).id()).has_keyword(k));
    }
    let step = super::declarations::combat_on(field, 1);
    let swing = reply(&step, super::declarations::attacks(&step, &[(1, B, None)])).into_state();
    let block = exile_hand::until(swing, |s| {
        matches!(
            s.pending_decision().map(|p| p.frame()),
            Some(RuleFrame::Combat(CombatDecision::Blockers(_)))
        )
    });
    let intercepted = reply(&block, super::declarations::blocks(&block, &[(5, 1)])).into_state();
    let after = exile_hand::until(intercepted, |s| {
        s.turn().unwrap().position().step == Step::PostcombatMain
    });
    assert_eq!(after.execution().life_total(B), Some(&20.into()));
    let unblocked = reply(&block, DecisionReply::DeclareBlockers(vec![])).into_state();
    let choice = exile_hand::until(unblocked, |s| {
        matches!(
            s.pending_decision().map(|p| p.frame()),
            Some(RuleFrame::TriggerTarget(frame)) if frame.modal().is_some()
        )
    });
    assert_eq!(choice.execution().life_total(B), Some(&17.into()));
    for mask in 0u8..8 {
        let modes = (0..3)
            .filter(|mode| mask & (1 << mode) != 0)
            .map(|mode| spellwake_engine::mana::CastMode {
                mode,
                target: None,
                targets: vec![],
            })
            .collect();
        let picked = reply(&choice, DecisionReply::TriggerModes(modes)).into_state();
        let done = modal_selections(round(picked).into_state(), &[2, 3, 4, 10, 20, 30]);
        for (seat, body, hand, drawn, life) in [
            (A, 2, 10, 100, 20i64),
            (B, 3, 20, 101, 17),
            (C, 4, 30, 102, 20),
        ] {
            assert_eq!(
                physical(&done, hand).zone(),
                if mask & 1 != 0 {
                    ObjectZone::Graveyard(seat)
                } else {
                    ObjectZone::Hand(seat)
                }
            );
            assert_eq!(
                physical(&done, drawn).zone(),
                if mask & 2 != 0 {
                    ObjectZone::Hand(seat)
                } else {
                    ObjectZone::Library(seat)
                }
            );
            assert_eq!(
                done.execution().life_total(seat),
                Some(&(life - i64::from(mask & 2 != 0)).into())
            );
            assert_eq!(
                physical(&done, body).zone(),
                if mask & 4 != 0 {
                    ObjectZone::Graveyard(seat)
                } else {
                    ObjectZone::Battlefield
                }
            );
        }
    }
}
#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_vindictive_lich_chooses_one_or_more_death_modes_with_distinct_opponents_for_sacrifice_discard_and_life_loss()
 {
    use ManaType::*;
    let fourth = SeatId(3);
    let price = [(Colourless, 3), (Black, 1)];
    let cards = vec![
        named("Vindictive Lich", 1, A, ObjectDestination::Hand),
        creature(2, B, 1, 3, &[]),
        named("Island", 10, C, ObjectDestination::Hand),
        named("Forest", 11, C, ObjectDestination::Hand),
        named("Swamp", 12, C, ObjectDestination::Hand),
        super::super::cmm_gameplay::remove_spell(9, A),
    ];
    let before = game(vec![A, B, C, fourth], cards, &price, Step::PrecombatMain);
    let arrived = cast_card(&before, 1, &price);
    let death = round(targeted(&arrived, A, 9, marker(&arrived, 1)).into_state()).into_state();
    assert_eq!(physical(&death, 1).zone(), ObjectZone::Graveyard(A));
    let modes = |items: &[(u8, SeatId)]| {
        DecisionReply::TriggerModes(
            items
                .iter()
                .map(|&(mode, seat)| spellwake_engine::mana::CastMode {
                    mode,
                    target: Some(TargetRef::Player(seat)),
                    targets: vec![],
                })
                .collect(),
        )
    };
    for bad in [vec![], vec![(0, A)], vec![(0, B), (2, B)]] {
        super::declarations::invalid(&death, A, modes(&bad));
    }
    for mask in 1u8..8 {
        let picked = [(0, B), (1, C), (2, fourth)]
            .into_iter()
            .filter(|(m, _)| mask & (1 << m) != 0)
            .collect::<Vec<_>>();
        let stack = reply(&death, modes(&picked)).into_state();
        let result = modal_selections(round(stack).into_state(), &[2, 10, 11]);
        assert_eq!(
            physical(&result, 2).zone(),
            if mask & 1 != 0 {
                ObjectZone::Graveyard(B)
            } else {
                ObjectZone::Battlefield
            }
        );
        for n in [10, 11] {
            assert_eq!(
                physical(&result, n).zone(),
                if mask & 2 != 0 {
                    ObjectZone::Graveyard(C)
                } else {
                    ObjectZone::Hand(C)
                }
            );
        }
        assert_eq!(physical(&result, 12).zone(), ObjectZone::Hand(C));
        assert_eq!(
            result.execution().life_total(fourth),
            Some(&(if mask & 4 != 0 { 15 } else { 20 }).into())
        );
    }
}
