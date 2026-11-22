//! Owned Auth/Storage/RPC native casts into canonical facts and private SQL quests.
//! Release/profile and prior quest progress are declared fixture setup, not admission.
use super::*;
use crate::controller_possession::ControllerSecretStore;
use crate::native_game::preparation_source::NativePreparationRepository;
use crate::native_game::source_reader::command::{NativePlayerAuthority, NativePlayerCommit};
use crate::player_wire::{
    ManaColor, ManaSpend, PlayerAction, PlayerChoice, PlayerDomain, PlayerObjectIdentity,
    PlayerPayment, PlayerSnapshot, PlayerZoneKind,
};
use serde_json::Value;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};
const FOREST: &str = "b34bb2dc-c1af-4d77-b0b3-a0fb342a5fc6";
const BEARS: &str = "14c8f55d-d177-4c25-a931-ebeb9e6062a0";
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Case {
    game_id: Uuid,
    context_id: Uuid,
    mixed_ai: bool,
    first_player: usize,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Input {
    cases: Vec<Case>,
    owner_account_id: Uuid,
    quest_assignment_id: Uuid,
}
struct LostAck {
    armed: AtomicBool,
    lost: AtomicBool,
    submits: AtomicUsize,
    submit_attempts: AtomicUsize,
    forbidden: AtomicUsize,
    saved: Mutex<Option<Value>>,
}
impl fault_proxy::Fault for LostAck {
    fn request(&self, _: &reqwest::Method, path: &str, _: &mut Vec<u8>) -> Option<u16> {
        if matches!(
            path,
            "/rest/v1/rpc/submit_native_human_command"
                | "/rest/v1/rpc/submit_native_controller_command"
        ) {
            self.submit_attempts.fetch_add(1, Ordering::SeqCst);
        }
        if self.lost.load(Ordering::SeqCst) && path != "/rest/v1/rpc/read_native_human_receipts" {
            self.forbidden.fetch_add(1, Ordering::SeqCst);
            return Some(503);
        }
        if self.armed.load(Ordering::SeqCst) && path == "/rest/v1/rpc/submit_native_human_command" {
            self.submits.fetch_add(1, Ordering::SeqCst);
        }
        None
    }
    fn response(&self, _: &reqwest::Method, path: &str, status: u16, body: &mut Vec<u8>) {
        if self.armed.load(Ordering::SeqCst) && path == "/rest/v1/rpc/submit_native_human_command" {
            assert_eq!(status, 200);
            let value: Value = serde_json::from_slice(body).unwrap();
            assert_eq!(value["status"], "success");
            *self.saved.lock().unwrap() = Some(value);
        }
    }
    fn delivery(
        &self,
        _: &reqwest::Method,
        path: &str,
        _: u16,
        _: &[u8],
    ) -> fault_proxy::ResponseDelivery {
        if self.armed.load(Ordering::SeqCst)
            && path == "/rest/v1/rpc/submit_native_human_command"
            && !self.lost.swap(true, Ordering::SeqCst)
        {
            fault_proxy::ResponseDelivery::Truncate
        } else {
            fault_proxy::ResponseDelivery::Forward
        }
    }
}
fn named<'a>(
    snapshot: &'a PlayerSnapshot,
    name: &str,
) -> impl Iterator<Item = &'a crate::player_wire::PlayerObject> {
    snapshot.visible_objects.iter().filter(move |o|matches!(&o.identity,PlayerObjectIdentity::Known{characteristics,..}if characteristics.name==name))
}
fn choose(
    snapshot: &PlayerSnapshot,
    recovered: &RecoveredNativeGame,
    issued: &IssuedNativePlayerView<'_>,
) -> PlayerAction {
    let checkpoint = recovered.checkpoint();
    let scope = issued.view().scope();
    if checkpoint.state().pending_decision().is_some() {
        let decision = snapshot.decisions.first().unwrap();
        let response = match &decision.domain {
            PlayerDomain::Options { options, .. } => PlayerChoice::Option {
                option: options
                    .iter()
                    .find(|o| o.label == "Keep")
                    .unwrap()
                    .handle
                    .clone(),
            },
            PlayerDomain::Attackers { required, .. } => {
                assert!(required.is_empty());
                PlayerChoice::Attackers {
                    assignments: vec![],
                }
            }
            PlayerDomain::Blockers { required, .. } => {
                assert!(required.is_empty());
                PlayerChoice::Blockers {
                    assignments: vec![],
                }
            }
            _ => panic!("unexpected controlled native decision"),
        };
        return PlayerAction::Choose {
            decision: decision.decision.clone(),
            response,
        };
    }
    let turn = checkpoint.state().turn().unwrap();
    let native = checkpoint.state().execution().window().active_player();
    let active = checkpoint
        .roster
        .iter()
        .find(|b| b.native_seat == native)
        .unwrap()
        .seat_id;
    if active != snapshot.viewer_seat_id
        || turn.position().step != spellwake_engine::turn::Step::PrecombatMain
    {
        return PlayerAction::Pass {};
    };
    let hand = snapshot
        .zones
        .iter()
        .find(|z| {
            z.zone_type() == PlayerZoneKind::Hand
                && z.owner_seat_id() == Some(snapshot.viewer_seat_id)
        })
        .unwrap();
    if turn.lands_played() == &0.into()
        && let Some(land) = named(snapshot, "Forest").find(|o| hand.objects().contains(&o.handle))
    {
        return PlayerAction::PlayLand {
            source: land.handle.clone(),
        };
    }
    let battlefield = snapshot
        .zones
        .iter()
        .find(|z| z.zone_type() == PlayerZoneKind::Battlefield)
        .unwrap();
    let lands: Vec<_> = named(snapshot, "Forest")
        .filter(|o| {
            battlefield.objects().contains(&o.handle)
                && o.controller_seat_id == Some(snapshot.viewer_seat_id)
        })
        .collect();
    if lands.len() >= 2
        && let Some(bear) =
            named(snapshot, "Grizzly Bears").find(|o| hand.objects().contains(&o.handle))
    {
        let action = PlayerAction::CastUntargeted {
            source: bear.handle.clone(),
            payment: Some(PlayerPayment {
                from_lots: Default::default(),
                exile_cast_type: None,
                alternative_cost_source: None,
                graveyard_permission: None,
                use_granted_flashback: false,
                restricted_mana: Vec::new(),
                spend: vec![ManaSpend {
                    color: ManaColor::Green,
                    amount: "2".to_owned().try_into().unwrap(),
                }],
            }),
        };
        let command = crate::player_wire::PlayerCommand {
            request_id: Uuid::new_v4(),
            basis: snapshot.basis.clone(),
            action: action.clone(),
        };
        if scope.propose(&command).is_ok() {
            return action;
        }
        if let Some(land) = lands.into_iter().find(|o| !o.tapped) {
            return PlayerAction::ActivateMana {
                source: land.handle.clone(),
                ability: land
                    .abilities
                    .iter()
                    .find(|a| a.kind == crate::player_wire::PlayerAbilityKind::Mana)
                    .unwrap()
                    .handle
                    .clone(),
            };
        }
    }
    PlayerAction::Pass {}
}
fn consume(database: &str, game: Uuid) -> Value {
    lifecycle_reads::sql(
        database,
        "SELECT app.consume_quest_game_activity($1,1000)",
        &[&game],
    )
}
fn progress(database: &str, quest: Uuid) -> Value {
    lifecycle_reads::sql(
        database,
        "SELECT jsonb_build_object('progress',progress::text,'completed',completed_at IS NOT NULL,'operation',reward_operation_id,'fact',reward_source_fact_id) FROM app.quest_assignment WHERE id=$1",
        &[&quest],
    )
}
fn consume_with_rollback_and_competitor(database: &str, game: Uuid) -> Value {
    tokio::runtime::Runtime::new().unwrap().block_on(async{
        let (writer,connection)=tokio_postgres::connect(database,tokio_postgres::NoTls).await.unwrap();let writer_task=tokio::spawn(connection);
        writer.batch_execute("BEGIN").await.unwrap();let rolled:Value=writer.query_one("SELECT app.consume_quest_game_activity($1,1000)",&[&game]).await.unwrap().get(0);assert_eq!(rolled["data"]["rewards"],"1");writer.batch_execute("ROLLBACK").await.unwrap();
        let (reader,connection)=tokio_postgres::connect(database,tokio_postgres::NoTls).await.unwrap();let reader_task=tokio::spawn(connection);let pid:i32=reader.query_one("SELECT pg_backend_pid()",&[]).await.unwrap().get(0);
        writer.batch_execute("BEGIN").await.unwrap();let first:Value=writer.query_one("SELECT app.consume_quest_game_activity($1,1000)",&[&game]).await.unwrap().get(0);assert_eq!(first["data"]["rewards"],"1");
        let waiting=tokio::spawn(async move{let result:Value=reader.query_one("SELECT app.consume_quest_game_activity($1,1000)",&[&game]).await.unwrap().get(0);drop(reader);result});
        let deadline=std::time::Instant::now()+Duration::from_secs(15);loop{let waiting:bool=writer.query_one("SELECT EXISTS(SELECT 1 FROM pg_stat_activity WHERE pid=$1 AND wait_event_type='Lock')",&[&pid]).await.unwrap().get(0);if waiting{break;}assert!(std::time::Instant::now()<deadline);tokio::time::sleep(Duration::from_millis(20)).await;}
        writer.batch_execute("COMMIT").await.unwrap();let second=waiting.await.unwrap();assert_eq!(second["data"]["rewards"],"0");drop(writer);writer_task.await.unwrap().unwrap();reader_task.await.unwrap().unwrap();json!({"rolledBackReward":"1","committedReward":"1","competingReward":"0","actualLockWait":true})
    })
}
#[test]
#[ignore = "requires owned real Auth/SQL/Storage and genuine source inputs; finite cast-to-quest qualification"]
fn owned_native_cast_activity_to_quest_consumption() {
    assert_eq!(
        std::env::var("SPELLWAKE_NATIVE_STORAGE_QUALIFY").unwrap(),
        "1"
    );
    let input: Input = crate::decode_wire_object(
        &fs::read(std::env::var_os("SPELLWAKE_NATIVE_ACTIVITY_INPUT").unwrap()).unwrap(),
    )
    .unwrap();
    assert_eq!(input.cases.len(), 2);
    let read = |name| fs::read(std::env::var_os(name).unwrap()).unwrap();
    let programs = NativeProgramSource::from_bytes(
        NativeProgramPool::ResolutionSelectionNaturalMana,
        &read("SPELLWAKE_CATALOGUE_METADATA"),
        read("SPELLWAKE_CATALOGUE_INPUT"),
        &read("SPELLWAKE_CREATURE_VOCABULARY_INPUT"),
    )
    .unwrap();
    let store = NativeArtifactStore::from_env().unwrap();
    let repository = NativeSourceRepository::from_env().unwrap();
    let base = std::env::var("SPELLWAKE_SUPABASE_URL").unwrap();
    let key = std::env::var("SPELLWAKE_SUPABASE_SECRET_KEY").unwrap();
    let database = std::env::var("SPELLWAKE_NATIVE_FIXTURE_DATABASE_URL").unwrap();
    let runtime = NativeGameRuntime::from_env(&base, &key).unwrap().unwrap();
    let owner = NativeHostOwner::new("fixture:source-reader").unwrap();
    let auth = crate::actor::ActorClient::new(
        crate::actor::ActorConfig::new(
            &base,
            &std::env::var("SPELLWAKE_SUPABASE_PUBLISHABLE_KEY").unwrap(),
            &format!("{}/auth/v1", base.trim_end_matches('/')),
        )
        .unwrap(),
    )
    .unwrap();
    let tokens = [
        std::env::var("SPELLWAKE_NATIVE_STORAGE_CALLER").unwrap(),
        std::env::var("SPELLWAKE_NATIVE_SOURCE_PEER_BEARER").unwrap(),
    ];
    let actors = [
        auth.resolve(&tokens[0]).unwrap(),
        auth.resolve(&tokens[1]).unwrap(),
    ];
    let mut cases = Vec::new();
    let mut tampered = 0;
    for case in input.cases {
        let reserved = NativePreparationRepository::from_env()
            .unwrap()
            .reserved_game(case.game_id)
            .unwrap()
            .unwrap();
        let r = reserved.metadata();
        assert!(case.first_player < r.seats.len());
        let selection = r
            .seats
            .iter()
            .map(|seat| {
                assert_eq!(seat.entries.len(), 2);
                let forest = seat
                    .entries
                    .iter()
                    .find(|e| e.oracle_ref.as_str() == FOREST)
                    .unwrap();
                let bears = seat
                    .entries
                    .iter()
                    .find(|e| e.oracle_ref.as_str() == BEARS)
                    .unwrap();
                assert_eq!(forest.quantity, Amount::from(20));
                assert_eq!(bears.quantity, Amount::from(20));
                assert!(!bears.unlimited_basic_land);
                NativeInitialSelection {
                    seat_id: seat.seat_id,
                    entry_ids: (0..40)
                        .map(|i| {
                            if i % 2 == 0 {
                                forest.entry_id
                            } else {
                                bears.entry_id
                            }
                        })
                        .collect(),
                }
            })
            .collect();
        let initial = prepare_selected_initial(
            case.context_id,
            &programs,
            &reserved,
            NativeInitialSpec {
                first_player_seat_id: r.seats[case.first_player].seat_id,
                participants: r
                    .seats
                    .iter()
                    .map(|seat| NativeInitialParticipant {
                        seat_id: seat.seat_id,
                        life: 20.into(),
                        cards: (0..40)
                            .map(|i| NativeInitialCard {
                                oracle_id: spellwake_card_programs::OracleId::try_from(
                                    if i % 2 == 0 { FOREST } else { BEARS }.to_owned(),
                                )
                                .unwrap(),
                                zone: NativeInitialZone::Library,
                            })
                            .collect(),
                    })
                    .collect(),
                entropy: vec![0.into(); 2048],
            },
            selection,
        )
        .unwrap();
        assert_eq!(
            store
                .retain(
                    case.game_id,
                    case.context_id,
                    NativeArtifactRole::Initial,
                    initial.original_bytes()
                )
                .unwrap()
                .as_str(),
            initial.reference().as_str()
        );
        let first = NativeCheckpoint::initial(initial).unwrap();
        first.retain(&store).unwrap();
        let facts = first.prepare_results().unwrap().retain(&store).unwrap();
        let event = first.prepare_event().unwrap().retain(&store).unwrap();
        let command=crate::admission_ports::AcceptReservedGame{match_id:r.match_id,expected_revision:r.match_revision.clone(),game_id:r.game_id,owner:"fixture:source-reader".parse().unwrap(),opening:crate::admission_ports::OpeningArtifacts::from_bytes(&serde_json::to_vec(&json!({"eventSchemaRevision":"native-transition-v1","contextId":case.context_id,"snapshotRef":first.initial_reference(),"contextRevision":"fixture:native-cast-activity-v1","stateRef":first.reference(),"factsRef":facts,"eventRef":event,"qualificationRef":"fixture:partial-programs-not-admission","clockObservation":first.opening_clock_observation().unwrap()})).unwrap()).unwrap(),original_accepted_basis:Some(r.match_revision.clone())};
        let original = NativeSystemOriginal::prepare_opening(&command, &first, &programs).unwrap();
        let _ack = repository
            .accept_opening(&store, &reserved, &command, &first, &programs, &original)
            .unwrap();
        let secrets = ControllerSecretStore::default();
        let controller = if case.mixed_ai {
            let meta = lifecycle_reads::sql(
                &database,
                "SELECT jsonb_build_object('grant',g.id,'binding',b.id,'epoch',b.binding_epoch::text,'reference',g.grant_secret_ref) FROM app.controller_grant g JOIN app.ai_runner_binding b ON b.id=g.binding_id WHERE b.match_seat_id=$1 AND g.state='active' AND g.binding_epoch=b.binding_epoch",
                &[&r.seats[1].seat_id],
            );
            Some(super::controller::install(&secrets, &meta))
        } else {
            None
        };
        let faults = Arc::new(LostAck {
            armed: AtomicBool::new(false),
            lost: AtomicBool::new(false),
            submits: AtomicUsize::new(0),
            submit_attempts: AtomicUsize::new(0),
            forbidden: AtomicUsize::new(0),
            saved: Mutex::new(None),
        });
        let proxy = fault_proxy::Proxy::new(&base, None, faults.clone());
        let proxied = NativeSourceRepository::new(
            BackendClient::new(
                BackendConfig::new(proxy.url(), &key).unwrap(),
                "native-cast-connected",
                Duration::from_secs(60),
            )
            .unwrap(),
        );
        let mut human_casts = 0;
        let mut ai_casts = 0;
        let mut turns = 0;
        for _ in 0..300 {
            let recovered = repository
                .recover_with_history(
                    case.game_id,
                    &store,
                    &programs,
                    NativeRecoveryBudget::new(10000, 100_000_000).unwrap(),
                )
                .unwrap()
                .unwrap();
            let checkpoint = recovered.checkpoint();
            let native = if let Some(decision) = checkpoint.state().pending_decision() {
                decision.chooser()
            } else {
                let spellwake_engine::stack_window::WindowPhase::Priority(
                    spellwake_engine::priority::PriorityPhase::Ready { holder },
                ) = checkpoint.state().execution().window().phase()
                else {
                    panic!("controlled actual priority required")
                };
                holder
            };
            let index = usize::try_from(native.0).unwrap();
            let is_ai = case.mixed_ai && index == 1;
            let human_access = if is_ai {
                None
            } else {
                let NativeHumanAccessOutcome::Granted(access) = repository
                    .human_access(&actors[index], case.game_id, case.context_id)
                    .unwrap()
                else {
                    panic!("actual human access")
                };
                Some(access)
            };
            let controller_access = if is_ai {
                let NativeControllerAccessOutcome::Granted(access) = repository
                    .controller_access(
                        &secrets,
                        &controller.as_ref().unwrap().1,
                        case.game_id,
                        case.context_id,
                    )
                    .unwrap()
                else {
                    panic!("actual possessed controller access")
                };
                Some(access)
            } else {
                None
            };
            let authority = if let Some(access) = human_access.as_ref() {
                NativePlayerAuthority::Human(&actors[index], access)
            } else {
                NativePlayerAuthority::Controller(
                    &secrets,
                    &controller.as_ref().unwrap().1,
                    controller_access.as_ref().unwrap(),
                )
            };
            let display =
                NativeImmutableDisplay::new(&programs, checkpoint.provenance().unwrap()).unwrap();
            let metadata = NativeProjectionMetadata {
                through_event_sequence: recovered.source().last_event_sequence().clone(),
                deadline_at: None,
                disclosure: NativeDisclosure::LiveParticipant,
            };
            let issued = authority
                .issue(&repository, &store, &recovered, &metadata, &display)
                .unwrap();
            let snapshot = issued.view().snapshot();
            let action = choose(snapshot, &recovered, &issued);
            let casting = matches!(action, PlayerAction::CastUntargeted { .. });
            let original = crate::player_wire::PlayerCommand {
                request_id: Uuid::new_v4(),
                basis: checkpoint.basis().clone(),
                action,
            };
            if casting {
                let proposal = issued.view().scope().propose(&original).unwrap();
                let bytes = issued.view().scope().encode_input(&proposal).unwrap();
                let reference = store
                    .retain(
                        case.game_id,
                        case.context_id,
                        NativeArtifactRole::Input,
                        &bytes,
                    )
                    .unwrap();
                let next = checkpoint
                    .append(
                        &store,
                        reference,
                        NativeCheckpointInput::Player,
                        authority.viewer(),
                    )
                    .unwrap();
                next.retain(&store).unwrap();
                next.prepare_activity_event()
                    .unwrap()
                    .retain(&store)
                    .unwrap();
                next.prepare_activity_results()
                    .unwrap()
                    .retain(&store)
                    .unwrap();
                let produced = recovered.prepare_activity_candidate(&next).unwrap();
                let mut body: Value = serde_json::from_str(produced.original_json()).unwrap();
                if is_ai {
                    assert_eq!(body["facts"], json!([]));
                    body["facts"] = json!([{"eventIndex":"0","accountId":input.owner_account_id,"kind":"native:cast-red-or-green","revision":"native-cast-colors-v1","value":"1"}]);
                } else {
                    assert_eq!(body["facts"].as_array().unwrap().len(), 2);
                    body["facts"][0]["value"] = json!("2");
                }
                let forged = crate::admission_ports::HostCandidate::from_bytes(
                    &serde_json::to_vec(&body).unwrap(),
                )
                .unwrap();
                let submit_attempts = faults.submit_attempts.load(Ordering::SeqCst);
                assert!(
                    proxied
                        .commit_player(
                            &store,
                            NativePlayerCommit {
                                authority,
                                recovered: &recovered,
                                issued: &issued,
                                original: &original,
                                next: &next,
                                candidate: &forged
                            }
                        )
                        .is_err()
                );
                assert!(
                    authority
                        .original(&repository, &original)
                        .unwrap()
                        .is_none()
                );
                assert_eq!(
                    faults.submit_attempts.load(Ordering::SeqCst),
                    submit_attempts
                );
                tampered += 1;
                if case.mixed_ai && !is_ai {
                    faults.armed.store(true, Ordering::SeqCst);
                }
            }
            let result = if let Some(access) = human_access.as_ref() {
                runtime.command_current_checked(
                    if faults.armed.load(Ordering::SeqCst) {
                        &proxied
                    } else {
                        &repository
                    },
                    &actors[index],
                    &owner,
                    access,
                    &original,
                )
            } else {
                runtime.controller_command(
                    &repository,
                    &secrets,
                    &controller.as_ref().unwrap().1,
                    &owner,
                    &original,
                )
            };
            assert!(matches!(result.unwrap(), ApiResponse::Success { .. }));
            turns += 1;
            if casting {
                let current = lifecycle_reads::sql(
                    &database,
                    "SELECT coalesce(jsonb_agg(jsonb_build_object('accountId',f.account_id,'kind',f.kind,'revision',f.fact_revision,'value',f.value::text) ORDER BY f.kind),'[]') FROM app.game_activity_fact f JOIN app.game_event e ON e.id=f.source_event_id WHERE e.game_id=$1 AND e.accepted_revision=$2::text::numeric",
                    &[
                        &case.game_id,
                        &(checkpoint.basis().revision.get() + 1).to_string(),
                    ],
                );
                if is_ai {
                    ai_casts += 1;
                    assert_eq!(current, json!([]));
                    let consumed = consume(&database, case.game_id);
                    assert_eq!(consumed["data"]["rewards"], "0");
                    assert_eq!(
                        progress(&database, input.quest_assignment_id)["progress"],
                        "19"
                    );
                } else {
                    human_casts += 1;
                    assert_eq!(current.as_array().unwrap().len(), 2);
                    assert!(
                        current
                            .as_array()
                            .unwrap()
                            .iter()
                            .all(|f| f["accountId"] == json!(input.owner_account_id)
                                && f["value"] == "1")
                    );
                    if case.mixed_ai {
                        assert_eq!(faults.submits.load(Ordering::SeqCst), 1);
                        assert!(faults.lost.load(Ordering::SeqCst));
                        assert_eq!(faults.forbidden.load(Ordering::SeqCst), 0);
                        let repeated = runtime
                            .command_current_checked(
                                &proxied,
                                &actors[index],
                                &owner,
                                human_access.as_ref().unwrap(),
                                &original,
                            )
                            .unwrap();
                        assert!(matches!(repeated, ApiResponse::Success { .. }));
                        assert_eq!(faults.submits.load(Ordering::SeqCst), 1);
                    } else {
                        assert_eq!(consume(&database, case.game_id)["data"]["rewards"], "0");
                        assert_eq!(
                            progress(&database, input.quest_assignment_id)["progress"],
                            "19"
                        );
                    }
                    break;
                }
            }
        }
        assert_eq!(human_casts, 1);
        assert_eq!(ai_casts, usize::from(case.mixed_ai));
        let recovered = repository
            .recover_with_history(
                case.game_id,
                &store,
                &programs,
                NativeRecoveryBudget::new(10000, 100_000_000).unwrap(),
            )
            .unwrap()
            .unwrap();
        assert_eq!(
            recovered.history_evidence(),
            NativeHistoryEvidence::ActivityTransitionsV1
        );
        let reward = if case.mixed_ai {
            let result = consume_with_rollback_and_competitor(&database, case.game_id);
            assert_eq!(
                progress(&database, input.quest_assignment_id)["progress"],
                "20"
            );
            assert_eq!(consume(&database, case.game_id)["data"]["events"], "0");
            result
        } else {
            json!({"openProgress":"19","rewards":"0"})
        };
        cases.push(json!({"gameId":case.game_id,"mixedAi":case.mixed_ai,"humanCasts":human_casts,"aiCasts":ai_casts,"originalCommands":turns,"acceptedRevision":recovered.checkpoint().basis().revision,"checkedHistory":"ActivityTransitionsV1","reward":reward}));
    }
    let accounting = lifecycle_reads::sql(
        &database,
        "SELECT jsonb_build_object('progress',q.progress::text,'reward',d.reward_coins::text,'target',d.target::text,'completedAtMatchesFact',q.completed_at=e.accepted_at,'operationState',o.state,'ledgerOperations',(SELECT count(*) FROM app.ledger_operation WHERE source_scope='quest_reward_v1' AND source_ref=q.id::text),'rewardReceipts',(SELECT count(*) FROM app.command_receipt WHERE scope='quest_reward_v1' AND target_ref=q.id::text AND state='accepted'),'credited',(SELECT sum(quantity)::text FROM app.ledger_posting WHERE operation_id=o.id),'sourceKind',f.kind,'sourceValue',f.value::text,'wallet',(SELECT sum(i.owned_quantity)::text FROM app.inventory_asset i JOIN app.asset_owner owner ON owner.id=i.owner_id JOIN app.asset_definition a ON a.id=i.asset_definition_id WHERE owner.account_id=q.account_id AND a.currency_key='coin')) FROM app.quest_assignment q JOIN app.quest_definition_revision d ON d.id=q.definition_id JOIN app.game_activity_fact f ON f.id=q.reward_source_fact_id JOIN app.game_event e ON e.id=f.source_event_id JOIN app.ledger_operation o ON o.id=q.reward_operation_id WHERE q.id=$1",
        &[&input.quest_assignment_id],
    );
    assert_eq!(accounting["reward"], "500");
    assert_eq!(accounting["target"], "20");
    assert_eq!(accounting["progress"], "20");
    assert_eq!(accounting["sourceValue"], "1");
    assert_eq!(accounting["credited"], "500");
    assert_eq!(accounting["wallet"], "15000");
    assert_eq!(accounting["ledgerOperations"], 1);
    assert_eq!(accounting["rewardReceipts"], 1);
    assert_eq!(accounting["completedAtMatchesFact"], true);
    assert_eq!(accounting["operationState"], "committed");
    assert_eq!(tampered, 3);
    println!(
        "OWNED_NATIVE_CAST_ACTIVITY_OK {}",
        json!({"cases":cases,"tamperedFramesRefusedBeforeSubmit":tampered,"accounting":accounting,"priorQuestProgressFixture":"19","nativeCreditedCurrentCast":"1","fullAdmissionQualified":false,"fullObjectiveRosterQualified":false})
    );
}
