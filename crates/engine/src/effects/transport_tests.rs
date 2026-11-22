//! Private transport units over actual owner operations, not new opcode execution.
use super::*;
use crate::program::ExecutableBody;

fn ready(state: EffectWindow) -> EffectWindow {
    state.settle_checks(state.revision()).unwrap()
}

#[test]
fn draw_prefix_survives_pending_and_later_replies_emit_only_the_new_suffix() {
    let a = SeatId(0);
    let b = SeatId(1);
    let mut state = ready(
        EffectWindow::new(GameSeed::with_uniform_life(
            vec![a, b],
            a,
            vec![
                SeedCard {
                    card: CardId(1),
                    owner: a,
                    destination: ObjectDestination::Hand,
                    program: Program::instant(
                        ManaCost::free(),
                        ExecutableBody::new(
                            BodyTarget::SingleCreature,
                            vec![Instruction::Damage { amount: 1.into() }],
                        ),
                    ),
                },
                SeedCard {
                    card: CardId(2),
                    owner: b,
                    destination: ObjectDestination::Hand,
                    program: Program::instant(
                        ManaCost::free(),
                        ExecutableBody::new(
                            BodyTarget::SingleSpell(
                                crate::program::SpellFilter::copyable_instruction(),
                            ),
                            vec![Instruction::CopySpell, Instruction::CopySpell],
                        ),
                    ),
                },
                SeedCard {
                    card: CardId(3),
                    owner: a,
                    destination: ObjectDestination::Battlefield { controller: a },
                    program: Program::creature(
                        None,
                        1.into(),
                        10.into(),
                        std::collections::BTreeSet::new(),
                    ),
                },
                SeedCard {
                    card: CardId(4),
                    owner: b,
                    destination: ObjectDestination::LibraryBottom,
                    program: Program::creature(
                        None,
                        1.into(),
                        1.into(),
                        std::collections::BTreeSet::new(),
                    ),
                },
            ],
            vec![CardId(3)],
            vec![],
            20.into(),
            CombatRules::UnrestrictedFreeForAll,
            crate::test_support::creature_types::synthetic_vocabulary(),
            std::sync::Arc::new(spellwake_contracts::names::NameRules::literal_names()),
        ))
        .unwrap(),
    );
    let object = |s: &EffectWindow, id| s.window.cards().current_identity(CardId(id)).unwrap();
    state = ready(
        state
            .cast(
                state.revision(),
                a,
                object(&state, 1),
                TargetRef::Object(object(&state, 3)),
            )
            .unwrap(),
    );
    let original = state.window.entries()[0].id();
    state = ready(state.pass(state.revision(), a).unwrap());
    state = ready(
        state
            .cast(
                state.revision(),
                b,
                object(&state, 2),
                TargetRef::Stack(original),
            )
            .unwrap(),
    );
    state = ready(state.pass(state.revision(), b).unwrap());
    state = state.pass(state.revision(), a).unwrap();
    let (mut state, RuleResult::Pending { .. }, _) = state
        .begin_resolution(state.revision())
        .unwrap()
        .into_parts()
    else {
        panic!("copy body did not suspend")
    };
    // The fixture contributes a real physical draw cut at the private driver seam.
    // It does not implement DrawController or claim an ordinary root pause here.
    let (window, prefix) = state
        .window
        .draw_cards(state.window.revision(), b, 1.into())
        .unwrap();
    state.window = window;
    let (state, RuleResult::Pending { frame, draws, .. }, _) = state
        .progress_rules(
            DamageJournal::default(),
            vec![],
            vec![],
            prefix.clone(),
            RulesJournal::default(),
        )
        .unwrap()
        .into_parts()
    else {
        panic!("missing first copy choice")
    };
    assert_eq!(draws, prefix);
    assert_eq!(draws.events.len(), 1);
    let (state, RuleResult::Pending { frame, draws, .. }, _) = state
        .resume_rule(
            state.revision(),
            &frame,
            b,
            DecisionReply::CopyTargets(CopyTargetChoice::Retain),
            None,
        )
        .unwrap()
        .into_parts()
    else {
        panic!("missing second copy choice")
    };
    assert!(draws.events.is_empty());
    let (state, RuleResult::Complete { draws, .. }, _) = state
        .resume_rule(
            state.revision(),
            &frame,
            b,
            DecisionReply::CopyTargets(CopyTargetChoice::Retain),
            None,
        )
        .unwrap()
        .into_parts()
    else {
        panic!("body did not complete")
    };
    assert!(draws.events.is_empty());
    assert_eq!(
        state.window.cards().get(object(&state, 4)).unwrap().zone(),
        ObjectZone::Hand(b)
    );
}
