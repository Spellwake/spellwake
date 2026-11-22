//! Remaining frozen trusted owner units. No case is a complete card/game claim.
#[path = "permanent_copy_unit_tests.rs"]
mod permanent_copy_unit_tests;
use super::*;
use crate::{
    activated::{ActivatedAbility, ActivatedAbilityId, SourceCost},
    characteristics::{
        ResolvedDuration, ResolvedPart, ResolvedPartSpec, ResolvedParts, ResolvedPartsSpec,
        SubtypeOperation, TypeChange, TypeOperation,
    },
    object_identity::{CardId, ObjectDestination, ObjectZone},
    occurrence::{EventCause, RulesOccurrence, RulesRecord, ZoneReason},
    program::{
        CardType, PermanentDefinition, PlayerRelation, ProgramKind, TriggerSpec, TriggeredAbility,
        TriggeredAbilityId,
    },
    targets::TargetCheck,
    turn::Step,
};

const A: SeatId = SeatId(0);
const B: SeatId = SeatId(1);
fn vocabulary() -> std::sync::Arc<CreatureTypeVocabulary> {
    crate::test_support::creature_types::selected_vocabulary()
        .expect("explicit qualification vocabulary")
}
fn body(target: BodyTarget, instructions: Vec<Instruction>) -> ExecutableBody {
    ExecutableBody::new(target, instructions)
}
fn draw(amount: i64) -> Instruction {
    Instruction::DrawController {
        amount: crate::program::ValueExpr::Literal(amount.into()),
    }
}
fn trigger(condition: TriggerSpec) -> TriggeredAbility {
    TriggeredAbility {
        grouping: spellwake_engine::program::TriggerGrouping::EachOccurrence,
        id: TriggeredAbilityId(1),
        condition,
        body: body(BodyTarget::None, vec![draw(1)]),
    }
}
fn activation(source_cost: SourceCost) -> ActivatedAbility {
    ActivatedAbility {
        id: ActivatedAbilityId(1),
        mana_cost: ManaCost::free(),
        source_cost,
        body: body(
            BodyTarget::SingleCreature,
            vec![Instruction::ReturnToOwnersHand],
        ),
    }
}
fn creature() -> Program {
    Program::creature(None, 1.into(), 1.into(), BTreeSet::new())
}
fn forest() -> Program {
    Program::land(
        std::collections::BTreeSet::new(),
        vec![ManaAbility {
            id: ManaAbilityId(1),
            output: ManaOutput::Fixed(BTreeMap::from([(ManaType::Green, 1.into())])),
        }],
    )
}
fn reach() -> Program {
    Program::instant(
        ManaCost::one(ManaType::Blue),
        body(BodyTarget::None, vec![draw(1)]),
    )
}
fn leave_sac() -> Program {
    let mut p = Program::artifact(None, vec![activation(SourceCost::Sacrifice)]);
    p.triggered
        .push(trigger(crate::program::TriggerSpec::leaves_battlefield(
            crate::program::ObjectFilter::self_object(),
        )));
    p
}
fn tap_self() -> Program {
    let mut p = Program::land(
        std::collections::BTreeSet::new(),
        vec![ManaAbility {
            id: ManaAbilityId(1),
            output: ManaOutput::ChooseColor {
                amount: 1.into(),
                colors: BTreeSet::from([ManaType::Green, ManaType::Blue]),
            },
        }],
    );
    p.triggered
        .push(trigger(crate::program::TriggerSpec::becomes_tapped(
            crate::program::ObjectFilter::self_object(),
        )));
    p
}
fn staff() -> Program {
    let mut a = activation(SourceCost::Tap);
    a.body = body(
        BodyTarget::SingleCreature,
        vec![Instruction::Damage { amount: 1.into() }],
    );
    let mut p = Program::artifact(Some(ManaCost::generic(6.into())), vec![a]);
    p.triggered
        .push(trigger(crate::program::TriggerSpec::step_began(
            Step::Upkeep,
            PlayerRelation::Controller,
        )));
    p
}
fn definition(p: &mut Program) -> &mut PermanentDefinition {
    let ProgramKind::Permanent(d) = &mut p.kind else {
        panic!("permanent template")
    };
    d
}
fn seed(cards: Vec<SeedCard>, order: Vec<CardId>) -> GameSeed {
    GameSeed::with_uniform_life(
        vec![A, B],
        A,
        cards,
        order,
        vec![],
        20.into(),
        CombatRules::UnrestrictedFreeForAll,
        vocabulary(),
        std::sync::Arc::new(spellwake_contracts::names::NameRules::literal_names()),
    )
}
fn card(id: u64, p: Program, destination: ObjectDestination) -> SeedCard {
    SeedCard {
        card: CardId(id),
        owner: A,
        destination,
        program: p,
    }
}
fn prepared_history(state: EffectWindow) -> EffectWindow {
    let state = state
        .enter_turn_step(state.revision(), A, false, false)
        .unwrap()
        .into_state();
    let state = state.record_turn_start(state.revision()).unwrap();
    state
        .enter_turn_step(state.revision(), A, true, false)
        .unwrap()
        .into_state()
}

/// Exact finite native templates from program_definitions.rs: nine original
/// positive roles, ten reviewed successors, and all fourteen retained negatives.
fn historical_roles() -> Vec<(&'static str, Program, bool)> {
    let mut sage = Program::creature(
        Some(ManaCost {
            generic: 1.into(),
            required: BTreeMap::from([(ManaType::Blue, 1.into())]),
        }),
        1.into(),
        1.into(),
        BTreeSet::new(),
    );
    sage.triggered
        .push(trigger(crate::program::TriggerSpec::dies(
            crate::program::ObjectFilter::self_object(),
        )));
    let mut rows = vec![
        ("DA-P01", forest(), true),
        ("DA-P02", creature(), true),
        ("DA-P03", leave_sac(), true),
        ("DA-P04", tap_self(), true),
        ("DA-P05", reach(), true),
        ("DA-P06", staff(), true),
        ("DA-P07", sage, true),
        (
            "DA-P08",
            Program::instant(
                ManaCost::one(ManaType::Blue),
                body(
                    BodyTarget::SingleSpell(crate::program::SpellFilter::any()),
                    vec![Instruction::CounterSpell],
                ),
            ),
            true,
        ),
        (
            "DA-P09",
            Program::creature(None, (-1).into(), (-1).into(), BTreeSet::new()),
            true,
        ),
    ];
    for (id, types) in [
        ("CL-ADMIT-DA-P02", [CardType::Artifact, CardType::Creature]),
        ("CL-ADMIT-DA-P03", [CardType::Land, CardType::Creature]),
    ] {
        let mut p = creature();
        p.card_types = types.into();
        rows.push((id, p, true));
    }
    let mut p = creature();
    definition(&mut p).printed_pt = None;
    rows.push(("CL-ADMIT-DA-P04", p, true));
    for (id, mut p) in [("CL-ADMIT-DA-P05", staff()), ("CL-ADMIT-DA-P06", forest())] {
        definition(&mut p).printed_pt = Some(
            PowerToughness {
                power: 1.into(),
                toughness: 1.into(),
            }
            .into(),
        );
        rows.push((id, p, true));
    }
    let mut p = forest();
    p.mana.clear();
    rows.push(("CL-ADMIT-DA-P08", p, true));
    let mut p = forest();
    p.card_types = [CardType::Artifact].into();
    rows.push(("CL-ADMIT-DA-P09", p, true));
    let mut p = forest();
    let donor = creature();
    p.card_types = donor.card_types.clone();
    p.creature_subtypes = donor.creature_subtypes.clone();
    *definition(&mut p) = donor.permanent().unwrap().clone();
    rows.push(("CL-ADMIT-DA-P10", p, true));
    let mut p = leave_sac();
    p.card_types = [CardType::Land].into();
    p.mana = forest().mana;
    rows.push(("CL-ADMIT-DA-P11", p, true));
    let mut p = leave_sac();
    let donor = creature();
    p.card_types = donor.card_types.clone();
    p.creature_subtypes = donor.creature_subtypes.clone();
    *definition(&mut p) = donor.permanent().unwrap().clone();
    rows.push(("CL-ADMIT-DA-P12", p, true));
    let mut p = creature();
    p.card_types.clear();
    rows.push(("DA-N01", p, false));
    let mut p = forest();
    definition(&mut p).casting_cost = Some(ManaCost::free());
    rows.push(("DA-N07", p, false));
    let mut p = reach();
    p.mana = forest().mana;
    rows.push(("DA-N13", p, false));
    let mut p = reach();
    p.activated = leave_sac().activated;
    rows.push(("DA-N14", p, false));
    let mut p = reach();
    p.triggered = tap_self().triggered;
    rows.push(("DA-N15", p, false));
    let mut p = forest();
    p.mana.push(p.mana[0].clone());
    rows.push(("DA-N16", p, false));
    let mut p = leave_sac();
    p.activated.push(p.activated[0].clone());
    rows.push(("DA-N17", p, false));
    let mut p = tap_self();
    p.triggered.push(p.triggered[0].clone());
    rows.push(("DA-N18", p, false));
    let mut p = leave_sac();
    p.activated[0].body = body(BodyTarget::None, vec![draw(1)]);
    rows.push(("DA-N19", p, false));
    let mut p = tap_self();
    p.triggered[0].body = body(
        BodyTarget::SingleCreature,
        vec![Instruction::Damage { amount: 1.into() }],
    );
    rows.push(("DA-N20", p, false));
    rows.push((
        "DA-N21",
        Program::instant(ManaCost::free(), body(BodyTarget::None, vec![draw(-1)])),
        false,
    ));
    let mut p = tap_self();
    p.triggered[0].body.instructions.clear();
    rows.push(("DA-N22", p, false));
    let mut p = creature();
    definition(&mut p).casting_cost = Some(ManaCost::generic((-1).into()));
    rows.push(("DA-N23", p, false));
    let mut p = leave_sac();
    p.activated[0].mana_cost = ManaCost::generic((-1).into());
    rows.push(("DA-N24", p, false));
    rows
}
fn da_successor(id: &str) {
    let rows = historical_roles();
    assert_eq!(rows.len(), 33);
    let (_, program, _) = rows.iter().find(|row| row.0 == id).unwrap();
    let program = program.clone();
    let saved = program.clone();
    let v = vocabulary();
    assert_eq!(
        v.id().to_string(),
        "39721c01eef9fd3f869588b097651bb6115f9cdde5d61bf822d65a8e5435c792"
    );
    // MAP CL-ADMIT-DA assertion 1
    assert_eq!(program.validate(&v), Ok(()), "{id}");
    assert_eq!(program, saved);
    let input = seed(
        vec![card(1, program.clone(), ObjectDestination::Hand)],
        vec![],
    );
    let saved_seed = input.clone();
    // MAP CL-ADMIT-DA assertion 2
    let state = EffectWindow::new(input.clone()).unwrap();
    assert_eq!(input, saved_seed);
    assert_eq!(
        state.programs().collect::<BTreeMap<_, _>>(),
        BTreeMap::from([(CardId(1), &program)])
    );
    assert_eq!(state.window.participants(), &[A, B]);
    assert_eq!(state.window.active_player(), A);
    assert_eq!(state.life_total(A), Some(&20.into()));
    assert_eq!(state.life_total(B), Some(&20.into()));
    assert!(state.mana.is_empty());
    // MAP CL-ADMIT-DA assertion 3
    let object = state.window.cards().current_identity(CardId(1)).unwrap();
    let physical = state.window.cards().get(object).unwrap();
    assert_eq!(physical.owner(), A);
    assert_eq!(physical.controller(), None);
    assert_eq!(physical.zone(), ObjectZone::Hand(A));
    // CR 208.3/208.5: current PT can exist in hand. The old
    // battlefield-only absence assertion no longer describes this query.
    let expected = match id {
        "CL-ADMIT-DA-P02" | "CL-ADMIT-DA-P03" | "CL-ADMIT-DA-P05" | "CL-ADMIT-DA-P06"
        | "CL-ADMIT-DA-P10" | "CL-ADMIT-DA-P12" => Some(PowerToughness {
            power: 1.into(),
            toughness: 1.into(),
        }),
        "CL-ADMIT-DA-P04" => Some(PowerToughness::zero()),
        _ => None,
    };
    assert_eq!(
        state.power_toughness(object).unwrap().map(|pt| pt.derived),
        expected
    );
    assert!(!state.damage.contains_key(&object));
    assert_eq!(
        state.characteristics.counters(object),
        PowerToughnessCounters {
            plus_one: 0.into(),
            minus_one: 0.into()
        }
    );
}
#[test]
fn cl_admit_da_p02() {
    da_successor("CL-ADMIT-DA-P02");
}
#[test]
fn cl_admit_da_p03() {
    da_successor("CL-ADMIT-DA-P03");
}
#[test]
fn cl_admit_da_p04() {
    da_successor("CL-ADMIT-DA-P04");
}
#[test]
fn cl_admit_da_p05() {
    da_successor("CL-ADMIT-DA-P05");
}
#[test]
fn cl_admit_da_p06() {
    da_successor("CL-ADMIT-DA-P06");
}
#[test]
fn cl_admit_da_p08() {
    da_successor("CL-ADMIT-DA-P08");
}
#[test]
fn cl_admit_da_p09() {
    da_successor("CL-ADMIT-DA-P09");
}
#[test]
fn cl_admit_da_p10() {
    da_successor("CL-ADMIT-DA-P10");
}
#[test]
fn cl_admit_da_p11() {
    da_successor("CL-ADMIT-DA-P11");
}
#[test]
fn cl_admit_da_p12() {
    da_successor("CL-ADMIT-DA-P12");
}
#[test]
fn historical_33_roles_remain_explicit() {
    assert_eq!(
        vocabulary().id().to_string(),
        "39721c01eef9fd3f869588b097651bb6115f9cdde5d61bf822d65a8e5435c792"
    );
    let rows = historical_roles();
    assert_eq!(rows.iter().filter(|row| row.2).count(), 19);
    assert_eq!(rows.iter().filter(|row| !row.2).count(), 14);
    for (id, p, accept) in rows {
        let result = EffectWindow::new(seed(vec![card(1, p, ObjectDestination::Hand)], vec![]));
        // Keep the historical classification above. Current admission also covers
        // sacrificing the source to draw and mandatory targeted triggers.
        if accept || matches!(id, "DA-N19" | "DA-N20") {
            assert!(result.is_ok(), "{id}");
        } else {
            assert_eq!(result, Err(EffectError::InvalidSetup), "{id}");
        }
    }
}

#[test]
fn cl_counter_precheck() {
    let mut x = Program::artifact(Some(ManaCost::free()), vec![]);
    definition(&mut x).printed_pt = Some(
        PowerToughness {
            power: 2.into(),
            toughness: 2.into(),
        }
        .into(),
    );
    let input = seed(
        vec![
            card(1, x, ObjectDestination::Battlefield { controller: A }),
            card(
                2,
                Program::creature(Some(ManaCost::free()), 1.into(), 1.into(), BTreeSet::new()),
                ObjectDestination::Battlefield { controller: A },
            ),
        ],
        vec![CardId(1), CardId(2)],
    );
    let state = prepared_history(EffectWindow::new(input).unwrap());
    let x = state.window.cards().current_identity(CardId(1)).unwrap();
    let y = state.window.cards().current_identity(CardId(2)).unwrap();
    let state = state
        .qualification_prepare_creature_fields(
            &[
                (
                    x,
                    PowerToughnessCounters {
                        plus_one: 2.into(),
                        minus_one: 1.into(),
                    },
                ),
                (
                    y,
                    PowerToughnessCounters {
                        plus_one: 1.into(),
                        minus_one: 1.into(),
                    },
                ),
            ],
            &[(x, 1.into()), (y, 1.into())],
        )
        .unwrap();
    let saved = state.clone();
    let result = state.begin_checks(state.revision()).unwrap();
    let next = result.state();
    // MAP CL-COUNTER-PRECHECK assertion 1
    assert_eq!(
        next.window.cards().get(x).unwrap().zone(),
        ObjectZone::Battlefield
    );
    assert_eq!(next.power_toughness(x).unwrap(), None);
    assert_eq!(next.battlefield_marked_damage(x), Some(1.into()));
    assert_eq!(
        next.power_toughness_counters(x),
        Some(PowerToughnessCounters {
            plus_one: 1.into(),
            minus_one: 0.into()
        })
    );
    // MAP CL-COUNTER-PRECHECK assertion 2
    let new_y = next.window.cards().current_identity(CardId(2)).unwrap();
    assert_ne!(new_y, y);
    assert_eq!(
        next.window.cards().get(new_y).unwrap().zone(),
        ObjectZone::Graveyard(A)
    );
    // MAP CL-COUNTER-PRECHECK assertion 3
    let groups: Vec<_> = result
        .append()
        .records
        .iter()
        .filter_map(|record| match record {
            RulesRecord::Event(event)
                if matches!(event.cause, EventCause::StateBasedActions { .. }) =>
            {
                Some(event)
            }
            _ => None,
        })
        .collect();
    assert_eq!(groups.len(), 1);
    let cancellations: BTreeSet<_> = groups[0]
        .occurrences
        .iter()
        .filter_map(|row| match row.value {
            RulesOccurrence::CountersCancelled { object, .. } => Some(object),
            _ => None,
        })
        .collect();
    assert_eq!(cancellations, BTreeSet::from([x, y]));
    let deaths: Vec<_> = groups[0]
        .occurrences
        .iter()
        .filter_map(|row| match &row.value {
            RulesOccurrence::ZoneChange(change) => Some((change.before.object.id(), change.reason)),
            _ => None,
        })
        .collect();
    assert_eq!(
        deaths,
        vec![(
            y,
            ZoneReason::StateBased(spellwake_engine::occurrence::StateBasedMoveReasons::new(
                spellwake_engine::occurrence::StateBasedMoveReason::LethalDestruction
            ))
        )]
    );
    assert_eq!(state, saved);
}

#[test]
fn cl_target_depart_return() {
    let mut p = Program::creature(Some(ManaCost::free()), 2.into(), 3.into(), BTreeSet::new());
    p.activated.push(ActivatedAbility {
        id: ActivatedAbilityId(1),
        mana_cost: ManaCost::free(),
        source_cost: SourceCost::Tap,
        body: body(
            BodyTarget::SingleCreature,
            vec![
                Instruction::ReturnToOwnersHand,
                Instruction::CreateContinuousEffect {
                    parts: ResolvedPartsSpec(vec![ResolvedPartSpec::ModifyPT(PowerToughness {
                        power: 1.into(),
                        toughness: 1.into(),
                    })]),
                    duration: ResolvedDuration::NoStatedDuration,
                },
                Instruction::Damage { amount: 2.into() },
            ],
        ),
    });
    let state = EffectWindow::new(seed(
        vec![card(1, p, ObjectDestination::Battlefield { controller: A })],
        vec![CardId(1)],
    ))
    .unwrap();
    let state = prepared_history(state);
    let state = state.settle_checks(state.revision()).unwrap();
    let old = state.window.cards().current_identity(CardId(1)).unwrap();
    let old_timestamp = state
        .characteristic_timestamps()
        .find(|(object, _)| *object == old)
        .unwrap()
        .1;
    let activation = state
        .activate_ability(
            state.revision(),
            A,
            old,
            ActivatedAbilityId(1),
            TargetRef::Object(old),
            None,
        )
        .unwrap();
    assert_eq!(activation.result().activation.source.tapped(), Some(false));
    let state = activation.into_state();
    let state = state.settle_checks(state.revision()).unwrap();
    let state = state.pass(state.revision(), A).unwrap();
    let state = state.settle_checks(state.revision()).unwrap();
    let mut state = state.pass(state.revision(), B).unwrap();
    let entry = state.window.checking_targets().unwrap().clone();
    let bound = entry.targets().targets()[0].choice();
    assert_eq!(bound.target, TargetRef::Object(old));
    assert_eq!(state.window.cards().get(old).unwrap().tapped(), Some(true));
    let legal = state
        .legal_body_target(
            state.payloads[&entry.id()].body().unwrap(),
            bound.target,
            entry.controller(),
        )
        .unwrap();
    assert!(legal);
    state.window = state
        .window
        .check_resolution_targets(
            state.window.revision(),
            entry.id(),
            vec![TargetCheck {
                target: bound.id,
                legal,
            }],
        )
        .unwrap();
    // Exact private cursor cut permitted by the frozen unit: gate accepted once,
    // body owned by the real paid activation, zero locals, instruction zero.
    state.pending_work = Some(PendingWork::Resolution(ResolutionCursor {
        revealed_piles: None,
        chosen_opponent: None,
        chosen_damage_recipient: None,
        damage_offer_opponents: None,
        vowed_creatures: Default::default(),
        meld_pair: None,
        manifest_remaining: None,
        random_selection: None,
        results: ResolutionResults::default(),
        chosen_effect_color: None,
        chosen_mana_split: None,
        tempting_token_offer: None,
        destroyed_permanents: None,
        chosen_loyalty_removals: None,
        discarded_nonland_mana_value: None,
        last_created_tokens: Vec::new(),
        chosen_token_equipment: None,
        found_spells: Vec::new(),
        largest_discard: None,
        impulse: ImpulseState::default(),
        entry: entry.id(),
        instruction: 0,
        exiled_target: None,
        remembered_permanent: None,
        values: BTreeMap::new(),
    }));
    let mut damage = DamageJournal::default();
    let mut append = RulesJournal::default();
    state
        .apply_instruction(
            entry.clone(),
            Instruction::ReturnToOwnersHand,
            None,
            &mut damage,
            &mut append,
        )
        .unwrap();
    state.advance_instruction().unwrap();
    let hand = state.window.cards().current_identity(CardId(1)).unwrap();
    assert_ne!(hand, old);
    let before = state.rules_snapshot(None).unwrap();
    let (window, moves) = state
        .window
        .qualification_zone_moves(
            state.window.revision(),
            &[(hand, ObjectDestination::Battlefield { controller: A })],
        )
        .unwrap();
    state.window = window;
    let returned = moves[0].after.id();
    state
        .characteristics
        .register_battlefield_entry(returned)
        .unwrap();
    assert!(
        state
            .characteristic_timestamps()
            .find(|(object, _)| *object == returned)
            .unwrap()
            .1
            > old_timestamp
    );
    state
        .emit_from(
            EventCause::MechanismUnit,
            &before,
            None,
            &mut append,
            |previous, current, controllers| {
                moves
                    .iter()
                    .map(|movement| {
                        zone_occurrence_at(
                            previous,
                            current,
                            movement,
                            ZoneReason::MechanismUnit,
                            controllers,
                        )
                    })
                    .collect()
            },
        )
        .unwrap();
    // MAP CL-TARGET-DEPART-RETURN assertion 3
    assert_eq!(
        state.window.resolving().unwrap().targets().targets()[0]
            .choice()
            .target,
        TargetRef::Object(old)
    );
    assert_eq!(state.instruction_index(), Some(1));
    let result = state
        .progress_rules(damage, vec![], vec![], DrawJournal::default(), append)
        .unwrap();
    let next = result.state();
    assert_eq!(
        entry.targets().targets()[0].choice().target,
        TargetRef::Object(old)
    );
    let StackKind::ActivatedAbility { source } = entry.kind() else {
        panic!("actual ability origin")
    };
    assert_eq!(source.id(), old);
    assert_eq!(source.tapped(), Some(false));
    // MAP CL-TARGET-DEPART-RETURN assertion 1
    assert_ne!(returned, old);
    assert_ne!(returned, hand);
    assert_eq!(
        next.window.cards().get(returned).unwrap().zone(),
        ObjectZone::Battlefield
    );
    let creature = next.creature(returned).unwrap().unwrap();
    assert_eq!(creature.power, 2.into());
    assert_eq!(creature.toughness, 3.into());
    assert_eq!(creature.damage, 0.into());
    // MAP CL-TARGET-DEPART-RETURN assertion 2
    assert_eq!(next.continuous_effects().count(), 0);
    let RuleResult::Complete { journal, .. } = result.result() else {
        panic!("completed cursor")
    };
    assert!(journal.attempts.is_empty());
}

impl EffectWindow {
    /// The frozen pure-combat unit supplies two preexisting records with retired
    /// trusted creator entries. Generate/retire those IDs in the actual window,
    /// then add literal records before any legal declaration, without extra cards.
    pub(crate) fn qualification_prepare_frozen_transient(&mut self, object: ObjectId) {
        let source = self.window.cards().get(object).unwrap().clone();
        for operation in [
            TypeOperation::Remove(BTreeSet::from([CardType::Creature])),
            TypeOperation::Add(BTreeSet::from([CardType::Creature])),
        ] {
            self.window = self
                .window
                .record_activation(self.window.revision(), A, source.clone())
                .unwrap();
            let entry = self.window.entries().last().unwrap().id();
            self.window = self.window.finish_checks(self.window.revision()).unwrap();
            self.window = self.window.pass(self.window.revision(), A).unwrap();
            self.window = self.window.finish_checks(self.window.revision()).unwrap();
            self.window = self.window.pass(self.window.revision(), B).unwrap();
            self.window = self
                .window
                .finish_resolution(self.window.revision(), entry, None)
                .unwrap();
            self.window = self.window.finish_checks(self.window.revision()).unwrap();
            self.characteristics
                .add_effect(
                    entry,
                    object,
                    ResolvedParts(vec![ResolvedPart::Type(TypeChange {
                        land_subtypes: SubtypeOperation::Keep,
                        supertypes: TypeOperation::Keep,
                        types: operation,
                        subtypes: SubtypeOperation::Keep,
                    })]),
                    ResolvedDuration::NoStatedDuration,
                )
                .unwrap();
        }
    }
}
