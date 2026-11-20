use super::*;

const PINNED_LINE: &str = include_str!("../../tests/fixtures/creature-section.input.txt");
const INPUT_VECTOR: &str = include_str!("../../tests/fixtures/creature-vocabulary.input.json");
const INPUT_TOKENS: &str = include_str!("../../tests/fixtures/creature-source-tokens.input.json");

fn known_source() -> CreatureTypeSource {
    CreatureTypeSource {
        rules_revision: RULES_REVISION.into(),
        rules_sha256: RULES_SHA256,
    }
}

// An independently prepared receipt shape, not the result of a complete-source compile.
pub(super) fn prepared_dto() -> serde_json::Value {
    let vector: serde_json::Value = serde_json::from_str(INPUT_VECTOR).unwrap();
    let tokens: serde_json::Value = serde_json::from_str(INPUT_TOKENS).unwrap();
    serde_json::json!({
        "tag": "Complete",
        "publication": {
            "schema_version": 1,
            "kind": "source-qualified-creature-type-publication",
            "source": {"rules_revision": "CR-20260925", "rules_sha256": "8d860e451f20f38865b725b42d82feb714c725373dd8f3b32b8652b3eeb070ca", "section": "205.3m"},
            "section": {
                "physical_lf_line": "1443",
                "source_byte_start": "164082",
                "source_byte_end_exclusive": "166965",
                "raw_line_utf8": PINNED_LINE,
                "raw_line_sha256": "8824f7c20b514512b6fc0d074ffe7b20951ae5cc9a416319d8dcca545f6d0379",
                "line_ending_hex": "0a",
                "consumed_bytes": "2883",
                "unconsumed_bytes": "0",
                "tokens": tokens["tokens"]
            },
            "members": vector["entries"],
            "identity": {"algorithm": "spellwake.creature-types.v1", "framed_bytes": "3423", "vocabulary_id": "39721c01eef9fd3f869588b097651bb6115f9cdde5d61bf822d65a8e5435c792"},
            "provider_subtype_aliases": [
                {"provider_raw":"C'tan", "canonical_label":"C’tan", "scope":"provider-subtype-field"},
                {"provider_raw":"Shi'ar", "canonical_label":"Shi’ar", "scope":"provider-subtype-field"}
            ]
        }
    })
}

#[test]
fn vm01_unsupported_declared_source_has_no_partial_members_even_for_bad_bytes() {
    for declared in [
        CreatureTypeSource {
            rules_revision: "CR-20260926".into(),
            rules_sha256: RULES_SHA256,
        },
        CreatureTypeSource {
            rules_revision: RULES_REVISION.into(),
            rules_sha256: [0; 32],
        },
    ] {
        let result = compile(&[0xff], declared.clone()).unwrap();
        let CreatureTypeCompilation::Incomplete(incomplete) = &result else {
            panic!("unsupported source was published")
        };
        assert_eq!(
            incomplete.reason(),
            CreatureTypeIncompleteReason::UnsupportedSource
        );
        let dto = serde_json::to_value(&result).unwrap();
        assert_eq!(dto["tag"], "Incomplete");
        assert_eq!(
            dto["incomplete"]["declared_source"],
            serde_json::to_value(declared).unwrap()
        );
        for name in ["members", "vocabulary", "vocabulary_id", "publication"] {
            assert!(dto.get(name).is_none());
            assert!(dto["incomplete"].get(name).is_none());
        }
    }
}

#[test]
fn vm01_known_pin_bad_raw_bytes_reject_before_utf8_or_section_parsing() {
    let raw = [0xff];
    let actual = Sha256::digest(raw).into();
    assert_eq!(
        compile(&raw, known_source()),
        Err(CreatureTypeCompileError::SourceDigestMismatch {
            expected: RULES_SHA256,
            actual
        })
    );
    let error = serde_json::to_value(compile(&raw, known_source()).unwrap_err()).unwrap();
    assert_eq!(error["tag"], "SourceDigestMismatch");
    assert_eq!(error["expected"], hex(&RULES_SHA256));
    assert_eq!(error["actual"], hex(&actual));
}

#[test]
fn vm01_synthetic_utf8_helper_fault_is_not_a_full_source_publication() {
    assert_eq!(
        decode_source_utf8(&[0xff]),
        Err(CreatureTypeCompileError::InvalidUtf8 {
            valid_up_to: 0,
            error_len: Some(1)
        })
    );
    assert_eq!(decode_source_utf8(b"valid"), Ok("valid"));
    assert_eq!(
        decode_source_utf8(&[b'a', 0xe2, 0x82]),
        Err(CreatureTypeCompileError::InvalidUtf8 {
            valid_up_to: 1,
            error_len: None
        })
    );
}

#[test]
fn vm01_synthetic_locator_missing_and_ambiguous_sections_are_incomplete() {
    let missing = locate_creature_section("not a section\n").unwrap_err();
    assert_eq!(missing.reason, CreatureTypeIncompleteReason::MissingSection);
    let duplicate = format!("{PINNED_LINE}\n{PINNED_LINE}\n");
    let ambiguous = locate_creature_section(&duplicate).unwrap_err();
    assert_eq!(
        ambiguous.reason,
        CreatureTypeIncompleteReason::AmbiguousSection
    );
    let source = format!("é\n{PINNED_LINE}\nother\n");
    let positive = locate_creature_section(&source).unwrap();
    assert_eq!(positive.line, PINNED_LINE);
    assert_eq!(positive.line_ending, "\n");
    assert_eq!(positive.source_byte_start, 3);
    assert_eq!(positive.physical_lf_line, 2);
}

#[test]
fn vm01_synthetic_section_helper_accounts_every_token_without_publication_claim() {
    let mut parsed = parse_creature_section(PINNED_LINE).unwrap();
    parsed.members.sort();
    let vector: serde_json::Value = serde_json::from_str(INPUT_VECTOR).unwrap();
    let labels: Vec<_> = vector["entries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| entry["canonical_label"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(parsed.members, labels);
    assert_eq!(parsed.members.len(), 324);
    let shape =
        CreatureTypeVocabulary::new(RULES_REVISION.into(), RULES_SHA256, parsed.members).unwrap();
    let mut cursor = 0;
    for token in &mut parsed.tokens {
        assert_eq!(token.line_byte_start, cursor.to_string());
        cursor += token.raw_utf8.len();
        assert_eq!(token.line_byte_end_exclusive, cursor.to_string());
        if token.role == TokenRole::Member {
            token.member_ordinal = Some(
                shape
                    .resolve_label(&token.raw_utf8)
                    .unwrap()
                    .ordinal()
                    .to_string(),
            );
        } else {
            assert_eq!(token.member_ordinal, None);
        }
    }
    assert_eq!(cursor, 2883);
    assert_eq!(
        parsed
            .tokens
            .iter()
            .map(|token| token.raw_utf8.as_str())
            .collect::<String>(),
        PINNED_LINE
    );
    let expected_tokens: serde_json::Value = serde_json::from_str(INPUT_TOKENS).unwrap();
    assert_eq!(
        serde_json::to_value(parsed.tokens).unwrap(),
        expected_tokens["tokens"]
    );
    assert_eq!(shape.resolve_label("Time Lord").unwrap().ordinal(), 291);
    assert_eq!(shape.resolve_label("C'tan"), None);
    assert_eq!(shape.resolve_label("Shi'ar"), None);
}

#[test]
fn vm01_synthetic_parser_faults_are_exact_incompleteness_without_partial_output() {
    let cases = [
        (
            "S2C-VM01-012",
            PINNED_LINE.replacen("Advisor, Aetherborn", "Advisor,, Aetherborn", 1),
            CreatureTypeDiagnosticCode::MalformedSeparator,
        ),
        (
            "S2C-VM01-013",
            PINNED_LINE.replacen("Advisor, Aetherborn", "Advisor, Advisor", 1),
            CreatureTypeDiagnosticCode::DuplicateMember,
        ),
        (
            "S2C-VM01-014",
            PINNED_LINE.replacen(", and Zubera.", ", Zubera.", 1),
            CreatureTypeDiagnosticCode::MissingFinalConjunction,
        ),
        (
            "S2C-VM01-015",
            format!("{PINNED_LINE} trailing"),
            CreatureTypeDiagnosticCode::UnconsumedText,
        ),
        (
            "S2C-VM01-016",
            PINNED_LINE.replacen(
                "One creature type is two words long: Time Lord.",
                "One creature type is two words long: Time, Lord.",
                1,
            ),
            CreatureTypeDiagnosticCode::UnexpectedSentence,
        ),
        (
            "S2C-VM01-017",
            PINNED_LINE.replacen("Advisor, Aetherborn", "Advisor, Time Lord", 1),
            CreatureTypeDiagnosticCode::InvalidMemberToken,
        ),
    ];
    for (case, line, code) in cases {
        let failure = parse_creature_section(&line).unwrap_err();
        assert_eq!(
            failure.reason,
            CreatureTypeIncompleteReason::UnrecognizedSectionSyntax,
            "{case}"
        );
        assert_eq!(failure.diagnostics[0].code, code, "{case}");
        let result = incomplete(known_source(), failure);
        let dto = serde_json::to_value(result).unwrap();
        assert_eq!(dto["tag"], "Incomplete", "{case}");
        assert!(dto["incomplete"].get("members").is_none(), "{case}");
    }
}

struct BrokenWriter {
    attempts: usize,
}
impl io::Write for BrokenWriter {
    fn write(&mut self, _bytes: &[u8]) -> io::Result<usize> {
        self.attempts += 1;
        Err(io::ErrorKind::BrokenPipe.into())
    }
    fn flush(&mut self) -> io::Result<()> {
        panic!("failed writer must not be flushed")
    }
}

#[test]
fn vm01_prepared_dto_writer_failure_is_typed_and_never_retried() {
    // S2C-VM01-019: prepared DTO only, no pure compiler or actual CLI invocation.
    let response = ApiResponse::Success {
        data: prepared_dto(),
    };
    let mut writer = BrokenWriter { attempts: 0 };
    assert_eq!(
        write_output(&mut writer, &response),
        Err(CreatureTypeCliError::Serialization {
            io_kind: io::ErrorKind::BrokenPipe
        })
    );
    assert_eq!(writer.attempts, 1);
}

#[test]
fn vm01_missing_explicit_local_source_is_a_typed_read_failure() {
    let root = std::env::temp_dir().join(format!(
        "spellwake-creature-read-unit-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&root).unwrap();
    let missing = root.join("absent.txt");
    assert!(!missing.exists());
    assert_eq!(
        read_source(&missing),
        Err(CreatureTypeCliError::Read {
            io_kind: io::ErrorKind::NotFound
        })
    );
    std::fs::remove_dir(root).unwrap();
}

#[test]
fn writer_flush_failure_is_also_serialization_and_retains_written_prefix_honestly() {
    struct FlushFailure(Vec<u8>);
    impl io::Write for FlushFailure {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.0.extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Err(io::ErrorKind::BrokenPipe.into())
        }
    }
    let response = ApiResponse::Success {
        data: prepared_dto(),
    };
    let mut writer = FlushFailure(Vec::new());
    assert_eq!(
        write_output(&mut writer, &response),
        Err(CreatureTypeCliError::Serialization {
            io_kind: io::ErrorKind::BrokenPipe
        })
    );
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&writer.0).unwrap()["status"],
        "success"
    );
}

fn retained_full_source() -> Vec<u8> {
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../work/rules/MagicCompRules-20260925.txt");
    let bytes = read_source(&path).expect("explicit retained qualification source must exist");
    assert_eq!(<[u8; 32]>::from(Sha256::digest(&bytes)), RULES_SHA256);
    bytes
}

#[test]
#[ignore = "explicit source qualification against retained work/rules pin; run with --ignored"]
fn full_pin_publication_matches_independent_members_identity_and_source_token_ledger() {
    let result = compile(&retained_full_source(), known_source()).unwrap();
    let CreatureTypeCompilation::Complete(publication) = &result else {
        panic!("supported exact pin was not complete")
    };
    assert_eq!(publication.vocabulary().len(), 324);
    assert_eq!(
        publication.vocabulary().id().to_string(),
        "39721c01eef9fd3f869588b097651bb6115f9cdde5d61bf822d65a8e5435c792"
    );
    assert_eq!(serde_json::to_value(&result).unwrap(), prepared_dto());
    let clone = publication.clone().into_vocabulary();
    assert_eq!(&clone, publication.vocabulary());
}

#[test]
#[ignore = "explicit full-source hash controls against retained work/rules pin; run with --ignored"]
fn full_pin_controls_reject_all_changed_bytes_before_any_publication() {
    let bytes = retained_full_source();
    let source = std::str::from_utf8(&bytes).unwrap();
    let mut xor = bytes.clone();
    xor[0] ^= 1;
    let mut non_utf8 = bytes.clone();
    non_utf8[0] = 0xff;
    let controls = [
        xor,
        source.replacen("Child, ", "", 1).into_bytes(),
        source.replacen("C’tan", "C'tan", 1).into_bytes(),
        source
            .replacen("and Zubera.", "and Zubera, ForgeOnlySynthetic.", 1)
            .into_bytes(),
        non_utf8,
    ];
    for raw in controls {
        assert_ne!(raw, bytes);
        assert_eq!(
            compile(&raw, known_source()),
            Err(CreatureTypeCompileError::SourceDigestMismatch {
                expected: RULES_SHA256,
                actual: Sha256::digest(&raw).into()
            })
        );
    }
}
