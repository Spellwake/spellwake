//! Genuine bearer expiry while a completed delivery worker result stays unpolled.
use super::delivery::{Calls, json_response, request, source_fingerprint};
use super::delivery_presentation::frame;
use super::*;
use crate::api::NativeStreamProbe;
use axum::http::StatusCode;
use std::{
    future::Future,
    sync::Arc,
    task::Poll,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
}
pub(super) fn run(
    r: &Reservation,
    store: &NativeArtifactStore,
    programs: &NativeProgramSource,
    phase: &str,
) {
    let archive = phase.starts_with("archive-handoff-");
    let account_none = phase.starts_with("handoff-account-");
    let operation = if account_none {
        if phase.ends_with("-history") {
            "history"
        } else if phase.ends_with("-replay") {
            "replay"
        } else {
            panic!("unknown account handoff")
        }
    } else if archive {
        history_delivery::archive_operation(phase)
    } else {
        match phase {
            "handoff-view" => "view",
            "handoff-reconnect" => "reconnect",
            "handoff-stream" => "stream",
            "handoff-history" => "history",
            "handoff-replay" => "replay",
            _ => panic!("unknown delivery handoff case"),
        }
    };
    let repository = NativeSourceRepository::from_env().unwrap();
    let recovered = repository
        .recover_with_history(
            r.game_id,
            store,
            programs,
            NativeRecoveryBudget::new(100, 4_000_000).unwrap(),
        )
        .unwrap()
        .unwrap();
    assert_eq!(
        recovered.history_evidence(),
        NativeHistoryEvidence::TransitionV1
    );
    super::selected_origin(
        recovered.checkpoint().provenance(),
        recovered.checkpoint().state(),
        r,
    );
    let before = source_fingerprint(&repository, r.game_id);
    let base = std::env::var("SPELLWAKE_SUPABASE_URL").unwrap();
    let key = std::env::var("SPELLWAKE_SUPABASE_SECRET_KEY").unwrap();
    let mut bearer = std::env::var("SPELLWAKE_NATIVE_STORAGE_CALLER").unwrap();
    let mut expires: u64 = std::env::var("SPELLWAKE_NATIVE_EXPIRY")
        .unwrap()
        .parse()
        .unwrap();
    let calls = Arc::new(Calls::default());
    let proxy = fault_proxy::Proxy::new(&base, None, calls.clone());
    let composed = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let observed = composed.clone();
    let (origin_url, secret) = (proxy.url().to_owned(), key.clone());
    let probe = NativeStreamProbe::new(http::configuration(
        proxy.url(),
        &key,
        &base,
        move |runtime| {
            let runtime = history_delivery::runtime(runtime, operation, &origin_url, &secret);
            if archive || account_none {
                runtime.with_read_delivery_hook(
                    history_delivery::stage(operation),
                    move |_, recovered, _, _| {
                        assert_eq!(recovered.is_none(), archive);
                        observed.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    },
                )
            } else {
                runtime
            }
        },
        None,
    ));
    let endpoint = if archive { "replay" } else { operation };
    let payload = if archive {
        history_delivery::query(r, operation)
    } else if operation == "view" {
        json!({"gameId":r.game_id,"contextId":r.context_id})
    } else if operation == "history" {
        json!({"gameId":r.game_id,"contextId":r.context_id,"cursor":null,"limit":"3"})
    } else if operation == "replay" {
        json!({"gameId":r.game_id,"contextId":r.context_id,"atEventSequence":"0",
            "perspectiveSeatId":r.seat_ids[0],"mode":"historical-seat",
            "history":{"gameId":r.game_id,"contextId":r.context_id,"cursor":null,"limit":"3"}})
    } else if operation == "stream" {
        json!({"gameId":r.game_id,"contextId":r.context_id,"cursor":null})
    } else {
        json!({"receipts":{"gameId":r.game_id,"contextId":r.context_id,"outstanding":[]},"cursor":null})
    };
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let json_history = archive || matches!(operation, "history" | "replay");
    let public = std::env::var("SPELLWAKE_SUPABASE_PUBLISHABLE_KEY").unwrap();
    if archive || account_none {
        bearer = history_delivery::fresh_owner(&base, &key, &public);
    }
    let mut parked_bearer_replies = 0;
    let mut warm_json = None;
    runtime.block_on(async {
        let positive = probe
            .request(request(&bearer, r.game_id, endpoint, &payload))
            .await;
        assert_eq!(positive.status(), StatusCode::OK);
        if operation == "stream" {
            let mut body = positive.into_body().into_data_stream();
            assert_eq!(frame(&mut body).await["status"], "success");
            drop(body);
        } else {
            let body = json_response(positive).await;
            assert_eq!(body["status"], "success");
            warm_json = Some(body);
        }
        assert_eq!(probe.available(), (16, 16));
    });
    if account_none {
        let (token, actor) = read_account::unregistered(&base, &key, &public);
        bearer = token;
        expires = actor.test_verified_bearer_expiry();
        assert!(expires <= now() + 31 && expires > now() + 7);
    } else if archive {
        (bearer, expires) = history_delivery::fresh_owner_deadline(&base, &key, &public);
    }
    let actual = crate::actor::ActorClient::new(
        crate::actor::ActorConfig::new(&base, &public, &format!("{base}/auth/v1")).unwrap(),
    )
    .unwrap()
    .resolve(&bearer)
    .unwrap();
    runtime.block_on(async {
        assert!(now() < expires);
        let initial_calls = calls.count();
        let initial_initiated = calls.initiated();
        let initial_auth = calls.auth_count();
        let initial_successful_auth = calls.successful_auth().len();
        if json_history {
            assert_eq!(initial_auth, 2);
            assert_eq!(initial_successful_auth, 2);
        }
        let mut parked = Box::pin(probe.request(request(&bearer, r.game_id, endpoint, &payload)));
        // Poll the REAL router future into spawn_blocking, then deliberately stop
        // polling it. No fake actor/response or test publication constructor exists.
        std::future::poll_fn(|cx| {
            assert!(
                parked.as_mut().poll(cx).is_pending(),
                "real worker must be pending on first poll"
            );
            Poll::Ready(())
        })
        .await;
        tokio::time::timeout(Duration::from_secs(15), async {
            loop {
                if calls.count() > initial_calls
                    && calls.last_rpc().as_deref()
                        == Some(if json_history {
                            "/rest/v1/rpc/read_current_session"
                        } else {
                            "/rest/v1/rpc/read_native_human_access"
                        })
                    && (!json_history
                        || if account_none {
                            matches!(calls.auth_count() - initial_auth, 1 | 2)
                        } else {
                            calls.auth_count() == initial_auth + 2
                        })
                    && (!json_history
                        || calls.successful_auth().len() - initial_successful_auth
                            == calls.auth_count() - initial_auth)
                    && probe.available() == (if operation == "stream" { 15 } else { 16 }, 16)
                {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        // Last real source renewal returned and the actual worker released its
        // permit while the original signed credential was still valid. Its result
        // is now retained by the unpolled router/JoinHandle, not by the proxy.
        assert!(
            now() < expires,
            "worker must finish before genuine credential expiry"
        );
        if json_history {
            parked_bearer_replies = calls.auth_count() - initial_auth;
            if account_none {
                assert!(matches!(parked_bearer_replies, 1 | 2));
                read_account::replies(
                    &calls.successful_auth()[initial_successful_auth..],
                    &actual,
                    parked_bearer_replies,
                );
            } else {
                assert_eq!(
                    parked_bearer_replies, 2,
                    "this parked request's initial and final genuine session replies"
                );
                let successful = calls.successful_auth();
                let subject = actual.subject();
                let session = actual.session();
                for reply in &successful[initial_successful_auth..] {
                    assert_eq!(reply["data"]["subjectId"], json!(subject));
                    assert_eq!(reply["data"]["sessionId"], json!(session));
                    assert_eq!(reply["data"]["state"], "active");
                    assert_eq!(
                        reply["data"]["accountId"],
                        successful[initial_successful_auth - 1]["data"]["accountId"]
                    );
                    assert_eq!(
                        reply["data"]["method"],
                        successful[initial_successful_auth - 1]["data"]["method"]
                    );
                }
            }
        }
        let completed_calls = calls.count();
        let completed_initiated = calls.initiated();
        assert!(completed_initiated > initial_initiated);
        if account_none {
            assert_eq!(
                completed_initiated - initial_initiated,
                parked_bearer_replies,
                "no native/Storage work for unregistered caller"
            );
            assert_eq!(composed.load(std::sync::atomic::Ordering::SeqCst), 1);
        }
        if archive {
            assert_eq!(composed.load(std::sync::atomic::Ordering::SeqCst), 2);
        }
        tokio::time::sleep(Duration::from_secs((expires + 1).saturating_sub(now()))).await;
        assert!(now() >= expires);
        assert_eq!(calls.count(), completed_calls);
        assert_eq!(calls.initiated(), completed_initiated);
        let response = parked.await;
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(
            json_response(response).await,
            json!({"status":"fail","data":{"session":"unauthorized"}})
        );
        assert_eq!(probe.available(), (16, 16));
        assert_eq!(
            calls.count(),
            completed_calls,
            "resuming and consuming the parked response performs no further IO"
        );
        assert_eq!(calls.initiated(), completed_initiated);
        if archive {
            assert_eq!(composed.load(std::sync::atomic::Ordering::SeqCst), 2);
        }
        if json_history {
            assert_eq!(calls.auth_count(), initial_auth + 2);
            assert_eq!(calls.successful_auth().len(), initial_successful_auth + 2);
        }
    });
    if account_none {
        assert_eq!(
            parked_bearer_replies, 2,
            "corrected passes require actual initial and final bearer replies"
        );
        assert_eq!(composed.load(std::sync::atomic::Ordering::SeqCst), 1);
        let owner = history_delivery::fresh_owner(&base, &key, &public);
        runtime.block_on(async {
            let response = probe
                .request(request(&owner, r.game_id, endpoint, &payload))
                .await;
            assert_eq!(response.status(), StatusCode::OK);
            assert_eq!(json_response(response).await, warm_json.unwrap());
        });
    }
    assert_eq!(source_fingerprint(&repository, r.game_id), before);
    println!(
        "OWNED_NATIVE_SOURCE_READER_OK {}",
        json!({"phase":phase,"actualNativeTransitionEvidence":true,"initialVersion2":true,"selectedPrintingProvenance":true,"selectedSourceActualSql":true,"immutableSourceDisplay":true,"positiveDelivery":true,"actualUnpolledWorkerComplete":true,"parkedFinalBearerReplies":parked_bearer_replies,"actualFinalBearerRepliesAccepted":json_history,"archiveReaderNoRecovery":archive,"expiryCredentialAfterPreparation":archive || account_none,"noResumeInitiatedIo":true,"actualAccountNone":account_none,"unregisteredSubject":if account_none {json!(actual.subject())} else {serde_json::Value::Null},"noNativeIo":account_none,"restoredExactPositive":account_none,"noResumeIo":true,"actualOriginalExpiryCrossed":true,"expectedUnauthorized":true,"sourceUnchanged":true,"fullyQualifiedCards":0,"semanticFactsEventsQualified":false,"deckFormatAdmissionQualified":false})
    );
}
