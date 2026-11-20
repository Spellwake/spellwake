//! Genuine legacy Scheduled fixture preservation and error/timing controls.
use super::*;
use crate::{ordinary_game::record, projection};
use serde_json::json;
use spellwake_engine::game::{GameError, GameState};
use spellwake_engine::turn::TurnError;
use std::path::PathBuf;

fn required(name: &str) -> PathBuf {
    std::env::var_os(name)
        .map(PathBuf::from)
        .unwrap_or_else(|| panic!("explicit {name} required"))
}
fn observe(fixture: &Fixture) -> Vec<u8> {
    let raw = record::static_input_bytes_for_test(fixture).unwrap();
    serde_json::to_vec(&json!({
        "fixture_debug":format!("{fixture:?}"),
        "seed_input":projection::game_seed_input(&fixture.setup.seed),
        "context_input":projection::context(&fixture.context),
        // Preserve the maintained serializer's exact bytes, including key order.
        "static_inputs_raw":String::from_utf8(raw).unwrap(),
    }))
    .unwrap()
}
fn save(name: &str, bytes: &[u8]) {
    if let Some(directory) = std::env::var_os("SPELLWAKE_SCHEDULED_OBSERVATIONS_OUTPUT") {
        let directory = PathBuf::from(directory);
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(directory.join(name), bytes).unwrap();
    }
}

#[test]
#[ignore = "requires actual pinned catalogue and source-qualified vocabulary; fixture/serializer preservation only"]
fn mandatory_scheduled_fixture_and_static_bytes_are_preserved() {
    let vocabulary = crate::creature_types::selected_vocabulary().unwrap();
    let source = MandatorySource::load(
        &required("SPELLWAKE_CATALOGUE_METADATA"),
        &required("SPELLWAKE_CATALOGUE_INPUT"),
        &vocabulary,
    )
    .unwrap();
    let mut digests = Vec::new();
    for case in MandatoryCase::ALL
        .into_iter()
        .filter(|&case| !MandatoryCase::FULL_GAMES.contains(&case))
    {
        let fixture = source.fixture(case, vocabulary.clone()).unwrap();
        let bytes = observe(&fixture);
        let digest = format!("{:x}", Sha256::digest(&bytes));
        save(&format!("mandatory-{}.json", case.as_str()), &bytes);
        digests.push((case.as_str(), digest));
    }
    assert_eq!(digests.len(), 21);
    save(
        "mandatory-digests.json",
        &serde_json::to_vec(&digests).unwrap(),
    );
    let expected = [
        (
            "T01.own-then-opponent",
            "8aebe3b1a587aab2aa9b52132a92201aaf2966cffcf6112cf934947aa3601acd",
        ),
        (
            "T01.opponent-active",
            "2c2da053d08c1cbdb13b5cd8d8edecb359679085513911188822530c44d84686",
        ),
        (
            "T02.own-main",
            "7ef25b3c0614342b07b924787cb4985914ab56c70b22a31809c36fa18b7ce96a",
        ),
        (
            "T02.opponent-main",
            "b5f774e0fea116263af4e9acc199a8bf1b29d05dcff7646d32585d23fcbedf5a",
        ),
        (
            "T03.countered",
            "9777bff0b4c66e5c8dc5a7de55ea013d7f9ef6418f59584d551c6d921d4e6349",
        ),
        (
            "T03.departed-recast",
            "9cbc459aa2f00ac600afa9bd906f5399d72bbc1912a701c8f053de6941f420d3",
        ),
        (
            "T04.one-card",
            "da25f1794c60815a63d08bafbe08dcfc0f1ca49877b2a5ed68852fa1b5f67978",
        ),
        (
            "T04.empty",
            "03d0ea30695d05e6ff219b6efd9376fb99e5f9f83a19eea6d970ecda0662e679",
        ),
        (
            "T04.one-life",
            "cc54824928af7c95e5e47f59c9b0f4375da648f9e2efa8ce00239aecdb609e55",
        ),
        (
            "T04.empty-one-life",
            "01f2038d49ebf789b3a24e90333b2df6dd40d8b5fab7a1cb741af70946dcd2db",
        ),
        (
            "T05.combat-dies",
            "2271d5ac71f0b85ff42017a8fe93b4243a13abc65d5581a14c6d9bb54f300b1c",
        ),
        (
            "T05.bounce-not-dies",
            "b713dc09d368ef7dbb15d13ea927e9d7d0f8a04c59b12297bb5478ce99da73d8",
        ),
        (
            "T06.dissenter-top",
            "4cfe3f182cca2757941e646f41573cccb7cbe3e9805267670904d7cd1ffcc624",
        ),
        (
            "T06.artist-top",
            "0740311534aa1860c00cddaa56411dfed73c00c1987d6c13b13129a6ed95097a",
        ),
        (
            "T06.split-targets",
            "e77ade75d8937f38d94b71919d5d378577bbfdb37279772134232c364649bb51",
        ),
        (
            "T07.death-birth-entry",
            "1a96790aa48d49a386f6b3f3a5080b693e478359500b5257ac53109513b2f945",
        ),
        (
            "T08.self",
            "22a6a63704dcfd1a46cdce5160ad407b2ab869ecadd3aadd1d41b3826228f4cd",
        ),
        (
            "T08.opponent",
            "d890a8e125bf99cc9fd52d9bbc106c53a3367df5cd4bb94c2d8065851a7ed584",
        ),
        (
            "T09.fresh-next-turn",
            "e577968a3dc03d363cf34384ce87117fb62d578fa633032dfd54ad8890b61d3a",
        ),
        (
            "T09.bounce-recast-stale",
            "b1e057e98883231369f242f1c9fe1e8ff318489a03cdc55e7092a53d07b681fd",
        ),
        (
            "T11.token-death-cessation",
            "8343317858cb5f50bac6f4e9c40a37ff3b48c27e0eb7610e8587dc21f2b4c274",
        ),
    ];
    assert_eq!(
        digests
            .iter()
            .map(|(case, digest)| (*case, digest.as_str()))
            .collect::<Vec<_>>(),
        expected
    );
}

#[test]
#[ignore = "requires actual pinned catalogue and source-qualified vocabulary; legacy construction boundary controls"]
fn mandatory_scheduled_errors_ordering_and_turn_zero_timing_are_preserved() {
    let vocabulary = crate::creature_types::selected_vocabulary().unwrap();
    let mut source = MandatorySource::load(
        &required("SPELLWAKE_CATALOGUE_METADATA"),
        &required("SPELLWAKE_CATALOGUE_INPUT"),
        &vocabulary,
    )
    .unwrap();
    let case = MandatoryCase::T01OwnThenOpponent;
    let original = source
        .inputs
        .cases
        .iter()
        .find(|row| row.id == case)
        .unwrap()
        .clone();
    let mut construct = |modified: &Case<MandatoryCase>| {
        let row = source
            .inputs
            .cases
            .iter_mut()
            .find(|row| row.id == case)
            .unwrap();
        *row = modified.clone();
        source.fixture(case, vocabulary.clone())
    };
    let mut outcomes = Vec::<(String, String)>::new();
    for scenario in [
        "reversed_roles",
        "omitted_role",
        "duplicate_role",
        "unknown_hand",
        "unknown_library",
        "unknown_graveyard_reverse",
        "unknown_exile",
        "foreign_zone",
        "duplicate_zone",
        "unknown_battlefield",
        "unknown_battlefield_before_tapped",
        "duplicate_battlefield",
        "duplicate_zone_battlefield",
        "tapped_battlefield",
        "incomplete",
        "bad_life_before_roles",
    ] {
        let mut modified = original.clone();
        let Initialization::Scheduled {
            zones,
            battlefield_oldest_first,
            life,
            ..
        } = &mut modified.initialization
        else {
            panic!("scheduled genuine case")
        };
        match scenario {
            "reversed_roles" => zones.swap(0, 1),
            "omitted_role" => {
                zones.pop();
            }
            "duplicate_role" => zones[1].role = Role::P0,
            "unknown_hand" => zones[0].hand.insert(0, "unknown.hand".into()),
            "unknown_library" => zones[0]
                .library_top_first
                .insert(0, "unknown.library".into()),
            "unknown_graveyard_reverse" => {
                zones[0].graveyard_top_first = vec![
                    "unknown.graveyard.first".into(),
                    "unknown.graveyard.last".into(),
                ]
            }
            "unknown_exile" => zones[0].exile.insert(0, "unknown.exile".into()),
            "foreign_zone" => {
                let card = zones[0].hand.remove(0);
                zones[1].hand.insert(0, card);
            }
            "duplicate_zone" => {
                let card = zones[0].hand[0].clone();
                zones[0].hand.push(card);
            }
            "unknown_battlefield" => {
                battlefield_oldest_first[0].alias = "unknown.battlefield".into()
            }
            "unknown_battlefield_before_tapped" => {
                battlefield_oldest_first[0].alias = "unknown.battlefield".into();
                battlefield_oldest_first[0].tapped = true;
            }
            "duplicate_battlefield" => {
                battlefield_oldest_first.push(battlefield_oldest_first[0].clone())
            }
            "duplicate_zone_battlefield" => {
                battlefield_oldest_first[0].alias = zones[0].hand[0].clone()
            }
            "tapped_battlefield" => battlefield_oldest_first[0].tapped = true,
            "incomplete" => {
                zones[0].hand.remove(0);
            }
            "bad_life_before_roles" => {
                life.p0 = "020".into();
                zones.swap(0, 1);
            }
            _ => unreachable!(),
        }
        let error = construct(&modified).unwrap_err();
        let expected = match scenario {
            "reversed_roles" | "omitted_role" | "duplicate_role" => {
                "mandatory scheduled role partition mismatch"
            }
            "unknown_hand" => "unknown fixture alias unknown.hand",
            "unknown_library" => "unknown fixture alias unknown.library",
            "unknown_graveyard_reverse" => "unknown fixture alias unknown.graveyard.last",
            "unknown_exile" => "unknown fixture alias unknown.exile",
            "unknown_battlefield" | "unknown_battlefield_before_tapped" => {
                "unknown fixture alias unknown.battlefield"
            }
            "foreign_zone" | "duplicate_zone" => "mandatory duplicate/foreign zone member",
            "duplicate_battlefield" | "duplicate_zone_battlefield" | "tapped_battlefield" => {
                "mandatory battlefield premise mismatch"
            }
            "incomplete" => "mandatory incomplete physical partition",
            "bad_life_before_roles" => {
                "invalid mandatory integer: expected a canonical decimal integer string"
            }
            _ => unreachable!(),
        };
        assert_eq!(error, expected, "{scenario}");
        outcomes.push((scenario.into(), error));
    }
    let mut zero = original.clone();
    let Initialization::Scheduled { turn, .. } = &mut zero.initialization else {
        panic!("scheduled")
    };
    *turn = 0;
    let fixture = construct(&zero).expect("legacy construction must still accept turn zero");
    let FixtureInitialization::Scheduled(start) = fixture.initialization else {
        panic!("scheduled")
    };
    assert_eq!(start.number, 0);
    let actual = GameState::initialize_scheduled(
        fixture.game,
        fixture.context.clone(),
        fixture.setup.seed.clone(),
        start,
    );
    assert_eq!(actual, Err(GameError::Turn(TurnError::InvalidStart)));
    outcomes.push((
        "turn_zero_constructed_native_rejects".into(),
        format!("{actual:?}"),
    ));
    // A truthful derivative tests all zone orderings absent from some frozen rows.
    let mut ordered = original.clone();
    let Initialization::Scheduled {
        zones,
        battlefield_oldest_first,
        life,
        active,
        ..
    } = &mut ordered.initialization
    else {
        panic!("scheduled")
    };
    *active = Role::P1;
    life.p0 = "0".into();
    life.p1 = "-1".into();
    let a = zones[0].hand.remove(0);
    let b = zones[0].library_top_first.remove(0);
    zones[0].graveyard_top_first = vec![a.clone(), b.clone()];
    let ex = zones[1].library_top_first.remove(0);
    zones[1].exile.push(ex);
    battlefield_oldest_first[0].controller = match battlefield_oldest_first[0].controller {
        Role::P0 => Role::P1,
        Role::P1 => Role::P0,
    };
    let fixture = construct(&ordered).unwrap();
    assert_eq!(fixture.setup.seed.active, B);
    assert_eq!(fixture.setup.seed.life[0].total, 0.into());
    assert_eq!(fixture.setup.seed.life[1].total, (-1).into());
    assert!(fixture.setup.seed.mana.is_empty());
    let actual_aliases = fixture
        .setup
        .seed
        .cards
        .iter()
        .map(|card| fixture.alias(card.card).unwrap().alias.clone())
        .collect::<Vec<_>>();
    assert_eq!(
        actual_aliases.iter().position(|v| v == &b).unwrap() + 1,
        actual_aliases.iter().position(|v| v == &a).unwrap()
    );
    let static_result = record::static_input_bytes_for_test(&fixture);
    assert_eq!(
        static_result,
        Err("ordinary fixture seed is not library-only".into())
    );
    let bytes = serde_json::to_vec(&json!({
        "fixture_debug":format!("{fixture:?}"),
        "seed_input":projection::game_seed_input(&fixture.setup.seed),
        "context_input":projection::context(&fixture.context),
        "static_inputs_result":format!("{static_result:?}"),
    }))
    .unwrap();
    save("mandatory-ordered-derivative.json", &bytes);
    outcomes.push((
        "ordered_derivative_hash".into(),
        format!("{:x}", Sha256::digest(&bytes)),
    ));
    assert_eq!(
        outcomes.last().unwrap().1,
        "d3ea739032db4ea3f7f7d9c1901f5a8ad415a3f60e98ac625a5c2e37e4c67a77"
    );
    save(
        "mandatory-controls.json",
        &serde_json::to_vec(&outcomes).unwrap(),
    );
}

#[test]
fn old_scheduled_grammar_keeps_five_mana_types_and_no_seeded_pool_field() {
    for name in ["White", "Blue", "Black", "Red", "Green"] {
        assert!(serde_json::from_value::<Mana>(json!(name)).is_ok());
    }
    assert!(serde_json::from_value::<Mana>(json!("Colourless")).is_err());
    let raw: serde_json::Value =
        serde_json::from_str(include_str!("../mandatory-inputs-v1.json")).unwrap();
    let initial = raw["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["initialization"]["kind"] == "scheduled")
        .unwrap()["initialization"]
        .clone();
    assert!(serde_json::from_value::<Initialization>(initial.clone()).is_ok());
    let mut changed = initial.clone();
    changed["floating_mana"] = json!([]);
    assert!(serde_json::from_value::<Initialization>(changed).is_err());
    let mut changed = initial;
    changed["zones"][0]["role"] = json!("P2");
    assert!(serde_json::from_value::<Initialization>(changed).is_err());
}
