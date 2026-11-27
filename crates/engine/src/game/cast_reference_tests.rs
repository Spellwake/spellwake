//! Independently produced actual public-root observations for the frozen four cases.
//! These read no expected, Forge or historical observation files.
use super::*;
use crate::{
    land::BasicLandType,
    mana::{ManaAbilityRef, ManaCost, ManaType},
    object_identity::{CardId, ObjectDestination},
    program::{
        PlayerRelation, Program, TriggerGrouping, TriggerSpec, TriggeredAbility, TriggeredAbilityId,
    },
    test_support::{creature_types, packet::AliasContext, projection},
};
use serde_json::json;
use std::{collections::BTreeSet, fs::OpenOptions, io::Write, sync::Arc};

const A: SeatId = SeatId(0);
const B: SeatId = SeatId(1);

use super::reference_trace::Trace;

impl Trace {
    fn cast(
        &mut self,
        state: &GameState,
        card: u64,
        island: u64,
        target: Option<TargetRef>,
    ) -> NativeCandidate {
        let mana = self.step(
            state,
            A,
            GameCommand::ActivateMana {
                source: object(state, island),
                ability: ManaAbilityRef::BasicLand(BasicLandType::Island),
            },
        );
        let command = if let Some(target) = target {
            GameCommand::Cast {
                spell: object(mana.state(), card),
                target,
            }
        } else {
            GameCommand::CastUntargeted {
                spell: object(mana.state(), card),
                payment: None,
            }
        };
        let cast = self.step(mana.state(), A, command);
        assert!(cast.mana_payment().is_some());
        cast
    }
    fn order(&mut self, state: &GameState) -> NativeCandidate {
        let pending = state.pending_decision().unwrap();
        let RuleFrame::Triggers(frame) = pending.frame() else {
            panic!("unexpected reference-fixture decision")
        };
        self.step(
            state,
            pending.chooser(),
            GameCommand::Reply {
                decision: pending.id(),
                choice: DecisionReply::OrderTriggers(frame.instances().to_vec()),
            },
        )
    }
    fn resolve(&mut self, state: &GameState) -> NativeCandidate {
        let WindowPhase::Priority(PriorityPhase::Ready { holder }) =
            state.execution().window().phase()
        else {
            panic!("expected normal priority")
        };
        let other = if holder == A { B } else { A };
        let first = self.step(state, holder, GameCommand::Pass);
        self.step(first.state(), other, GameCommand::Pass)
    }
    fn drain(&mut self, mut state: GameState, keep_original: bool) -> GameState {
        for _ in 0..20 {
            if state.pending_decision().is_some() {
                state = self.order(&state).into_state();
            } else if state.execution().window().entries().len() > usize::from(keep_original) {
                state = self.resolve(&state).into_state();
            } else {
                return state;
            }
        }
        panic!("normal fixture resolution did not settle");
    }
}
fn object(state: &GameState, card: u64) -> ObjectId {
    state
        .execution()
        .window()
        .cards()
        .current_identity(CardId(card))
        .unwrap()
}
fn gain(n: i64) -> ExecutableBody {
    ExecutableBody::new(
        BodyTarget::None,
        vec![crate::program::Instruction::AdjustLife(Box::new(
            crate::toolbox::AdjustLifeSpec::gain(
                crate::toolbox::PlayerSelector::Resolving(
                    crate::program::ResolvingPlayer::Controller,
                ),
                crate::program::ValueExpr::Literal(n.into()),
            ),
        ))],
    )
}
fn trigger(condition: TriggerSpec) -> TriggeredAbility {
    TriggeredAbility {
        id: TriggeredAbilityId(1),
        condition,
        grouping: TriggerGrouping::EachOccurrence,
        body: gain(1),
    }
}
fn run(case: &str) {
    let vocabulary = creature_types::selected_vocabulary().unwrap();
    let mut producer = if case == "permanent-self" {
        Program::artifact(Some(ManaCost::one(ManaType::Blue)), vec![])
    } else {
        Program::instant(ManaCost::one(ManaType::Blue), gain(2))
    };
    if case != "watcher" {
        producer
            .triggered
            .push(trigger(crate::program::TriggerSpec::self_cast()));
    }
    let mut cards = vec![SeedCard {
        card: CardId(1),
        owner: A,
        destination: ObjectDestination::Hand,
        program: producer,
    }];
    let mut names = vec![(CardId(1), "Producer".into())];
    if case == "watcher" || case == "copy-self" {
        let mut watcher = Program::artifact(None, vec![]);
        watcher
            .triggered
            .push(trigger(crate::program::TriggerSpec::spell_cast(
                PlayerRelation::Controller,
                crate::program::SpellFilter::instant_or_sorcery(),
            )));
        cards.push(SeedCard {
            card: CardId(2),
            owner: A,
            destination: ObjectDestination::Battlefield { controller: A },
            program: watcher,
        });
        names.push((CardId(2), "Observer".into()));
    }
    if case == "watcher" {
        cards.push(SeedCard {
            card: CardId(3),
            owner: A,
            destination: ObjectDestination::Hand,
            program: Program::artifact(Some(ManaCost::one(ManaType::Blue)), vec![]),
        });
        names.push((CardId(3), "Negative".into()));
    }
    if case == "copy-self" {
        cards.push(SeedCard {
            card: CardId(4),
            owner: A,
            destination: ObjectDestination::Hand,
            program: Program::instant(
                ManaCost::one(ManaType::Blue),
                ExecutableBody::new(
                    BodyTarget::SingleSpell(crate::program::SpellFilter::any()),
                    vec![crate::program::Instruction::CopySpell],
                ),
            ),
        });
        names.push((CardId(4), "Copier".into()));
    }
    for n in 0..2 {
        cards.push(SeedCard {
            card: CardId(100 + n),
            owner: A,
            destination: ObjectDestination::Battlefield { controller: A },
            program: Program::land(BTreeSet::from([BasicLandType::Island.land_type()]), vec![]),
        });
        names.push((CardId(100 + n), format!("Island {n}")));
    }
    for player in [A, B] {
        for n in 0..24 {
            let id = CardId(1000 + u64::from(player.0) * 100 + n);
            cards.push(SeedCard {
                card: id,
                owner: player,
                destination: ObjectDestination::LibraryBottom,
                program: Program::artifact(None, vec![]),
            });
            names.push((id, format!("Library {} {n}", player.0)));
        }
    }
    let order = cards
        .iter()
        .filter(|card| matches!(card.destination, ObjectDestination::Battlefield { .. }))
        .map(|card| card.card)
        .collect();
    let seed = GameSeed::with_uniform_life(
        vec![B, A],
        A,
        cards,
        order,
        vec![],
        20.into(),
        CombatRules::UnrestrictedFreeForAll,
        vocabulary,
        Arc::new(spellwake_contracts::names::NameRules::literal_names()),
    );
    let context = GameContext::new(
        "cast-trigger-reference".into(),
        seed.creature_vocabulary.rules_revision().into(),
        "frozen-literal-inputs".into(),
        "native-cast-v1".into(),
        "prepared-main".into(),
        seed.creature_vocabulary.id(),
        seed.name_rules.id(),
    )
    .unwrap();
    let aliases = AliasContext::from_card_aliases(&names).unwrap();
    let initialized = GameState::initialize_scheduled(
        GameId(6012),
        context,
        seed,
        TurnStart {
            number: 1,
            step: Step::PrecombatMain,
        },
    )
    .unwrap();
    let mut trace = Trace {
        aliases,
        rows: vec![],
    };
    let initialization = projection::candidate(&initialized, None, &trace.aliases).unwrap();
    let cast = trace.cast(initialized.state(), 1, 100, None);
    let original = cast.state().execution().window().entries()[0].id();
    let state = if case == "copy-self" {
        let state = trace.drain(cast.into_state(), true);
        let copier = trace.cast(&state, 4, 101, Some(TargetRef::Stack(original)));
        trace.drain(copier.into_state(), false)
    } else {
        let state = trace.drain(cast.into_state(), false);
        if case == "watcher" {
            let negative = trace.cast(&state, 3, 101, None);
            trace.drain(negative.into_state(), false)
        } else {
            state
        }
    };
    assert!(state.pending_decision().is_none());
    assert!(state.execution().window().entries().is_empty());
    let report = json!({"scenario":case,"source_kind":"actual-public-root-native-transitions","reads_expected_or_reference":false,"initialization":initialization,"rows":trace.rows,"final":projection::state(&state,&trace.aliases).unwrap(),"scope":"normal paid partial-program casts and actual journals/bindings; seeded libraries/main phase/Islands, no full card, runtime, format, host/privacy/replay or performance qualification"});
    if let Some(directory) = std::env::var_os("SPELLWAKE_CAST_TRIGGER_EVIDENCE_DIR") {
        let path = std::path::Path::new(&directory).join(format!("native-{case}.json"));
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .unwrap();
        serde_json::to_writer(&mut file, &report).unwrap();
        file.write_all(b"\n").unwrap();
    }
}

#[test]
fn normal_paid_filtered_watcher() {
    run("watcher");
}
#[test]
fn normal_paid_permanent_self_cast() {
    run("permanent-self");
}
#[test]
fn normal_paid_instruction_self_cast() {
    run("instruction-self");
}
#[test]
fn normal_paid_copy_keeps_trigger_without_casting() {
    run("copy-self");
}
