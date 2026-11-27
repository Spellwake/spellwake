//! CR111.2/603.6a/613.7m/704.5j-k through real root transitions.
//! Synthetic literal definitions and prepared scheduled seeds are explicit.
#[path = "../test_support/creature_vocabulary.rs"]
mod creature_vocabulary;
#[path = "../test_support/characteristic_trajectories.rs"]
mod trajectories;
use spellwake_engine::{
    characteristics::{
        Color, ColorOperation, PermanentSelector, StaticAbilityId, StaticColorOperand,
        StaticContinuousAbility, StaticPart, StaticParts,
    },
    combat::CombatRules,
    effects::SeedCard,
    game::{
        DecisionReply, GameCommand, GameContext, GameId, GameInput, GameState, NativeCandidate,
        RuleFrame,
    },
    object_identity::{CardId, ObjectDestination},
    occurrence::{RulesOccurrence, RulesRecord, SupportedAppearance, TriggerRelated},
    priority::{PriorityPhase, SeatId},
    program::{
        BodyTarget, CardType, ExecutableBody, Instruction, Program, Supertype, TriggeredAbility,
        TriggeredAbilityId,
    },
    setup::GameSeed,
    stack_window::WindowPhase,
    token::{TokenCreation, TokenDefinition, TokenName, TokenRecipients},
    turn::{Step, TurnStart},
};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};
use trajectories::{P0, P1, card, object, reply, spell, submit, vocabulary};
fn template(program: Program) -> TokenDefinition {
    TokenDefinition::new(
        program,
        TokenName::Default {
            ordered_subtypes: vec![],
        },
        &vocabulary(),
    )
    .unwrap()
}
fn creature() -> TokenDefinition {
    template(Program::creature(None, 1.into(), 1.into(), BTreeSet::new()))
}
fn group(definition: TokenDefinition, count: i64, recipients: TokenRecipients) -> TokenCreation {
    TokenCreation {
        definition,
        amount: spellwake_engine::program::ValueExpr::Literal(count.into()),
        base_size: None,
        enters_tapped: false,
        recipients,
    }
}
fn create(creations: Vec<TokenCreation>) -> Instruction {
    Instruction::CreateTokens { creations }
}
fn root(
    cards: Vec<SeedCard>,
    participants: Vec<SeatId>,
    active: SeatId,
    world_cohorts: Vec<Vec<CardId>>,
) -> GameState {
    let v = vocabulary();
    let names = Arc::new(spellwake_contracts::names::NameRules::literal_names());
    let context = GameContext::new(
        "simultaneous".into(),
        v.rules_revision().into(),
        "literal".into(),
        "batch".into(),
        "prepared".into(),
        v.id(),
        names.id(),
    )
    .unwrap();
    let order = cards
        .iter()
        .filter(|c| matches!(c.destination, ObjectDestination::Battlefield { .. }))
        .map(|c| c.card)
        .collect();
    let mut seed = GameSeed::with_uniform_life(
        participants,
        active,
        cards,
        order,
        vec![],
        20.into(),
        CombatRules::UnrestrictedFreeForAll,
        v,
        names,
    );
    seed.initial_world_cohorts = world_cohorts;
    GameState::initialize_scheduled(
        GameId(6137),
        context,
        seed,
        TurnStart {
            number: 3,
            step: Step::PrecombatMain,
        },
    )
    .unwrap()
    .into_state()
}
fn start(instructions: Vec<Instruction>, other: Vec<SeedCard>) -> GameState {
    let mut cards = vec![spell(1, BodyTarget::None, instructions)];
    cards.extend(other);
    root(cards, vec![P0, P1], P0, vec![])
}
fn cast(game: &GameState) -> GameState {
    submit(
        game,
        P0,
        GameCommand::CastUntargeted {
            spell: object(game, 1),
            payment: None,
        },
    )
    .into_state()
}
fn resolve(game: &GameState) -> NativeCandidate {
    let mut current = game.clone();
    for index in 0..game.execution().window().participants().len() {
        let WindowPhase::Priority(PriorityPhase::Ready { holder }) =
            current.execution().window().phase()
        else {
            panic!("priority")
        };
        let next = submit(&current, holder, GameCommand::Pass);
        if index + 1 == game.execution().window().participants().len() {
            return next;
        }
        current = next.into_state();
    }
    panic!("participants")
}
fn order(game: &GameState, reverse: bool) -> NativeCandidate {
    let d = game.pending_decision().unwrap();
    let RuleFrame::TimestampOrder(frame) = d.frame() else {
        panic!("timestamp frame")
    };
    let mut values = frame.members().iter().map(|m| m.member).collect::<Vec<_>>();
    if reverse {
        values.reverse();
    }
    reply(game, d.chooser(), DecisionReply::TimestampOrder(values))
}
fn births(candidate: &NativeCandidate) -> Vec<&SupportedAppearance> {
    candidate
        .rules_journal()
        .records
        .iter()
        .filter_map(|r| {
            if let RulesRecord::Event(e) = r {
                Some(e)
            } else {
                None
            }
        })
        .flat_map(|e| &e.occurrences)
        .filter_map(|o| {
            if let RulesOccurrence::TokenCreated { after, .. } = &o.value {
                Some(after.as_ref())
            } else {
                None
            }
        })
        .collect()
}
fn watcher(program: &mut Program) {
    program.triggered.push(TriggeredAbility {
        grouping: spellwake_engine::program::TriggerGrouping::EachOccurrence,
        id: TriggeredAbilityId(7),
        condition: spellwake_engine::program::TriggerSpec::enters(
            spellwake_engine::program::ObjectFilter::creature(),
        ),
        body: ExecutableBody::new(
            BodyTarget::None,
            vec![spellwake_engine::program::Instruction::AdjustLife(
                Box::new(spellwake_engine::toolbox::AdjustLifeSpec::gain(
                    spellwake_engine::toolbox::PlayerSelector::Resolving(
                        spellwake_engine::program::ResolvingPlayer::Controller,
                    ),
                    spellwake_engine::program::ValueExpr::Literal(1.into()),
                )),
            )],
        ),
    });
}
#[test]
fn previews_do_not_allocate_and_all_newcomers_discover_each_other_at_one_cut() {
    let mut p = creature().program().clone();
    watcher(&mut p);
    let game = start(
        vec![create(vec![group(
            template(p),
            2,
            TokenRecipients::Controller,
        )])],
        vec![],
    );
    let cast = cast(&game);
    let next_object = cast.execution().window().cards().next_identity_sequence();
    let pending = resolve(&cast);
    assert!(births(&pending).is_empty());
    assert_eq!(
        pending
            .state()
            .execution()
            .window()
            .cards()
            .next_identity_sequence(),
        next_object
    );
    assert_eq!(
        pending
            .state()
            .execution()
            .window()
            .cards()
            .iter()
            .filter(|o| o.is_token())
            .count(),
        0
    );
    assert_eq!(
        pending
            .state()
            .execution()
            .resolution_state()
            .unwrap()
            .instruction(),
        0
    );
    let committed = order(pending.state(), true);
    let created = births(&committed);
    assert_eq!(created.len(), 2);
    let events = committed
        .rules_journal()
        .records
        .iter()
        .filter_map(|r| {
            if let RulesRecord::Event(e) = r {
                Some(e)
            } else {
                None
            }
        })
        .filter(|e| {
            e.occurrences
                .iter()
                .any(|o| matches!(o.value, RulesOccurrence::TokenCreated { .. }))
        })
        .collect::<Vec<_>>();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].occurrences.len(), 2);
    let ids = created
        .iter()
        .map(|a| a.object.id())
        .collect::<BTreeSet<_>>();
    let pairs = committed
        .rules_journal()
        .records
        .iter()
        .filter_map(|r| {
            if let RulesRecord::TriggerDiscovered(w) = r
                && let TriggerRelated::TokenCreated { after, .. } = &w.causes.members()[0].related
            {
                return Some((w.source.object.id(), after.object.id()));
            }
            None
        })
        .collect::<BTreeSet<_>>();
    assert_eq!(
        pairs,
        ids.iter()
            .flat_map(|a| ids.iter().map(move |b| (*a, *b)))
            .collect()
    );
    let stamps = committed
        .state()
        .execution()
        .characteristic_timestamps()
        .collect::<BTreeMap<_, _>>();
    assert!(stamps[&created[1].object.id()] < stamps[&created[0].object.id()]);
}
#[test]
fn selected_order_changes_independent_color_statics_without_changing_receipt_order() {
    let make = |color| {
        let mut p = Program::artifact(None, vec![]);
        p.continuous = Some(StaticContinuousAbility::Battlefield {
            id: StaticAbilityId(1),
            selector: PermanentSelector {
                required_types: BTreeSet::from([CardType::Creature]),
                ..PermanentSelector::default()
            },
            parts: StaticParts(vec![StaticPart::Color(ColorOperation::Set(
                StaticColorOperand::Literal(BTreeSet::from([color])),
            ))]),
        });
        template(p)
    };
    let game = start(
        vec![create(vec![
            group(make(Color::Red), 1, TokenRecipients::Controller),
            group(make(Color::Blue), 1, TokenRecipients::Controller),
        ])],
        vec![card(
            10,
            Program::creature(None, 2.into(), 2.into(), BTreeSet::new()),
            true,
        )],
    );
    let pending = resolve(&cast(&game));
    let forward = order(pending.state(), false);
    let reverse = order(pending.state(), true);
    let target = object(&game, 10);
    let color = |state: &GameState| {
        state
            .execution()
            .evaluated_characteristics()
            .unwrap()
            .get(target)
            .unwrap()
            .current_colors
            .clone()
    };
    assert_eq!(color(forward.state()), BTreeSet::from([Color::Blue]));
    assert_eq!(color(reverse.state()), BTreeSet::from([Color::Red]));
    assert_eq!(
        births(&forward)
            .iter()
            .map(|a| a.object.id())
            .collect::<Vec<_>>(),
        births(&reverse)
            .iter()
            .map(|a| a.object.id())
            .collect::<Vec<_>>()
    );
}
#[test]
fn world_births_share_one_tenure_and_all_worlds_die_on_a_newest_tie() {
    let mut p = Program::artifact(None, vec![]);
    p.supertypes.insert(Supertype::World);
    let producer = spell(
        1,
        BodyTarget::None,
        vec![create(vec![group(
            template(p.clone()),
            2,
            TokenRecipients::Controller,
        )])],
    );
    let game = root(
        vec![producer, card(10, p, true)],
        vec![P0, P1],
        P0,
        vec![vec![CardId(10)]],
    );
    let pending = resolve(&cast(&game));
    let completed = order(pending.state(), false);
    assert_eq!(births(&completed).len(), 2);
    assert!(
        !completed
            .state()
            .execution()
            .window()
            .cards()
            .iter()
            .any(|o| o.is_token())
    );
    assert_eq!(
        completed
            .state()
            .execution()
            .window()
            .cards()
            .get(object(&game, 10)),
        None
    );
    let gains = completed
        .rules_journal()
        .records
        .iter()
        .filter_map(|r| {
            if let RulesRecord::Event(e) = r {
                Some(e)
            } else {
                None
            }
        })
        .flat_map(|e| &e.occurrences)
        .filter_map(|o| {
            if let RulesOccurrence::TokenCreated { after, .. } = &o.value {
                Some(after.object.id())
            } else {
                None
            }
        })
        .collect::<BTreeSet<_>>();
    assert_eq!(gains.len(), 2);
}

#[test]
fn apnap_orders_rotate_from_active_player_and_wait_until_every_group_is_chosen() {
    let third = SeatId(2);
    let game = root(
        vec![spell(
            1,
            BodyTarget::None,
            vec![create(vec![group(
                creature(),
                2,
                TokenRecipients::EachPlayer,
            )])],
        )],
        vec![P0, P1, third],
        P1,
        vec![],
    );
    let a = submit(&game, P1, GameCommand::Pass).into_state();
    let a = submit(&a, third, GameCommand::Pass).into_state();
    let pending = resolve(&cast(&a));
    let first = pending.state().pending_decision().unwrap().clone();
    assert_eq!(first.chooser(), P1);
    let RuleFrame::TimestampOrder(frame) = first.frame() else {
        panic!()
    };
    assert_eq!(
        frame
            .members()
            .iter()
            .map(|m| m.member.ordinal())
            .collect::<Vec<_>>(),
        vec![2, 3]
    );
    let second = order(pending.state(), true);
    assert_eq!(second.state().pending_decision().unwrap().chooser(), third);
    assert!(births(&second).is_empty());
    assert_eq!(
        second
            .state()
            .execution()
            .window()
            .cards()
            .iter()
            .filter(|o| o.is_token())
            .count(),
        0
    );
    let saved = second.state().clone();
    assert!(
        saved
            .advance(GameInput {
                basis: saved.basis().clone(),
                actor: P1,
                command: GameCommand::Reply {
                    decision: first.id(),
                    choice: DecisionReply::TimestampOrder(
                        frame.members().iter().map(|m| m.member).collect()
                    )
                }
            })
            .is_err()
    );
    assert_eq!(second.state(), &saved);
    let last = order(second.state(), true);
    assert_eq!(last.state().pending_decision().unwrap().chooser(), P0);
    assert!(births(&last).is_empty());
    let completed = order(last.state(), true);
    let born = births(&completed);
    assert_eq!(born.len(), 6);
    assert_eq!(
        born.iter().map(|a| a.object.owner()).collect::<Vec<_>>(),
        vec![P0, P0, P1, P1, third, third]
    );
    let stamps = completed
        .state()
        .execution()
        .characteristic_timestamps()
        .collect::<BTreeMap<_, _>>();
    let actual_order = [3, 2, 5, 4, 1, 0].map(|i| stamps[&born[i].object.id()]);
    assert!(actual_order.windows(2).all(|v| v[0] < v[1]));
}
#[test]
fn each_opponent_is_derived_from_actual_controller_and_skips_its_own_seat() {
    let game = start(
        vec![create(vec![group(
            creature(),
            2,
            TokenRecipients::EachOpponent,
        )])],
        vec![],
    );
    let pending = resolve(&cast(&game));
    assert_eq!(pending.state().pending_decision().unwrap().chooser(), P1);
    let completed = order(pending.state(), false);
    assert_eq!(births(&completed).len(), 2);
    assert!(
        births(&completed)
            .iter()
            .all(|a| a.object.owner() == P1 && a.object.controller() == Some(P1))
    );
}
#[test]
fn zero_toughness_newcomers_are_discovered_before_death_and_captures_survive_cessation() {
    let mut p = Program::creature(None, 0.into(), 0.into(), BTreeSet::new());
    watcher(&mut p);
    let game = start(
        vec![create(vec![group(
            template(p),
            2,
            TokenRecipients::Controller,
        )])],
        vec![],
    );
    let pending = resolve(&cast(&game));
    let completed = order(pending.state(), false);
    assert_eq!(births(&completed).len(), 2);
    assert_eq!(
        completed
            .state()
            .execution()
            .window()
            .cards()
            .iter()
            .filter(|o| o.is_token())
            .count(),
        0
    );
    let records = &completed.rules_journal().records;
    let discovered = records
        .iter()
        .enumerate()
        .filter_map(|(i, r)| {
            if let RulesRecord::TriggerDiscovered(w) = r {
                Some((i, w))
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    assert_eq!(discovered.len(), 4);
    let departure=records.iter().position(|r|matches!(r,RulesRecord::Event(e) if e.occurrences.iter().any(|o|matches!(&o.value,RulesOccurrence::ZoneChange(z) if z.before.object.is_token())))).unwrap();
    assert!(discovered.iter().all(|(index, w)| *index < departure
        && w.source.object.is_token()
        && w.body.instructions.len() == 1));
    assert!(records.iter().skip(departure+1).any(|r|matches!(r,RulesRecord::Event(e) if e.occurrences.iter().any(|o|matches!(&o.value,RulesOccurrence::TokenCeased{..})))));
    assert!(matches!(
        completed.state().pending_decision().unwrap().frame(),
        RuleFrame::Triggers(_)
    ));
}

#[test]
fn oversized_materialization_rejects_without_a_rule_cap_or_partial_birth() {
    for amount in [
        "999999999999999999999999999999999999999999999999"
            .to_owned()
            .try_into()
            .unwrap(),
        spellwake_contracts::RulesInteger::from_usize(usize::MAX - 2),
    ] {
        let game = start(
            vec![create(vec![TokenCreation {
                definition: creature(),
                amount: spellwake_engine::program::ValueExpr::Literal(amount),
                base_size: None,
                enters_tapped: false,
                recipients: TokenRecipients::Controller,
            }])],
            vec![],
        );
        let cast = cast(&game);
        let first = submit(&cast, P0, GameCommand::Pass).into_state();
        let saved = first.clone();
        assert!(
            first
                .advance(GameInput {
                    basis: first.basis().clone(),
                    actor: P1,
                    command: GameCommand::Pass
                })
                .is_err()
        );
        assert_eq!(first, saved);
    }
}

#[test]
fn owned_copied_batch_creates_for_copy_controller_after_the_original_is_countered() {
    use spellwake_engine::targets::TargetRef;
    let mut copier = spell(
        2,
        BodyTarget::SingleSpell(spellwake_engine::program::SpellFilter::any()),
        vec![Instruction::CopySpell],
    );
    copier.owner = P1;
    let counter = spell(
        3,
        BodyTarget::SingleSpell(spellwake_engine::program::SpellFilter::any()),
        vec![Instruction::CounterSpell],
    );
    let game = start(
        vec![create(vec![group(
            creature(),
            2,
            TokenRecipients::Controller,
        )])],
        vec![copier, counter],
    );
    let original_cast = cast(&game);
    let original = original_cast.execution().window().entries()[0].id();
    let passed = submit(&original_cast, P0, GameCommand::Pass);
    let copy_cast = submit(
        passed.state(),
        P1,
        GameCommand::Cast {
            spell: object(passed.state(), 2),
            target: TargetRef::Stack(original),
        },
    );
    let copied = resolve(copy_cast.state());
    let counter_cast = submit(
        copied.state(),
        P0,
        GameCommand::Cast {
            spell: object(copied.state(), 3),
            target: TargetRef::Stack(original),
        },
    );
    let countered = resolve(counter_cast.state());
    assert!(
        !countered
            .state()
            .execution()
            .window()
            .entries()
            .iter()
            .any(|e| e.id() == original)
    );
    let pending = resolve(countered.state());
    let RuleFrame::TimestampOrder(frame) = pending.state().pending_decision().unwrap().frame()
    else {
        panic!()
    };
    assert_eq!(frame.chooser(), P1);
    assert!(frame.members().iter().all(|m| m.creator == P1));
    let completed = order(pending.state(), false);
    assert_eq!(births(&completed).len(), 2);
    assert!(
        births(&completed)
            .iter()
            .all(|a| a.object.owner() == P1 && a.object.controller() == Some(P1))
    );
}

#[test]
fn zero_groups_do_not_leave_member_holes_or_create_world_history_in_a_nonempty_batch() {
    let mut zero_world = Program::artifact(None, vec![]);
    zero_world.supertypes.insert(Supertype::World);
    let game = start(
        vec![create(vec![
            group(template(zero_world.clone()), 0, TokenRecipients::EachPlayer),
            group(creature(), 2, TokenRecipients::Controller),
            group(template(zero_world), 0, TokenRecipients::EachOpponent),
        ])],
        vec![],
    );
    let cast = cast(&game);
    let before = cast.execution().operational_counters();
    let pending = resolve(&cast);
    let RuleFrame::TimestampOrder(frame) = pending.state().pending_decision().unwrap().frame()
    else {
        panic!()
    };
    assert_eq!(
        frame
            .members()
            .iter()
            .map(|m| m.member.ordinal())
            .collect::<Vec<_>>(),
        vec![0, 1]
    );
    let completed = order(pending.state(), true);
    let born = births(&completed);
    assert_eq!(born.len(), 2);
    assert!(born.iter().all(|a| a.object.owner() == P0
        && a.types.contains(&CardType::Creature)
        && !a.current_supertypes.contains(&Supertype::World)));
    let after = completed.state().execution().operational_counters();
    assert_eq!(
        after.next_characteristic_timestamp,
        before.next_characteristic_timestamp + 2
    );
    assert_eq!(after.next_world_tenure, before.next_world_tenure);
}

#[path = "support/trigger_grouping_cases.rs"]
mod trigger_grouping_cases;
