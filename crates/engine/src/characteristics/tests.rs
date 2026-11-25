//! Retained frozen U/O recipes plus separate layer-6 ability-removal regressions.
//! Expectations come from expectation-candidate-v2/candidate.expanded.json and
//! the root G03 acceptance, never from a native result or a second evaluator.
use super::private_qualification::{qualification_ordering_selection, qualification_static_probe};
use super::*;
#[path = "ability_removal_tests.rs"]
mod ability_removal;
#[path = "color_definition_tests.rs"]
mod color_definitions;
#[path = "defining_types_pt_tests.rs"]
mod defining_types_pt;
#[path = "land_tests.rs"]
mod land;
#[path = "names_tests.rs"]
mod names;
#[path = "supertypes_tests.rs"]
mod supertypes;
#[path = "world_tests.rs"]
mod world;
use crate::{
    mana::ManaCost,
    object_identity::{CardId, GameObjects, ObjectDestination, ObjectId},
    priority::SeatId,
    program::{CardType, PermanentDefinition, Program, ProgramKind},
};
use spellwake_contracts::creature_types::{CreatureTypeId, CreatureTypeVocabulary};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

const P0: SeatId = SeatId(0);

#[derive(Debug, Clone, PartialEq, Eq)]
struct Fixture {
    cards: GameObjects,
    programs: BTreeMap<CardId, Program>,
    state: CharacteristicState,
    vocabulary: Arc<CreatureTypeVocabulary>,
    mana: BTreeMap<(SeatId, crate::mana::ManaType), RulesInteger>,
    name_rules: spellwake_contracts::names::NameRules,
    objects: BTreeMap<&'static str, ObjectId>,
}
impl Fixture {
    fn new(
        vocabulary: Arc<CreatureTypeVocabulary>,
        rows: Vec<(&'static str, ObjectDestination, Program)>,
        order: &[&str],
    ) -> Self {
        let mut cards = GameObjects::new(0);
        let mut programs = BTreeMap::new();
        let mut card_ids = BTreeMap::new();
        for (index, (alias, destination, program)) in rows.into_iter().enumerate() {
            program.validate(&vocabulary).unwrap();
            let card = CardId(u64::try_from(index).unwrap() + 1);
            cards = cards
                .seed_card(cards.revision(), card, P0, destination)
                .unwrap()
                .next;
            card_ids.insert(alias, card);
            programs.insert(card, program);
        }
        // The frozen BF objects explicitly have continuous_control=true. This
        // trusted physical hook supplies that premise without claiming a game.
        cards = cards.record_turn_start(cards.revision(), P0).unwrap();
        let objects = card_ids
            .iter()
            .map(|(alias, card)| (*alias, cards.current_identity(*card).unwrap()))
            .collect();
        let mut state = CharacteristicState::default();
        let initial_order: Vec<_> = order.iter().map(|alias| card_ids[*alias]).collect();
        state
            .register_initial_battlefield(&cards, &initial_order)
            .unwrap();
        Self {
            cards,
            programs,
            state,
            vocabulary,
            mana: BTreeMap::new(),
            name_rules: spellwake_contracts::names::NameRules::literal_names(),
            objects,
        }
    }
    fn world(&self) -> CharacteristicWorld<'_> {
        static NO_ENTRY_CHOICES: BTreeMap<ObjectId, CreatureTypeId> = BTreeMap::new();
        CharacteristicWorld {
            active_player: SeatId(0),
            step: None,
            participants: &[],
            life: None,
            entry_creature_types: &NO_ENTRY_CHOICES,
            mana: &self.mana,
            combat: None,
            cards: &self.cards,
            programs: &self.programs,
            state: &self.state,
            vocabulary: &self.vocabulary,
            name_rules: &self.name_rules,
        }
    }
    fn object(&self, alias: &str) -> ObjectId {
        self.objects[alias]
    }
    fn key(&self, alias: &str) -> EffectInstanceKey {
        EffectInstanceKey::Static {
            source: self.object(alias),
            ability: StaticAbilityId(1),
        }
    }
    fn affected(&self, aliases: &[&str]) -> BTreeSet<ObjectId> {
        aliases.iter().map(|alias| self.object(alias)).collect()
    }
    fn waits(&self, edges: &[(&str, &str)]) -> BTreeSet<ComponentWait> {
        edges
            .iter()
            .map(|(waiting, prerequisite)| ComponentWait {
                waiting: self.key(waiting),
                prerequisite: self.key(prerequisite),
            })
            .collect()
    }
    fn component(
        &self,
        source: &str,
        bucket: CharacteristicBucket,
        affected: &[&str],
        operation: ResolvedPart,
    ) -> GeneratedComponent {
        let affected = self.affected(affected);
        GeneratedComponent {
            key: self.key(source),
            bucket,
            present: true,
            generated: affected
                .iter()
                .map(|object| (*object, operation.clone()))
                .collect(),
            affected,
        }
    }
}
fn vocabulary() -> Arc<CreatureTypeVocabulary> {
    // V is an explicit execution input. No synthetic Elf/Goblin/Human table or
    // implicit vocabulary publication is substituted for the frozen premise.
    crate::test_support::creature_types::selected_vocabulary()
        .expect("explicit qualification vocabulary")
}
fn pair(power: i64, toughness: i64) -> PowerToughness {
    PowerToughness {
        power: power.into(),
        toughness: toughness.into(),
    }
}
fn zero_cost() -> Option<ManaCost> {
    Some(ManaCost {
        generic: 0.into(),
        required: BTreeMap::new(),
    })
}
fn permanent(
    types: &[CardType],
    pt: (i64, i64),
    subtypes: &[CreatureTypeId],
    casting_cost: Option<ManaCost>,
    continuous: Option<StaticContinuousAbility>,
) -> Program {
    Program {
        cannot_be_copied: false,
        conditional_flash: None,
        activation_cost_reductions: BTreeMap::new(),
        activation_target_cost_reductions: BTreeMap::new(),
        zone_replacement_extras: BTreeMap::new(),
        cast_reductions: vec![],
        activation_modes: Default::default(),
        entry_counter_abilities: vec![],
        conditional_cost: None,
        plot: None,
        adventure: None,
        prepare_spell: None,
        enters_prepared: false,
        splice: None,
        entry_copy: None,
        entry_tap_unless: None,
        gift: None,
        casualty: None,
        conspire: false,
        dredge: None,
        assist: false,
        cipher: false,
        opening_hand_battlefield: false,
        nightbound_back: None,
        transform_back: None,
        modal_back: None,
        flip_back: None,
        prototype: None,
        split: None,
        madness: None,
        miracle: None,
        cast_from_own_exile: false,
        exile_action: None,
        flashback: None,
        flashback_x_mana_symbols: 0,
        sacrifice_creature_cast_cost: false,
        sacrifice_cast_cost_filter: None,
        tap_creatures_cast_cost: false,
        reveal_colorless_creature_cast_cost: false,
        discard_cast_cost: false,
        discard_cast_cost_filter: None,
        graveyard_creature_exile_cost: 0,
        graveyard_exile_x_cost: false,
        graveyard_self_cast: None,
        flashback_sacrifice_creatures: 0,
        flashback_tap_white_creatures: 0,
        flashback_life: 0,
        additional_cast_costs: vec![],
        activation_restrictions: Default::default(),
        activation_hybrid_symbols: Default::default(),
        extra_activation_costs: Default::default(),
        extra_cast_costs: Vec::new(),
        loyalty_costs: Default::default(),
        variable_loyalty_costs: Default::default(),
        face_up_abilities: Vec::new(),
        printed_keywords: Default::default(),
        printed_toxic: None,
        x_mana_symbols: 0,
        hybrid_symbols: Vec::new(),
        cannot_be_countered: false,
        attack_requirement: None,
        additional_attack_requirements: Vec::new(),
        combat_limits: Default::default(),
        cast_during_own_combat: false,
        cast_timing: None,
        additional_life_cost: None,
        siege_back: None,
        disturb: None,
        meld_partner: None,
        printed_defense: None,
        printed_loyalty: None,
        zone_replacements: vec![],
        card_types: types.iter().copied().collect(),
        creature_subtypes: subtypes.iter().copied().collect(),
        land_subtypes: std::collections::BTreeSet::new(),
        spell_subtypes: std::collections::BTreeSet::new(),
        artifact_subtypes: std::collections::BTreeSet::new(),
        enchantment_subtypes: std::collections::BTreeSet::new(),
        planeswalker_subtypes: Default::default(),
        names: spellwake_contracts::names::ObjectNames::nameless(),
        supertypes: BTreeSet::new(),
        color_indicator: None,
        effect_defined_colors: None,
        kind: ProgramKind::Permanent(PermanentDefinition {
            entry_tap: None,
            attachment: None,
            printed_pt: Some(pair(pt.0, pt.1).into()),
            casting_cost,
        }),
        mana: vec![],
        activated: vec![],
        triggered: vec![],
        continuous,
        additional_battlefield_abilities: vec![],
        static_conditions: Default::default(),
    }
}
fn ability(
    required: &[CardType],
    forbidden: &[CardType],
    exclude_source: bool,
    parts: Vec<StaticPart>,
) -> Option<StaticContinuousAbility> {
    Some(StaticContinuousAbility::Battlefield {
        id: StaticAbilityId(1),
        selector: PermanentSelector {
            required_types: required.iter().copied().collect(),
            forbidden_types: forbidden.iter().copied().collect(),
            exclude_source,
            ..PermanentSelector::default()
        },
        parts: StaticParts(parts),
    })
}
fn static_types(operation: TypeOperation) -> StaticPart {
    StaticPart::Type(StaticTypeChange {
        land_subtypes: SubtypeOperation::Keep,
        supertypes: TypeOperation::Keep,
        types: operation,
        subtypes: SubtypeOperation::Keep,
    })
}
fn add_type(kind: CardType) -> TypeOperation {
    TypeOperation::Add(BTreeSet::from([kind]))
}
fn remove_type(kind: CardType) -> TypeOperation {
    TypeOperation::Remove(BTreeSet::from([kind]))
}
fn resolved_types(operation: TypeOperation) -> ResolvedPart {
    ResolvedPart::Type(TypeChange {
        land_subtypes: SubtypeOperation::Keep,
        supertypes: TypeOperation::Keep,
        types: operation,
        subtypes: SubtypeOperation::Keep,
    })
}
fn set_two() -> StaticPart {
    StaticPart::SetPT(NumericPair {
        power: NumericExpression::Literal(2.into()),
        toughness: NumericExpression::Literal(2.into()),
    })
}
fn battlefield() -> ObjectDestination {
    ObjectDestination::Battlefield { controller: P0 }
}
fn assert_selection(
    fixture: &Fixture,
    actual: &ComponentSelection,
    components: Vec<GeneratedComponent>,
    waits: &[(&str, &str)],
    chosen: &str,
) {
    assert_eq!(actual.bucket, CharacteristicBucket::Type);
    let generated: BTreeMap<_, _> = actual
        .generated
        .iter()
        .map(|value| (value.key, value))
        .collect();
    let expected: BTreeMap<_, _> = components.iter().map(|value| (value.key, value)).collect();
    assert_eq!(generated, expected);
    assert_eq!(actual.original_waits, fixture.waits(waits));
    assert_eq!(actual.chosen, fixture.key(chosen));
}
fn assert_reduced(fixture: &Fixture, actual: &ComponentSelection, edges: &[(&str, &str)]) {
    let reduced: BTreeSet<_> = actual
        .derived_timestamp_waits
        .iter()
        .chain(actual.retained_outside_waits.iter())
        .copied()
        .collect();
    assert_eq!(reduced, fixture.waits(edges));
}
fn assert_cycles(fixture: &Fixture, actual: &ComponentSelection, cycles: &[&[&str]]) {
    let actual: BTreeSet<_> = actual
        .strongly_connected
        .iter()
        .filter(|group| group.len() > 1)
        .cloned()
        .collect();
    let expected: BTreeSet<_> = cycles
        .iter()
        .map(|group| {
            group
                .iter()
                .map(|alias| fixture.key(alias))
                .collect::<BTreeSet<_>>()
        })
        .collect();
    assert_eq!(actual, expected);
}
fn assert_order(fixture: &Fixture, result: &EvaluatedCharacteristics, order: &[&str]) {
    let actual: Vec<_> = result
        .trace
        .applied
        .iter()
        .filter(|part| part.bucket == CharacteristicBucket::Type)
        .map(|part| part.key)
        .collect();
    let expected: Vec<_> = order.iter().map(|alias| fixture.key(alias)).collect();
    assert_eq!(actual, expected);
}
fn assert_current(
    fixture: &Fixture,
    result: &EvaluatedCharacteristics,
    alias: &str,
    types: &[CardType],
    pt: Option<(i64, i64)>,
) {
    let current = result.get(fixture.object(alias)).unwrap();
    assert_eq!(current.current_types, types.iter().copied().collect());
    assert_eq!(
        current
            .power_toughness
            .as_ref()
            .map(|value| value.derived.clone()),
        pt.map(|value| pair(value.0, value.1))
    );
}
fn assert_retained_set(
    fixture: &Fixture,
    result: &EvaluatedCharacteristics,
    source: &str,
    affected: &[&str],
) {
    let expected = fixture.component(
        source,
        CharacteristicBucket::SetPT,
        affected,
        ResolvedPart::SetPT(pair(2, 2)),
    );
    let selection = result
        .trace
        .selections
        .iter()
        .find(|selection| {
            selection.bucket == CharacteristicBucket::SetPT
                && selection.chosen == fixture.key(source)
        })
        .unwrap();
    assert_eq!(selection.generated, vec![expected.clone()]);
    assert_eq!(
        result.trace.bindings[&fixture.key(source)],
        PassBinding::Processed(fixture.affected(affected))
    );
    let application = result
        .trace
        .applied
        .iter()
        .find(|part| part.bucket == CharacteristicBucket::SetPT && part.key == fixture.key(source))
        .unwrap();
    assert_eq!(application.affected, expected.affected);
    assert_eq!(application.generated, expected.generated);
}

fn selector_fixture(order: &[&str]) -> Fixture {
    use CardType::{Artifact, Creature};
    Fixture::new(
        vocabulary(),
        vec![
            (
                "A",
                battlefield(),
                permanent(
                    &[Artifact],
                    (6, 6),
                    &[],
                    zero_cost(),
                    ability(
                        &[Artifact],
                        &[],
                        false,
                        vec![static_types(remove_type(Creature))],
                    ),
                ),
            ),
            (
                "B",
                battlefield(),
                permanent(
                    &[Artifact],
                    (6, 6),
                    &[],
                    zero_cost(),
                    ability(
                        &[Creature],
                        &[],
                        false,
                        vec![static_types(add_type(Artifact))],
                    ),
                ),
            ),
            (
                "R",
                battlefield(),
                permanent(&[Artifact], (4, 4), &[], zero_cost(), None),
            ),
            (
                "T",
                battlefield(),
                permanent(&[Creature], (1, 3), &[], zero_cost(), None),
            ),
        ],
        order,
    )
}
fn dependency_selector(order: &[&str]) {
    use CardType::{Artifact, Creature};
    let fixture = selector_fixture(order);
    let saved = fixture.clone();
    let result = evaluate(fixture.world()).unwrap();
    observe_result(&saved, &fixture, &result);
    // MAP CL-DEP-SELECTOR assertion 1
    assert_selection(
        &fixture,
        &result.trace.selections[0],
        vec![
            fixture.component(
                "A",
                CharacteristicBucket::Type,
                &["A", "B", "R"],
                resolved_types(remove_type(Creature)),
            ),
            fixture.component(
                "B",
                CharacteristicBucket::Type,
                &["T"],
                resolved_types(add_type(Artifact)),
            ),
        ],
        &[("A", "B")],
        "B",
    );
    assert_reduced(&fixture, &result.trace.selections[0], &[("A", "B")]);
    // MAP CL-DEP-SELECTOR assertion 2
    assert_selection(
        &fixture,
        &result.trace.selections[1],
        vec![fixture.component(
            "A",
            CharacteristicBucket::Type,
            &["A", "B", "R", "T"],
            resolved_types(remove_type(Creature)),
        )],
        &[],
        "A",
    );
    // MAP CL-DEP-SELECTOR assertion 3
    assert_order(&fixture, &result, &["B", "A"]);
    // MAP CL-DEP-SELECTOR assertion 4
    for alias in ["A", "B", "R", "T"] {
        assert_current(&fixture, &result, alias, &[Artifact], None);
    }
    // MAP CL-DEP-SELECTOR assertion 5
    assert_eq!(fixture, saved);
}
#[test]
fn cl_dep_selector_ab() {
    dependency_selector(&["A", "B", "R", "T"]);
}
#[test]
fn cl_dep_selector_ba() {
    dependency_selector(&["B", "A", "R", "T"]);
}

fn dependency_generated(order: &[&str]) {
    use CardType::{Artifact, Creature, Land};
    let vocabulary = vocabulary();
    let elf = vocabulary.resolve_label("Elf").unwrap();
    let goblin = vocabulary.resolve_label("Goblin").unwrap();
    let human = vocabulary.resolve_label("Human").unwrap();
    let fixture = Fixture::new(
        vocabulary,
        vec![
            (
                "A",
                battlefield(),
                permanent(
                    &[Artifact, Creature],
                    (2, 3),
                    &[elf],
                    zero_cost(),
                    ability(
                        &[Land, Creature],
                        &[],
                        false,
                        vec![StaticPart::Type(StaticTypeChange {
                            land_subtypes: SubtypeOperation::Keep,
                            supertypes: TypeOperation::Keep,
                            types: TypeOperation::Keep,
                            subtypes: SubtypeOperation::Add(
                                StaticSubtypeOperand::SourceCurrentSubtypes,
                            ),
                        })],
                    ),
                ),
            ),
            (
                "B",
                battlefield(),
                permanent(
                    &[Artifact],
                    (6, 6),
                    &[],
                    zero_cost(),
                    ability(
                        &[Artifact, Creature],
                        &[],
                        false,
                        vec![StaticPart::Type(StaticTypeChange {
                            land_subtypes: SubtypeOperation::Keep,
                            supertypes: TypeOperation::Keep,
                            types: TypeOperation::Keep,
                            subtypes: SubtypeOperation::Set(StaticSubtypeOperand::Literal(
                                BTreeSet::from([goblin]),
                            )),
                        })],
                    ),
                ),
            ),
            (
                "R",
                battlefield(),
                permanent(&[Land, Creature], (2, 5), &[human], None, None),
            ),
        ],
        order,
    );
    let saved = fixture.clone();
    let result = evaluate(fixture.world()).unwrap();
    let add_elf = ResolvedPart::Type(TypeChange {
        land_subtypes: SubtypeOperation::Keep,
        supertypes: TypeOperation::Keep,
        types: TypeOperation::Keep,
        subtypes: SubtypeOperation::Add(BTreeSet::from([elf])),
    });
    let add_goblin = ResolvedPart::Type(TypeChange {
        land_subtypes: SubtypeOperation::Keep,
        supertypes: TypeOperation::Keep,
        types: TypeOperation::Keep,
        subtypes: SubtypeOperation::Add(BTreeSet::from([goblin])),
    });
    let set_goblin = ResolvedPart::Type(TypeChange {
        land_subtypes: SubtypeOperation::Keep,
        supertypes: TypeOperation::Keep,
        types: TypeOperation::Keep,
        subtypes: SubtypeOperation::Set(BTreeSet::from([goblin])),
    });
    // MAP CL-DEP-GENERATED assertion 1
    assert_selection(
        &fixture,
        &result.trace.selections[0],
        vec![
            fixture.component("A", CharacteristicBucket::Type, &["R"], add_elf),
            fixture.component("B", CharacteristicBucket::Type, &["A"], set_goblin),
        ],
        &[("A", "B")],
        "B",
    );
    // The next generated component is the actual B-then-A cut, with the same
    // recipient and Goblin intent. This also checks assertion 1's probe field.
    // MAP CL-DEP-GENERATED assertion 2
    assert_selection(
        &fixture,
        &result.trace.selections[1],
        vec![fixture.component("A", CharacteristicBucket::Type, &["R"], add_goblin)],
        &[],
        "A",
    );
    // MAP CL-DEP-GENERATED assertion 3
    assert_order(&fixture, &result, &["B", "A"]);
    // MAP CL-DEP-GENERATED assertion 4
    assert_eq!(
        result
            .get(fixture.object("A"))
            .unwrap()
            .current_creature_subtypes,
        BTreeSet::from([goblin])
    );
    assert_eq!(
        result
            .get(fixture.object("B"))
            .unwrap()
            .current_creature_subtypes,
        BTreeSet::new()
    );
    assert_eq!(
        result
            .get(fixture.object("R"))
            .unwrap()
            .current_creature_subtypes,
        BTreeSet::from([goblin, human])
    );
    // MAP CL-DEP-GENERATED assertion 5
    assert_eq!(fixture, saved);
}
#[test]
fn cl_dep_generated_ab() {
    dependency_generated(&["A", "B", "R"]);
}
#[test]
fn cl_dep_generated_ba() {
    dependency_generated(&["B", "A", "R"]);
}

fn reconsider_fixture(order: &[&str]) -> Fixture {
    use CardType::{Artifact, Creature};
    Fixture::new(
        vocabulary(),
        vec![
            (
                "A",
                battlefield(),
                permanent(
                    &[Artifact],
                    (6, 6),
                    &[],
                    zero_cost(),
                    ability(
                        &[Artifact],
                        &[],
                        false,
                        vec![static_types(remove_type(Creature))],
                    ),
                ),
            ),
            (
                "B",
                battlefield(),
                permanent(
                    &[Artifact],
                    (6, 6),
                    &[],
                    zero_cost(),
                    ability(
                        &[Creature],
                        &[],
                        false,
                        vec![static_types(add_type(Artifact))],
                    ),
                ),
            ),
            (
                "C",
                battlefield(),
                permanent(
                    &[Artifact],
                    (6, 6),
                    &[],
                    zero_cost(),
                    ability(
                        &[Artifact],
                        &[Creature],
                        false,
                        vec![static_types(add_type(Creature)), set_two()],
                    ),
                ),
            ),
            (
                "R",
                battlefield(),
                permanent(&[Artifact], (4, 4), &[], zero_cost(), None),
            ),
            (
                "T",
                battlefield(),
                permanent(&[Creature], (1, 3), &[], zero_cost(), None),
            ),
        ],
        order,
    )
}
fn dependency_reconsider(order: &[&str], a_before_b: bool) {
    use CardType::{Artifact, Creature};
    let fixture = reconsider_fixture(order);
    let saved = fixture.clone();
    let result = evaluate(fixture.world()).unwrap();
    observe_result(&saved, &fixture, &result);
    // MAP CL-DEP-RECONSIDER assertion 1
    assert_selection(
        &fixture,
        &result.trace.selections[0],
        vec![
            fixture.component(
                "A",
                CharacteristicBucket::Type,
                &["A", "B", "C", "R"],
                resolved_types(remove_type(Creature)),
            ),
            fixture.component(
                "B",
                CharacteristicBucket::Type,
                &["T"],
                resolved_types(add_type(Artifact)),
            ),
            fixture.component(
                "C",
                CharacteristicBucket::Type,
                &["A", "B", "C", "R"],
                resolved_types(add_type(Creature)),
            ),
        ],
        &[("A", "B"), ("B", "C")],
        "C",
    );
    assert_cycles(&fixture, &result.trace.selections[0], &[]);
    // MAP CL-DEP-RECONSIDER assertion 2
    assert_selection(
        &fixture,
        &result.trace.selections[1],
        vec![
            fixture.component(
                "A",
                CharacteristicBucket::Type,
                &["A", "B", "C", "R"],
                resolved_types(remove_type(Creature)),
            ),
            fixture.component(
                "B",
                CharacteristicBucket::Type,
                &["A", "B", "C", "R", "T"],
                resolved_types(add_type(Artifact)),
            ),
        ],
        &[("A", "B"), ("B", "A")],
        if a_before_b { "A" } else { "B" },
    );
    assert_cycles(&fixture, &result.trace.selections[1], &[&["A", "B"]]);
    assert_eq!(
        result.trace.selections[1].derived_timestamp_waits,
        fixture.waits(if a_before_b {
            &[("B", "A")]
        } else {
            &[("A", "B")]
        })
    );
    // MAP CL-DEP-RECONSIDER assertion 3
    let remaining = if a_before_b {
        fixture.component(
            "B",
            CharacteristicBucket::Type,
            &["T"],
            resolved_types(add_type(Artifact)),
        )
    } else {
        fixture.component(
            "A",
            CharacteristicBucket::Type,
            &["A", "B", "C", "R", "T"],
            resolved_types(remove_type(Creature)),
        )
    };
    assert_selection(
        &fixture,
        &result.trace.selections[2],
        vec![remaining],
        &[],
        if a_before_b { "B" } else { "A" },
    );
    // MAP CL-DEP-RECONSIDER assertion 4
    assert_order(
        &fixture,
        &result,
        if a_before_b {
            &["C", "A", "B"]
        } else {
            &["C", "B", "A"]
        },
    );
    // MAP CL-DEP-RECONSIDER assertion 5
    assert_retained_set(&fixture, &result, "C", &["A", "B", "C", "R"]);
    // MAP CL-DEP-RECONSIDER assertion 6
    for alias in ["A", "B", "C", "R"] {
        assert_current(&fixture, &result, alias, &[Artifact], None);
    }
    if a_before_b {
        assert_current(&fixture, &result, "T", &[Artifact, Creature], Some((1, 3)));
    } else {
        assert_current(&fixture, &result, "T", &[Artifact], None);
    }
    // MAP CL-DEP-RECONSIDER assertion 7
    assert_eq!(fixture, saved);
}
#[test]
fn cl_dep_reconsider_ab() {
    dependency_reconsider(&["A", "B", "C", "R", "T"], true);
}
#[test]
fn cl_dep_reconsider_ba() {
    dependency_reconsider(&["B", "A", "C", "R", "T"], false);
}

fn loop_fixture(order: &[&str], late_outside: bool) -> Fixture {
    use CardType::{Artifact, Creature, Land};
    let a_parts = if late_outside {
        vec![static_types(add_type(Creature)), set_two()]
    } else {
        vec![static_types(add_type(Creature))]
    };
    let b_parts = if late_outside {
        vec![static_types(add_type(Artifact))]
    } else {
        vec![static_types(add_type(Artifact)), set_two()]
    };
    let c_part = if late_outside {
        static_types(remove_type(Creature))
    } else {
        static_types(add_type(Artifact))
    };
    Fixture::new(
        vocabulary(),
        vec![
            (
                "A",
                battlefield(),
                permanent(
                    &[Artifact],
                    (6, 6),
                    &[],
                    zero_cost(),
                    ability(&[Artifact], &[], false, a_parts),
                ),
            ),
            (
                "B",
                battlefield(),
                permanent(
                    &[Artifact],
                    (6, 6),
                    &[],
                    zero_cost(),
                    ability(&[Creature], &[], false, b_parts),
                ),
            ),
            (
                "C",
                battlefield(),
                permanent(
                    &[Artifact],
                    (6, 6),
                    &[],
                    zero_cost(),
                    ability(&[Land], &[], false, vec![c_part]),
                ),
            ),
            (
                "X",
                battlefield(),
                permanent(&[Artifact], (4, 4), &[], zero_cost(), None),
            ),
            (
                "Y",
                battlefield(),
                permanent(&[Creature], (1, 3), &[], zero_cost(), None),
            ),
            (
                "Z",
                battlefield(),
                permanent(
                    if late_outside {
                        &[Artifact, Land, Creature]
                    } else {
                        &[Land]
                    },
                    (5, 5),
                    &[],
                    None,
                    None,
                ),
            ),
        ],
        order,
    )
}
fn loop_early(order: &[&str], a_before_b: bool) {
    use CardType::{Artifact, Creature, Land};
    let fixture = loop_fixture(order, false);
    let saved = fixture.clone();
    let result = evaluate(fixture.world()).unwrap();
    observe_result(&saved, &fixture, &result);
    // MAP CL-LOOP-EARLY-OUTSIDE assertion 1
    assert_selection(
        &fixture,
        &result.trace.selections[0],
        vec![
            fixture.component(
                "A",
                CharacteristicBucket::Type,
                &["X", "A", "B", "C"],
                resolved_types(add_type(Creature)),
            ),
            fixture.component(
                "B",
                CharacteristicBucket::Type,
                &["Y"],
                resolved_types(add_type(Artifact)),
            ),
            fixture.component(
                "C",
                CharacteristicBucket::Type,
                &["Z"],
                resolved_types(add_type(Artifact)),
            ),
        ],
        &[("A", "B"), ("A", "C"), ("B", "A")],
        if a_before_b { "C" } else { "B" },
    );
    assert_cycles(&fixture, &result.trace.selections[0], &[&["A", "B"]]);
    assert_reduced(
        &fixture,
        &result.trace.selections[0],
        if a_before_b {
            &[("A", "C"), ("B", "A")]
        } else {
            &[("A", "B"), ("A", "C")]
        },
    );
    // MAP CL-LOOP-EARLY-OUTSIDE assertion 2
    if a_before_b {
        assert_selection(
            &fixture,
            &result.trace.selections[1],
            vec![
                fixture.component(
                    "A",
                    CharacteristicBucket::Type,
                    &["X", "Z", "A", "B", "C"],
                    resolved_types(add_type(Creature)),
                ),
                fixture.component(
                    "B",
                    CharacteristicBucket::Type,
                    &["Y"],
                    resolved_types(add_type(Artifact)),
                ),
            ],
            &[("A", "B"), ("B", "A")],
            "A",
        );
        assert_reduced(&fixture, &result.trace.selections[1], &[("B", "A")]);
    } else {
        assert_selection(
            &fixture,
            &result.trace.selections[1],
            vec![
                fixture.component(
                    "A",
                    CharacteristicBucket::Type,
                    &["X", "Y", "A", "B", "C"],
                    resolved_types(add_type(Creature)),
                ),
                fixture.component(
                    "C",
                    CharacteristicBucket::Type,
                    &["Z"],
                    resolved_types(add_type(Artifact)),
                ),
            ],
            &[("A", "C")],
            "C",
        );
        assert_reduced(&fixture, &result.trace.selections[1], &[("A", "C")]);
    }
    // MAP CL-LOOP-EARLY-OUTSIDE assertion 3
    assert_selection(
        &fixture,
        &result.trace.selections[2],
        vec![fixture.component(
            if a_before_b { "B" } else { "A" },
            CharacteristicBucket::Type,
            &["X", "Y", "Z", "A", "B", "C"],
            resolved_types(add_type(if a_before_b { Artifact } else { Creature })),
        )],
        &[],
        if a_before_b { "B" } else { "A" },
    );
    // MAP CL-LOOP-EARLY-OUTSIDE assertion 4
    assert_order(
        &fixture,
        &result,
        if a_before_b {
            &["C", "A", "B"]
        } else {
            &["B", "C", "A"]
        },
    );
    // MAP CL-LOOP-EARLY-OUTSIDE assertion 5
    assert_retained_set(
        &fixture,
        &result,
        "B",
        if a_before_b {
            &["X", "Y", "Z", "A", "B", "C"]
        } else {
            &["Y"]
        },
    );
    // MAP CL-LOOP-EARLY-OUTSIDE assertions 6-7
    for alias in ["A", "B", "C", "X"] {
        assert_current(
            &fixture,
            &result,
            alias,
            &[Artifact, Creature],
            Some(if a_before_b {
                (2, 2)
            } else if alias == "X" {
                (4, 4)
            } else {
                (6, 6)
            }),
        );
    }
    assert_current(&fixture, &result, "Y", &[Artifact, Creature], Some((2, 2)));
    assert_current(
        &fixture,
        &result,
        "Z",
        &[Artifact, Land, Creature],
        Some(if a_before_b { (2, 2) } else { (5, 5) }),
    );
    // MAP CL-LOOP-EARLY-OUTSIDE assertion 8
    assert_eq!(fixture, saved);
}
#[test]
fn cl_loop_early_outside_ab() {
    loop_early(&["A", "B", "C", "X", "Y", "Z"], true);
}
#[test]
fn cl_loop_early_outside_ba() {
    loop_early(&["B", "A", "C", "X", "Y", "Z"], false);
}

fn loop_late(order: &[&str], a_before_b: bool) {
    use CardType::{Artifact, Creature, Land};
    let fixture = loop_fixture(order, true);
    let saved = fixture.clone();
    let result = evaluate(fixture.world()).unwrap();
    observe_result(&saved, &fixture, &result);
    // MAP CL-LOOP-LATE-OUTSIDE assertion 1
    assert_selection(
        &fixture,
        &result.trace.selections[0],
        vec![
            fixture.component(
                "A",
                CharacteristicBucket::Type,
                &["X", "Z", "A", "B", "C"],
                resolved_types(add_type(Creature)),
            ),
            fixture.component(
                "B",
                CharacteristicBucket::Type,
                &["Y", "Z"],
                resolved_types(add_type(Artifact)),
            ),
            fixture.component(
                "C",
                CharacteristicBucket::Type,
                &["Z"],
                resolved_types(remove_type(Creature)),
            ),
        ],
        &[("A", "B"), ("B", "A"), ("B", "C")],
        if a_before_b { "A" } else { "C" },
    );
    assert_reduced(
        &fixture,
        &result.trace.selections[0],
        if a_before_b {
            &[("B", "A"), ("B", "C")]
        } else {
            &[("A", "B"), ("B", "C")]
        },
    );
    // The exact wait set above also asserts the frozen absence of A/C intent dependence.
    // MAP CL-LOOP-LATE-OUTSIDE assertion 2
    if a_before_b {
        assert_selection(
            &fixture,
            &result.trace.selections[1],
            vec![
                fixture.component(
                    "B",
                    CharacteristicBucket::Type,
                    &["X", "Y", "Z", "A", "B", "C"],
                    resolved_types(add_type(Artifact)),
                ),
                fixture.component(
                    "C",
                    CharacteristicBucket::Type,
                    &["Z"],
                    resolved_types(remove_type(Creature)),
                ),
            ],
            &[("B", "C")],
            "C",
        );
    } else {
        assert_selection(
            &fixture,
            &result.trace.selections[1],
            vec![
                fixture.component(
                    "A",
                    CharacteristicBucket::Type,
                    &["X", "Z", "A", "B", "C"],
                    resolved_types(add_type(Creature)),
                ),
                fixture.component(
                    "B",
                    CharacteristicBucket::Type,
                    &["Y"],
                    resolved_types(add_type(Artifact)),
                ),
            ],
            &[("A", "B"), ("B", "A")],
            "B",
        );
        assert_reduced(&fixture, &result.trace.selections[1], &[("A", "B")]);
    }
    // MAP CL-LOOP-LATE-OUTSIDE assertion 3
    assert_selection(
        &fixture,
        &result.trace.selections[2],
        vec![fixture.component(
            if a_before_b { "B" } else { "A" },
            CharacteristicBucket::Type,
            if a_before_b {
                &["X", "Y", "A", "B", "C"]
            } else {
                &["X", "Y", "Z", "A", "B", "C"]
            },
            resolved_types(add_type(if a_before_b { Artifact } else { Creature })),
        )],
        &[],
        if a_before_b { "B" } else { "A" },
    );
    // MAP CL-LOOP-LATE-OUTSIDE assertion 4
    assert_order(
        &fixture,
        &result,
        if a_before_b {
            &["A", "C", "B"]
        } else {
            &["C", "B", "A"]
        },
    );
    // MAP CL-LOOP-LATE-OUTSIDE assertion 5
    assert_retained_set(
        &fixture,
        &result,
        "A",
        if a_before_b {
            &["X", "Z", "A", "B", "C"]
        } else {
            &["X", "Y", "Z", "A", "B", "C"]
        },
    );
    // MAP CL-LOOP-LATE-OUTSIDE assertion 6
    for alias in ["A", "B", "C", "X"] {
        assert_current(
            &fixture,
            &result,
            alias,
            &[Artifact, Creature],
            Some((2, 2)),
        );
    }
    assert_current(
        &fixture,
        &result,
        "Y",
        &[Artifact, Creature],
        Some(if a_before_b { (1, 3) } else { (2, 2) }),
    );
    if a_before_b {
        assert_current(&fixture, &result, "Z", &[Artifact, Land], None);
    } else {
        assert_current(
            &fixture,
            &result,
            "Z",
            &[Artifact, Land, Creature],
            Some((2, 2)),
        );
    }
    // Set(2,2) for Z is asserted in the generated 7b intent above. No current
    // numeric PT is invented for the AB noncreature explanatory latent field.
    // MAP CL-LOOP-LATE-OUTSIDE assertion 7
    assert_eq!(fixture, saved);
}
#[test]
fn cl_loop_late_outside_ab() {
    loop_late(&["A", "B", "C", "X", "Y", "Z"], true);
}
#[test]
fn cl_loop_late_outside_ba() {
    loop_late(&["B", "A", "C", "X", "Y", "Z"], false);
}

fn started_fixture(source_in_hand: bool) -> Fixture {
    use CardType::{Artifact, Creature};
    Fixture::new(
        vocabulary(),
        vec![
            (
                "A",
                if source_in_hand {
                    ObjectDestination::Hand
                } else {
                    battlefield()
                },
                permanent(
                    &[Artifact],
                    (6, 6),
                    &[],
                    zero_cost(),
                    ability(
                        &[Artifact],
                        &[Creature],
                        true,
                        vec![static_types(add_type(Creature)), set_two()],
                    ),
                ),
            ),
            (
                "X",
                battlefield(),
                permanent(
                    if source_in_hand {
                        &[Artifact]
                    } else {
                        &[Artifact, Creature]
                    },
                    (4, 4),
                    &[],
                    zero_cost(),
                    None,
                ),
            ),
        ],
        if source_in_hand { &["X"] } else { &["A", "X"] },
    )
}
#[test]
fn cl_started_empty() {
    use CardType::{Artifact, Creature};
    let fixture = started_fixture(false);
    let saved = fixture.clone();
    let result = evaluate(fixture.world()).unwrap();
    observe_result(&saved, &fixture, &result);
    // MAP CL-STARTED-EMPTY assertion 1
    assert_current(&fixture, &result, "X", &[Artifact, Creature], Some((4, 4)));
    // MAP CL-STARTED-EMPTY assertion 2
    assert_retained_set(&fixture, &result, "A", &[]);
    assert_eq!(
        result
            .trace
            .applied
            .iter()
            .find(|part| part.key == fixture.key("A") && part.bucket == CharacteristicBucket::Type)
            .unwrap()
            .affected,
        BTreeSet::new()
    );
    let mut scratch = result.objects.clone();
    scratch.get_mut(&fixture.object("X")).unwrap().current_types =
        BTreeSet::from([CardType::Artifact]);
    let saved_scratch = scratch.clone();
    let generated = qualification_static_probe(
        fixture.world(),
        fixture.object("A"),
        CharacteristicBucket::SetPT,
        &scratch,
        PassBinding::Processed(BTreeSet::new()),
    )
    .unwrap();
    // MAP CL-STARTED-EMPTY assertion 3
    assert_eq!(
        generated,
        fixture.component(
            "A",
            CharacteristicBucket::SetPT,
            &[],
            ResolvedPart::SetPT(pair(2, 2))
        )
    );
    // MAP CL-STARTED-EMPTY assertion 4
    assert_eq!(scratch, saved_scratch);
    assert_eq!(fixture, saved);
}
#[test]
fn cl_started_absent() {
    use CardType::Artifact;
    let fixture = started_fixture(true);
    let saved = fixture.clone();
    let result = evaluate(fixture.world()).unwrap();
    observe_result(&saved, &fixture, &result);
    // MAP CL-STARTED-ABSENT assertion 1
    assert_current(&fixture, &result, "X", &[Artifact], None);
    // MAP CL-STARTED-ABSENT assertion 2
    assert!(!result.trace.bindings.contains_key(&fixture.key("A")));
    assert!(result.trace.selections.is_empty());
    assert!(result.trace.applied.is_empty());
    // MAP CL-STARTED-ABSENT assertion 3
    assert_eq!(fixture, saved);
}

fn ordering_key(index: u64) -> EffectInstanceKey {
    EffectInstanceKey::Resolved(ContinuousEffectId(index))
}
fn ordering_wait(waiting: u64, prerequisite: u64) -> ComponentWait {
    ComponentWait {
        waiting: ordering_key(waiting),
        prerequisite: ordering_key(prerequisite),
    }
}
#[test]
fn cl_order_disjoint() {
    // A=1, B=2, C=3, D=4, E=5 are supplied graph vertices, not live effects.
    let timestamps = BTreeMap::from([
        (ordering_key(1), CharacteristicTimestamp(1)),
        (ordering_key(4), CharacteristicTimestamp(2)),
        (ordering_key(3), CharacteristicTimestamp(3)),
        (ordering_key(2), CharacteristicTimestamp(4)),
        (ordering_key(5), CharacteristicTimestamp(5)),
    ]);
    let waits = BTreeSet::from([
        ordering_wait(1, 2),
        ordering_wait(2, 1),
        ordering_wait(4, 5),
        ordering_wait(5, 4),
    ]);
    let saved = (timestamps.clone(), waits.clone());
    let result =
        qualification_ordering_selection(CharacteristicBucket::Type, &timestamps, &waits).unwrap();
    // MAP CL-ORDER-DISJOINT assertion 1
    assert_eq!(
        result
            .strongly_connected
            .iter()
            .cloned()
            .collect::<BTreeSet<_>>(),
        BTreeSet::from([
            BTreeSet::from([ordering_key(1), ordering_key(2)]),
            BTreeSet::from([ordering_key(4), ordering_key(5)]),
            BTreeSet::from([ordering_key(3)]),
        ])
    );
    assert_eq!(result.original_waits, waits);
    assert_eq!(
        result.derived_timestamp_waits,
        BTreeSet::from([ordering_wait(2, 1), ordering_wait(5, 4)])
    );
    assert!(result.retained_outside_waits.is_empty());
    assert_eq!(
        result.eligible,
        vec![ordering_key(1), ordering_key(4), ordering_key(3)]
    );
    assert_eq!(result.chosen, ordering_key(1));
    assert_eq!((timestamps, waits), saved);
    // The selection-only hook does not expose application/discard continuations.
}
#[test]
fn cl_order_overlap() {
    // A=1, B=2, C=3, D=4 are supplied graph vertices only.
    let timestamps = BTreeMap::from([
        (ordering_key(1), CharacteristicTimestamp(1)),
        (ordering_key(2), CharacteristicTimestamp(2)),
        (ordering_key(3), CharacteristicTimestamp(3)),
        (ordering_key(4), CharacteristicTimestamp(4)),
    ]);
    let waits = BTreeSet::from([
        ordering_wait(1, 2),
        ordering_wait(2, 1),
        ordering_wait(2, 3),
        ordering_wait(3, 2),
        ordering_wait(3, 4),
    ]);
    let saved = (timestamps.clone(), waits.clone());
    let result =
        qualification_ordering_selection(CharacteristicBucket::Type, &timestamps, &waits).unwrap();
    // MAP CL-ORDER-OVERLAP assertion 1
    assert_eq!(
        result
            .strongly_connected
            .iter()
            .cloned()
            .collect::<BTreeSet<_>>(),
        BTreeSet::from([
            BTreeSet::from([ordering_key(1), ordering_key(2), ordering_key(3)]),
            BTreeSet::from([ordering_key(4)]),
        ])
    );
    assert_eq!(result.original_waits, waits);
    assert_eq!(
        result.derived_timestamp_waits,
        BTreeSet::from([ordering_wait(2, 1), ordering_wait(3, 2)])
    );
    assert_eq!(
        result.retained_outside_waits,
        BTreeSet::from([ordering_wait(3, 4)])
    );
    assert_eq!(result.eligible, vec![ordering_key(1), ordering_key(4)]);
    assert_eq!(result.chosen, ordering_key(1));
    assert_eq!((timestamps, waits), saved);
    // No claim of card equivalence, Forge parity, or an observed apply/discard.
}

/// Capture only actual fixture owners and the one query result already used by assertions.
fn observe_result(before: &Fixture, after: &Fixture, result: &EvaluatedCharacteristics) {
    crate::effects::characteristic_reference_capture::observe_pure_fixture(
        before.world(),
        &before.objects,
        result,
        before == after,
    );
}

/// Finite reuse of the original private test routes; no second fixture implementation.
pub(crate) fn capture_static_case(case: &str) {
    match case {
        "CL-STARTED-EMPTY" => cl_started_empty(),
        "CL-STARTED-ABSENT" => cl_started_absent(),
        "CL-DEP-SELECTOR-AB" => cl_dep_selector_ab(),
        "CL-DEP-SELECTOR-BA" => cl_dep_selector_ba(),
        "CL-DEP-RECONSIDER-AB" => cl_dep_reconsider_ab(),
        "CL-DEP-RECONSIDER-BA" => cl_dep_reconsider_ba(),
        "CL-LOOP-EARLY-OUTSIDE-AB" => cl_loop_early_outside_ab(),
        "CL-LOOP-EARLY-OUTSIDE-BA" => cl_loop_early_outside_ba(),
        "CL-LOOP-LATE-OUTSIDE-AB" => cl_loop_late_outside_ab(),
        "CL-LOOP-LATE-OUTSIDE-BA" => cl_loop_late_outside_ba(),
        _ => panic!("outside the ten selected pure static fixtures"),
    }
}

#[test]
fn station_striation_turns_on_and_off_at_its_current_charge_total() {
    let mut fixture = Fixture::new(
        vocabulary(),
        vec![(
            "ship",
            ObjectDestination::Battlefield { controller: P0 },
            Program::station_spacecraft(
                ManaCost::free(),
                std::num::NonZeroU16::new(3).unwrap(),
                pair(6, 5),
                [KeywordAbility::Flying, KeywordAbility::Haste].into(),
            ),
        )],
        &["ship"],
    );
    let id = fixture.object("ship");
    for count in [0_i64, 2, 3, 4, 2, 0] {
        fixture.state.charge.insert(id, count.into());
        let evaluated = evaluate(fixture.world()).unwrap();
        let ship = evaluated.get(id).unwrap();
        assert_eq!(ship.is_creature(), count >= 3);
        assert_eq!(ship.has_keyword(KeywordAbility::Flying), count >= 3);
        assert_eq!(ship.has_keyword(KeywordAbility::Haste), count >= 3);
        assert_eq!(
            ship.power_toughness.as_ref().map(|p| p.derived.clone()),
            (count >= 3).then(|| pair(6, 5))
        );
    }
}
