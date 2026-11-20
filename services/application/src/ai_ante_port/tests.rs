use super::*;
use serde_json::json;
fn fixture() -> (AiAnteStake, AdmittedAiAnteContext) {
    let source = AiAnteStake {
        request_id: Uuid::new_v4(),
        match_id: Uuid::new_v4(),
        seat_id: Uuid::new_v4(),
        expected_terms_revision: 3.into(),
        coins: 300.into(),
    };
    let context = AdmittedAiAnteContext {
        match_id: source.match_id,
        seat_id: source.seat_id,
        sponsor_id: Uuid::new_v4(),
        match_started_at: "2019-12-31T23:59:59Z".parse().unwrap(),
        commitment_id: None,
        exposure_id: None,
    };
    (source, context)
}
#[test]
fn ai_stake_keeps_source_context_and_generated_commitment_separate_from_sponsor_money() {
    let (source, context) = fixture();
    let prepared = PreparedAiAnte::new(source.clone(), context.clone()).unwrap();
    assert_eq!(prepared.allowance_day().to_string(), "2019-12-31");
    let commitment = Uuid::new_v4();
    let response = json!({"status":"success","data":{"resourceId":commitment,"termsRevision":"4","termsHash":"a".repeat(64)}});
    assert!(
        prepared
            .decode_response(response.to_string().as_bytes())
            .is_ok()
    );
    let d = json!({"state":"accepted","response":response});
    assert!(
        prepared
            .decode_disposition(&source, &context, d.to_string().as_bytes())
            .is_ok()
    );
    for field in 0..5 {
        let mut changed = source.clone();
        match field {
            0 => changed.request_id = Uuid::new_v4(),
            1 => changed.match_id = Uuid::new_v4(),
            2 => changed.seat_id = Uuid::new_v4(),
            3 => changed.expected_terms_revision = 4.into(),
            _ => changed.coins = 100.into(),
        }
        assert!(
            prepared
                .decode_disposition(&changed, &context, d.to_string().as_bytes())
                .is_err()
        );
    }
    for field in 0..6 {
        let mut changed = context.clone();
        match field {
            0 => changed.match_id = Uuid::new_v4(),
            1 => changed.seat_id = Uuid::new_v4(),
            2 => changed.sponsor_id = Uuid::new_v4(),
            3 => changed.match_started_at = "2020-01-01T00:00:00Z".parse().unwrap(),
            4 => changed.commitment_id = Some(commitment),
            _ => changed.exposure_id = Some(Uuid::new_v4()),
        }
        assert!(
            prepared
                .decode_disposition(&source, &changed, d.to_string().as_bytes())
                .is_err()
        );
    }
    let known = AdmittedAiAnteContext {
        commitment_id: Some(Uuid::new_v4()),
        exposure_id: Some(Uuid::new_v4()),
        ..context.clone()
    };
    let known = PreparedAiAnte::new(source.clone(), known).unwrap();
    assert!(
        known
            .decode_response(response.to_string().as_bytes())
            .is_err()
    );
    let mut bad_context = context.clone();
    bad_context.exposure_id = Some(Uuid::new_v4());
    assert!(PreparedAiAnte::new(source.clone(), bad_context).is_err());
    for field in 0..3 {
        let mut changed = context.clone();
        match field {
            0 => changed.match_id = Uuid::new_v4(),
            1 => changed.seat_id = Uuid::new_v4(),
            _ => changed.sponsor_id = Uuid::nil(),
        }
        assert!(PreparedAiAnte::new(source.clone(), changed).is_err());
    }
    for coins in [0, 1, 300, 301] {
        let changed = AiAnteStake {
            coins: coins.into(),
            ..source.clone()
        };
        assert_eq!(
            PreparedAiAnte::new(changed, context.clone()).is_ok(),
            coins <= 300
        );
    }
}
#[test]
fn ai_stake_wire_rejects_human_assets_private_context_and_noncanonical_results() {
    use crate::qualified_intents::decode_intent;
    let (source, context) = fixture();
    let wire = json!({"requestId":source.request_id,"matchId":source.match_id,"seatId":source.seat_id,"expectedTermsRevision":"3","coins":"300"});
    assert!(
        decode_intent::<AiAnteStake>(wire.to_string().as_bytes())
            .unwrap()
            .valid()
    );
    for key in ["assets", "sponsorId", "day", "controllerGrantId"] {
        let mut wrong = wire.clone();
        wrong[key] = json!([]);
        assert!(decode_intent::<AiAnteStake>(wrong.to_string().as_bytes()).is_err());
    }
    for coins in [json!(300), json!(null), json!("0300"), json!("-1")] {
        let mut wrong = wire.clone();
        wrong["coins"] = coins;
        assert!(decode_intent::<AiAnteStake>(wrong.to_string().as_bytes()).is_err());
    }
    let prepared = PreparedAiAnte::new(source.clone(), context.clone()).unwrap();
    for (key, message) in OFFER_FAILURES {
        let r = json!({"status":"fail","data":{*key:*message}});
        assert!(prepared.decode_response(r.to_string().as_bytes()).is_ok());
        let d = json!({"state":"rejected","response":r});
        assert!(
            prepared
                .decode_disposition(&source, &context, d.to_string().as_bytes())
                .is_ok()
        );
    }
    for response in [
        json!({"status":"error","message":"unavailable"}),
        json!({"status":"fail","data":{"allowance":"exceeded"}}),
        json!({"status":"success","data":{"resourceId":Uuid::new_v4(),"termsRevision":"5","termsHash":"a".repeat(64)}}),
        json!(["success", {}]),
    ] {
        assert!(
            prepared
                .decode_response(response.to_string().as_bytes())
                .is_err()
        );
    }
    let duplicate = "{\"state\":\"unknown\",\"response\":null,\"response\":null}";
    assert!(
        prepared
            .decode_disposition(&source, &context, duplicate.as_bytes())
            .is_err()
    );
    for state in ["unknown", "pending"] {
        let d = json!({"state":state,"response":null});
        assert!(
            prepared
                .decode_disposition(&source, &context, d.to_string().as_bytes())
                .is_ok()
        );
    }
}
