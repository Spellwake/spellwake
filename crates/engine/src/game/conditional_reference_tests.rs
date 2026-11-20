//! Frozen-intent selected public-root producers; no expected/reference file reads.
use super::reference_trace::Trace;
use super::*;
use crate::{
    combat::CombatRules,
    effects::SeedCard,
    land::BasicLandType,
    life::LifeAdjustment,
    mana::{ManaAbilityRef, ManaCost, ManaPayment, ManaSpend, ManaType},
    object_identity::{CardId, ObjectDestination, ObjectId},
    program::{Comparison, ConditionValue, Instruction, Program, ResolutionCondition},
    test_support::{creature_types, packet::AliasContext, projection},
};
use serde_json::json;
use std::{collections::BTreeSet, fs::OpenOptions, io::Write, sync::Arc};
const A: SeatId = SeatId(0);
const B: SeatId = SeatId(1);
fn gain(amount: i64) -> Instruction {
    Instruction::AdjustControllerLife {
        adjustment: LifeAdjustment::Gain(amount.into()),
    }
}
fn branch(threshold: i64, destination: usize) -> Instruction {
    Instruction::BranchIfFalse {
        condition: ResolutionCondition {
            left: ConditionValue::ControllerLife,
            comparison: Comparison::GreaterOrEqual,
            right: ConditionValue::Literal(threshold.into()),
        },
        destination,
    }
}
fn spell(target: BodyTarget, body: Vec<Instruction>) -> Program {
    Program::instant(
        ManaCost::one(ManaType::Blue),
        ExecutableBody::new(target, body),
    )
}
fn object(game: &GameState, id: u64) -> ObjectId {
    game.execution()
        .window()
        .cards()
        .current_identity(CardId(id))
        .unwrap()
}
fn seed(case: &str) -> (NativeCandidate, AliasContext) {
    let v = creature_types::selected_vocabulary().unwrap();
    let (target, body) = match case {
        "CF1" => (BodyTarget::None, vec![branch(20, 2), gain(2), gain(1)]),
        "CF2" => (BodyTarget::None, vec![branch(21, 2), gain(2), gain(1)]),
        "CF3" => (
            BodyTarget::None,
            vec![gain(5), branch(25, 3), gain(2), gain(1)],
        ),
        "CF4" => (
            BodyTarget::SingleCreature,
            vec![branch(21, 2), Instruction::ReturnToOwnersHand, gain(1)],
        ),
        _ => panic!("unknown frozen case"),
    };
    let mut cards = vec![SeedCard {
        card: CardId(1),
        owner: A,
        destination: ObjectDestination::Hand,
        program: spell(target, body),
    }];
    let mut names = vec![(CardId(1), "S".into())];
    if case == "CF4" {
        cards.push(SeedCard {
            card: CardId(2),
            owner: B,
            destination: ObjectDestination::Battlefield { controller: B },
            program: Program::creature(None, 2.into(), 2.into(), BTreeSet::new()),
        });
        names.push((CardId(2), "C".into()));
        cards.push(SeedCard {
            card: CardId(3),
            owner: B,
            destination: ObjectDestination::Hand,
            program: spell(
                BodyTarget::SingleCreature,
                vec![Instruction::ReturnToOwnersHand],
            ),
        });
        names.push((CardId(3), "R".into()));
    }
    for player in [A, B] {
        let id = 1000 + u64::from(player.0);
        cards.push(SeedCard {
            card: CardId(id),
            owner: player,
            destination: ObjectDestination::Battlefield { controller: player },
            program: Program::land(BTreeSet::from([BasicLandType::Island.land_type()]), vec![]),
        });
        names.push((CardId(id), format!("I{}", player.0)));
        for n in 0..8 {
            let id = 2000 + u64::from(player.0) * 10 + n;
            cards.push(SeedCard {
                card: CardId(id),
                owner: player,
                destination: ObjectDestination::LibraryBottom,
                program: Program::artifact(None, vec![]),
            });
            names.push((CardId(id), format!("P{} Library {n}", player.0)));
        }
    }
    let order = cards
        .iter()
        .filter(|card| matches!(card.destination, ObjectDestination::Battlefield { .. }))
        .map(|card| card.card)
        .collect();
    let seed = GameSeed::with_uniform_life(
        vec![A, B],
        A,
        cards,
        order,
        vec![],
        20.into(),
        CombatRules::UnrestrictedFreeForAll,
        v,
        Arc::new(spellwake_contracts::names::NameRules::literal_names()),
    );
    let context = GameContext::new(
        "conditional-reference".into(),
        seed.creature_vocabulary.rules_revision().into(),
        "frozen-literal-partials".into(),
        "conditional-native-v1".into(),
        "prepared-main-real-Island-payment".into(),
        seed.creature_vocabulary.id(),
        seed.name_rules.id(),
    )
    .unwrap();
    let aliases = AliasContext::from_card_aliases(&names).unwrap();
    (
        GameState::initialize_scheduled(
            GameId(6050),
            context,
            seed,
            TurnStart {
                number: 3,
                step: Step::PrecombatMain,
            },
        )
        .unwrap(),
        aliases,
    )
}
fn paid(
    trace: &mut Trace,
    game: &GameState,
    actor: SeatId,
    id: u64,
    target: Option<TargetRef>,
) -> NativeCandidate {
    let mana = trace.step(
        game,
        actor,
        GameCommand::ActivateMana {
            source: object(game, 1000 + u64::from(actor.0)),
            ability: ManaAbilityRef::BasicLand(BasicLandType::Island),
        },
    );
    let payment = ManaPayment {
        spend: vec![ManaSpend {
            mana_type: ManaType::Blue,
            amount: 1.into(),
        }],
    };
    let command = if let Some(target) = target {
        GameCommand::CastPaid {
            spell: object(mana.state(), id),
            target,
            payment,
        }
    } else {
        GameCommand::CastUntargeted {
            spell: object(mana.state(), id),
            payment: Some(payment),
        }
    };
    let cast = trace.step(mana.state(), actor, command);
    assert!(cast.mana_payment().is_some());
    cast
}
fn resolve(trace: &mut Trace, game: &GameState) -> NativeCandidate {
    let WindowPhase::Priority(PriorityPhase::Ready { holder }) = game.execution().window().phase()
    else {
        panic!("normal priority")
    };
    let other = if holder == A { B } else { A };
    let first = trace.step(game, holder, GameCommand::Pass);
    trace.step(first.state(), other, GameCommand::Pass)
}
fn run(case: &str) {
    let output = std::env::var_os("SPELLWAKE_CONDITIONAL_EVIDENCE_DIR")
        .expect("explicit selected output required");
    let (initialized, aliases) = seed(case);
    let initialization = projection::candidate(&initialized, None, &aliases).unwrap();
    let mut trace = Trace {
        aliases,
        rows: vec![],
    };
    let initial = initialized.into_state();
    let target = (case == "CF4").then(|| TargetRef::Object(object(&initial, 2)));
    let cast = paid(&mut trace, &initial, A, 1, target);
    let mut state = cast.into_state();
    if case == "CF4" {
        state = trace.step(&state, A, GameCommand::Pass).into_state();
        state = paid(&mut trace, &state, B, 3, target).into_state();
        state = resolve(&mut trace, &state).into_state();
    }
    state = resolve(&mut trace, &state).into_state();
    assert!(state.execution().window().entries().is_empty());
    assert!(state.pending_decision().is_none());
    let report = json!({"schema_version":crate::test_support::current_profile::REPORT_SCHEMA_VERSION,"record_codec":"spellwake.current-native-qualification.v2","selected_protocol":"conditional-flow-v1","scenario":case,"source_kind":"actual-public-root-native-transitions","reads_expected_or_reference":false,"initialization":initialization,"rows":trace.rows,"final":projection::state(&state,&trace.aliases).unwrap(),"scope":"Prepared main/libraries, real Island activation/normal explicit payment, actual input/candidate/full-state repeated-result equality. Literal partial Programs only; exact cursor/branch facts native-only, no complete card/format/host/replay/runtime/capacity qualification."});
    let path = std::path::Path::new(&output).join(format!("native-{case}.json"));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .unwrap();
    serde_json::to_writer(&mut file, &report).unwrap();
    file.write_all(b"\n").unwrap();
}
#[test]
#[ignore = "explicit source-frozen conditional native producer"]
fn life_true() {
    run("CF1")
}
#[test]
#[ignore = "explicit source-frozen conditional native producer"]
fn life_false_tail() {
    run("CF2")
}
#[test]
#[ignore = "explicit source-frozen conditional native producer"]
fn changed_current_life() {
    run("CF3")
}
#[test]
#[ignore = "explicit source-frozen conditional native producer"]
fn illegal_target_prelude() {
    run("CF4")
}
