use super::*;
use crate::{
    characteristics::{Color, CreatureTypeTextChange},
    mana::{ManaCost, ManaType},
    program::{BodyTarget, ExecutableBody, Instruction},
    test_support::{creature_types::selected_vocabulary, program as codec},
};
use serde_json::json;

fn creature(v: &CreatureTypeVocabulary, labels: &[&str]) -> Program {
    Program::creature(
        None,
        1.into(),
        1.into(),
        labels
            .iter()
            .map(|label| v.resolve_label(label).unwrap())
            .collect(),
    )
}
fn default_name(v: &CreatureTypeVocabulary, labels: &[&str]) -> TokenName {
    TokenName::Default {
        ordered_subtypes: labels
            .iter()
            .map(|label| TokenSubtype::Creature(v.resolve_label(label).unwrap()))
            .collect(),
    }
}
fn name(program: &Program) -> &str {
    program.names.iter().next().unwrap().as_str()
}

#[test]
fn materialization_uses_authored_word_order_or_the_specified_proper_name() {
    let v = selected_vocabulary().unwrap();
    let template = TokenDefinition::new(
        creature(&v, &["Dwarf", "Berserker"]),
        default_name(&v, &["Dwarf", "Berserker"]),
        &v,
    )
    .unwrap();
    assert!(template.program().names.is_empty());
    assert_eq!(
        name(&template.materialize(&v).unwrap()),
        "Dwarf Berserker Token"
    );
    let named = TokenDefinition::new(
        creature(&v, &["Goblin"]),
        TokenName::Specified(EnglishCardName::new("Banner".into()).unwrap()),
        &v,
    )
    .unwrap();
    assert_eq!(name(&named.materialize(&v).unwrap()), "Banner");
    let empty = TokenDefinition::new(
        Program::artifact(None, vec![]),
        TokenName::Default {
            ordered_subtypes: vec![],
        },
        &v,
    )
    .unwrap();
    assert_eq!(name(&empty.materialize(&v).unwrap()), "Token");
}
#[test]
fn template_admission_rejects_duplicate_missing_extra_and_materialized_names() {
    let v = selected_vocabulary().unwrap();
    let program = creature(&v, &["Elf"]);
    assert_eq!(
        TokenDefinition::new(program.clone(), default_name(&v, &["Elf", "Elf"]), &v),
        Err(TokenDefinitionError::DuplicateSubtype)
    );
    for labels in [vec![], vec!["Goblin"], vec!["Elf", "Goblin"]] {
        assert_eq!(
            TokenDefinition::new(program.clone(), default_name(&v, &labels), &v),
            Err(TokenDefinitionError::SubtypeCoverage)
        );
    }
    let mut named = program;
    named.names = ObjectNames::single(EnglishCardName::new("Elf Token".into()).unwrap());
    assert_eq!(
        TokenDefinition::new(named, default_name(&v, &["Elf"]), &v),
        Err(TokenDefinitionError::TemplateHasNames)
    );
}
#[test]
fn text_changes_rewrite_default_type_names_but_preserve_specified_and_born_names() {
    let v = selected_vocabulary().unwrap();
    let change = CreatureTypeTextChange {
        from: v.resolve_label("Elf").unwrap(),
        to: v.resolve_label("Goblin").unwrap(),
    };
    let mut generated = TokenDefinition::new(
        creature(&v, &["Elf", "Goblin"]),
        default_name(&v, &["Elf", "Goblin"]),
        &v,
    )
    .unwrap();
    let born = generated.materialize(&v).unwrap();
    generated.rewrite_creature_types(change);
    assert_eq!(name(&generated.materialize(&v).unwrap()), "Goblin Token");
    assert_eq!(
        generated.program().creature_subtypes,
        BTreeSet::from([change.to])
    );
    assert_eq!(name(&born), "Elf Goblin Token");
    let mut specified = TokenDefinition::new(
        creature(&v, &["Elf"]),
        TokenName::Specified(EnglishCardName::new("Elf Token".into()).unwrap()),
        &v,
    )
    .unwrap();
    specified.rewrite_creature_types(change);
    let born = specified.materialize(&v).unwrap();
    assert_eq!(name(&born), "Elf Token");
    assert_eq!(born.creature_subtypes, BTreeSet::from([change.to]));
}
#[test]
fn copied_creating_bodies_own_nested_templates_and_strict_codecs_retain_name_provenance() {
    let v = selected_vocabulary().unwrap();
    let definition =
        TokenDefinition::new(creature(&v, &["Elf"]), default_name(&v, &["Elf"]), &v).unwrap();
    let original = Program::instant(
        ManaCost::free(),
        ExecutableBody::new(
            BodyTarget::None,
            vec![Instruction::CreateTokens {
                creations: vec![crate::token::TokenCreation::one(definition)],
            }],
        ),
    );
    let encoded = codec::view(&original);
    assert_eq!(codec::decode(encoded.clone(), &v).unwrap(), original);
    let mut copy = original.clone();
    crate::characteristics::rewrite_program_creature_types(
        CreatureTypeTextChange {
            from: v.resolve_label("Elf").unwrap(),
            to: v.resolve_label("Goblin").unwrap(),
        },
        &mut copy,
    );
    let crate::program::ProgramKind::Spell(body) = &copy.kind else {
        panic!("spell")
    };
    let Instruction::CreateTokens { creations } = &body.body.instructions[0] else {
        panic!("creation")
    };
    assert_eq!(
        name(&creations[0].definition.materialize(&v).unwrap()),
        "Goblin Token"
    );
    assert_eq!(codec::view(&original), encoded);
    for (key, value) in [
        ("name", json!({"tag":"Default"})),
        (
            "name",
            json!({"tag":"Specified","name":"Elf Token","default":true}),
        ),
        ("unknown", json!(0)),
    ] {
        let mut bad = encoded.clone();
        bad["kind"]["body"]["instructions"][0]["creations"][0]["definition"][key] = value;
        assert!(codec::decode(bad, &v).is_err());
    }
}
#[test]
fn effect_defined_colors_are_distinct_from_cost_indicator_and_explicit_colorless() {
    let v = selected_vocabulary().unwrap();
    let mut program =
        Program::artifact(Some(ManaCost::specific(ManaType::Green, 1.into())), vec![]);
    program.color_indicator = Some(BTreeSet::from([Color::Blue]));
    assert_eq!(program.defined_colors(), BTreeSet::from([Color::Blue]));
    for colors in [BTreeSet::new(), BTreeSet::from([Color::Red])] {
        program.effect_defined_colors = Some(colors.clone());
        assert_eq!(program.defined_colors(), colors);
        let encoded = codec::view(&program);
        assert_eq!(codec::decode(encoded.clone(), &v).unwrap(), program);
        let mut missing = encoded;
        missing
            .as_object_mut()
            .unwrap()
            .remove("effect_defined_colors");
        assert!(codec::decode(missing, &v).is_err());
    }
    program.effect_defined_colors = None;
    assert_eq!(program.defined_colors(), BTreeSet::from([Color::Blue]));
    program.color_indicator = None;
    assert_eq!(program.defined_colors(), BTreeSet::from([Color::Green]));
}

#[test]
fn counted_token_and_draw_share_strict_slot_codec_and_group_admission() {
    use crate::program::CreatureTypeSlot;
    let v = selected_vocabulary().unwrap();
    let slot = CreatureTypeSlot::new(0);
    let definition = TokenDefinition::new(
        Program::artifact(None, vec![]),
        TokenName::Default {
            ordered_subtypes: vec![],
        },
        &v,
    )
    .unwrap();
    let original = Program::instant(
        ManaCost::free(),
        ExecutableBody::new(
            BodyTarget::None,
            vec![
                Instruction::ChooseCreatureType {
                    into: slot,
                    excluding: BTreeSet::new(),
                },
                Instruction::CreateTokens {
                    creations: vec![TokenCreation {
                        definition,
                        amount: crate::program::ValueExpr::count_battlefield(
                            crate::program::ObjectFilter::any()
                                .controlled()
                                .of_chosen_slot(slot),
                        ),
                        base_size: None,
                        enters_tapped: false,
                        recipients: TokenRecipients::Controller,
                    }],
                },
                Instruction::DrawController {
                    amount: crate::program::ValueExpr::count_battlefield(
                        crate::program::ObjectFilter::any()
                            .controlled()
                            .of_chosen_slot(slot),
                    ),
                },
            ],
        ),
    );
    let encoded = codec::view(&original);
    assert_eq!(codec::decode(encoded.clone(), &v).unwrap(), original);
    assert_eq!(
        encoded["kind"]["body"]["instructions"][1]["creations"][0]["amount"]["chosen"],
        "0"
    );
    for value in [json!(0), json!("00"), json!("65536"), json!("not-a-slot")] {
        let mut bad = encoded.clone();
        bad["kind"]["body"]["instructions"][1]["creations"][0]["amount"]["chosen"] = value;
        assert!(codec::decode(bad, &v).is_err());
    }
    for field in ["amount", "definition", "recipients"] {
        let mut bad = encoded.clone();
        bad["kind"]["body"]["instructions"][1]["creations"][0]
            .as_object_mut()
            .unwrap()
            .remove(field);
        assert!(codec::decode(bad, &v).is_err());
    }
    let mut bad = encoded.clone();
    bad["kind"]["body"]["instructions"][1]["creations"] = json!([]);
    assert!(codec::decode(bad, &v).is_err());
    let mut bad = encoded.clone();
    bad["kind"]["body"]["instructions"][1]["creations"][0]["amount"] =
        json!({"tag":"Literal","value":"-1"});
    assert!(codec::decode(bad, &v).is_err());
    let mut bad = encoded;
    bad["kind"]["body"]["instructions"][1] = json!({"op":"CreateToken","definition":{}});
    assert!(codec::decode(bad, &v).is_err());
}
