BEGIN;

-- The native owner can establish the first prospective interval at an older
-- checkpoint without rewriting its accepted creator or any participant view.
CREATE TABLE app.native_response_adoption (
 checkpoint_id uuid PRIMARY KEY REFERENCES app.game_checkpoint(id),
 seat_id uuid NOT NULL REFERENCES app.match_seat(id),
 kind text NOT NULL CHECK (kind IN ('opening','priority','decision')),
 remaining_seats uuid[] NOT NULL CHECK (cardinality(remaining_seats) BETWEEN 2 AND 6 AND seat_id=ANY(remaining_seats)),
 owner_ref text NOT NULL,
 fence_epoch numeric(78,0) NOT NULL CHECK (fence_epoch>=1),
 started_at timestamptz NOT NULL
);
ALTER TABLE app.native_response_adoption ENABLE ROW LEVEL SECURITY;
REVOKE ALL ON TABLE app.native_response_adoption FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;
CREATE TRIGGER response_adoption_history BEFORE UPDATE OR DELETE ON app.native_response_adoption
 FOR EACH ROW EXECUTE FUNCTION app.reject_history_change();

-- Follow accepted, reciprocal native inputs only. Continue an interval across
-- a bystander departure or entropy delivery when the native obligation survived.
CREATE FUNCTION app.native_adopted_wait_window(p_game uuid,p_revision numeric)
RETURNS TABLE(seat_id uuid,decision_ref text,expires_at timestamptz,start_revision numeric)
LANGUAGE sql STABLE SECURITY DEFINER SET search_path='' AS $$
 WITH RECURSIVE anchor AS (
  SELECT cp.accepted_revision,cp.private_state_ref,a.seat_id,a.kind,a.started_at
  FROM app.native_response_adoption a JOIN app.game_checkpoint cp ON cp.id=a.checkpoint_id
  WHERE cp.game_id=p_game AND cp.accepted_revision<=p_revision
  ORDER BY cp.accepted_revision DESC LIMIT 1
 ), span(rev,proof,seat,kind,started,start_rev) AS (
  SELECT accepted_revision,private_state_ref,seat_id,kind,started_at,accepted_revision FROM anchor
  UNION ALL
  SELECT cp.accepted_revision,cp.private_state_ref,ob.seat,ob.kind,
   CASE WHEN keep.unchanged THEN s.started ELSE app.financial_instant(child.normalized_source->>'acceptedAt') END,
   CASE WHEN keep.unchanged THEN s.start_rev ELSE cp.accepted_revision END
  FROM span s JOIN app.game_checkpoint cp ON cp.game_id=p_game AND cp.accepted_revision=s.rev+1
  JOIN app.command_receipt child ON child.id=cp.source_receipt_id AND child.state='accepted' AND child.scope='host_candidate_v1'
  JOIN app.command_receipt parent ON parent.id::text=child.request_ref AND parent.state='accepted'
   AND parent.scope IN ('native_host_input_v1','native_player_v1') AND parent.target_ref=p_game::text
   AND parent.normalized_source->>'childReceiptId'=child.id::text
  CROSS JOIN LATERAL (SELECT child.normalized_source->'candidate' AS c) body
  CROSS JOIN LATERAL (SELECT coalesce(c->>'openingResponseSeatId',c->'turnResponse'->>'seatId')::uuid AS seat,
   CASE WHEN c ? 'openingResponseSeatId' THEN 'opening' ELSE c->'turnResponse'->>'kind' END AS kind) ob
  CROSS JOIN LATERAL (SELECT s.seat=ob.seat AND s.kind=ob.kind
   AND (ob.kind='opening' OR c->'turnResponse'->'sameNativeWait'='true'::jsonb)
   AND ((parent.scope='native_host_input_v1' AND parent.normalized_source->'original'->>'kind'='entropy')
    OR (parent.scope='native_player_v1' AND parent.normalized_source->'original'->'action'->>'kind'='concede'
     AND parent.normalized_source->>'actorSeatId'<>ob.seat::text)) AS unchanged) keep
  WHERE s.rev<p_revision AND ob.seat IS NOT NULL AND c->>'terminal' IS NULL
 )
 SELECT seat,proof,started+interval '120 seconds',start_rev FROM span WHERE rev=p_revision
$$;
REVOKE ALL ON FUNCTION app.native_adopted_wait_window(uuid,numeric) FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;

CREATE OR REPLACE FUNCTION app.native_match_wait_window(p_game uuid,p_revision numeric)
RETURNS TABLE(seat_id uuid,decision_ref text,expires_at timestamptz,start_revision numeric)
LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path='' AS $$
BEGIN
 RETURN QUERY SELECT * FROM app.native_adopted_wait_window(p_game,p_revision);
 IF FOUND THEN RETURN; END IF;
 RETURN QUERY SELECT * FROM app.native_turn_wait_window(p_game,p_revision);
 IF FOUND THEN RETURN; END IF;
 RETURN QUERY SELECT * FROM app.native_opening_wait_window(p_game,p_revision);
END $$;

CREATE FUNCTION app.adopt_native_response_wait(p_game uuid,p_checkpoint uuid,p_state text,p_owner text,p_fence numeric,
 p_runtime text,p_programs text,p_kind text,p_seat uuid,p_remaining uuid[]) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE game_match uuid; cp app.game_checkpoint; creator app.command_receipt; existing app.native_response_adoption;
 failed constant jsonb:='{"status":"fail","data":{"game":"unavailable"}}';
BEGIN
 SELECT match_id INTO game_match FROM app.game WHERE id=p_game;
 PERFORM id FROM app.match WHERE id=game_match AND current_game_id=p_game AND state='active' AND availability='available' FOR NO KEY UPDATE;
 IF NOT FOUND THEN RETURN failed; END IF;
 PERFORM g.id FROM app.game g WHERE g.id=p_game AND g.match_id=game_match AND g.state='active'
  AND EXISTS(SELECT 1 FROM app.game_checkpoint c WHERE c.game_id=g.id AND c.id=p_checkpoint AND c.accepted_revision=g.accepted_revision AND c.private_state_ref=p_state) FOR UPDATE;
 IF NOT FOUND THEN RETURN failed; END IF;
 PERFORM id FROM app.operational_state WHERE game_id=p_game AND owner_instance_ref=p_owner AND fence_epoch=p_fence AND availability='available' FOR NO KEY UPDATE;
 IF NOT FOUND THEN RETURN failed; END IF;
 SELECT * INTO STRICT cp FROM app.game_checkpoint WHERE id=p_checkpoint;
 SELECT * INTO STRICT creator FROM app.command_receipt WHERE id=cp.source_receipt_id;
 IF creator.state<>'accepted' OR creator.scope NOT IN ('game_opening_v1','host_candidate_v1')
  OR NOT EXISTS(SELECT 1 FROM app.command_receipt o WHERE o.scope='native_opening_v1' AND o.state='accepted'
    AND o.principal_key='native-opening:'||p_game::text AND o.target_ref=p_game::text AND o.request_ref=p_game::text
    AND o.normalized_source->>'runtimeId'=p_runtime AND o.normalized_source->>'programSourceId'=p_programs)
  OR creator.normalized_source->'candidate'->>'turnResponseProtocol'='native-turn-response-v2'
  OR (creator.scope='host_candidate_v1' AND NOT EXISTS(
    SELECT 1 FROM app.command_receipt parent WHERE parent.id::text=creator.request_ref
     AND parent.target_ref=p_game::text AND parent.state='accepted'
     AND parent.scope IN('native_host_input_v1','native_player_v1')
     AND parent.normalized_source->>'childReceiptId'=creator.id::text))
  OR (SELECT count(*) FROM app.match_seat WHERE match_id=game_match) NOT BETWEEN 3 AND 6 THEN RETURN failed; END IF;
 PERFORM app.check_native_creator_frame(creator.id);
 IF p_kind IS NULL OR p_kind NOT IN ('opening','priority','decision') OR p_seat IS NULL OR p_remaining IS NULL
  OR array_ndims(p_remaining) IS DISTINCT FROM 1 OR cardinality(p_remaining) NOT BETWEEN 2 AND 6
  OR (p_seat=ANY(p_remaining)) IS DISTINCT FROM true
  OR cardinality(p_remaining)<>(SELECT count(*) FROM app.match_seat WHERE match_id=game_match AND id=ANY(p_remaining))
 THEN RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid native response roster'; END IF;
 SELECT * INTO existing FROM app.native_response_adoption WHERE checkpoint_id=p_checkpoint;
 IF FOUND THEN
  IF existing.seat_id IS DISTINCT FROM p_seat OR existing.remaining_seats IS DISTINCT FROM p_remaining OR existing.kind IS DISTINCT FROM p_kind THEN
   RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='adopted response differs from the original native wait'; END IF;
 ELSIF NOT EXISTS(SELECT 1 FROM app.native_match_wait_window(p_game,cp.accepted_revision)) THEN
  INSERT INTO app.native_response_adoption(checkpoint_id,seat_id,kind,remaining_seats,owner_ref,fence_epoch,started_at)
  VALUES(p_checkpoint,p_seat,p_kind,p_remaining,p_owner,p_fence,clock_timestamp());
 END IF;
 RETURN app.read_native_opening_response_wait(p_game);
END $$;
REVOKE ALL ON FUNCTION app.adopt_native_response_wait(uuid,uuid,text,text,numeric,text,text,text,uuid,uuid[])
 FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;
GRANT EXECUTE ON FUNCTION app.adopt_native_response_wait(uuid,uuid,text,text,numeric,text,text,text,uuid,uuid[]) TO service_role;

CREATE OR REPLACE FUNCTION app.native_response_wait_for_fence(p_game uuid,p_revision numeric,p_fence numeric)
RETURNS TABLE(seat_id uuid,decision_ref text,expires_at timestamptz)
LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path='' AS $$
DECLARE w record; span record; reached boolean:=false; start_fence numeric;
BEGIN
 IF p_fence IS NULL OR p_fence<1 THEN RETURN; END IF;
 SELECT * INTO w FROM app.native_match_wait_window(p_game,p_revision);
 IF NOT FOUND THEN RETURN; END IF;
 seat_id:=w.seat_id; decision_ref:=w.decision_ref; expires_at:=w.expires_at;
 SELECT a.fence_epoch INTO start_fence FROM app.game_checkpoint cp JOIN app.native_response_adoption a ON a.checkpoint_id=cp.id
 WHERE cp.game_id=p_game AND cp.accepted_revision=w.start_revision;
 start_fence:=coalesce(start_fence,1);
 IF p_fence<start_fence THEN RETURN; END IF;
 IF p_fence=start_fence THEN RETURN NEXT; RETURN; END IF;
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
  IF span.accepted_revision>=w.start_revision AND span.resume_fence>start_fence THEN
   expires_at:=span.resumed+greatest(interval '0',expires_at-span.stopped);
  END IF;
  reached:=span.resume_fence=p_fence;
 END LOOP;
 IF reached THEN RETURN NEXT; END IF;
END $$;

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
 SELECT ARRAY(SELECT v::uuid FROM jsonb_array_elements_text(c.normalized_source->'candidate'->'turnResponse'->'remainingSeatIds') v
       WHERE v<>timing.seat_id::text ORDER BY v)
 INTO survivors FROM app.game_checkpoint cp JOIN app.command_receipt c ON c.id=cp.source_receipt_id
 WHERE cp.game_id=g AND cp.accepted_revision=(src->'original'->'basis'->>'revision')::numeric
  AND c.normalized_source->'candidate' ? 'turnResponse';
 IF NOT FOUND THEN
  SELECT ARRAY(SELECT v FROM unnest(a.remaining_seats) v WHERE v<>timing.seat_id ORDER BY v) INTO survivors
   FROM app.game_checkpoint cp JOIN app.native_response_adoption a ON a.checkpoint_id=cp.id
   WHERE cp.game_id=g AND cp.accepted_revision=(src->'original'->'basis'->>'revision')::numeric;
 END IF;
 IF NOT FOUND THEN
  survivors:=app.native_opening_other_survivors(g,(src->'original'->'basis'->>'revision')::numeric,timing.seat_id);
 END IF;
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

COMMIT;
