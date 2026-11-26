#![forbid(unsafe_code)]
//! Temporary convoke and improvise share ordinary payment and consume on the next successful cast.
#[path = "../test_support/creature_vocabulary.rs"]
mod creature_vocabulary;
#[path = "../test_support/characteristic_trajectories.rs"]
mod trajectories;
use trajectories::*;

use spellwake_engine::{
    characteristics::{KeywordAbility, ResolvedDuration},
    game::{GameCommand, GameError, GameInput, GameState, NativeCandidate},
    mana::{CastResource, CastingPayment, ManaCost, ManaPayment},
    object_identity::CardId,
    program::{
        BodyTarget, ExecutableBody, Instruction, ObjectFilter, Program, TemporaryRuleKind,
        ValueExpr,
    },
};
use std::collections::BTreeSet;

fn grant(keyword: KeywordAbility, count: usize) -> Program {
    Program::instant(
        ManaCost::free(),
        ExecutableBody::new(
            BodyTarget::None,
            (0..count)
                .map(|_| Instruction::CreateTemporaryRule {
                    rule: Box::new(TemporaryRuleKind::NextSpellKeyword {
                        filter: ObjectFilter::any(),
                        keyword,
                    }),
                    duration: ResolvedDuration::UntilEndOfTurn,
                })
                .collect(),
        ),
    )
}

fn life_spell(cost: i64, innate: Option<KeywordAbility>) -> Program {
    let mut program = Program::instant(
        ManaCost::generic(cost.into()),
        ExecutableBody::new(
            BodyTarget::None,
            vec![Instruction::GainControllerLife {
                amount: ValueExpr::literal(1),
            }],
        ),
    );
    if let Some(keyword) = innate {
        program.printed_keywords.insert(keyword);
    }
    program
}

fn prepared(keyword: KeywordAbility, count: usize, innate: bool) -> GameState {
    let vocab = vocabulary();
    let creature = Program::creature(Some(ManaCost::free()), 1.into(), 1.into(), BTreeSet::new());
    let artifact = Program::artifact(Some(ManaCost::free()), vec![]);
    let cards = vec![
        card(1, grant(keyword, count), false),
        card(2, life_spell(1, innate.then_some(keyword)), false),
        card(3, life_spell(1, None), false),
        card(4, life_spell(3, None), false),
        card(5, life_spell(0, None), false),
        card(10, creature.clone(), true),
        card(11, artifact.clone(), true),
        card(12, creature, true),
        card(13, artifact, true),
        library(20, P0, &vocab),
        library(21, P1, &vocab),
    ];
    for card in &cards {
        card.program
            .validate(&vocab)
            .expect("keyword fixture program");
    }
    let game = world(
        vocab,
        cards,
        vec![CardId(10), CardId(11), CardId(12), CardId(13)],
    );
    let grant = submit(
        &game,
        P0,
        GameCommand::CastUntargeted {
            spell: object(&game, 1),
            payment: Some(ManaPayment::default()),
        },
    );
    pass_pair(grant.state()).into_state()
}

fn attempt_with_resource(
    game: &GameState,
    spell: u64,
    keyword: KeywordAbility,
    spare: bool,
) -> Result<NativeCandidate, GameError> {
    let resource = match keyword {
        KeywordAbility::Convoke => CastResource::Convoke {
            creature: object(game, if spare { 12 } else { 10 }),
            color: None,
        },
        KeywordAbility::Improvise => {
            CastResource::Improvise(object(game, if spare { 13 } else { 11 }))
        }
        _ => panic!("fixture only covers convoke and improvise"),
    };
    observe_advance(
        game,
        GameInput {
            basis: game.basis().clone(),
            actor: P0,
            command: GameCommand::CastWithResources {
                spell: object(game, spell),
                target: None,
                payment: CastingPayment {
                    mana: Some(ManaPayment::default()),
                    objects: vec![resource],
                    ..Default::default()
                },
            },
        },
    )
}

#[test]
fn convoke_and_improvise_pay_for_only_the_next_spell() {
    for keyword in [KeywordAbility::Convoke, KeywordAbility::Improvise] {
        let game = prepared(keyword, 1, false);
        let cast = attempt_with_resource(&game, 2, keyword, false)
            .expect("granted keyword pays generic cost");
        let resolved = pass_pair(cast.state());
        assert_eq!(
            resolved.state().execution().life_total(P0),
            Some(&21.into())
        );
        assert!(
            attempt_with_resource(resolved.state(), 3, keyword, true).is_err(),
            "a fresh untapped resource cannot pay without the consumed keyword"
        );
    }
}

#[test]
fn innate_keyword_consumes_all_matching_temporary_grants() {
    for keyword in [KeywordAbility::Convoke, KeywordAbility::Improvise] {
        let game = prepared(keyword, 2, true);
        let cast = attempt_with_resource(&game, 2, keyword, false).expect("innate keyword cast");
        let resolved = pass_pair(cast.state());
        assert!(attempt_with_resource(resolved.state(), 3, keyword, true).is_err());
    }
}

#[test]
fn rejected_payment_preserves_grant_resources_and_state() {
    for keyword in [KeywordAbility::Convoke, KeywordAbility::Improvise] {
        let game = prepared(keyword, 1, false);
        let before = game.clone();
        assert!(
            attempt_with_resource(&game, 4, keyword, false).is_err(),
            "one tapped permanent cannot pay three generic mana"
        );
        assert_eq!(game, before);
        let cast = attempt_with_resource(&game, 2, keyword, false)
            .expect("same resource and grant survive rejection");
        let resolved = pass_pair(cast.state());
        assert_eq!(
            resolved.state().execution().life_total(P0),
            Some(&21.into())
        );
    }
}

#[test]
fn casting_without_using_the_keyword_still_consumes_the_grant() {
    for keyword in [KeywordAbility::Convoke, KeywordAbility::Improvise] {
        let game = prepared(keyword, 1, false);
        let free = submit(
            &game,
            P0,
            GameCommand::CastUntargeted {
                spell: object(&game, 5),
                payment: Some(ManaPayment::default()),
            },
        );
        let resolved = pass_pair(free.state());
        assert!(attempt_with_resource(resolved.state(), 2, keyword, false).is_err());
    }
}

#[test]
fn next_keyword_rejects_unimplemented_keyword_families() {
    assert!(
        grant(KeywordAbility::Flying, 1)
            .validate(&vocabulary())
            .is_err()
    );
}
