use super::*;

fn vocabulary() -> CreatureTypeVocabulary {
    CreatureTypeVocabulary::new(
        "wave12-prevention-followup".into(),
        [0; 32],
        vec!["Human".into()],
    )
    .unwrap()
}

fn prevention() -> InstructionAst {
    InstructionAst::PreventAllDamageFrom {
        combat_only: false,
        source: Box::new(SubjectRefAst::Group(Box::new(
            spellwake_oracle_compiler::filter::parse_object_filter(
                "creatures",
                &cached_compiler_vocab(&vocabulary()),
            )
            .unwrap(),
        ))),
        recipient: None,
        duration: DurationAst::UntilEndOfTurn,
    }
}

#[test]
fn wave12_prevention_rejects_dependent_life_but_preserves_independent_life() {
    for delta in [
        ValueExprAst::PriorResult,
        ValueExprAst::Add(
            Box::new(ValueExprAst::PriorResult),
            Box::new(ValueExprAst::Const(1)),
        ),
    ] {
        let mut builder = BodyBuilder::new();
        let result = lower_instruction_seq(
            &[
                prevention(),
                InstructionAst::ChangeLife {
                    player: PlayerRefAst::You,
                    delta: Box::new(delta),
                    gain: true,
                },
            ],
            &mut builder,
            &vocabulary(),
            &mut LoweringIds::default(),
            false,
            false,
            None,
        );
        assert_eq!(result, Err("oracle:unsupported-prevention-life-followup"));
        assert!(builder.instructions.is_empty());
    }
    let mut builder = BodyBuilder::new();
    lower_instruction_seq(
        &[
            prevention(),
            InstructionAst::ChangeLife {
                player: PlayerRefAst::You,
                delta: Box::new(ValueExprAst::Const(3)),
                gain: true,
            },
        ],
        &mut builder,
        &vocabulary(),
        &mut LoweringIds::default(),
        false,
        false,
        None,
    )
    .unwrap();
    assert!(matches!(builder.instructions.as_slice(), [
        Instruction::ApplyContinuous(prev_spec),
        Instruction::AdjustLife(life_spec),
    ] if matches!(
        prev_spec.payload,
        spellwake_engine::toolbox::ContinuousPayload::PreventDamage(_)
    ) && matches!(
        &life_spec.op,
        spellwake_engine::toolbox::LifeOp::Gain(amount) if *amount == ValueExpr::literal(3)
    )));
}

#[test]
fn wave12_nested_prevention_followup_rejects_before_emitting_optional_body() {
    let mut builder = BodyBuilder::new();
    let nested = InstructionAst::Optional(vec![
        prevention(),
        InstructionAst::ChangeLife {
            player: PlayerRefAst::You,
            delta: Box::new(ValueExprAst::PriorResult),
            gain: true,
        },
    ]);
    assert!(
        lower_instruction_seq(
            &[nested],
            &mut builder,
            &vocabulary(),
            &mut LoweringIds::default(),
            false,
            false,
            None
        )
        .is_err()
    );
}
#[test]
fn wave12_chant_of_vitu_ghazi_fails_closed_without_prevention_event_followup() {
    let card = InventoryCard {
        scryfall_id: "00000000-0000-0000-0000-000000000000".into(),
        oracle_id: Some("e245a736-5f65-4159-8e39-e279e1f8794f".into()),
        name: "Chant of Vitu-Ghazi".into(), layout: "normal".into(),
        faces: vec![spellwake_content::CatalogFace {
            name: "Chant of Vitu-Ghazi".into(), type_line: Some("Instant".into()),
            oracle_text: Some("Convoke (Your creatures can help cast this spell. Each creature you tap while casting this spell pays for {1} or one mana of that creature's color.)\nPrevent all damage that would be dealt by creatures this turn. You gain life equal to the damage prevented this way.".into()),
            mana_cost: Some("{6}{W}{W}".into()), power: None, toughness: None,
        }],
        legalities: BTreeMap::new(), eligible_formats: Vec::new(), required_by_base_formats: true,
        program_evidence: "test",
    };
    assert_eq!(
        compile_with_reason(&card, &vocabulary()).unwrap_err(),
        "oracle:unsupported-prevention-life-followup"
    );
}
