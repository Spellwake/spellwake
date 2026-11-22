//! Loyalty, linked exile and attack costs through the scheduled game root.
use super::cmm_gameplay::{creature, illegal, named};
use super::*;
use spellwake_engine::mana::{HybridSide, ManaAbilityId, ManaAbilityRef};

fn battlefield(seat: SeatId) -> ObjectDestination {
    ObjectDestination::Battlefield { controller: seat }
}
fn loyalty(game: &GameState, card: u64) -> i64 {
    String::from(
        game.execution()
            .loyalty_counters()
            .get(&physical(game, card).id())
            .cloned()
            .unwrap(),
    )
    .parse()
    .unwrap()
}
fn activate(game: &GameState, card: u64, ability: u64, cost: &[(ManaType, i64)]) -> GameCommand {
    GameCommand::ActivateUntargetedAbility {
        source: physical(game, card).id(),
        ability: ActivatedAbilityId(ability),
        payment: (!cost.is_empty()).then(|| spend(cost)),
    }
}

fn verify_commander_deck(name: &str, basic_land: &str) {
    let inventory = current_catalogue().0;
    let id = |name: &str| {
        OracleId::try_from(
            inventory
                .cards
                .iter()
                .find(|card| card.name == name && card.required_by_base_formats)
                .unwrap()
                .oracle_id
                .clone()
                .unwrap(),
        )
        .unwrap()
    };
    let commander = id(name);
    let deck = crate::commander::prepare_deck(
        inventory,
        &spellwake_contracts::names::NameRules::literal_names(),
        std::slice::from_ref(&commander),
        &vec![id(basic_land); 99],
        &Default::default(),
    )
    .unwrap();
    assert_eq!(deck.commanders(), &[commander]);
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_ugin_chain_veil_and_oath_of_gideon_preserve_linked_cards_and_loyalty_allowances() {
    use ManaType::*;
    let cards = vec![
        named("Ugin, the Ineffable", 1, A, ObjectDestination::Hand),
        named("Oath of Gideon", 2, A, ObjectDestination::Hand),
        named("The Chain Veil", 3, A, ObjectDestination::Hand),
        named("Goliath Sphinx", 4, B, battlefield(B)),
        seed(
            5,
            A,
            ObjectDestination::Hand,
            free_instant(BodyTarget::SingleCreature, Instruction::DestroyTarget),
        ),
        seed(
            6,
            A,
            ObjectDestination::LibraryBottom,
            Program::artifact(None, vec![]),
        ),
    ];
    let start = game(
        vec![A, B],
        cards,
        &[(Colourless, 14), (White, 1)],
        Step::PrecombatMain,
    );
    let oath =
        round(cast_untargeted(&start, 2, &[(Colourless, 2), (White, 1)]).into_state()).into_state();
    let ordering = round(oath).into_state();
    let RuleFrame::TimestampOrder(frame) = ordering.pending_decision().unwrap().frame() else {
        panic!("Kor ordering")
    };
    let made = reply(
        &ordering,
        DecisionReply::TimestampOrder(frame.members().iter().map(|member| member.member).collect()),
    );
    for token in tokens(&made, A, 2) {
        let card = made
            .state()
            .execution()
            .current_characteristics(token)
            .unwrap()
            .unwrap();
        let vocab = current_catalogue().1;
        assert_eq!(
            card.current_creature_subtypes,
            [
                vocab.resolve_label("Kor").unwrap(),
                vocab.resolve_label("Ally").unwrap()
            ]
            .into()
        );
        assert_eq!(card.current_colors, [Color::White].into());
        assert_eq!(
            card.power_toughness.unwrap().derived,
            PowerToughness {
                power: 1.into(),
                toughness: 1.into()
            }
        );
    }
    let ugin =
        round(cast_untargeted(made.state(), 1, &[(Colourless, 6)]).into_state()).into_state();
    assert_eq!(loyalty(&ugin, 1), 5);
    let veil = round(cast_untargeted(&ugin, 3, &[(Colourless, 2)]).into_state()).into_state();
    assert_eq!(veil.execution().mana(A, Colourless), 4.into());
    let penalty = exile_hand::until(veil.clone(), |s| s.turn().unwrap().position().number == 2);
    assert_eq!(penalty.execution().life_total(A), Some(&18.into()));
    illegal(
        &veil,
        A,
        GameCommand::ActivateAbility {
            source: physical(&veil, 1).id(),
            ability: ActivatedAbilityId(2),
            target: TargetRef::Object(physical(&veil, 3).id()),
            payment: None,
        },
    );
    let plus = command(&veil, A, activate(&veil, 1, 1, &[])).into_state();
    assert_eq!(loyalty(&plus, 1), 6);
    let linked = round(plus).into_state();
    assert_eq!(physical(&linked, 6).zone(), ObjectZone::Exile);
    assert_eq!(
        linked
            .execution()
            .face_down_exile_viewer(physical(&linked, 6).id()),
        Some(A)
    );
    let spirit = linked
        .execution()
        .window()
        .cards()
        .iter()
        .find(|card| {
            card.is_token()
                && linked
                    .execution()
                    .current_characteristics(card.id())
                    .unwrap()
                    .is_some_and(|sheet| {
                        sheet
                            .current_creature_subtypes
                            .contains(&current_catalogue().1.resolve_label("Spirit").unwrap())
                    })
        })
        .unwrap()
        .id();
    let desc = linked
        .execution()
        .current_characteristics(spirit)
        .unwrap()
        .unwrap();
    assert!(desc.current_colors.is_empty());
    assert_eq!(
        desc.power_toughness.unwrap().derived,
        PowerToughness {
            power: 2.into(),
            toughness: 2.into()
        }
    );
    let destroyed =
        round(targeted(&linked, A, 5, TargetRef::Object(spirit)).into_state()).into_state();
    assert_eq!(physical(&destroyed, 6).zone(), ObjectZone::Exile);
    let returned = round(destroyed).into_state();
    assert_eq!(physical(&returned, 6).zone(), ObjectZone::Hand(A));
    let minus = |s: &GameState| GameCommand::ActivateAbility {
        source: physical(s, 1).id(),
        ability: ActivatedAbilityId(2),
        target: TargetRef::Object(physical(s, 4).id()),
        payment: None,
    };
    illegal(&returned, A, minus(&returned));
    let extra =
        round(command(&returned, A, activate(&returned, 3, 1, &[(Colourless, 4)])).into_state())
            .into_state();
    assert_eq!(physical(&extra, 3).tapped(), Some(true));
    assert_eq!(extra.execution().mana(A, Colourless), 0.into());
    let reduced = command(&extra, A, minus(&extra)).into_state();
    assert_eq!(loyalty(&reduced, 1), 3);
    let destroyed = round(reduced).into_state();
    assert_eq!(physical(&destroyed, 4).zone(), ObjectZone::Graveyard(B));
    illegal(&destroyed, A, activate(&destroyed, 1, 1, &[]));
    let end = exile_hand::until(destroyed, |s| s.turn().unwrap().position().number == 2);
    assert_eq!(end.execution().life_total(A), Some(&20.into()));
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_baird_onakke_and_annex_charge_exactly_for_the_player_or_planeswalker_attacked() {
    use ManaType::*;
    let cards = vec![
        named("Norn's Annex", 1, A, ObjectDestination::Hand),
        named("Baird, Steward of Argive", 2, A, ObjectDestination::Hand),
        named("Onakke Oathkeeper", 3, A, ObjectDestination::Hand),
        named(
            "Ob Nixilis of the Black Oath",
            4,
            A,
            ObjectDestination::Hand,
        ),
        creature(5, B, 2, 4, &[KeywordAbility::Haste]),
        named("Sol Ring", 6, B, battlefield(B)),
        named("Plains", 7, B, battlefield(B)),
        creature(8, B, 2, 3, &[KeywordAbility::Haste]),
        named("Sol Ring", 9, B, battlefield(B)),
    ];
    let start = game(
        vec![A, B, C],
        cards,
        &[(Colourless, 9), (White, 4), (Black, 2)],
        Step::PrecombatMain,
    );
    let stack = command(
        &start,
        A,
        GameCommand::CastWithResources {
            spell: physical(&start, 1).id(),
            target: None,
            payment: CastingPayment {
                mana: Some(spend(&[(Colourless, 3), (White, 1)])),
                hybrid_choices: vec![HybridSide::First, HybridSide::Second],
                ..Default::default()
            },
        },
    )
    .into_state();
    assert_eq!(stack.execution().life_total(A), Some(&18.into()));
    let mut state = round(stack).into_state();
    for (id, cost) in [
        (2, vec![(Colourless, 2), (White, 2)]),
        (3, vec![(Colourless, 1), (White, 1)]),
        (4, vec![(Colourless, 3), (Black, 2)]),
    ] {
        state = round(cast_untargeted(&state, id, &cost).into_state()).into_state();
    }
    assert!(
        state
            .execution()
            .current_characteristics(physical(&state, 2).id())
            .unwrap()
            .unwrap()
            .has_keyword(KeywordAbility::Vigilance)
    );
    assert_eq!(loyalty(&state, 4), 3);
    let declare = exile_hand::until(state, |s| {
        s.turn().unwrap().position().number == 2
            && matches!(
                s.pending_decision().map(|d| d.frame()),
                Some(RuleFrame::Combat(CombatDecision::Attackers(_)))
            )
    });
    let baird_combat = exile_hand::until(
        reply(&declare, DecisionReply::DeclareAttackers(vec![])).into_state(),
        |s| {
            s.turn().unwrap().position().number == 4
                && matches!(
                    s.pending_decision().map(|d| d.frame()),
                    Some(RuleFrame::Combat(CombatDecision::Attackers(_)))
                )
        },
    );
    let baird_attacked = reply(
        &baird_combat,
        DecisionReply::DeclareAttackers(vec![AttackDeclaration {
            attacker: physical(&baird_combat, 2).id(),
            defender: C,
            defending_planeswalker: None,
        }]),
    )
    .into_state();
    assert_eq!(physical(&baird_attacked, 2).tapped(), Some(false));
    let attacker = physical(&declare, 5).id();
    let walker = physical(&declare, 4).id();
    let split = reply(
        &declare,
        DecisionReply::DeclareAttackers(vec![
            AttackDeclaration {
                attacker,
                defender: A,
                defending_planeswalker: None,
            },
            AttackDeclaration {
                attacker: physical(&declare, 8).id(),
                defender: A,
                defending_planeswalker: Some(walker),
            },
        ]),
    )
    .into_state();
    let RuleFrame::Combat(CombatDecision::Attackers(frame)) =
        split.pending_decision().unwrap().frame()
    else {
        panic!("counted attack cost");
    };
    let counted = frame.payment().unwrap();
    assert_eq!(counted.generic, 3.into());
    assert_eq!(counted.white_phyrexian, 2.into());
    let mut supply = split;
    for (card, ability) in [
        (6, ManaAbilityRef::Printed(ManaAbilityId(0))),
        (
            7,
            ManaAbilityRef::BasicLand(spellwake_engine::land::BasicLandType::Plains),
        ),
        (9, ManaAbilityRef::Printed(ManaAbilityId(0))),
    ] {
        supply = command(
            &supply,
            B,
            GameCommand::ActivateMana {
                source: physical(&supply, card).id(),
                ability,
            },
        )
        .into_state();
    }
    let mixed = reply(
        &supply,
        DecisionReply::PayAttackCost {
            life_symbols: 1.into(),
            mana: spend(&[(Colourless, 3), (White, 1)]),
        },
    )
    .into_state();
    assert_eq!(mixed.execution().life_total(B), Some(&18.into()));
    assert_eq!(mixed.execution().mana(B, White), 0.into());
    assert_eq!(mixed.execution().mana(B, Colourless), 1.into());
    for id in [5, 8] {
        assert_eq!(physical(&mixed, id).tapped(), Some(true));
    }
    let attack = |defender, defending_planeswalker| {
        DecisionReply::DeclareAttackers(vec![AttackDeclaration {
            attacker,
            defender,
            defending_planeswalker,
        }])
    };
    let free = reply(&declare, attack(C, None)).into_state();
    assert!(free.pending_decision().is_none());
    assert_eq!(physical(&free, 5).tapped(), Some(true));
    assert_eq!(free.execution().life_total(B), Some(&20.into()));
    for (target, generic) in [(None, 1_i64), (Some(walker), 2)] {
        let payment = reply(&declare, attack(A, target)).into_state();
        let decision = payment.pending_decision().unwrap();
        let RuleFrame::Combat(CombatDecision::Attackers(frame)) = decision.frame() else {
            panic!("attack payment")
        };
        let cost = frame.payment().unwrap();
        assert_eq!(cost.generic, generic.into());
        assert_eq!(cost.white_phyrexian, 1.into());
        assert_eq!(decision.chooser(), B);
        let pay = |mana| DecisionReply::PayAttackCost {
            life_symbols: 1.into(),
            mana: spend(&[(Colourless, mana)]),
        };
        illegal(
            &payment,
            B,
            GameCommand::Reply {
                decision: decision.id(),
                choice: pay(generic),
            },
        );
        let undo = reply(&payment, DecisionReply::RedeclareAttackers).into_state();
        assert_eq!(physical(&undo, 5).tapped(), Some(false));
        let mana = command(
            &payment,
            B,
            GameCommand::ActivateMana {
                source: physical(&payment, 6).id(),
                ability: ManaAbilityRef::Printed(ManaAbilityId(0)),
            },
        )
        .into_state();
        assert_eq!(mana.execution().mana(B, Colourless), 2.into());
        let mana_white = command(
            &mana,
            B,
            GameCommand::ActivateMana {
                source: physical(&mana, 7).id(),
                ability: ManaAbilityRef::BasicLand(spellwake_engine::land::BasicLandType::Plains),
            },
        )
        .into_state();
        let white_paid = reply(
            &mana_white,
            DecisionReply::PayAttackCost {
                life_symbols: 0.into(),
                mana: spend(&[(Colourless, generic), (White, 1)]),
            },
        )
        .into_state();
        assert_eq!(white_paid.execution().mana(B, White), 0.into());
        assert_eq!(white_paid.execution().life_total(B), Some(&20.into()));
        assert_eq!(physical(&white_paid, 5).tapped(), Some(true));
        let done = reply(&mana, pay(generic)).into_state();
        assert_eq!(physical(&done, 5).tapped(), Some(true));
        assert_eq!(physical(&done, 6).tapped(), Some(true));
        assert_eq!(done.execution().mana(B, Colourless), (2 - generic).into());
        assert_eq!(done.execution().life_total(B), Some(&18.into()));
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_oath_of_teferi_returns_to_owner_and_ob_nixilis_resolves_every_loyalty_ability() {
    use ManaType::*;
    verify_commander_deck("Ob Nixilis of the Black Oath", "Swamp");
    let mut cards = vec![
        named("Oath of Teferi", 1, A, ObjectDestination::Hand),
        named(
            "Ob Nixilis of the Black Oath",
            2,
            A,
            ObjectDestination::Hand,
        ),
        seed(3, B, battlefield(A), Program::artifact(None, vec![])),
        named("Sol Ring", 4, A, battlefield(A)),
        creature(5, A, 2, 3, &[]),
        named("Swamp", 6, A, battlefield(A)),
        creature(7, B, 2, 3, &[]),
    ];
    for id in 10..15 {
        cards.push(seed(
            id,
            A,
            ObjectDestination::LibraryBottom,
            Program::artifact(None, vec![]),
        ));
    }
    let start = game(
        vec![A, B, C],
        cards,
        &[(Colourless, 6), (White, 1), (Blue, 1), (Black, 2)],
        Step::PrecombatMain,
    );
    let oath =
        round(cast_untargeted(&start, 1, &[(Colourless, 3), (White, 1), (Blue, 1)]).into_state())
            .into_state();
    let target = |id| DecisionReply::TriggerTarget(TargetRef::Object(physical(&oath, id).id()));
    for id in [1, 7] {
        illegal(
            &oath,
            A,
            GameCommand::Reply {
                decision: oath.pending_decision().unwrap().id(),
                choice: target(id),
            },
        );
    }
    let exiled = round(reply(&oath, target(3)).into_state()).into_state();
    assert_eq!(physical(&exiled, 3).zone(), ObjectZone::Exile);
    let ob = round(cast_untargeted(&exiled, 2, &[(Colourless, 3), (Black, 2)]).into_state())
        .into_state();
    assert_eq!(loyalty(&ob, 2), 3);
    let demon = round(command(&ob, A, activate(&ob, 2, 1, &[])).into_state());
    let token = tokens(&demon, A, 1)[0];
    let desc = demon
        .state()
        .execution()
        .current_characteristics(token)
        .unwrap()
        .unwrap();
    assert_eq!(desc.current_colors, [Color::Black].into());
    assert_eq!(
        desc.current_creature_subtypes,
        [current_catalogue().1.resolve_label("Demon").unwrap()].into()
    );
    assert!(desc.has_keyword(KeywordAbility::Flying));
    assert_eq!(
        desc.power_toughness.unwrap().derived,
        PowerToughness {
            power: 5.into(),
            toughness: 5.into()
        }
    );
    assert_eq!(demon.state().execution().life_total(A), Some(&18.into()));
    assert_eq!(loyalty(demon.state(), 2), 1);
    let plus =
        |s: &GameState| round(command(s, A, activate(s, 2, 0, &[])).into_state()).into_state();
    let twice = plus(&plus(&ob));
    for seat in [B, C] {
        assert_eq!(twice.execution().life_total(seat), Some(&18.into()));
    }
    assert_eq!(twice.execution().life_total(A), Some(&24.into()));
    assert_eq!(loyalty(&twice, 2), 7);
    illegal(&twice, A, activate(&twice, 2, 0, &[]));
    let returned = exile_hand::until(twice, |s| physical(s, 3).zone() == ObjectZone::Battlefield);
    assert_eq!(returned.turn().unwrap().position().step, Step::End);
    assert_eq!(physical(&returned, 3).controller(), Some(B));
    assert_ne!(physical(&returned, 3).id(), physical(&start, 3).id());
    let main = exile_hand::until(returned, |s| {
        s.turn().unwrap().position().number == 4
            && s.turn().unwrap().position().step == Step::PrecombatMain
    });
    let nine = plus(&main);
    let emblem = round(command(&nine, A, activate(&nine, 2, 2, &[])).into_state()).into_state();
    assert_eq!(loyalty(&emblem, 2), 1);
    let owned: Vec<_> = emblem.execution().emblems().collect();
    assert_eq!(owned.len(), 1);
    assert_eq!(owned[0].controller, A);
    let emblem_id = owned[0].id;
    let mut mana = emblem;
    for (source, ability) in [
        (4, ManaAbilityRef::Printed(ManaAbilityId(0))),
        (
            6,
            ManaAbilityRef::BasicLand(spellwake_engine::land::BasicLandType::Swamp),
        ),
    ] {
        mana = command(
            &mana,
            A,
            GameCommand::ActivateMana {
                source: physical(&mana, source).id(),
                ability,
            },
        )
        .into_state();
    }
    let hand_count = |s: &GameState| {
        s.execution()
            .window()
            .cards()
            .iter()
            .filter(|c| c.zone() == ObjectZone::Hand(A))
            .count()
    };
    let hand_before = hand_count(&mana);
    let spend_emblem = |id| GameCommand::ActivateEmblem {
        emblem: emblem_id,
        creature: physical(&mana, id).id(),
        payment: Some(spend(&[(Colourless, 1), (Black, 1)])),
    };
    illegal(&mana, A, spend_emblem(7));
    let paid = command(&mana, A, spend_emblem(5)).into_state();
    assert_eq!(physical(&paid, 5).zone(), ObjectZone::Graveyard(A));
    assert_eq!(paid.execution().life_total(A), Some(&26.into()));
    assert_eq!(paid.execution().mana(A, Black), 0.into());
    assert_eq!(paid.execution().mana(A, Colourless), 1.into());
    let done = round(paid).into_state();
    assert_eq!(done.execution().life_total(A), Some(&28.into()));
    assert_eq!(hand_count(&done), hand_before + 2);
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_onakke_oathkeeper_exiles_as_a_cost_and_returns_only_an_owned_graveyard_planeswalker() {
    use ManaType::*;
    let cost = [(Colourless, 4), (White, 2)];
    let start = game(
        vec![A, B],
        vec![
            named("Onakke Oathkeeper", 1, A, ObjectDestination::Graveyard),
            named(
                "Ob Nixilis of the Black Oath",
                2,
                A,
                ObjectDestination::Graveyard,
            ),
            named("Ugin, the Ineffable", 3, B, ObjectDestination::Graveyard),
            named("Elvish Mystic", 4, A, ObjectDestination::Graveyard),
        ],
        &cost,
        Step::PrecombatMain,
    );
    let ability = |id| GameCommand::ActivateAbility {
        source: physical(&start, 1).id(),
        ability: ActivatedAbilityId(0),
        target: TargetRef::Object(physical(&start, id).id()),
        payment: Some(spend(&cost)),
    };
    illegal(&start, A, ability(3));
    illegal(&start, A, ability(4));
    let paid = command(&start, A, ability(2)).into_state();
    assert_eq!(paid.execution().mana(A, White), 0.into());
    assert_eq!(paid.execution().mana(A, Colourless), 0.into());
    assert_eq!(physical(&paid, 1).zone(), ObjectZone::Exile);
    assert_eq!(physical(&paid, 2).zone(), ObjectZone::Graveyard(A));
    let returned = round(paid).into_state();
    assert_eq!(physical(&returned, 2).zone(), ObjectZone::Battlefield);
    assert_eq!(physical(&returned, 2).controller(), Some(A));
    assert_eq!(loyalty(&returned, 2), 3);
}

fn loyalty_at(state: &GameState, ability: u64, target: TargetRef) -> GameCommand {
    GameCommand::ActivateAbility {
        source: physical(state, 1).id(),
        ability: ActivatedAbilityId(ability),
        target,
        payment: None,
    }
}

mod attack_reversal;
mod battlefield_spells;
mod battles;
mod combat_cards;
mod copies;
mod copy_permanents;
mod declarations;
mod equipment;
mod linked_permanents;
mod loyalty;
