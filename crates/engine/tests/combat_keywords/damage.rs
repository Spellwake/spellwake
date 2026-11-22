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
pub(super) fn change(ability: AbilityChange) -> Instruction {
    Instruction::CreateContinuousEffect {
        parts: ResolvedPartsSpec(vec![ResolvedPartSpec::Abilities(ability)]),
        duration: ResolvedDuration::UntilEndOfTurn,
    }
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
fn departed_last_blocker_allows_only_trample_to_hit_the_defender() {
    for (keys, life) in [(vec![Trample], 16), (vec![], 20)] {
        let blocked = blocks(
            vec![
                combat_creature(1, A, 4, 4, &keys),
                battlefield(2, B),
                instant(3, A, Instruction::ReturnToOwnersHand),
            ],
            &[(2, 1)],
        );
        let cleared = cast_on(&blocked, 3, 2);
        let damage = round(&cleared);
        assert_eq!(damage.state().execution().life_total(B), Some(&life.into()));
    }
}

#[test]
fn strike_steps_use_original_cohort_and_current_double_strike() {
    for (initial, modification, hits) in [
        (FirstStrike, AbilityChange::remove(AbilityRemoval::All), 1),
        (
            FirstStrike,
            AbilityChange::grant_keywords([DoubleStrike]),
            2,
        ),
        (DoubleStrike, AbilityChange::remove(AbilityRemoval::All), 1),
        (
            DoubleStrike,
            AbilityChange::grant_keywords([FirstStrike]),
            2,
        ),
    ] {
        let blocked = blocks(
            vec![
                combat_creature(1, A, 3, 5, &[initial]),
                battlefield(2, B),
                instant(3, A, change(modification)),
            ],
            &[],
        );
        let early = round(&blocked).into_state();
        assert_eq!(early.execution().life_total(B), Some(&17.into()));
        assert!(!early.turn().unwrap().position().additional_damage);
        let modified = cast_on(&early, 3, 1);
        let late = round(&modified).into_state();
        assert!(late.turn().unwrap().position().additional_damage);
        assert_eq!(
            late.execution().life_total(B),
            Some(&(20 - 3 * hits).into())
        );
        assert_eq!(
            round(&late).state().turn().unwrap().position().step,
            Step::EndCombat
        );
    }
}

#[test]
fn first_strike_blocker_kills_before_regular_damage_and_zero_power_still_adds_a_step() {
    for power in [0, 3] {
        let blocked = blocks(
            vec![
                combat_creature(1, A, 3, 3, &[]),
                combat_creature(2, B, power, 3, &[FirstStrike]),
                instant(3, A, change(AbilityChange::grant_keywords([FirstStrike]))),
            ],
            &[(2, 1)],
        );
        let early = round(&blocked).into_state();
        let late = if power == 0 {
            assert_eq!(object(&early, 1).zone(), ObjectZone::Battlefield);
            round(&cast_on(&early, 3, 1))
        } else {
            assert_eq!(object(&early, 1).zone(), ObjectZone::Graveyard(A));
            round(&early)
        };
        assert!(late.state().turn().unwrap().position().additional_damage);
        assert_eq!(
            object(late.state(), 2).zone(),
            if power == 0 {
                ObjectZone::Graveyard(B)
            } else {
                ObjectZone::Battlefield
            }
        );
    }
}

#[test]
fn deathtouch_uses_departed_sources_last_keywords_and_only_actual_positive_damage() {
    for (grant, prevented, dies) in [
        (true, false, true),
        (false, false, false),
        (true, true, false),
    ] {
        let mut source = battlefield(1, A);
        source.program = Program::artifact(
            None,
            vec![ActivatedAbility {
                id: ActivatedAbilityId(1),
                mana_cost: ManaCost::free(),
                source_cost: SourceCost::Sacrifice,
                body: ExecutableBody::new(
                    BodyTarget::SingleCreature,
                    vec![Instruction::Damage { amount: 1.into() }],
                ),
            }],
        );
        // Keyword removal on this permanent would also remove its activation.
        // A granted keyword tests the evaluated value at departure.
        let mut seeds = vec![
            source,
            combat_creature(2, B, 1, 6, &[]),
            instant(
                4,
                A,
                Instruction::CreatePreventionShield { amount: 1.into() },
            ),
        ];
        seeds.push(SeedCard {
            card: CardId(3),
            owner: A,
            destination: ObjectDestination::Hand,
            program: Program::instant(
                ManaCost::free(),
                ExecutableBody::new(
                    BodyTarget::SinglePermanent,
                    vec![change(AbilityChange::grant_keywords([Deathtouch]))],
                ),
            ),
        });
        let mut game = setup(vec![A, B], seeds, Step::PrecombatMain).into_state();
        if grant {
            game = cast_on(&game, 3, 1);
        }
        if prevented {
            game = cast_on(&game, 4, 2);
        }
        let activated = act(
            &game,
            A,
            GameCommand::ActivateAbility {
                source: object(&game, 1).id(),
                ability: ActivatedAbilityId(1),
                target: TargetRef::Object(object(&game, 2).id()),
                payment: Some(ManaPayment::default()),
            },
        );
        assert_eq!(
            object(activated.state(), 1).zone(),
            ObjectZone::Graveyard(A)
        );
        let resolved = round(activated.state());
        assert_eq!(
            object(resolved.state(), 2).zone(),
            if dies {
                ObjectZone::Graveyard(B)
            } else {
                ObjectZone::Battlefield
            }
        );
    }
}

#[test]
fn indestructible_ignores_destroy_and_consumes_deathtouch_at_the_next_check() {
    for (amount, dies_after_removal) in [(1, false), (4, true)] {
        let mut venom = instant(
            3,
            A,
            Instruction::Damage {
                amount: amount.into(),
            },
        );
        venom.program = venom.program.with_printed_keywords([Deathtouch]);
        let game = setup(
            vec![A, B],
            vec![
                combat_creature(1, B, 1, 4, &[Indestructible]),
                combat_creature(2, B, 1, 4, &[]),
                venom,
                instant(4, A, Instruction::DestroyTarget),
                instant(5, A, change(AbilityChange::remove(AbilityRemoval::All))),
            ],
            Step::PrecombatMain,
        )
        .into_state();
        let normal = cast_on(&game, 4, 2);
        assert_eq!(object(&normal, 2).zone(), ObjectZone::Graveyard(B));
        let protected = cast_on(&game, 4, 1);
        let marked = cast_on(&protected, 3, 1);
        assert_eq!(object(&marked, 1).zone(), ObjectZone::Battlefield);
        let vulnerable = cast_on(&marked, 5, 1);
        assert_eq!(
            object(&vulnerable, 1).zone(),
            if dies_after_removal {
                ObjectZone::Graveyard(B)
            } else {
                ObjectZone::Battlefield
            }
        );
    }
}

#[test]
fn zero_toughness_moves_even_an_indestructible_creature() {
    let effect = Instruction::CreateContinuousEffect {
        parts: ResolvedPartsSpec(vec![ResolvedPartSpec::SetPT(
            spellwake_engine::characteristics::PowerToughness::zero(),
        )]),
        duration: ResolvedDuration::UntilEndOfTurn,
    };
    let game = setup(
        vec![A, B],
        vec![
            combat_creature(1, B, 1, 3, &[Indestructible]),
            instant(2, A, effect),
        ],
        Step::PrecombatMain,
    )
    .into_state();
    let result = cast_on(&game, 2, 1);
    assert_eq!(object(&result, 1).zone(), ObjectZone::Graveyard(B));
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
fn lifelink_uses_the_sources_controller_at_departure_after_control_changes() {
    use spellwake_engine::characteristics::ControlRecipient;
    let source = SeedCard {
        card: CardId(1),
        owner: B,
        destination: ObjectDestination::Battlefield { controller: A },
        program: Program::artifact(
            None,
            vec![ActivatedAbility {
                id: ActivatedAbilityId(1),
                mana_cost: ManaCost::free(),
                source_cost: SourceCost::None,
                body: ExecutableBody::new(
                    BodyTarget::SingleCreature,
                    vec![Instruction::Damage { amount: 1.into() }],
                ),
            }],
        )
        .with_printed_keywords([KeywordAbility::Lifelink, Deathtouch]),
    };
    let permanent_spell = |id, instruction| SeedCard {
        card: CardId(id),
        owner: A,
        destination: ObjectDestination::Hand,
        program: Program::instant(
            ManaCost::free(),
            ExecutableBody::new(BodyTarget::SinglePermanent, vec![instruction]),
        ),
    };
    let transfer = permanent_spell(
        3,
        Instruction::CreateContinuousEffect {
            parts: ResolvedPartsSpec(vec![ResolvedPartSpec::Control(
                ControlRecipient::TargetOwner,
            )]),
            duration: ResolvedDuration::UntilEndOfTurn,
        },
    );
    let game = setup(
        vec![A, B],
        vec![
            source,
            battlefield(2, A),
            transfer,
            permanent_spell(4, Instruction::DestroyTarget),
        ],
        Step::PrecombatMain,
    )
    .into_state();
    let activated = act(
        &game,
        A,
        GameCommand::ActivateAbility {
            source: object(&game, 1).id(),
            ability: ActivatedAbilityId(1),
            target: TargetRef::Object(object(&game, 2).id()),
            payment: Some(ManaPayment::default()),
        },
    );
    let transferred = cast_on(activated.state(), 3, 1);
    assert_eq!(object(&transferred, 1).controller(), Some(B));
    let destroyed = cast_on(&transferred, 4, 1);
    let resolved = round(&destroyed);
    assert_eq!(resolved.state().execution().life_total(B), Some(&21.into()));
    assert_eq!(resolved.state().execution().life_total(A), Some(&20.into()));
    assert_eq!(object(resolved.state(), 2).zone(), ObjectZone::Graveyard(A));
    assert!(
        resolved
            .state()
            .execution()
            .departed_damage_sources()
            .is_empty()
    );
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
