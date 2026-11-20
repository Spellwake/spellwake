//! Late rejection preserves earlier accepted legend selections and physical state.
use super::*;
use crate::object_identity::{CardId, ObjectDestination};
use crate::{
    checks::{CheckChoice, LegendChoice},
    effects::{SeedCard, private_qualification::QualificationFault},
    program::{Program, Supertype},
    test_support::characteristic_trajectories as tr,
};
use spellwake_contracts::names::{EnglishCardName, NameRules, ObjectNames};
use std::sync::Arc;
fn root(labels: &[&[&str]]) -> GameState {
    let cards = labels
        .iter()
        .enumerate()
        .map(|(i, names)| {
            let mut p = Program::artifact(None, vec![]);
            p.supertypes.insert(Supertype::Legendary);
            p.names = ObjectNames::new(
                names
                    .iter()
                    .map(|n| EnglishCardName::new((*n).into()).unwrap())
                    .collect(),
            )
            .unwrap();
            SeedCard {
                card: CardId(u64::try_from(i).unwrap() + 1),
                owner: tr::P0,
                destination: ObjectDestination::Battlefield { controller: tr::P0 },
                program: p,
            }
        })
        .collect::<Vec<_>>();
    let order = cards.iter().map(|c| c.card).collect();
    let seed = GameSeed::with_uniform_life(
        vec![tr::P0, tr::P1],
        tr::P0,
        cards,
        order,
        vec![],
        20.into(),
        CombatRules::UnrestrictedFreeForAll,
        tr::vocabulary(),
        Arc::new(NameRules::literal_names()),
    );
    let context = GameContext::new(
        "legend-fault".into(),
        seed.creature_vocabulary.rules_revision().into(),
        "literal".into(),
        "legend".into(),
        "prepared".into(),
        seed.creature_vocabulary.id(),
        seed.name_rules.id(),
    )
    .unwrap();
    GameState::initialize_scheduled(
        GameId(7045),
        context,
        seed,
        TurnStart {
            number: 3,
            step: crate::turn::Step::PrecombatMain,
        },
    )
    .unwrap()
    .into_state()
}
fn frame(game: &GameState) -> &LegendChoice {
    let RuleFrame::Checks(CheckDecision::Legend(f)) = game.pending_decision().unwrap().frame()
    else {
        panic!()
    };
    f
}
fn input(game: &GameState, choice: DecisionReply) -> GameInput {
    GameInput {
        basis: game.basis().clone(),
        actor: tr::P0,
        command: GameCommand::Reply {
            decision: game.pending_decision().unwrap().id(),
            choice,
        },
    }
}
#[test]
fn legend_final_movement_faults_keep_the_accepted_keep_and_all_source_state() {
    let game = root(&[&["Alpha"], &["Alpha"], &["Alpha"]]);
    let prefix = game
        .advance(input(
            &game,
            DecisionReply::KeepLegend {
                conflict: frame(&game).conflicts()[0].id(),
                keep: tr::object(&game, 1),
            },
        ))
        .unwrap();
    assert!(matches!(
        prefix.check_journal().choices[0].choice,
        CheckChoice::LegendKeep { .. }
    ));
    assert!(prefix.check_journal().moves.is_empty());
    for fault in [
        QualificationFault::Event(u64::MAX),
        QualificationFault::ObjectIdentity(u64::MAX),
        QualificationFault::WindowRevision(u64::MAX),
    ] {
        let mut source = prefix.state().clone();
        source.execution.qualification_set_fault(fault);
        let saved = source.clone();
        let result = source.advance(input(
            &source,
            DecisionReply::OrderGraveyard(vec![tr::object(&source, 3), tr::object(&source, 2)]),
        ));
        assert!(result.is_err(), "{fault:?}");
        assert_eq!(source, saved);
        assert_eq!(
            source.execution.operational_counters(),
            saved.execution.operational_counters()
        );
        assert_eq!(source.pending_decision(), saved.pending_decision());
    }
}
#[test]
fn late_frame_allocation_failure_preserves_accepted_nonfirst_overlap_keep() {
    let game = root(&[&["Alpha"], &["Alpha", "Beta"], &["Beta"]]);
    let ids = frame(&game)
        .conflicts()
        .iter()
        .map(|c| c.id())
        .collect::<Vec<_>>();
    let prefix = game
        .advance(input(
            &game,
            DecisionReply::KeepLegend {
                conflict: ids[1],
                keep: tr::object(&game, 2),
            },
        ))
        .unwrap();
    let mut source = prefix.into_state();
    source.next_decision = u64::MAX;
    let saved = source.clone();
    assert_eq!(
        source.advance(input(
            &source,
            DecisionReply::KeepLegend {
                conflict: ids[0],
                keep: tr::object(&source, 1)
            }
        )),
        Err(GameError::DecisionIdentityExhausted)
    );
    assert_eq!(source, saved);
    assert_eq!(frame(&source).conflicts().len(), 1);
}

#[test]
fn native_legend_frame_reconstruction_and_conflict_context_rejections_are_atomic() {
    let source = root(&[&["Alpha"], &["Alpha", "Beta"], &["Beta"]]);
    let original = frame(&source).clone();
    let keep = tr::object(&source, 1);
    let execution = source.execution();
    let saved = execution.clone();
    let valid = DecisionReply::KeepLegend {
        conflict: original.conflicts()[0].id(),
        keep,
    };
    for mutation in 0..6 {
        let mut wrong = original.clone();
        match mutation {
            0 => wrong.revision += 1,
            1 => wrong.event.pass += 1,
            2 => wrong.conflicts.reverse(),
            3 => wrong.conflicts[0].domain.reverse(),
            4 => wrong.conflicts[0].id.controller = tr::P1,
            5 => {
                wrong.conflicts.remove(0);
            }
            _ => unreachable!(),
        }
        assert_eq!(
            execution.resume_rule(
                execution.revision(),
                &crate::effects::EffectDecision::Checks(CheckDecision::Legend(wrong)),
                tr::P0,
                valid.clone(),
                None
            ),
            Err(EffectError::Check(crate::checks::CheckError::WrongFrame))
        );
        assert_eq!(execution, &saved);
    }
    for mutation in 0..3 {
        let mut id = original.conflicts()[0].id();
        match mutation {
            0 => id.event.pass += 1,
            1 => id.controller = tr::P1,
            2 => id.ordinal = u64::MAX,
            _ => unreachable!(),
        }
        assert_eq!(
            execution.resume_rule(
                execution.revision(),
                &crate::effects::EffectDecision::Checks(CheckDecision::Legend(original.clone())),
                tr::P0,
                DecisionReply::KeepLegend { conflict: id, keep },
                None
            ),
            Err(EffectError::Check(
                crate::checks::CheckError::InvalidLegendConflict
            ))
        );
        assert_eq!(execution, &saved);
    }
}
