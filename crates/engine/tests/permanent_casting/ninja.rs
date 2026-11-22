use super::*;
use spellwake_engine::{
    combat::{AttackDeclaration, BlockDeclaration, BlockStatus},
    mana::{CastResource, CastingPayment},
    program::ConditionalCastCost,
};
fn reply(game: &GameState, choice: DecisionReply) -> GameState {
    let pending = game.pending_decision().unwrap();
    accept(
        game,
        pending.chooser(),
        GameCommand::Reply {
            decision: pending.id(),
            choice,
        },
    )
    .into_state()
}
#[test]
fn ninjutsu_costs_retain_blocked_status_and_late_ninjutsu_stays_distinct_from_sneak() {
    let mut cards = seeds();
    let mut ninja = creature(100, A, ObjectDestination::Hand, Some(ManaCost::free()), 5);
    ninja.program.activated.push(ActivatedAbility::ninjutsu(
        ActivatedAbilityId(7),
        ManaCost::free(),
    ));
    cards.push(ninja);
    let mut sneak = creature(101, A, ObjectDestination::Hand, Some(ManaCost::free()), 5);
    sneak.program.conditional_cost = Some(ConditionalCastCost::Sneak(ManaCost::free()));
    cards.push(sneak);
    for (n, seat) in [(102, A), (103, A), (104, A), (105, B)] {
        let mut fighter = creature(
            n,
            seat,
            ObjectDestination::Battlefield { controller: seat },
            None,
            6,
        );
        if seat == A {
            fighter
                .program
                .printed_keywords
                .insert(spellwake_engine::characteristics::KeywordAbility::Haste);
        }
        cards.push(fighter);
    }
    cards.push(spell(
        106,
        A,
        Instruction::ReturnToOwnersHand,
        ManaCost::free(),
        BodyTarget::SingleCreature,
    ));
    let game = setup(Some(Step::DeclareAttackers), cards);
    let ids = [102, 103, 104].map(|n| object(&game, n));
    let blockers = object(&game, 105);
    let hidden = object(&game, 100);
    let activate = |attacker| GameCommand::Ninjutsu {
        source: hidden,
        ability: ActivatedAbilityId(7),
        attacker,
        payment: None,
    };
    let game = reply(
        &game,
        DecisionReply::DeclareAttackers(
            ids.iter()
                .map(|&attacker| AttackDeclaration {
                    attacker,
                    defender: B,
                    defending_planeswalker: None,
                })
                .collect(),
        ),
    );
    assert!(game.advance(input(&game, A, activate(ids[1]))).is_err());
    let game = resolve(&game).into_state();
    let game = reply(
        &game,
        DecisionReply::DeclareBlockers(vec![BlockDeclaration {
            attacker: ids[0],
            blocker: blockers,
        }]),
    );
    let bounced = targeted(&game, A, 106, TargetRef::Object(blockers));
    let game = resolve(bounced.state()).into_state();
    assert_eq!(
        game.execution()
            .combat()
            .unwrap()
            .attacker(ids[0])
            .unwrap()
            .block_status(),
        BlockStatus::Blocked
    );
    assert!(game.advance(input(&game, A, activate(ids[0]))).is_err());
    let damage = resolve(&game).into_state();
    assert_eq!(damage.turn().unwrap().position().step, Step::CombatDamage);
    assert!(
        damage
            .advance(input(
                &damage,
                A,
                GameCommand::CastWithResources {
                    spell: object(&damage, 101),
                    target: None,
                    payment: CastingPayment {
                        use_alternative: true,
                        objects: vec![CastResource::Sneak(ids[1])],
                        ..Default::default()
                    }
                }
            ))
            .is_err()
    );
    let first = accept(&damage, A, activate(ids[1])).into_state();
    let second = accept(&first, A, activate(ids[2])).into_state();
    assert!(second.execution().revealed_by_ninjutsu(hidden));
    let entered = resolve(&second).into_state();
    assert!(!entered.execution().revealed_by_ninjutsu(hidden));
    assert_eq!(physical(&entered, 100).tapped(), Some(true));
    let entrant = object(&entered, 100);
    assert_eq!(
        entered
            .execution()
            .combat()
            .unwrap()
            .attacker(entrant)
            .unwrap()
            .defender(),
        B
    );
    let done = resolve(&entered).into_state();
    assert_eq!(object(&done, 100), entrant);
    assert!(done.execution().window().entries().is_empty());
    assert_eq!(
        done.execution().life_total(B),
        damage.execution().life_total(B)
    );
}
