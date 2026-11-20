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
fn optional_scheduled_fixture_and_static_bytes_are_preserved() {
    let vocabulary = crate::creature_types::selected_vocabulary().unwrap();
    let source = OptionalSource::load(
        &required("SPELLWAKE_CATALOGUE_METADATA"),
        &required("SPELLWAKE_CATALOGUE_INPUT"),
        &vocabulary,
    )
    .unwrap();
    let mut digests = Vec::new();
    for case in OptionalCase::ALL
        .into_iter()
        .filter(|&case| !case.is_full_game())
    {
        let fixture = source.fixture(case, vocabulary.clone()).unwrap();
        let bytes = observe(&fixture);
        let digest = format!("{:x}", Sha256::digest(&bytes));
        save(&format!("optional-{}.json", case.as_str()), &bytes);
        digests.push((case.as_str(), digest));
    }
    assert_eq!(digests.len(), 24);
    save(
        "optional-digests.json",
        &serde_json::to_vec(&digests).unwrap(),
    );
    let expected = [
        (
            "OP-R01",
            "43b4228d075801a4a32136b464cfee67b3bc25daa54785e0e5404df512a7250d",
        ),
        (
            "OP-R02",
            "0dae843997aeead8f4ecb7dee9c61f2668857c7fa7715e8d8af958318c357791",
        ),
        (
            "OP-R03",
            "ea953be3f6425cf2bb6da0199c9cee2eb95a2ff210c05249352ce833a91c8eac",
        ),
        (
            "OP-R04",
            "2ddad7d6d00d04cd348ed9fe3c3df2f7611c0661d02461f6a313d710229a4219",
        ),
        (
            "OP-R05",
            "52365eacbbb290f41540e5396b48bfb4b58109fe16765eeaf548a181e779b328",
        ),
        (
            "OP-R06",
            "09ef502bd038a0f50a44e6dc89267f0acf6baa66be0e08c984116d99a496be5f",
        ),
        (
            "OP-R07",
            "cb74b7e51fb7b437ed395f0e1265455f55def83ed446b85deaf0d1a26498af92",
        ),
        (
            "OP-R08",
            "d3afe1986cbb2fda1e6668585e6e6c9ceaaebb953dd5c57e8710f7c01d347160",
        ),
        (
            "OP-R09",
            "4582e0fd22e71946aa7119f2d64ef1bd2f85385e18365813c114f6fb26bdb9ce",
        ),
        (
            "OP-R10",
            "8a909e3a36d1a0999abc9908fa59630c02c7076dc24f204e0ad25ea7b6ac05b1",
        ),
        (
            "OP-R11",
            "039a7f4062a56ae2d7d1fff242af846af8266adde094e9c6e0add25ee3e1e41a",
        ),
        (
            "OP-R12",
            "9732917be2c759216d3274fa0bed658bcc7c6cc1e07a573075acadd51fe8eef6",
        ),
        (
            "OP-R13",
            "875d8b3f436ff721c5f0163b3bf846b543954fd289cef75d2ef00bd7a29a785d",
        ),
        (
            "OP-R14",
            "1c0d07b6b6202456544039a78a231d41edd0a529d7a9f6e1cabdf2633d925d08",
        ),
        (
            "OP-R15",
            "b1585c76953b9712a53ada19f680547467ac0f4dece2c255530fd88e6944fe4f",
        ),
        (
            "OP-R16",
            "41170bc86e2892545e89903f0a84d6aa546c4fe021a52260b416bfec33e4a93a",
        ),
        (
            "OP-R17",
            "9876b923b190e87303e8bb6330d2c38625ba6b74e1a21fd6d4f3506339e64e20",
        ),
        (
            "OP-R18",
            "538a5a148b9d41708b964b94529c6a61857fa3d4319d0dd22ea15dd68f49f3be",
        ),
        (
            "OP-R19",
            "11298e78ec824cea270f03d24b2da56977ac41240b409347cb565d08165c58c3",
        ),
        (
            "OP-R20",
            "72ff398ba8ffa33b0378f5bdfa037ffa34f0e12d87e02ef4e83ecfcdb3c35015",
        ),
        (
            "OP-R21",
            "f5367a9af65521b0be03fcca15ea67e0b91709b41f326c4f9a47c1c20213ee7b",
        ),
        (
            "OP-R22",
            "5ac73b4329d9a6059fd1e40a5094bdb413796130cc0390db94c6044088842064",
        ),
        (
            "OP-R23",
            "e4d2332f47e0be90669e991b25ba31f78305203458d0ca387eff0e28f37ac914",
        ),
        (
            "OP-R24",
            "fe7bd2f72ab6fbef7fcdb99c1280c619fe0616c6c3ed45c0320d91001d646459",
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
fn optional_scheduled_errors_ordering_and_turn_zero_timing_are_preserved() {
    let vocabulary = crate::creature_types::selected_vocabulary().unwrap();
    let source = OptionalSource::load(
        &required("SPELLWAKE_CATALOGUE_METADATA"),
        &required("SPELLWAKE_CATALOGUE_INPUT"),
        &vocabulary,
    )
    .unwrap();
    let case = OptionalCase::R11;
    let original = recipe(case).unwrap();
    let construct =
        |modified: &Case<CaseId>| source.fixture_from_recipe(case, vocabulary.clone(), modified);
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
                "optional prepared role partition mismatch"
            }
            "unknown_hand" => "unknown fixture alias unknown.hand",
            "unknown_library" => "unknown fixture alias unknown.library",
            "unknown_graveyard_reverse" => "unknown fixture alias unknown.graveyard.last",
            "unknown_exile" => "unknown fixture alias unknown.exile",
            "unknown_battlefield" | "unknown_battlefield_before_tapped" => {
                "unknown fixture alias unknown.battlefield"
            }
            "foreign_zone" | "duplicate_zone" => "optional duplicate or foreign zone member",
            "duplicate_battlefield" | "duplicate_zone_battlefield" | "tapped_battlefield" => {
                "optional unsupported or duplicate initial battlefield member"
            }
            "incomplete" => "optional incomplete physical partition",
            "bad_life_before_roles" => {
                "optional initial life: expected a canonical decimal integer string"
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
    let bytes = observe(&fixture);
    save("optional-ordered-derivative.json", &bytes);
    outcomes.push((
        "ordered_derivative_hash".into(),
        format!("{:x}", Sha256::digest(&bytes)),
    ));
    assert_eq!(
        outcomes.last().unwrap().1,
        "4a92c8b76dc17d3fbd74042cdf6fd6417fc8beeb717bc49ec2ddf8bcdd009af9"
    );
    save(
        "optional-controls.json",
        &serde_json::to_vec(&outcomes).unwrap(),
    );
}
