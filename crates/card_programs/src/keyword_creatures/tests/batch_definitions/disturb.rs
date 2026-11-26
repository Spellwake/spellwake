//! Dated transforming cards demonstrate alternate-face casting, entry and departure.
use super::cmm_gameplay::{creature, illegal, named};
use super::*;

fn disturb(s: &GameState, target: Option<TargetRef>, mana: &[(ManaType, i64)]) -> GameCommand {
    GameCommand::CastWithResources {
        spell: physical(s, 1).id(),
        target,
        payment: CastingPayment {
            disturb: true,
            mana: Some(spend(mana)),
            ..Default::default()
        },
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn shared_disturb_baithook_angler_casts_its_flying_reverse_from_the_graveyard_and_exiles_only_for_graveyard_destinations()
 {
    use ManaType::*;
    use spellwake_engine::characteristics::KeywordAbility::Flying;
    let price = [(Colourless, 1), (Blue, 1)];
    let initial = game(
        vec![A, B],
        vec![
            named(
                "Baithook Angler // Hook-Haunt Drifter",
                1,
                A,
                ObjectDestination::Hand,
            ),
            seed(
                2,
                A,
                ObjectDestination::Hand,
                free_instant(
                    BodyTarget::SingleCreature,
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
            seed(
                3,
                A,
                ObjectDestination::Hand,
                free_instant(
                    BodyTarget::SingleCreature,
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
            seed(
                4,
                A,
                ObjectDestination::Hand,
                free_instant(
                    BodyTarget::SingleCreature,
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::return_to_owners_hand(
                    ),
                ),
            ),
            seed(
                5,
                B,
                ObjectDestination::Hand,
                free_instant(
                    BodyTarget::SingleSpell(spellwake_engine::program::SpellFilter::any()),
                    Instruction::CounterSpell,
                ),
            ),
        ],
        &[(Colourless, 2), (Blue, 2)],
        Step::PrecombatMain,
    );
    let card = |s: &GameState| {
        s.execution()
            .current_characteristics(physical(s, 1).id())
            .unwrap()
            .unwrap()
    };
    let role = |s: &GameState, back| {
        let view = card(s);
        assert_eq!(physical(s, 1).transformed(), back);
        assert_eq!(
            view.current_names.iter().next().unwrap().as_str(),
            if back {
                "Hook-Haunt Drifter"
            } else {
                "Baithook Angler"
            }
        );
        assert_eq!(view.current_colors, [Color::Blue].into());
        assert_eq!(view.defined.mana_value, 2.into());
        assert_eq!(view.has_keyword(Flying), back);
    };
    illegal(&initial, A, disturb(&initial, None, &price));
    let start = round(cast_untargeted(&initial, 1, &price).into_state()).into_state();
    role(&start, false);
    let destroy = |s: &GameState, n| {
        round(targeted(s, A, n, TargetRef::Object(physical(s, 1).id())).into_state()).into_state()
    };
    let grave = destroy(&start, 2);
    assert_eq!(physical(&grave, 1).zone(), ObjectZone::Graveyard(A));
    illegal(&grave, A, disturb(&grave, None, &[(Blue, 1)]));
    let stack = command(&grave, A, disturb(&grave, None, &price)).into_state();
    role(&stack, true);
    assert_eq!(stack.execution().mana(A, Blue), 0.into());
    let opp = pass(&stack).into_state();
    let target = TargetRef::Stack(opp.execution().window().entries().last().unwrap().id());
    let countered = round(targeted(&opp, B, 5, target).into_state()).into_state();
    assert_eq!(physical(&countered, 1).zone(), ObjectZone::Exile);
    role(&countered, false);
    let field = round(stack).into_state();
    role(&field, true);
    assert_eq!(physical(&destroy(&field, 3), 1).zone(), ObjectZone::Exile);
    let returned = destroy(&field, 4);
    assert_eq!(physical(&returned, 1).zone(), ObjectZone::Hand(A));
    role(&returned, false);
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn shared_disturb_drogskol_infantry_casts_its_aura_reverse_with_a_creature_target_and_exiles_when_unattached()
 {
    use ManaType::*;
    let price = [(Colourless, 3), (White, 1)];
    let initial = game(
        vec![A, B],
        vec![
            named(
                "Drogskol Infantry // Drogskol Armaments",
                1,
                A,
                ObjectDestination::Graveyard,
            ),
            creature(2, B, 1, 2, &[]),
            seed(
                3,
                A,
                ObjectDestination::Hand,
                free_instant(
                    BodyTarget::SingleCreature,
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
        ],
        &price,
        Step::PrecombatMain,
    );
    illegal(&initial, A, disturb(&initial, None, &price));
    illegal(
        &initial,
        A,
        disturb(&initial, Some(TargetRef::Player(B)), &price),
    );
    let target = TargetRef::Object(physical(&initial, 2).id());
    let stacked = command(&initial, A, disturb(&initial, Some(target), &price)).into_state();
    let view = stacked
        .execution()
        .current_characteristics(physical(&stacked, 1).id())
        .unwrap()
        .unwrap();
    assert_eq!(view.current_types, [CardType::Enchantment].into());
    assert_eq!(view.defined.mana_value, 2.into());
    let field = round(stacked).into_state();
    assert!(physical(&field, 1).transformed());
    assert_eq!(
        field
            .execution()
            .power_toughness(physical(&field, 2).id())
            .unwrap()
            .unwrap()
            .derived,
        PowerToughness {
            power: 3.into(),
            toughness: 4.into()
        }
    );
    let result = round(targeted(&field, A, 3, target).into_state());
    assert_eq!(physical(result.state(), 2).zone(), ObjectZone::Graveyard(B));
    assert_eq!(physical(result.state(), 1).zone(), ObjectZone::Exile);
}
