//! Opt-in actual Rust HTTP -> managed private RPC observation. No native bytes.
use super::*;
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RoomQuery {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    room_id: Uuid,
    basis: Amount,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    release_id: Uuid,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    legality_id: Uuid,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct BetweenQuery {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    decision_id: Uuid,
    basis: Amount,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    selected_revision_id: Uuid,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Fixture {
    scope: String,
    base: String,
    secret: String,
    #[serde(deserialize_with = "crate::provider_json::object")]
    room: RoomQuery,
    #[serde(deserialize_with = "crate::provider_json::object")]
    between: BetweenQuery,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    reserved_game_id: Uuid,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    opening_game_id: Uuid,
}
#[test]
#[ignore = "requires owned live managed Supabase preparation sources"]
fn managed_preparation_sources_use_actual_postgrest() {
    let path = std::env::var("SPELLWAKE_MANAGED_PREPARATION_INPUT")
        .expect("owned managed fixture required");
    let raw = std::fs::read(path).expect("owned fixture readable");
    assert!(raw.len() <= 8192);
    let f: Fixture = crate::decode_wire_object(&raw).expect("valid owned fixture");
    assert_eq!(f.scope, "spellwake-owned-native-preparation-v1");
    let origin = reqwest::Url::parse(&f.base).expect("owned loopback origin");
    assert!(origin.scheme() == "http" && origin.host_str() == Some("127.0.0.1"));
    let source_repo = crate::native_game::NativeSourceRepository::new(
        BackendClient::new(
            BackendConfig::new(&f.base, &f.secret).expect("owned backend"),
            "managed-opening-contract",
            Duration::from_secs(10),
        )
        .expect("owned transport"),
    );
    let original = source_repo
        .opening(f.opening_game_id)
        .expect("actual opening RPC")
        .expect("accepted original");
    assert_eq!(original.ids().1, f.opening_game_id);
    assert_eq!(original.native_epoch().get(), 0);
    assert!(source_repo.opening(Uuid::new_v4()).unwrap().is_none());
    let repo = NativePreparationRepository::new(
        BackendClient::new(
            BackendConfig::new(&f.base, &f.secret).expect("owned backend"),
            "managed-preparation-contract",
            Duration::from_secs(10),
        )
        .expect("owned transport"),
    );
    let room = repo
        .room(
            f.room.room_id,
            &f.room.basis,
            f.room.release_id,
            f.room.legality_id,
        )
        .expect("actual room RPC")
        .expect("available room");
    assert_eq!(room.terms().room.id, f.room.room_id);
    assert!(!room.terms().slots.is_empty());
    let between = repo
        .between(
            f.between.decision_id,
            &f.between.basis,
            f.between.selected_revision_id,
        )
        .expect("actual between RPC")
        .expect("available decision");
    assert_eq!(
        between.terms().selected_revision_id,
        f.between.selected_revision_id
    );
    assert!(!between.terms().entries.is_empty());
    let game = repo
        .reserved_game(f.reserved_game_id)
        .expect("actual reserved RPC")
        .expect("available reservation");
    assert_eq!(game.metadata().game_id, f.reserved_game_id);
    assert!(game.metadata().game_index.is_zero());
    assert!(
        repo.room(
            Uuid::new_v4(),
            &f.room.basis,
            f.room.release_id,
            f.room.legality_id
        )
        .unwrap()
        .is_none()
    );
    assert!(
        repo.between(
            Uuid::new_v4(),
            &f.between.basis,
            f.between.selected_revision_id
        )
        .unwrap()
        .is_none()
    );
    assert!(repo.reserved_game(Uuid::new_v4()).unwrap().is_none());
}
