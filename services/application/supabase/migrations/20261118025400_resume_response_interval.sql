BEGIN;

CREATE OR REPLACE FUNCTION app.native_opening_wait_window(p_game uuid,p_revision numeric)
RETURNS TABLE(seat_id uuid,decision_ref text,expires_at timestamptz, start_revision numeric)
LANGUAGE sql STABLE SECURITY DEFINER SET search_path='' AS $$
 WITH RECURSIVE waits(rev,seat,proof,expiry,first_rev) AS (
  SELECT 0::numeric,w.seat_id,w.decision_ref,w.expires_at,0::numeric
  FROM app.native_opening_response_wait(p_game) w
  UNION ALL
  SELECT cp.accepted_revision,s.id,cp.private_state_ref,
   CASE WHEN continuation.preserved
    THEN w.expiry ELSE app.financial_instant(child.normalized_source->>'acceptedAt')+interval '120 seconds' END,
   CASE WHEN continuation.preserved THEN w.first_rev ELSE cp.accepted_revision END
  FROM waits w JOIN app.game_checkpoint cp ON cp.game_id=p_game AND cp.accepted_revision=w.rev+1
  JOIN app.command_receipt child ON child.id=cp.source_receipt_id AND child.scope='host_candidate_v1' AND child.state='accepted'
  JOIN app.command_receipt original ON original.id=child.request_ref::uuid AND original.state='accepted'
   AND original.target_ref=p_game::text AND original.normalized_source->>'childReceiptId'=child.id::text
  JOIN app.game g ON g.id=p_game
  JOIN app.match_seat s ON s.match_id=g.match_id AND s.id::text=child.normalized_source->'candidate'->>'openingResponseSeatId'
  CROSS JOIN LATERAL (SELECT s.id=w.seat AND (original.scope='native_host_input_v1'
     OR original.normalized_source->>'actorSeatId'<>w.seat::text) AS preserved) continuation
  WHERE w.rev<p_revision AND (
    (original.scope='native_player_v1' AND (
      (original.normalized_source->'original'->'action'->>'kind'='choose' AND original.normalized_source->>'actorSeatId'=w.seat::text)
      OR (original.normalized_source->'original'->'action'->>'kind'='concede' AND
       (original.normalized_source->>'actorSeatId'=w.seat::text OR s.id=w.seat))))
    OR (original.scope='native_host_input_v1' AND original.normalized_source->'original'->>'kind'='entropy'
        AND s.id=w.seat)
    OR (original.scope='native_host_input_v1' AND original.normalized_source->'original'->>'kind'='clock'
        AND child.normalized_source->'candidate'->'nativeEliminations'->0->>'seatId'=w.seat::text AND s.id<>w.seat))
 )
 SELECT seat,proof,expiry,first_rev FROM waits WHERE rev=p_revision
$$;

CREATE OR REPLACE FUNCTION app.native_response_wait_at(p_game uuid,p_revision numeric)
RETURNS TABLE(seat_id uuid,decision_ref text,expires_at timestamptz)
LANGUAGE sql STABLE SECURITY DEFINER SET search_path='' AS $$
 SELECT w.seat_id,w.decision_ref,w.expires_at FROM app.native_opening_wait_window(p_game,p_revision) w
$$;

-- Each completed incident/restoration pair is immutable. Resolve the clock at
-- the caller's accepted fence so later outages cannot change historical receipts.
CREATE FUNCTION app.native_response_wait_for_fence(p_game uuid,p_revision numeric,p_fence numeric)
RETURNS TABLE(seat_id uuid,decision_ref text,expires_at timestamptz)
LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path='' AS $$
DECLARE w record; span record; reached boolean:=false;
BEGIN
 IF p_fence IS NULL OR p_fence<1 THEN RETURN; END IF;
 SELECT * INTO w FROM app.native_opening_wait_window(p_game,p_revision);
 IF NOT FOUND THEN RETURN; END IF;
 seat_id:=w.seat_id; decision_ref:=w.decision_ref; expires_at:=w.expires_at;
 IF p_fence=1 THEN RETURN NEXT; RETURN; END IF;
 FOR span IN
  SELECT cp.accepted_revision,
   (restored.response_payload->'data'->>'epoch')::numeric AS resume_fence,
   app.financial_instant(incident.normalized_source->>'incidentAt') AS stopped,
   app.financial_instant(restored.normalized_source->>'resumedAt') AS resumed
  FROM app.command_receipt restored JOIN app.command_receipt incident
   ON incident.id::text=restored.normalized_source->>'incidentId'
  JOIN app.game_checkpoint cp ON cp.game_id=p_game
   AND cp.id::text=restored.normalized_source->>'checkpointId'
   AND cp.id::text=incident.normalized_source->>'checkpointId'
  WHERE restored.principal_key='recovery:'||p_game::text AND restored.target_ref=p_game::text
   AND restored.scope='game_restoration_v1' AND restored.state='accepted'
   AND incident.principal_key=restored.principal_key AND incident.target_ref=restored.target_ref
   AND incident.scope='game_incident_v1' AND incident.state='accepted'
   AND restored.normalized_source->>'version'='game-restoration-v1'
   AND incident.normalized_source->>'version'='game-incident-v1'
   AND cp.accepted_revision::text=restored.normalized_source->>'acceptedRevision'
   AND cp.accepted_revision::text=incident.normalized_source->>'acceptedRevision'
   AND cp.accepted_revision<=p_revision
   AND restored.normalized_source->>'epoch'=incident.response_payload->'data'->>'epoch'
   AND restored.normalized_source->>'basis'=incident.response_payload->'data'->>'operationalRevision'
   AND (restored.response_payload->'data'->>'epoch')::numeric<=p_fence
  ORDER BY resume_fence
 LOOP
  IF span.resumed<span.stopped THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='restoration predates trusted incident'; END IF;
  IF span.accepted_revision>=w.start_revision THEN
   expires_at:=span.resumed+greatest(interval '0',expires_at-span.stopped);
  END IF;
  reached:=span.resume_fence=p_fence;
 END LOOP;
 IF reached THEN RETURN NEXT; END IF;
END $$;
REVOKE ALL ON FUNCTION app.native_opening_wait_window(uuid,numeric),app.native_response_wait_for_fence(uuid,numeric,numeric)
 FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;

CREATE OR REPLACE FUNCTION app.read_native_opening_response_wait(p_game uuid) RETURNS jsonb
LANGUAGE sql STABLE SECURITY DEFINER SET search_path='' AS $$
 SELECT coalesce((SELECT jsonb_build_object('status','success','data',jsonb_build_object(
   'seatId',w.seat_id,'decisionRef',w.decision_ref,'policy','multiplayer-response-v1',
   'expiresAt',to_char(w.expires_at AT TIME ZONE 'UTC','YYYY-MM-DD"T"HH24:MI:SS.US"Z"'),
   'due',statement_timestamp()>=w.expires_at))
 FROM app.game g JOIN app.match m ON m.id=g.match_id
 JOIN app.operational_state op ON op.game_id=g.id
 CROSS JOIN LATERAL app.native_response_wait_for_fence(p_game,g.accepted_revision,op.fence_epoch) w
 WHERE g.id=p_game AND g.state='active' AND m.current_game_id=g.id
   AND m.state='active' AND m.availability='available' AND op.availability='available'
   ),
   jsonb_build_object('status','fail','data',jsonb_build_object('game','unavailable')))
$$;

CREATE OR REPLACE FUNCTION app.guard_native_response_deadline() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE src jsonb:=NEW.normalized_source; frame jsonb:=src->'prepared'->'candidate';
 item jsonb; timing record; survivors uuid[]; child app.command_receipt; g uuid:=NEW.target_ref::uuid;
BEGIN
 IF NOT (frame ? 'nativeEliminations') OR frame->'nativeEliminations' IN ('null'::jsonb,'[]'::jsonb) THEN RETURN NEW; END IF;
 IF src->'original'->>'kind' IS DISTINCT FROM 'clock'
  OR jsonb_array_length(frame->'nativeEliminations')<>1
  OR (frame->'bankTimeout' IS NOT NULL AND frame->'bankTimeout'<>'null'::jsonb) THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='multiplayer response requires a supported native wait';
 END IF;
 SELECT * INTO timing FROM app.native_response_wait_for_fence(g,(src->'original'->'basis'->>'revision')::numeric,(src->'prepared'->>'fenceEpoch')::numeric);
 IF NOT FOUND OR NOT EXISTS(SELECT 1 FROM app.operational_state WHERE game_id=g AND availability='available' AND fence_epoch=(src->'prepared'->>'fenceEpoch')::numeric) THEN
  RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='multiplayer opening interval unavailable';
 END IF;
 item:=frame->'nativeEliminations'->0;
 SELECT * INTO STRICT child FROM app.command_receipt WHERE id=(src->>'childReceiptId')::uuid;
 survivors:=app.native_opening_other_survivors(g,(src->'original'->'basis'->>'revision')::numeric,timing.seat_id);
 IF cardinality(survivors)=0 OR
   (cardinality(survivors)=1 AND (frame->'terminal'->>'reason' IS DISTINCT FROM 'native_response_timeout'
    OR frame->'terminal'->>'outcome' IS DISTINCT FROM 'win'
    OR frame->'terminal'->'winnerSeatIds' IS DISTINCT FROM to_jsonb(survivors)))
   OR (cardinality(survivors)>1 AND frame->>'terminal' IS NOT NULL) THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='opening timeout must preserve its remaining seats or award the last survivor';END IF;
 IF item->>'sourceRevision' IS DISTINCT FROM src->'original'->'basis'->>'revision' OR item->>'reason' IS DISTINCT FROM 'native_response_timeout'
  OR item->>'seatId' IS DISTINCT FROM timing.seat_id::text
  OR item->>'decisionRef' IS DISTINCT FROM ('multiplayer-response-v1:'||timing.decision_ref)
  OR item->>'expiryRef' IS DISTINCT FROM src->'original'->>'inputRef'
  OR app.financial_instant(item->>'expiredAt') IS DISTINCT FROM timing.expires_at
  OR app.financial_instant(child.normalized_source->>'acceptedAt') < timing.expires_at THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='multiplayer timeout does not match the due opening interval';
 END IF;
 RETURN NEW;
END $$;

CREATE OR REPLACE FUNCTION app.read_native_transition_sources(p_game uuid,p_context uuid,p_anchor_checkpoint uuid,p_after_revision numeric,p_limit integer) RETURNS jsonb
LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path='' AS $$
DECLARE source jsonb;anchor app.game_checkpoint;checkpoint_row app.game_checkpoint;
 creator app.command_receipt;timing record;response_evidence jsonb;item jsonb;native_parent app.command_receipt;bank_evidence jsonb;before_bank jsonb;body jsonb;marker jsonb;events jsonb;groups jsonb:='[]'::jsonb;
 last_revision numeric;last_sequence numeric;next_revision numeric;result jsonb;
 unavailable constant jsonb:='{"status":"fail","data":{"game":"unavailable"}}'::jsonb;
BEGIN
 IF p_game IS NULL OR p_context IS NULL OR p_anchor_checkpoint IS NULL
  OR '00000000-0000-0000-0000-000000000000'::uuid IN(p_game,p_context,p_anchor_checkpoint)
  OR p_limit IS NULL OR p_limit NOT BETWEEN 1 AND 32
  OR (p_after_revision IS NOT NULL AND p_after_revision::text !~ '^(0|[1-9][0-9]{0,77})$') THEN
  RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='bounded anchored native history request required';END IF;
 source:=app.read_native_game_source(p_game);
 IF source->>'status' IS DISTINCT FROM 'success' OR source->'data'->>'contextId' IS DISTINCT FROM p_context::text THEN RETURN unavailable;END IF;
 SELECT * INTO anchor FROM app.game_checkpoint WHERE id=p_anchor_checkpoint AND game_id=p_game AND context_id=p_context
  AND accepted_revision<=(source->'data'->>'acceptedRevision')::numeric AND source_receipt_id IS NOT NULL;
 IF anchor.id IS NULL THEN RETURN unavailable;END IF;
 PERFORM app.check_native_creator_frame(anchor.source_receipt_id);
 IF p_after_revision IS NOT NULL AND (p_after_revision>anchor.accepted_revision OR NOT EXISTS(
  SELECT 1 FROM app.game_checkpoint WHERE game_id=p_game AND context_id=p_context AND accepted_revision=p_after_revision)) THEN
  RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='native history cursor is outside its anchor';END IF;
 SELECT max(sequence) INTO last_sequence FROM app.game_event WHERE game_id=p_game AND context_id=p_context AND accepted_revision<=anchor.accepted_revision;
 IF last_sequence IS NULL THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='native history anchor has no event prefix';END IF;
 FOR checkpoint_row IN SELECT * FROM app.game_checkpoint WHERE game_id=p_game AND context_id=p_context
   AND accepted_revision<=anchor.accepted_revision AND (p_after_revision IS NULL OR accepted_revision>p_after_revision)
   ORDER BY accepted_revision LIMIT p_limit LOOP
  PERFORM app.check_native_creator_frame(checkpoint_row.source_receipt_id);
  SELECT * INTO STRICT creator FROM app.command_receipt WHERE id=checkpoint_row.source_receipt_id;
  body:=CASE WHEN creator.scope='game_opening_v1' THEN creator.normalized_source ELSE creator.normalized_source->'candidate' END;
  -- check_native_creator_frame rejects explicit null/unknown raw markers before
  -- ->> projects historical absence to the nullable inspection representation.
  marker:=CASE WHEN body ? 'eventSchemaRevision' THEN body->'eventSchemaRevision' ELSE 'null'::jsonb END;
  bank_evidence:=NULL;
  -- Only the maintained new cause claims bank-input semantics. Historical
  -- generic timeout groups retain their exact previous projection.
  IF body->'terminal'->>'reason'='native_bank_expired' THEN
   IF creator.scope<>'host_candidate_v1' OR creator.state<>'accepted'
    OR body->'bankTimeout' IS NULL OR body->'bankTimeout'='null'::jsonb THEN
    RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='native bank history requires its timeout creator';END IF;
   SELECT * INTO native_parent FROM app.command_receipt
    WHERE scope='native_host_input_v1' AND state='accepted'
     AND normalized_source->>'childReceiptId'=creator.id::text;
   -- check_native_commit_receipt deliberately ignores absent/other families;
   -- establish the exact accepted host parent before invoking that checker.
   IF native_parent.id IS NULL OR native_parent.normalized_source->'original'->>'kind' IS DISTINCT FROM 'clock'
    OR native_parent.normalized_source->'original'->>'inputRef' IS DISTINCT FROM body->'bankTimeout'->>'proofRef' THEN
    RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='native bank history requires exact clock original and input proof';END IF;
   PERFORM app.check_native_commit_receipt(native_parent.id);
   PERFORM app.check_bank_forfeit_game(p_game);
   SELECT value INTO before_bank FROM jsonb_array_elements(creator.normalized_source->'before') value
    WHERE value->>'seatId'=body->'bankTimeout'->>'seatId';
   IF before_bank IS NULL OR before_bank->>'clockRevision' IS DISTINCT FROM body->'bankTimeout'->>'clockRevision' THEN
    RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='native bank history requires its immutable clock basis';END IF;
   bank_evidence:=jsonb_build_object('originalRequestId',native_parent.request_ref,
    'original',native_parent.normalized_source->'original','timeout',body->'bankTimeout',
    'beforeClock',before_bank,'acceptedAt',creator.normalized_source->'acceptedAt');
  END IF;

  response_evidence:=NULL;
  item:=body->'nativeEliminations'->0;
  IF starts_with(item->>'decisionRef','multiplayer-response-v1:') THEN
   SELECT * INTO STRICT timing FROM app.native_response_wait_for_fence(p_game,checkpoint_row.accepted_revision-1,(creator.normalized_source->>'epoch')::numeric);
   SELECT * INTO STRICT native_parent FROM app.command_receipt
    WHERE scope='native_host_input_v1' AND state='accepted' AND normalized_source->>'childReceiptId'=creator.id::text;
   PERFORM app.check_native_commit_receipt(native_parent.id);
   IF native_parent.normalized_source->'original'->'basis'->>'revision' IS DISTINCT FROM (checkpoint_row.accepted_revision-1)::text OR item->>'sourceRevision' IS DISTINCT FROM (checkpoint_row.accepted_revision-1)::text OR jsonb_array_length(body->'nativeEliminations')<>1
     OR native_parent.normalized_source->'original'->>'kind' IS DISTINCT FROM 'clock'
     OR item->>'seatId' IS DISTINCT FROM timing.seat_id::text
     OR item->>'decisionRef' IS DISTINCT FROM ('multiplayer-response-v1:'||timing.decision_ref)
     OR item->>'expiryRef' IS DISTINCT FROM native_parent.normalized_source->'original'->>'inputRef'
     OR app.financial_instant(item->>'expiredAt') IS DISTINCT FROM timing.expires_at
     OR app.financial_instant(creator.normalized_source->>'acceptedAt')<timing.expires_at THEN
    RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='multiplayer opening timeout history differs';
   END IF;
   response_evidence:=jsonb_build_object('originalRequestId',native_parent.request_ref,'original',native_parent.normalized_source->'original',
     'seatId',timing.seat_id,'decisionRef',timing.decision_ref,'expiresAt',item->>'expiredAt','acceptedAt',creator.normalized_source->>'acceptedAt');
  END IF;
  SELECT coalesce(jsonb_agg(jsonb_build_object(
    'eventId',e.id,'sequence',e.sequence::text,'acceptedRevision',e.accepted_revision::text,
    'privatePayloadRef',e.private_payload_ref,'acceptedAt',to_char(e.accepted_at AT TIME ZONE 'UTC','YYYY-MM-DD"T"HH24:MI:SS.US"Z"'),
    'creatorReceiptId',e.source_receipt_id,'facts',(
      SELECT coalesce(jsonb_agg(jsonb_build_object('accountId',f.account_id,'kind',f.kind,'factRevision',f.fact_revision,'value',f.value::text)
        ORDER BY f.account_id,f.kind,f.fact_revision,f.id),'[]'::jsonb)
      FROM app.game_activity_fact f WHERE f.source_event_id=e.id)) ORDER BY e.sequence),'[]'::jsonb)
    INTO events FROM app.game_event e WHERE e.game_id=p_game AND e.context_id=p_context AND e.source_receipt_id=creator.id;
  IF jsonb_array_length(events)=0 THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='native history checkpoint has no exact creator events';END IF;
  groups:=groups||jsonb_build_array(jsonb_build_object('checkpointId',checkpoint_row.id,'acceptedRevision',checkpoint_row.accepted_revision::text,
    'stateRef',checkpoint_row.private_state_ref,'factsRef',checkpoint_row.facts_ref,'creatorReceiptId',creator.id,
    'eventSchemaRevision',marker,'events',events)||CASE WHEN body ? 'openingResponseSeatId' THEN jsonb_build_object('openingResponseSeatId',body->'openingResponseSeatId') ELSE '{}'::jsonb END||CASE WHEN bank_evidence IS NULL THEN '{}'::jsonb ELSE jsonb_build_object('bankExpiry',bank_evidence) END||CASE WHEN response_evidence IS NULL THEN '{}'::jsonb ELSE jsonb_build_object('openingResponseExpiry',response_evidence) END);
  last_revision:=checkpoint_row.accepted_revision;
 END LOOP;
 IF last_revision IS NOT NULL AND EXISTS(SELECT 1 FROM app.game_checkpoint WHERE game_id=p_game AND context_id=p_context
  AND accepted_revision>last_revision AND accepted_revision<=anchor.accepted_revision) THEN next_revision:=last_revision;END IF;
 result:=jsonb_build_object('status','success','data',jsonb_build_object('anchor',jsonb_build_object(
  'gameId',p_game,'contextId',p_context,'anchorCheckpointId',anchor.id,'anchorStateRef',anchor.private_state_ref,
  'anchorCreatorReceiptId',anchor.source_receipt_id,'throughAcceptedRevision',anchor.accepted_revision::text,'throughEventSequence',last_sequence::text),
  'groups',groups,'nextAfterRevision',next_revision::text));
 IF octet_length(result::text)>8388608 THEN RAISE EXCEPTION USING ERRCODE='54000',MESSAGE='native history response exceeds bounded source budget';END IF;
 RETURN result;
END $$;

CREATE OR REPLACE FUNCTION app.submit_native_player_command(p_subject uuid, p_session uuid, p_grant uuid, p_epoch numeric, p_secret_ref text, p_original jsonb, p_prepared jsonb)
 RETURNS jsonb
 LANGUAGE plpgsql
 SECURITY DEFINER
 SET search_path TO ''
AS $function$
DECLARE player app.account;actor jsonb;parent app.command_receipt;child app.command_receipt;
 native_game_id uuid;native_match_id uuid;native_seat_id uuid;source jsonb;basis_state jsonb;view_receipt app.command_receipt;
 expired_choice jsonb;
 canonical jsonb;ack jsonb;checkpoint_id uuid;normalized jsonb;prepared_valid boolean;is_accept boolean;is_human boolean:=p_subject IS NOT NULL;
BEGIN
 IF current_setting('transaction_isolation')<>'read committed' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='native player commit requires read committed retry';END IF;
 PERFORM pg_advisory_xact_lock_shared(hashtextextended('supply-population-cutoff-v1',0));
 IF is_human THEN
  IF p_grant IS NOT NULL OR p_epoch IS NOT NULL OR p_secret_ref IS NOT NULL THEN RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='mixed native authority';END IF;
  player:=app.lock_session_account(p_subject,p_session);
 ELSIF p_session IS NOT NULL OR p_grant IS NULL THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('actor','unavailable'));END IF;
 IF NOT app.native_player_original_is_valid(p_original) THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('request','invalid'));END IF;
 native_game_id:=(p_original->'basis'->>'gameId')::uuid;
 SELECT g.match_id INTO native_match_id FROM app.game g WHERE g.id=native_game_id;
 IF is_human THEN
  SELECT jsonb_build_object('seatId',seat.id,'kind','human','accountId',player.id,'grantId',NULL,'bindingId',NULL,'bindingEpoch',NULL) INTO actor
   FROM app.match_seat seat WHERE seat.match_id=native_match_id AND seat.account_id=player.id AND seat.controller_kind='human';
 ELSE actor:=app.lock_native_controller_actor(p_grant,p_epoch,p_secret_ref,native_match_id);END IF;
 IF actor IS NULL THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('actor','unavailable'));END IF;
 native_seat_id:=(actor->>'seatId')::uuid;
 parent:=app.begin_native_source_receipt('native-seat:'||native_seat_id::text,p_original->>'requestId','native_player_v1',native_game_id,
  jsonb_build_object('actorSeatId',native_seat_id,'original',p_original),player.id,p_grant);
 -- This lookup precedes current basis, host fence, new handles and preparation.
 IF parent.state<>'pending' THEN RETURN parent.response_payload;END IF;
 prepared_valid:=app.native_player_prepared_is_valid(p_prepared,p_original,native_seat_id);
 is_accept:=prepared_valid AND p_prepared->>'version'='native-player-accept-v1';
 IF is_accept AND (p_prepared->'candidate'->'eventSchemaRevision' IS DISTINCT FROM '"native-transition-v1"'::jsonb AND p_prepared->'candidate'->'eventSchemaRevision' IS DISTINCT FROM '"native-transition-activity-v1"'::jsonb AND p_prepared->'candidate'->'eventSchemaRevision' IS DISTINCT FROM '"native-transition-land-play-v1"'::jsonb) THEN
  RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='new native player acceptance requires transition event schema';END IF;
 BEGIN
  IF is_accept THEN
   child:=app.claim_accept_host_candidate(native_game_id,p_prepared->>'owner',(p_prepared->>'fenceEpoch')::numeric,
    (p_original->'basis'->>'revision')::numeric,parent.id,p_prepared->'candidate');
   IF child.state<>'pending' THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='new native original has preexisting canonical child';END IF;
  END IF;
  source:=app.lock_native_game_source(native_game_id);
  PERFORM app.recheck_native_player_actor(p_subject,p_session,p_grant,p_epoch,p_secret_ref,actor,native_match_id);
  IF is_accept AND(p_prepared->>'owner' IS DISTINCT FROM source->'native'->'operational'->>'ownerRef'
   OR p_prepared->>'fenceEpoch' IS DISTINCT FROM source->'native'->'operational'->>'fenceEpoch') THEN
   RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='native prepared operational fence lost';END IF;
  basis_state:=app.native_original_basis_state(p_original,source);
  IF basis_state->>'state'='stale' THEN RAISE EXCEPTION USING ERRCODE='SW003',MESSAGE='verified stale native original';END IF;
  IF NOT prepared_valid THEN RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='current native original requires valid trusted preparation';END IF;
  IF p_prepared->>'runtimeId' IS DISTINCT FROM source->'opening'->>'runtimeId'
   OR p_prepared->>'programSourceId' IS DISTINCT FROM source->'opening'->>'programSourceId' THEN
   RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='native preparation has different original runtime source';END IF;
  SELECT * INTO view_receipt FROM app.command_receipt WHERE id=(p_prepared->>'viewReceiptId')::uuid AND scope='native_view_v1' AND state='accepted';
  IF view_receipt.id IS NULL OR view_receipt.target_ref IS DISTINCT FROM native_game_id::text
   OR view_receipt.normalized_source->'basis' IS DISTINCT FROM p_original->'basis'
   OR view_receipt.normalized_source->>'viewerSeatId' IS DISTINCT FROM native_seat_id::text
   OR view_receipt.normalized_source->>'disclosure' IS DISTINCT FROM 'live' THEN
   RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='native preparation lacks own exact issued View';END IF;
  PERFORM app.check_native_view_receipt(view_receipt.id);
  normalized:=jsonb_build_object('version','native-player-v1','actorSeatId',native_seat_id,'original',p_original,
   'prepared',p_prepared,'openingReceiptId',source->'opening'->>'receiptId','childReceiptId',NULL,'acceptedCheckpointId',NULL);
  IF NOT is_accept THEN
   UPDATE app.command_receipt SET normalized_source=normalized WHERE id=parent.id;
   ack:=app.finish_account_command(parent.id,false,p_prepared->'errors');
  ELSE
   IF p_original->'basis'->>'revision'='18446744073709551615' THEN RAISE EXCEPTION USING ERRCODE='22003',MESSAGE='native revision exhausted';END IF;
   IF p_prepared->'candidate'->>'eventSchemaRevision'='native-transition-land-play-v1' THEN
    UPDATE app.command_receipt SET normalized_source=normalized WHERE id=parent.id;
   END IF;
   canonical:=app.apply_accept_host_candidate(child.id,native_game_id,p_prepared->>'owner',(p_prepared->>'fenceEpoch')::numeric,
    (p_original->'basis'->>'revision')::numeric,parent.id,p_prepared->'candidate');
   IF canonical->>'status' IS DISTINCT FROM 'success' THEN
    IF canonical->'data' ? 'basis' THEN
     basis_state:=app.native_original_basis_state(p_original,app.lock_native_game_source(native_game_id));
     IF basis_state->>'state'='stale' THEN RAISE EXCEPTION USING ERRCODE='SW003',MESSAGE='verified stale native original';END IF;
     RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='canonical basis failure disagrees with locked native source';
    ELSIF canonical->'data' ?| ARRAY['game','context','availability','fence','clocks'] THEN
     RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='native operational candidate not accepted';
    ELSE RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='unexpected canonical native rejection';END IF;
   END IF;
   -- Use the same locked database instant as the tentative checkpoint. An
   -- expired reply rolls back its child and every resulting game mutation.
   IF p_original->'action'->>'kind'='choose' THEN
    SELECT jsonb_build_object('kind','expired-response','decisionRef',w.decision_ref,
      'expiresAt',to_char(w.expires_at AT TIME ZONE 'UTC','YYYY-MM-DD"T"HH24:MI:SS.US"Z"'),
      'observedAt',h.normalized_source->>'acceptedAt','viewReceiptId',view_receipt.id)
      || CASE WHEN op.restoration_receipt_id IS NULL THEN '{}'::jsonb ELSE jsonb_build_object('restorationId',op.restoration_receipt_id) END
    INTO expired_choice
    FROM app.operational_state op CROSS JOIN LATERAL app.native_response_wait_for_fence(native_game_id,
      (p_original->'basis'->>'revision')::numeric,op.fence_epoch) w
    JOIN app.command_receipt h ON h.id=child.id AND h.state='accepted'
    WHERE op.game_id=native_game_id AND op.availability='available' AND w.seat_id=native_seat_id AND app.financial_instant(h.normalized_source->>'acceptedAt')>=w.expires_at;
    IF FOUND THEN RAISE EXCEPTION USING ERRCODE='SW004',MESSAGE='native response interval elapsed'; END IF;
   END IF;
   checkpoint_id:=(canonical->'data'->>'resourceId')::uuid;
   normalized:=normalized||jsonb_build_object('childReceiptId',child.id,'acceptedCheckpointId',checkpoint_id);
   UPDATE app.command_receipt SET normalized_source=normalized WHERE id=parent.id;
   ack:=app.finish_account_command(parent.id,true,jsonb_build_object('requestId',p_original->>'requestId','actorSeatId',native_seat_id,
    'gameId',native_game_id,'contextId',p_original->'basis'->>'contextId','epoch',p_original->'basis'->>'epoch','revision',canonical->'data'->>'revision'));
  END IF;
  PERFORM app.recheck_native_player_actor(p_subject,p_session,p_grant,p_epoch,p_secret_ref,actor,native_match_id);
 EXCEPTION WHEN SQLSTATE 'SW004' THEN
  IF expired_choice IS NULL THEN RAISE;END IF;
  source:=app.lock_native_game_source(native_game_id);
  PERFORM app.recheck_native_player_actor(p_subject,p_session,p_grant,p_epoch,p_secret_ref,actor,native_match_id);
  UPDATE app.command_receipt SET normalized_source=jsonb_build_object('version','native-player-v1',
    'actorSeatId',native_seat_id,'original',p_original,'prepared',NULL,
    'openingReceiptId',source->'opening'->>'receiptId','childReceiptId',NULL,
    'acceptedCheckpointId',NULL,'rejectionSource',expired_choice) WHERE id=parent.id;
  ack:=app.finish_account_command(parent.id,false,jsonb_build_object('decision','stale'));
 WHEN SQLSTATE 'SW003' THEN
  -- PL/pgSQL variables survive rollback; never reuse a rolled-back child, frame,
  -- response or source. Parent/account/controller scope predates this savepoint.
  child:=NULL;source:=NULL;basis_state:=NULL;view_receipt:=NULL;canonical:=NULL;ack:=NULL;checkpoint_id:=NULL;normalized:=NULL;
  source:=app.lock_native_game_source(native_game_id);
  PERFORM app.recheck_native_player_actor(p_subject,p_session,p_grant,p_epoch,p_secret_ref,actor,native_match_id);
  IF is_accept AND(p_prepared->>'owner' IS DISTINCT FROM source->'native'->'operational'->>'ownerRef'
   OR p_prepared->>'fenceEpoch' IS DISTINCT FROM source->'native'->'operational'->>'fenceEpoch') THEN
   RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='native prepared operational fence lost';END IF;
  basis_state:=app.native_original_basis_state(p_original,source);
  IF basis_state->>'state' IS DISTINCT FROM 'stale' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='native stale observation changed before rejection fence';END IF;
  normalized:=jsonb_build_object('version','native-player-v1','actorSeatId',native_seat_id,'original',p_original,'prepared',NULL,
   'openingReceiptId',source->'opening'->>'receiptId','childReceiptId',NULL,'acceptedCheckpointId',NULL,'rejectionSource',basis_state->'rejectionSource');
  UPDATE app.command_receipt SET normalized_source=normalized WHERE id=parent.id;
  ack:=app.finish_account_command(parent.id,false,jsonb_build_object('basis','stale'));
 END;
 RETURN ack;
EXCEPTION WHEN invalid_authorization_specification THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object(CASE WHEN is_human THEN 'session' ELSE 'actor' END,CASE WHEN is_human THEN 'unauthorized' ELSE 'unavailable' END));
WHEN SQLSTATE 'SW002' THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('game','unavailable'));
WHEN check_violation THEN IF SQLERRM='request identity was used for different command terms' THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('request','conflict'));END IF;RAISE;
END $function$;

CREATE OR REPLACE FUNCTION app.check_native_commit_receipt(p_receipt uuid) RETURNS void
LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path='' AS $$
DECLARE parent app.command_receipt;child app.command_receipt;opening app.command_receipt;checkpoint app.game_checkpoint;
 old_checkpoint app.game_checkpoint;observed_checkpoint app.game_checkpoint;view_receipt app.command_receipt;
 seat app.match_seat;game_row app.game;source jsonb;original jsonb;prepared jsonb;expected jsonb;terms jsonb;
 basis jsonb;rejection jsonb;observed jsonb;actor_id uuid;is_player boolean;native_game_id uuid;native_context_id uuid;next_revision numeric;
BEGIN
 SELECT * INTO parent FROM app.command_receipt WHERE id=p_receipt AND scope IN('native_player_v1','native_host_input_v1');
 IF parent.id IS NULL THEN RETURN;END IF;
 source:=parent.normalized_source;original:=source->'original';prepared:=source->'prepared';is_player:=parent.scope='native_player_v1';
 IF parent.state='pending' OR jsonb_typeof(source) IS DISTINCT FROM 'object'
  OR(is_player AND(NOT app.native_player_original_is_valid(original) OR source->>'version' IS DISTINCT FROM 'native-player-v1'))
  OR(NOT is_player AND(NOT app.native_host_original_is_valid(original) OR source->>'version' IS DISTINCT FROM 'native-host-commit-v1')) THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='native original lacks complete terminal source';END IF;
 basis:=original->'basis';native_game_id:=(basis->>'gameId')::uuid;native_context_id:=(basis->>'contextId')::uuid;
 SELECT * INTO game_row FROM app.game WHERE id=native_game_id;
 SELECT * INTO opening FROM app.command_receipt WHERE id=(source->>'openingReceiptId')::uuid AND scope='native_opening_v1' AND state='accepted';
 IF game_row.id IS NULL OR opening.id IS NULL OR opening.target_ref IS DISTINCT FROM native_game_id::text
  OR opening.normalized_source->'opening'->>'contextId' IS DISTINCT FROM native_context_id::text
  OR opening.normalized_source->>'nativeEpoch' IS DISTINCT FROM basis->>'epoch' OR parent.target_ref IS DISTINCT FROM native_game_id::text THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='native original has different opening lineage';END IF;
 PERFORM app.check_native_opening_receipt(opening.id);
 expected:=jsonb_build_object('version',CASE WHEN is_player THEN 'native-player-v1' ELSE 'native-host-commit-v1' END,
  'original',original,'prepared',prepared,'openingReceiptId',opening.id,'childReceiptId',source->'childReceiptId','acceptedCheckpointId',source->'acceptedCheckpointId');
 IF is_player THEN
  IF (parent.account_id IS NULL)=(parent.controller_grant_id IS NULL) THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='native original requires one retained actor owner';END IF;
  IF NOT app.native_uuid_is_canonical(source->>'actorSeatId') THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='native original actor absent';END IF;
  actor_id:=(source->>'actorSeatId')::uuid;
  SELECT * INTO seat FROM app.match_seat WHERE id=actor_id AND match_id=game_row.match_id;
  IF seat.id IS NULL OR seat.deck_accepted_at IS NULL OR parent.principal_key IS DISTINCT FROM 'native-seat:'||actor_id::text
   OR parent.request_ref IS DISTINCT FROM original->>'requestId'
   OR parent.payload_hash IS DISTINCT FROM app.auth_json_hash(jsonb_build_object('actorSeatId',actor_id,'original',original))
   OR NOT((seat.controller_kind='human' AND parent.account_id=seat.account_id AND parent.controller_grant_id IS NULL)
    OR(seat.controller_kind='ai' AND parent.account_id IS NULL AND parent.controller_grant_id IS NOT NULL
     AND EXISTS(SELECT 1 FROM app.controller_grant WHERE id=parent.controller_grant_id AND match_seat_id=seat.id))) THEN
   RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='native original has different retained actor';END IF;
  expected:=expected||jsonb_build_object('actorSeatId',actor_id);
 ELSE
  IF parent.principal_key IS DISTINCT FROM 'native-system:'||native_game_id::text OR NOT app.native_uuid_is_canonical(parent.request_ref)
   OR parent.account_id IS NOT NULL OR parent.controller_grant_id IS NOT NULL OR parent.payload_hash IS DISTINCT FROM app.auth_json_hash(original)
   OR original->>'runtimeId' IS DISTINCT FROM opening.normalized_source->>'runtimeId'
   OR original->>'programSourceId' IS DISTINCT FROM opening.normalized_source->>'programSourceId' THEN
   RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='native system original has different retained source';END IF;
 END IF;
 IF parent.state='accepted' THEN
  IF jsonb_typeof(prepared) IS DISTINCT FROM 'object'
   OR NOT app.native_uuid_is_canonical(source->>'childReceiptId') OR NOT app.native_uuid_is_canonical(source->>'acceptedCheckpointId') THEN
   RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='accepted native original lacks canonical frame';END IF;
  IF is_player THEN
   IF NOT app.native_player_prepared_is_valid(prepared,original,actor_id) OR prepared->>'version' IS DISTINCT FROM 'native-player-accept-v1'
    OR prepared->>'runtimeId' IS DISTINCT FROM opening.normalized_source->>'runtimeId'
    OR prepared->>'programSourceId' IS DISTINCT FROM opening.normalized_source->>'programSourceId' THEN
    RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='accepted native original preparation differs';END IF;
  ELSE
   IF EXISTS(SELECT 1 FROM jsonb_object_keys(prepared) k WHERE k NOT IN('owner','fenceEpoch','candidate'))
    OR jsonb_typeof(prepared->'owner') IS DISTINCT FROM 'string' OR length(btrim(prepared->>'owner')) NOT BETWEEN 1 AND 256
    OR jsonb_typeof(prepared->'fenceEpoch') IS DISTINCT FROM 'string' OR prepared->>'fenceEpoch' !~ '^(0|[1-9][0-9]{0,77})$'
    OR NOT app.native_candidate_refs_are_valid(prepared->'candidate',native_game_id,native_context_id) THEN
    RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='accepted native system preparation differs';END IF;
  END IF;
  SELECT * INTO child FROM app.command_receipt WHERE id=(source->>'childReceiptId')::uuid;
  SELECT * INTO checkpoint FROM app.game_checkpoint WHERE id=(source->>'acceptedCheckpointId')::uuid;
  next_revision:=(basis->>'revision')::numeric+1;
  terms:=jsonb_build_object('owner',prepared->>'owner','epoch',prepared->>'fenceEpoch','basis',basis->>'revision','candidate',prepared->'candidate');
  IF NOT app.native_u64_is_canonical(next_revision::text) OR child.id IS NULL OR checkpoint.id IS NULL
   OR child.scope<>'host_candidate_v1' OR child.state<>'accepted' OR child.principal_key IS DISTINCT FROM 'host:'||native_game_id::text
   OR child.target_ref IS DISTINCT FROM native_game_id::text OR child.request_ref IS DISTINCT FROM parent.id::text
   OR child.payload_hash IS DISTINCT FROM app.auth_json_hash(terms)
   OR child.normalized_source->>'version' IS DISTINCT FROM 'host-clock-transition-v1'
   OR child.normalized_source->>'basisRevision' IS DISTINCT FROM basis->>'revision'
   OR child.normalized_source->>'acceptedRevision' IS DISTINCT FROM next_revision::text
   OR child.normalized_source->'candidate' IS DISTINCT FROM prepared->'candidate'
   OR checkpoint.game_id IS DISTINCT FROM native_game_id OR checkpoint.context_id IS DISTINCT FROM native_context_id
   OR checkpoint.accepted_revision IS DISTINCT FROM next_revision OR checkpoint.source_receipt_id IS DISTINCT FROM child.id
   OR checkpoint.private_state_ref IS DISTINCT FROM prepared->'candidate'->>'stateRef'
   OR checkpoint.facts_ref IS DISTINCT FROM prepared->'candidate'->>'factsRef'
   OR child.response_payload IS DISTINCT FROM jsonb_build_object('status','success','data',jsonb_build_object('resourceId',checkpoint.id,'revision',next_revision::text)) THEN
   RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='native original and canonical frame are not reciprocal';END IF;
  PERFORM app.check_native_creator_frame(child.id);
  IF is_player THEN
   SELECT * INTO view_receipt FROM app.command_receipt WHERE id=(prepared->>'viewReceiptId')::uuid AND scope='native_view_v1' AND state='accepted';
   IF view_receipt.id IS NULL OR view_receipt.target_ref IS DISTINCT FROM native_game_id::text
    OR view_receipt.normalized_source->'basis' IS DISTINCT FROM basis OR view_receipt.normalized_source->>'viewerSeatId' IS DISTINCT FROM actor_id::text
    OR view_receipt.normalized_source->>'disclosure' IS DISTINCT FROM 'live'
    OR parent.response_payload IS DISTINCT FROM jsonb_build_object('status','success','data',jsonb_build_object('requestId',original->>'requestId',
     'actorSeatId',actor_id,'gameId',native_game_id,'contextId',native_context_id,'epoch',basis->>'epoch','revision',next_revision::text)) THEN
    RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='native player acknowledgment or issued View differs';END IF;
   PERFORM app.check_native_view_receipt(view_receipt.id);
  ELSIF parent.response_payload IS DISTINCT FROM child.response_payload THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='native system acknowledgment differs';END IF;
 ELSE
  IF NOT is_player OR parent.state<>'rejected' OR source->'childReceiptId' IS DISTINCT FROM 'null'::jsonb
   OR source->'acceptedCheckpointId' IS DISTINCT FROM 'null'::jsonb OR NOT app.native_rejection_is_valid(parent.response_payload->'data')
   OR EXISTS(SELECT 1 FROM app.command_receipt c WHERE c.scope='host_candidate_v1' AND c.principal_key='host:'||native_game_id::text AND c.request_ref=parent.id::text) THEN
   RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='native rejection has a canonical side effect or invalid response';END IF;
  IF prepared='null'::jsonb AND source->'rejectionSource'->>'kind'='expired-response' THEN
   rejection:=source->'rejectionSource';
   SELECT * INTO view_receipt FROM app.command_receipt
    WHERE id=(rejection->>'viewReceiptId')::uuid AND scope='native_view_v1' AND state='accepted';
   IF original->'action'->>'kind' IS DISTINCT FROM 'choose'
    OR view_receipt.id IS NULL OR view_receipt.target_ref IS DISTINCT FROM native_game_id::text
    OR view_receipt.normalized_source->'basis' IS DISTINCT FROM basis
    OR view_receipt.normalized_source->>'viewerSeatId' IS DISTINCT FROM actor_id::text
    OR view_receipt.normalized_source->>'disclosure' IS DISTINCT FROM 'live'
    OR parent.response_payload IS DISTINCT FROM '{"status":"fail","data":{"decision":"stale"}}'::jsonb
    OR NOT EXISTS(SELECT 1 FROM app.native_response_wait_for_fence(native_game_id,(basis->>'revision')::numeric,
       CASE WHEN rejection ? 'restorationId' THEN
        (SELECT (r.response_payload->'data'->>'epoch')::numeric FROM app.command_receipt r
         WHERE r.id::text=rejection->>'restorationId' AND r.scope='game_restoration_v1' AND r.state='accepted'
          AND r.target_ref=native_game_id::text AND r.principal_key='recovery:'||native_game_id::text
          AND app.financial_instant(r.normalized_source->>'resumedAt')<=app.financial_instant(rejection->>'observedAt'))
       ELSE 1 END) w
       WHERE w.seat_id=actor_id AND app.financial_instant(rejection->>'observedAt')>=w.expires_at
         AND rejection=jsonb_build_object('kind','expired-response','decisionRef',w.decision_ref,
           'expiresAt',to_char(w.expires_at AT TIME ZONE 'UTC','YYYY-MM-DD"T"HH24:MI:SS.US"Z"'),
           'observedAt',to_char(app.financial_instant(rejection->>'observedAt') AT TIME ZONE 'UTC','YYYY-MM-DD"T"HH24:MI:SS.US"Z"'),
           'viewReceiptId',view_receipt.id) || CASE WHEN rejection ? 'restorationId' THEN jsonb_build_object('restorationId',rejection->>'restorationId') ELSE '{}'::jsonb END) THEN
    RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='expired response lacks the original chooser, View and elapsed deadline';
   END IF;
   PERFORM app.check_native_view_receipt(view_receipt.id);
   expected:=expected||jsonb_build_object('rejectionSource',rejection);
  ELSIF prepared='null'::jsonb THEN
   rejection:=source->'rejectionSource';observed:=rejection->'observedBasis';
   IF jsonb_typeof(rejection) IS DISTINCT FROM 'object' OR rejection->>'kind' IS DISTINCT FROM 'stale-basis'
    OR NOT app.native_basis_is_valid(observed) OR observed->>'gameId' IS DISTINCT FROM basis->>'gameId'
    OR observed->>'contextId' IS DISTINCT FROM basis->>'contextId' OR observed->>'epoch' IS DISTINCT FROM basis->>'epoch'
    OR jsonb_typeof(rejection->'terminal') IS DISTINCT FROM 'boolean'
    OR NOT app.native_uuid_is_canonical(rejection->>'originalCheckpointId') OR NOT app.native_uuid_is_canonical(rejection->>'observedCheckpointId')
    OR parent.response_payload IS DISTINCT FROM jsonb_build_object('status','fail','data',jsonb_build_object('basis','stale')) THEN
    RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='native stale rejection lacks exact observed source';END IF;
   SELECT * INTO old_checkpoint FROM app.game_checkpoint cp WHERE cp.id=(rejection->>'originalCheckpointId')::uuid AND cp.game_id=native_game_id
    AND cp.context_id=native_context_id AND accepted_revision=(basis->>'revision')::numeric;
   SELECT * INTO observed_checkpoint FROM app.game_checkpoint cp WHERE cp.id=(rejection->>'observedCheckpointId')::uuid AND cp.game_id=native_game_id
    AND cp.context_id=native_context_id AND accepted_revision=(observed->>'revision')::numeric;
   IF old_checkpoint.id IS NULL OR observed_checkpoint.id IS NULL OR old_checkpoint.accepted_revision>observed_checkpoint.accepted_revision
    OR(old_checkpoint.id=observed_checkpoint.id AND rejection->'terminal'<>'true'::jsonb)
    OR(rejection->'terminal'='true'::jsonb AND game_row.state='active')
    OR rejection IS DISTINCT FROM jsonb_build_object('kind','stale-basis','originalCheckpointId',old_checkpoint.id,
     'observedCheckpointId',observed_checkpoint.id,'observedBasis',observed,'terminal',rejection->'terminal') THEN
    RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='native stale rejection does not prove accepted history';END IF;
   PERFORM app.check_native_creator_frame(old_checkpoint.source_receipt_id);
   PERFORM app.check_native_creator_frame(observed_checkpoint.source_receipt_id);
   expected:=expected||jsonb_build_object('rejectionSource',rejection);
  ELSE
   IF NOT app.native_player_prepared_is_valid(prepared,original,actor_id) OR prepared->>'version' IS DISTINCT FROM 'native-player-reject-v1'
    OR prepared->>'runtimeId' IS DISTINCT FROM opening.normalized_source->>'runtimeId' OR prepared->>'programSourceId' IS DISTINCT FROM opening.normalized_source->>'programSourceId'
    OR parent.response_payload IS DISTINCT FROM jsonb_build_object('status','fail','data',prepared->'errors') THEN
    RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='native rejection differs from native validation source';END IF;
   SELECT * INTO view_receipt FROM app.command_receipt WHERE id=(prepared->>'viewReceiptId')::uuid AND scope='native_view_v1' AND state='accepted';
   IF view_receipt.id IS NULL OR view_receipt.target_ref IS DISTINCT FROM native_game_id::text OR view_receipt.normalized_source->'basis' IS DISTINCT FROM basis
    OR view_receipt.normalized_source->>'viewerSeatId' IS DISTINCT FROM actor_id::text OR view_receipt.normalized_source->>'disclosure' IS DISTINCT FROM 'live' THEN
    RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='native rejection has different issued View';END IF;
   PERFORM app.check_native_view_receipt(view_receipt.id);
  END IF;
 END IF;
 IF source IS DISTINCT FROM expected THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='native terminal source has unrecognized fields';END IF;
END $$;

COMMIT;
