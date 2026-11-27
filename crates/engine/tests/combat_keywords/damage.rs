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
