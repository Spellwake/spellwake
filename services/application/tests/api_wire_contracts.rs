//! The same fixed wire artifact runs through native and TypeScript boundary validators.
use serde::Deserialize;
use spellwake_application::{
    decks::import::{ImportAck, ImportDeck, ImportDisposition},
    matchmaking::{
        EnqueueAck, EnqueueMatchmaking, ProfileConfigurationPage, QueueIntent, QueuePage,
        TableProfilePage,
    },
    progression::{QuestPage, RankPage, SeasonPage},
    rooms::{
        RoomCommand,
        configuration::{ConfigurationAck, ConfigureRoom, MatchStructure},
        invitations::{IssueInvitation, IssuedInvitation, JoinAck, JoinRoom, RevokeInvitation},
        reads::RoomDetail,
    },
};
use uuid::Uuid;
#[path = "support/human_wire.rs"]
mod human_wire;
#[derive(Deserialize)]
struct Artifact {
    owner: Uuid,
    cases: Vec<Case>,
}
#[derive(Deserialize)]
struct Case {
    #[serde(rename = "targetId")]
    target_id: Option<Uuid>,
    operation: Option<String>,
    name: String,
    kind: String,
    wire: String,
    valid: bool,
    #[serde(rename = "formatId")]
    format_id: Option<Uuid>,
    #[serde(rename = "seasonId")]
    season_id: Option<Uuid>,
    #[serde(rename = "roomId")]
    room_id: Option<Uuid>,
    #[serde(rename = "intentId")]
    intent_id: Option<Uuid>,
    #[serde(rename = "profileId")]
    profile_id: Option<Uuid>,
    #[serde(rename = "afterIndex")]
    after_index: Option<u16>,
    #[serde(rename = "matchId")]
    match_id: Option<Uuid>,
    #[serde(rename = "playerId")]
    player_id: Option<Uuid>,
    limit: Option<usize>,
    #[serde(rename = "expectedRevision")]
    expected_revision: Option<spellwake_application::amount::Amount>,
}
#[test]
fn shared_api_wire_contracts_match_typescript_expectations() {
    let artifact: Artifact = serde_json::from_str(include_str!(
        "../../../packages/application-contracts/fixtures/api-tier-wire.json"
    ))
    .unwrap();
    for case in artifact.cases {
        let valid = match case.kind.as_str() {
            "asset_identity" => spellwake_application::decode_wire_object::<
                spellwake_application::inventory::AssetIdentity,
            >(case.wire.as_bytes())
            .is_ok(),
            "deck_library" => spellwake_application::decode_wire_object::<
                spellwake_application::account::DeckLibraryPage,
            >(case.wire.as_bytes())
            .is_ok_and(|v| v.validate_after(100, None).is_ok()),
            "profile_art_command" => spellwake_application::decode_wire_object::<
                spellwake_application::profile_art::ChangeProfileArt,
            >(case.wire.as_bytes())
            .is_ok_and(|v| v.validate().is_ok()),
            "own_profile_art" => spellwake_application::decode_wire_object::<
                spellwake_application::profile_art::OwnProfileArt,
            >(case.wire.as_bytes())
            .is_ok_and(|v| v.validate(artifact.owner).is_ok()),
            "public_profile_art" => spellwake_application::decode_wire_object::<
                spellwake_application::profile_art::PublicProfileArt,
            >(case.wire.as_bytes())
            .is_ok_and(|v| v.validate(artifact.owner, case.player_id.unwrap()).is_ok()),
            "game_chat" => spellwake_application::decode_wire_object::<
                spellwake_application::game_chat::GameChatPage,
            >(case.wire.as_bytes())
            .is_ok_and(|v| {
                v.validate(artifact.owner, case.match_id.unwrap(), None, 16)
                    .is_ok()
            }),
            "send_game_message" => spellwake_application::decode_wire_object::<
                spellwake_application::game_chat::SendGameMessage,
            >(case.wire.as_bytes())
            .is_ok_and(|v| !v.request_id.is_nil()),
            "notifications" => spellwake_application::decode_wire_object::<
                spellwake_application::notifications::NotificationPage,
            >(case.wire.as_bytes())
            .is_ok_and(|v| v.validate(artifact.owner, None, 100).is_ok()),
            "read_notifications" => spellwake_application::decode_wire_object::<
                spellwake_application::notifications::ReadNotifications,
            >(case.wire.as_bytes())
            .is_ok_and(|v| v.validate().is_ok()),
            "enqueue" => spellwake_application::decode_wire_object::<EnqueueMatchmaking>(
                case.wire.as_bytes(),
            )
            .is_ok_and(|v| v.validate().is_ok()),
            "enqueue_ack" => {
                spellwake_application::decode_wire_object::<EnqueueAck>(case.wire.as_bytes())
                    .is_ok_and(|v| v.valid())
            }
            "queue_page" => {
                spellwake_application::decode_wire_object::<QueuePage>(case.wire.as_bytes())
                    .is_ok_and(|v| {
                        v.validate(artifact.owner, case.intent_id, None, 100)
                            .is_ok()
                    })
            }
            "queue_intent" => {
                spellwake_application::decode_wire_object::<QueueIntent>(case.wire.as_bytes())
                    .is_ok_and(|v| v.validate(artifact.owner, case.intent_id.unwrap()).is_ok())
            }
            "profile_page" => {
                spellwake_application::decode_wire_object::<TableProfilePage>(case.wire.as_bytes())
                    .is_ok_and(|v| v.validate(artifact.owner, None, 100).is_ok())
            }
            "profile_configurations" => spellwake_application::decode_wire_object::<
                ProfileConfigurationPage,
            >(case.wire.as_bytes())
            .is_ok_and(|v| {
                v.validate(
                    artifact.owner,
                    case.profile_id.unwrap(),
                    case.after_index,
                    case.limit.unwrap_or(100),
                )
                .is_ok()
            }),
            "issue_invitation" => {
                spellwake_application::decode_wire_object::<IssueInvitation>(case.wire.as_bytes())
                    .is_ok_and(|v| !v.request_id.is_nil())
            }
            "join_room" => {
                spellwake_application::decode_wire_object::<JoinRoom>(case.wire.as_bytes())
                    .is_ok_and(|v| !v.request_id.is_nil())
            }
            "revoke_invitation" => {
                spellwake_application::decode_wire_object::<RevokeInvitation>(case.wire.as_bytes())
                    .is_ok_and(|v| !v.request_id.is_nil())
            }
            "issued_invitation" => {
                spellwake_application::decode_wire_object::<IssuedInvitation>(case.wire.as_bytes())
                    .is_ok_and(|v| !v.resource_id.is_nil())
            }
            "join_ack" => {
                spellwake_application::decode_wire_object::<JoinAck>(case.wire.as_bytes())
                    .is_ok_and(|v| v.valid(case.room_id.unwrap()))
            }
            "configuration_ack" => {
                let input = ConfigureRoom {
                    request_id: artifact.owner,
                    profile_id: artifact.owner,
                    expected_revision: case.expected_revision.clone().unwrap(),
                    structure: MatchStructure::Bo1,
                    teams: vec![],
                };
                spellwake_application::decode_wire_object::<ConfigurationAck>(case.wire.as_bytes())
                    .is_ok_and(|ack| input.valid_ack(case.room_id.unwrap(), &ack))
            }
            "configuration" => {
                spellwake_application::decode_wire_object::<ConfigureRoom>(case.wire.as_bytes())
                    .is_ok_and(|v| v.validate().is_ok())
            }
            "room_detail" => {
                spellwake_application::decode_wire_object::<RoomDetail>(case.wire.as_bytes())
                    .is_ok_and(|v| {
                        v.validate(artifact.owner, case.room_id.unwrap(), None, 100)
                            .is_ok()
                    })
            }
            "room" => {
                spellwake_application::decode_wire_object::<RoomCommand>(case.wire.as_bytes())
                    .is_ok_and(|v| v.validate().is_ok())
            }
            "import" => {
                spellwake_application::decode_wire_object::<ImportDeck>(case.wire.as_bytes())
                    .is_ok_and(|v| v.validate().is_ok())
            }
            "ack" => {
                spellwake_application::decode_wire_object::<ImportAck>(case.wire.as_bytes()).is_ok()
            }
            "disposition" => {
                spellwake_application::decode_wire_object::<ImportDisposition>(case.wire.as_bytes())
                    .is_ok()
            }
            "quests" => {
                spellwake_application::decode_wire_object::<QuestPage>(case.wire.as_bytes())
                    .is_ok_and(|v| v.validate(artifact.owner, None, 100).is_ok())
            }
            "ranks" => spellwake_application::decode_wire_object::<RankPage>(case.wire.as_bytes())
                .is_ok_and(|v| {
                    v.validate(artifact.owner, None, 100, case.format_id, case.season_id)
                        .is_ok()
                }),
            "seasons" => {
                spellwake_application::decode_wire_object::<SeasonPage>(case.wire.as_bytes())
                    .is_ok_and(|v| v.validate(artifact.owner, None, 100).is_ok())
            }
            other => human_wire::valid(&case, artifact.owner)
                .unwrap_or_else(|| panic!("unknown fixture kind {other}")),
        };
        assert_eq!(valid, case.valid, "{}", case.name);
    }
}

#[test]
fn adjacent_command_mappings_preserve_all_serialized_variants() {
    use serde::de::DeserializeOwned;
    fn roundtrip<T: DeserializeOwned + serde::Serialize>(wire: &str) {
        let value: T = spellwake_application::decode_wire_object(wire.as_bytes()).unwrap();
        assert_eq!(
            serde_json::to_value(value).unwrap(),
            serde_json::from_str::<serde_json::Value>(wire).unwrap()
        );
    }
    let artifact: Artifact = serde_json::from_str(include_str!(
        "../../../packages/application-contracts/fixtures/api-tier-wire.json"
    ))
    .unwrap();
    let mut mappings = 0;
    for case in artifact
        .cases
        .iter()
        .filter(|case| case.name.starts_with("enum-command-"))
    {
        match case.kind.as_str() {
            "deck_command" => roundtrip::<spellwake_application::decks::DeckCommand>(&case.wire),
            "social_command" => {
                roundtrip::<spellwake_application::commands::SocialCommand>(&case.wire)
            }
            "economy_command" => {
                roundtrip::<spellwake_application::economy_commands::EconomyCommand>(&case.wire)
            }
            "prediction_command" => {
                roundtrip::<spellwake_application::predictions::PredictionCommand>(&case.wire)
            }
            "room" => roundtrip::<spellwake_application::rooms::RoomCommand>(&case.wire),
            _ => panic!("unexpected command fixture"),
        }
        mappings += 1;
    }
    assert_eq!(mappings, 20);
}

#[test]
fn adjacent_command_original_duplicate_keys_never_normalize_before_decode() {
    use serde::de::DeserializeOwned;
    fn reject<T: DeserializeOwned>(wire: &str) {
        assert!(spellwake_application::decode_wire_object::<T>(wire.as_bytes()).is_err());
    }
    let artifact: Artifact = serde_json::from_str(include_str!(
        "../../../packages/application-contracts/fixtures/api-tier-wire.json"
    ))
    .unwrap();
    let cases: Vec<_> = artifact
        .cases
        .iter()
        .filter(|case| {
            matches!(
                case.name.as_str(),
                "enum-command-deck_command-create"
                    | "enum-command-social_command-friendship"
                    | "enum-command-economy_command-trade_create"
                    | "enum-command-prediction_command-publish_market"
                    | "enum-command-room-create"
            )
        })
        .collect();
    assert_eq!(cases.len(), 5);
    for case in cases {
        let body: serde_json::Value = serde_json::from_str(&case.wire).unwrap();
        for wire in [
            format!("{{\"kind\":{},{}", body["kind"], &case.wire[1..]),
            format!("{{\"k\\u0069nd\":{},{}", body["kind"], &case.wire[1..]),
            format!("{{\"command\":{},{}", body["command"], &case.wire[1..]),
            case.wire.replacen(
                "\"requestId\":",
                "\"requestId\":\"11111111-1111-4111-8111-111111111111\",\"requestId\":",
                1,
            ),
            case.wire.replacen(
                "\"requestId\":",
                "\"request\\u0049d\":\"11111111-1111-4111-8111-111111111111\",\"requestId\":",
                1,
            ),
        ] {
            match case.kind.as_str() {
                "deck_command" => reject::<spellwake_application::decks::DeckCommand>(&wire),
                "social_command" => reject::<spellwake_application::commands::SocialCommand>(&wire),
                "economy_command" => {
                    reject::<spellwake_application::economy_commands::EconomyCommand>(&wire)
                }
                "prediction_command" => {
                    reject::<spellwake_application::predictions::PredictionCommand>(&wire)
                }
                "room" => reject::<spellwake_application::rooms::RoomCommand>(&wire),
                _ => panic!("unexpected command fixture"),
            }
        }
    }
}

#[test]
fn legitimate_internal_tag_objects_keep_string_discriminators() {
    use spellwake_application::{ApiResponse, commands::CommandAck, inventory::AssetIdentity};
    let id = Uuid::new_v4();
    let success = serde_json::json!({"status":"success","data":{"resourceId":id}});
    let coin = serde_json::json!({"kind":"coin","currencyKey":"fixture"});
    assert!(
        spellwake_application::decode_wire_object::<ApiResponse<CommandAck>>(
            success.to_string().as_bytes()
        )
        .is_ok()
    );
    assert!(
        spellwake_application::decode_wire_object::<AssetIdentity>(coin.to_string().as_bytes())
            .is_ok()
    );
    let mut malformed = success;
    malformed["status"] = serde_json::json!({"success":null});
    assert!(
        spellwake_application::decode_wire_object::<ApiResponse<CommandAck>>(
            malformed.to_string().as_bytes()
        )
        .is_err()
    );
    let mut malformed = coin;
    malformed["kind"] = serde_json::json!({"coin":null});
    assert!(
        spellwake_application::decode_wire_object::<AssetIdentity>(
            malformed.to_string().as_bytes()
        )
        .is_err()
    );
}
