//! Guard catalogue identity/signature boundaries rather than treating text presence as support.
use spellwake_card_programs::{
    BOLT, BindingStatus, COUNTERSPELL, LION, OracleId, ProgramKey, Registry, RegistryError, Scope,
    UNSUMMON,
};
use spellwake_content::{Inventory, snapshot::decode};
use spellwake_engine::{
    card_identity::{CardDestination, CardId},
    effects::{Instruction, Program},
    mana::{ManaCost, ManaType},
    priority::SeatId,
};
fn inventory() -> Inventory {
    decode(
        include_bytes!("../../../tests/catalogue/registry-cards.meta.json"),
        include_bytes!("../../../tests/catalogue/registry-cards.jsonl.gz").to_vec(),
    )
    .unwrap()
}

#[test]
fn absent_printed_mana_cost_never_becomes_an_authored_free_cost() {
    let mut source = inventory();
    let card = source
        .cards
        .iter_mut()
        .find(|card| card.oracle_id.as_deref() == Some(BOLT))
        .unwrap();
    card.faces[0].mana_cost = None;
    let registry = Registry::from_inventory(&source).unwrap();
    assert!(registry.resolve(&id(BOLT)).is_none());
    let coverage = registry.coverage();
    let card = coverage
        .cards
        .iter()
        .find(|card| card.oracle_id.as_deref() == Some(BOLT));
    assert!(matches!(
        card.unwrap().binding,
        BindingStatus::Blocked { .. }
    ));
}
fn id(value: &str) -> OracleId {
    value.to_owned().try_into().unwrap()
}

#[test]
fn known_identities_bind_versioned_partial_programs_without_claiming_behavior_or_full_support() {
    let registry = Registry::from_inventory(&inventory()).unwrap();
    let coverage = registry.coverage();
    assert_eq!(
        (
            coverage.source_records,
            coverage.required_records,
            coverage.required_partial_bindings,
            coverage.required_without_binding
        ),
        (4, 4, 4, 0)
    );
    assert_eq!(coverage.fully_qualified_cards, 0);
    assert_eq!(coverage.behavior_evidence, "not-evaluated-by-registry");
    assert_eq!(
        registry.resolve(&id(BOLT)).unwrap().key(),
        ProgramKey::BoltCreatureV1
    );
    assert_eq!(
        registry.resolve(&id(UNSUMMON)).unwrap().scope(),
        Scope::CreatureTargetOnly
    );
    assert_eq!(
        registry.resolve(&id(LION)).unwrap().scope(),
        Scope::SeededVanillaBattlefieldOnly
    );
    let bolt = registry
        .seed(&id(BOLT), CardId(99), SeatId(1), CardDestination::Hand)
        .unwrap();
    assert_eq!(
        bolt.program,
        Program::Instant {
            cost: ManaCost::one(ManaType::Red),
            instructions: vec![Instruction::Damage { amount: 3.into() }]
        }
    );
    let counter = registry
        .seed(
            &id(COUNTERSPELL),
            CardId(100),
            SeatId(1),
            CardDestination::Hand,
        )
        .unwrap();
    assert_eq!(
        counter.program,
        Program::Instant {
            cost: ManaCost::specific(ManaType::Blue, 2.into()),
            instructions: vec![Instruction::CounterSpell]
        }
    );
    assert_eq!(
        registry.resolve(&id(COUNTERSPELL)).unwrap().scope(),
        Scope::SupportedInstantSpellTargetOnly
    );
    let lion = registry
        .seed(
            &id(LION),
            CardId(2),
            SeatId(1),
            CardDestination::Battlefield {
                controller: SeatId(0),
            },
        )
        .unwrap();
    assert_eq!(lion.owner, SeatId(1));
    assert_eq!(
        lion.program,
        Program::VanillaCreature {
            power: 2.into(),
            toughness: 2.into()
        }
    );
}

#[test]
fn oracle_ids_are_canonical_and_distinct_from_names_or_printing_ids() {
    let oracle = id(BOLT);
    assert_eq!(
        serde_json::to_string(&oracle).unwrap(),
        format!("\"{BOLT}\"")
    );
    for invalid in [
        "Lightning Bolt".to_owned(),
        "".to_owned(),
        BOLT.to_uppercase(),
        format!(" {BOLT}"),
        BOLT.replace('-', ""),
        "é".repeat(18),
    ] {
        assert!(OracleId::try_from(invalid.clone()).is_err());
        assert!(serde_json::from_value::<OracleId>(serde_json::json!(invalid)).is_err());
    }
    let mut source = inventory();
    source.cards[0].oracle_id = Some("00000000-0000-0000-0000-000000000001".into());
    let registry = Registry::from_inventory(&source).unwrap();
    assert!(registry.resolve(&id(BOLT)).is_none());
    assert!(matches!(
        registry.coverage().cards[0].binding,
        BindingStatus::Missing
    ));
}

#[test]
fn changed_rules_or_characteristics_block_execution_instead_of_reusing_an_old_program() {
    for field in [
        "name",
        "layout",
        "face-name",
        "type",
        "mana",
        "text",
        "power",
        "toughness",
        "faces",
    ] {
        let mut source = inventory();
        let card = &mut source.cards[0];
        match field {
            "name" => card.name = "Other".into(),
            "layout" => card.layout = "transform".into(),
            "face-name" => card.faces[0].name = "Other".into(),
            "type" => card.faces[0].type_line = Some("Sorcery".into()),
            "mana" => card.faces[0].mana_cost = Some("{1}{R}".into()),
            "text" => {
                card.faces[0].oracle_text =
                    Some("Lightning Bolt deals 4 damage to any target.".into())
            }
            "power" => card.faces[0].power = Some("3".into()),
            "toughness" => card.faces[0].toughness = Some("3".into()),
            "faces" => card.faces.clear(),
            _ => unreachable!(),
        }
        let registry = Registry::from_inventory(&source).unwrap();
        assert!(registry.resolve(&id(BOLT)).is_none(), "{field}");
        assert_eq!(registry.coverage().required_partial_bindings, 3);
        assert!(matches!(
            registry.coverage().cards[0].binding,
            BindingStatus::Blocked {
                reason: "semantic-guard-mismatch"
            }
        ));
        assert!(matches!(
            registry.seed(&id(BOLT), CardId(1), SeatId(0), CardDestination::Hand),
            Err(RegistryError::MissingBinding)
        ));
    }
    let mut source = inventory();
    source.cards[1].faces[0].power = Some("3".into());
    assert!(
        Registry::from_inventory(&source)
            .unwrap()
            .resolve(&id(LION))
            .is_none()
    );
}

#[test]
fn duplicate_or_missing_oracle_identities_are_ambiguous_not_first_match_wins() {
    let mut source = inventory();
    source.cards[1].oracle_id = Some(BOLT.into());
    let registry = Registry::from_inventory(&source).unwrap();
    assert!(registry.resolve(&id(BOLT)).is_none());
    for card in &registry.coverage().cards[..2] {
        assert!(matches!(
            card.binding,
            BindingStatus::Blocked {
                reason: "ambiguous-oracle-id"
            }
        ));
    }
    for invalid in [None, Some(" ".to_owned())] {
        let mut source = inventory();
        source.cards[0].oracle_id = invalid;
        let registry = Registry::from_inventory(&source).unwrap();
        assert!(registry.resolve(&id(BOLT)).is_none());
        assert!(matches!(
            registry.coverage().cards[0].binding,
            BindingStatus::Blocked {
                reason: "missing-or-invalid-oracle-id"
            }
        ));
    }
}

#[test]
fn fingerprint_tracks_rules_data_not_printing_or_format_metadata_and_coverage_counts_only_required_rows()
 {
    let mut source = inventory();
    let initial = Registry::from_inventory(&source).unwrap();
    source.cards[0].scryfall_id = "different-printing".into();
    source.cards[0].legalities.clear();
    source.cards[0].required_by_base_formats = false;
    let registry = Registry::from_inventory(&source).unwrap();
    assert_eq!(
        initial.coverage().cards[0].binding,
        registry.coverage().cards[0].binding
    );
    assert_eq!(
        (
            registry.coverage().required_records,
            registry.coverage().required_partial_bindings
        ),
        (3, 3)
    );
    assert!(registry.resolve(&id(BOLT)).is_some()); // Program semantics and format admission are separate.
}

#[test]
fn seeded_battlefield_scope_is_enforced_and_does_not_admit_creature_casting() {
    let registry = Registry::from_inventory(&inventory()).unwrap();
    assert!(matches!(
        registry.seed(&id(LION), CardId(1), SeatId(0), CardDestination::Hand),
        Err(RegistryError::UnsupportedPlacement)
    ));
    assert!(matches!(
        registry.seed(
            &id("021d654e-b731-40d0-a18e-eaff43f9fdb9"),
            CardId(1),
            SeatId(0),
            CardDestination::Hand
        ),
        Err(RegistryError::MissingBinding)
    )); // New Blood is not a simplified steal program.
}

#[test]
fn twincast_guard_binds_only_its_exact_source_to_partial_supported_instant_semantics() {
    let mut source = decode(
        include_bytes!("../../../tests/catalogue/spell-copy-cards.meta.json"),
        include_bytes!("../../../tests/catalogue/spell-copy-cards.jsonl.gz").to_vec(),
    )
    .unwrap();
    let registry = Registry::from_inventory(&source).unwrap();
    let twincast = id(spellwake_card_programs::TWINCAST);
    assert_eq!(registry.coverage().required_partial_bindings, 5);
    assert_eq!(registry.coverage().registry_version, 3);
    assert_eq!(
        registry.resolve(&twincast).unwrap().key(),
        ProgramKey::TwincastSingleTargetInstantV1
    );
    let seed = registry
        .seed(&twincast, CardId(100), SeatId(1), CardDestination::Hand)
        .unwrap();
    assert!(matches!(
        seed.program,
        Program::Instant {
            instructions,
            ..
        } if instructions == vec![Instruction::CopySpell]
    ));
    source
        .cards
        .iter_mut()
        .find(|card| card.oracle_id.as_deref() == Some(spellwake_card_programs::TWINCAST))
        .unwrap()
        .faces[0]
        .oracle_text = Some("Copy target spell.".into());
    assert!(
        Registry::from_inventory(&source)
            .unwrap()
            .resolve(&twincast)
            .is_none()
    );
}
