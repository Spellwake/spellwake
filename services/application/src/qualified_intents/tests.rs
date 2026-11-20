use super::*;
use serde_json::{Value, json};
fn ready(i: &RoomAdmissionIntent, room: Uuid, actor: Uuid) -> QualifiedReady {
    QualifiedReady {
        actor_id: actor,
        room_id: room,
        basis: i.expected_revision.clone(),
        ready: true,
        selected_revision_id: Some(Uuid::new_v4()),
        qualification_ref: Some("  fixture:original-qualified  ".parse().unwrap()),
        request_id: i.request_id,
    }
}
#[test]
fn retained_room_source_binds_original_lookup_actor_action_and_full_intent() {
    let i = RoomAdmissionIntent {
        request_id: Uuid::new_v4(),
        expected_revision: 3.into(),
    };
    let room = Uuid::new_v4();
    let actor = Uuid::new_v4();
    let p = PreparedRoomAdmission::ready(&i, room, actor, ready(&i, room, actor)).unwrap();
    for field in 0..4 {
        let mut source = ready(&i, room, actor);
        match field {
            0 => source.actor_id = Uuid::new_v4(),
            1 => source.room_id = Uuid::new_v4(),
            2 => source.request_id = Uuid::new_v4(),
            _ => source.basis = 99.into(),
        }
        assert!(PreparedRoomAdmission::ready(&i, room, actor, source).is_err());
    }
    assert_eq!(
        p.ready_source()
            .unwrap()
            .qualification_ref
            .as_ref()
            .unwrap()
            .as_str(),
        "  fixture:original-qualified  "
    );
    let response = json!({"status":"success","data":{"resourceId":room,"revision":"4"}});
    assert!(p.decode_response(response.to_string().as_bytes()).is_ok());
    let d = json!({"state":"accepted","response":response});
    assert!(
        p.decode_disposition(&i, room, actor, false, d.to_string().as_bytes())
            .is_ok()
    );
    assert!(
        p.decode_disposition(&i, room, actor, true, d.to_string().as_bytes())
            .is_err()
    );
    assert!(
        p.decode_disposition(&i, room, Uuid::new_v4(), false, d.to_string().as_bytes())
            .is_err()
    );
    assert!(
        p.decode_disposition(&i, Uuid::new_v4(), actor, false, d.to_string().as_bytes())
            .is_err()
    );
    let changed = RoomAdmissionIntent {
        request_id: i.request_id,
        expected_revision: 99.into(),
    };
    assert!(
        p.decode_disposition(&changed, room, actor, false, d.to_string().as_bytes())
            .is_err()
    );
    let changed = RoomAdmissionIntent {
        request_id: Uuid::new_v4(),
        expected_revision: 3.into(),
    };
    assert!(!p.matches_request(&changed, room, actor, false));
    let mut source = ready(&i, room, actor);
    source.ready = false;
    assert!(PreparedRoomAdmission::ready(&i, room, actor, source).is_err());
    let source = QualifiedCountdown {
        actor_id: actor,
        room_id: room,
        basis: 3.into(),
        qualification_ref: "fixture:countdown".parse().unwrap(),
        request_id: i.request_id,
    };
    let p = PreparedRoomAdmission::force_start(&i, room, actor, source).unwrap();
    assert!(
        p.decode_disposition(&i, room, actor, true, d.to_string().as_bytes())
            .is_ok()
    );
    assert!(
        p.decode_disposition(&i, room, actor, false, d.to_string().as_bytes())
            .is_err()
    );
}
fn between(i: &BetweenGameIntent) -> BetweenGameReply {
    BetweenGameReply {
        decision_id: i.decision_id,
        basis: i.expected_revision.clone(),
        selected_revision_id: i.selected_revision_id,
        reply_ref: " fixture:original-reply ".parse().unwrap(),
        qualification_ref: "fixture:pool-original".parse().unwrap(),
        request_id: i.request_id,
    }
}
#[test]
fn original_between_selection_retains_owner_and_strips_private_sentinel_and_proof() {
    let i = BetweenGameIntent {
        request_id: Uuid::new_v4(),
        decision_id: Uuid::new_v4(),
        expected_revision: 7.into(),
        selected_revision_id: Uuid::new_v4(),
    };
    let actor = Uuid::new_v4();
    let owner = BetweenDecisionOwner {
        decision_id: i.decision_id,
        account_id: actor,
    };
    let p = PreparedBetweenGame::new(&i, actor, &owner, between(&i)).unwrap();
    for field in 0..4 {
        let mut source = between(&i);
        match field {
            0 => source.decision_id = Uuid::new_v4(),
            1 => source.request_id = Uuid::new_v4(),
            2 => source.basis = 99.into(),
            _ => source.selected_revision_id = Uuid::new_v4(),
        }
        assert!(PreparedBetweenGame::new(&i, actor, &owner, source).is_err());
    }
    assert_eq!(p.source().reply_ref.as_str(), " fixture:original-reply ");
    let private = json!({"status":"success","data":{"resourceId":i.decision_id,"revision":"8","reservedGameId":"","qualificationRef":"fixture:pool-original"}});
    let public = p.decode_response(private.to_string().as_bytes()).unwrap();
    let value = serde_json::to_value(public).unwrap();
    assert_eq!(
        value,
        json!({"status":"success","data":{"resourceId":i.decision_id,"revision":"8","reservedGameId":null}})
    );
    let d = json!({"state":"accepted","response":value});
    assert!(
        p.decode_disposition(&i, actor, d.to_string().as_bytes())
            .is_ok()
    );
    let mut changed = i.clone();
    changed.selected_revision_id = Uuid::new_v4();
    assert!(
        p.decode_disposition(&changed, actor, d.to_string().as_bytes())
            .is_err()
    );
    assert!(
        p.decode_disposition(&i, Uuid::new_v4(), d.to_string().as_bytes())
            .is_err()
    );
    assert!(PreparedBetweenGame::new(&i, Uuid::new_v4(), &owner, between(&i)).is_err());
    let mut wrong = private;
    wrong["data"]["qualificationRef"] = json!("fixture:changed-pool");
    assert!(p.decode_response(wrong.to_string().as_bytes()).is_err());
    let mut leaked = d;
    leaked["response"]["data"]["qualificationRef"] = json!("fixture:pool-original");
    assert!(
        p.decode_disposition(&i, actor, leaked.to_string().as_bytes())
            .is_err()
    );
}
#[test]
fn public_original_body_bound_is_checked_by_the_actual_decoder() {
    let raw = json!({"requestId":Uuid::new_v4(),"assignmentId":Uuid::new_v4()}).to_string();
    let mut bytes = raw.into_bytes();
    bytes.resize(MAX_QUALIFIED_INTENT_BYTES, b' ');
    assert!(decode_intent::<QuestRerollIntent>(&bytes).is_ok());
    bytes.push(b' ');
    assert!(decode_intent::<QuestRerollIntent>(&bytes).is_err());
    let public: Value = json!({"requestId":Uuid::new_v4(),"assignmentId":Uuid::new_v4(),"definitionId":Uuid::new_v4()});
    assert!(decode_intent::<QuestRerollIntent>(public.to_string().as_bytes()).is_err());
}
