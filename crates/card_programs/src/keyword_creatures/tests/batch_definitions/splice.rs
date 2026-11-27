//! Sourced splice cards exercise the ordinary casting and resolution commands.
use super::cmm_gameplay::named;
use super::*;
use spellwake_engine::mana::SpliceChoice;

fn cast(
    state: &GameState,
    host: u64,
    target: Option<TargetRef>,
    cards: &[(u64, Option<TargetRef>)],
    mana: &[(ManaType, i64)],
) -> GameCommand {
    GameCommand::CastWithResources {
        spell: physical(state, host).id(),
        target,
        payment: CastingPayment {
            splice: cards
                .iter()
                .map(|&(id, target)| SpliceChoice {
                    card: physical(state, id).id(),
                    target,
                })
                .collect(),
            mana: Some(spend(mana)),
            ..Default::default()
        },
    }
}

#[test]
#[ignore = "requires dated Oracle catalogue and creature vocabulary"]
fn splice_orders_revealed_cards_preserves_host_and_reuses_hand_cards() {
    use ManaType::{Blue, Red};
    let to_a = Some(TargetRef::Player(A));
    let to_b = Some(TargetRef::Player(B));
    let price = [(Blue, 5), (Red, 2)];
    for ray_first in [false, true] {
        let start = game(
            vec![A, B],
            vec![
                named("Glacial Ray", 1, A, ObjectDestination::Hand),
                named("Glacial Ray", 2, A, ObjectDestination::Hand),
                named("Everdream", 3, A, ObjectDestination::Hand),
            ],
            &price,
            Step::PrecombatMain,
        );
        let additions = if ray_first {
            [(2, to_a), (3, None)]
        } else {
            [(3, None), (2, to_a)]
        };
        let announced = command(&start, A, cast(&start, 1, to_b, &additions, &price));
        let reveals = events(&announced)
            .filter_map(|event| {
                let cards: Vec<_> = event
                    .occurrences
                    .iter()
                    .filter_map(|o| match &o.value {
                        RulesOccurrence::HandCardRevealed { object } => object.card_id(),
                        _ => None,
                    })
                    .collect();
                (!cards.is_empty()).then_some(cards)
            })
            .collect::<Vec<_>>();
        assert_eq!(reveals, vec![additions.map(|(id, _)| CardId(id)).to_vec()]);
        let state = announced.into_state();
        let entry = state.execution().window().entries().last().unwrap();
        let host = state
            .execution()
            .current_characteristics(physical(&state, 1).id())
            .unwrap()
            .unwrap();
        assert_eq!(
            host.current_colors,
            [spellwake_engine::characteristics::Color::Red].into()
        );
        assert_eq!(
            host.current_spell_subtypes,
            [spellwake_engine::program::SpellSubtype::Arcane].into()
        );
        let ops = &state
            .execution()
            .stack_body(entry.id())
            .unwrap()
            .instructions;
        assert_eq!(ops.len(), 3);
        assert!(matches!(ops[0], Instruction::DealDamage(_)));
        assert!(matches!(
            ops[if ray_first { 1 } else { 2 }],
            Instruction::DealDamage(_)
        ));
        assert!(matches!(
            ops[if ray_first { 2 } else { 1 }],
            Instruction::DrawCards(_)
        ));
        let resolved = round(state).into_state();
        assert_eq!(resolved.execution().life_total(A), Some(&18.into()));
        assert_eq!(resolved.execution().life_total(B), Some(&18.into()));
        for id in [2, 3, 100] {
            assert_eq!(physical(&resolved, id).zone(), ObjectZone::Hand(A));
        }
        assert_eq!(physical(&resolved, 1).zone(), ObjectZone::Graveyard(A));
        let printed = resolved
            .execution()
            .copiable_program(physical(&resolved, 1).id())
            .unwrap();
        let ProgramKind::Spell(definition) = &printed.kind else {
            panic!("printed spell")
        };
        assert_eq!(definition.body.instructions.len(), 1);
        assert_eq!(printed.mana_value(), 2.into());
    }
}

#[test]
#[ignore = "requires dated Oracle catalogue and creature vocabulary"]
fn splice_rejects_duplicate_wrong_zone_ineligible_host_missing_target_and_underpayment() {
    use ManaType::{Blue, Red};
    let budget = [(Blue, 10), (Red, 10)];
    let start = game(
        vec![A, B],
        vec![
            named("Glacial Ray", 1, A, ObjectDestination::Hand),
            named("Glacial Ray", 2, A, ObjectDestination::Hand),
            named("Everdream", 3, A, ObjectDestination::Hand),
            named("Glacial Ray", 4, B, ObjectDestination::Hand),
        ],
        &budget,
        Step::PrecombatMain,
    );
    let t = Some(TargetRef::Player(B));
    for invalid in [
        cast(&start, 1, t, &[(2, t), (2, t)], &[(Red, 6)]),
        cast(&start, 1, t, &[(4, t)], &[(Red, 4)]),
        cast(&start, 3, None, &[(2, t)], &[(Blue, 1), (Red, 4)]),
        cast(&start, 1, t, &[(1, t)], &[(Red, 4)]),
        cast(&start, 1, t, &[(2, None)], &[(Red, 4)]),
        cast(&start, 1, t, &[(3, t)], &[(Red, 2), (Blue, 3)]),
        cast(&start, 1, t, &[(2, t)], &[(Red, 2)]),
    ] {
        assert!(
            start
                .advance(GameInput {
                    basis: start.basis().clone(),
                    actor: A,
                    command: invalid
                })
                .is_err()
        );
    }
    let legal = command(&start, A, cast(&start, 1, t, &[(2, t)], &[(Red, 4)]));
    let resolved = round(legal.into_state()).into_state();
    assert_eq!(resolved.execution().life_total(B), Some(&16.into()));
}

#[test]
#[ignore = "requires dated Oracle catalogue and creature vocabulary"]
fn splice_rechecks_all_targets_and_preserves_untargeted_text_only_if_spell_resolves() {
    use ManaType::{Blue, Red};
    for retain_one in [true, false] {
        let start = game(
            vec![A, B],
            vec![
                named("Glacial Ray", 1, A, ObjectDestination::Hand),
                named("Glacial Ray", 2, A, ObjectDestination::Hand),
                named("Everdream", 3, A, ObjectDestination::Hand),
                cmm_gameplay::creature(4, A, 1, 1, &[]),
                seed(
                    5,
                    A,
                    ObjectDestination::Hand,
                    free_instant(BodyTarget::SINGLE_CREATURE, crate::keyword_creatures::oracle_lowering::toolbox_emit::return_to_owners_hand()),
                ),
            ],
            &[(Blue, 5), (Red, 2)],
            Step::PrecombatMain,
        );
        let creature_ref = TargetRef::Object(physical(&start, 4).id());
        let creature = Some(creature_ref);
        let other = if retain_one {
            Some(TargetRef::Player(B))
        } else {
            creature
        };
        let announced = command(
            &start,
            A,
            cast(
                &start,
                1,
                creature,
                &[(2, other), (3, None)],
                &[(Blue, 5), (Red, 2)],
            ),
        )
        .into_state();
        let response = targeted(&announced, A, 5, creature_ref).into_state();
        let original = round(response).into_state();
        let end = round(original).into_state();
        assert_eq!(
            end.execution().life_total(B),
            Some(&(if retain_one { 18 } else { 20 }).into())
        );
        assert_eq!(
            physical(&end, 100).zone(),
            if retain_one {
                ObjectZone::Hand(A)
            } else {
                ObjectZone::Library(A)
            }
        );
    }
}

#[test]
#[ignore = "requires dated Oracle catalogue and creature vocabulary"]
fn splice_spell_copies_keep_added_effects_and_choose_each_target_without_repaying() {
    use ManaType::Red;
    use spellwake_engine::effects::CopyTargetChoice;
    let start = game(
        vec![A, B],
        vec![
            named("Glacial Ray", 1, A, ObjectDestination::Hand),
            named("Glacial Ray", 2, A, ObjectDestination::Hand),
            seed(
                3,
                A,
                ObjectDestination::Hand,
                free_instant(
                    BodyTarget::single_spell(
                        spellwake_engine::program::SpellFilter::copyable_instruction(),
                    ),
                    Instruction::StackOp(Box::new(
                        spellwake_engine::toolbox::StackOpSpec::copy_targeted_spell(),
                    )),
                ),
            ),
        ],
        &[(Red, 4)],
        Step::PrecombatMain,
    );
    let announced = command(
        &start,
        A,
        cast(
            &start,
            1,
            Some(TargetRef::Player(B)),
            &[(2, Some(TargetRef::Player(B)))],
            &[(Red, 4)],
        ),
    )
    .into_state();
    let host = announced
        .execution()
        .window()
        .entries()
        .last()
        .unwrap()
        .id();
    let response = targeted(&announced, A, 3, TargetRef::Stack(host)).into_state();
    let choose = round(response).into_state();
    let first = reply(
        &choose,
        DecisionReply::CopyTargets(CopyTargetChoice::Retain),
    )
    .into_state();
    let second = reply(
        &first,
        DecisionReply::CopyTargets(CopyTargetChoice::Change(TargetRef::Player(A))),
    )
    .into_state();
    let copy = round(second).into_state();
    assert_eq!(copy.execution().life_total(A), Some(&18.into()));
    assert_eq!(copy.execution().life_total(B), Some(&18.into()));
    let end = round(copy).into_state();
    assert_eq!(end.execution().life_total(B), Some(&14.into()));
    assert_eq!(physical(&end, 2).zone(), ObjectZone::Hand(A));
}

#[test]
#[ignore = "requires dated Oracle catalogue and creature vocabulary"]
fn splice_everdream_can_add_its_draw_to_a_nonarcane_instant() {
    let state = game(
        vec![A, B],
        vec![
            named("Everdream", 1, A, ObjectDestination::Hand),
            named("Everdream", 2, A, ObjectDestination::Hand),
            seed(
                3,
                A,
                ObjectDestination::LibraryBottom,
                Program::artifact(None, vec![]),
            ),
        ],
        &[(ManaType::Blue, 5)],
        Step::PrecombatMain,
    );
    let announced = command(
        &state,
        A,
        cast(&state, 1, None, &[(2, None)], &[(ManaType::Blue, 5)]),
    )
    .into_state();
    let end = round(announced).into_state();
    for card in [2, 3, 100] {
        assert_eq!(physical(&end, card).zone(), ObjectZone::Hand(A));
    }
}

#[test]
#[ignore = "requires dated Oracle catalogue and creature vocabulary"]
fn splice_resolves_desperate_ritual_and_dampen_thought_from_the_host_in_announced_order() {
    use ManaType::{Blue, Red};
    let mut cards = vec![
        named("Glacial Ray", 1, A, ObjectDestination::Hand),
        named("Desperate Ritual", 2, A, ObjectDestination::Hand),
        named("Dampen Thought", 3, A, ObjectDestination::Hand),
    ];
    cards.extend((4..8).map(|id| {
        seed(
            id,
            B,
            ObjectDestination::LibraryBottom,
            Program::artifact(None, vec![]),
        )
    }));
    let price = [(Blue, 2), (Red, 4)];
    let state = game(vec![A, B], cards, &price, Step::PrecombatMain);
    let announced = command(
        &state,
        A,
        cast(
            &state,
            1,
            Some(TargetRef::Player(A)),
            &[(2, None), (3, Some(TargetRef::Player(B)))],
            &price,
        ),
    )
    .into_state();
    let mut end = round(announced).into_state();
    if let Some(RuleFrame::ResolutionGraveyardOrder(frame)) =
        end.pending_decision().map(|decision| decision.frame())
    {
        end = reply(
            &end,
            DecisionReply::OrderResolutionGraveyard(frame.domain().to_vec()),
        )
        .into_state();
    }
    assert_eq!(end.execution().life_total(A), Some(&18.into()));
    assert_eq!(end.execution().life_total(B), Some(&20.into()));
    assert_eq!(end.execution().mana(A, Red), 3.into());
    assert_eq!(end.execution().window().cards().library(B).len(), 1);
    assert_eq!(end.execution().window().cards().library(A).len(), 1);
}

#[test]
#[ignore = "requires dated Oracle catalogue and creature vocabulary"]
fn splice_switches_creature_stats_and_applies_hideous_laughter_to_all_creatures() {
    use ManaType::{Black, Red};
    let price = [(Red, 4), (Black, 5)];
    let state = game(
        vec![A, B],
        vec![
            named("Glacial Ray", 1, A, ObjectDestination::Hand),
            named("Strange Inversion", 2, A, ObjectDestination::Hand),
            named("Hideous Laughter", 3, A, ObjectDestination::Hand),
            cmm_gameplay::creature(4, B, 3, 6, &[]),
            cmm_gameplay::creature(5, A, 2, 2, &[]),
        ],
        &price,
        Step::PrecombatMain,
    );
    let target = physical(&state, 4).id();
    let announced = command(
        &state,
        A,
        cast(
            &state,
            1,
            Some(TargetRef::Player(B)),
            &[(2, Some(TargetRef::Object(target))), (3, None)],
            &price,
        ),
    )
    .into_state();
    let end = round(announced).into_state();
    let view = end
        .execution()
        .current_characteristics(target)
        .unwrap()
        .unwrap();
    let stats = &view.power_toughness.unwrap().derived;
    assert_eq!(
        (stats.power.clone(), stats.toughness.clone()),
        (4.into(), 1.into())
    );
    assert_eq!(physical(&end, 5).zone(), ObjectZone::Graveyard(A));
}
