//! Entry copies, continuous effects and resolving triggers through scheduled games.
use super::cmm_gameplay::{creature, illegal, named};
use super::*;
use spellwake_engine::effects::{
    OptionalEffectChoice, ZoneReplacementReply, ZoneReplacementRequest,
};

fn on_field(owner: SeatId) -> ObjectDestination {
    ObjectDestination::Battlefield { controller: owner }
}

fn sheet(
    state: &GameState,
    number: u64,
) -> spellwake_engine::characteristics::ObjectCharacteristics {
    state
        .execution()
        .current_characteristics(physical(state, number).id())
        .unwrap()
        .unwrap()
}

fn size(state: &GameState, number: u64) -> (i64, i64) {
    let pt = sheet(state, number).power_toughness.unwrap().derived;
    (
        String::from(pt.power).parse().unwrap(),
        String::from(pt.toughness).parse().unwrap(),
    )
}

fn copied(state: &GameState, donor: Option<u64>) -> GameState {
    reply(
        state,
        DecisionReply::ZoneReplacement(ZoneReplacementReply::EntryCopy(
            donor.map(|n| physical(state, n).id()),
        )),
    )
    .into_state()
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_body_double_uses_either_graveyard_and_preserves_its_own_identity() {
    use ManaType::*;
    let cost = [(Colourless, 4), (Blue, 1)];
    let game = game(
        vec![A, B],
        vec![
            named("Body Double", 1, A, ObjectDestination::Hand),
            named("Goliath Sphinx", 2, B, ObjectDestination::Graveyard),
            named("Elvish Mystic", 3, A, ObjectDestination::Graveyard),
            creature(4, B, 1, 1, &[]),
            seed(
                5,
                A,
                ObjectDestination::Graveyard,
                Program::artifact(None, vec![]),
            ),
        ],
        &cost,
        Step::PrecombatMain,
    );
    let choice = round(cast_untargeted(&game, 1, &cost).into_state()).into_state();
    let RuleFrame::ZoneReplacement(frame) = choice.pending_decision().unwrap().frame() else {
        panic!("entry copy")
    };
    let ZoneReplacementRequest::EntryCopy { donors, .. } = frame.request() else {
        panic!("entry copy candidates")
    };
    assert_eq!(
        donors.iter().copied().collect::<BTreeSet<_>>(),
        [physical(&choice, 2).id(), physical(&choice, 3).id()].into()
    );
    assert_eq!(
        physical(&copied(&choice, None), 1).zone(),
        ObjectZone::Graveyard(A)
    );
    let made = copied(&choice, Some(2));
    assert_eq!(physical(&made, 1).owner(), A);
    assert_eq!(physical(&made, 1).zone(), ObjectZone::Battlefield);
    assert_eq!(physical(&made, 2).zone(), ObjectZone::Graveyard(B));
    let current = sheet(&made, 1);
    assert!(
        current
            .current_names
            .iter()
            .any(|n| n.as_str() == "Goliath Sphinx")
    );
    assert!(current.has_keyword(KeywordAbility::Flying));
    assert_eq!(size(&made, 1), (8, 7));
    assert_eq!(size(&copied(&choice, Some(3)), 1), (1, 1));
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_spark_double_selects_own_donor_and_adds_only_the_applicable_counter() {
    use ManaType::*;
    let cost = [(Colourless, 3), (Blue, 1)];
    let game = game(
        vec![A, B],
        vec![
            named("Spark Double", 1, A, ObjectDestination::Hand),
            named("Zetalpa, Primal Dawn", 2, A, on_field(A)),
            named(
                "Ob Nixilis of the Black Oath",
                3,
                A,
                ObjectDestination::Hand,
            ),
            creature(4, B, 2, 2, &[]),
        ],
        &[(Colourless, 6), (Blue, 1), (Black, 2)],
        Step::PrecombatMain,
    );
    let walker =
        round(cast_untargeted(&game, 3, &[(Colourless, 3), (Black, 2)]).into_state()).into_state();
    assert_eq!(
        walker
            .execution()
            .loyalty_counters()
            .get(&physical(&walker, 3).id()),
        Some(&3.into())
    );
    let choice = round(cast_untargeted(&walker, 1, &cost).into_state()).into_state();
    let RuleFrame::ZoneReplacement(frame) = choice.pending_decision().unwrap().frame() else {
        panic!("entry copy")
    };
    let ZoneReplacementRequest::EntryCopy { donors, .. } = frame.request() else {
        panic!("entry copy donors")
    };
    assert_eq!(
        donors.iter().copied().collect::<BTreeSet<_>>(),
        [physical(&choice, 2).id(), physical(&choice, 3).id()].into()
    );
    assert_eq!(
        physical(&copied(&choice, None), 1).zone(),
        ObjectZone::Graveyard(A)
    );
    let beast = copied(&choice, Some(2));
    assert_eq!(size(&beast, 1), (5, 9));
    assert_eq!(size(&beast, 2), (4, 8));
    assert!(
        !sheet(&beast, 1)
            .current_supertypes
            .contains(&Supertype::Legendary)
    );
    assert!(
        sheet(&beast, 2)
            .current_supertypes
            .contains(&Supertype::Legendary)
    );
    assert_eq!(
        beast
            .execution()
            .power_toughness_counters(physical(&beast, 1).id())
            .unwrap()
            .plus_one,
        1.into()
    );
    assert_eq!(
        beast
            .execution()
            .copiable_program(physical(&beast, 1).id())
            .unwrap()
            .permanent()
            .unwrap()
            .printed_pt,
        beast
            .execution()
            .copiable_program(physical(&beast, 2).id())
            .unwrap()
            .permanent()
            .unwrap()
            .printed_pt
    );
    let walker = copied(&choice, Some(3));
    assert!(sheet(&walker, 1).has_type(CardType::Planeswalker));
    assert!(
        !sheet(&walker, 1)
            .current_supertypes
            .contains(&Supertype::Legendary)
    );
    assert_eq!(
        walker
            .execution()
            .loyalty_counters()
            .get(&physical(&walker, 1).id()),
        Some(&4.into())
    );
    assert!(
        walker
            .execution()
            .power_toughness_counters(physical(&walker, 1).id())
            .is_none_or(|c| c.plus_one == 0.into())
    );
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_starfield_animates_by_mana_value_and_restores_the_fifth_enchantment() {
    use ManaType::*;
    let cost = [(Colourless, 4), (White, 1)];
    let mut cards = vec![
        named("Starfield of Nyx", 1, A, ObjectDestination::Hand),
        creature(7, A, 2, 2, &[]),
        seed(
            5,
            A,
            ObjectDestination::Hand,
            Program::aura(
                Some(ManaCost::free()),
                spellwake_engine::attachment::EnchantRestriction::Creature,
            ),
        ),
        seed(
            6,
            A,
            ObjectDestination::Hand,
            free_instant(BodyTarget::SinglePermanent, Instruction::DestroyTarget),
        ),
        seed(
            8,
            B,
            on_field(B),
            Program::enchantment(Some(ManaCost::generic(3.into()))),
        ),
    ];
    for n in 2..=4 {
        cards.push(seed(
            n,
            A,
            on_field(A),
            Program::enchantment(Some(ManaCost::generic((n as i64).into()))),
        ));
    }
    let initial = game(vec![A, B], cards, &cost, Step::PrecombatMain);
    let four = round(cast_untargeted(&initial, 1, &cost).into_state()).into_state();
    assert!(!sheet(&four, 2).is_creature());
    let five =
        round(targeted(&four, A, 5, TargetRef::Object(physical(&four, 7).id())).into_state())
            .into_state();
    for n in 2..=4 {
        assert!(sheet(&five, n).has_type(CardType::Enchantment));
        assert_eq!(size(&five, n), (n as i64, n as i64));
    }
    for n in [1, 5, 8] {
        assert!(!sheet(&five, n).is_creature());
    }
    let lost =
        round(targeted(&five, A, 6, TargetRef::Object(physical(&five, 4).id())).into_state())
            .into_state();
    assert!(!sheet(&lost, 2).is_creature());
    assert_eq!(physical(&lost, 4).zone(), ObjectZone::Graveyard(A));
    let choice = exile_hand::until(lost, |s| {
        matches!(
            s.pending_decision().map(|d| d.frame()),
            Some(RuleFrame::TriggerTarget(_))
        )
    });
    let stack = reply(
        &choice,
        DecisionReply::TriggerTarget(TargetRef::Object(physical(&choice, 4).id())),
    )
    .into_state();
    let offer = round(stack).into_state();
    let decline = reply(
        &offer,
        DecisionReply::OptionalEffect(OptionalEffectChoice::Decline),
    )
    .into_state();
    assert_eq!(physical(&decline, 4).zone(), ObjectZone::Graveyard(A));
    let restored = reply(
        &offer,
        DecisionReply::OptionalEffect(OptionalEffectChoice::Take),
    )
    .into_state();
    assert_eq!(physical(&restored, 4).zone(), ObjectZone::Battlefield);
    assert_eq!(size(&restored, 2), (2, 2));
    assert_eq!(size(&restored, 4), (4, 4));
    assert!(!sheet(&restored, 5).is_creature());
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_myriad_construct_counts_opposing_nonbasic_lands_and_uses_last_power() {
    use ManaType::Colourless;
    for (kicker, size) in [(false, 4_i64), (true, 6)] {
        let mut basic = Program::land(Default::default(), vec![]);
        basic.supertypes.insert(Supertype::Basic);
        let cards = vec![
            named("Myriad Construct", 1, A, ObjectDestination::Hand),
            seed(2, A, on_field(A), Program::land(Default::default(), vec![])),
            seed(3, B, on_field(B), Program::land(Default::default(), vec![])),
            seed(4, C, on_field(C), Program::land(Default::default(), vec![])),
            seed(5, B, on_field(B), basic),
            seed(
                6,
                A,
                ObjectDestination::Hand,
                free_instant(BodyTarget::SingleCreature, Instruction::DestroyTarget),
            ),
        ];
        let paid = if kicker { 7 } else { 4 };
        let start = game(
            vec![A, B, C],
            cards,
            &[(Colourless, 7)],
            Step::PrecombatMain,
        );
        let cast = command(
            &start,
            A,
            GameCommand::CastWithResources {
                spell: physical(&start, 1).id(),
                target: None,
                payment: CastingPayment {
                    additional: if kicker {
                        [(0, 1.into())].into()
                    } else {
                        Default::default()
                    },
                    mana: Some(spend(&[(Colourless, paid)])),
                    ..Default::default()
                },
            },
        )
        .into_state();
        let entering = round(cast).into_state();
        assert_eq!(entering.execution().mana(A, Colourless), (7 - paid).into());
        assert_eq!(
            entering
                .execution()
                .power_toughness_counters(physical(&entering, 1).id())
                .unwrap()
                .plus_one,
            (size - 4).into()
        );
        let stacked = targeted(
            &entering,
            A,
            6,
            TargetRef::Object(physical(&entering, 1).id()),
        )
        .into_state();
        assert_eq!(stacked.execution().window().entries().len(), 2);
        let ordering = round(stacked).into_state();
        assert_eq!(physical(&ordering, 1).zone(), ObjectZone::Graveyard(A));
        let RuleFrame::TimestampOrder(frame) = ordering.pending_decision().unwrap().frame() else {
            panic!("Construct arrival order")
        };
        let made = reply(
            &ordering,
            DecisionReply::TimestampOrder(frame.members().iter().map(|m| m.member).collect()),
        );
        for object in tokens(&made, A, size as usize) {
            let card = made
                .state()
                .execution()
                .current_characteristics(object)
                .unwrap()
                .unwrap();
            assert_eq!(
                card.current_types,
                [CardType::Artifact, CardType::Creature].into()
            );
            assert_eq!(
                card.current_creature_subtypes,
                [current_catalogue().1.resolve_label("Construct").unwrap()].into()
            );
            assert!(card.current_colors.is_empty());
            assert_eq!(
                card.power_toughness.unwrap().derived,
                PowerToughness {
                    power: 1.into(),
                    toughness: 1.into()
                }
            );
        }
        let done = round(made.into_state()).into_state();
        assert_eq!(physical(&done, 6).zone(), ObjectZone::Graveyard(A));
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_assault_suit_prevents_sacrifice_and_attacks_on_its_controller_and_lends_until_cleanup() {
    use ManaType::{Black, Colourless};
    let start = game(
        vec![A, B, C],
        vec![
            named("Assault Suit", 1, A, ObjectDestination::Hand),
            creature(2, A, 2, 3, &[]),
            named(
                "Ob Nixilis of the Black Oath",
                4,
                A,
                ObjectDestination::Hand,
            ),
            seed(
                3,
                A,
                ObjectDestination::Hand,
                free_instant(BodyTarget::None, sacrifice_each()),
            ),
        ],
        &[(Colourless, 10), (Black, 2)],
        Step::PrecombatMain,
    );
    let start =
        round(cast_untargeted(&start, 4, &[(Colourless, 3), (Black, 2)]).into_state()).into_state();
    let suit = round(cast_untargeted(&start, 1, &[(Colourless, 4)]).into_state()).into_state();
    let wearer = physical(&suit, 2).id();
    let equip = command(
        &suit,
        A,
        GameCommand::ActivateAbility {
            source: physical(&suit, 1).id(),
            ability: ActivatedAbilityId(1),
            target: TargetRef::Object(wearer),
            payment: Some(spend(&[(Colourless, 3)])),
        },
    )
    .into_state();
    let equipped = round(equip).into_state();
    assert_eq!(size(&equipped, 2), (4, 5));
    assert!(sheet(&equipped, 2).has_keyword(KeywordAbility::Haste));
    assert_eq!(
        equipped
            .execution()
            .attached_to(physical(&equipped, 1).id()),
        Some(TargetRef::Object(wearer))
    );
    let protected = round(cast_untargeted(&equipped, 3, &[]).into_state()).into_state();
    assert_eq!(physical(&protected, 2).zone(), ObjectZone::Battlefield);
    let combat = combat_frame(&protected, true);
    let attack = |defender| {
        DecisionReply::DeclareAttackers(vec![AttackDeclaration {
            attacker: wearer,
            defender,
            defending_planeswalker: None,
        }])
    };
    let tapped = reply(&combat, attack(C)).into_state();
    let offer = exile_hand::until(tapped, |state| {
        matches!(
            state.pending_decision().map(|d| d.frame()),
            Some(RuleFrame::OptionalEffect(_))
        )
    });
    let declined = reply(
        &offer,
        DecisionReply::OptionalEffect(OptionalEffectChoice::Decline),
    )
    .into_state();
    assert_eq!(physical(&declined, 2).controller(), Some(A));
    assert_eq!(physical(&declined, 2).tapped(), Some(true));
    let lent = reply(
        &offer,
        DecisionReply::OptionalEffect(OptionalEffectChoice::Take),
    )
    .into_state();
    assert_eq!(physical(&lent, 2).controller(), Some(B));
    assert_eq!(physical(&lent, 2).tapped(), Some(false));
    let declaration = combat_frame(&lent, true);
    illegal(
        &declaration,
        B,
        GameCommand::Reply {
            decision: declaration.pending_decision().unwrap().id(),
            choice: attack(A),
        },
    );
    illegal(
        &declaration,
        B,
        GameCommand::Reply {
            decision: declaration.pending_decision().unwrap().id(),
            choice: DecisionReply::DeclareAttackers(vec![AttackDeclaration {
                attacker: wearer,
                defender: A,
                defending_planeswalker: Some(physical(&declaration, 4).id()),
            }]),
        },
    );
    let attacked = reply(&declaration, attack(C)).into_state();
    let returned = exile_hand::until(attacked, |state| {
        state.turn().unwrap().position().number == 3
    });
    assert_eq!(physical(&returned, 2).controller(), Some(A));
    assert_eq!(size(&returned, 2), (4, 5));
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_myriad_construct_ignores_ability_targets_and_uses_equipped_power_when_sacrifice_fails_or_source_leaves()
 {
    use ManaType::Colourless;
    let initial = game(
        vec![A, B],
        vec![
            named("Myriad Construct", 1, A, ObjectDestination::Hand),
            named("Assault Suit", 2, A, on_field(A)),
            seed(
                3,
                A,
                ObjectDestination::Hand,
                free_instant(BodyTarget::SingleCreature, Instruction::DestroyTarget),
            ),
            seed(
                4,
                A,
                ObjectDestination::Hand,
                free_instant(
                    BodyTarget::None,
                    Instruction::DestroyGroup(Box::new(
                        spellwake_engine::program::ObjectFilter::creature()
                            .with_power(spellwake_engine::program::PowerFilter::AtLeast(4)),
                    )),
                ),
            ),
        ],
        &[(Colourless, 7)],
        Step::PrecombatMain,
    );
    let cast = round(cast_untargeted(&initial, 1, &[(Colourless, 4)]).into_state()).into_state();
    let equip = command(
        &cast,
        A,
        GameCommand::ActivateAbility {
            source: physical(&cast, 2).id(),
            ability: ActivatedAbilityId(1),
            target: TargetRef::Object(physical(&cast, 1).id()),
            payment: Some(spend(&[(Colourless, 3)])),
        },
    )
    .into_state();
    assert_eq!(equip.execution().window().entries().len(), 1);
    let equipped = round(equip).into_state();
    assert_eq!(size(&equipped, 1), (6, 6));
    let spell = targeted(
        &equipped,
        A,
        3,
        TargetRef::Object(physical(&equipped, 1).id()),
    )
    .into_state();
    for removed in [false, true] {
        let stack = if removed {
            let wipe = cast_untargeted(&spell, 4, &[]).into_state();
            let left = round(wipe).into_state();
            assert_eq!(physical(&left, 1).zone(), ObjectZone::Graveyard(A));
            left
        } else {
            spell.clone()
        };
        let order = round(stack).into_state();
        let RuleFrame::TimestampOrder(frame) = order.pending_decision().unwrap().frame() else {
            panic!("Construct order");
        };
        let made = reply(
            &order,
            DecisionReply::TimestampOrder(frame.members().iter().map(|m| m.member).collect()),
        );
        tokens(&made, A, 6);
        assert_eq!(
            physical(made.state(), 1).zone(),
            if removed {
                ObjectZone::Graveyard(A)
            } else {
                ObjectZone::Battlefield
            }
        );
        let finish = round(made.into_state()).into_state();
        assert_eq!(physical(&finish, 1).zone(), ObjectZone::Graveyard(A));
        assert_eq!(physical(&finish, 3).zone(), ObjectZone::Graveyard(A));
    }
}
