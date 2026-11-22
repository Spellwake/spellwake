//! Named group spells with independent type, color and destruction controls.
use super::*;

fn finish_graveyard_choices(mut state: GameState) -> GameState {
    while let Some(decision) = state.pending_decision() {
        let RuleFrame::ResolutionGraveyardOrder(frame) = decision.frame() else {
            panic!(
                "expected owner's graveyard order, got {:?}",
                decision.frame()
            );
        };
        state = reply(
            &state,
            DecisionReply::OrderResolutionGraveyard(frame.domain().to_vec()),
        )
        .into_state();
    }
    state
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_group_destruction_and_sacrifice_distinguish_types_color_and_regeneration() {
    use ManaType::*;
    use spellwake_engine::activated::{ActivatedAbility, ActivatedAbilityId, SourceCost};
    let cases = [
        (
            "Wrath of God",
            vec![(Colourless, 2), (White, 2)],
            [false, true, true, false, false],
        ),
        (
            "Extinguish All Hope",
            vec![(Colourless, 4), (Black, 2)],
            [true, true, true, true, false],
        ),
        (
            "All Is Dust",
            vec![(Colourless, 7)],
            [false, true, false, true, false],
        ),
    ];
    for (name, price, survivors) in cases {
        let mut shielded = creature(5, A, 2, 2, &[]);
        shielded.program.activated.push(ActivatedAbility {
            id: ActivatedAbilityId(0),
            mana_cost: ManaCost::free(),
            source_cost: SourceCost::None,
            body: ExecutableBody::new(BodyTarget::None, vec![Instruction::RegenerateSource]),
        });
        let setup = game(
            vec![A, B, C],
            vec![
                hand(name),
                named(
                    "Spirited Companion",
                    2,
                    A,
                    ObjectDestination::Battlefield { controller: A },
                ),
                named(
                    "Darksteel Ingot",
                    3,
                    A,
                    ObjectDestination::Battlefield { controller: A },
                ),
                named(
                    "Zetalpa, Primal Dawn",
                    4,
                    B,
                    ObjectDestination::Battlefield { controller: B },
                ),
                shielded,
                named(
                    "Fencing Ace",
                    6,
                    C,
                    ObjectDestination::Battlefield { controller: C },
                ),
                named(
                    "Plains",
                    7,
                    C,
                    ObjectDestination::Battlefield { controller: C },
                ),
            ],
            &price,
            Step::PrecombatMain,
        );
        let activation = GameCommand::ActivateUntargetedAbility {
            source: physical(&setup, 5).id(),
            ability: ActivatedAbilityId(0),
            payment: None,
        };
        let armed = round(command(&setup, A, activation).into_state()).into_state();
        let finished = finish_graveyard_choices(cast_named(&armed, &price));
        for (id, present) in (2..=6).zip(survivors) {
            assert_eq!(
                physical(&finished, id).zone() == ObjectZone::Battlefield,
                present,
                "{name} card {id}"
            );
        }
        assert_eq!(physical(&finished, 7).zone(), ObjectZone::Battlefield);
        assert_eq!(physical(&finished, 1).zone(), ObjectZone::Graveyard(A));
        if name == "Extinguish All Hope" {
            assert_eq!(physical(&finished, 5).tapped(), Some(true));
        }
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_kindred_dominance_and_crippling_fear_preserve_the_chosen_type() {
    use ManaType::*;
    let human = current_catalogue().1.resolve_label("Human").unwrap();
    for (name, generic, shrink) in [("Kindred Dominance", 5, false), ("Crippling Fear", 2, true)] {
        let price = [(Colourless, generic), (Black, 2)];
        let initial = game(
            vec![A, B, C],
            vec![
                hand(name),
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
                creature(4, A, 4, 4, &[]),
                creature(5, B, 3, 3, &[Indestructible]),
                named(
                    "Mind Stone",
                    6,
                    C,
                    ObjectDestination::Battlefield { controller: C },
                ),
            ],
            &price,
            Step::PrecombatMain,
        );
        let choose = cast_named(&initial, &price);
        let resolved = finish_graveyard_choices(
            reply(&choose, DecisionReply::CreatureType(human)).into_state(),
        );
        for id in [2, 3, 6] {
            assert_eq!(physical(&resolved, id).zone(), ObjectZone::Battlefield);
        }
        let power = |state: &GameState| {
            state
                .execution()
                .power_toughness(physical(state, 4).id())
                .unwrap()
                .unwrap()
                .derived
                .power
        };
        if shrink {
            assert_eq!(physical(&resolved, 5).zone(), ObjectZone::Graveyard(B));
            assert_eq!(power(&resolved), 1.into());
            assert_eq!(power(&next_main(resolved, 2)), 4.into());
        } else {
            assert_eq!(physical(&resolved, 4).zone(), ObjectZone::Graveyard(A));
            assert_eq!(physical(&resolved, 5).zone(), ObjectZone::Battlefield);
        }
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_star_of_extinction_damages_creatures_and_planeswalkers_only_after_a_legal_land_target() {
    use ManaType::*;
    let price = [(Colourless, 5), (Red, 2)];
    let initial = game(
        vec![A, B],
        vec![
            hand("Star of Extinction"),
            named(
                "Forest",
                2,
                B,
                ObjectDestination::Battlefield { controller: B },
            ),
            creature(3, B, 2, 2, &[]),
            named(
                "Zetalpa, Primal Dawn",
                4,
                A,
                ObjectDestination::Battlefield { controller: A },
            ),
            named(
                "Mind Stone",
                5,
                B,
                ObjectDestination::Battlefield { controller: B },
            ),
            named("Jace Beleren", 6, A, ObjectDestination::Hand),
            seed(
                7,
                A,
                ObjectDestination::Hand,
                free_instant(BodyTarget::SinglePermanent, Instruction::DestroyTarget),
            ),
        ],
        &[(Colourless, 6), (Red, 2), (Blue, 2)],
        Step::PrecombatMain,
    );
    let walker = round(cast_untargeted(&initial, 6, &[(Colourless, 1), (Blue, 2)]).into_state())
        .into_state();
    let choose = |id| GameCommand::CastPaid {
        spell: physical(&walker, 1).id(),
        target: TargetRef::Object(physical(&walker, id).id()),
        payment: spend(&price),
    };
    illegal(&walker, A, choose(5));
    let cast = command(&walker, A, choose(2)).into_state();
    let removed =
        round(targeted(&cast, A, 7, TargetRef::Object(physical(&cast, 2).id())).into_state())
            .into_state();
    let fizzled = round(removed).into_state();
    for id in [3, 4, 5, 6] {
        assert_eq!(physical(&fizzled, id).zone(), ObjectZone::Battlefield);
    }
    let outcome = finish_graveyard_choices(round(cast).into_state());
    for id in [2, 3] {
        assert_eq!(physical(&outcome, id).zone(), ObjectZone::Graveyard(B));
    }
    assert_eq!(physical(&outcome, 6).zone(), ObjectZone::Graveyard(A));
    assert_eq!(physical(&outcome, 5).zone(), ObjectZone::Battlefield);
    let survivor = physical(&outcome, 4).id();
    assert_eq!(
        outcome.execution().battlefield_marked_damage(survivor),
        Some(20.into())
    );
    assert_eq!(outcome.execution().life_total(A), Some(&20.into()));
    assert_eq!(outcome.execution().life_total(B), Some(&20.into()));
}
