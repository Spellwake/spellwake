BEGIN;

-- Shared transport bound only: source predicates and mutation authority stay in
-- their specific owners. Callers supply at most their requested limit plus one.
CREATE FUNCTION app.native_bounded_work_page(p_rows jsonb,p_limit integer) RETURNS jsonb
LANGUAGE plpgsql IMMUTABLE SET search_path='' AS $$
DECLARE row_body jsonb;items jsonb:='[]';cursor_id uuid;more boolean:=false;
BEGIN
 IF p_limit IS NULL OR p_limit NOT BETWEEN 1 AND 100 OR jsonb_typeof(p_rows) IS DISTINCT FROM 'array'
  OR jsonb_array_length(p_rows)>p_limit+1 THEN RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid native work page';END IF;
 FOR row_body IN SELECT * FROM jsonb_array_elements(p_rows) LOOP
  IF jsonb_array_length(items)=p_limit THEN more:=true;EXIT;END IF;
  IF octet_length(jsonb_build_object('items',items||jsonb_build_array(row_body),'nextAfter',row_body->>'id')::text)>1048576 THEN
   IF jsonb_array_length(items)=0 THEN RAISE EXCEPTION USING ERRCODE='54000',MESSAGE='native work source exceeds page budget';END IF;
   more:=true;EXIT;
  END IF;
  cursor_id:=(row_body->>'id')::uuid;items:=items||jsonb_build_array(row_body);
 END LOOP;
 RETURN jsonb_build_object('items',items,'nextAfter',CASE WHEN more THEN cursor_id END);
END $$;

CREATE FUNCTION app.read_native_countdown_work(p_after uuid,p_limit integer) RETURNS jsonb
LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path='' AS $$
DECLARE rows jsonb;
BEGIN
 IF p_after='00000000-0000-0000-0000-000000000000'::uuid OR p_limit IS NULL OR p_limit NOT BETWEEN 1 AND 100 THEN
  RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid native countdown page';END IF;
 SELECT coalesce(jsonb_agg(body ORDER BY id),'[]') INTO rows FROM(
  SELECT r.id,jsonb_build_object('id',r.id,'revision',r.revision::text,
   'countdownAt',to_char(r.countdown_at AT TIME ZONE 'UTC','YYYY-MM-DD"T"HH24:MI:SS.US"Z"'),
   'profileId',r.profile_id,'formatId',r.format_id,'mode',r.mode,'competitive',r.competitive,'structure',r.structure) body
  FROM app.room r JOIN app.account a ON a.id=r.host_id AND a.state='active'
  JOIN app.table_profile_revision p ON p.id=r.profile_id AND p.admission_qualification_ref IS NOT NULL
  WHERE r.state='countdown' AND r.countdown_at<=statement_timestamp()
   AND(p_after IS NULL OR r.id>p_after) ORDER BY r.id LIMIT p_limit+1) selected;
 RETURN app.native_bounded_work_page(rows,p_limit);
END $$;

CREATE FUNCTION app.read_native_ai_binding_work(p_after uuid,p_limit integer) RETURNS jsonb
LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path='' AS $$
DECLARE rows jsonb;
BEGIN
 IF p_after='00000000-0000-0000-0000-000000000000'::uuid OR p_limit IS NULL OR p_limit NOT BETWEEN 1 AND 100 THEN
  RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid native binding page';END IF;
 SELECT coalesce(jsonb_agg(body ORDER BY id),'[]') INTO rows FROM(
  SELECT b.id,jsonb_build_object('id',b.id,'epoch',b.binding_epoch::text,
   'desiredState',b.desired_state,'observedState',b.observed_state,
   'startWork',app.read_ai_start_work(b.id,b.binding_epoch),
   'hasPendingStops',EXISTS(SELECT 1 FROM app.read_ai_stop_work(b.id)),
   'grant',(SELECT jsonb_build_object('id',g.id,'epoch',g.binding_epoch::text,'secretRef',g.grant_secret_ref,
    'deliveryRevision',g.delivery_revision::text,'deliveryState',g.delivery_state)
    FROM app.controller_grant g WHERE g.binding_id=b.id AND g.binding_epoch=b.binding_epoch
     AND g.match_seat_id IS NOT DISTINCT FROM b.match_seat_id AND g.state='active')) body
  FROM app.ai_runner_binding b JOIN app.room_slot s ON s.id=b.room_slot_id
  JOIN app.room r ON r.id=s.room_id JOIN app.account a ON a.id=s.sponsor_id
  WHERE(p_after IS NULL OR b.id>p_after) AND(
   EXISTS(SELECT 1 FROM app.read_ai_stop_work(b.id)) OR(
    b.bootstrap_receipt_id IS NOT NULL AND b.desired_state='running' AND s.retired_at IS NULL
    AND r.state<>'closed' AND(a.state='active' OR app.ai_sponsor_continuing_match(b.id,a.id))
    AND NOT EXISTS(SELECT 1 FROM app.match WHERE room_id=r.id AND state IN('completed','no_contest'))
    AND NOT app.ai_slot_ready(s.id))) ORDER BY b.id LIMIT p_limit+1) selected;
 RETURN app.native_bounded_work_page(rows,p_limit);
END $$;

CREATE FUNCTION app.read_native_recovery_work(p_after uuid,p_limit integer) RETURNS jsonb
LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path='' AS $$
DECLARE rows jsonb;
BEGIN
 IF p_after='00000000-0000-0000-0000-000000000000'::uuid OR p_limit IS NULL OR p_limit NOT BETWEEN 1 AND 100 THEN
  RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid native game observation page';END IF;
 SELECT coalesce(jsonb_agg(body ORDER BY id),'[]') INTO rows FROM(
  SELECT o.id,jsonb_build_object('id',o.id,'gameId',g.id,'matchId',m.id,
   'ownerRef',o.owner_instance_ref,'fenceEpoch',o.fence_epoch::text,'operationalRevision',o.revision::text,
   'availability',o.availability,'recoveryCheckpointId',o.recovery_checkpoint_id,'incidentReceiptId',o.incident_receipt_id) body
  FROM app.operational_state o JOIN app.game g ON g.id=o.game_id
  JOIN app.match m ON m.id=g.match_id AND m.current_game_id=g.id
  WHERE(p_after IS NULL OR o.id>p_after) AND g.state='active' AND m.state='active'
   AND o.availability IN('available','recovering') AND o.availability=m.availability
   AND app.read_native_game_source(g.id)->>'status'='success'
  ORDER BY o.id LIMIT p_limit+1) selected;
 RETURN app.native_bounded_work_page(rows,p_limit);
END $$;

CREATE FUNCTION app.read_native_match_recovery_work(p_after uuid,p_limit integer) RETURNS jsonb
LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path='' AS $$
DECLARE rows jsonb;
BEGIN
 IF p_after='00000000-0000-0000-0000-000000000000'::uuid OR p_limit IS NULL OR p_limit NOT BETWEEN 1 AND 100 THEN
  RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid native match observation page';END IF;
 SELECT coalesce(jsonb_agg(body ORDER BY id),'[]') INTO rows FROM(
  SELECT m.id,jsonb_build_object('id',m.id,'revision',m.revision::text,
   'availabilityRevision',m.availability_revision::text,'availability',m.availability,
   'incidentReceiptId',CASE WHEN m.availability='recovering' THEN m.incident_receipt_id END,
   'profileId',m.profile_id,'state',m.state,
   'startedAt',to_char(m.started_at AT TIME ZONE 'UTC','YYYY-MM-DD"T"HH24:MI:SS.US"Z"'),
   'reservedGameId',m.reserved_game_id) body
  FROM app.match m WHERE(p_after IS NULL OR m.id>p_after) AND m.current_game_id IS NULL
   AND m.state IN('ready','between_games') AND m.availability IN('available','recovering')
   AND m.started_at IS NOT NULL AND m.room_id IS NOT NULL AND m.accepted_release_id IS NOT NULL
   AND EXISTS(SELECT 1 FROM app.command_receipt a WHERE a.principal_key='admission:'||m.id::text
    AND a.scope='match_decks_v1' AND a.target_ref=m.id::text AND a.state='accepted'
    AND a.response_payload->'data'->>'releaseId'=m.accepted_release_id::text
    AND a.response_payload->'data'->>'qualificationRef'=m.admission_qualification_ref)
   AND EXISTS(SELECT 1 FROM app.game g0 JOIN app.command_receipt start
    ON start.scope='room_start_v1' AND start.principal_key='room-start:'||m.room_id::text
     AND start.target_ref=m.room_id::text AND start.state='accepted'
     AND start.response_payload->'data'->>'resourceId'=m.id::text
     AND start.response_payload->'data'->>'gameId'=g0.id::text
    WHERE g0.match_id=m.id AND g0.game_index=0)
   AND(m.availability='available' OR EXISTS(SELECT 1 FROM app.command_receipt incident
    WHERE incident.id=m.incident_receipt_id AND incident.scope='match_incident_v1'
     AND incident.principal_key='match-recovery:'||m.id::text AND incident.target_ref=m.id::text
     AND incident.state='accepted' AND incident.normalized_source->>'matchId'=m.id::text
     AND incident.normalized_source->>'matchAvailabilityRevision'=m.availability_revision::text
     AND incident.normalized_source->>'phase'=m.state))
  ORDER BY m.id LIMIT p_limit+1) selected;
 RETURN app.native_bounded_work_page(rows,p_limit);
END $$;

CREATE FUNCTION app.read_native_unscheduled_decision_work(p_after uuid,p_limit integer) RETURNS jsonb
LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path='' AS $$
DECLARE rows jsonb;
BEGIN
 IF p_after='00000000-0000-0000-0000-000000000000'::uuid OR p_limit IS NULL OR p_limit NOT BETWEEN 1 AND 100 THEN
  RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid native unarmed decision page';END IF;
 SELECT coalesce(jsonb_agg(body ORDER BY id),'[]') INTO rows FROM(
  SELECT d.id,jsonb_build_object('id',d.id,'matchId',m.id,'seatId',d.seat_id,'afterGameId',d.after_game_id,
   'profileId',m.profile_id,'matchRevision',m.revision::text,'clockRevision',d.clock_revision::text,
   'deadlineRevision',d.deadline_revision::text,'availabilityRevision',m.availability_revision::text,
   'openedAt',to_char(d.opened_at AT TIME ZONE 'UTC','YYYY-MM-DD"T"HH24:MI:SS.US"Z"')) body
  FROM app.between_game_decision d JOIN app.match m ON m.id=d.match_id
  JOIN app.game g ON g.id=d.after_game_id AND g.match_id=m.id
  JOIN app.match_seat seat ON seat.id=d.seat_id AND seat.match_id=m.id
  JOIN app.table_profile_revision profile ON profile.id=m.profile_id AND profile.admission_qualification_ref IS NOT NULL
  WHERE(p_after IS NULL OR d.id>p_after) AND d.state='pending' AND d.deadline_policy_ref IS NULL
   AND m.state='between_games' AND m.availability='available' AND m.current_game_id IS NULL
   AND m.started_at IS NOT NULL AND m.accepted_release_id IS NOT NULL AND m.admission_qualification_ref IS NOT NULL
   AND g.state IN('completed','no_contest') AND NOT EXISTS(SELECT 1 FROM app.game later WHERE later.match_id=m.id AND later.game_index>g.game_index)
   AND EXISTS(SELECT 1 FROM app.game_result result JOIN app.command_receipt consumed
    ON consumed.scope='game_outcome_v1' AND consumed.principal_key='match-lifecycle:'||m.id::text
     AND consumed.target_ref=result.id::text AND consumed.request_ref=result.id::text
     AND consumed.state='accepted' AND consumed.response_payload->'data'->>'phase'='between_games'
    WHERE result.game_id=g.id)
  ORDER BY d.id LIMIT p_limit+1) selected;
 RETURN app.native_bounded_work_page(rows,p_limit);
END $$;

REVOKE ALL ON FUNCTION app.native_bounded_work_page(jsonb,integer),app.read_native_countdown_work(uuid,integer),
 app.read_native_ai_binding_work(uuid,integer),app.read_native_recovery_work(uuid,integer),
 app.read_native_match_recovery_work(uuid,integer),app.read_native_unscheduled_decision_work(uuid,integer)
 FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;
COMMIT;
