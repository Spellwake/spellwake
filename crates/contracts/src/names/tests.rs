use super::*;
fn names(labels: &[&str]) -> ObjectNames {
    ObjectNames::new(
        labels
            .iter()
            .map(|x| EnglishCardName::new((*x).into()).unwrap())
            .collect(),
    )
    .unwrap()
}
#[test]
fn exact_labels_and_strict_sets_reject_malformed_input() {
    for label in ["", " Elf", "Elf ", "Elf\nVampire"] {
        assert_eq!(
            EnglishCardName::new(label.into()),
            Err(NameDefinitionError::InvalidLabel)
        );
    }
    for label in [
        "C’tan",
        "SP//dr, Piloted by Peni",
        "Imp's Mischief",
        "IMP'S MSCHF",
    ] {
        assert_eq!(EnglishCardName::new(label.into()).unwrap().as_str(), label);
    }
    assert_eq!(
        ObjectNames::new(vec![EnglishCardName::new("Elf".into()).unwrap(); 2]),
        Err(NameDefinitionError::DuplicateName)
    );
    assert!(serde_json::from_str::<ObjectNames>("[\"Elf\",\"Elf\"]").is_err());
    assert!(serde_json::from_str::<ObjectNames>("[\"\"]").is_err());
    assert_eq!(
        serde_json::to_string(&names(&["Z", "A"])).unwrap(),
        "[\"A\",\"Z\"]"
    );
    assert_eq!(
        serde_json::from_str::<ObjectNames>("[]").unwrap(),
        ObjectNames::nameless()
    );
}
#[test]
fn overlap_is_symmetric_but_neither_transitive_nor_global_pairwise_intersection() {
    let rules = NameRules::literal_names();
    let a = names(&["A"]);
    let ab = names(&["A", "B"]);
    let b = names(&["B"]);
    let bc = names(&["B", "C"]);
    let ac = names(&["A", "C"]);
    assert!(rules.shares_name(&a, &ab));
    assert!(rules.shares_name(&ab, &a));
    assert!(rules.shares_name(&ab, &b));
    assert!(!rules.shares_name(&a, &b));
    assert!(
        rules.shares_name(&ab, &bc) && rules.shares_name(&ab, &ac) && rules.shares_name(&bc, &ac)
    );
    assert!(!rules.common_name(&ab, &bc, &[&ac]));
    assert!(rules.common_name(&a, &ab, &[&ac]));
}
#[test]
fn nameless_and_group_direction_contracts_are_distinct() {
    let r = NameRules::literal_names();
    let empty = ObjectNames::nameless();
    let a = names(&["A"]);
    let b = names(&["B"]);
    assert!(!r.shares_name(&empty, &empty));
    assert!(!r.different_from(&empty, &[&a]));
    assert!(r.different_from(&a, &[&empty]));
    assert!(!r.pairwise_different(&[&a, &empty]));
    assert!(r.pairwise_different(&[&a, &b]));
    assert!(r.pairwise_different(&[]));
    assert!(r.pairwise_different(&[&a]));
    assert!(!r.pairwise_different(&[&empty]));
    assert!(r.different_from(&a, &[]));
    assert!(!r.different_from(&empty, &[]));
    assert!(!r.common_name(&a, &empty, &[]));
}
#[test]
fn explicit_interchangeable_forms_match_without_erasing_forms_or_inferring_titles() {
    let c = names(&["Cecily, Haunted Mage"]);
    let e = names(&["Eleven, the Mage"]);
    let rules = NameRules::new(vec![names(&["Eleven, the Mage", "Cecily, Haunted Mage"])]).unwrap();
    assert_ne!(c, e);
    assert!(rules.shares_name(&c, &e));
    assert!(!rules.pairwise_different(&[&c, &e]));
    assert!(rules.common_name(&c, &e, &[]));
    assert!(!rules.shares_name(
        &names(&["Zilortha, Strength Incarnate"]),
        &names(&["Godzilla, King of the Monsters"])
    ));
    assert_eq!(c.iter().next().unwrap().as_str(), "Cecily, Haunted Mage");
}
#[test]
fn relation_identity_is_value_based_and_invalid_classes_are_rejected() {
    let first = NameRules::new(vec![names(&["B", "A"]), names(&["D", "C"])]).unwrap();
    let reordered = NameRules::new(vec![names(&["C", "D"]), names(&["A", "B"])]).unwrap();
    assert_eq!(first, reordered);
    assert_eq!(first.id(), reordered.id());
    assert_ne!(first.id(), NameRules::literal_names().id());
    assert_eq!(
        NameRules::new(vec![names(&["A"])]),
        Err(NameDefinitionError::TooSmallClass)
    );
    assert_eq!(
        NameRules::new(vec![names(&["A", "B"]), names(&["B", "C"])]),
        Err(NameDefinitionError::OverlappingClasses)
    );
    let mut held = names(&["A"]);
    held.add(&names(&["A", "B"]));
    assert_eq!(held, names(&["A", "B"]));
}

#[test]
fn comparison_forms_are_pure_deduplicated_and_deterministic_under_only_the_supplied_relation() {
    let rules = NameRules::new(vec![names(&["Alternate", "Alpha"])]).unwrap();
    let held = names(&["Zulu", "Alternate", "Alpha", "Beta"]);
    let before = held.clone();
    let forms = || {
        rules
            .comparison_forms(&held)
            .into_iter()
            .map(EnglishCardName::as_str)
            .collect::<Vec<_>>()
    };
    assert_eq!(forms(), ["Alpha", "Beta", "Zulu"]);
    assert_eq!(forms(), ["Alpha", "Beta", "Zulu"]);
    assert_eq!(held, before);
    assert!(rules.comparison_forms(&ObjectNames::nameless()).is_empty());
    assert_eq!(NameRules::literal_names().comparison_forms(&held).len(), 4);
}
