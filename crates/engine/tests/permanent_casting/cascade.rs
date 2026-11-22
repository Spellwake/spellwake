//! Real cast-trigger, private randomness and zone-identity boundaries of CR 702.85a.
use super::*;
use spellwake_engine::{
    effects::{ExileCastAbility, OptionalEffectChoice},
    mana::CastingPayment,
    program::{TriggeredAbility, TriggeredAbilityId},
};
fn scenario(hit: bool, copies: u64) -> GameState {
    let mut original = creature(
        1,
        A,
        ObjectDestination::Hand,
        Some(ManaCost::generic(4.into())),
        4,
    );
    original.program.triggered = (0..copies)
        .map(|i| TriggeredAbility::cascade(TriggeredAbilityId(i)))
        .collect();
    let low = creature(
        4,
        A,
        ObjectDestination::LibraryBottom,
        Some(ManaCost::generic(if hit { 2.into() } else { 4.into() })),
        2,
    );
    let mut answer = spell(
        6,
        B,
        Instruction::CounterSpell,
        ManaCost::free(),
        BodyTarget::SingleSpell(spellwake_engine::program::SpellFilter::any()),
    );
    answer.destination = ObjectDestination::Hand;
    initialize(
        Some(Step::PrecombatMain),
        vec![
            original,
            land(2, ObjectDestination::LibraryBottom, false),
            creature(
                3,
                A,
                ObjectDestination::LibraryBottom,
                Some(ManaCost::generic(4.into())),
                2,
            ),
            low,
            land(5, ObjectDestination::LibraryBottom, false),
            answer,
        ],
        vec![FloatingMana {
            player: A,
            mana_type: ManaType::Green,
            amount: 4.into(),
        }],
    )
    .into_state()
}
fn origin(game: &GameState) -> GameState {
    cast(game, A, 1, Some(payment(&[(ManaType::Green, 4)]))).into_state()
}
fn answer(g: &GameState, choice: DecisionReply) -> GameState {
    let p = g.pending_decision().unwrap();
    accept(
        g,
        p.chooser(),
        GameCommand::Reply {
            decision: p.id(),
            choice,
        },
    )
    .into_state()
}
fn ids(g: &GameState) -> Vec<u64> {
    let cards = g.execution().window().cards();
    cards
        .library(A)
        .iter()
        .map(|id| cards.get(*id).unwrap().card_id().unwrap().0)
        .collect()
}
#[test]
fn cascade_cast_and_decline_bottom_only_the_exiled_cards_retaining_the_original_library_top() {
    let orig = origin(&scenario(true, 1));
    assert_eq!(orig.execution().window().entries().len(), 2);
    let stack = orig
        .supply_entropy(orig.basis(), vec![1, 0])
        .unwrap()
        .into_state();
    let offer = resolve(&stack).into_state();
    assert_eq!(
        offer
            .cascade_reveals()
            .map(|(o, _)| o.card_id().unwrap().0)
            .collect::<Vec<_>>(),
        vec![2, 3, 4]
    );
    assert_eq!(ids(&offer), vec![5]);
    let RuleFrame::OptionalEffect(f) = offer.pending_decision().unwrap().frame() else {
        panic!("cascade cast offer");
    };
    assert_eq!(f.exile_cast().unwrap().ability, ExileCastAbility::Cascade);
    let state = answer(
        &offer,
        DecisionReply::CascadeCast {
            target: None,
            payment: CastingPayment::default(),
        },
    );
    assert_eq!(ids(&state), vec![5, 2, 3]);
    assert_eq!(state.entropy().unwrap().position(), 1);
    assert_eq!(physical(&state, 4).zone(), ObjectZone::Stack);
    assert_eq!(state.execution().window().entries().len(), 2);
    let after = resolve(&state).into_state();
    assert_eq!(physical(&after, 4).zone(), ObjectZone::Battlefield);
    assert_eq!(physical(&after, 1).zone(), ObjectZone::Stack);
    let declined = answer(
        &offer,
        DecisionReply::OptionalEffect(OptionalEffectChoice::Decline),
    );
    assert_eq!(ids(&declined), vec![5, 4, 2, 3]);
    assert_eq!(declined.entropy().unwrap().position(), 2);
}
#[test]
fn countering_original_spell_leaves_cascade_on_stack_and_uses_its_captured_value() {
    let orig = origin(&scenario(true, 1));
    let orig = orig
        .supply_entropy(orig.basis(), vec![0, 0])
        .unwrap()
        .into_state();
    let target = orig.execution().window().entries().first().unwrap().id();
    let passed = pass(&orig).into_state();
    let response = targeted(&passed, B, 6, TargetRef::Stack(target)).into_state();
    let after = resolve(&response).into_state();
    assert_eq!(physical(&after, 1).zone(), ObjectZone::Graveyard(A));
    let triggered = resolve(&after).into_state();
    let RuleFrame::OptionalEffect(f) = triggered.pending_decision().unwrap().frame() else {
        panic!("cascade still resolves");
    };
    assert_eq!(f.exile_cast().unwrap().spell, object(&triggered, 4));
}
#[test]
fn no_qualifying_card_randomizes_the_full_cohort_atomically_after_entropy_is_supplied() {
    let pre = origin(&scenario(false, 1));
    let passed = pass(&pre).into_state();
    let request = input(&passed, holder(&passed), GameCommand::Pass);
    assert_eq!(
        passed.advance(request),
        Err(GameError::Entropy(
            spellwake_engine::shuffle::EntropyError::Exhausted
        ))
    );
    assert_eq!(ids(&passed), vec![2, 3, 4, 5]);
    let ready = passed
        .supply_entropy(passed.basis(), vec![0, 0, 0])
        .unwrap()
        .into_state();
    let done = pass(&ready);
    assert!(done.shuffle_journal().is_empty());
    assert!(done.state().pending_decision().is_none());
    assert_eq!(ids(done.state()), vec![3, 4, 5, 2]);
    assert_eq!(done.state().cascade_reveals().count(), 4);
}
#[test]
fn multiple_printed_instances_create_distinct_cast_triggers() {
    let g = origin(&scenario(true, 2));
    let p = g.pending_decision().expect("two triggers to order");
    let RuleFrame::Triggers(frame) = p.frame() else {
        panic!("order simultaneous cascade triggers");
    };
    assert_eq!(frame.instances().len(), 2);
    let g = answer(&g, DecisionReply::OrderTriggers(frame.instances().to_vec()));
    assert_eq!(g.execution().window().entries().len(), 3);
}
