//! Publication interleavings through actual mounted history/replay/stream routes.
use super::delivery::source_fingerprint;
use super::*;
use crate::actor::{ActorClient, ActorConfig};
use serde_json::Value;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};

fn manifest(repository: &NativeSourceRepository, game: Uuid, context: Uuid) -> Option<Uuid> {
    let bytes = repository
        .backend
        .select_bytes_bounded(
            "replay_manifest",
            &[
                ("select", "id,game_id,context_id".to_owned()),
                ("game_id", format!("eq.{game}")),
                ("limit", "2".to_owned()),
            ],
            4096,
        )
        .unwrap();
    let rows: Vec<Value> = serde_json::from_slice(&bytes).unwrap();
    if rows.is_empty() {
        return None;
    }
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["game_id"], json!(game));
    assert_eq!(rows[0]["context_id"], json!(context));
    Some(rows[0]["id"].as_str().unwrap().parse().unwrap())
}
pub(super) fn run(r: &Reservation, phase: &str) {
    let source_case = phase == "publication-source";
    let logout_case = phase == "publication-logout";
    let stream_case = phase == "publication-stream";
    let incident = phase == "publication-incident";
    assert!(
        source_case || logout_case || stream_case || incident || phase == "publication-completed"
    );
    let repository = NativeSourceRepository::from_env().unwrap();
    let base = std::env::var("SPELLWAKE_SUPABASE_URL").unwrap();
    let key = std::env::var("SPELLWAKE_SUPABASE_SECRET_KEY").unwrap();
    let public = std::env::var("SPELLWAKE_SUPABASE_PUBLISHABLE_KEY").unwrap();
    let token = std::env::var("SPELLWAKE_NATIVE_STORAGE_CALLER").unwrap();
    let commands = http::HttpProbe::owned(&base, &key, "fixture:source-reader");
    if incident {
        no_contest::terminalize(&repository, r);
    } else if !source_case {
        commands.concede(&token, r.game_id, r.context_id);
    }
    let before = source_fingerprint(&repository, r.game_id);
    assert!(manifest(&repository, r.game_id, r.context_id).is_none());
    let source = repository.read(r.game_id).unwrap().unwrap();
    assert_eq!(
        source.phases().0,
        if incident {
            crate::match_history::GamePhase::NoContest
        } else if source_case {
            crate::match_history::GamePhase::Active
        } else {
            crate::match_history::GamePhase::Completed
        }
    );
    let auth =
        ActorClient::new(ActorConfig::new(&base, &public, &format!("{base}/auth/v1")).unwrap())
            .unwrap();
    let actor = auth.resolve(&token).unwrap();
    let NativeHumanAccessOutcome::Granted(before_access) = repository
        .human_access(&actor, r.game_id, r.context_id)
        .unwrap()
    else {
        panic!("actual original participant required")
    };
    assert_eq!(before_access.replay(), (None, false));
    let callback_actor = auth.resolve(&token).unwrap();
    let callback_access = before_access.clone();
    let history = json!({"gameId":r.game_id,"contextId":r.context_id,"cursor":null,"limit":"3"});
    let publication_request = super::replay_frames::request(
        r,
        &json!("0"),
        if incident {
            r.seat_ids[0]
        } else {
            r.seat_ids[1]
        },
        if incident {
            "historical-seat"
        } else {
            "completed-full-information"
        },
        &Value::Null,
        3,
    );
    let armed = Arc::new(AtomicBool::new(false));
    let hits = Arc::new(AtomicUsize::new(0));
    let accepted = Arc::new(Mutex::new(None));
    let (enabled, count, canonical) = (armed.clone(), hits.clone(), accepted.clone());
    let (callback_base, callback_key, callback_public, callback_token) =
        (base.clone(), key.clone(), public.clone(), token.clone());
    let (game, context) = (r.game_id, r.context_id);
    let configure = move |runtime: NativeGameRuntime| {
        runtime.with_read_delivery_hook(
            if stream_case {
                TestDeliveryStage::Refresh
            } else {
                TestDeliveryStage::HistoryCompose
            },
            move |repo, recovered, _, _| {
                assert!(
                    recovered.is_some(),
                    "original first reader composed before final renewal"
                );
                if !enabled.swap(false, Ordering::SeqCst) {
                    return;
                }
                assert!(manifest(repo, game, context).is_none());
                let publisher =
                    http::HttpProbe::owned(&callback_base, &callback_key, "fixture:source-reader");
                if source_case {
                    publisher.concede(&callback_token, game, context);
                }
                let published =
                    publisher.replay(Some(&callback_token), game, &publication_request, 200);
                assert_eq!(published["data"]["schemaRevision"], "spellwake-replay-v2");
                let id = manifest(repo, game, context)
                    .expect("actual accepted publication during first reader composition");
                assert!(canonical.lock().unwrap().replace(id).is_none());
                let NativeHumanAccessOutcome::Granted(renewed) =
                    repo.human_access(&callback_actor, game, context).unwrap()
                else {
                    panic!("participant authority before optional logout")
                };
                assert_eq!(renewed.replay(), (Some(id), incident));
                assert_eq!(
                    callback_access.same_except_publication(&renewed),
                    !source_case
                );
                assert!(!callback_access.same_delivery_source(&renewed));
                count.fetch_add(1, Ordering::SeqCst);
                if logout_case {
                    super::history_delivery::logout(
                        &callback_base,
                        &callback_public,
                        &callback_token,
                    );
                }
            },
        )
    };
    let mut strict_stream = false;
    if stream_case {
        use crate::api::NativeStreamProbe;
        use axum::http::StatusCode;
        use futures::StreamExt;
        let probe =
            NativeStreamProbe::new(http::configuration(&base, &key, &base, configure, None));
        let runtime = tokio::runtime::Runtime::new().unwrap();
        runtime.block_on(async {
            let query = json!({"gameId":game,"contextId":context,"cursor":null});
            let positive = probe
                .request(super::delivery::request(&token, game, "stream", &query))
                .await;
            assert_eq!(positive.status(), StatusCode::OK);
            let mut body = positive.into_body().into_data_stream();
            let positive = super::delivery_presentation::frame(&mut body).await;
            assert_eq!(positive["status"], "success");
            drop(body);
            assert_eq!(probe.available(), (16, 16));
            armed.store(true, Ordering::SeqCst);
            let raced = probe
                .request(super::delivery::request(&token, game, "stream", &query))
                .await;
            assert_eq!(
                raced.status(),
                StatusCode::OK,
                "preflight publication exception may precede SSE headers"
            );
            let mut body = raced.into_body().into_data_stream();
            assert_eq!(
                super::delivery_presentation::frame(&mut body).await,
                json!({"status":"fail","data":{"basis":"stale"}})
            );
            assert!(body.next().await.is_none());
            drop(body);
            assert_eq!(probe.available(), (16, 16));
            let restored = probe
                .request(super::delivery::request(&token, game, "stream", &query))
                .await;
            assert_eq!(restored.status(), StatusCode::OK);
            let mut body = restored.into_body().into_data_stream();
            let restored = super::delivery_presentation::frame(&mut body).await;
            assert_eq!(restored["status"], "success");
            assert_eq!(restored["data"]["snapshot"], positive["data"]["snapshot"]);
            drop(body);
            assert_eq!(probe.available(), (16, 16));
        });
        strict_stream = true;
    } else {
        let reader = http::HttpProbe::configure(&base, &key, configure);
        let positive = reader.history(Some(&token), game, &history, 200);
        assert!(manifest(&repository, game, context).is_none());
        armed.store(true, Ordering::SeqCst);
        let status = if source_case {
            409
        } else if logout_case {
            401
        } else {
            200
        };
        let raced = reader.history(Some(&token), game, &history, status);
        if source_case {
            assert_eq!(raced, json!({"status":"fail","data":{"basis":"stale"}}));
        } else if logout_case {
            assert_eq!(
                raced,
                json!({"status":"fail","data":{"session":"unauthorized"}})
            );
        } else {
            assert_eq!(
                raced, positive,
                "publication alone preserves the exact composed history page"
            );
        }
        let fresh = if logout_case {
            super::history_delivery::fresh_owner(&base, &key, &public)
        } else {
            token.clone()
        };
        let restored = reader.history(Some(&fresh), game, &history, 200);
        if !source_case {
            assert_eq!(restored, positive);
        }
        if incident {
            let own = super::replay_frames::request(
                r,
                &json!("0"),
                r.seat_ids[0],
                "historical-seat",
                &Value::Null,
                3,
            );
            commands.replay(Some(&fresh), game, &own, 200);
            for (seat, mode) in [
                (r.seat_ids[0], "completed-full-information"),
                (r.seat_ids[1], "historical-seat"),
            ] {
                let refused =
                    super::replay_frames::request(r, &json!("0"), seat, mode, &Value::Null, 3);
                assert_eq!(
                    commands.replay(Some(&fresh), game, &refused, 409)["data"]["mode"],
                    "unavailable"
                );
            }
        }
    }
    if logout_case {
        assert!(matches!(
            repository.human_access(&actor, game, context).unwrap(),
            NativeHumanAccessOutcome::Unauthorized
        ));
    }
    assert_eq!(
        hits.load(Ordering::SeqCst),
        1,
        "one actual publication interleaving"
    );
    let accepted = accepted.lock().unwrap().unwrap();
    assert_eq!(manifest(&repository, game, context), Some(accepted));
    if source_case {
        assert_ne!(source_fingerprint(&repository, game), before);
    } else {
        assert_eq!(source_fingerprint(&repository, game), before);
    }
    println!(
        "OWNED_NATIVE_SOURCE_READER_OK {}",
        json!({"phase":phase,"actualNativeTransitionEvidence":true,"initialVersion2":true,"selectedPrintingProvenance":true,"selectedSourceActualSql":true,"immutableSourceDisplay":true,"actualConcurrentPublication":true,"sameCanonicalManifest":true,"publicationInterleavings":1,"sourceAdvanced":source_case,"sourceUnchanged":!source_case,"actorRevoked":logout_case,"accessPublicationOnly":!source_case,"operationalPrefixOnly":incident,"strictStreamTerminalStale":strict_stream,"fullyQualifiedCards":0,"semanticFactsEventsQualified":false,"deckFormatAdmissionQualified":false})
    );
}
