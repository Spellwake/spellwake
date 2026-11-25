//! End-to-end native Rust AI runtime integration tests (offline + optional live Codex CLI smoke).
use super::{
    codex_app_server::{CodexAppServerConfig, CodexAppServerProvider},
    compact::{AiActionPlan, compact_prompt, compact_reply_schema_for_snapshot},
    provider::{
        AiDecisionProvider, AiModelRequest, AiTierCatalog, CodexCliProvider, StaticScriptedProvider,
    },
    quota::AiTier,
    scheduler::{AiScheduler, ScheduleDecision, WakeCondition, WakeReason},
};
use crate::{
    native_game::{
        NativeAbilityReference, NativeBinding, NativeDisclosure, NativeDisplayScope,
        NativeDisplaySource, NativeInitialCard, NativeInitialIds, NativeInitialParticipant,
        NativeInitialSpec, NativeInitialZone, NativeObjectDisplay, NativeProgramPool,
        NativeProgramSource, NativeProjectionError, NativeProjectionMetadata, NativeScope,
        NativeSeatBinding, prepare_initial,
    },
    player_wire::{
        NativeRevision, PlayerAbilityKind, PlayerAction, PlayerCardType, PlayerCommand,
        PlayerSnapshot,
    },
};
use serde_json::json;
use spellwake_card_programs::OracleId;
use spellwake_engine::{
    characteristics::ObjectCharacteristics,
    effects::DamageAllocationDomain,
    game::{GameContext, GameState},
    object_identity::GameObject,
    occurrence::{ExecutionBindings, WaitingInstance},
    priority::{PriorityPhase, SeatId},
    program::{ExecutableBody, Program},
    stack_window::{StackEntry, StackKind, WindowPhase},
    token::TokenEntryPreview,
};
use std::{fs, path::PathBuf};
use uuid::Uuid;

struct TestDisplay<'a> {
    source: &'a NativeProgramSource,
    context: GameContext,
}

impl NativeDisplaySource for TestDisplay<'_> {
    fn context(&self) -> &GameContext {
        &self.context
    }
    fn scope(&self) -> NativeDisplayScope {
        NativeDisplayScope::SourceOnly
    }
    fn object(
        &self,
        _: &GameObject,
        definition: &Program,
    ) -> Result<NativeObjectDisplay, NativeProjectionError> {
        let ch = self
            .source
            .printed_card(definition)
            .map_err(NativeProjectionError::metadata)?
            .map(|c| c.characteristics().clone())
            .ok_or(NativeProjectionError::MissingDisplay)?;
        Ok(NativeObjectDisplay {
            definition_id: None,
            printing_id: None,
            custom_card_revision_id: None,
            characteristics: ch,
        })
    }
    fn ability(
        &self,
        _: &GameObject,
        _: &ObjectCharacteristics,
        ability: NativeAbilityReference,
    ) -> Result<String, NativeProjectionError> {
        Ok(match ability {
            NativeAbilityReference::Mana(_) => "Add mana".into(),
            _ => "Activated ability".into(),
        })
    }
    fn stack(
        &self,
        entry: &StackEntry,
        _: Option<&ExecutableBody>,
        _: Option<&ExecutionBindings>,
    ) -> Result<String, NativeProjectionError> {
        Ok(match entry.kind() {
            StackKind::CardSpell(_) => "Card spell".into(),
            _ => "Stack entry".into(),
        })
    }
    fn token_preview(&self, _: &TokenEntryPreview) -> Result<String, NativeProjectionError> {
        Err(NativeProjectionError::MissingDisplay)
    }
    fn trigger(&self, _: &WaitingInstance) -> Result<String, NativeProjectionError> {
        Ok("Trigger".into())
    }
    fn prevention_attempt(
        &self,
        _: &DamageAllocationDomain,
    ) -> Result<String, NativeProjectionError> {
        Ok("Damage".into())
    }
}

fn seat_uuid(index: usize) -> Uuid {
    Uuid::from_u128(101 + index as u128)
}

fn make_entropy(seed: u64) -> Vec<NativeRevision> {
    let mut x = seed;
    (0..1024)
        .map(|_| {
            x = x.wrapping_add(0x9e3779b97f4a7c15);
            let mut z = x;
            z = (z ^ (z >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94d049bb133111eb);
            (z ^ (z >> 31)).into()
        })
        .collect()
}

fn make_bears_deck() -> Vec<NativeInitialCard> {
    let forest = OracleId::try_from("b34bb2dc-c1af-4d77-b0b3-a0fb342a5fc6".to_owned()).unwrap();
    let bears = OracleId::try_from("14c8f55d-d177-4c25-a931-ebeb9e6062a0".to_owned()).unwrap();
    let mut deck = Vec::with_capacity(60);
    for _ in 0..30 {
        deck.push(NativeInitialCard {
            oracle_id: forest.clone(),
            zone: NativeInitialZone::Library,
        });
    }
    for _ in 0..30 {
        deck.push(NativeInitialCard {
            oracle_id: bears.clone(),
            zone: NativeInitialZone::Library,
        });
    }
    deck
}

fn open_test_game() -> (NativeProgramSource, GameState, GameContext) {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("repo root");
    let metadata = fs::read(root.join("work/catalogue/oracle-metadata.json")).expect("metadata");
    let snapshot =
        fs::read(root.join("work/catalogue/oracle-cards-20261029210158.jsonl.gz")).expect("snap");
    let vocab = fs::read(root.join(
        "work/resolution-value-choices/current-s1-packets-v1/creature-vocabulary.input.v1.json",
    ))
    .expect("vocab");
    let source = NativeProgramSource::from_bytes(
        NativeProgramPool::ResolutionSelectionNaturalMana,
        &metadata,
        snapshot,
        &vocab,
    )
    .expect("load native program source");

    let opening = prepare_initial(
        NativeInitialIds {
            game_id: Uuid::from_u128(9001),
            context_id: Uuid::from_u128(9002),
            release_id: Uuid::from_u128(9003),
            profile_id: Uuid::from_u128(9004),
        },
        &source,
        NativeInitialSpec {
            first_player_seat_id: seat_uuid(0),
            participants: vec![
                NativeInitialParticipant {
                    seat_id: seat_uuid(0),
                    life: 20.into(),
                    cards: make_bears_deck(),
                },
                NativeInitialParticipant {
                    seat_id: seat_uuid(1),
                    life: 20.into(),
                    cards: make_bears_deck(),
                },
            ],
            entropy: make_entropy(2026112299),
        },
    )
    .expect("prepare initial game");

    let (_, _, candidate, _) = opening.into_parts();
    let state = candidate.into_state();
    let context = state.basis().context.clone();
    (source, state, context)
}

fn binding_for(state: &GameState, viewer: Uuid) -> NativeBinding {
    NativeBinding::new(
        Uuid::from_u128(9002),
        state.basis().clone(),
        vec![
            NativeSeatBinding {
                seat_id: seat_uuid(0),
                native_seat: SeatId(0),
            },
            NativeSeatBinding {
                seat_id: seat_uuid(1),
                native_seat: SeatId(1),
            },
        ],
        viewer,
    )
    .expect("valid binding")
}

fn current_actor(state: &GameState) -> Uuid {
    let native = if let Some(d) = state.pending_decision() {
        d.chooser()
    } else if let WindowPhase::Priority(PriorityPhase::Ready { holder }) =
        state.execution().window().phase()
    {
        holder
    } else {
        panic!("expected decision or priority");
    };
    match native {
        SeatId(0) => seat_uuid(0),
        SeatId(1) => seat_uuid(1),
        _ => panic!("unexpected seat"),
    }
}

fn probe_basic(scope: &NativeScope<'_>, snap: &PlayerSnapshot) -> Vec<PlayerAction> {
    let mut out = Vec::new();
    if !snap.decisions.is_empty() {
        return out;
    }
    for obj in &snap.visible_objects {
        if obj.owner_seat_id != snap.viewer_seat_id
            && obj.controller_seat_id != Some(snap.viewer_seat_id)
        {
            continue;
        }
        if obj
            .public_properties
            .as_ref()
            .is_some_and(|p| p.card_types.contains(&PlayerCardType::Land))
        {
            let act = PlayerAction::PlayLand {
                source: obj.handle.clone(),
            };
            let cmd = PlayerCommand {
                request_id: Uuid::new_v4(),
                basis: scope.basis().clone(),
                action: act.clone(),
            };
            if scope.propose(&cmd).is_ok() {
                out.push(act);
            }
        }
        for ab in &obj.abilities {
            if ab.kind == PlayerAbilityKind::Mana {
                let act = PlayerAction::ActivateMana {
                    source: obj.handle.clone(),
                    ability: ab.handle.clone(),
                };
                let cmd = PlayerCommand {
                    request_id: Uuid::new_v4(),
                    basis: scope.basis().clone(),
                    action: act.clone(),
                };
                if scope.propose(&cmd).is_ok() {
                    out.push(act);
                }
            }
        }
    }
    out
}

#[test]
#[ignore = "requires local work/catalogue and creature vocabulary inputs"]
fn native_rust_ai_pipeline_compacts_decides_schedules_and_advances_real_game() {
    let (source, mut state, context) = open_test_game();
    let display = TestDisplay {
        source: &source,
        context,
    };

    let keep_reply = json!({
        "action": {
            "kind": "choose",
            "decision": "d1",
            "response": { "kind": "option", "option": "opt1" }
        },
        "delegate_pass": false,
        "wake": [],
        "max_skips": 0
    })
    .to_string();
    let pass_delegate = json!({
        "action": { "kind": "pass" },
        "delegate_pass": true,
        "wake": ["own_main", "stack_changed"],
        "max_skips": 4
    })
    .to_string();
    let provider = StaticScriptedProvider::new(
        vec![keep_reply.clone(), keep_reply, pass_delegate.clone()],
        pass_delegate,
    );

    let mut sched0 = AiScheduler::new();
    let mut seq = 0_u64;

    // Step 1 & 2: both players resolve opening mulligan decisions via compact_v1.
    for _ in 0..2 {
        let actor = current_actor(&state);
        let mut scope = NativeScope::new(&state, binding_for(&state, actor)).unwrap();
        let meta = NativeProjectionMetadata {
            through_event_sequence: seq.into(),
            deadline_at: None,
            disclosure: NativeDisclosure::LiveParticipant,
        };
        let snap = scope.project(&meta, &display).unwrap();
        assert!(!snap.decisions.is_empty());

        let (prompt, codec) = compact_prompt(&snap, &[], &[], None).unwrap();
        assert!(
            !prompt.contains("n1."),
            "raw handle leaked into compact prompt"
        );
        assert!(prompt.contains("\"boundary\":\"required_decision\""));

        let resp = provider
            .decide(&AiModelRequest {
                tier: AiTier::Normal,
                game_id: snap.basis.game_id,
                seat_id: actor,
                prompt,
                schema: compact_reply_schema_for_snapshot(&snap),
                cache_key: "test".into(),
            })
            .unwrap();
        let expanded = codec.expand_reply(&resp.raw_json, &snap.basis).unwrap();
        let AiActionPlan::Single(cmd) = expanded.plan else {
            panic!("expected single choose command");
        };
        let prepared = scope.propose(&cmd).unwrap();
        state = prepared.into_parts().3.into_state();
        seq += 1;
    }

    // Step 3: Turn 1 upkeep priority — verify scheduler calls model, installs pass grant,
    // and wakes on `own_main` when reaching precombat main!
    let actor = current_actor(&state);
    assert_eq!(actor, seat_uuid(0));
    let mut scope = NativeScope::new(&state, binding_for(&state, actor)).unwrap();
    let meta = NativeProjectionMetadata {
        through_event_sequence: seq.into(),
        deadline_at: None,
        disclosure: NativeDisclosure::LiveParticipant,
    };
    let snap = scope.project(&meta, &display).unwrap();
    assert!(snap.decisions.is_empty());
    assert_eq!(
        sched0.evaluate(&snap),
        ScheduleDecision::CallModel(WakeReason::NoGrant)
    );
    let basic = probe_basic(&scope, &snap);
    let (prompt, codec) = compact_prompt(&snap, &basic, &[], None).unwrap();
    assert!(!prompt.contains("n1."));
    assert!(prompt.contains("\"boundary\":\"priority\""));
    assert!(prompt.contains("\"decisions\":[]"));
    assert!(prompt.contains("\"can_play_lands\":"));

    let resp = provider
        .decide(&AiModelRequest {
            tier: AiTier::Advanced,
            game_id: snap.basis.game_id,
            seat_id: actor,
            prompt,
            schema: compact_reply_schema_for_snapshot(&snap),
            cache_key: "test".into(),
        })
        .unwrap();
    let expanded = codec.expand_reply(&resp.raw_json, &snap.basis).unwrap();
    let AiActionPlan::Single(cmd) = expanded.plan else {
        panic!("expected pass");
    };
    let prepared = scope.propose(&cmd).unwrap();
    state = prepared.into_parts().3.into_state();
    seq += 1;

    // Install grant on post-accepted snapshot.
    let mut post_scope = NativeScope::new(&state, binding_for(&state, seat_uuid(0))).unwrap();
    let post_meta = NativeProjectionMetadata {
        through_event_sequence: seq.into(),
        deadline_at: None,
        disclosure: NativeDisclosure::LiveParticipant,
    };
    let post_snap = post_scope.project(&post_meta, &display).unwrap();
    sched0.record_model_acceptance(
        expanded.delegate_pass,
        expanded.wake,
        expanded.max_skips,
        &post_snap,
    );

    // Seat 1 passes upkeep -> advances to Turn 1 precombat-main where Seat 0 has priority.
    let s1_scope = NativeScope::new(&state, binding_for(&state, seat_uuid(1))).unwrap();
    let s1_cmd = PlayerCommand {
        request_id: Uuid::new_v4(),
        basis: s1_scope.basis().clone(),
        action: PlayerAction::Pass {},
    };
    state = s1_scope
        .propose(&s1_cmd)
        .unwrap()
        .into_parts()
        .3
        .into_state();
    seq += 1;

    // Now Seat 0 holds priority in precombat-main: scheduler MUST wake on OwnMain!
    assert_eq!(current_actor(&state), seat_uuid(0));
    let mut main_scope = NativeScope::new(&state, binding_for(&state, seat_uuid(0))).unwrap();
    let main_meta = NativeProjectionMetadata {
        through_event_sequence: seq.into(),
        deadline_at: None,
        disclosure: NativeDisclosure::LiveParticipant,
    };
    let main_snap = main_scope.project(&main_meta, &display).unwrap();
    assert_eq!(
        sched0.evaluate(&main_snap),
        ScheduleDecision::CallModel(WakeReason::ConditionTriggered(WakeCondition::OwnMain))
    );
}

/// Live end-to-end Rust verification against the local pinned Codex CLI binary for BOTH
/// Normal AI (`gpt-6-luna` medium) and Advanced AI (`gpt-6.1-terra` medium) playing real
/// Spellwake native turns with zero Python in the loop.
#[test]
#[ignore = "requires local Codex CLI binary via SPELLWAKE_AI_CODEX_CLI"]
fn live_codex_cli_normal_and_advanced_native_turns() {
    let cli = std::env::var("SPELLWAKE_AI_CODEX_CLI")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            PathBuf::from(
                "/Users/quzzar/Projects/ai-harness-test/work/native-cli-update-recovery-001/CodexCLI.app/Contents/MacOS/codex",
            )
        });
    let provider = CodexCliProvider::new(cli, AiTierCatalog::from_env()).expect("codex provider");
    let (source, mut state, context) = open_test_game();
    let display = TestDisplay {
        source: &source,
        context,
    };
    let tiers = [AiTier::Normal, AiTier::Advanced];

    // Execute 4 live native boundaries: Seat 0 (Normal/Luna) mulligan, Seat 1 (Advanced/Terra)
    // mulligan, plus Turn 1 upkeep/main priority turns.
    for step in 0_u64..4 {
        let actor = current_actor(&state);
        let seat_idx = usize::from(actor == seat_uuid(1));
        let tier = tiers[seat_idx];
        let mut scope = NativeScope::new(&state, binding_for(&state, actor)).unwrap();
        let meta = NativeProjectionMetadata {
            through_event_sequence: step.into(),
            deadline_at: None,
            disclosure: NativeDisclosure::LiveParticipant,
        };
        let snap = scope.project(&meta, &display).unwrap();
        let basic = probe_basic(&scope, &snap);
        let (prompt, codec) = compact_prompt(&snap, &basic, &[], None).unwrap();
        let prompt_len = prompt.len();
        let resp = provider
            .decide(&AiModelRequest {
                tier,
                game_id: snap.basis.game_id,
                seat_id: actor,
                prompt: prompt.clone(),
                schema: compact_reply_schema_for_snapshot(&snap),
                cache_key: format!("rust-live-smoke:{}:{}", tier.as_str(), seat_idx),
            })
            .expect("live codex decision succeeded");
        eprintln!(
            "step={step} tier={} model={} prompt_chars={prompt_len} latency_ms={} in={} cached={} out={} raw={}",
            tier.as_str(),
            resp.model,
            resp.latency_ms,
            resp.usage.input_tokens,
            resp.usage.cached_input_tokens,
            resp.usage.output_tokens,
            resp.raw_json,
        );
        let expanded = codec
            .expand_reply(&resp.raw_json, &snap.basis)
            .expect("expanded compact reply valid");
        match expanded.plan {
            AiActionPlan::Single(cmd) => {
                let prepared = scope.propose(&cmd).unwrap_or_else(|err| {
                    panic!(
                        "native engine rejected AI action on step={step}: {err}\nPROMPT:\n{prompt}\nREPLY:\n{}",
                        resp.raw_json
                    );
                });
                state = prepared.into_parts().3.into_state();
            }
            AiActionPlan::CastBatch(cmds) => {
                let prepared = scope.prepare_cast_batch(&cmds).unwrap_or_else(|err| {
                    panic!(
                        "native engine rejected AI cast-batch on step={step}: {err}\nPROMPT:\n{prompt}\nREPLY:\n{}",
                        resp.raw_json
                    );
                });
                state = prepared.into_parts().0;
            }
        }
    }
}

/// Live end-to-end Rust verification of the production persistent ChatGPT-login
/// `CodexAppServerProvider` playing BOTH Normal AI (`gpt-6-luna`) and Advanced AI
/// (`gpt-6.1-terra`) across 6 native Spellwake boundaries, proving prompt cache
/// hits (`cached_input_tokens > 0`) on reused per-seat ephemeral threads with
/// zero OpenAI API key and zero Python in the loop.
#[test]
#[ignore = "requires local ChatGPT-logged-in Codex CLI via SPELLWAKE_AI_CODEX_CLI"]
fn live_chatgpt_app_server_normal_and_advanced_with_cache_hits() {
    let cli = std::env::var("SPELLWAKE_AI_CODEX_CLI")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            PathBuf::from(
                "/Users/quzzar/Projects/ai-harness-test/work/native-cli-update-recovery-001/CodexCLI.app/Contents/MacOS/codex",
            )
        });
    let provider = CodexAppServerProvider::new(CodexAppServerConfig {
        cli_path: cli,
        codex_home: None,
        seed_auth_json: None,
        catalog: AiTierCatalog::from_env(),
        concurrency: 2,
    })
    .expect("app-server provider");
    provider
        .verify_chatgpt_login()
        .expect("verified ChatGPT login");

    let (source, mut state, context) = open_test_game();
    let display = TestDisplay {
        source: &source,
        context,
    };
    let tiers = [AiTier::Normal, AiTier::Advanced];
    let mut saw_cache_hit = false;

    for step in 0_u64..6 {
        let actor = current_actor(&state);
        let seat_idx = usize::from(actor == seat_uuid(1));
        let tier = tiers[seat_idx];
        let mut scope = NativeScope::new(&state, binding_for(&state, actor)).unwrap();
        let meta = NativeProjectionMetadata {
            through_event_sequence: step.into(),
            deadline_at: None,
            disclosure: NativeDisclosure::LiveParticipant,
        };
        let snap = scope.project(&meta, &display).unwrap();
        let basic = probe_basic(&scope, &snap);
        let (prompt, codec) = compact_prompt(&snap, &basic, &[], None).unwrap();
        let resp = provider
            .decide(&AiModelRequest {
                tier,
                game_id: snap.basis.game_id,
                seat_id: actor,
                prompt: prompt.clone(),
                schema: compact_reply_schema_for_snapshot(&snap),
                cache_key: format!("rust-chatgpt-smoke:{}:{}", tier.as_str(), seat_idx),
            })
            .expect("live ChatGPT app-server decision succeeded");
        if resp.usage.cached_input_tokens > 0 {
            saw_cache_hit = true;
        }
        eprintln!(
            "chatgpt-step={step} seat={seat_idx} tier={} model={} latency_ms={} in={} cached={} out={} raw={}",
            tier.as_str(),
            resp.model,
            resp.latency_ms,
            resp.usage.input_tokens,
            resp.usage.cached_input_tokens,
            resp.usage.output_tokens,
            resp.raw_json,
        );
        let expanded = codec
            .expand_reply(&resp.raw_json, &snap.basis)
            .expect("expanded compact reply valid");
        match expanded.plan {
            AiActionPlan::Single(cmd) => {
                let prepared = scope.propose(&cmd).unwrap_or_else(|err| {
                    panic!(
                        "native engine rejected ChatGPT AI action on step={step}: {err}\nREPLY:\n{}",
                        resp.raw_json
                    );
                });
                state = prepared.into_parts().3.into_state();
            }
            AiActionPlan::CastBatch(cmds) => {
                let prepared = scope.prepare_cast_batch(&cmds).unwrap_or_else(|err| {
                    panic!(
                        "native engine rejected ChatGPT AI cast-batch on step={step}: {err}\nREPLY:\n{}",
                        resp.raw_json
                    );
                });
                state = prepared.into_parts().0;
            }
        }
    }

    assert!(
        saw_cache_hit,
        "expected prompt cache hits across reused per-seat ChatGPT app-server threads"
    );
}
