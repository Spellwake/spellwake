BEGIN;
-- Immutable original disposition takes precedence over today's game or host lease.
CREATE FUNCTION app.observe_native_clock_job(p_game uuid,p_basis jsonb,p_owner text,p_request uuid) RETURNS jsonb
LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path='' AS $$
DECLARE parent app.command_receipt; child app.command_receipt; deadline timestamptz; seat uuid; cp app.game_checkpoint; response jsonb;
BEGIN
 SELECT * INTO parent FROM app.command_receipt WHERE principal_key='native-system:'||p_game::text
  AND scope='native_host_input_v1' AND request_ref=p_request::text AND target_ref=p_game::text;
 IF FOUND THEN
  SELECT * INTO child FROM app.command_receipt WHERE id::text=parent.normalized_source->>'childReceiptId' AND request_ref=parent.id::text;
  IF parent.state IS DISTINCT FROM 'accepted' OR child.state IS DISTINCT FROM 'accepted' OR child.scope IS DISTINCT FROM 'host_candidate_v1'
   OR parent.normalized_source->'original'->'basis' IS DISTINCT FROM p_basis
   OR parent.normalized_source->'original'->>'kind' IS DISTINCT FROM 'clock'
   OR (jsonb_typeof(child.normalized_source->'candidate'->'bankTimeout')='object' OR child.normalized_source->'candidate'->'nativeEliminations' @> '[{"reason":"native_response_timeout"}]') IS DISTINCT FROM true
  THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='clock request identity conflict'; END IF;
  PERFORM app.check_native_commit_receipt(parent.id);
  RETURN jsonb_build_object('status','accepted','checkpointId',child.response_payload->'data'->'resourceId','revision',child.response_payload->'data'->'revision');
 END IF;
 SELECT c.* INTO cp FROM app.game g JOIN app.match m ON m.id=g.match_id AND m.current_game_id=g.id
 JOIN app.operational_state op ON op.game_id=g.id JOIN app.game_checkpoint c ON c.game_id=g.id AND c.accepted_revision=g.accepted_revision
 JOIN app.command_receipt o ON o.principal_key='native-opening:'||g.id::text AND o.scope='native_opening_v1' AND o.state='accepted'
 WHERE g.id=p_game AND g.state='active' AND m.state='active' AND m.availability='available' AND op.availability='available'
  AND op.owner_instance_ref=p_owner AND p_basis=jsonb_build_object('gameId',g.id,'contextId',c.context_id,
   'epoch',o.normalized_source->>'nativeEpoch','revision',c.accepted_revision::text);
 IF NOT FOUND THEN RETURN '{"status":"inactive"}'; END IF;
 SELECT c.match_seat_id,CASE WHEN c.remaining_ms=0 THEN c.last_accounted_at
  ELSE date_trunc('milliseconds',c.last_accounted_at)+c.remaining_ms*interval '1 millisecond' END INTO seat,deadline
 FROM app.match_clock c JOIN app.match_seat s ON s.id=c.match_seat_id JOIN app.game g ON g.match_id=s.match_id
 WHERE g.id=p_game AND c.eligibility_receipt_id IS NOT NULL AND c.policy_ref='bo3-overall-bank-v1' AND (NOT c.paused OR c.remaining_ms=0)
 ORDER BY 2,s.id LIMIT 1;
 IF FOUND THEN
  IF deadline<=statement_timestamp() THEN RETURN jsonb_build_object('status','bank','seatId',seat); END IF;
  RETURN jsonb_build_object('status','waiting','expiresAt',to_char(deadline AT TIME ZONE 'UTC','YYYY-MM-DD"T"HH24:MI:SS.US"Z"'));
 END IF;
 response:=app.read_native_opening_response_wait(p_game);
 IF response->>'status'='success' AND response->'data'->'due'='false'::jsonb THEN
  RETURN jsonb_build_object('status','waiting','expiresAt',response->'data'->>'expiresAt');
 END IF;
 RETURN '{"status":"response"}';
END $$;

-- A handoff is acknowledged only after the owning operation has durably accepted.
CREATE FUNCTION app.deliver_native_match_handoff(p_handoff uuid) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE h app.durable_handoff; prep jsonb; res jsonb; terminal app.match_result; row_id uuid; ack jsonb; m app.match;
BEGIN
 SELECT * INTO STRICT h FROM app.durable_handoff WHERE id=p_handoff;
 IF (h.destination,h.kind) NOT IN (('game_host','ready_match'),('game_host','reserved_game'),('match_lifecycle','game_outcome'),('match_settlement','match_outcome'),('match_lifecycle','between_deadline'))
 THEN RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid native match delivery'; END IF;
 prep:=app.prepare_handoff_delivery(h.destination,h.id);
 IF h.destination='game_host' THEN
  IF NOT EXISTS(SELECT 1 FROM app.command_receipt WHERE scope='native_opening_v1' AND principal_key='native-opening:'||h.game_id::text
    AND target_ref=h.game_id::text AND state='accepted' AND normalized_source->>'matchId'=h.match_id::text) THEN
   RETURN '{"status":"waiting"}'; END IF;
 ELSIF h.kind='between_deadline' THEN
  -- The durable dispatch is accepted once the generation is answered or superseded.
  -- Active current generations are delivered separately from the due decision page.
  IF EXISTS(SELECT 1 FROM app.between_game_decision d
     WHERE d.id=(prep->'data'->'manifest'->'timer'->>'decisionId')::uuid AND d.state='pending'
      AND d.deadline_source_receipt_id::text=prep->'data'->'manifest'->'timer'->>'sourceReceiptId') THEN
   RETURN '{"status":"waiting"}'; END IF;
 ELSIF h.destination='match_lifecycle' THEN
  SELECT * INTO STRICT m FROM app.match WHERE id=h.match_id;
  res:=app.consume_game_outcome(h.source_ref::uuid,m.revision);
  IF res->>'status' IS DISTINCT FROM 'success' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='game outcome unavailable'; END IF;
  FOR row_id IN SELECT id FROM app.game_participation WHERE game_id=h.game_id ORDER BY account_id LOOP
   PERFORM app.record_monthly_game_activity(row_id);
  END LOOP;
 ELSE
  SELECT * INTO STRICT terminal FROM app.match_result WHERE id=h.source_ref::uuid AND match_id=h.match_id;
  IF terminal.rank_sequence IS NOT NULL THEN
   res:=app.consume_ranked_match_result(terminal.id);
   IF res->>'status' IS DISTINCT FROM 'success' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='rank consumption pending'; END IF;
  END IF;
  FOR row_id IN SELECT id FROM app.ante_commitment WHERE match_id=h.match_id AND state='committed' ORDER BY id LOOP
   res:=app.settle_ante(row_id,terminal.id);
   IF res->>'status' IS DISTINCT FROM 'success' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='ante settlement pending'; END IF;
  END LOOP;
 END IF;
 IF h.state<>'acknowledged' THEN
  ack:=app.acknowledge_handoff_delivery(h.destination,h.id,(prep->'data'->>'preparationId')::uuid,'published','native-delivery:'||h.id::text,h.id);
  IF ack->>'status' IS DISTINCT FROM 'success' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='handoff acknowledgment pending'; END IF;
 END IF;
 RETURN '{"status":"accepted"}';
END $$;

CREATE FUNCTION app.read_native_match_delivery_page(p_owner text,p_lane text,p_after uuid,p_limit integer) RETURNS jsonb
LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path='' AS $$
DECLARE rows jsonb;
BEGIN
 IF p_limit IS NULL OR p_limit NOT BETWEEN 1 AND 100 OR p_owner IS NULL OR btrim(p_owner)='' THEN RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid delivery page'; END IF;
 IF p_lane='handoff' THEN
  SELECT coalesce(jsonb_agg(body ORDER BY id),'[]') INTO rows FROM(
   SELECT h.id,jsonb_build_object('id',h.id,'kind',h.kind,'gameId',h.game_id) body FROM app.durable_handoff h
   WHERE h.state='pending' AND h.kind IN ('ready_match','reserved_game','game_outcome','match_outcome','between_deadline')
    AND (h.destination<>'game_host' OR EXISTS(SELECT 1 FROM app.match m WHERE m.id=h.match_id AND m.structure='bo3'
     AND (SELECT count(*) FROM app.match_seat s WHERE s.match_id=m.id)=2
     AND NOT EXISTS(SELECT 1 FROM app.match_seat s WHERE s.match_id=m.id AND s.controller_kind<>'human')))
    AND (p_after IS NULL OR h.id>p_after) ORDER BY h.id LIMIT p_limit+1) page;
 ELSIF p_lane='between' THEN
  SELECT coalesce(jsonb_agg(body ORDER BY id),'[]') INTO rows FROM(
   SELECT d.id,jsonb_build_object('id',d.id,'matchRevision',m.revision::text,'profileId',m.profile_id,'releaseId',m.accepted_release_id,
    'releaseProof',r.evidence_ref,'selectedRevisionId',coalesce(previous.selected_deck_revision_id,seat.source_revision_id),
    'deadlineRevision',d.deadline_revision::text,'requestId',coalesce(d.deadline_source_receipt_id,d.id)) body
   FROM app.between_game_decision d JOIN app.match m ON m.id=d.match_id JOIN app.match_seat seat ON seat.id=d.seat_id
    JOIN app.qualified_release r ON r.id=m.accepted_release_id JOIN app.game g ON g.id=d.after_game_id
    LEFT JOIN app.game prev ON prev.match_id=m.id AND prev.game_index=g.game_index-1
    LEFT JOIN app.between_game_decision previous ON previous.after_game_id=prev.id AND previous.seat_id=d.seat_id AND previous.state='answered'
   WHERE (p_after IS NULL OR d.id>p_after) AND d.state='pending' AND m.state='between_games' AND m.availability='available'
    AND (d.deadline_policy_ref IS NULL OR (d.deadline_policy_ref='native-between-keep-deck-v1' AND d.deadline_at<=statement_timestamp()))
   ORDER BY d.id LIMIT p_limit+1) page;
 ELSIF p_lane='clock' THEN
  SELECT coalesce(jsonb_agg(body ORDER BY id),'[]') INTO rows FROM(
   SELECT g.id,jsonb_build_object('id',g.id,'requestId',c.id,'basis',jsonb_build_object('gameId',g.id,'contextId',c.context_id,
    'epoch',o.normalized_source->>'nativeEpoch','revision',g.accepted_revision::text)) body
   FROM app.game g JOIN app.match m ON m.id=g.match_id AND m.current_game_id=g.id JOIN app.operational_state op ON op.game_id=g.id
    JOIN app.game_checkpoint c ON c.game_id=g.id AND c.accepted_revision=g.accepted_revision
    JOIN app.command_receipt o ON o.principal_key='native-opening:'||g.id::text AND o.scope='native_opening_v1' AND o.state='accepted'
   WHERE op.owner_instance_ref=p_owner AND op.availability='available' AND g.state='active' AND m.state='active' AND m.availability='available'
    AND (p_after IS NULL OR g.id>p_after) ORDER BY g.id LIMIT p_limit+1) page;
 ELSE RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid delivery lane'; END IF;
 RETURN app.native_bounded_work_page(rows,p_limit);
END $$;
REVOKE ALL ON FUNCTION app.observe_native_clock_job(uuid,jsonb,text,uuid),app.deliver_native_match_handoff(uuid),
 app.read_native_match_delivery_page(text,text,uuid,integer) FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;
GRANT EXECUTE ON FUNCTION app.observe_native_clock_job(uuid,jsonb,text,uuid),app.deliver_native_match_handoff(uuid),
 app.read_native_match_delivery_page(text,text,uuid,integer) TO service_role;
COMMIT;
