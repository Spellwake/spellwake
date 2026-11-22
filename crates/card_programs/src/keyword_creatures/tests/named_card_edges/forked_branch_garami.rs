use super::*;
use spellwake_engine::{
    effects::OptionalEffectChoice,
    game::RuleFrame,
    mana::{CastResource, CastingPayment},
    stack_window::StackKind,
};

const CAST_GARAMI: u64 = 1;
const STOLEN_GARAMI: u64 = 2;
const LANTERN_KAMI: u64 = 3;
const KAMI_OF_OLD_STONE: u64 = 4;
const HIGH_COST_SPIRIT: u64 = 5;
const NON_SPIRIT: u64 = 6;
const OPPONENT_SPIRIT: u64 = 7;
const MURDER: u64 = 8;
const MURDEROUS_CUT: u64 = 9;
const BEARS: u64 = 10;

fn place_soulshift_triggers(targets: [u64; 2]) -> GameState {
    use ManaType::{Black, Colourless, Green};
    use ObjectDestination::{Battlefield, Graveyard, Hand};
    let state = game(
        vec![
            seed("Forked-Branch Garami", CAST_GARAMI, A, Hand),
            seed(
                "Forked-Branch Garami",
                STOLEN_GARAMI,
                B,
                Battlefield { controller: A },
            ),
            seed("Lantern Kami", LANTERN_KAMI, A, Graveyard),
            seed("Kami of Old Stone", KAMI_OF_OLD_STONE, A, Graveyard),
            seed("Kami of the Palace Fields", HIGH_COST_SPIRIT, A, Graveyard),
            seed("Forest", NON_SPIRIT, A, Graveyard),
            seed("Lantern Kami", OPPONENT_SPIRIT, B, Graveyard),
            seed("Murder", MURDER, B, Hand),
            seed("Murderous Cut", MURDEROUS_CUT, A, Hand),
            seed("Grizzly Bears", BEARS, B, Battlefield { controller: B }),
        ],
        &[
            (A, Colourless, 6),
            (A, Green, 2),
            (A, Black, 1),
            (B, Colourless, 1),
            (B, Black, 2),
        ],
    );
    let cast = advance(
        &state,
        A,
        GameCommand::CastUntargeted {
            spell: object(&state, CAST_GARAMI),
            payment: Some(payment(&[(Colourless, 3), (Green, 2)])),
        },
    );
    assert_eq!(cast.execution().mana(A, Colourless), 3.into());
    assert_eq!(cast.execution().mana(A, Green), 0.into());
    let resolved = resolve_top(&cast);
    let entered = resolved.permanent_entry().unwrap();
    assert_eq!(entered.after.id(), object(resolved.state(), CAST_GARAMI));
    let state = resolved.into_state();
    let garami = state
        .execution()
        .current_characteristics(object(&state, CAST_GARAMI))
        .unwrap()
        .unwrap();
    assert_eq!(garami.current_colors, [Color::Green].into());
    let pt = garami.power_toughness.unwrap().derived;
    assert_eq!((pt.power, pt.toughness), (4.into(), 4.into()));

    let stale_garami = object(&state, STOLEN_GARAMI);
    assert_eq!(physical(&state, STOLEN_GARAMI).owner(), B);
    assert_eq!(physical(&state, STOLEN_GARAMI).controller(), Some(A));
    let state = advance(&state, A, GameCommand::Pass);
    let murder = advance(
        &state,
        B,
        targeted_spell(&state, MURDER, stale_garami, &[(Colourless, 1), (Black, 2)]),
    );
    let death = resolve_top(&murder).into_state();
    assert_eq!(
        physical(&death, STOLEN_GARAMI).zone(),
        ObjectZone::Graveyard(B)
    );
    assert_ne!(object(&death, STOLEN_GARAMI), stale_garami);
    let decision = death.pending_decision().unwrap();
    let RuleFrame::Triggers(order) = decision.frame() else {
        panic!("expected two Garami death triggers, got {decision:?}");
    };
    let [first, second] = order.instances() else {
        panic!("expected exactly two Soulshift instances");
    };
    assert_ne!(first, second);
    let mut state = reply(
        &death,
        A,
        DecisionReply::OrderTriggers(vec![*first, *second]),
    );
    let mut entries = Vec::new();
    for (instance, card) in [*first, *second].into_iter().zip(targets) {
        let decision = state.pending_decision().unwrap();
        let RuleFrame::TriggerTarget(frame) = decision.frame() else {
            panic!("expected one target for each Soulshift instance, got {decision:?}");
        };
        assert_eq!(frame.instance(), instance);
        assert_eq!(
            frame.domain(),
            &[
                TargetRef::Object(object(&state, LANTERN_KAMI)),
                TargetRef::Object(object(&state, KAMI_OF_OLD_STONE)),
            ]
        );
        entries.push(frame.entry());
        let target = TargetRef::Object(object(&state, card));
        state = reply(&state, A, DecisionReply::TriggerTarget(target));
    }
    assert_ne!(entries[0], entries[1]);
    let stack = state.execution().window().entries();
    assert_eq!(stack.len(), 2);
    for ((entry, id), card) in stack.iter().zip(entries).zip(targets) {
        assert_eq!(entry.id(), id);
        assert_eq!(entry.controller(), A);
        assert!(
            matches!(entry.kind(), StackKind::TriggeredAbility { source } if source.id() == stale_garami)
        );
        let [target] = entry.targets().targets() else {
            panic!("expected a single bound target per Soulshift trigger");
        };
        assert_eq!(
            target.choice().target,
            TargetRef::Object(object(&state, card))
        );
    }
    state
}

fn resolve_soulshift(state: &GameState, choice: OptionalEffectChoice) -> GameState {
    let top = state.execution().window().entries().last().unwrap().id();
    let resolving = resolve_top(state).into_state();
    let decision = resolving.pending_decision().unwrap();
    let RuleFrame::OptionalEffect(frame) = decision.frame() else {
        panic!("expected Soulshift's independent optional effect, got {decision:?}");
    };
    assert_eq!(frame.entry(), top);
    reply(&resolving, A, DecisionReply::OptionalEffect(choice))
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn forked_branch_garami_can_return_two_distinct_spirits_with_separate_choices() {
    let stacked = place_soulshift_triggers([LANTERN_KAMI, KAMI_OF_OLD_STONE]);
    let first = resolve_soulshift(&stacked, OptionalEffectChoice::Take);
    assert_eq!(
        physical(&first, KAMI_OF_OLD_STONE).zone(),
        ObjectZone::Hand(A)
    );
    assert_eq!(
        physical(&first, LANTERN_KAMI).zone(),
        ObjectZone::Graveyard(A)
    );
    assert_eq!(first.execution().window().entries().len(), 1);

    let declined = resolve_soulshift(&first, OptionalEffectChoice::Decline);
    assert_eq!(
        physical(&declined, LANTERN_KAMI).zone(),
        ObjectZone::Graveyard(A)
    );
    assert!(declined.execution().window().entries().is_empty());
    let both = resolve_soulshift(&first, OptionalEffectChoice::Take);
    assert_eq!(physical(&both, LANTERN_KAMI).zone(), ObjectZone::Hand(A));
    assert_eq!(
        physical(&both, KAMI_OF_OLD_STONE).zone(),
        ObjectZone::Hand(A)
    );
    assert!(both.execution().window().entries().is_empty());
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn forked_branch_garami_can_choose_the_same_spirit_twice_and_loses_the_second_target() {
    let stacked = place_soulshift_triggers([LANTERN_KAMI, LANTERN_KAMI]);
    let bottom = stacked.execution().window().entries()[0].id();
    let first = resolve_soulshift(&stacked, OptionalEffectChoice::Take);
    assert_eq!(physical(&first, LANTERN_KAMI).zone(), ObjectZone::Hand(A));
    let removed = resolve_top(&first);
    assert_all_targets_illegal(&removed, bottom);
    assert!(removed.state().execution().window().entries().is_empty());
    assert_eq!(
        physical(removed.state(), KAMI_OF_OLD_STONE).zone(),
        ObjectZone::Graveyard(A)
    );
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn forked_branch_garami_returns_only_the_spirit_that_survives_delve() {
    use ManaType::{Black, Colourless};
    let stacked = place_soulshift_triggers([LANTERN_KAMI, KAMI_OF_OLD_STONE]);
    let top = stacked.execution().window().entries()[1].id();
    let exiled_target = object(&stacked, KAMI_OF_OLD_STONE);
    let cast = advance(
        &stacked,
        A,
        GameCommand::CastWithResources {
            spell: object(&stacked, MURDEROUS_CUT),
            target: Some(TargetRef::Object(object(&stacked, BEARS))),
            payment: CastingPayment {
                mana: Some(payment(&[(Colourless, 3), (Black, 1)])),
                objects: vec![CastResource::Delve(exiled_target)],
                ..Default::default()
            },
        },
    );
    assert_eq!(physical(&cast, KAMI_OF_OLD_STONE).zone(), ObjectZone::Exile);
    assert_ne!(object(&cast, KAMI_OF_OLD_STONE), exiled_target);
    let response = resolve_top(&cast).into_state();
    assert_eq!(physical(&response, BEARS).zone(), ObjectZone::Graveyard(B));
    let removed = resolve_top(&response);
    assert_all_targets_illegal(&removed, top);
    let surviving = resolve_soulshift(removed.state(), OptionalEffectChoice::Take);
    assert_eq!(
        physical(&surviving, LANTERN_KAMI).zone(),
        ObjectZone::Hand(A)
    );
    assert_eq!(
        physical(&surviving, KAMI_OF_OLD_STONE).zone(),
        ObjectZone::Exile
    );
    assert!(surviving.execution().window().entries().is_empty());
}
