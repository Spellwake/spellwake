//! Type-changing Auras and triggered Equipment.
use super::super::declarations::{attacks, combat_on, invalid};
use super::*;
use spellwake_engine::land::{BasicLandType, LandType};

fn enchant(state: &GameState, aura: u64, host: u64, cost: &[(ManaType, i64)]) -> GameState {
    round(
        command(
            state,
            A,
            GameCommand::CastPaid {
                spell: physical(state, aura).id(),
                target: marker(state, host),
                payment: spend(cost),
            },
        )
        .into_state(),
    )
    .into_state()
}
fn main_step(state: GameState, turn: u64) -> GameState {
    exile_hand::until(state, |s| {
        s.turn().unwrap().position().number == turn
            && s.turn().unwrap().position().step == Step::PrecombatMain
    })
}
#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_darksteel_mutation_and_song_of_the_dryads_replace_the_hosts_characteristics_until_the_aura_leaves()
 {
    use ManaType::*;
    for (name, price, song) in [
        (
            "Darksteel Mutation",
            vec![(Colourless, 1), (White, 1)],
            false,
        ),
        (
            "Song of the Dryads",
            vec![(Colourless, 2), (Green, 1)],
            true,
        ),
    ] {
        let mut sphinx = named("Goliath Sphinx", 2, A, battlefield(A));
        sphinx.program.card_types.insert(CardType::Enchantment);
        let cards = vec![
            named(name, 1, A, ObjectDestination::Hand),
            sphinx,
            seed(
                3,
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
            named("Sol Ring", 4, A, battlefield(A)),
        ];
        let initial = game(vec![A, B], cards, &price, Step::PrecombatMain);
        let affected = enchant(&initial, 1, 2, &price);
        let id = physical(&affected, 2).id();
        let view = sheet(&affected, id);
        assert!(!view.has_keyword(KeywordAbility::Flying));
        if song {
            assert_eq!(view.current_types, [CardType::Land].into());
            assert!(view.current_colors.is_empty());
            assert_eq!(view.current_land_subtypes, [LandType::Forest].into());
            assert!(view.power_toughness.is_none());
            let ground = enchant(&initial, 1, 4, &price);
            let source = physical(&ground, 4).id();
            illegal(
                &ground,
                A,
                GameCommand::ActivateMana {
                    source,
                    ability: ManaAbilityRef::Printed(ManaAbilityId(0)),
                },
            );
            let tapped = command(
                &ground,
                A,
                GameCommand::ActivateMana {
                    source,
                    ability: ManaAbilityRef::BasicLand(BasicLandType::Forest),
                },
            )
            .into_state();
            assert_eq!(tapped.execution().mana(A, Green), 1.into());
        } else {
            illegal(
                &initial,
                A,
                GameCommand::CastPaid {
                    spell: physical(&initial, 1).id(),
                    target: marker(&initial, 4),
                    payment: spend(&price),
                },
            );
            assert_eq!(
                view.current_types,
                [CardType::Artifact, CardType::Creature].into()
            );
            assert_eq!(view.current_colors, [Color::Blue].into());
            assert_eq!(
                view.current_creature_subtypes,
                [current_catalogue().1.resolve_label("Insect").unwrap()].into()
            );
            assert_eq!(view.keywords, [KeywordAbility::Indestructible].into());
            stats(&affected, id, 0, 1);
            let safe =
                round(targeted(&affected, A, 3, marker(&affected, 2)).into_state()).into_state();
            assert_eq!(physical(&safe, 2).zone(), ObjectZone::Battlefield);
        }
        let restored =
            round(targeted(&affected, A, 3, marker(&affected, 1)).into_state()).into_state();
        let sheet = sheet(&restored, id);
        assert_eq!(
            sheet.current_types,
            [CardType::Enchantment, CardType::Creature].into()
        );
        assert_eq!(sheet.current_colors, [Color::Blue].into());
        assert_eq!(
            sheet.current_creature_subtypes,
            [current_catalogue().1.resolve_label("Sphinx").unwrap()].into()
        );
        assert!(sheet.has_keyword(KeywordAbility::Flying));
        stats(&restored, id, 8, 7);
    }
}
#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_fall_from_favor_taps_the_host_grants_monarch_and_allows_untap_only_when_its_controller_is_monarch()
 {
    use ManaType::*;
    let price = [(Colourless, 2), (Blue, 1)];
    let mut cards = vec![
        named("Fall from Favor", 1, A, ObjectDestination::Hand),
        creature(2, B, 2, 3, &[]),
        seed(
            3,
            B,
            ObjectDestination::Hand,
            free_instant(
                BodyTarget::None,
                crate::keyword_creatures::oracle_lowering::toolbox_emit::become_monarch(
                    spellwake_engine::toolbox::PlayerSelector::Resolving(
                        spellwake_engine::program::ResolvingPlayer::Controller,
                    ),
                ),
            ),
        ),
    ];
    for n in 10..18 {
        cards.push(named(
            "Island",
            n,
            if n < 14 { A } else { B },
            ObjectDestination::LibraryBottom,
        ));
    }
    let initial = game(vec![A, B], cards, &price, Step::PrecombatMain);
    let entered = enchant(&initial, 1, 2, &price);
    assert_eq!(physical(&entered, 2).tapped(), Some(false));
    let claimed = round(entered).into_state();
    assert_eq!(physical(&claimed, 2).tapped(), Some(true));
    assert_eq!(claimed.execution().monarch(), Some(A));
    let next = main_step(claimed, 2);
    assert_eq!(physical(&next, 2).tapped(), Some(true));
    let change = round(
        command(
            &next,
            B,
            GameCommand::CastUntargeted {
                spell: physical(&next, 3).id(),
                payment: None,
            },
        )
        .into_state(),
    )
    .into_state();
    assert_eq!(change.execution().monarch(), Some(B));
    let unlock = main_step(change, 4);
    assert_eq!(physical(&unlock, 2).tapped(), Some(false));
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_explorers_scope_and_sword_of_the_animist_trigger_for_the_equipped_attacker_and_distinguish_looking_from_searching()
 {
    use ManaType::*;
    use spellwake_engine::effects::LibraryTopReply;
    for (name, amount, searching) in [
        ("Explorer's Scope", 1, false),
        ("Sword of the Animist", 2, true),
    ] {
        for on_top in ["Sol Ring", "Plains"] {
            let initial = game(
                vec![A, B],
                vec![
                    named(name, 1, A, ObjectDestination::Hand),
                    creature(2, A, 2, 3, &[KeywordAbility::Haste]),
                    creature(3, A, 1, 2, &[KeywordAbility::Haste]),
                    named(on_top, 10, A, ObjectDestination::LibraryBottom),
                    named("Island", 11, A, ObjectDestination::LibraryBottom),
                    named("Rogue's Passage", 12, A, ObjectDestination::LibraryBottom),
                ],
                &[(Colourless, 2 * amount)],
                Step::PrecombatMain,
            );
            let entered = cast_card(&initial, 1, &[(Colourless, amount)]);
            let equipped = wield(&entered, physical(&entered, 1).id(), 2, amount);
            stats(
                &equipped,
                physical(&equipped, 2).id(),
                if searching { 3 } else { 2 },
                if searching { 4 } else { 3 },
            );
            let combat = combat_on(equipped, 1);
            let other = reply(&combat, attacks(&combat, &[(3, B, None)])).into_state();
            assert!(other.execution().window().entries().is_empty());
            let attacked = reply(&combat, attacks(&combat, &[(2, B, None)])).into_state();
            let offer = round(attacked).into_state();
            let select = if searching {
                let skipped = optional(&offer, false);
                assert_eq!(physical(&skipped, 11).zone(), ObjectZone::Library(A));
                let look = optional(&offer, true);
                look.supply_entropy(look.basis(), vec![0; 16])
                    .unwrap()
                    .into_state()
            } else {
                offer
            };
            let choice = |card: Option<u64>| {
                DecisionReply::LibraryTop(LibraryTopReply::Search {
                    selected: card.map(|n| physical(&select, n).id()),
                })
            };
            let RuleFrame::LibraryTop(frame) = select.pending_decision().unwrap().frame() else {
                panic!("library action");
            };
            if searching {
                invalid(&select, A, choice(Some(12)));
                let failed = reply(&select, choice(None));
                assert_eq!(failed.shuffle_journal().len(), 1);
                let found = reply(&select, choice(Some(11)));
                assert_eq!(found.shuffle_journal().len(), 1);
                assert_eq!(physical(found.state(), 11).zone(), ObjectZone::Battlefield);
                assert_eq!(physical(found.state(), 11).tapped(), Some(true));
            } else {
                assert_eq!(frame.domain_top_first(), &[physical(&select, 10).id()]);
                let no = reply(&select, choice(None));
                assert!(no.shuffle_journal().is_empty());
                assert_eq!(
                    no.state().execution().window().cards().library(A)[0],
                    physical(no.state(), 10).id()
                );
                if on_top == "Plains" {
                    let land = reply(&select, choice(Some(10))).into_state();
                    assert_eq!(physical(&land, 10).zone(), ObjectZone::Battlefield);
                    assert_eq!(physical(&land, 10).tapped(), Some(true));
                } else {
                    invalid(&select, A, choice(Some(10)));
                }
            }
        }
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_fiendlash_gives_power_and_reach_and_the_equipped_creature_damages_the_chosen_player_or_planeswalker()
 {
    use ManaType::*;
    let walker = named("Jace Beleren", 3, A, ObjectDestination::Hand);
    let cards = vec![
        named("Fiendlash", 1, A, ObjectDestination::Hand),
        creature(2, A, 2, 4, &[]),
        walker,
        creature(4, B, 1, 4, &[]),
        seed(
            5,
            A,
            ObjectDestination::Hand,
            free_instant(
                BodyTarget::SingleCreature,
                crate::keyword_creatures::oracle_lowering::toolbox_emit::deal_damage(
                    spellwake_engine::toolbox::DamageTargetQuery::Targets,
                    spellwake_engine::program::ValueExpr::literal(1),
                    false,
                ),
            ),
        ),
    ];
    let initial = game(
        vec![A, B],
        cards,
        &[(Colourless, 4), (Red, 2), (Blue, 2)],
        Step::PrecombatMain,
    );
    let field = cast_card(&initial, 3, &[(Colourless, 1), (Blue, 2)]);
    assert_eq!(loyalty(&field, 3), 3);
    let entered = cast_card(&field, 1, &[(Colourless, 1), (Red, 1)]);
    let artifact = physical(&entered, 1).id();
    let attach = GameCommand::ActivateAbility {
        source: artifact,
        ability: ActivatedAbilityId(1),
        target: marker(&entered, 2),
        payment: Some(spend(&[(Colourless, 2), (Red, 1)])),
    };
    let equipped = round(command(&entered, A, attach).into_state()).into_state();
    let host = physical(&equipped, 2).id();
    stats(&equipped, host, 4, 4);
    assert!(sheet(&equipped, host).has_keyword(KeywordAbility::Reach));
    let unrelated =
        round(targeted(&equipped, A, 5, marker(&equipped, 4)).into_state()).into_state();
    assert!(
        unrelated.pending_decision().is_none()
            && unrelated.execution().window().entries().is_empty()
    );
    let struck = round(targeted(&equipped, A, 5, marker(&equipped, 2)).into_state()).into_state();
    invalid(&struck, A, DecisionReply::TriggerTarget(marker(&struck, 4)));
    for target in [TargetRef::Player(B), marker(&struck, 3)] {
        let stack = reply(&struck, DecisionReply::TriggerTarget(target)).into_state();
        let result = round(stack);
        assert!(
            result
                .damage_journal()
                .applied
                .iter()
                .any(
                    |hit| hit.source.id() == DamageSourceId::Object(host) && hit.amount == 4.into()
                )
        );
        if target == TargetRef::Player(B) {
            assert_eq!(result.state().execution().life_total(B), Some(&16.into()));
        } else {
            assert_eq!(physical(result.state(), 3).zone(), ObjectZone::Graveyard(A));
        }
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_meteoric_mace_cascades_past_a_land_and_equal_cost_spell_then_grants_its_equipped_bonus_and_trample()
 {
    use ManaType::*;
    let price = [(Colourless, 4), (Red, 2)];
    let cards = vec![
        named("Meteoric Mace", 1, A, ObjectDestination::Hand),
        creature(2, A, 2, 4, &[]),
        creature(3, B, 1, 1, &[]),
        named("Island", 10, A, ObjectDestination::LibraryBottom),
        seed(
            11,
            A,
            ObjectDestination::LibraryBottom,
            Program::artifact(Some(ManaCost::generic(6.into())), vec![]),
        ),
        named("Sol Ring", 12, A, ObjectDestination::LibraryBottom),
    ];
    let initial = game(
        vec![A, B],
        cards,
        &[(Colourless, 8), (Red, 2)],
        Step::PrecombatMain,
    );
    let cast = cast_untargeted(&initial, 1, &price).into_state();
    let cast = cast
        .supply_entropy(cast.basis(), vec![47; 16])
        .unwrap()
        .into_state();
    let offer = round(cast).into_state();
    assert_eq!(physical(&offer, 1).zone(), ObjectZone::Stack);
    for n in [10, 11, 12] {
        assert_eq!(physical(&offer, n).zone(), ObjectZone::Exile);
    }
    let declined = optional(&offer, false);
    for n in [10, 11, 12] {
        assert_eq!(physical(&declined, n).zone(), ObjectZone::Library(A));
    }
    let free = reply(
        &offer,
        DecisionReply::CascadeCast {
            target: None,
            payment: Default::default(),
        },
    )
    .into_state();
    let resolved = round(free).into_state();
    assert_eq!(physical(&resolved, 12).zone(), ObjectZone::Battlefield);
    assert_eq!(physical(&resolved, 1).zone(), ObjectZone::Stack);
    let entered = round(resolved).into_state();
    assert_eq!(entered.execution().mana(A, Colourless), 4.into());
    let mace = physical(&entered, 1).id();
    illegal(&entered, A, equip(&entered, mace, 3, 4));
    let equipped = wield(&entered, mace, 2, 4);
    stats(&equipped, physical(&equipped, 2).id(), 6, 4);
    assert!(sheet(&equipped, physical(&equipped, 2).id()).has_keyword(KeywordAbility::Trample));
}
#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_scytheclaw_keeps_its_zero_base_germ_alive_and_only_player_combat_damage_causes_rounded_up_life_loss()
 {
    use ManaType::*;
    let initial = game(
        vec![A, B],
        vec![
            named("Scytheclaw", 1, A, ObjectDestination::Hand),
            creature(2, A, 2, 2, &[KeywordAbility::Haste]),
            creature(3, B, 0, 7, &[]),
        ],
        &[(Colourless, 8)],
        Step::PrecombatMain,
    );
    let entered = cast_card(&initial, 1, &[(Colourless, 5)]);
    let made = round(entered);
    let germ = tokens(&made, A, 1)[0];
    let armed = made.into_state();
    let equipment = physical(&armed, 1).id();
    assert_eq!(
        armed.execution().attached_to(equipment),
        Some(TargetRef::Object(germ))
    );
    let s = sheet(&armed, germ);
    assert_eq!(s.current_colors, [Color::Black].into());
    assert_eq!(
        s.current_creature_subtypes,
        ["Phyrexian", "Germ"]
            .map(|k| current_catalogue().1.resolve_label(k).unwrap())
            .into()
    );
    stats(&armed, germ, 1, 1);
    let equipped = wield(&armed, equipment, 2, 3);
    assert!(equipped.execution().window().cards().get(germ).is_none());
    stats(&equipped, physical(&equipped, 2).id(), 3, 3);
    let step = combat_on(equipped, 1);
    let attacking = reply(&step, attacks(&step, &[(2, B, None)])).into_state();
    let blocking = exile_hand::until(attacking, |s| {
        matches!(
            s.pending_decision().map(|p| p.frame()),
            Some(RuleFrame::Combat(CombatDecision::Blockers(_)))
        )
    });
    let intercepted = reply(
        &blocking,
        super::super::declarations::blocks(&blocking, &[(3, 2)]),
    )
    .into_state();
    let done = exile_hand::until(intercepted, |s| {
        s.turn().unwrap().position().step == Step::PostcombatMain
    });
    assert_eq!(done.execution().life_total(B), Some(&20.into()));
    let hit = reply(&blocking, DecisionReply::DeclareBlockers(vec![])).into_state();
    let wound = exile_hand::until(hit, |s| !s.execution().window().entries().is_empty());
    assert_eq!(wound.execution().life_total(B), Some(&17.into()));
    let loss = round(wound).into_state();
    assert_eq!(loss.execution().life_total(B), Some(&8.into()));
}
