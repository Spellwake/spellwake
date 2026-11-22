use super::*;
use crate::{
    damage::{DamageEffectId, DamageOutcome, DamageRecipient},
    effects::DamageDecision,
    program::DamageGroup,
};

#[test]
fn damage_order_refreshes_after_a_recipient_leaves_without_replaying_resolution() {
    let fourth = SeatId(19);
    let barrier = || Instruction::CreatePreventionShield { amount: 1.into() };
    let amplify = || Instruction::CreateDamageMultiplier { factor: 2.into() };
    let mut cards = [barrier(), amplify(), barrier(), amplify()]
        .into_iter()
        .enumerate()
        .map(|(index, instruction)| {
            card(
                u64::try_from(index).unwrap() + 1,
                A,
                ObjectDestination::Hand,
                Program::instant(
                    ManaCost::free(),
                    ExecutableBody::new(BodyTarget::SingleCreature, vec![instruction]),
                ),
            )
        })
        .collect::<Vec<_>>();
    cards.push(card(
        5,
        A,
        ObjectDestination::Hand,
        Program::instant(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::None,
                vec![
                    Instruction::AdjustControllerLife {
                        adjustment: LifeAdjustment::Gain(1.into()),
                    },
                    Instruction::DamageGroup {
                        group: DamageGroup::CreaturesAndPlayers,
                        amount: 5.into(),
                    },
                    Instruction::AdjustControllerLife {
                        adjustment: LifeAdjustment::Gain(2.into()),
                    },
                ],
            ),
        ),
    ));
    for (id, seat) in [(10, B), (11, C)] {
        cards.push(card(
            id,
            seat,
            ObjectDestination::Battlefield { controller: seat },
            Program::creature(None, 1.into(), 20.into(), BTreeSet::new()),
        ));
    }
    let roster = [A, B, C, fourth];
    let mut state = root(cards, &[20; 4], Some(Step::PrecombatMain), roster.to_vec());
    let leaving_creature = object(&state, 10);
    let surviving_creature = object(&state, 11);
    for id in 1..=5 {
        let spell = object(&state, id);
        let cast = match id {
            5 => GameCommand::CastUntargeted {
                spell,
                payment: None,
            },
            1 | 2 => GameCommand::Cast {
                spell,
                target: TargetRef::Object(leaving_creature),
            },
            _ => GameCommand::Cast {
                spell,
                target: TargetRef::Object(surviving_creature),
            },
        };
        state = step(&state, A, cast);
        for actor in roster {
            state = step(&state, actor, GameCommand::Pass);
        }
    }

    assert_eq!(state.execution().life_total(A), Some(&21.into()));
    let issued = state.pending_decision().unwrap();
    let RuleFrame::Damage(DamageDecision::Order(original)) = issued.frame() else {
        panic!("departing creature's controller must own the first prevention choice");
    };
    assert_eq!(issued.chooser(), B);
    let before = state.execution().pending_damage().unwrap();
    let removed_attempts = before
        .components()
        .filter_map(|component| {
            [
                DamageRecipient::Player(B),
                DamageRecipient::Creature(leaving_creature),
            ]
            .contains(&component.attempt().target)
            .then_some(component.attempt().id)
        })
        .collect::<BTreeSet<_>>();
    assert_eq!(removed_attempts.len(), 2);

    let departed = state
        .advance(input(&state, B, GameCommand::Concede))
        .unwrap();
    assert!(departed.damage_journal().attempts.is_empty());
    let state = departed.into_state();
    let rebound = state.pending_decision().unwrap();
    let RuleFrame::Damage(DamageDecision::Order(revised)) = rebound.frame() else {
        panic!("surviving creature's controller must own the next prevention choice");
    };
    assert_ne!(rebound.id(), issued.id());
    assert_eq!(rebound.chooser(), C);
    assert_eq!(revised.batch(), original.batch());
    assert_eq!(state.execution().life_total(A), Some(&21.into()));
    let prevention = revised
        .applications()
        .iter()
        .copied()
        .find(|application| matches!(application.effect, DamageEffectId::Prevention(_)))
        .unwrap();
    let completed = state
        .advance(input(
            &state,
            C,
            GameCommand::Reply {
                decision: rebound.id(),
                choice: DecisionReply::DamageOrder(prevention),
            },
        ))
        .unwrap();
    let damage = completed.damage_journal();
    assert!(damage.attempts.is_empty());
    assert_eq!(
        damage
            .completions
            .iter()
            .filter(|completion| completion.outcome == DamageOutcome::RecipientUnavailable)
            .map(|completion| completion.attempt)
            .collect::<BTreeSet<_>>(),
        removed_attempts
    );
    let state = completed.state();
    assert_eq!(state.execution().life_total(B), Some(&20.into()));
    assert_eq!(state.execution().life_total(A), Some(&18.into()));
    assert_eq!(
        state
            .execution()
            .battlefield_marked_damage(surviving_creature),
        Some(8.into())
    );
}
