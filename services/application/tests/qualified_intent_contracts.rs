//! Common original-byte human contracts; private qualification is tested separately.
use serde::Deserialize;
use spellwake_application::{
    commands::{CommandDisposition, RevisionAck},
    qualified_intents::*,
};
use uuid::Uuid;

#[derive(Deserialize)]
struct Fixture {
    cases: Vec<Case>,
}
#[derive(Deserialize)]
struct Case {
    name: String,
    kind: String,
    wire: String,
    valid: bool,
    room: Uuid,
    original_room: RoomAdmissionIntent,
    original_between: BetweenGameIntent,
    original_reroll: QuestRerollIntent,
}
#[test]
fn common_original_human_wire_cases() {
    let f: Fixture = serde_json::from_str(include_str!(
        "../../../packages/application-contracts/fixtures/qualified-intents.json"
    ))
    .unwrap();
    assert!(!f.cases.is_empty());
    for c in f.cases {
        let bytes = c.wire.as_bytes();
        let valid = match c.kind.as_str() {
            "room_intent" => decode_intent::<RoomAdmissionIntent>(bytes).is_ok(),
            "between_intent" => decode_intent::<BetweenGameIntent>(bytes).is_ok_and(|i| i.valid()),
            "reroll_intent" => decode_intent::<QuestRerollIntent>(bytes).is_ok_and(|i| i.valid()),
            "room_ack" => decode_intent::<RevisionAck>(bytes)
                .is_ok_and(|a| valid_room_ack(&c.original_room, c.room, &a)),
            "between_ack" => decode_intent::<BetweenGameAck>(bytes)
                .is_ok_and(|a| c.original_between.valid_ack(&a)),
            "reroll_ack" => decode_intent::<QuestRerollAck>(bytes)
                .is_ok_and(|a| c.original_reroll.valid_ack(&a)),
            "ready_disposition" | "force_disposition" => decode_intent::<
                CommandDisposition<RevisionAck>,
            >(bytes)
            .is_ok_and(|d| {
                valid_room_disposition(&c.original_room, c.room, c.kind == "force_disposition", &d)
            }),
            "between_disposition" => decode_intent::<CommandDisposition<BetweenGameAck>>(bytes)
                .is_ok_and(|d| c.original_between.valid_disposition(&d)),
            "reroll_disposition" => decode_intent::<CommandDisposition<QuestRerollAck>>(bytes)
                .is_ok_and(|d| c.original_reroll.valid_disposition(&d)),
            _ => panic!("unknown case kind"),
        };
        assert_eq!(valid, c.valid, "{} ({})", c.name, c.kind);
    }
}
