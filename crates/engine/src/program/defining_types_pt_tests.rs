use super::*;
use crate::{
    characteristics::{
        DefiningCreatureTypes, DefiningPart, DefiningParts, NumericExpression, PermanentSelector,
        PrintedPowerToughness, PrintedStat, StaticAbilityId,
    },
    test_support::{creature_types::selected_vocabulary, program as codec},
};
use serde_json::json;

fn program(parts: Vec<DefiningPart>) -> Program {
    let mut p = Program::creature(Some(ManaCost::free()), 0.into(), 1.into(), BTreeSet::new());
    let ProgramKind::Permanent(d) = &mut p.kind else {
        unreachable!()
    };
    d.printed_pt = Some(PrintedPowerToughness {
        power: PrintedStat::Star {
            multiplier: 1.into(),
            offset: 0.into(),
        },
        toughness: PrintedStat::Star {
            multiplier: 1.into(),
            offset: 1.into(),
        },
    });
    p.continuous = Some(StaticContinuousAbility::CharacteristicDefining {
        id: StaticAbilityId(1),
        parts: DefiningParts(parts),
    });
    p
}
fn pt_part() -> DefiningPart {
    DefiningPart::PowerToughness {
        power: Some(Box::new(NumericExpression::count(
            PermanentSelector::default(),
        ))),
        toughness: None,
    }
}

#[test]
fn defining_parts_printed_stars_and_explicit_count_transforms_roundtrip() {
    let v = selected_vocabulary().unwrap();
    for types in [
        DefiningCreatureTypes::All,
        DefiningCreatureTypes::Literal(BTreeSet::from([v.resolve_label("Elf").unwrap()])),
    ] {
        let p = program(vec![
            DefiningPart::CreatureTypes(types),
            DefiningPart::Colors(BTreeSet::new()),
            pt_part(),
        ]);
        let raw = codec::view(&p);
        assert_eq!(codec::decode(raw.clone(), &v).unwrap(), p);
        assert_eq!(
            raw["kind"]["printed_pt"]["toughness"],
            json!({"tag":"Star","multiplier":"1","offset":"1"})
        );
        assert_eq!(raw["continuous"]["parts"][2]["power"]["multiplier"], "1");
        assert_eq!(raw["continuous"]["parts"][2]["power"]["offset"], "0");
        assert!(raw["continuous"]["parts"][2]["toughness"].is_null());
    }
}

#[test]
fn owner_rejects_empty_duplicate_and_empty_pt_defining_parts() {
    let v = selected_vocabulary().unwrap();
    for parts in [
        vec![],
        vec![pt_part(), pt_part()],
        vec![
            DefiningPart::CreatureTypes(DefiningCreatureTypes::All),
            DefiningPart::CreatureTypes(DefiningCreatureTypes::Literal(BTreeSet::new())),
        ],
        vec![DefiningPart::PowerToughness {
            power: None,
            toughness: None,
        }],
    ] {
        let p = program(parts);
        assert_eq!(p.validate(&v), Err(DefinitionError::ContinuousEffect));
        assert!(codec::decode(codec::view(&p), &v).is_err());
    }
}

#[test]
fn defining_literals_and_count_predicates_reject_foreign_vocabulary_or_contradictions() {
    let v = selected_vocabulary().unwrap();
    let foreign = crate::test_support::creature_types::synthetic_vocabulary()
        .resolve_label("Elf")
        .unwrap();
    assert_ne!(foreign.vocabulary(), v.id());
    for part in [
        DefiningPart::CreatureTypes(DefiningCreatureTypes::Literal(BTreeSet::from([foreign]))),
        DefiningPart::PowerToughness {
            power: Some(Box::new(NumericExpression::count(PermanentSelector {
                creature_subtype: Some(foreign),
                ..PermanentSelector::default()
            }))),
            toughness: None,
        },
        DefiningPart::PowerToughness {
            power: Some(Box::new(NumericExpression::count(PermanentSelector {
                required_supertypes: std::collections::BTreeSet::new(),
                forbidden_supertypes: std::collections::BTreeSet::new(),
                required_types: BTreeSet::from([CardType::Land]),
                forbidden_types: BTreeSet::from([CardType::Land]),
                ..PermanentSelector::default()
            }))),
            toughness: None,
        },
    ] {
        assert_eq!(
            program(vec![part]).validate(&v),
            Err(DefinitionError::ContinuousEffect)
        );
    }
}

#[test]
fn printed_stat_codec_requires_tagged_exact_formulas_and_current_field() {
    let v = selected_vocabulary().unwrap();
    let raw = codec::view(&program(vec![pt_part()]));
    for bad in [
        json!("*"),
        json!(1),
        json!({"tag":"Number","value":1}),
        json!({"tag":"Star","offset":"1"}),
        json!({"tag":"Star","multiplier":"1"}),
        json!({"tag":"Star","multiplier":"1","offset":"00"}),
        json!({"tag":"Star","multiplier":"1","offset":"0","computed":"7"}),
        json!({"tag":"Unknown"}),
    ] {
        let mut x = raw.clone();
        x["kind"]["printed_pt"]["power"] = bad;
        assert!(codec::decode(x, &v).is_err());
    }
    let mut missing = raw.clone();
    missing["kind"]
        .as_object_mut()
        .unwrap()
        .remove("printed_pt");
    assert!(codec::decode(missing, &v).is_err());
    let mut old = raw;
    let box_value = old["kind"]
        .as_object_mut()
        .unwrap()
        .remove("printed_pt")
        .unwrap();
    old["kind"]["literal_pt"] = box_value;
    assert!(codec::decode(old, &v).is_err());
}

#[test]
fn defining_codec_rejects_ordinary_parts_conditions_old_variants_and_missing_stat_fields() {
    let v = selected_vocabulary().unwrap();
    let raw = codec::view(&program(vec![pt_part()]));
    for part in [
        json!({"tag":"SetPT","power":"2","toughness":"2"}),
        json!({"tag":"Colors","colors":[],"condition":"Always"}),
        json!({"tag":"PowerToughness","power":null}),
        json!({"tag":"CreatureTypes","types":{"tag":"All","types":[]}}),
        json!({"tag":"Colors","colors":["Blue","Blue"]}),
    ] {
        let mut x = raw.clone();
        x["continuous"]["parts"] = json!([part]);
        assert!(codec::decode(x, &v).is_err());
    }
    let mut old = raw;
    old["continuous"] = json!({"tag":"DefineColors","id":1,"colors":[]});
    assert!(codec::decode(old, &v).is_err());
}

#[test]
fn count_wire_never_injects_constructor_defaults_and_preserves_huge_signed_values() {
    let v = selected_vocabulary().unwrap();
    let raw = codec::view(&program(vec![pt_part()]));
    for key in ["multiplier", "offset"] {
        let mut missing = raw.clone();
        missing["continuous"]["parts"][0]["power"]
            .as_object_mut()
            .unwrap()
            .remove(key);
        assert!(codec::decode(missing, &v).is_err());
        let mut number = raw.clone();
        number["continuous"]["parts"][0]["power"][key] = json!(1);
        assert!(codec::decode(number, &v).is_err());
    }
    let mut exact = raw;
    exact["continuous"]["parts"][0]["power"]["multiplier"] = json!("-90071992547409931234567890");
    exact["continuous"]["parts"][0]["power"]["offset"] = json!("90071992547409931234567891");
    assert_eq!(
        codec::view(&codec::decode(exact.clone(), &v).unwrap()),
        exact
    );
}
