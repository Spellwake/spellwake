//! Frozen admission recipes only. These tests do not initialize a game owner.
use super::*;
use crate::{
    activated::{ActivatedAbility, ActivatedAbilityId, SourceCost},
    characteristics::{
        AbilityChange, ControllerSelector, InstructionSubtypeOperand, NumericExpression,
        NumericPair, PermanentSelector, PowerToughness, ResolvedDuration, ResolvedPartSpec,
        ResolvedPartsSpec, StaticAbilityId, StaticContinuousAbility, StaticPart, StaticParts,
        StaticTypeChange, SubtypeOperation, TypeChangeSpec, TypeOperation,
    },
    mana::{ManaAbility, ManaAbilityId, ManaCost, ManaOutput, ManaType},
};
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};

#[test]
fn counted_mana_admits_typed_selectors_and_rejects_invalid_or_literal_zero_shortcuts() {
    use crate::test_support::program as codec;
    let v = vocabulary();
    let mut p = Program::artifact(None, vec![]);
    p.mana.push(ManaAbility {
        id: ManaAbilityId(1),
        output: ManaOutput::Counted {
            mana_type: Some(ManaType::Green),
            permanents: PermanentSelector {
                creature_subtype: Some(v.resolve_label("Elf").unwrap()),
                controller: ControllerSelector::SourceController,
                ..PermanentSelector::default()
            },
        },
    });
    let value = codec::view(&p);
    assert_eq!(codec::decode(value.clone(), &v).unwrap(), p);
    for bad in [
        json!("Any"),
        json!([]),
        json!(null),
        json!({"required_types":[]}),
    ] {
        let mut wrong = value.clone();
        wrong["mana"][0]["output"]["permanents"] = bad;
        assert!(codec::decode(wrong, &v).is_err());
    }
    let mut contradiction = value.clone();
    contradiction["mana"][0]["output"]["permanents"]["required_types"] = json!(["Creature"]);
    contradiction["mana"][0]["output"]["permanents"]["forbidden_types"] = json!(["Creature"]);
    let decoded = codec::decode_for_owner(contradiction.clone(), &v).unwrap();
    assert_eq!(decoded.validate(&v), Err(DefinitionError::ManaSource));
    assert!(codec::decode(contradiction, &v).is_err());
    let mut foreign = value.clone();
    foreign["mana"][0]["output"]["permanents"]["creature_subtype"]["vocabulary"] =
        json!("0000000000000000000000000000000000000000000000000000000000000000");
    assert!(codec::decode(foreign, &v).is_err());
    let mut amount = value.clone();
    amount["mana"][0]["output"]["amount"] = json!("0");
    assert!(codec::decode(amount, &v).is_err());
    let mut mana_type = value;
    mana_type["mana"][0]["output"]["mana_type"] = json!("AnyColor");
    assert!(codec::decode(mana_type, &v).is_err());
    p.mana[0].output = ManaOutput::Fixed(BTreeMap::from([(ManaType::Green, 0.into())]));
    assert_eq!(p.validate(&v), Err(DefinitionError::ManaSource));
}

#[test]
fn color_parts_and_indicators_have_strict_context_and_five_color_admission() {
    use crate::{
        characteristics::{Color, ColorFilter, ColorOperation, StaticColorOperand},
        test_support::program as codec,
    };
    let v = vocabulary();
    let blue = BTreeSet::from([Color::Blue]);
    let mut indicator = Program::artifact(None, vec![]);
    indicator.color_indicator = Some(blue.clone());
    assert_eq!(indicator.defined_colors(), blue);
    assert_eq!(
        codec::decode(codec::view(&indicator), &v).unwrap(),
        indicator
    );
    indicator.color_indicator = Some(BTreeSet::new());
    assert_eq!(indicator.validate(&v), Err(DefinitionError::ColorIndicator));
    let mut value = codec::view(&Program::artifact(None, vec![]));
    value.as_object_mut().unwrap().remove("color_indicator");
    assert!(codec::decode_for_owner(value, &v).is_err());
    for operation in [
        ColorOperation::Set(blue.clone()),
        ColorOperation::Add(blue.clone()),
        ColorOperation::Remove(blue.clone()),
        ColorOperation::Set(BTreeSet::new()),
    ] {
        let p = Program::instant(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::SinglePermanent,
                vec![Instruction::CreateContinuousEffect {
                    parts: ResolvedPartsSpec(vec![ResolvedPartSpec::Color(operation)]),
                    duration: ResolvedDuration::UntilEndOfTurn,
                }],
            ),
        );
        let value = codec::view(&p);
        assert_eq!(codec::decode(value.clone(), &v).unwrap(), p);
        for bad in [
            json!(["Colorless"]),
            json!(["Colourless"]),
            json!(["Blue", "Blue"]),
            json!([0]),
            json!("Blue"),
            json!(null),
        ] {
            let mut bad_p = value.clone();
            bad_p["kind"]["body"]["instructions"][0]["parts"][0]["change"]["value"] =
                json!({"tag":"Literal","colors":bad});
            assert!(codec::decode(bad_p, &v).is_err());
        }
        let mut live_in_resolved = value;
        live_in_resolved["kind"]["body"]["instructions"][0]["parts"][0]["change"]["value"] =
            json!({"tag":"SourceCurrentColors"});
        assert!(codec::decode(live_in_resolved, &v).is_err());
    }
    for filter in [
        ColorFilter::Any,
        ColorFilter::Has(Color::Blue),
        ColorFilter::Colorless,
        ColorFilter::Monocolored,
        ColorFilter::Multicolored,
    ] {
        let mut p = static_program(
            &[],
            &[],
            vec![StaticPart::Color(ColorOperation::Set(
                StaticColorOperand::SourceCurrentColors,
            ))],
        );
        let StaticContinuousAbility::Battlefield { selector, .. } = p.continuous.as_mut().unwrap()
        else {
            panic!("ordinary static fixture");
        };
        selector.color = filter;
        let value = codec::view(&p);
        assert_eq!(codec::decode(value.clone(), &v).unwrap(), p);
        let mut missing = value.clone();
        missing["continuous"]["selector"]
            .as_object_mut()
            .unwrap()
            .remove("color");
        assert!(codec::decode(missing, &v).is_err());
        let mut bad = value;
        bad["continuous"]["selector"]["color"] = json!({"tag":"Has","color":"Colorless"});
        assert!(codec::decode(bad, &v).is_err());
    }
}

#[test]
fn text_parts_validate_singular_words_local_scope_and_strict_wire_context() {
    use crate::{
        characteristics::{
            CreatureTypeTextChange, CreatureTypeTextChangeSpec, CreatureTypeWord, ResolvedPart,
        },
        priority::SeatId,
        test_support::{program as codec, projection},
    };
    let v = vocabulary();
    let elf = v.resolve_label("Elf").unwrap();
    let vampire = v.resolve_label("Vampire").unwrap();
    let slot = CreatureTypeSlot::new(0);
    let parts = ResolvedPartsSpec(vec![ResolvedPartSpec::Text(CreatureTypeTextChangeSpec {
        from: CreatureTypeWord::Chosen(slot),
        to: CreatureTypeWord::Literal(vampire),
    })]);
    assert_eq!(
        parts.validate(&v, &BTreeSet::new()),
        Err(CharacteristicDefinitionError::UndefinedCreatureTypeSlot)
    );
    assert!(parts.validate(&v, &BTreeSet::from([slot])).is_ok());
    assert_eq!(
        parts.materialize(&BTreeMap::new(), SeatId(0), SeatId(1)),
        Err(crate::characteristics::CharacteristicInvariantError::MissingChosenCreatureType(slot))
    );
    assert_eq!(
        parts
            .materialize(&BTreeMap::from([(slot, elf)]), SeatId(0), SeatId(1))
            .unwrap()
            .0,
        vec![ResolvedPart::Text(CreatureTypeTextChange {
            from: elf,
            to: vampire
        })]
    );
    let program = Program::instant(
        ManaCost::free(),
        ExecutableBody::new(
            BodyTarget::SinglePermanent,
            vec![
                Instruction::ChooseCreatureType {
                    into: slot,
                    excluding: BTreeSet::from([vampire]),
                },
                Instruction::CreateContinuousEffect {
                    parts: parts.clone(),
                    duration: ResolvedDuration::UntilEndOfTurn,
                },
            ],
        ),
    );
    let encoded = codec::view(&program);
    assert_eq!(codec::decode(encoded.clone(), &v).unwrap(), program);
    for bad in [
        json!("Elf"),
        json!(0),
        json!({"tag":"Chosen","slot":0}),
        json!({"tag":"Literal","value":[elf.vocabulary().to_string(),elf.ordinal().to_string()]}),
        json!({"tag":"Literal","value":projection::creature_type_id(elf),"slot":"0"}),
        json!({"tag":"Literal","value":{"vocabulary":"0000000000000000000000000000000000000000000000000000000000000000","ordinal":"0"}}),
    ] {
        let mut bad_program = encoded.clone();
        bad_program["kind"]["body"]["instructions"][1]["parts"][0]["from"] = bad;
        assert!(codec::decode(bad_program, &v).is_err());
    }
    let live = static_program(
        &[],
        &[],
        vec![StaticPart::Text(CreatureTypeTextChange {
            from: elf,
            to: vampire,
        })],
    );
    assert_eq!(codec::decode(codec::view(&live), &v).unwrap(), live);
    let mut bad_live = codec::view(&live);
    bad_live["continuous"]["parts"][0]["from"] = json!({"tag":"Chosen","slot":"0"});
    assert!(codec::decode(bad_live, &v).is_err());
    let mut duplicate = parts.clone();
    duplicate.0.extend(parts.0);
    assert_eq!(
        duplicate.validate(&v, &BTreeSet::from([slot])),
        Err(CharacteristicDefinitionError::DuplicateBucket)
    );
}

#[test]
fn control_parts_bind_resolution_context_and_reject_foreign_operand_families() {
    use crate::{
        characteristics::{ControlRecipient, ResolvedPart},
        priority::SeatId,
        test_support::program as codec,
    };
    let v = vocabulary();
    for (recipient, expected) in [
        (ControlRecipient::ResolvingController, SeatId(0)),
        (ControlRecipient::TargetOwner, SeatId(1)),
    ] {
        let parts = ResolvedPartsSpec(vec![ResolvedPartSpec::Control(recipient)]);
        assert_eq!(
            parts
                .materialize(&BTreeMap::new(), SeatId(0), SeatId(1))
                .unwrap()
                .0,
            vec![ResolvedPart::Control(expected)]
        );
        let program = Program::instant(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::SinglePermanent,
                vec![Instruction::CreateContinuousEffect {
                    parts,
                    duration: ResolvedDuration::UntilEndOfTurn,
                }],
            ),
        );
        assert_eq!(codec::decode(codec::view(&program), &v).unwrap(), program);
        for bad in [
            json!("Unknown"),
            json!(0),
            json!({"ResolvingController":null}),
        ] {
            let mut encoded = codec::view(&program);
            encoded["kind"]["body"]["instructions"][0]["parts"][0]["recipient"] = bad;
            assert!(codec::decode(encoded, &v).is_err());
        }
    }
    let live = static_program(&[], &[], vec![StaticPart::ControlSourceController]);
    assert_eq!(codec::decode(codec::view(&live), &v).unwrap(), live);
    let mut wrong_live = codec::view(&live);
    wrong_live["continuous"]["parts"][0] =
        json!({"tag":"Control","recipient":"ResolvingController"});
    assert!(codec::decode(wrong_live, &v).is_err());
    let targeted_spell = ExecutableBody::new(
        BodyTarget::SingleSpell(crate::program::SpellFilter::any()),
        vec![Instruction::CreateContinuousEffect {
            parts: ResolvedPartsSpec(vec![ResolvedPartSpec::Control(
                ControlRecipient::ResolvingController,
            )]),
            duration: ResolvedDuration::NoStatedDuration,
        }],
    );
    assert_eq!(
        targeted_spell.validate(&v),
        Err(DefinitionError::OperandFamily)
    );
}

#[test]
fn keyword_components_round_trip_and_reject_unknown_duplicate_or_omitted_fields() {
    use crate::{
        characteristics::{
            AbilityChange, AbilityRemoval,
            KeywordAbility::{Haste, Vigilance},
        },
        test_support::program as codec,
    };
    let v = vocabulary();
    for change in [
        AbilityChange::grant_keywords([Haste, Vigilance]),
        AbilityChange {
            removal: Some(AbilityRemoval::All),
            granted_exalted: 0,
            granted_toxic: None,
            grants_unblockable: false,
            granted_trigger: None,
            granted_combat_keywords: vec![],
            granted_keywords: BTreeSet::from([Haste]),
            suppressed_activations: None,
            doesnt_untap: false,
            suppressed_keywords: BTreeSet::new(),
        },
    ] {
        let program = static_program(&[], &[], vec![StaticPart::Abilities(change)]);
        let encoded = codec::view(&program);
        assert_eq!(codec::decode(encoded.clone(), &v).unwrap(), program);
        for keywords in [
            json!(["UnknownKeyword"]),
            json!(["Haste", "Haste"]),
            json!([1]),
            json!([{"Haste":null}]),
        ] {
            let mut bad = encoded.clone();
            bad["continuous"]["parts"][0]["granted_keywords"] = keywords;
            assert!(codec::decode(bad, &v).is_err());
        }
        for field in ["removal", "granted_keywords"] {
            let mut bad = encoded.clone();
            bad["continuous"]["parts"][0]
                .as_object_mut()
                .unwrap()
                .remove(field);
            assert!(codec::decode(bad, &v).is_err());
        }
    }
}

#[test]
fn player_target_programs_round_trip_and_reject_cross_family_life_adjustments() {
    use crate::{life::LifeAdjustment, test_support::program as codec};
    let v = vocabulary();
    for target in [BodyTarget::SinglePlayer, BodyTarget::SingleCreatureOrPlayer] {
        let program = Program::instant(
            ManaCost::free(),
            ExecutableBody::new(target, vec![Instruction::Damage { amount: 3.into() }]),
        );
        assert_eq!(program.validate(&v), Ok(()));
        assert_eq!(codec::decode(codec::view(&program), &v).unwrap(), program);
    }
    let instruction = Instruction::AdjustTargetLife {
        adjustment: LifeAdjustment::Lose(3.into()),
    };
    let program = Program::instant(
        ManaCost::free(),
        ExecutableBody::new(BodyTarget::SinglePlayer, vec![instruction.clone()]),
    );
    assert_eq!(codec::decode(codec::view(&program), &v).unwrap(), program);
    for target in [
        BodyTarget::None,
        BodyTarget::SingleCreature,
        BodyTarget::SingleCreatureOrPlayer,
        BodyTarget::SinglePermanent,
    ] {
        let body = ExecutableBody::new(target, vec![instruction.clone()]);
        assert_eq!(body.validate(&v), Err(DefinitionError::OperandFamily));
    }
}

#[test]
fn life_adjustment_admission_preserves_signed_sets_and_rejects_negative_gain_or_loss() {
    use crate::{life::LifeAdjustment, test_support::program as codec};
    let v = vocabulary();
    for adjustment in [
        LifeAdjustment::Gain(0.into()),
        LifeAdjustment::Lose(0.into()),
        LifeAdjustment::Set((-9).into()),
    ] {
        let program = Program::instant(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::None,
                vec![Instruction::AdjustControllerLife { adjustment }],
            ),
        );
        assert_eq!(program.validate(&v), Ok(()));
        assert_eq!(codec::decode(codec::view(&program), &v).unwrap(), program);
    }
    for adjustment in [
        LifeAdjustment::Gain((-1).into()),
        LifeAdjustment::Lose((-1).into()),
    ] {
        let program = Program::instant(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::None,
                vec![Instruction::AdjustControllerLife { adjustment }],
            ),
        );
        assert_eq!(
            program.validate(&v),
            Err(DefinitionError::InvalidInstruction)
        );
        assert!(codec::decode(codec::view(&program), &v).is_err());
    }
    let program = Program::instant(
        ManaCost::free(),
        ExecutableBody::new(
            BodyTarget::None,
            vec![Instruction::AdjustControllerLife {
                adjustment: LifeAdjustment::Gain(1.into()),
            }],
        ),
    );
    let mut encoded = codec::view(&program);
    encoded["kind"]["body"]["instructions"][0]["adjustment"]["amount"] = json!(1);
    assert!(codec::decode(encoded, &v).is_err());
}

#[test]
fn ability_removal_definitions_round_trip_and_reject_malformed_or_duplicate_parts() {
    use crate::{characteristics::AbilityRemoval, test_support::program as codec};

    let vocabulary = vocabulary();
    for removal in [AbilityRemoval::All, AbilityRemoval::NonMana] {
        let resolved = Program::instant(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::SinglePermanent,
                vec![Instruction::CreateContinuousEffect {
                    parts: ResolvedPartsSpec(vec![ResolvedPartSpec::Abilities(
                        AbilityChange::remove(removal),
                    )]),
                    duration: ResolvedDuration::UntilEndOfTurn,
                }],
            ),
        );
        let live = static_program(
            &[],
            &[],
            vec![StaticPart::Abilities(AbilityChange::remove(removal))],
        );
        for original in [resolved, live] {
            let encoded = codec::view(&original);
            assert_eq!(codec::decode(encoded, &vocabulary).unwrap(), original);
        }
    }
    let value = static_program(
        &[],
        &[],
        vec![StaticPart::Abilities(AbilityChange::remove(
            AbilityRemoval::All,
        ))],
    );
    for malformed in [json!("Unknown"), json!({"All":null}), json!(1)] {
        let mut encoded = codec::view(&value);
        encoded["continuous"]["parts"][0]["removal"] = malformed;
        assert!(codec::decode(encoded, &vocabulary).is_err());
    }
    let mut encoded = codec::view(&value);
    encoded["continuous"]["parts"][0]["unexpected"] = json!(true);
    assert!(codec::decode(encoded, &vocabulary).is_err());
    let mut missing = codec::view(&value);
    missing["continuous"]["parts"][0]
        .as_object_mut()
        .unwrap()
        .remove("removal");
    assert!(codec::decode(missing, &vocabulary).is_err());
    let duplicate = static_program(
        &[],
        &[],
        vec![
            StaticPart::Abilities(AbilityChange::remove(AbilityRemoval::All)),
            StaticPart::Abilities(AbilityChange::remove(AbilityRemoval::NonMana)),
        ],
    );
    assert_eq!(
        duplicate.validate(&vocabulary),
        Err(DefinitionError::ContinuousEffect)
    );
}

fn vocabulary() -> std::sync::Arc<CreatureTypeVocabulary> {
    crate::test_support::creature_types::selected_vocabulary()
        .expect("explicit qualification vocabulary")
}
fn permanent(types: &[CardType], pt: Option<PowerToughness>) -> Program {
    Program::new(
        types.iter().copied().collect(),
        BTreeSet::new(),
        BTreeSet::new(),
        ProgramKind::Permanent(PermanentDefinition {
            entry_tap: None,
            attachment: None,
            printed_pt: pt.map(Into::into),
            casting_cost: None,
        }),
    )
}
fn choice_ability(targetless: bool) -> ActivatedAbility {
    let mut instructions = vec![Instruction::ChooseCreatureType {
        into: CreatureTypeSlot::new(0),
        excluding: BTreeSet::new(),
    }];
    if !targetless {
        instructions.push(Instruction::Damage { amount: 1.into() });
    }
    instructions.push(Instruction::DrawController {
        amount: crate::program::ValueExpr::count_battlefield(
            crate::program::ObjectFilter::any()
                .controlled()
                .of_chosen_slot(CreatureTypeSlot::new(0)),
        ),
    });
    ActivatedAbility {
        id: ActivatedAbilityId(1),
        mana_cost: ManaCost::free(),
        source_cost: SourceCost::Tap,
        body: ExecutableBody::new(
            if targetless {
                BodyTarget::None
            } else {
                BodyTarget::SingleCreature
            },
            instructions,
        ),
    }
}
fn two_two() -> PowerToughness {
    PowerToughness {
        power: 2.into(),
        toughness: 2.into(),
    }
}

#[test]
fn cl_admit_mixed_elf() {
    let v = vocabulary();
    let mut value = permanent(&[CardType::Artifact, CardType::Creature], Some(two_two()));
    value
        .creature_subtypes
        .insert(v.resolve_label("Elf").unwrap());
    // MAP CL-ADMIT-MIXED-ELF assertion 1
    assert_eq!(value.validate(&v), Ok(()));
    // MAP CL-ADMIT-MIXED-ELF assertion 2: this call path has no game/effect constructor.
}
#[test]
fn cl_admit_creature_tap() {
    let v = vocabulary();
    let mut value = permanent(&[CardType::Creature], Some(two_two()));
    value.activated.push(choice_ability(false));
    // MAP CL-ADMIT-CREATURE-TAP assertion 1
    assert_eq!(value.validate(&v), Ok(()));
    // MAP CL-ADMIT-CREATURE-TAP assertion 2: Program admission only.
}
#[test]
fn cl_adm_artelf_005() {
    let v = vocabulary();
    let mut value = permanent(&[CardType::Artifact], None);
    value
        .creature_subtypes
        .insert(v.resolve_label("Elf").unwrap());
    // MAP CL-ADM-ARTELF-005 assertion 1
    assert_eq!(
        value.validate(&v),
        Err(DefinitionError::InvalidCreatureSubtypes)
    );
    // MAP CL-ADM-ARTELF-005 assertion 2: no game/effect owner initialized.
}
#[test]
fn cl_adm_landelf_006() {
    let v = vocabulary();
    let mut value = permanent(&[CardType::Land], None);
    value
        .creature_subtypes
        .insert(v.resolve_label("Elf").unwrap());
    value.mana.push(ManaAbility {
        id: ManaAbilityId(1),
        output: ManaOutput::Fixed(BTreeMap::from([(ManaType::Green, 1.into())])),
    });
    // MAP CL-ADM-LANDELF-006 assertion 1
    assert_eq!(
        value.validate(&v),
        Err(DefinitionError::InvalidCreatureSubtypes)
    );
    // MAP CL-ADM-LANDELF-006 assertion 2: no game/effect constructor is called.
}
#[test]
fn cl_adm_codec_067() {
    let v = vocabulary();
    let elf = crate::test_support::projection::creature_type_id(v.resolve_label("Elf").unwrap());
    let authored = json!({"names":[],"supertypes":[],"land_subtypes":[],"card_types":["Artifact"],"creature_subtypes":[elf],"kind":{"tag":"Permanent","printed_pt":null,"casting_cost":null},"color_indicator":null,"effect_defined_colors":null,"mana":[],"activated":[],"triggered":[],"continuous":null});
    // MAP CL-ADM-CODEC-067 assertion 1
    let value = crate::test_support::program::decode_for_owner(authored, &v).unwrap();
    assert_eq!(
        value.validate(&v),
        Err(DefinitionError::InvalidCreatureSubtypes)
    );
    // MAP CL-ADM-CODEC-067 assertion 2: syntax materialization is not owner initialization.
}
#[test]
fn cl_adm_targetless_022() {
    let mut value = permanent(&[CardType::Artifact], None);
    value.activated.push(choice_ability(true));
    // Targetless ChooseCreatureType activated abilities are admitted under CR 602.2.
    assert_eq!(value.validate(&vocabulary()), Ok(()));

    // A target-requiring instruction with BodyTarget::None is still rejected.
    let mut bad = permanent(&[CardType::Artifact], None);
    let mut ability = choice_ability(true);
    ability.body = ExecutableBody::new(
        BodyTarget::None,
        vec![Instruction::Damage { amount: 1.into() }],
    );
    bad.activated.push(ability);
    assert_eq!(
        bad.validate(&vocabulary()),
        Err(DefinitionError::OperandFamily)
    );
}
#[test]
fn cl_adm_duplicate_030() {
    let mut value = permanent(&[CardType::Artifact], None);
    value.activated = vec![choice_ability(false), choice_ability(false)];
    // MAP CL-ADM-DUPLICATE-030 assertion 1
    assert_eq!(
        value.validate(&vocabulary()),
        Err(DefinitionError::DuplicateAbility)
    );
    // MAP CL-ADM-DUPLICATE-030 assertion 2: Program admission only.
}
#[test]
fn cl_adm_static_singleton() {
    let selector = |required| json!({"name":{"tag":"Any"},"required_supertypes":[],"forbidden_supertypes":[],"required_types":[required],"forbidden_types":[],"land_subtype":null,"creature_subtype":null,"color":{"tag":"Any"},"controller":"Any","exclude_source":false});
    let authored = json!({"names":[],"supertypes":[],"land_subtypes":[],"card_types":["Artifact"],"creature_subtypes":[],"kind":{"tag":"Permanent","printed_pt":{"power":{"tag":"Number","value":"6"},"toughness":{"tag":"Number","value":"6"}},"casting_cost":{"generic":"0","required":{}}},"color_indicator":null,"effect_defined_colors":null,"mana":[],"activated":[],"triggered":[],"continuous":[
        {"id":1,"selector":selector("Artifact"),"parts":[{"tag":"Type","land_subtypes":{"tag":"Keep"},"supertypes":{"tag":"Keep"},"types":{"tag":"Add","types":["Creature"]},"subtypes":{"tag":"Keep"}}]},
        {"id":2,"selector":selector("Land"),"parts":[{"tag":"Type","land_subtypes":{"tag":"Keep"},"supertypes":{"tag":"Keep"},"types":{"tag":"Add","types":["Artifact"]},"subtypes":{"tag":"Keep"}}]}
    ]});
    // MAP CL-ADM-STATIC-SINGLETON assertion 1
    assert!(crate::test_support::program::decode_for_owner(authored, &vocabulary()).is_err());
    // MAP CL-ADM-STATIC-SINGLETON assertion 2: rejected before a Program or owner exists.
}
#[test]
fn cl_adm_undefined_slot() {
    let value = Program::instant(
        ManaCost::free(),
        ExecutableBody::new(
            BodyTarget::SinglePermanent,
            vec![Instruction::CreateContinuousEffect {
                parts: ResolvedPartsSpec(vec![ResolvedPartSpec::Type(TypeChangeSpec {
                    land_subtypes: SubtypeOperation::Keep,
                    supertypes: TypeOperation::Keep,
                    types: TypeOperation::Keep,
                    subtypes: SubtypeOperation::Set(InstructionSubtypeOperand::Chosen(
                        CreatureTypeSlot::new(9),
                    )),
                })]),
                duration: ResolvedDuration::NoStatedDuration,
            }],
        ),
    );
    // MAP CL-ADM-UNDEFINED-SLOT assertion 1: frozen symbolic q9 binds native slot 9.
    assert_eq!(
        value.validate(&vocabulary()),
        Err(DefinitionError::UndefinedCreatureTypeSlot)
    );
    // MAP CL-ADM-UNDEFINED-SLOT assertion 2: Program admission only.
}
fn static_program(
    required: &[CardType],
    forbidden: &[CardType],
    parts: Vec<StaticPart>,
) -> Program {
    let mut value = permanent(
        &[CardType::Artifact],
        Some(PowerToughness {
            power: 6.into(),
            toughness: 6.into(),
        }),
    );
    let ProgramKind::Permanent(definition) = &mut value.kind else {
        unreachable!()
    };
    definition.casting_cost = Some(ManaCost::free());
    value.continuous = Some(StaticContinuousAbility::Battlefield {
        id: StaticAbilityId(1),
        selector: PermanentSelector {
            required_types: required.iter().copied().collect(),
            forbidden_types: forbidden.iter().copied().collect(),
            ..PermanentSelector::default()
        },
        parts: StaticParts(parts),
    });
    value
}
#[test]
fn cl_adm_contradictory_selector() {
    let value = static_program(
        &[CardType::Creature],
        &[CardType::Creature],
        vec![StaticPart::ModifyPT(NumericPair {
            power: NumericExpression::Literal(1.into()),
            toughness: NumericExpression::Literal(1.into()),
        })],
    );
    // MAP CL-ADM-CONTRADICTORY-SELECTOR assertion 1
    assert_eq!(
        value.validate(&vocabulary()),
        Err(DefinitionError::ContinuousEffect)
    );
    // MAP CL-ADM-CONTRADICTORY-SELECTOR assertion 2: no runtime owner initialized.
}
#[test]
fn cl_adm_duplicate_bucket() {
    let value = static_program(
        &[],
        &[],
        [CardType::Creature, CardType::Artifact]
            .into_iter()
            .map(|kind| {
                StaticPart::Type(StaticTypeChange {
                    land_subtypes: SubtypeOperation::Keep,
                    supertypes: TypeOperation::Keep,
                    types: TypeOperation::Add(BTreeSet::from([kind])),
                    subtypes: SubtypeOperation::Keep,
                })
            })
            .collect(),
    );
    // MAP CL-ADM-DUPLICATE-BUCKET assertion 1
    assert_eq!(
        value.validate(&vocabulary()),
        Err(DefinitionError::ContinuousEffect)
    );
    // MAP CL-ADM-DUPLICATE-BUCKET assertion 2: no runtime owner initialized.
}

#[test]
fn printed_keywords_round_trip_without_changing_unkeyworded_definitions() {
    use crate::{characteristics::KeywordAbility, test_support::program as codec};
    let vocabulary = vocabulary();
    let base = Program::artifact(None, vec![]);
    let plain = codec::view(&base);
    assert!(plain.get("printed_keywords").is_none());
    assert_eq!(codec::decode(plain.clone(), &vocabulary).unwrap(), base);
    let program = base.with_printed_keywords([KeywordAbility::Flying, KeywordAbility::SplitSecond]);
    assert_eq!(
        codec::decode(codec::view(&program), &vocabulary).unwrap(),
        program
    );
    for bad in [
        json!(null),
        json!("Flying"),
        json!(["flying"]),
        json!(["Flying", "Flying"]),
    ] {
        let mut wrong = plain.clone();
        wrong["printed_keywords"] = bad;
        assert!(codec::decode(wrong, &vocabulary).is_err());
    }
}
