use super::*;
use spellwake_engine::program::{
    CombatTriggerKeyword as Keyword, TriggeredAbility, TriggeredAbilityId,
};

pub(super) fn add(seeds: &mut [SeedCard], card: u64, keywords: &[Keyword]) {
    let program = &mut seeds
        .iter_mut()
        .find(|seed| seed.card == CardId(card))
        .unwrap()
        .program;
    for (i, k) in keywords.iter().enumerate() {
        program.triggered.push(TriggeredAbility::combat_keyword(
            TriggeredAbilityId(i as u64),
            k.clone(),
        ));
    }
}

pub(super) fn place(mut game: GameState) -> GameState {
    while let Some(decision) = game.pending_decision() {
        let RuleFrame::Triggers(order) = decision.frame() else {
            break;
        };
        game = reply(
            &game,
            DecisionReply::OrderTriggers(order.instances().to_vec()),
        )
        .into_state();
    }
    game
}

fn power(game: &GameState, id: u64) -> i64 {
    let value = game
        .execution()
        .creature(object(game, id).id())
        .unwrap()
        .unwrap()
        .power;
    i64::try_from(value.to_usize().unwrap()).unwrap()
}

fn block_step(seeds: Vec<SeedCard>, attackers_selected: &[(u64, SeatId)]) -> GameState {
    let ready = attackers(setup(vec![A, B], seeds, Step::Untap).state());
    let attack = declared(&ready, attackers_selected);
    round(attack.state()).into_state()
}

#[test]
fn prowess_triggers_for_each_instance_only_on_its_controllers_noncreature_cast() {
    let mut seeds = cards();
    add(&mut seeds, 1, &[Keyword::Prowess, Keyword::Prowess]);
    let game = setup(vec![A, B], seeds, Step::PrecombatMain).into_state();
    let creature = act(
        &game,
        A,
        GameCommand::CastUntargeted {
            spell: object(&game, 40).id(),
            payment: None,
        },
    )
    .into_state();
    assert_eq!(creature.execution().window().entries().len(), 1);
    let game = round(&creature).into_state();
    let b = act(&game, A, GameCommand::Pass).into_state();
    let theirs = act(
        &b,
        B,
        GameCommand::Cast {
            spell: object(&b, 31).id(),
            target: TargetRef::Object(object(&b, 3).id()),
        },
    )
    .into_state();
    assert_eq!(theirs.execution().window().entries().len(), 1);
    let game = round(&theirs).into_state();
    let ours = act(
        &game,
        A,
        GameCommand::Cast {
            spell: object(&game, 30).id(),
            target: TargetRef::Object(object(&game, 4).id()),
        },
    )
    .into_state();
    let mut state = place(ours);
    assert_eq!(state.execution().window().entries().len(), 3);
    for expected in [3, 4] {
        state = round(&state).into_state();
        assert_eq!(power(&state, 1), expected);
    }
    assert_eq!(state.execution().window().entries().len(), 1);
}

#[test]
fn battle_cry_affects_only_other_current_attackers_and_survives_source_departure() {
    let mut seeds = cards();
    add(&mut seeds, 1, &[Keyword::BattleCry]);
    seeds.push(battlefield(5, A));
    let ready = attackers(setup(vec![A, B], seeds, Step::Untap).state());
    let attack = declared(&ready, &[(1, B), (2, B)]).into_state();
    assert_eq!(attack.execution().window().entries().len(), 1);
    assert!(
        attack.execution().window().entries()[0]
            .targets()
            .targets()
            .is_empty()
    );
    let response = act(
        &attack,
        A,
        GameCommand::Cast {
            spell: object(&attack, 30).id(),
            target: TargetRef::Object(object(&attack, 1).id()),
        },
    )
    .into_state();
    let bounced = round(&response).into_state();
    let pumped = round(&bounced).into_state();
    assert_eq!(power(&pumped, 2), 3);
    assert_eq!(power(&pumped, 5), 2);
    assert_eq!(power(&pumped, 3), 2);
}

#[test]
fn bushido_triggers_once_when_double_blocked_and_independently_for_the_blocker() {
    let mut seeds = cards();
    add(&mut seeds, 1, &[Keyword::Bushido(2.into())]);
    add(&mut seeds, 3, &[Keyword::Bushido(1.into())]);
    let game = block_step(seeds, &[(1, B)]);
    let state = place(blocked(&game, &[(3, 1), (4, 1)]).into_state());
    assert_eq!(state.execution().window().entries().len(), 2);
    let first = round(&state).into_state();
    assert_eq!(power(&first, 3), 3);
    let second = round(&first).into_state();
    assert_eq!(power(&second, 1), 4);
    assert_eq!(power(&second, 4), 2);
}

#[test]
fn flanking_matches_each_nonflanking_blocker_for_each_instance_and_ignores_shroud() {
    let mut seeds = cards();
    add(&mut seeds, 1, &[Keyword::Flanking, Keyword::Flanking]);
    add(&mut seeds, 4, &[Keyword::Flanking]);
    seeds.push(battlefield(5, B));
    seeds
        .iter_mut()
        .find(|seed| seed.card == CardId(3))
        .unwrap()
        .program
        .printed_keywords
        .insert(spellwake_engine::characteristics::KeywordAbility::Shroud);
    let game = block_step(seeds, &[(1, B)]);
    let mut state = place(blocked(&game, &[(3, 1), (4, 1), (5, 1)]).into_state());
    assert_eq!(state.execution().window().entries().len(), 4);
    assert!(
        state
            .execution()
            .window()
            .entries()
            .iter()
            .all(|e| e.targets().targets().is_empty())
    );
    for _ in 0..4 {
        state = round(&state).into_state();
    }
    assert_eq!(
        (power(&state, 3), power(&state, 4), power(&state, 5)),
        (0, 2, 0)
    );
}

#[test]
fn rampage_counts_current_blockers_at_resolution_and_keeps_the_result() {
    let mut seeds = cards();
    seeds.push(battlefield(5, B));
    add(&mut seeds, 1, &[Keyword::Rampage(3.into())]);
    let game = block_step(seeds, &[(1, B)]);
    let blocks = blocked(&game, &[(3, 1), (4, 1), (5, 1)]).into_state();
    assert_eq!(blocks.execution().window().entries().len(), 1);
    let bounce = act(
        &blocks,
        A,
        GameCommand::Cast {
            spell: object(&blocks, 30).id(),
            target: TargetRef::Object(object(&blocks, 3).id()),
        },
    )
    .into_state();
    let updated = round(&bounce).into_state();
    let boosted = round(&updated).into_state();
    assert_eq!(power(&boosted, 1), 5);
    let other = act(&boosted, A, GameCommand::Pass).into_state();
    let again = act(
        &other,
        B,
        GameCommand::Cast {
            spell: object(&other, 31).id(),
            target: TargetRef::Object(object(&other, 4).id()),
        },
    )
    .into_state();
    assert_eq!(power(round(&again).state(), 1), 5);
}

#[test]
fn frenzy_waits_until_blockers_are_declared_and_only_pumps_unblocked_attackers() {
    let mut seeds = cards();
    add(&mut seeds, 1, &[Keyword::Frenzy(2.into())]);
    add(&mut seeds, 2, &[Keyword::Frenzy(3.into())]);
    let game = block_step(seeds, &[(1, B), (2, B)]);
    assert!(game.execution().window().entries().is_empty());
    let blocks = blocked(&game, &[(3, 1)]).into_state();
    assert_eq!(blocks.execution().window().entries().len(), 1);
    let resolved = round(&blocks).into_state();
    assert_eq!((power(&resolved, 1), power(&resolved, 2)), (2, 5));
    let sheet = resolved
        .execution()
        .creature(object(&resolved, 2).id())
        .unwrap()
        .unwrap();
    assert_eq!(sheet.toughness, 4.into());
}

#[test]
fn two_sliver_grants_stack_and_disappear_when_their_sources_leave() {
    use spellwake_engine::characteristics::{
        AbilityChange, PermanentSelector, StaticAbilityId, StaticContinuousAbility, StaticPart,
        StaticParts,
    };
    use spellwake_engine::program::CardType;
    let mut seeds = cards();
    // Every creature in this prepared game receives the two independent grants, including opponents.
    for card in [1, 3] {
        let seed = seeds
            .iter_mut()
            .find(|seed| seed.card == CardId(card))
            .unwrap();
        seed.program.continuous = Some(StaticContinuousAbility::Battlefield {
            id: StaticAbilityId(0),
            selector: PermanentSelector {
                required_types: [CardType::Creature].into(),
                ..Default::default()
            },
            parts: StaticParts(vec![StaticPart::Abilities(AbilityChange {
                granted_combat_keywords: vec![Keyword::Frenzy(1.into()), Keyword::Flanking],
                ..Default::default()
            })]),
        });
    }
    let game = block_step(seeds, &[(2, B)]);
    let mut triggered = place(blocked(&game, &[]).into_state());
    assert_eq!(triggered.execution().window().entries().len(), 2);
    let bounce = act(
        &triggered,
        A,
        GameCommand::Cast {
            spell: object(&triggered, 30).id(),
            target: TargetRef::Object(object(&triggered, 1).id()),
        },
    )
    .into_state();
    triggered = round(&bounce).into_state();
    let current = triggered
        .execution()
        .current_characteristics(object(&triggered, 4).id())
        .unwrap()
        .unwrap();
    assert_eq!(current.combat_trigger_keywords().count(), 2);
    for _ in 0..2 {
        triggered = round(&triggered).into_state();
    }
    assert_eq!(power(&triggered, 2), 4);
}
