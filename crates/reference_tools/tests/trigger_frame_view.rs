//! Exact privileged projection from a real prepared-root trigger decision.
use spellwake_engine::{
    combat::CombatRules,
    effects::SeedCard,
    game::{GameContext, GameId, GameState, RuleFrame},
    object_identity::{CardId, ObjectDestination},
    priority::SeatId,
    program::{
        BodyTarget, ExecutableBody, Instruction, Program, TriggeredAbility, TriggeredAbilityId,
    },
    setup::GameSeed,
};
use spellwake_reference_tools::check_trace::trigger_order_view;
use std::collections::BTreeSet;

#[test]
fn trigger_order_projection_retains_all_actual_frame_and_domain_values() {
    let a = SeatId(0);
    let b = SeatId(1);
    let mut observer = Program::artifact(None, vec![]);
    observer.triggered.push(TriggeredAbility {
        grouping: spellwake_engine::program::TriggerGrouping::EachOccurrence,
        id: TriggeredAbilityId(1),
        condition: spellwake_engine::program::TriggerSpec::dies(
            spellwake_engine::program::ObjectFilter::creature(),
        ),
        body: ExecutableBody::new(
            BodyTarget::None,
            vec![Instruction::DrawController {
                amount: spellwake_engine::program::ValueExpr::Literal(1.into()),
            }],
        ),
    });
    let candidate = GameState::initialize(
        GameId(788),
        GameContext::new(
            "frame-view-tests".into(),
            "cr-20260925".into(),
            "literal-fixtures".into(),
            "ordinary-trigger-view".into(),
            "unrestricted-ffa".into(),
            spellwake_reference_tools::creature_types::synthetic_vocabulary_for_rules(
                "cr-20260925",
            )
            .id(),
            spellwake_contracts::names::NameRules::literal_names().id(),
        )
        .unwrap(),
        GameSeed::with_uniform_life(
            vec![a, b],
            a,
            vec![
                SeedCard {
                    card: CardId(1),
                    owner: a,
                    destination: ObjectDestination::Battlefield { controller: a },
                    program: observer,
                },
                SeedCard {
                    card: CardId(2),
                    owner: a,
                    destination: ObjectDestination::Battlefield { controller: a },
                    program: Program::creature(
                        None,
                        0.into(),
                        0.into(),
                        std::collections::BTreeSet::new(),
                    ),
                },
                SeedCard {
                    card: CardId(3),
                    owner: b,
                    destination: ObjectDestination::Battlefield { controller: b },
                    program: Program::creature(
                        None,
                        0.into(),
                        0.into(),
                        std::collections::BTreeSet::new(),
                    ),
                },
            ],
            vec![CardId(1), CardId(2), CardId(3)],
            vec![],
            20.into(),
            CombatRules::UnrestrictedFreeForAll,
            spellwake_reference_tools::creature_types::synthetic_vocabulary_for_rules(
                "cr-20260925",
            ),
            std::sync::Arc::new(spellwake_contracts::names::NameRules::literal_names()),
        ),
    )
    .unwrap();
    let RuleFrame::Triggers(frame) = candidate.state().pending_decision().unwrap().frame() else {
        panic!("real frame required")
    };
    let value = serde_json::to_value(trigger_order_view(frame)).unwrap();
    assert_eq!(
        value
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect::<BTreeSet<_>>(),
        BTreeSet::from(["revision", "boundary", "event", "wave", "chooser", "domain"])
    );
    assert_eq!(value["revision"], frame.revision().to_string());
    assert_eq!(value["boundary"]["kind"], "priority");
    assert_eq!(
        value["event"]["started_revision"],
        frame.event().started_revision()
    );
    assert_eq!(value["event"]["pass"], frame.event().pass());
    assert_eq!(value["wave"], frame.wave().sequence().to_string());
    assert_eq!(value["chooser"], frame.chooser().0);
    let actual_ids = value["domain"]
        .as_array()
        .unwrap()
        .iter()
        .map(|id| id.as_str().unwrap())
        .collect::<Vec<_>>();
    let input_ids = frame
        .instances()
        .iter()
        .map(|id| id.sequence().to_string())
        .collect::<Vec<_>>();
    assert_eq!(actual_ids, input_ids);
}
