#![allow(dead_code, unused_imports)]
//! Damage keyword interactions through GameState's scheduled input loop.
use super::*;
use KeywordAbility::{Deathtouch, DoubleStrike, FirstStrike, Indestructible, Trample};
use spellwake_engine::{
    activated::{ActivatedAbility, ActivatedAbilityId, SourceCost},
    characteristics::{
        AbilityChange, AbilityRemoval, KeywordAbility, ResolvedDuration, ResolvedPartSpec,
        ResolvedPartsSpec,
    },
    mana::ManaPayment,
};

pub(super) fn combat_creature(
    id: u64,
    owner: SeatId,
    power: i64,
    toughness: i64,
    keywords: &[KeywordAbility],
) -> SeedCard {
    let mut card = battlefield(id, owner);
    card.program = Program::creature(None, power.into(), toughness.into(), Default::default())
        .with_printed_keywords(keywords.iter().copied());
    card
}
pub(super) fn cast_on(game: &GameState, spell: u64, target: u64) -> GameState {
    let cast = act(
        game,
        A,
        GameCommand::Cast {
            spell: object(game, spell).id(),
            target: TargetRef::Object(object(game, target).id()),
        },
    );
    round(cast.state()).into_state()
}
pub(super) fn blocks(seeds: Vec<SeedCard>, blockers: &[(u64, u64)]) -> GameState {
    let ready = attackers(setup(vec![A, B], seeds, Step::Untap).state());
    let attacking = declared(&ready, &[(1, B)]);
    let blocking = round(attacking.state());
    if blocking.state().pending_decision().is_none() {
        assert!(blockers.is_empty());
        return blocking.into_state();
    }
    blocked(blocking.state(), blockers).into_state()
}

#[test]
fn trample_respects_all_blockers_marked_damage_and_deathtouch_lethal() {
    for (keywords, floor, player_damage) in
        [(vec![Trample], 3, 2), (vec![Trample, Deathtouch], 1, 6)]
    {
        let blocked = blocks(
            vec![
                combat_creature(1, A, 8, 10, &keywords),
                combat_creature(2, B, 1, 4, &[]),
                combat_creature(3, B, 1, 3, &[Indestructible]),
                instant(4, A, Instruction::Damage { amount: 1.into() }),
            ],
            &[(2, 1), (3, 1)],
        );
        let marked = cast_on(&blocked, 4, 2);
        let assigning = round(&marked).into_state();
        let RuleFrame::Combat(CombatDecision::DamageAssignments(frame)) =
            assigning.pending_decision().unwrap().frame()
        else {
            panic!("attacker assigns trample")
        };
        let source = object(&assigning, 1).id();
        let blockers = [object(&assigning, 2).id(), object(&assigning, 3).id()];
        assert_eq!(
            frame.domains()[0]
                .trample_lethal()
                .values()
                .cloned()
                .collect::<Vec<_>>(),
            vec![floor.into(), floor.into()]
        );
        let allocate = |first: i64, second: i64, player: i64| {
            [
                (DamageRecipient::Creature(blockers[0]), first),
                (DamageRecipient::Creature(blockers[1]), second),
                (DamageRecipient::Player(B), player),
            ]
            .into_iter()
            .filter(|(_, n)| *n > 0)
            .map(|(target, n)| CombatDamageAssignment {
                source,
                target,
                amount: n.into(),
            })
            .collect()
        };
        reject(
            &assigning,
            input(
                &assigning,
                A,
                GameCommand::Reply {
                    decision: assigning.pending_decision().unwrap().id(),
                    choice: DecisionReply::AssignCombatDamage(allocate(
                        floor - 1,
                        floor,
                        player_damage + 1,
                    )),
                },
            ),
            GameError::Effect(EffectError::Combat(
                CombatError::InsufficientTrampleAssignment,
            )),
        );
        let result = reply(
            &assigning,
            DecisionReply::AssignCombatDamage(allocate(floor, floor, player_damage)),
        )
        .into_state();
        assert_eq!(
            result.execution().life_total(B),
            Some(&(20 - player_damage).into())
        );
        assert_eq!(object(&result, 2).zone(), ObjectZone::Graveyard(B));
        assert_eq!(object(&result, 3).zone(), ObjectZone::Battlefield);
        // Overassigning everything to one blocker is legal if no damage goes to the player.
        let all_on_blocker = reply(
            &assigning,
            DecisionReply::AssignCombatDamage(allocate(8, 0, 0)),
        );
        assert_eq!(
            all_on_blocker.state().execution().life_total(B),
            Some(&20.into())
        );
    }
}

#[test]
fn double_strike_trample_replies_against_current_marks_on_both_damage_steps() {
    let blocked = blocks(
        vec![
            combat_creature(1, A, 5, 10, &[Trample, DoubleStrike]),
            combat_creature(2, B, 1, 3, &[Indestructible]),
        ],
        &[(2, 1)],
    );
    let mut game = round(&blocked).into_state();
    let source = object(&game, 1).id();
    let blocker = object(&game, 2).id();
    for (additional, minimum) in [(false, 3), (true, 0)] {
        let RuleFrame::Combat(CombatDecision::DamageAssignments(frame)) =
            game.pending_decision().unwrap().frame()
        else {
            panic!("trample damage split")
        };
        assert_eq!(frame.position().additional_damage, additional);
        assert_eq!(
            frame.domains()[0].trample_lethal().get(&blocker),
            Some(&minimum.into())
        );
        let mut rows = vec![CombatDamageAssignment {
            source,
            target: DamageRecipient::Player(B),
            amount: (5 - minimum).into(),
        }];
        if minimum > 0 {
            rows.push(CombatDamageAssignment {
                source,
                target: DamageRecipient::Creature(blocker),
                amount: minimum.into(),
            });
        }
        let dealt = reply(&game, DecisionReply::AssignCombatDamage(rows));
        if additional {
            assert_eq!(dealt.state().execution().life_total(B), Some(&13.into()));
        }
        game = round(dealt.state()).into_state();
    }
    assert_eq!(game.turn().unwrap().position().step, Step::EndCombat);
}

#[test]
fn copied_deathtouch_spell_retargets_and_applies_its_own_keywords() {
    let mut venom = instant(3, A, Instruction::Damage { amount: 1.into() });
    venom.program = venom.program.with_printed_keywords([Deathtouch]);
    let copy = SeedCard {
        card: CardId(4),
        owner: A,
        destination: ObjectDestination::Hand,
        program: Program::instant(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::SingleSpell(spellwake_engine::program::SpellFilter::any()),
                vec![Instruction::CopySpell],
            ),
        ),
    };
    let game = setup(
        vec![A, B],
        vec![battlefield(1, B), battlefield(2, B), venom, copy],
        Step::PrecombatMain,
    )
    .into_state();
    let original = act(
        &game,
        A,
        GameCommand::Cast {
            spell: object(&game, 3).id(),
            target: TargetRef::Object(object(&game, 1).id()),
        },
    );
    let original_entry = original.state().execution().window().entries()[0].id();
    let copy = act(
        original.state(),
        A,
        GameCommand::Cast {
            spell: object(&game, 4).id(),
            target: TargetRef::Stack(original_entry),
        },
    );
    let choosing = round(copy.state());
    let copying = reply(
        choosing.state(),
        DecisionReply::CopyTargets(spellwake_engine::effects::CopyTargetChoice::Change(
            TargetRef::Object(object(&game, 2).id()),
        )),
    );
    let copy_done = round(copying.state());
    assert_eq!(
        object(copy_done.state(), 2).zone(),
        ObjectZone::Graveyard(B)
    );
    assert_eq!(object(copy_done.state(), 1).zone(), ObjectZone::Battlefield);
    let both_done = round(copy_done.state());
    assert_eq!(
        object(both_done.state(), 1).zone(),
        ObjectZone::Graveyard(B)
    );
}

fn life_events(result: &NativeCandidate) -> Vec<&spellwake_engine::life::LifeChange> {
    use spellwake_engine::occurrence::{RulesOccurrence, RulesRecord};
    result
        .rules_journal()
        .records
        .iter()
        .filter_map(|record| match record {
            RulesRecord::Event(event) => Some(&event.occurrences),
            _ => None,
        })
        .flatten()
        .filter_map(|row| match &row.value {
            RulesOccurrence::LifeChanged(change) => Some(change),
            _ => None,
        })
        .collect()
}

#[test]
fn lifelink_groups_each_source_across_recipients_and_discovers_one_gain_trigger_per_source() {
    use KeywordAbility::Lifelink;
    use spellwake_engine::{
        life::LifeChangeKind,
        occurrence::RulesRecord,
        program::{PlayerRelation, TriggerGrouping, TriggeredAbility, TriggeredAbilityId},
    };
    let mut witness = battlefield(5, A);
    witness.program = Program::artifact(None, vec![]);
    witness.program.triggered.push(TriggeredAbility {
        id: TriggeredAbilityId(3),
        grouping: TriggerGrouping::EachOccurrence,
        condition: spellwake_engine::program::TriggerSpec::life_changed(
            LifeChangeKind::Gain,
            PlayerRelation::Controller,
        ),
        body: ExecutableBody::new(
            BodyTarget::None,
            vec![Instruction::DrawController {
                amount: spellwake_engine::program::ValueExpr::Literal(0.into()),
            }],
        ),
    });
    let seeded = setup(
        vec![A, B],
        vec![
            combat_creature(1, A, 6, 8, &[Lifelink, Trample, Deathtouch]),
            combat_creature(2, A, 3, 6, &[Lifelink]),
            combat_creature(3, B, 1, 5, &[]),
            instant(
                4,
                A,
                Instruction::CreatePreventionShield { amount: 2.into() },
            ),
            witness,
        ],
        Step::Untap,
    )
    .into_state();
    let game = attackers(&cast_on(&seeded, 4, 3));
    let attacking = declared(&game, &[(1, B), (2, B)]);
    let blocking = round(attacking.state());
    let blocked = blocked(blocking.state(), &[(3, 1)]);
    let allocating = round(blocked.state());
    let game = allocating.state();
    let one = object(game, 1).id();
    let two = object(game, 2).id();
    let blocker = object(game, 3).id();
    let result = reply(
        game,
        DecisionReply::AssignCombatDamage(vec![
            CombatDamageAssignment {
                source: one,
                target: DamageRecipient::Creature(blocker),
                amount: 3.into(),
            },
            CombatDamageAssignment {
                source: one,
                target: DamageRecipient::Player(B),
                amount: 3.into(),
            },
            CombatDamageAssignment {
                source: two,
                target: DamageRecipient::Player(B),
                amount: 3.into(),
            },
        ]),
    );
    assert_eq!(result.state().execution().life_total(A), Some(&27.into()));
    assert_eq!(result.state().execution().life_total(B), Some(&14.into()));
    assert_eq!(object(result.state(), 3).zone(), ObjectZone::Graveyard(B));
    let gains: Vec<_> = life_events(&result)
        .into_iter()
        .filter(|event| event.kind == LifeChangeKind::Gain)
        .collect();
    assert_eq!(gains.len(), 2);
    for (event, source, amount) in [(gains[0], one, 4), (gains[1], two, 3)] {
        let origin = event.lifelink.as_ref().unwrap();
        assert_eq!(origin.source, DamageSourceId::Object(source));
        let members: Vec<_> = result
            .damage_journal()
            .applied
            .iter()
            .filter(|fact| fact.source.id() == origin.source && fact.batch == origin.batch)
            .map(|fact| fact.attempt)
            .collect();
        assert_eq!(origin.attempts, members);
        assert_eq!(event.amount, amount.into());
        assert_eq!(
            (event.before.clone(), event.after.clone()),
            (20.into(), 27.into())
        );
    }
    assert_eq!(
        result
            .rules_journal()
            .records
            .iter()
            .filter(|row| matches!(row, RulesRecord::TriggerDiscovered(_)))
            .count(),
        2
    );
}

#[test]
fn lifelink_simultaneous_self_damage_keeps_both_events_at_the_same_total() {
    use spellwake_engine::life::LifeChangeKind;
    let spell = SeedCard {
        card: CardId(1),
        owner: A,
        destination: ObjectDestination::Hand,
        program: Program::instant(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::SinglePlayer,
                vec![Instruction::Damage { amount: 25.into() }],
            ),
        )
        .with_printed_keywords([KeywordAbility::Lifelink]),
    };
    let game = setup(vec![A, B], vec![spell], Step::PrecombatMain).into_state();
    let cast = act(
        &game,
        A,
        GameCommand::Cast {
            spell: object(&game, 1).id(),
            target: TargetRef::Player(A),
        },
    );
    let resolved = round(cast.state());
    assert!(resolved.state().outcome().is_none());
    assert_eq!(resolved.state().execution().life_total(A), Some(&20.into()));
    let events = life_events(&resolved);
    assert_eq!(events.len(), 2);
    assert_eq!(
        (events[0].kind, events[1].kind),
        (LifeChangeKind::Gain, LifeChangeKind::Loss)
    );
    for change in events {
        assert_eq!(change.amount, 25.into());
        assert_eq!(change.before, 20.into());
        assert_eq!(change.after, 20.into());
    }
}

#[test]
fn trample_with_no_excess_assigns_automatically_to_its_only_blocker() {
    for power in [1, 3] {
        let blocked = blocks(
            vec![
                combat_creature(1, A, power, 5, &[Trample]),
                combat_creature(2, B, 1, 3, &[Indestructible]),
            ],
            &[(2, 1)],
        );
        let damage = round(&blocked);
        assert!(damage.state().pending_decision().is_none());
        assert_eq!(damage.state().execution().life_total(B), Some(&20.into()));
        assert_eq!(
            damage
                .state()
                .execution()
                .battlefield_marked_damage(object(damage.state(), 2).id()),
            Some(power.into())
        );
    }
}

pub(super) fn change(ability: AbilityChange) -> Instruction {
    Instruction::ApplyContinuous(Box::new(spellwake_engine::toolbox::ApplyContinuousSpec {
        selector: Some(spellwake_engine::toolbox::ObjectQuery::Targets),
        payload: spellwake_engine::toolbox::ContinuousPayload::ResolvedParts(Box::new(
            ResolvedPartsSpec(vec![ResolvedPartSpec::Abilities(ability)]),
        )),
        duration: ResolvedDuration::UntilEndOfTurn,
        condition: None,
        one_shot: false,
        as_emblem: None,
        emblem_label: None,
    }))
}
