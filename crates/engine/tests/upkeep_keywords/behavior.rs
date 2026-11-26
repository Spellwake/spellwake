#![allow(dead_code, unused_imports)]
//! CR 702.24 and 702.30 through the ordinary turn scheduler, stack and payments.
use super::keyword_triggers::place;
use super::*;
use spellwake_engine::{
    characteristics::{ControlRecipient, ResolvedDuration, ResolvedPartSpec, ResolvedPartsSpec},
    effects::ResolutionPaymentReply,
    mana::{ManaAbility, ManaAbilityId, ManaAbilityRef, ManaOutput, ManaPayment, ManaSpend},
    occurrence::{RulesOccurrence, RulesRecord, ZoneReason},
    program::{TriggeredAbilityId, UpkeepKeyword},
};

fn source(keywords: &[UpkeepKeyword], cost: ManaCost) -> SeedCard {
    let mut seed = battlefield(1, A);
    seed.program.triggered = keywords
        .iter()
        .enumerate()
        .map(|(n, k)| k.ability(TriggeredAbilityId(n as u64), cost.clone()))
        .collect();
    seed
}
pub(super) fn land(id: u64, color: ManaType, amount: i64) -> SeedCard {
    SeedCard {
        card: CardId(id),
        owner: A,
        destination: ObjectDestination::Battlefield { controller: A },
        program: Program::land(
            Default::default(),
            vec![ManaAbility {
                id: ManaAbilityId(1),
                output: ManaOutput::Fixed([(color, amount.into())].into()),
            }],
        ),
    }
}
fn offer(state: &GameState, controller: SeatId, cost: &ManaCost) -> GameState {
    let offered = round(state).into_state();
    let RuleFrame::ResolutionPayment(frame) = offered.pending_decision().unwrap().frame() else {
        panic!("missing upkeep payment")
    };
    assert_eq!(frame.chooser(), controller);
    assert_eq!(frame.cost(), cost);
    offered
}
fn answer(state: &GameState, choice: ResolutionPaymentReply) -> NativeCandidate {
    reply(state, DecisionReply::ResolutionPayment(choice))
}
pub(super) fn upkeep(mut state: GameState, number: u64) -> GameState {
    for _ in 0..96 {
        let pos = state.turn().unwrap().position();
        if pos.number == number && pos.step == Step::Upkeep {
            return state;
        }
        if let Some(d) = state.pending_decision() {
            assert!(matches!(
                d.frame(),
                RuleFrame::Combat(CombatDecision::Attackers(_))
            ));
            state = reply(&state, DecisionReply::DeclareAttackers(vec![])).into_state();
        } else {
            state = act(&state, holder(&state), GameCommand::Pass).into_state();
        }
    }
    panic!("upkeep {number} never reached")
}
fn gain_control(state: &GameState, player: SeatId, spell: u64) -> GameState {
    let passed = act(state, holder(state), GameCommand::Pass).into_state();
    assert_eq!(holder(&passed), player);
    let cast = act(
        &passed,
        player,
        GameCommand::Cast {
            spell: object(&passed, spell).id(),
            target: TargetRef::Object(object(&passed, 1).id()),
        },
    );
    round(cast.state()).into_state()
}
#[test]
fn cumulative_instances_share_age_and_require_the_entire_current_price() {
    let one = ManaCost::one(ManaType::Blue);
    let ready = place(
        setup(
            vec![A, B],
            vec![
                source(&[UpkeepKeyword::CumulativeUpkeep; 2], one.clone()),
                land(2, ManaType::Blue, 3),
            ],
            Step::Untap,
        )
        .into_state(),
    );
    assert_eq!(ready.execution().window().entries().len(), 2);
    let id = object(&ready, 1).id();
    let first = offer(&ready, A, &one);
    assert_eq!(first.execution().age_counters()[&id], 1.into());
    let tapped = act(
        &first,
        A,
        GameCommand::ActivateMana {
            source: object(&first, 2).id(),
            ability: ManaAbilityRef::Printed(ManaAbilityId(1)),
        },
    )
    .into_state();
    let pay = |amount: i64| {
        ResolutionPaymentReply::Pay(ManaPayment {
            from_lots: Default::default(),
            spend: vec![ManaSpend {
                mana_type: ManaType::Blue,
                amount: amount.into(),
            }],
        })
    };
    let settled = answer(&tapped, pay(1)).into_state();
    let second = offer(&settled, A, &ManaCost::specific(ManaType::Blue, 2.into()));
    assert_eq!(second.execution().age_counters()[&id], 2.into());
    let pending = second.pending_decision().unwrap();
    assert!(
        second
            .advance(input(
                &second,
                A,
                GameCommand::Reply {
                    decision: pending.id(),
                    choice: DecisionReply::ResolutionPayment(pay(1))
                }
            ))
            .is_err()
    );
    assert_eq!(second.execution().mana(A, ManaType::Blue), 2.into());
    let kept = answer(&second, pay(2)).into_state();
    assert_eq!(kept.execution().mana(A, ManaType::Blue), 0.into());
    assert_eq!(object(&kept, 1).id(), id);
    let next = place(upkeep(kept, 3));
    let third = offer(&next, A, &ManaCost::specific(ManaType::Blue, 3.into()));
    let unpaid = answer(&third, ResolutionPaymentReply::Decline).into_state();
    assert_eq!(object(&unpaid, 1).zone(), ObjectZone::Graveyard(A));
    assert!(unpaid.execution().age_counters().is_empty());
    let missing = round(&unpaid).into_state();
    assert!(missing.pending_decision().is_none());
    assert!(missing.execution().window().entries().is_empty());
}
