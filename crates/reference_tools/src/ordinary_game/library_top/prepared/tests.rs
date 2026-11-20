use super::*;

#[test]
fn exact_input_identity_and_closed_case_grammar_are_required() {
    let input = parse(AUTHORED_INPUT).unwrap();
    assert_eq!(input.cases.len(), 8);
    let mut changed = AUTHORED_INPUT.to_vec();
    changed.push(b'\n');
    assert!(parse(&changed).is_err());
    assert!(packet::unique_json(br#"{"case_id":"LT01","case_id":"LT02"}"#).is_err());
    let mut changed = packet::unique_json(AUTHORED_INPUT).unwrap();
    changed["cases"][0]["expected_state"] = json!({});
    assert!(serde_json::from_value::<Input>(changed).is_err());
    let mut changed = packet::unique_json(AUTHORED_INPUT).unwrap();
    changed["definitions"]["Opt"]["Program"] = json!({});
    assert!(serde_json::from_value::<Input>(changed).is_err());
}

#[test]
fn authored_movement_to_native_top_first_translation_is_before_execution() {
    let input = parse(AUTHORED_INPUT).unwrap();
    for (ordinal, owner) in [(4, "A"), (5, "B")] {
        let authored = &input.cases[ordinal].graveyard_movement_order;
        assert_eq!(authored, &[format!("{owner}.L2"), format!("{owner}.L1")]);
        assert_eq!(
            authored.iter().rev().collect::<Vec<_>>(),
            vec![&format!("{owner}.L1"), &format!("{owner}.L2")]
        );
    }
    assert!(input.cases[6].graveyard_movement_order.is_empty());
    assert!(input.cases[7].graveyard_movement_order.is_empty());
    assert_eq!(input.cases[6].a_library_top_first.len(), 2);
    assert_eq!(input.cases[7].a_library_top_first.len(), 2);
}

#[test]
#[ignore = "requires explicit genuine full metadata/gzip/native vocabulary inputs"]
fn prepared_consumer_rejects_source_pin_and_unused_native_binding_before_execution() {
    let path = |name| {
        std::env::var_os(name)
            .map(std::path::PathBuf::from)
            .expect("explicit genuine input")
    };
    let mut source = LibraryTopSource::load(
        &path("SPELLWAKE_CATALOGUE_METADATA"),
        &path("SPELLWAKE_CATALOGUE_INPUT"),
        crate::creature_vocabulary::load_vocabulary(&path("SPELLWAKE_CREATURE_VOCABULARY_INPUT"))
            .unwrap(),
    )
    .unwrap();
    let mut input = parse(AUTHORED_INPUT).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let pin = &mut input.definitions.get_mut("Opt").unwrap().source_row;
    let mut changed = std::fs::read(&pin.path).unwrap();
    changed[0] ^= 1;
    let file = dir.path().join("changed-source-row");
    std::fs::write(&file, changed).unwrap();
    pin.path = file.to_str().unwrap().into();
    assert!(
        matches!(source_admission(&source, &input), Err(PreparedError::Admission(message)) if message == "prepared source-row file pin mismatch")
    );
    // LT01 physically uses Opt and Island, but an unused Consider binding must
    // still pass complete source4 validation before preparing that first case.
    source
        .inventory
        .cards
        .iter_mut()
        .find(|row| row.oracle_id.as_deref() == Some(super::super::CONSIDER))
        .unwrap()
        .scryfall_id = "00000000-0000-0000-0000-000000000000".into();
    assert!(matches!(
        execute(&source, AUTHORED_INPUT),
        Err(PreparedError::Source(LibraryTopSourceError::Registry(_)))
    ));
}
