//! Shared original-wire compatibility, without native adapter/runtime qualification.
use serde::Deserialize;
use spellwake_application::player_wire::*;
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
    context: Option<Context>,
}
#[derive(Deserialize)]
struct AbilityFixture {
    source: PlayerHandle,
    handle: PlayerHandle,
    kind: PlayerAbilityKind,
}
#[derive(Deserialize)]
struct Context {
    basis: PlayerBasis,
    viewer: uuid::Uuid,
    roster: Vec<uuid::Uuid>,
    descriptor: Option<PlayerDecision>,
    query: Option<PlayerChoiceQuery>,
    #[serde(default)]
    pages: Vec<PlayerChoicePage>,
    #[serde(default, rename = "authorizedFullDisclosure")]
    authorized_full_disclosure: bool,
    #[serde(rename = "throughSequence")]
    through_sequence: Option<spellwake_application::amount::Amount>,
    #[serde(default, rename = "visibleObjects")]
    visible_objects: Vec<PlayerHandle>,
    #[serde(default, rename = "knownIdentities")]
    known_identities: Vec<PlayerHandle>,
    #[serde(default, rename = "orderedZones")]
    ordered_zones: Vec<PlayerHandle>,
    #[serde(default, rename = "creatureTypeMembers")]
    creature_type_members: Vec<String>,
    #[serde(default, rename = "abilityBindings")]
    ability_bindings: Vec<AbilityFixture>,
    original: Option<PlayerCommand>,
    #[serde(rename = "receiptQuery")]
    receipt_query: Option<PlayerReceiptQuery>,
    #[serde(rename = "reconnectRequest")]
    reconnect_request: Option<PlayerReconnectRequest>,
    #[serde(rename = "issuedCursor")]
    issued_cursor: Option<PlayerCursor>,
    #[serde(rename = "snapshotCursor")]
    snapshot_cursor: Option<PlayerCursor>,
    #[serde(rename = "eventFacts")]
    event_facts: Option<EventFixture>,
    #[serde(rename = "historyQuery")]
    history_query: Option<PlayerHistoryQuery>,
    #[serde(rename = "historyEvents", default)]
    history_events: Vec<EventFixture>,
    #[serde(rename = "scannedCursor")]
    scanned_cursor: Option<PlayerCursor>,
    #[serde(rename = "hasMore", default)]
    has_more: bool,
    #[serde(rename = "previousCursor")]
    previous_cursor: Option<PlayerCursor>,
    presentation: Option<PlayerPresentation>,
    #[serde(rename = "streamGap", default)]
    stream_gap: bool,
    #[serde(rename = "expectedEffect")]
    expected_effect: Option<String>,
    #[serde(rename = "replayRequest")]
    replay_request: Option<PlayerReplayRequest>,
    #[serde(rename = "replayAuth")]
    replay_auth: Option<ReplayFixture>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReplayFixture {
    requester_account_id: uuid::Uuid,
    participant_account_ids: Vec<uuid::Uuid>,
    historical_seat_id: uuid::Uuid,
    historical_seat_owner: Option<uuid::Uuid>,
    completed: bool,
    full_information_eligible: bool,
    source_game_id: uuid::Uuid,
    source_context_id: uuid::Uuid,
    release_id: uuid::Uuid,
    schema_revision: PlayerReplaySchema,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct EventFixture {
    before: PlayerSnapshot,
    after: PlayerSnapshot,
    before_context: Box<Context>,
    after_context: Box<Context>,
    terminal: Option<PlayerTerminal>,
}
impl EventFixture {
    fn produce(&self) -> Result<Option<PlayerVisibleEvent>, InvalidPlayerWire> {
        self.before_context.with_projection(|before_projection| {
            self.after_context.with_projection(|after_projection| {
                PlayerVisibleEvent::from_projections(&PlayerEventFacts {
                    before: &self.before,
                    before_projection,
                    after: &self.after,
                    after_projection,
                    terminal: self.terminal.as_ref(),
                })
            })
        })
    }
}
impl Context {
    fn with_history(&self, run: impl FnOnce(PlayerHistoryFacts<'_>) -> bool) -> bool {
        // These synthetic page fixtures have no source vocabulary or ability handles.
        // The separate event cases exercise the vocabulary-bearing projection path.
        let events: Vec<_> = self
            .history_events
            .iter()
            .map(|e| {
                for p in [&e.before_context, &e.after_context] {
                    assert!(p.creature_type_members.is_empty() && p.ability_bindings.is_empty());
                }
                PlayerEventFacts {
                    before: &e.before,
                    after: &e.after,
                    before_projection: e.before_context.projection(None, &[]),
                    after_projection: e.after_context.projection(None, &[]),
                    terminal: e.terminal.as_ref(),
                }
            })
            .collect();
        let facts = PlayerHistoryFacts {
            viewer: self.viewer,
            requested_cursor: self.issued_cursor.as_ref(),
            scanned_cursor: self.scanned_cursor.as_ref().unwrap(),
            has_more: self.has_more,
            events: &events,
        };
        run(facts)
    }
    fn with_projection<T>(&self, run: impl FnOnce(PlayerProjectionFacts<'_>) -> T) -> T {
        // Synthetic vocabulary checks representation/membership only, never engine qualification.
        let vocabulary = if self.creature_type_members.is_empty() {
            None
        } else {
            Some(
                spellwake_contracts::creature_types::CreatureTypeVocabulary::new(
                    "fixture-public-vocabulary".to_owned(),
                    [0; 32],
                    self.creature_type_members.clone(),
                )
                .unwrap(),
            )
        };
        let bindings: Vec<_> = self
            .ability_bindings
            .iter()
            .map(|a| PlayerAbilityBinding {
                source: a.source.clone(),
                handle: a.handle.clone(),
                kind: a.kind,
            })
            .collect();
        run(self.projection(vocabulary.as_ref(), &bindings))
    }
    fn projection<'a>(
        &'a self,
        vocabulary: Option<&'a spellwake_contracts::creature_types::CreatureTypeVocabulary>,
        bindings: &'a [PlayerAbilityBinding],
    ) -> PlayerProjectionFacts<'a> {
        PlayerProjectionFacts {
            basis: &self.basis,
            viewer: self.viewer,
            roster: &self.roster,
            through_sequence: self.through_sequence.as_ref().unwrap(),
            visible_objects: &self.visible_objects,
            known_identities: &self.known_identities,
            ordered_zones: &self.ordered_zones,
            authorized_full_disclosure: self.authorized_full_disclosure,
            creature_vocabulary: vocabulary,
            ability_bindings: bindings,
        }
    }
}
#[test]
fn player_wire_contracts_match_the_shared_original_byte_cases() {
    let fixture: Fixture = serde_json::from_str(include_str!(
        "../../../packages/application-contracts/fixtures/player-wire.json"
    ))
    .unwrap();
    for case in fixture.cases {
        // Only standalone scalar fixtures lack a record decoder. Object cases
        // must exercise the production bound rather than a test-side precheck.
        let scalar_budget = case.wire.len() <= MAX_PLAYER_WIRE_BYTES;
        let valid = match case.kind.as_str() {
            "replay_schema" => {
                scalar_budget && serde_json::from_str::<PlayerReplaySchema>(&case.wire).is_ok()
            }
            "replay_mode" => {
                scalar_budget && serde_json::from_str::<PlayerReplayMode>(&case.wire).is_ok()
            }
            "replay_request" => {
                let c = case.context.as_ref().unwrap();
                decode_player_record::<PlayerReplayRequest>(case.wire.as_bytes())
                    .is_ok_and(|x| x.validate(c.issued_cursor.as_ref()).is_ok())
            }
            "replay_frame" => {
                let c = case.context.as_ref().unwrap();
                let a = c.replay_auth.as_ref().unwrap();
                c.with_projection(|projection| {
                    c.with_history(|history| {
                        let facts = PlayerReplayFacts {
                            requester_account_id: a.requester_account_id,
                            participant_account_ids: &a.participant_account_ids,
                            historical_seat_id: a.historical_seat_id,
                            historical_seat_owner: a.historical_seat_owner,
                            completed: a.completed,
                            full_information_eligible: a.full_information_eligible,
                            source: PlayerReplaySource {
                                game_id: a.source_game_id,
                                context_id: a.source_context_id,
                                release_id: a.release_id,
                                schema_revision: &a.schema_revision,
                            },
                            projection,
                            history,
                        };
                        decode_player_record::<PlayerReplayFrame>(case.wire.as_bytes()).is_ok_and(
                            |x| {
                                x.validate(c.replay_request.as_ref().unwrap(), &facts)
                                    .is_ok()
                            },
                        )
                    })
                })
            }
            "rules_integer" => {
                scalar_budget && serde_json::from_str::<RulesInteger>(&case.wire).is_ok()
            }
            "rules_count" => {
                scalar_budget && serde_json::from_str::<RulesCount>(&case.wire).is_ok()
            }
            "native_revision" => {
                scalar_budget && serde_json::from_str::<NativeRevision>(&case.wire).is_ok()
            }
            "handle" => scalar_budget && serde_json::from_str::<PlayerHandle>(&case.wire).is_ok(),
            "basis" => decode_player_record::<PlayerBasis>(case.wire.as_bytes()).is_ok(),
            "command" => decode_player_record::<PlayerCommand>(case.wire.as_bytes())
                .is_ok_and(|x| x.validate().is_ok()),
            "decision" => {
                let c = case.context.as_ref().unwrap();
                decode_player_record::<PlayerDecision>(case.wire.as_bytes()).is_ok_and(|x| {
                    x.validate(&c.basis, c.viewer, &c.roster, c.authorized_full_disclosure)
                        .is_ok()
                })
            }
            "choice" => {
                let c = case.context.as_ref().unwrap();
                decode_player_record::<PlayerCommand>(case.wire.as_bytes()).is_ok_and(|x| {
                    let valid = c
                        .descriptor
                        .as_ref()
                        .unwrap()
                        .validate_response(&x, c.viewer, &c.roster, &c.pages)
                        .is_ok();
                    if valid {
                        // Handle sequences are meaningful, particularly physical-owner
                        // top-to-bottom orders. Validation must not sort or normalize them.
                        assert_eq!(
                            serde_json::to_value(&x).unwrap(),
                            serde_json::from_str::<serde_json::Value>(&case.wire).unwrap(),
                            "{} response sequence changed",
                            case.name,
                        );
                    }
                    valid
                })
            }
            "choice_query" => decode_player_record::<PlayerChoiceQuery>(case.wire.as_bytes())
                .is_ok_and(|x| x.validate().is_ok()),
            "choice_page" => {
                let c = case.context.as_ref().unwrap();
                decode_player_record::<PlayerChoicePage>(case.wire.as_bytes())
                    .is_ok_and(|x| x.validate(c.query.as_ref().unwrap(), c.viewer).is_ok())
            }
            "snapshot" | "reconnect" | "stream" => {
                let c = case.context.as_ref().unwrap();
                c.with_projection(|facts| {
                    if case.kind == "snapshot" {
                        decode_player_record::<PlayerSnapshot>(case.wire.as_bytes())
                            .is_ok_and(|x| x.validate(&facts).is_ok())
                    } else if case.kind == "reconnect" {
                        let reconnect_facts = PlayerReconnectFacts {
                            projection: facts,
                            requested_cursor: c.issued_cursor.as_ref(),
                            snapshot_cursor: c.snapshot_cursor.as_ref().unwrap(),
                        };
                        decode_player_record::<PlayerReconnect>(case.wire.as_bytes()).is_ok_and(
                            |x| {
                                x.validate(c.reconnect_request.as_ref().unwrap(), &reconnect_facts)
                                    .is_ok()
                            },
                        )
                    } else {
                        let facts = if c.stream_gap {
                            PlayerStreamFacts::Gap {
                                requested_cursor: c.issued_cursor.as_ref().unwrap(),
                            }
                        } else {
                            PlayerStreamFacts::Snapshot {
                                projection: facts,
                                issued_cursor: c.snapshot_cursor.as_ref().unwrap(),
                                presentation: c.presentation.as_ref().unwrap(),
                            }
                        };
                        decode_player_record::<PlayerStreamFrame>(case.wire.as_bytes()).is_ok_and(
                            |x| match x.validate(c.previous_cursor.as_ref(), &facts) {
                                Ok(effect) => {
                                    let effect = match effect {
                                        PlayerStreamEffect::ApplySnapshot => "apply-snapshot",
                                        PlayerStreamEffect::RefreshPresentation => {
                                            "refresh-presentation"
                                        }
                                        PlayerStreamEffect::RequireSnapshot => "require-snapshot",
                                    };
                                    assert_eq!(
                                        c.expected_effect.as_deref(),
                                        Some(effect),
                                        "{}",
                                        case.name
                                    );
                                    true
                                }
                                Err(_) => false,
                            },
                        )
                    }
                })
            }
            "history_query" => {
                let c = case.context.as_ref().unwrap();
                decode_player_record::<PlayerHistoryQuery>(case.wire.as_bytes())
                    .is_ok_and(|x| x.validate(c.viewer, c.issued_cursor.as_ref()).is_ok())
            }
            "history_page" => {
                let c = case.context.as_ref().unwrap();
                c.with_history(|facts| {
                    decode_player_record::<PlayerHistoryPage>(case.wire.as_bytes()).is_ok_and(|x| {
                        x.validate(c.history_query.as_ref().unwrap(), &facts)
                            .is_ok()
                    })
                })
            }
            "event" => {
                let f = case.context.as_ref().unwrap().event_facts.as_ref().unwrap();
                f.before_context.with_projection(|before_projection| {
                    f.after_context.with_projection(|after_projection| {
                        let facts = PlayerEventFacts {
                            before: &f.before,
                            before_projection,
                            after: &f.after,
                            after_projection,
                            terminal: f.terminal.as_ref(),
                        };
                        decode_player_record::<PlayerVisibleEvent>(case.wire.as_bytes())
                            .is_ok_and(|x| x.validate(&facts).is_ok())
                    })
                })
            }
            "receipt" => {
                let c = case.context.as_ref().unwrap();
                decode_player_record::<PlayerReceipt>(case.wire.as_bytes())
                    .is_ok_and(|x| x.validate(c.original.as_ref().unwrap(), c.viewer).is_ok())
            }
            "receipt_query" => decode_player_record::<PlayerReceiptQuery>(case.wire.as_bytes())
                .is_ok_and(|x| x.validate().is_ok()),
            "cursor" => {
                let c = case.context.as_ref().unwrap();
                decode_player_record::<PlayerCursor>(case.wire.as_bytes()).is_ok_and(|x| {
                    x.validate(c.issued_cursor.as_ref().unwrap(), c.viewer)
                        .is_ok()
                })
            }
            "stream_request" => {
                let c = case.context.as_ref().unwrap();
                decode_player_record::<PlayerStreamRequest>(case.wire.as_bytes())
                    .is_ok_and(|x| x.validate(c.viewer, c.issued_cursor.as_ref()).is_ok())
            }
            "reconnect_request" => {
                let c = case.context.as_ref().unwrap();
                decode_player_record::<PlayerReconnectRequest>(case.wire.as_bytes())
                    .is_ok_and(|x| x.validate(c.viewer, c.issued_cursor.as_ref()).is_ok())
            }
            "receipt_results" => {
                #[derive(Deserialize)]
                #[serde(deny_unknown_fields)]
                struct Results {
                    receipts: Vec<PlayerReceipt>,
                }
                let c = case.context.as_ref().unwrap();
                decode_player_record::<Results>(case.wire.as_bytes()).is_ok_and(|x| {
                    c.receipt_query
                        .as_ref()
                        .unwrap()
                        .validate_receipts(&x.receipts, c.viewer)
                        .is_ok()
                })
            }
            _ => panic!("unknown maintained player fixture kind"),
        };
        assert_eq!(valid, case.valid, "{}", case.name);
    }
}

fn event_fixture(name: &str) -> EventFixture {
    let fixtures: Fixture = serde_json::from_str(include_str!(
        "../../../packages/application-contracts/fixtures/player-wire.json"
    ))
    .unwrap();
    fixtures
        .cases
        .into_iter()
        .find(|case| case.name == name)
        .unwrap()
        .context
        .unwrap()
        .event_facts
        .unwrap()
}

#[test]
fn projected_history_includes_every_changed_seat_value_once() {
    // Synthetic independent projection pairs test the pure observation producer,
    // not actual native execution, disclosure or archive/source qualification.
    for field in [
        "life",
        "counters",
        "mana",
        "poison",
        "eliminated",
        "combined",
    ] {
        let mut f = event_fixture("event-replace-seat-mana-add");
        let seat = &mut f.after.players[0];
        seat.mana_pool.clear();
        match field {
            "life" => seat.life = "19".to_owned().try_into().unwrap(),
            "counters" => {
                seat.counters.push(PlayerCounter {
                    name: "experience".to_owned(),
                    count: "1".to_owned().try_into().unwrap(),
                });
            }
            "mana" | "combined" => {
                seat.mana_pool = serde_json::from_value(serde_json::json!([
                    {"color":"blue","amount":"1"}
                ]))
                .unwrap();
                if field == "combined" {
                    seat.life = "19".to_owned().try_into().unwrap();
                }
            }
            "poison" => seat.poison = "1".to_owned().try_into().unwrap(),
            "eliminated" => seat.eliminated = true,
            _ => unreachable!(),
        }
        let event = f.produce().unwrap().unwrap();
        assert_eq!(event.changes.len(), 1, "{field}");
        let change = &event.changes[0];
        assert!(match field {
            "life" => matches!(change, PlayerPublicChange::Life { .. }),
            "counters" => matches!(change, PlayerPublicChange::Counters { .. }),
            _ =>
                matches!(change, PlayerPublicChange::ReplaceSeat { seat } if seat == &f.after.players[0]),
        });
    }
    let f = event_fixture("event-replace-seat-library-count");
    let event = f.produce().unwrap().unwrap();
    assert!(matches!(
        event.changes.as_slice(),
        [
            PlayerPublicChange::ReplaceSeat { .. },
            PlayerPublicChange::ReplaceZone { .. }
        ]
    ));
    let f = event_fixture("event-replace-seat-mana-empty");
    assert!(matches!(
        f.produce().unwrap().unwrap().changes.as_slice(),
        [PlayerPublicChange::ReplaceSeat { seat }] if seat.mana_pool.is_empty()
    ));
}

#[test]
fn projected_history_preserves_silent_progress_and_rejects_foreign_epochs() {
    let f = event_fixture("event-replace-seat-unchanged");
    assert!(f.produce().unwrap().is_none());
    let mut f = event_fixture("event-replace-seat-mana-add");
    f.after.basis.epoch = 2_u64.into();
    f.after_context.basis.epoch = 2_u64.into();
    assert!(f.produce().is_err());
}

#[test]
fn projected_history_captures_object_counters_and_terminal_result() {
    let mut f = event_fixture("event-sparse-sequence-not-revision");
    // The old validation-only fixture intentionally lists just life despite
    // supplying a terminal fact. Production must include that observation too.
    assert!(matches!(
        f.produce().unwrap().unwrap().changes.as_slice(),
        [
            PlayerPublicChange::Life { .. },
            PlayerPublicChange::Terminal { .. }
        ]
    ));
    f.terminal = None;
    f.after.players[0].life = f.before.players[0].life.clone();
    f.after.visible_objects[0].counters.push(PlayerCounter {
        name: "+1/+1".to_owned(),
        count: "1".to_owned().try_into().unwrap(),
    });
    let event = f.produce().unwrap().unwrap();
    assert!(matches!(
        event.changes.as_slice(),
        [PlayerPublicChange::Counters { subject, counters }]
          if matches!(subject.target(), PlayerTarget::Object { handle } if handle == &f.after.visible_objects[0].handle)
          && counters == &f.after.visible_objects[0].counters
    ));
    f.terminal = Some(PlayerTerminal {
        outcome: spellwake_application::match_history::Outcome::Win,
        winner_seat_ids: vec![f.after.players[0].seat_id],
    });
    let event = f.produce().unwrap().unwrap();
    assert!(matches!(
        event.changes.last(),
        Some(PlayerPublicChange::Terminal { result }) if Some(result) == f.terminal.as_ref()
    ));
}
