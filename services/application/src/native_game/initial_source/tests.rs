//! Genuine guarded-source initialization and original private replay qualification.
use super::*;
use crate::{
    backend::{BackendClient, BackendConfig},
    player_wire::{PlayerAction, PlayerChoice},
    test_http::{Mock, Step},
};
use serde_json::{Value, json};
use spellwake_engine::{
    combat::CombatRules,
    game::{GameId, OpeningSetup, RuleFrame},
    object_identity::{CardId, ObjectDestination},
    opening::{MulliganChoice, OpeningDecision},
    setup::{GameSeed, StartingLife},
    shuffle::EntropyTape,
};
use std::{fs, sync::OnceLock, time::Duration};
pub(in crate::native_game) mod cast_activity;
mod clocks;
mod events;
mod results;
mod selected;
const ISLAND: &str = "b2c6aa39-2d2a-459c-a555-fb48ba993373";
const FOREST: &str = "b34bb2dc-c1af-4d77-b0b3-a0fb342a5fc6";
fn source_bytes() -> (Vec<u8>, Vec<u8>, Vec<u8>) {
    let read = |name| {
        fs::read(std::env::var_os(name).expect("explicit genuine source input required")).unwrap()
    };
    (
        read("SPELLWAKE_CATALOGUE_METADATA"),
        read("SPELLWAKE_CATALOGUE_INPUT"),
        read("SPELLWAKE_CREATURE_VOCABULARY_INPUT"),
    )
}
#[test]
#[ignore = "requires explicit genuine catalogue/vocabulary inputs; exact source retention, not card qualification"]
fn original_vocabulary_bytes_survive_source_construction() {
    let (metadata, snapshot, mut vocabulary) = source_bytes();
    // Keep valid, deliberately noncanonical JSON whitespace: regeneration would
    // change the exact blob identity recorded by the original source package.
    vocabulary.insert(0, b' ');
    vocabulary.push(b'\n');
    let expected = vocabulary.clone();
    let source = NativeProgramSource::from_bytes(
        NativeProgramPool::ResolutionSelection,
        &metadata,
        snapshot,
        &vocabulary,
    )
    .unwrap();
    vocabulary.fill(0);
    assert_eq!(source.vocabulary_input(), expected);
    let verified = verify_program_identity(
        source.program_identity_bytes(),
        &expected,
        native_runtime_identity(),
        source.identity(),
    )
    .unwrap();
    assert_eq!(verified.catalogue_id(), source.snapshot.as_str());
    assert_eq!(verified.name_rules_id(), source.names.id().to_string());
    let descriptor: SourceIdentity =
        crate::decode_wire_object(source.program_identity_bytes()).unwrap();
    assert_eq!(
        serde_json::to_vec(&descriptor).unwrap(),
        source.program_identity_bytes()
    );
    let changed = serde_json::to_vec(&serde_json::from_slice::<Value>(&expected).unwrap()).unwrap();
    assert_ne!(changed, expected);
    assert!(matches!(
        verify_program_identity(
            source.program_identity_bytes(),
            &changed,
            native_runtime_identity(),
            source.identity(),
        ),
        Err(NativeInitialError::ForeignSource)
    ));
}
pub(in crate::native_game) fn source(pool: NativeProgramPool) -> &'static NativeProgramSource {
    static NINE: OnceLock<NativeProgramSource> = OnceLock::new();
    static KEYWORDS: OnceLock<NativeProgramSource> = OnceLock::new();
    static TWELVE: OnceLock<NativeProgramSource> = OnceLock::new();
    let cell = match pool {
        NativeProgramPool::KeywordGameplay => &KEYWORDS,
        NativeProgramPool::ResolutionSelection => &NINE,
        NativeProgramPool::ResolutionSelectionNaturalMana => &TWELVE,
    };
    cell.get_or_init(|| {
        let (m, s, v) = source_bytes();
        NativeProgramSource::from_bytes(pool, &m, s, &v).unwrap()
    })
}
pub(in crate::native_game) fn ids() -> NativeInitialIds {
    NativeInitialIds {
        game_id: Uuid::from_u128(0x1ab1),
        context_id: Uuid::from_u128(0x1ab2),
        release_id: Uuid::from_u128(0x1ab3),
        profile_id: Uuid::from_u128(0x1ab4),
    }
}
fn seats() -> Vec<Uuid> {
    vec![Uuid::from_u128(0x1ab5), Uuid::from_u128(0x1ab6)]
}
pub(in crate::native_game) fn spec() -> NativeInitialSpec {
    NativeInitialSpec {
        first_player_seat_id: seats()[1],
        participants: seats()
            .into_iter()
            .enumerate()
            .map(|(index, seat_id)| NativeInitialParticipant {
                seat_id,
                life: if index == 0 {
                    RulesInteger::try_from("184467440737095516160".to_owned()).unwrap()
                } else {
                    20.into()
                },
                cards: (0..21)
                    .map(|n| NativeInitialCard {
                        oracle_id: OracleId::try_from(
                            if n % 2 == 0 { ISLAND } else { FOREST }.to_owned(),
                        )
                        .unwrap(),
                        zone: if n == 3 {
                            NativeInitialZone::Command
                        } else {
                            NativeInitialZone::Library
                        },
                    })
                    .collect(),
            })
            .collect(),
        entropy: vec![0.into(); 200],
    }
}
fn expected(reference: NativeArtifactRef) -> NativeInitialExpected {
    NativeInitialExpected::new(reference, ids().release_id, ids().profile_id, seats()).unwrap()
}
fn with_store<T>(
    bytes: &[u8],
    reference: &NativeArtifactRef,
    apply: impl FnOnce(&NativeArtifactStore) -> T,
) -> T {
    let mock = Mock::new(vec![
        Step::get(
            "/storage/v1/bucket/native-evidence".into(),
            200,
            br#"{"id":"native-evidence","public":false}"#.to_vec(),
        ),
        Step::get(
            format!(
                "/storage/v1/object/authenticated/native-evidence/{}",
                reference.as_str()
            ),
            200,
            bytes.to_vec(),
        ),
    ]);
    let store = NativeArtifactStore::new(
        BackendClient::new(
            BackendConfig::new(&mock.base, "sb_secret_test_only").unwrap(),
            "native-initial-test",
            Duration::from_secs(2),
        )
        .unwrap(),
        "native-evidence".into(),
    )
    .unwrap();
    let result = apply(&store);
    assert_eq!(mock.finish().len(), 2);
    result
}
pub(in crate::native_game) fn read_root(
    bytes: &[u8],
    expected: &NativeInitialExpected,
    source: &NativeProgramSource,
) -> Result<PreparedNativeOpening, NativeInitialError> {
    with_store(bytes, expected.reference(), |store| {
        load_initial(store, expected, source)
    })
}
fn reference(bytes: &[u8]) -> NativeArtifactRef {
    NativeArtifactRef::for_bytes(
        ids().game_id,
        ids().context_id,
        NativeArtifactRole::Initial,
        bytes,
    )
    .unwrap()
}
#[test]
fn foreign_compressed_source_rejects_before_decoder_and_errors_stay_private() {
    let error = NativeProgramSource::from_bytes(
        NativeProgramPool::ResolutionSelection,
        b"private-metadata",
        b"not-a-gzip-private-source".to_vec(),
        b"private-vocabulary",
    )
    .err()
    .unwrap();
    assert!(matches!(error, NativeInitialError::ForeignSource));
    assert!(!format!("{error:?}").contains("private-metadata"));
    assert!(
        NativeInitialExpected::new(
            NativeArtifactRef::for_bytes(
                ids().game_id,
                ids().context_id,
                NativeArtifactRole::Input,
                b"x"
            )
            .unwrap(),
            ids().release_id,
            ids().profile_id,
            seats()
        )
        .is_err()
    );
}
#[test]
#[ignore = "requires explicitly supplied complete source-qualified catalogue and vocabulary"]
fn genuine_pools_keep_separate_identity_and_reject_changed_original_source() {
    let a = source(NativeProgramPool::ResolutionSelection);
    let b = source(NativeProgramPool::ResolutionSelectionNaturalMana);
    assert_ne!(a.identity(), b.identity());
    assert_eq!(a.fully_qualified_cards(), 0);
    assert_eq!(b.fully_qualified_cards(), 0);
    assert!(a.supports(&OracleId::try_from(ISLAND.to_owned()).unwrap()));
    assert!(!a.supports(&OracleId::try_from(FOREST.to_owned()).unwrap()));
    assert!(b.supports(&OracleId::try_from(FOREST.to_owned()).unwrap()));
    assert_eq!(native_runtime_identity().len(), 64);
    let (m, s, v) = source_bytes();
    let mut changed = s.clone();
    changed[100] ^= 1;
    assert!(matches!(
        NativeProgramSource::from_bytes(b.pool(), &m, changed, &v),
        Err(NativeInitialError::ForeignSource)
    ));
    let mut vocab: Value = serde_json::from_slice(&v).unwrap();
    vocab["id"] = json!("0".repeat(64));
    assert!(
        NativeProgramSource::from_bytes(
            b.pool(),
            &m,
            s.clone(),
            &serde_json::to_vec(&vocab).unwrap()
        )
        .is_err()
    );
    assert!(NativeProgramSource::from_bytes(b.pool(), b"{}", s, &v).is_err());
}
#[test]
#[ignore = "requires explicitly supplied complete source-qualified catalogue and vocabulary"]
fn actual_guarded_opening_matches_direct_native_setup_and_private_store_reconstruction() {
    let source = source(NativeProgramPool::ResolutionSelectionNaturalMana);
    let initial = prepare_initial(ids(), source, spec()).unwrap();
    let spec = spec();
    let mut cards = vec![];
    let mut life = vec![];
    let mut number = 1;
    for (index, p) in spec.participants.iter().enumerate() {
        let owner = SeatId(u32::try_from(index).unwrap());
        life.push(StartingLife {
            player: owner,
            total: p.life.clone(),
        });
        for card in &p.cards {
            cards.push(
                source
                    .registry
                    .seed(
                        &card.oracle_id,
                        CardId(number),
                        owner,
                        match card.zone {
                            NativeInitialZone::Library => ObjectDestination::LibraryBottom,
                            NativeInitialZone::Command => ObjectDestination::Command,
                        },
                    )
                    .unwrap(),
            );
            number += 1;
        }
    }
    let seed = GameSeed {
        seats: vec![SeatId(0), SeatId(1)],
        active: SeatId(1),
        combat_rules: CombatRules::UnrestrictedFreeForAll,
        cards,
        initial_battlefield_order: vec![],
        initial_world_cohorts: vec![],
        mana: vec![],
        life,
        creature_vocabulary: Arc::clone(&source.vocabulary),
        name_rules: Arc::clone(&source.names),
    };
    let before_shuffle = GameState::initialize(
        GameId(ids().game_id.as_u128()),
        source.context(ids().release_id, ids().profile_id).unwrap(),
        seed.clone(),
    )
    .unwrap();
    for (seat, base) in [(SeatId(0), 1_u64), (SeatId(1), 22_u64)] {
        let objects = before_shuffle.state().execution().window().cards();
        let actual: Vec<_> = objects
            .library(seat)
            .iter()
            .map(|id| objects.get(*id).unwrap().card_id().unwrap())
            .collect();
        let expected: Vec<_> = (0..21)
            .filter(|n| *n != 3)
            .map(|n| CardId(base + n))
            .collect();
        assert_eq!(actual, expected);
    }
    let direct = GameState::initialize_opening(
        GameId(ids().game_id.as_u128()),
        source.context(ids().release_id, ids().profile_id).unwrap(),
        OpeningSetup {
            seed,
            entropy: EntropyTape::new(vec![0; 200]),
        },
    )
    .unwrap();
    assert_eq!(initial.candidate(), &direct);
    let restored = read_root(
        initial.original_bytes(),
        &expected(initial.reference().clone()),
        source,
    )
    .unwrap();
    assert_eq!(restored.candidate(), initial.candidate());
    assert_eq!(restored.original_bytes(), initial.original_bytes());
    assert_eq!(
        restored.candidate().state().basis().context.programs(),
        source.identity().as_str()
    );
    assert!(restored.binding(Uuid::new_v4()).is_err());
}
#[test]
#[ignore = "requires explicitly supplied complete source-qualified catalogue and vocabulary"]
fn original_root_shapes_and_authorized_tuple_substitutions_fail_closed() {
    let source = source(NativeProgramPool::ResolutionSelectionNaturalMana);
    let initial = prepare_initial(ids(), source, spec()).unwrap();
    let original: Value = serde_json::from_slice(initial.original_bytes()).unwrap();
    let mut mutations = vec![];
    for (field, value) in [
        ("version", json!("native-initial-v2")),
        (
            "programSourceId",
            json!("sha256:".to_owned() + &"0".repeat(64)),
        ),
        ("releaseId", json!(Uuid::new_v4())),
        ("profileId", json!(Uuid::new_v4())),
        ("firstPlayerSeatId", json!(Uuid::new_v4())),
        ("entropy", json!([])),
        ("entropy", json!(["01"])),
        ("entropy", json!([0])),
        ("extra", json!(true)),
    ] {
        let mut v = original.clone();
        v[field] = value;
        mutations.push(serde_json::to_vec(&v).unwrap());
    }
    let mut reordered = original.clone();
    reordered["participants"].as_array_mut().unwrap().reverse();
    mutations.push(serde_json::to_vec(&reordered).unwrap());
    let mut duplicated = original.clone();
    duplicated["participants"][1]["seatId"] = duplicated["participants"][0]["seatId"].clone();
    mutations.push(serde_json::to_vec(&duplicated).unwrap());
    let mut positional = original.clone();
    positional["participants"][0]["cards"][0] = json!([ISLAND, "library"]);
    mutations.push(serde_json::to_vec(&positional).unwrap());
    let mut alias = original.clone();
    alias["gameId"] = json!(ids().game_id.simple().to_string());
    mutations.push(serde_json::to_vec(&alias).unwrap());
    mutations.push(b"[]".to_vec());
    mutations.push(
        format!(
            "{{\"version\":\"native-initial-v1\",{}",
            std::str::from_utf8(initial.original_bytes())
                .unwrap()
                .strip_prefix('{')
                .unwrap()
        )
        .into_bytes(),
    );
    for bytes in mutations {
        assert!(read_root(&bytes, &expected(reference(&bytes)), source).is_err());
    }
    assert!(
        read_root(
            initial.original_bytes(),
            &expected(initial.reference().clone()),
            super::tests::source(NativeProgramPool::ResolutionSelection)
        )
        .is_err()
    );
    let wrong = NativeInitialExpected::new(
        initial.reference().clone(),
        Uuid::new_v4(),
        ids().profile_id,
        seats(),
    )
    .unwrap();
    assert!(read_root(initial.original_bytes(), &wrong, source).is_err());
    let mut invalid = spec();
    invalid.entropy = vec![0.into()];
    assert!(prepare_initial(ids(), source, invalid).is_err());
    let mut invalid = spec();
    invalid.participants[0].cards[0].oracle_id =
        OracleId::try_from("00000000-0000-4000-8000-000000000001".to_owned()).unwrap();
    assert!(prepare_initial(ids(), source, invalid).is_err());
}
fn binding_for(state: &GameState, viewer: Uuid) -> NativeBinding {
    NativeBinding::new(
        ids().context_id,
        state.basis().clone(),
        seats()
            .into_iter()
            .enumerate()
            .map(|(i, seat_id)| NativeSeatBinding {
                seat_id,
                native_seat: SeatId(u32::try_from(i).unwrap()),
            })
            .collect(),
        viewer,
    )
    .unwrap()
}
fn replay(
    source: &NativeProgramSource,
    initial: &PreparedNativeOpening,
    store: Option<&NativeArtifactStore>,
) -> usize {
    let mut state = initial.candidate().state().clone();
    let viewer = seats()[0];
    let scope = NativeScope::new(&state, binding_for(&state, viewer)).unwrap();
    let entropy = scope.prepare_entropy(vec![u64::MAX, 1, 2]).unwrap();
    let (bytes, candidate) = entropy.into_parts();
    state = candidate.into_state();
    let mut records = vec![(true, viewer, bytes, state.clone())];
    let mut mulligan = false;
    for _ in 0..16 {
        let Some(pending) = state.pending_decision() else {
            break;
        };
        let viewer = seats()[usize::try_from(pending.chooser().0).unwrap()];
        let mut scope = NativeScope::new(&state, binding_for(&state, viewer)).unwrap();
        let response = match pending.frame() {
            RuleFrame::Opening(OpeningDecision::Declare { .. }) => {
                let choice = if !mulligan {
                    mulligan = true;
                    MulliganChoice::Mulligan
                } else {
                    MulliganChoice::Keep
                };
                PlayerChoice::Option {
                    option: scope.issue(NativeReference::Mulligan(choice)).unwrap(),
                }
            }
            RuleFrame::Opening(OpeningDecision::Bottom { count, hand, .. }) => {
                PlayerChoice::Objects {
                    objects: hand
                        .iter()
                        .take(*count)
                        .rev()
                        .map(|id| scope.issue(NativeReference::Object(*id)).unwrap())
                        .collect(),
                }
            }
            _ => panic!("unexpected opening decision"),
        };
        let decision = scope
            .issue(NativeReference::Decision(pending.id()))
            .unwrap();
        let intent = PlayerCommand {
            request_id: Uuid::new_v4(),
            basis: scope.basis().clone(),
            action: PlayerAction::Choose { decision, response },
        };
        let proposal = scope.propose(&intent).unwrap();
        let bytes = scope.encode_input(&proposal).unwrap();
        state = proposal.into_parts().3.into_state();
        records.push((false, viewer, bytes, state.clone()));
    }
    assert!(state.opening().unwrap().complete());
    assert!(mulligan);
    let expected = expected(initial.reference().clone());
    let rebuilt = if let Some(store) = store {
        load_initial(store, &expected, source).unwrap()
    } else {
        read_root(initial.original_bytes(), &expected, source).unwrap()
    };
    let mut restored = rebuilt.candidate().state().clone();
    for (is_entropy, viewer, bytes, expected) in &records {
        restored = if *is_entropy {
            restore_entropy(&restored, binding_for(&restored, *viewer), bytes)
                .unwrap()
                .into_state()
        } else {
            restore_input(&restored, binding_for(&restored, *viewer), bytes)
                .unwrap()
                .into_parts()
                .3
                .into_state()
        };
        assert_eq!(&restored, expected);
    }
    assert_eq!(restored, state);
    records.len()
}
#[test]
#[ignore = "requires explicitly supplied complete source-qualified catalogue and vocabulary"]
fn guarded_source_fresh_opening_replays_ordered_player_and_entropy_inputs() {
    let source = source(NativeProgramPool::ResolutionSelectionNaturalMana);
    let initial = prepare_initial(ids(), source, spec()).unwrap();
    assert!(replay(source, &initial, None) >= 5);
}
#[test]
#[ignore = "requires explicitly owned disposable private Storage and genuine source inputs"]
fn owned_native_initial_source_and_replay() {
    assert_eq!(
        std::env::var("SPELLWAKE_NATIVE_STORAGE_QUALIFY").unwrap(),
        "1"
    );
    let source = source(NativeProgramPool::ResolutionSelectionNaturalMana);
    let initial = prepare_initial(ids(), source, spec()).unwrap();
    let store = NativeArtifactStore::from_env().unwrap();
    let retained = store
        .retain(
            ids().game_id,
            ids().context_id,
            NativeArtifactRole::Initial,
            initial.original_bytes(),
        )
        .unwrap();
    assert!(retained == *initial.reference());
    let count = replay(source, &initial, Some(&store));
    println!(
        "OWNED_NATIVE_INITIAL_OK {}",
        json!({"initialBytes":initial.original_bytes().len(),"orderedInputRecords":count,"fullyQualifiedCards":source.fully_qualified_cards(),"freshGuardedSourceReconstruction":true})
    );
}

#[test]
#[ignore = "requires explicitly supplied complete source-qualified catalogue and vocabulary"]
fn original_setup_bounds_nested_shapes_and_full_ref_are_enforced() {
    let source = source(NativeProgramPool::ResolutionSelectionNaturalMana);
    for count in [1, 7] {
        let mut invalid = spec();
        invalid.participants = (0..count)
            .map(|n| {
                let mut p = spec().participants.remove(0);
                p.seat_id = Uuid::from_u128(100 + n);
                p
            })
            .collect();
        invalid.first_player_seat_id = invalid.participants[0].seat_id;
        assert!(prepare_initial(ids(), source, invalid).is_err());
    }
    for life in [0, -1] {
        let mut invalid = spec();
        invalid.participants[0].life = life.into();
        assert!(prepare_initial(ids(), source, invalid).is_err());
    }
    let mut invalid = spec();
    invalid.entropy = vec![0.into(); 65_537];
    assert!(prepare_initial(ids(), source, invalid).is_err());
    let mut invalid = spec();
    invalid.participants[0].cards = (0..1001)
        .map(|_| NativeInitialCard {
            oracle_id: OracleId::try_from(ISLAND.to_owned()).unwrap(),
            zone: NativeInitialZone::Library,
        })
        .collect();
    assert!(prepare_initial(ids(), source, invalid).is_err());
    let initial = prepare_initial(ids(), source, spec()).unwrap();
    let original: Value = serde_json::from_slice(initial.original_bytes()).unwrap();
    let mut mutations = vec![];
    for field in ["gameId", "contextId"] {
        let mut v = original.clone();
        v[field] = json!(Uuid::new_v4());
        mutations.push(serde_json::to_vec(&v).unwrap());
    }
    let mut v = original.clone();
    v["entropy"] = json!(["18446744073709551616"]);
    mutations.push(serde_json::to_vec(&v).unwrap());
    for field in ["seatId", "extra"] {
        let mut v = original.clone();
        v["participants"][0][field] = json!(Uuid::nil());
        mutations.push(serde_json::to_vec(&v).unwrap());
    }
    let mut v = original.clone();
    v["participants"][0]["cards"][0]["extra"] = json!(true);
    mutations.push(serde_json::to_vec(&v).unwrap());
    let mut v = original.clone();
    v["participants"][0]["cards"][0]["oracleId"] = json!(ISLAND.to_uppercase());
    mutations.push(serde_json::to_vec(&v).unwrap());
    let raw = std::str::from_utf8(initial.original_bytes()).unwrap();
    mutations.push(
        raw.replacen(
            "\"seatId\":",
            &format!("\"seatId\":\"{}\",\"seatId\":", seats()[0]),
            1,
        )
        .into_bytes(),
    );
    mutations.push(
        raw.replacen(
            "\"oracleId\":",
            &format!("\"oracleId\":\"{ISLAND}\",\"oracleId\":"),
            1,
        )
        .into_bytes(),
    );
    for bytes in mutations {
        assert!(read_root(&bytes, &expected(reference(&bytes)), source).is_err());
    }
    let mut padded = initial.original_bytes().to_vec();
    padded.push(b' ');
    assert!(read_root(&padded, &expected(initial.reference().clone()), source).is_err());
    let different_ref = reference(&padded);
    let artifact = with_store(&padded, &different_ref, |store| {
        store
            .read(
                &different_ref,
                ids().game_id,
                ids().context_id,
                NativeArtifactRole::Initial,
            )
            .unwrap()
    });
    assert!(matches!(
        restore_initial(artifact, &expected(initial.reference().clone()), source),
        Err(NativeInitialError::ForeignSource)
    ));
    let huge = vec![b' '; 1_048_577];
    assert!(
        NativeInitialExpected::new(
            reference(&huge),
            ids().release_id,
            ids().profile_id,
            seats()
        )
        .is_err()
    );
    let (m, s, v) = source_bytes();
    let huge = vec![b' '; 1_048_577];
    assert!(matches!(
        NativeProgramSource::from_bytes(source.pool(), &huge, s.clone(), &v),
        Err(NativeInitialError::Size)
    ));
    assert!(matches!(
        NativeProgramSource::from_bytes(source.pool(), &m, s.clone(), &huge),
        Err(NativeInitialError::Size)
    ));
    assert!(matches!(
        NativeProgramSource::from_bytes(source.pool(), &m, vec![0; 33_554_433], &v),
        Err(NativeInitialError::Size)
    ));
    let mut vocab: Value = serde_json::from_slice(&v).unwrap();
    vocab["members"].as_array_mut().unwrap().pop();
    assert!(
        NativeProgramSource::from_bytes(source.pool(), &m, s, &serde_json::to_vec(&vocab).unwrap())
            .is_err()
    );
}
// Reuse the explicit genuine-source fixture; no synthetic fallback catalogue.
pub(in crate::native_game) struct CheckpointFixture {
    pub(in crate::native_game) tip: NativeCheckpoint,
    pub(in crate::native_game) states: Vec<(NativeArtifactRef, Vec<u8>)>,
    pub(in crate::native_game) inputs: Vec<(NativeArtifactRef, Vec<u8>)>,
    pub(in crate::native_game) original: Vec<u8>,
    pub(in crate::native_game) candidates: Vec<NativeCandidate>,
}
pub(in crate::native_game) fn checkpoint_fixture(
    store: Option<&NativeArtifactStore>,
) -> CheckpointFixture {
    let source = source(NativeProgramPool::ResolutionSelectionNaturalMana);
    let opening = prepare_initial(ids(), source, spec()).unwrap();
    checkpoint_fixture_with_opening(opening, store)
}
fn checkpoint_fixture_with_opening(
    opening: PreparedNativeOpening,
    store: Option<&NativeArtifactStore>,
) -> CheckpointFixture {
    let original = opening.original_bytes().to_vec();
    if let Some(store) = store {
        assert!(
            store
                .retain(
                    ids().game_id,
                    ids().context_id,
                    NativeArtifactRole::Initial,
                    &original
                )
                .unwrap()
                == *opening.reference()
        );
    }
    let mut checkpoint = NativeCheckpoint::initial(opening).unwrap();
    let mut states = vec![(
        checkpoint.reference().clone(),
        checkpoint.original_bytes().to_vec(),
    )];
    let mut inputs = vec![];
    let mut candidates = vec![checkpoint.candidate().clone()];
    if let Some(store) = store {
        assert!(checkpoint.retain(store).unwrap() == *checkpoint.reference());
    }
    for index in 0..4 {
        let viewer = if index == 0 {
            seats()[0]
        } else {
            let Some(pending) = checkpoint.state().pending_decision() else {
                break;
            };
            seats()[usize::try_from(pending.chooser().0).unwrap()]
        };
        let mut scope =
            NativeScope::new(checkpoint.state(), checkpoint.binding(viewer).unwrap()).unwrap();
        let (kind, bytes, actual) = if index == 0 {
            let proposal = scope.prepare_entropy(vec![u64::MAX, 1, 2]).unwrap();
            let (bytes, candidate) = proposal.into_parts();
            (NativeCheckpointInput::Entropy, bytes, candidate)
        } else {
            let pending = checkpoint.state().pending_decision().unwrap();
            assert!(matches!(
                pending.frame(),
                RuleFrame::Opening(OpeningDecision::Declare { .. })
            ));
            let decision = scope
                .issue(NativeReference::Decision(pending.id()))
                .unwrap();
            let option = scope
                .issue(NativeReference::Mulligan(MulliganChoice::Keep))
                .unwrap();
            let proposal = scope
                .propose(&PlayerCommand {
                    request_id: Uuid::new_v4(),
                    basis: scope.basis().clone(),
                    action: PlayerAction::Choose {
                        decision,
                        response: PlayerChoice::Option { option },
                    },
                })
                .unwrap();
            let bytes = scope.encode_input(&proposal).unwrap();
            (
                NativeCheckpointInput::Player,
                bytes,
                proposal.into_parts().3,
            )
        };
        let input = NativeArtifactRef::for_bytes(
            ids().game_id,
            ids().context_id,
            NativeArtifactRole::Input,
            &bytes,
        )
        .unwrap();
        let before = checkpoint.state().clone();
        let next = if let Some(store) = store {
            assert!(
                store
                    .retain(
                        ids().game_id,
                        ids().context_id,
                        NativeArtifactRole::Input,
                        &bytes
                    )
                    .unwrap()
                    == input
            );
            checkpoint
                .append(store, input.clone(), kind, viewer)
                .unwrap()
        } else {
            with_store(&bytes, &input, |store| {
                checkpoint.append(store, input.clone(), kind, viewer)
            })
            .unwrap()
        };
        assert_eq!(checkpoint.state(), &before);
        assert_eq!(next.candidate(), &actual);
        checkpoint = next;
        candidates.push(checkpoint.candidate().clone());
        if let Some(store) = store {
            assert!(checkpoint.retain(store).unwrap() == *checkpoint.reference());
        }
        inputs.push((input, bytes));
        states.push((
            checkpoint.reference().clone(),
            checkpoint.original_bytes().to_vec(),
        ));
    }
    assert!(checkpoint.state().opening().unwrap().complete());
    assert_eq!(inputs.len(), 3);
    CheckpointFixture {
        tip: checkpoint,
        states,
        inputs,
        original,
        candidates,
    }
}
pub(in crate::native_game) fn checkpoint_read_steps(
    reference: &NativeArtifactRef,
    bytes: &[u8],
) -> Vec<Step> {
    vec![
        Step::get(
            "/storage/v1/bucket/native-evidence".into(),
            200,
            br#"{"id":"native-evidence","public":false}"#.to_vec(),
        ),
        Step::get(
            format!(
                "/storage/v1/object/authenticated/native-evidence/{}",
                reference.as_str()
            ),
            200,
            bytes.to_vec(),
        ),
    ]
}
pub(in crate::native_game) fn checkpoint_store(mock: &Mock) -> NativeArtifactStore {
    NativeArtifactStore::new(
        BackendClient::new(
            BackendConfig::new(&mock.base, "sb_secret_test_only").unwrap(),
            "native-checkpoint-test",
            Duration::from_secs(2),
        )
        .unwrap(),
        "native-evidence".into(),
    )
    .unwrap()
}
#[test]
#[ignore = "requires explicitly supplied complete source-qualified catalogue and vocabulary"]
fn linked_checkpoint_recovery_reexecutes_exact_private_input_chain() {
    let source = source(NativeProgramPool::ResolutionSelectionNaturalMana);
    let CheckpointFixture {
        tip,
        states,
        inputs,
        original,
        candidates,
    } = checkpoint_fixture(None);
    let initial_ref = reference(&original);
    let expected =
        NativeCheckpointExpected::new(tip.reference().clone(), tip.basis().clone()).unwrap();
    let mut steps = vec![];
    for (r, b) in states.iter().rev() {
        steps.extend(checkpoint_read_steps(r, b));
    }
    steps.extend(checkpoint_read_steps(&initial_ref, &original));
    for (r, b) in &inputs {
        steps.extend(checkpoint_read_steps(r, b));
    }
    let total_bytes = states
        .iter()
        .chain(inputs.iter())
        .map(|(r, _)| r.size())
        .sum::<u64>()
        + initial_ref.size();
    let mock = Mock::new(steps);
    let store = checkpoint_store(&mock);
    let restored = load_checkpoint(
        &store,
        &expected,
        &self::expected(initial_ref),
        source,
        NativeRecoveryBudget::new(states.len(), total_bytes).unwrap(),
    )
    .unwrap();
    assert_eq!(restored.state(), tip.state());
    assert_eq!(restored.candidate(), tip.candidate());
    assert!(restored.reference() == tip.reference());
    assert_eq!(restored.original_bytes(), tip.original_bytes());
    assert_eq!(mock.finish().len(), 16);
    for budget in [
        NativeRecoveryBudget::new(1, tip.reference().size() - 1).unwrap(),
        NativeRecoveryBudget::new(1, 1).unwrap(),
    ] {
        let mock = Mock::new(vec![]);
        assert!(matches!(
            load_checkpoint(
                &checkpoint_store(&mock),
                &expected,
                &self::expected(reference(&original)),
                source,
                budget
            ),
            Err(NativeCheckpointError::Budget)
        ));
        assert!(mock.finish().is_empty());
    }
    for index in 0..states.len() {
        let native_basis = candidates[index].state().basis();
        let expected_basis = crate::player_wire::PlayerBasis {
            game_id: ids().game_id,
            context_id: ids().context_id,
            epoch: native_basis.epoch.into(),
            revision: native_basis.revision.into(),
        };
        let accepted =
            NativeCheckpointExpected::new(states[index].0.clone(), expected_basis).unwrap();
        let mut steps = vec![];
        for (r, b) in states.iter().take(index + 1).rev() {
            steps.extend(checkpoint_read_steps(r, b));
        }
        steps.extend(checkpoint_read_steps(&reference(&original), &original));
        for (r, b) in inputs.iter().take(index) {
            steps.extend(checkpoint_read_steps(r, b));
        }
        let count = steps.len();
        let mock = Mock::new(steps);
        let restored = load_checkpoint(
            &checkpoint_store(&mock),
            &accepted,
            &self::expected(reference(&original)),
            source,
            NativeRecoveryBudget::new(100, 10_000_000).unwrap(),
        )
        .unwrap();
        assert_eq!(restored.candidate(), &candidates[index]);
        assert_eq!(restored.original_bytes(), states[index].1);
        assert_eq!(mock.finish().len(), count);
    }
    let huge = vec![b' '; 1_048_577];
    let overbound = NativeArtifactRef::for_bytes(
        ids().game_id,
        ids().context_id,
        NativeArtifactRole::Input,
        &huge,
    )
    .unwrap();
    let mock = Mock::new(vec![]);
    assert!(matches!(
        tip.append(
            &checkpoint_store(&mock),
            overbound,
            NativeCheckpointInput::Player,
            seats()[0]
        ),
        Err(NativeCheckpointError::Invalid)
    ));
    assert!(mock.finish().is_empty());
    let mock = Mock::new(vec![]);
    assert!(matches!(
        tip.append(
            &checkpoint_store(&mock),
            inputs[0].0.clone(),
            NativeCheckpointInput::Entropy,
            Uuid::new_v4()
        ),
        Err(NativeCheckpointError::Binding(_))
    ));
    assert!(mock.finish().is_empty());
    let mut wrong_basis = tip.basis().clone();
    wrong_basis.revision = 2.into();
    let wrong = NativeCheckpointExpected::new(tip.reference().clone(), wrong_basis).unwrap();
    let mock = Mock::new(checkpoint_read_steps(tip.reference(), tip.original_bytes()));
    assert!(matches!(
        load_checkpoint(
            &checkpoint_store(&mock),
            &wrong,
            &self::expected(reference(&original)),
            source,
            NativeRecoveryBudget::new(100, 10_000_000).unwrap()
        ),
        Err(NativeCheckpointError::Invalid)
    ));
    assert_eq!(mock.finish().len(), 2);
    // Exhaust record budget after exactly the accepted tip read, with no extra IO.
    let mock = Mock::new(checkpoint_read_steps(tip.reference(), tip.original_bytes()));
    assert!(matches!(
        load_checkpoint(
            &checkpoint_store(&mock),
            &expected,
            &self::expected(reference(&original)),
            source,
            NativeRecoveryBudget::new(1, u64::MAX).unwrap()
        ),
        Err(NativeCheckpointError::Budget)
    ));
    assert_eq!(mock.finish().len(), 2);
    // Charge all input bytes; a one-byte shortage fails before fetching the last input.
    let mut steps = vec![];
    for (r, b) in states.iter().rev() {
        steps.extend(checkpoint_read_steps(r, b));
    }
    steps.extend(checkpoint_read_steps(&reference(&original), &original));
    for (r, b) in inputs.iter().take(inputs.len() - 1) {
        steps.extend(checkpoint_read_steps(r, b));
    }
    let mock = Mock::new(steps);
    assert!(matches!(
        load_checkpoint(
            &checkpoint_store(&mock),
            &expected,
            &self::expected(reference(&original)),
            source,
            NativeRecoveryBudget::new(states.len(), total_bytes - 1).unwrap()
        ),
        Err(NativeCheckpointError::Budget)
    ));
    assert_eq!(mock.finish().len(), 14);
}
#[test]
#[ignore = "requires explicitly owned disposable private Storage and genuine source inputs"]
fn owned_native_checkpoint_chain_recovery() {
    assert_eq!(
        std::env::var("SPELLWAKE_NATIVE_STORAGE_QUALIFY").unwrap(),
        "1"
    );
    let store = NativeArtifactStore::from_env().unwrap();
    let CheckpointFixture {
        tip,
        states,
        inputs,
        original,
        candidates,
    } = checkpoint_fixture(Some(&store));
    let restored = load_checkpoint(
        &store,
        &NativeCheckpointExpected::new(tip.reference().clone(), tip.basis().clone()).unwrap(),
        &expected(reference(&original)),
        source(NativeProgramPool::ResolutionSelectionNaturalMana),
        NativeRecoveryBudget::new(100, 10_000_000).unwrap(),
    )
    .unwrap();
    assert_eq!(restored.state(), tip.state());
    assert_eq!(restored.candidate(), tip.candidate());
    assert!(restored.reference() == tip.reference());
    for (index, candidate) in candidates.iter().enumerate() {
        let basis = candidate.state().basis();
        let accepted = NativeCheckpointExpected::new(
            states[index].0.clone(),
            crate::player_wire::PlayerBasis {
                game_id: ids().game_id,
                context_id: ids().context_id,
                epoch: basis.epoch.into(),
                revision: basis.revision.into(),
            },
        )
        .unwrap();
        let restored = load_checkpoint(
            &store,
            &accepted,
            &expected(reference(&original)),
            source(NativeProgramPool::ResolutionSelectionNaturalMana),
            NativeRecoveryBudget::new(100, 10_000_000).unwrap(),
        )
        .unwrap();
        assert_eq!(restored.candidate(), candidate);
    }
    println!(
        "OWNED_NATIVE_CHECKPOINT_OK {}",
        json!({"stateManifests":states.len(),"orderedInputRecords":inputs.len(),"sameActualCandidate":true,"verifiedTips":candidates.len(),"fullyQualifiedCards":0})
    );
}
#[test]
#[ignore = "requires explicitly supplied complete source-qualified catalogue and vocabulary"]
fn linked_checkpoint_rejects_foreign_source_gaps_wrong_kind_and_stale_inputs() {
    let source = source(NativeProgramPool::ResolutionSelectionNaturalMana);
    let CheckpointFixture {
        tip,
        states,
        inputs,
        original,
        ..
    } = checkpoint_fixture(None);
    let initial_ref = reference(&original);
    for (field, value) in [
        ("version", json!("native-checkpoint-v2")),
        (
            "initialRef",
            json!(
                NativeArtifactRef::for_bytes(
                    ids().game_id,
                    ids().context_id,
                    NativeArtifactRole::Initial,
                    b"other-original-source"
                )
                .unwrap()
                .as_str()
            ),
        ),
        ("parentRef", json!(inputs[0].0.as_str())),
        ("viewerSeatId", json!(Uuid::nil())),
        ("extra", json!(true)),
    ] {
        let mut v: Value = serde_json::from_slice(tip.original_bytes()).unwrap();
        v[field] = value;
        let bytes = serde_json::to_vec(&v).unwrap();
        let reference = NativeArtifactRef::for_bytes(
            ids().game_id,
            ids().context_id,
            NativeArtifactRole::State,
            &bytes,
        )
        .unwrap();
        let expected =
            NativeCheckpointExpected::new(reference.clone(), tip.basis().clone()).unwrap();
        let mock = Mock::new(checkpoint_read_steps(&reference, &bytes));
        assert!(
            load_checkpoint(
                &checkpoint_store(&mock),
                &expected,
                &self::expected(initial_ref.clone()),
                source,
                NativeRecoveryBudget::new(100, 10_000_000).unwrap()
            )
            .is_err()
        );
        assert_eq!(mock.finish().len(), 2);
    }
    // Valid-scope parent that skips revisions is rejected at the next manifest.
    let mut v: Value = serde_json::from_slice(tip.original_bytes()).unwrap();
    v["parentRef"] = json!(states[0].0.as_str());
    let bytes = serde_json::to_vec(&v).unwrap();
    let reference = NativeArtifactRef::for_bytes(
        ids().game_id,
        ids().context_id,
        NativeArtifactRole::State,
        &bytes,
    )
    .unwrap();
    let expected = NativeCheckpointExpected::new(reference.clone(), tip.basis().clone()).unwrap();
    let mut steps = checkpoint_read_steps(&reference, &bytes);
    steps.extend(checkpoint_read_steps(&states[0].0, &states[0].1));
    let mock = Mock::new(steps);
    assert!(matches!(
        load_checkpoint(
            &checkpoint_store(&mock),
            &expected,
            &self::expected(initial_ref.clone()),
            source,
            NativeRecoveryBudget::new(100, 10_000_000).unwrap()
        ),
        Err(NativeCheckpointError::Invalid)
    ));
    assert_eq!(mock.finish().len(), 4);
    for wrong_kind in [true, false] {
        let mut changed = states.clone();
        // Rehash and relink all descendants; Store sees internally valid original
        // bytes, so rejection must come from real input reconstruction semantics.
        for index in 1..changed.len() {
            let mut v: Value = serde_json::from_slice(&changed[index].1).unwrap();
            v["parentRef"] = json!(changed[index - 1].0.as_str());
            if index == 1 && wrong_kind {
                v["kind"] = json!("player");
            }
            if index == 2 && !wrong_kind {
                v["inputRef"] = json!(inputs[0].0.as_str());
            }
            let bytes = serde_json::to_vec(&v).unwrap();
            changed[index] = (
                NativeArtifactRef::for_bytes(
                    ids().game_id,
                    ids().context_id,
                    NativeArtifactRole::State,
                    &bytes,
                )
                .unwrap(),
                bytes,
            );
        }
        let expected =
            NativeCheckpointExpected::new(changed.last().unwrap().0.clone(), tip.basis().clone())
                .unwrap();
        let mut steps = vec![];
        for (r, b) in changed.iter().rev() {
            steps.extend(checkpoint_read_steps(r, b));
        }
        steps.extend(checkpoint_read_steps(&initial_ref, &original));
        steps.extend(checkpoint_read_steps(&inputs[0].0, &inputs[0].1));
        if !wrong_kind {
            steps.extend(checkpoint_read_steps(&inputs[0].0, &inputs[0].1));
        }
        let count = steps.len();
        let mock = Mock::new(steps);
        assert!(matches!(
            load_checkpoint(
                &checkpoint_store(&mock),
                &expected,
                &self::expected(initial_ref.clone()),
                source,
                NativeRecoveryBudget::new(100, 10_000_000).unwrap()
            ),
            Err(NativeCheckpointError::Input(_))
        ));
        assert_eq!(mock.finish().len(), count);
    }
    assert_eq!(tip.state().basis().revision, 3);
}

#[test]
#[ignore = "requires genuine retained catalogue/vocabulary"]
fn frozen_catalogue_preserves_every_selected_record_and_excludes_unbound_sources() {
    let (metadata, bytes, _) = source_bytes();
    let inventory = spellwake_content::snapshot::decode(&metadata, bytes).unwrap();
    for pool in [
        NativeProgramPool::ResolutionSelection,
        NativeProgramPool::ResolutionSelectionNaturalMana,
    ] {
        let package = source(pool);
        let expected: BTreeSet<_> = package
            .registry
            .coverage()
            .cards
            .iter()
            .filter_map(|row| row.oracle_id.as_ref())
            .filter_map(|id| OracleId::try_from(id.clone()).ok())
            .filter(|id| package.registry.resolve(id).is_some())
            .collect();
        assert!(package.catalogue.keys().eq(expected.iter()));
        assert_eq!(
            expected.len(),
            if pool == NativeProgramPool::ResolutionSelection {
                9
            } else {
                12
            }
        );
        for id in expected {
            let originals: Vec<_> = inventory
                .cards
                .iter()
                .filter(|card| card.oracle_id.as_deref() == Some(id.as_str()))
                .collect();
            assert_eq!(originals.len(), 1);
            let retained = package.source_card(&id).unwrap();
            assert_eq!(
                serde_json::to_value(retained).unwrap(),
                serde_json::to_value(originals[0]).unwrap()
            );
        }
        let unsupported = inventory
            .cards
            .iter()
            .filter_map(|card| card.oracle_id.as_ref())
            .filter_map(|id| OracleId::try_from(id.clone()).ok())
            .find(|id| package.registry.resolve(id).is_none())
            .unwrap();
        assert!(package.source_card(&unsupported).is_none());
        assert_eq!(package.fully_qualified_cards(), 0);
    }
}

#[test]
#[ignore = "requires genuine retained catalogue/vocabulary"]
fn printed_descriptions_join_exact_guarded_programs_without_selecting_printings() {
    let (metadata, bytes, _) = source_bytes();
    let inventory = spellwake_content::snapshot::decode(&metadata, bytes).unwrap();
    for pool in [
        NativeProgramPool::ResolutionSelection,
        NativeProgramPool::ResolutionSelectionNaturalMana,
    ] {
        let package = source(pool);
        let mut matched = 0;
        for oracle in package.catalogue.keys() {
            let seed = package
                .registry
                .seed(
                    oracle,
                    CardId(1),
                    SeatId(0),
                    ObjectDestination::LibraryBottom,
                )
                .unwrap();
            let description = package.printed_card(&seed.program).unwrap().unwrap();
            assert_eq!(description.oracle_id(), oracle);
            let original = inventory
                .cards
                .iter()
                .find(|card| card.oracle_id.as_deref() == Some(oracle.as_str()))
                .unwrap();
            let [face] = original.faces.as_slice() else {
                panic!("guarded single-face source expected")
            };
            let text = description.characteristics();
            assert_eq!(text.name, face.name);
            assert_eq!(Some(text.type_line.as_str()), face.type_line.as_deref());
            assert_eq!(
                text.rules_text,
                face.oracle_text.as_deref().unwrap_or_default()
            );
            assert_eq!(text.mana_cost, face.mana_cost);
            assert_eq!(text.printed_power, face.power);
            assert_eq!(text.printed_toughness, face.toughness);
            assert!(text.printed_loyalty.is_none());
            // A changed copiable definition cannot reuse the saved description.
            let mut changed = seed.program.clone();
            changed.names = spellwake_contracts::names::ObjectNames::nameless();
            assert!(package.printed_card(&changed).unwrap().is_none());
            let mut same_name = seed.program.clone();
            let artifact = spellwake_engine::program::CardType::Artifact;
            if !same_name.card_types.insert(artifact) {
                same_name.card_types.remove(&artifact);
            }
            assert!(package.printed_card(&same_name).unwrap().is_none());
            matched += 1;
        }
        assert_eq!(
            matched,
            if pool == NativeProgramPool::ResolutionSelection {
                9
            } else {
                12
            }
        );
        assert_eq!(package.fully_qualified_cards(), 0);
    }
}

#[test]
#[ignore = "requires genuine retained catalogue/vocabulary; altered inventories are fault controls only"]
fn frozen_catalogue_rejects_missing_and_duplicate_materialized_source_records() {
    let package = source(NativeProgramPool::ResolutionSelectionNaturalMana);
    let (metadata, bytes, _) = source_bytes();
    let mut missing = spellwake_content::snapshot::decode(&metadata, bytes.clone()).unwrap();
    let selected = package.catalogue.keys().next().unwrap();
    let index = missing
        .cards
        .iter()
        .position(|card| card.oracle_id.as_deref() == Some(selected.as_str()))
        .unwrap();
    missing.cards.remove(index);
    assert!(matches!(
        super::catalogue::retain(&missing.cards, &package.registry),
        Err(NativeInitialError::Invalid)
    ));
    let mut duplicate = spellwake_content::snapshot::decode(&metadata, bytes).unwrap();
    // Move the genuine selected record from a separate decode; no Clone mirror
    // or synthetic field substitution can accidentally normalize its contents.
    let (m, b, _) = source_bytes();
    let mut donor = spellwake_content::snapshot::decode(&m, b).unwrap();
    let index = donor
        .cards
        .iter()
        .position(|card| card.oracle_id.as_deref() == Some(selected.as_str()))
        .unwrap();
    duplicate.cards.push(donor.cards.remove(index));
    assert!(matches!(
        super::catalogue::retain(&duplicate.cards, &package.registry),
        Err(NativeInitialError::Invalid)
    ));
}

// Shape/binding fixtures are not source qualification or a materialized registry.
fn identity_fixture(vocabulary: &[u8]) -> SourceIdentity {
    SourceIdentity {
        version: "native-program-source-v1".into(),
        runtime: "original-runtime-not-current".into(),
        pool: "original-pool".into(),
        registry_version: 1,
        metadata: blob(b"original-metadata").unwrap(),
        snapshot: blob(b"original-catalogue").unwrap(),
        vocabulary: blob(vocabulary).unwrap(),
        name_rules: "original-name-rules".into(),
    }
}
#[test]
fn original_program_identity_preserves_exact_encoding_hash_and_runtime() {
    let vocabulary = b"original vocabulary bytes";
    let descriptor = identity_fixture(vocabulary);
    let encoded = serde_json::to_vec(&descriptor).unwrap();
    let expected = program_identity(&encoded).unwrap();
    // Explicitly retain the old field order, strings and length-prefixed hash.
    let old_encoding = format!(
        "{{\"version\":\"native-program-source-v1\",\"runtime\":\"original-runtime-not-current\",\"pool\":\"original-pool\",\"registryVersion\":1,\"metadata\":{},\"snapshot\":{},\"vocabulary\":{},\"nameRules\":\"original-name-rules\"}}",
        serde_json::to_string(&descriptor.metadata).unwrap(),
        serde_json::to_string(&descriptor.snapshot).unwrap(),
        serde_json::to_string(&descriptor.vocabulary).unwrap(),
    );
    assert_eq!(encoded, old_encoding.as_bytes());
    let mut old_hash = Sha256::new();
    old_hash.update(b"spellwake.native-program-source.v1\0");
    old_hash.update(u64::try_from(encoded.len()).unwrap().to_be_bytes());
    old_hash.update(&encoded);
    assert_eq!(expected, hash_text(&old_hash.finalize()));
    let verify = |bytes: &[u8], input: &[u8], runtime: &str, id: &EvidenceHash| {
        verify_program_identity(bytes, input, runtime, id)
    };
    let checked = verify(&encoded, vocabulary, &descriptor.runtime, &expected).unwrap();
    assert_eq!(checked.catalogue_id(), descriptor.snapshot.sha256.as_str());
    assert_eq!(checked.name_rules_id(), descriptor.name_rules);
    assert_ne!(descriptor.runtime, native_runtime_identity());
    assert!(matches!(
        verify(&encoded, vocabulary, native_runtime_identity(), &expected),
        Err(NativeInitialError::ForeignSource)
    ));
    assert!(matches!(
        verify(
            &encoded,
            b"changed vocabulary bytes",
            &descriptor.runtime,
            &expected
        ),
        Err(NativeInitialError::ForeignSource)
    ));
    assert!(matches!(
        verify(
            &encoded,
            vocabulary,
            &descriptor.runtime,
            &digest(b"foreign program")
        ),
        Err(NativeInitialError::ForeignSource)
    ));
    for mutate in [
        "runtime",
        "pool",
        "nameRules",
        "metadata",
        "snapshot",
        "vocabulary",
    ] {
        let mut value = serde_json::to_value(&descriptor).unwrap();
        if value[mutate].is_object() {
            value[mutate]["sha256"] = json!(digest(b"substituted").as_str());
        } else {
            value[mutate] = json!("substituted");
        }
        let parsed: SourceIdentity = serde_json::from_value(value).unwrap();
        let changed = serde_json::to_vec(&parsed).unwrap();
        assert!(matches!(
            verify(&changed, vocabulary, &descriptor.runtime, &expected),
            Err(NativeInitialError::ForeignSource)
        ));
    }
    // Recompute the program ID to isolate the exact vocabulary digest/length check.
    for field in ["sha256", "bytes"] {
        let mut changed = identity_fixture(vocabulary);
        if field == "sha256" {
            changed.vocabulary.sha256 = digest(b"foreign vocabulary");
        } else {
            changed.vocabulary.bytes += 1;
        }
        let bytes = serde_json::to_vec(&changed).unwrap();
        assert!(matches!(
            verify(
                &bytes,
                vocabulary,
                &descriptor.runtime,
                &program_identity(&bytes).unwrap()
            ),
            Err(NativeInitialError::ForeignSource)
        ));
    }
    let pretty = serde_json::to_vec_pretty(&descriptor).unwrap();
    assert!(matches!(
        verify(&pretty, vocabulary, &descriptor.runtime, &expected),
        Err(NativeInitialError::ForeignSource)
    ));
}
#[test]
fn original_program_identity_rejects_root_and_nested_shape_faults_with_json_causes() {
    let vocabulary = b"original vocabulary bytes";
    let descriptor = identity_fixture(vocabulary);
    let encoded = serde_json::to_vec(&descriptor).unwrap();
    let expected = program_identity(&encoded).unwrap();
    let valid: Value = serde_json::from_slice(&encoded).unwrap();
    let reject = |bytes: &[u8]| {
        let error = verify_program_identity(bytes, vocabulary, &descriptor.runtime, &expected)
            .err()
            .unwrap();
        assert!(matches!(error, NativeInitialError::Json(_)));
        assert!(error.source().is_some());
    };
    for at in [None, Some("metadata"), Some("snapshot"), Some("vocabulary")] {
        let object = at.map_or(&valid, |field| &valid[field]);
        for field in object.as_object().unwrap().keys() {
            let mut changed = valid.clone();
            let target = match at {
                Some(at) => changed.get_mut(at).unwrap(),
                None => &mut changed,
            };
            target.as_object_mut().unwrap().remove(field);
            reject(&serde_json::to_vec(&changed).unwrap());
            // Insert an equal-valued duplicate into the raw object, preserving it.
            let duplicate = format!("{{{}:{},", json!(field), object[field]);
            let nested = serde_json::to_string(object).unwrap();
            let repeated = nested.replacen('{', &duplicate, 1);
            let raw = match at {
                Some(at) => {
                    let mut outer = valid.clone();
                    outer[at] = Value::Null;
                    serde_json::to_string(&outer).unwrap().replacen(
                        &format!("\"{at}\":null"),
                        &format!("\"{at}\":{repeated}"),
                        1,
                    )
                }
                None => repeated,
            };
            reject(raw.as_bytes());
        }
        // Match each struct's actual field order so a sequence-compatible
        // Serde struct would otherwise accept this control.
        let array = match at {
            Some(_) => json!([object["sha256"], object["bytes"]]),
            None => json!([
                object["version"],
                object["runtime"],
                object["pool"],
                object["registryVersion"],
                object["metadata"],
                object["snapshot"],
                object["vocabulary"],
                object["nameRules"],
            ]),
        };
        let mut unknown = object.clone();
        unknown["unknown"] = json!(true);
        for replacement in [unknown, array] {
            let mut changed = valid.clone();
            match at {
                Some(at) => changed[at] = replacement,
                None => changed = replacement,
            }
            reject(&serde_json::to_vec(&changed).unwrap());
        }
    }
    for bytes in [&b""[..], &vec![b' '; MAX_PROGRAM_IDENTITY_BYTES + 1]] {
        assert!(matches!(
            verify_program_identity(bytes, vocabulary, &descriptor.runtime, &expected),
            Err(NativeInitialError::Size)
        ));
    }
}

#[test]
#[ignore = "requires the saved catalogue and vocabulary to open both seats through the keyword source pool"]
fn keyword_pool_opens_a_game_with_real_lands_and_morph_and_foretell_cards() {
    let source = source(NativeProgramPool::KeywordGameplay);
    let oracle = |name: &str| {
        source
            .catalogue
            .iter()
            .find(|(_, card)| card.name == name)
            .unwrap_or_else(|| panic!("missing {name}"))
            .0
            .clone()
    };
    let mut spec = spec();
    for p in &mut spec.participants {
        p.cards = [
            "Island",
            "Plains",
            "Zoetic Cavern",
            "Scorn Effigy",
            "Durkwood Baloth",
            "Scab-Clan Mauler",
            "Putrid Goblin",
            "Saw It Coming",
        ]
        .into_iter()
        .map(|name| NativeInitialCard {
            oracle_id: oracle(name),
            zone: NativeInitialZone::Library,
        })
        .collect();
    }
    let expected_cards = spec
        .participants
        .iter()
        .map(|p| p.cards.len())
        .sum::<usize>();
    let opened = prepare_initial(ids(), source, spec).unwrap();
    assert_eq!(
        opened
            .candidate()
            .state()
            .execution()
            .window()
            .cards()
            .iter()
            .count(),
        expected_cards
    );
    let effigy = source
        .registry
        .seed(
            &oracle("Scorn Effigy"),
            CardId(1),
            SeatId(0),
            ObjectDestination::Hand,
        )
        .unwrap();
    let printed = source.printed_card(&effigy.program).unwrap().unwrap();
    assert_eq!(printed.characteristics().name, "Scorn Effigy");
    assert!(printed.characteristics().rules_text.contains("Foretell"));
}
