//! Real card definitions for copy effects, triggered casts and token identity.
use super::equipment::{cast_card, marker, sheet, stats};
use super::*;
mod copy_and_cast;
pub(super) fn main_on(state: GameState, turn: u64) -> GameState {
    exile_hand::until(state, |s| {
        s.turn().unwrap().position().number == turn
            && s.turn().unwrap().position().step == Step::PrecombatMain
    })
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_mirage_mirror_copies_the_chosen_artifact_creature_enchantment_or_land_until_cleanup() {
    use ManaType::*;
    let mirror = named("Mirage Mirror", 1, A, ObjectDestination::Hand);
    let ability = mirror.program.activated[0].id;
    let initial = game(
        vec![A, B],
        vec![
            mirror,
            named("Sol Ring", 2, B, battlefield(B)),
            named("Goliath Sphinx", 3, B, battlefield(B)),
            named("Glorious Anthem", 4, B, battlefield(B)),
            named("Mountain", 5, B, battlefield(B)),
            named("Jace Beleren", 6, A, ObjectDestination::Hand),
        ],
        &[(Colourless, 6), (Blue, 2)],
        Step::PrecombatMain,
    );
    let walker = cast_card(&initial, 6, &[(Colourless, 1), (Blue, 2)]);
    let live = cast_card(&walker, 1, &[(Colourless, 3)]);
    let source = physical(&live, 1).id();
    let transform = |n| GameCommand::ActivateAbility {
        source,
        ability,
        target: marker(&live, n),
        payment: Some(spend(&[(Colourless, 2)])),
    };
    illegal(&live, A, transform(6));
    for n in [2, 3, 4, 5] {
        let donor = sheet(&live, physical(&live, n).id());
        let changed = round(command(&live, A, transform(n)).into_state()).into_state();
        let mirror = sheet(&changed, source);
        assert_eq!(mirror.controller, Some(A));
        assert_eq!(mirror.current_names, donor.current_names);
        assert_eq!(mirror.current_colors, donor.current_colors);
        assert_eq!(mirror.current_types, donor.current_types);
        match n {
            2 | 5 => {
                let kind = if n == 2 {
                    ManaAbilityRef::Printed(ManaAbilityId(0))
                } else {
                    ManaAbilityRef::BasicLand(spellwake_engine::land::BasicLandType::Mountain)
                };
                let tap = command(
                    &changed,
                    A,
                    GameCommand::ActivateMana {
                        source,
                        ability: kind,
                    },
                )
                .into_state();
                assert_eq!(
                    tap.execution()
                        .mana(A, if n == 2 { Colourless } else { Red }),
                    (if n == 2 { 2 } else { 1 }).into()
                );
            }
            3 => {
                assert!(mirror.has_keyword(KeywordAbility::Flying));
                stats(&changed, source, 8, 7);
            }
            _ => {}
        }
        let expired = main_on(changed, 2);
        let old = sheet(&expired, source);
        assert_eq!(old.current_names, sheet(&live, source).current_names);
        assert_eq!(old.current_types, [CardType::Artifact].into());
    }
}
#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_zada_copies_only_its_controllers_single_target_spell_for_each_other_legal_controlled_creature()
 {
    use ManaType::*;
    let mut cards = vec![
        named("Zada, Hedron Grinder", 1, A, ObjectDestination::Hand),
        creature(2, A, 1, 3, &[]),
        creature(3, A, 2, 3, &[]),
        creature(4, A, 2, 3, &[KeywordAbility::Shroud]),
        creature(5, B, 3, 3, &[]),
    ];
    for (n, owner) in [(6, A), (7, B)] {
        cards.push(seed(
            n,
            owner,
            ObjectDestination::Hand,
            free_instant(
                BodyTarget::SINGLE_CREATURE,
                crate::keyword_creatures::toolbox_emit::deal_damage(
                    spellwake_engine::toolbox::DamageTargetQuery::Targets,
                    spellwake_engine::program::ValueExpr::literal(1),
                    false,
                ),
            ),
        ));
    }
    let two_targets = named("Curtains' Call", 8, A, ObjectDestination::Hand);
    let ProgramKind::Spell(printed) = &two_targets.program.kind else {
        panic!("two-target spell");
    };
    cards.push(seed(
        8,
        A,
        ObjectDestination::Hand,
        Program::instant(ManaCost::free(), printed.body.clone()),
    ));
    let price = [(Colourless, 3), (Red, 1)];
    let before = game(vec![A, B], cards, &price, Step::PrecombatMain);
    let field = cast_card(&before, 1, &price);
    let both = command(
        &field,
        A,
        GameCommand::CastWithResources {
            spell: physical(&field, 8).id(),
            target: None,
            payment: CastingPayment {
                permanent_targets: vec![marker(&field, 1), marker(&field, 2)],
                ..Default::default()
            },
        },
    )
    .into_state();
    assert_eq!(both.execution().window().entries().len(), 1);
    let ordinary = targeted(&field, A, 6, marker(&field, 2)).into_state();
    assert_eq!(ordinary.execution().window().entries().len(), 1);
    let rival = pass(&field).into_state();
    let foreign = targeted(&rival, B, 7, marker(&rival, 1)).into_state();
    assert_eq!(foreign.execution().window().entries().len(), 1);
    let spell = targeted(&field, A, 6, marker(&field, 1)).into_state();
    assert_eq!(spell.execution().window().entries().len(), 2);
    let order = round(spell).into_state();
    use spellwake_engine::effects::CopyTargetChoice;
    let place = |n| DecisionReply::CopyTargets(CopyTargetChoice::Change(marker(&order, n)));
    for illegal in [1, 4, 5] {
        super::declarations::invalid(&order, A, place(illegal));
    }
    let mut copies = reply(&order, place(2)).into_state();
    assert_eq!(copies.execution().window().entries().len(), 3);
    for _ in 0..3 {
        copies = round(copies).into_state();
    }
    for n in [1, 2, 3, 4, 5] {
        assert_eq!(
            copies
                .execution()
                .battlefield_marked_damage(physical(&copies, n).id()),
            Some((if n <= 3 { 1 } else { 0 }).into())
        );
    }
}
#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_maelstrom_wanderer_resolves_two_independent_cascades_before_it_enters_and_grants_haste_to_all_its_controllers_creatures()
 {
    use ManaType::*;
    let price = [(Colourless, 5), (Green, 1), (Blue, 1), (Red, 1)];
    let cards = vec![
        named("Maelstrom Wanderer", 1, A, ObjectDestination::Hand),
        creature(2, A, 2, 2, &[]),
        creature(3, B, 2, 3, &[]),
        named("Sol Ring", 10, A, ObjectDestination::LibraryBottom),
        named("Elvish Mystic", 11, A, ObjectDestination::LibraryBottom),
        seed(
            12,
            A,
            ObjectDestination::Hand,
            Program::creature(
                Some(ManaCost::free()),
                1.into(),
                2.into(),
                Default::default(),
            ),
        ),
    ];
    let initial = game(vec![A, B], cards, &price, Step::PrecombatMain);
    let cast = cast_untargeted(&initial, 1, &price).into_state();
    let RuleFrame::Triggers(order) = cast.pending_decision().unwrap().frame() else {
        panic!("two cascade triggers");
    };
    assert_eq!(order.instances().len(), 2);
    let mut stacked = reply(
        &cast,
        DecisionReply::OrderTriggers(order.instances().to_vec()),
    )
    .into_state();
    assert_eq!(stacked.execution().window().entries().len(), 3);
    for id in [10, 11] {
        let offer = round(stacked).into_state();
        let RuleFrame::OptionalEffect(frame) = offer.pending_decision().unwrap().frame() else {
            panic!("optional cascade cast");
        };
        assert_eq!(frame.exile_cast().unwrap().spell, physical(&offer, id).id());
        let declined = super::equipment::optional(&offer, false);
        assert_eq!(physical(&declined, id).zone(), ObjectZone::Library(A));
        let launch = reply(
            &offer,
            DecisionReply::CascadeCast {
                target: None,
                payment: Default::default(),
            },
        )
        .into_state();
        stacked = round(launch).into_state();
        assert_eq!(physical(&stacked, id).zone(), ObjectZone::Battlefield);
        assert_eq!(physical(&stacked, 1).zone(), ObjectZone::Stack);
    }
    let entered = round(stacked).into_state();
    let after = cast_card(&entered, 12, &[]);
    for n in [1, 2, 3, 11, 12] {
        assert_eq!(
            sheet(&after, physical(&after, n).id()).has_keyword(KeywordAbility::Haste),
            n != 3
        );
    }
    let ready = super::declarations::combat_on(after, 1);
    let _ = reply(
        &ready,
        super::declarations::attacks(&ready, &[(1, B, None), (11, B, None), (12, B, None)]),
    );
}
