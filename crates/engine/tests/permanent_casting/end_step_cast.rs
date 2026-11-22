use super::*;
use spellwake_engine::{
    characteristics::{
        ControlRecipient, KeywordAbility, ResolvedDuration, ResolvedPartSpec, ResolvedPartsSpec,
    },
    combat::CombatDecision,
    mana::CastingPayment,
    program::{EndStepCast, TriggeredAbilityId},
};

fn to_end_trigger(mut game: GameState) -> GameState {
    for _ in 0..80 {
        if game.turn().unwrap().position().step == Step::End
            && !game.execution().window().entries().is_empty()
        {
            return game;
        }
        if let Some(p) = game.pending_decision() {
            let choice = match p.frame() {
                RuleFrame::Combat(CombatDecision::Attackers(_)) => {
                    DecisionReply::DeclareAttackers(vec![])
                }
                RuleFrame::Combat(CombatDecision::Blockers(_)) => {
                    DecisionReply::DeclareBlockers(vec![])
                }
                other => panic!("unexpected {other:?}"),
            };
            game = accept(
                &game,
                p.chooser(),
                GameCommand::Reply {
                    decision: p.id(),
                    choice,
                },
            )
            .into_state();
        } else {
            game = pass(&game).into_state();
        }
    }
    panic!("end step not reached");
}

#[test]
fn blitz_only_sacrifices_what_its_trigger_controller_can_sacrifice_and_draws_for_the_new_controller()
 {
    let mut cards = seeds();
    let mut card = creature(100, A, ObjectDestination::Hand, Some(ManaCost::free()), 5);
    card.program = card.program.with_end_step_cast(
        EndStepCast::Blitz,
        ManaCost::free(),
        TriggeredAbilityId(1),
    );
    cards.push(card);
    cards.push(spell(
        101,
        B,
        Instruction::CreateContinuousEffect {
            parts: ResolvedPartsSpec(vec![ResolvedPartSpec::Control(
                ControlRecipient::ResolvingController,
            )]),
            duration: ResolvedDuration::NoStatedDuration,
        },
        ManaCost::free(),
        BodyTarget::SingleCreature,
    ));
    cards.push(spell(
        102,
        A,
        Instruction::DestroyTarget,
        ManaCost::free(),
        BodyTarget::SingleCreature,
    ));
    let game = setup(Some(Step::PrecombatMain), cards);
    let normal = cast(&game, A, 100, None);
    let normal = resolve(normal.state()).into_state();
    let current = normal
        .execution()
        .current_characteristics(object(&normal, 100))
        .unwrap()
        .unwrap();
    assert!(!current.has_keyword(KeywordAbility::Haste));
    assert!(current.abilities.triggered.is_empty());
    let cast = accept(
        &game,
        A,
        GameCommand::CastWithResources {
            spell: object(&game, 100),
            target: None,
            payment: CastingPayment {
                use_alternative: true,
                ..Default::default()
            },
        },
    );
    let entered = resolve(cast.state()).into_state();
    let subject = object(&entered, 100);
    let priority = pass(&entered);
    let stolen = targeted(priority.state(), B, 101, TargetRef::Object(subject));
    let controlled = resolve(stolen.state()).into_state();
    let triggered = to_end_trigger(controlled);
    let preserved = resolve(&triggered).into_state();
    assert_eq!(physical(&preserved, 100).controller(), Some(B));
    let destroy = targeted(&preserved, A, 102, TargetRef::Object(subject));
    let died = resolve(destroy.state()).into_state();
    assert_eq!(physical(&died, 100).zone(), ObjectZone::Graveyard(A));
    assert_eq!(died.execution().window().entries()[0].controller(), B);
    let drawn = resolve(&died).into_state();
    assert!(drawn.execution().window().cards().library(B).is_empty());
}
