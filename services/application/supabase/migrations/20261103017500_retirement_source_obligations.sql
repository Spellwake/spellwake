BEGIN;
-- History is retained. Retirement waits for unfinished source obligations rather
-- than treating a retained rank, participation fact or closed room as a debt.
CREATE FUNCTION app.retirement_match_ready(p_match uuid) RETURNS boolean LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path='' AS $$
DECLARE m app.match; result app.match_result;
BEGIN
 SELECT * INTO m FROM app.match WHERE id=p_match;
 IF m.id IS NULL OR m.accepted_release_id IS NULL OR m.state NOT IN('completed','no_contest') OR m.current_game_id IS NOT NULL OR m.reserved_game_id IS NOT NULL OR NOT EXISTS(SELECT 1 FROM app.command_receipt source WHERE source.scope='room_start_v1' AND source.principal_key='room-start:'||m.room_id::text AND source.target_ref=m.room_id::text AND source.state='accepted' AND source.response_payload->'data'->>'resourceId'=m.id::text) THEN RETURN false;END IF;
 SELECT * INTO result FROM app.match_result WHERE match_id=m.id;
 IF result.id IS NULL THEN RETURN false;END IF;
 IF result.terminal_kind='unrecoverable_incident' THEN
 IF m.terminal_receipt_id IS NULL OR result.terminal_receipt_id IS DISTINCT FROM m.terminal_receipt_id OR NOT EXISTS(SELECT 1 FROM app.command_receipt source WHERE source.id=m.terminal_receipt_id AND source.scope='match_unrecoverable_v1' AND source.state='accepted' AND source.target_ref=m.id::text AND source.response_payload->'data'->>'terminalResultId'=result.id::text) THEN RETURN false;END IF;
 PERFORM app.check_operational_match_terminal(m.id);
 ELSE
 IF result.source_game_result_id IS NULL OR NOT EXISTS(SELECT 1 FROM app.command_receipt source WHERE source.scope='game_outcome_v1' AND source.principal_key='match-lifecycle:'||m.id::text AND source.request_ref=result.source_game_result_id::text AND source.target_ref=result.source_game_result_id::text AND source.state='accepted' AND source.normalized_source->>'matchId'=m.id::text AND source.response_payload->'data'->>'terminalResultId'=result.id::text) THEN RETURN false;END IF;
 END IF;
 IF EXISTS(SELECT 1 FROM app.game_result r JOIN app.game g ON g.id=r.game_id WHERE g.match_id=m.id AND NOT EXISTS(SELECT 1 FROM app.command_receipt source WHERE source.scope='game_outcome_v1' AND source.principal_key='match-lifecycle:'||m.id::text AND source.target_ref=r.id::text AND source.request_ref=r.id::text AND source.state='accepted')) THEN RETURN false;END IF;
 IF m.competitive AND NOT EXISTS(SELECT 1 FROM app.command_receipt source JOIN app.format format ON format.id=m.format_id WHERE source.scope='ranked_match_v1' AND source.principal_key='ranked-format:'||m.format_id::text AND source.target_ref=result.id::text AND source.request_ref=result.id::text AND source.state='accepted' AND source.response_payload->'data'->>'resourceId'=result.id::text AND format.last_ranked_sequence>=result.rank_sequence) THEN RETURN false;END IF;
 IF m.mode='collection' AND EXISTS(SELECT 1 FROM app.game g WHERE g.match_id=m.id AND EXISTS(SELECT 1 FROM app.game_event e WHERE e.game_id=g.id) AND NOT EXISTS(SELECT 1 FROM app.activity_consumer_cursor c WHERE c.game_id=g.id AND c.consumer_key='quests-v1' AND c.initialized AND c.last_sequence>=(SELECT max(e.sequence) FROM app.game_event e WHERE e.game_id=g.id))) THEN RETURN false;END IF;
 RETURN true;
END $$;
CREATE FUNCTION app.account_game_retirement_blocked(p_account uuid) RETURNS boolean LANGUAGE sql STABLE SECURITY DEFINER SET search_path='' AS $$
 SELECT
 EXISTS(SELECT 1 FROM app.match m WHERE EXISTS(SELECT 1 FROM app.match_seat seat WHERE seat.match_id=m.id AND p_account IN(seat.account_id,seat.sponsor_id)) AND NOT app.retirement_match_ready(m.id))
 OR EXISTS(SELECT 1 FROM app.queue_entry WHERE account_id=p_account AND state IN('queued','claimed'))
 OR EXISTS(SELECT 1 FROM app.invitation WHERE issuer_id=p_account AND state='active')
 OR EXISTS(SELECT 1 FROM app.room r WHERE (r.host_id=p_account OR EXISTS(SELECT 1 FROM app.room_slot s WHERE s.room_id=r.id AND s.retired_at IS NULL AND p_account IN(s.account_id,s.sponsor_id))) AND (
 r.state IN('lobby','countdown') OR (r.state='started' AND NOT EXISTS(SELECT 1 FROM app.match m WHERE m.room_id=r.id AND app.retirement_match_ready(m.id)))
 OR (r.state='closed' AND NOT EXISTS(SELECT 1 FROM app.command_receipt source WHERE source.state='accepted' AND ((source.scope='room_leave' AND source.account_id=r.host_id AND source.response_payload->'data'->>'resourceId'=r.id::text) OR (source.scope='queue_cancel' AND EXISTS(SELECT 1 FROM app.queue_entry q WHERE q.claimed_room_id=r.id AND q.id::text=source.target_ref AND q.account_id=source.account_id)))))))
 OR EXISTS(SELECT 1 FROM app.ai_runner_binding b JOIN app.room_slot slot ON slot.id=b.room_slot_id JOIN app.room room ON room.id=slot.room_id WHERE (slot.sponsor_id=p_account OR room.host_id=p_account) AND (b.bootstrap_receipt_id IS NULL OR b.desired_state<>'stopped' OR b.stop_receipt_id IS NULL OR NOT app.ai_stop_confirmed(b.id,b.stop_receipt_id) OR EXISTS(SELECT 1 FROM app.read_ai_stop_work(b.id)) OR EXISTS(SELECT 1 FROM app.controller_grant WHERE binding_id=b.id AND state='active')))
 OR EXISTS(SELECT 1 FROM app.ai_loss_exposure e JOIN app.ai_loss_bucket b ON b.id=e.bucket_id WHERE b.sponsor_id=p_account AND e.state='reserved')
 OR EXISTS(SELECT 1 FROM app.ante_commitment c WHERE EXISTS(SELECT 1 FROM app.match_seat seat WHERE seat.match_id=c.match_id AND p_account IN(seat.account_id,seat.sponsor_id)) AND c.state NOT IN('canceled','settled','refunded'))
 OR EXISTS(SELECT 1 FROM app.rank rank WHERE rank.account_id=p_account AND NOT EXISTS(SELECT 1 FROM app.competitive_result c JOIN app.command_receipt source ON source.scope='ranked_match_v1' AND source.principal_key='ranked-format:'||rank.format_id::text AND source.request_ref=c.match_result_id::text AND source.target_ref=c.match_result_id::text AND source.state='accepted' WHERE c.account_id=rank.account_id AND c.format_id=rank.format_id AND c.season_id=rank.season_id AND c.rating_before IS NOT NULL))
 OR EXISTS(SELECT 1 FROM app.competitive_result c WHERE c.account_id=p_account AND NOT EXISTS(SELECT 1 FROM app.command_receipt source WHERE source.scope='competitive_win_v1' AND source.principal_key='rewards:'||p_account::text AND source.target_ref=c.id::text AND source.request_ref=c.id::text AND source.state='accepted' AND (NOT c.win OR EXISTS(SELECT 1 FROM app.ledger_operation operation WHERE operation.id=source.operation_id AND operation.source_scope='competitive_win_v1' AND operation.source_ref=c.id::text AND operation.state='committed'))))
 OR EXISTS(SELECT 1 FROM app.command_receipt source WHERE source.state='pending' AND (
 (source.scope IN('host_candidate_v1','game_opening_v1','game_replay_v1') AND EXISTS(SELECT 1 FROM app.game g JOIN app.match_seat seat ON seat.match_id=g.match_id WHERE g.id::text=source.target_ref AND p_account IN(seat.account_id,seat.sponsor_id)))
 OR (source.scope='game_outcome_v1' AND EXISTS(SELECT 1 FROM app.game_result r JOIN app.game g ON g.id=r.game_id JOIN app.match_seat seat ON seat.match_id=g.match_id WHERE r.id::text=source.target_ref AND p_account IN(seat.account_id,seat.sponsor_id)))
 OR (source.scope='ranked_match_v1' AND EXISTS(SELECT 1 FROM app.match_result r JOIN app.match_seat seat ON seat.match_id=r.match_id WHERE r.id::text=source.target_ref AND p_account IN(seat.account_id,seat.sponsor_id)))
 OR (source.scope IN('ante_settlement_v1','ante_cancel_v1','ante_accept_v1') AND EXISTS(SELECT 1 FROM app.ante_commitment c JOIN app.match_seat seat ON seat.match_id=c.match_id WHERE (c.id::text=source.target_ref OR c.settlement_result_id::text=source.target_ref) AND p_account IN(seat.account_id,seat.sponsor_id)))
 OR (source.scope LIKE 'ai_%' AND EXISTS(SELECT 1 FROM app.ai_runner_binding b JOIN app.room_slot slot ON slot.id=b.room_slot_id WHERE (b.id::text=source.target_ref OR slot.id::text=source.target_ref OR b.match_seat_id::text=source.target_ref OR EXISTS(SELECT 1 FROM app.controller_grant grant_row WHERE grant_row.binding_id=b.id AND grant_row.id::text=source.target_ref)) AND slot.sponsor_id=p_account))
 ))
$$;
CREATE OR REPLACE FUNCTION app.account_retirement_blocked(p_account uuid,p_owner uuid) RETURNS boolean
LANGUAGE sql STABLE SECURITY DEFINER SET search_path='' AS $$
 SELECT
 EXISTS(SELECT 1 FROM app.trade_offer WHERE p_account IN (sender_id,recipient_id) AND state NOT IN ('accepted','rejected','canceled'))
 OR EXISTS(SELECT 1 FROM app.market_listing WHERE seller_id=p_account AND state NOT IN ('sold','canceled'))
 OR EXISTS(SELECT 1 FROM app.prediction_order WHERE account_id=p_account AND state='open')
 OR EXISTS(SELECT 1 FROM app.prediction_order o JOIN app.prediction_market m ON m.id=o.market_id
  WHERE o.account_id=p_account AND o.filled_quantity>0 AND (m.state<>'resolved' OR o.state<>'settled'
   OR NOT EXISTS(SELECT 1 FROM app.ledger_operation l WHERE l.id=m.settlement_operation_id AND l.state='committed'
    AND l.kind='prediction_settlement' AND l.source_scope='prediction_settlement' AND l.source_ref=m.id::text)))
 OR EXISTS(SELECT 1 FROM app.asset_hold WHERE owner_id=p_owner AND state='held')
 OR EXISTS(SELECT 1 FROM app.ledger_operation WHERE actor_account_id=p_account AND state='pending')
 OR EXISTS(SELECT 1 FROM app.command_receipt WHERE account_id=p_account AND state='pending')
 OR EXISTS(SELECT 1 FROM app.purchase p WHERE p.account_id=p_account AND (
  p.checkout_terms IS NULL OR p.state NOT IN ('paid','refunded','reversed','canceled')
  OR NOT (p.state='canceled' AND p.first_create_attempt_at IS NULL) AND NOT EXISTS(
   SELECT 1 FROM app.provider_event e WHERE e.id=p.provider_event_id AND e.provider='stripe' AND e.account_id=p_account
    AND e.processing_state='processed' AND e.effect->>'purchaseId'=p.id::text AND e.effect->>'disposition'='applied'
    AND e.effect->'result'->>'state'=p.state)))
 OR EXISTS(SELECT 1 FROM app.provider_event e WHERE e.provider='stripe' AND e.processing_state IN ('received','verified')
  AND (e.account_id=p_account OR EXISTS(SELECT 1 FROM app.purchase p WHERE p.account_id=p_account AND p.stripe_checkout_ref=e.capture->>'targetId')))
 -- Provider identity/role minimization needs the actual association producer.
 OR EXISTS(SELECT 1 FROM app.patreon_membership WHERE account_id=p_account)
 OR EXISTS(SELECT 1 FROM app.provider_event WHERE account_id=p_account AND provider='patreon')
 -- Patreon minimization remains gated until its source-preserving path is installed.
 OR app.account_game_retirement_blocked(p_account)
$$;
REVOKE ALL ON FUNCTION app.retirement_match_ready(uuid),app.account_game_retirement_blocked(uuid) FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;
COMMIT;
