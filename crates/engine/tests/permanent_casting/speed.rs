use super::*;
use spellwake_engine::{
    characteristics::{
        ControlRecipient, KeywordAbility, ResolvedDuration, ResolvedPartSpec, ResolvedPartsSpec,
    },
    life::{LifeAdjustment, LifeChangeKind},
    program::{PlayerRelation, TriggerGrouping, TriggeredAbility, TriggeredAbilityId},
};
#[test]
fn speed_survives_losing_every_engine_and_orders_its_sourceless_trigger_with_an_ordinary_trigger() {
    let mut cards = seeds();
    let mut engine = creature(
        100,
        A,
        ObjectDestination::Battlefield { controller: A },
        None,
        3,
    );
    engine
        .program
        .printed_keywords
        .insert(KeywordAbility::StartYourEngines);
    cards.push(engine);
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
    cards.push(spell(
        103,
        A,
        Instruction::AdjustTargetLife {
            adjustment: LifeAdjustment::Lose(1.into()),
        },
        ManaCost::free(),
        BodyTarget::SinglePlayer,
    ));
    let mut watcher = creature(
        104,
        A,
        ObjectDestination::Battlefield { controller: A },
        None,
        3,
    );
    watcher.program.triggered.push(TriggeredAbility {
        id: TriggeredAbilityId(4),
        grouping: TriggerGrouping::EachOccurrence,
        condition: spellwake_engine::program::TriggerSpec::life_changed(
            LifeChangeKind::Loss,
            PlayerRelation::Opponent,
        ),
        body: ExecutableBody::new(
            BodyTarget::None,
            vec![Instruction::AdjustControllerLife {
                adjustment: LifeAdjustment::Gain(1.into()),
            }],
        ),
    });
    cards.push(watcher);
    let game = setup(Some(Step::PrecombatMain), cards);
    assert_eq!(
        (game.execution().speed(A), game.execution().speed(B)),
        (1, 0)
    );
    let source = object(&game, 100);
    let priority = pass(&game);
    let cast = targeted(priority.state(), B, 101, TargetRef::Object(source));
    let stolen = resolve(cast.state()).into_state();
    assert_eq!(
        (stolen.execution().speed(A), stolen.execution().speed(B)),
        (1, 1)
    );
    let destroyed = targeted(&stolen, A, 102, TargetRef::Object(source));
    let removed = resolve(destroyed.state()).into_state();
    assert_eq!(physical(&removed, 100).zone(), ObjectZone::Graveyard(A));
    let ping = targeted(&removed, A, 103, TargetRef::Player(B));
    let order = resolve(ping.state()).into_state();
    let choice = order.pending_decision().unwrap();
    let RuleFrame::Triggers(frame) = choice.frame() else {
        panic!("order")
    };
    let mut ids = frame.instances().to_vec();
    assert_eq!(ids.len(), 2);
    ids.sort_by_key(|id| order.execution().waiting_speed_trigger(*id).is_some());
    let placed = accept(
        &order,
        A,
        GameCommand::Reply {
            decision: choice.id(),
            choice: DecisionReply::OrderTriggers(ids),
        },
    )
    .into_state();
    assert_eq!(
        placed.execution().window().entries().last().unwrap().kind(),
        &StackKind::Speed
    );
    let faster = resolve(&placed).into_state();
    assert_eq!(
        (faster.execution().speed(A), faster.execution().speed(B)),
        (2, 1)
    );
    let done = resolve(&faster).into_state();
    assert_eq!(done.execution().life_total(A), Some(&21.into()));
}
