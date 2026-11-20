BEGIN;
CREATE FUNCTION app.guard_match_availability_history() RETURNS trigger LANGUAGE plpgsql SET search_path='' AS $$
BEGIN
 IF TG_OP='INSERT' THEN
 IF NEW.availability<>'available' OR NEW.availability_revision<>0 OR NEW.incident_receipt_id IS NOT NULL OR NEW.restoration_receipt_id IS NOT NULL THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='match starts without an incident';END IF;
 ELSIF (NEW.availability,NEW.availability_revision,NEW.incident_receipt_id,NEW.restoration_receipt_id) IS DISTINCT FROM (OLD.availability,OLD.availability_revision,OLD.incident_receipt_id,OLD.restoration_receipt_id) THEN
 IF NEW.availability_revision<>OLD.availability_revision+1 OR (OLD.incident_receipt_id IS NOT NULL AND NEW.incident_receipt_id IS NULL) THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='match availability history requires monotone retained source';END IF;
 END IF;RETURN NEW;
END $$;
CREATE TRIGGER match_availability_history BEFORE INSERT OR UPDATE ON app.match FOR EACH ROW EXECUTE FUNCTION app.guard_match_availability_history();
CREATE FUNCTION app.check_match_availability(p_match uuid) RETURNS void LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE m app.match; incident app.command_receipt; restored app.command_receipt; host app.operational_state; pending_ids jsonb;
BEGIN
 SELECT * INTO m FROM app.match WHERE id=p_match;
 IF m.id IS NULL OR m.incident_receipt_id IS NULL THEN RETURN;END IF;
 SELECT * INTO incident FROM app.command_receipt WHERE id=m.incident_receipt_id;
 IF incident.state IS DISTINCT FROM 'accepted' OR incident.normalized_source->>'matchId' IS DISTINCT FROM m.id::text OR incident.scope NOT IN ('game_incident_v1','match_incident_v1') THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='match availability lacks accepted incident';END IF;
 IF m.availability='recovering' THEN
 IF m.restoration_receipt_id IS NOT NULL OR incident.normalized_source->>'matchAvailabilityRevision' IS DISTINCT FROM m.availability_revision::text THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='paused availability differs from incident revision';END IF;
 ELSE
 SELECT * INTO restored FROM app.command_receipt WHERE id=m.restoration_receipt_id;
 IF restored.state IS DISTINCT FROM 'accepted' OR restored.scope NOT IN ('game_restoration_v1','match_restoration_v1') OR restored.normalized_source->>'incidentId' IS DISTINCT FROM incident.id::text OR restored.normalized_source->>'matchId' IS DISTINCT FROM m.id::text OR restored.normalized_source->>'matchAvailabilityRevision' IS DISTINCT FROM m.availability_revision::text THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='match resume lacks exact accepted incident disposition';END IF;
 END IF;
 IF incident.scope='game_incident_v1' THEN
 SELECT * INTO host FROM app.operational_state WHERE incident_receipt_id=incident.id;
 IF host.id IS NULL OR NOT EXISTS(SELECT 1 FROM app.game WHERE id=host.game_id AND match_id=m.id) OR host.availability<>m.availability OR host.restoration_receipt_id IS DISTINCT FROM m.restoration_receipt_id THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='match availability differs from its game incident';END IF;
 IF m.availability='recovering' AND (incident.normalized_source->>'matchRevision' IS DISTINCT FROM m.revision::text OR incident.normalized_source->>'matchPhase' IS DISTINCT FROM m.state OR incident.normalized_source->>'currentGameId' IS DISTINCT FROM m.current_game_id::text OR incident.normalized_source->>'reservedGameId' IS DISTINCT FROM m.reserved_game_id::text) THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='paused active match frontier changed';END IF;
 ELSE
 IF incident.principal_key IS DISTINCT FROM 'match-recovery:'||m.id::text OR incident.target_ref IS DISTINCT FROM m.id::text OR incident.normalized_source->>'version' IS DISTINCT FROM 'match-incident-v1' THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='match-only incident source identity differs';END IF;
 IF m.availability='recovering' THEN
 SELECT coalesce(jsonb_agg(id ORDER BY id),'[]'::jsonb) INTO pending_ids FROM app.between_game_decision WHERE match_id=m.id AND state='pending';
 IF incident.response_payload->'data'->>'revision' IS DISTINCT FROM m.revision::text OR incident.normalized_source->>'phase' IS DISTINCT FROM m.state OR incident.normalized_source->>'reservedGameId' IS DISTINCT FROM m.reserved_game_id::text OR m.current_game_id IS NOT NULL OR pending_ids IS DISTINCT FROM incident.normalized_source->'pendingDecisionIds'
 OR app.match_clock_snapshot(m.id) IS DISTINCT FROM incident.normalized_source->'clockValues'
 OR EXISTS(SELECT 1 FROM app.match_clock c JOIN app.match_seat seat ON seat.id=c.match_seat_id WHERE seat.match_id=m.id AND NOT c.paused)
 OR EXISTS(SELECT 1 FROM app.between_game_decision WHERE match_id=m.id AND state='pending' AND deadline_at IS NOT NULL) THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='paused match-only phase, decisions or banks changed';END IF;
 ELSE IF restored.scope<>'match_restoration_v1' OR restored.principal_key IS DISTINCT FROM 'match-recovery:'||m.id::text OR restored.target_ref IS DISTINCT FROM m.id::text THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='match-only resume source differs';END IF;END IF;
 END IF;
END $$;
CREATE FUNCTION app.guard_match_availability() RETURNS trigger LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE parent uuid;
BEGIN
 IF TG_TABLE_NAME='match' THEN parent:=NEW.id;
 ELSIF TG_TABLE_NAME='match_clock' THEN SELECT match_id INTO parent FROM app.match_seat WHERE id=NEW.match_seat_id;
 ELSE parent:=NEW.match_id;END IF;
 PERFORM app.check_match_availability(parent);RETURN NULL;
END $$;
CREATE CONSTRAINT TRIGGER match_availability_source AFTER INSERT OR UPDATE ON app.match DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION app.guard_match_availability();
CREATE CONSTRAINT TRIGGER match_availability_clock AFTER INSERT OR UPDATE ON app.match_clock DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION app.guard_match_availability();
CREATE CONSTRAINT TRIGGER match_availability_decision AFTER INSERT OR UPDATE ON app.between_game_decision DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION app.guard_match_availability();
REVOKE ALL ON FUNCTION app.guard_match_availability_history(),app.check_match_availability(uuid),app.guard_match_availability() FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;
CREATE OR REPLACE FUNCTION app.accept_host_candidate(p_game uuid,p_owner text,p_epoch numeric,p_basis numeric,p_request uuid,p_candidate jsonb)
RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE source_game app.game; host app.operational_state; receipt app.command_receipt;
 fingerprint text; context_id uuid; checkpoint_id uuid; event_ids uuid[]:='{}'; event_id uuid; result_id uuid;
 item jsonb; next_sequence numeric; next_revision numeric; next_clock numeric; terminal jsonb;
 accepted_at timestamptz; discovered_match uuid; seat_count bigint; clock_count bigint;
BEGIN
 IF current_setting('transaction_isolation')<>'read committed' THEN
  RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='candidate acceptance requires read committed retry'; END IF;
 PERFORM pg_advisory_xact_lock_shared(hashtextextended('supply-population-cutoff-v1',0));
 IF p_game IS NULL OR p_request IS NULL OR p_request='00000000-0000-0000-0000-000000000000'::uuid
 OR p_owner IS NULL OR length(btrim(p_owner)) NOT BETWEEN 1 AND 256
 OR p_epoch IS NULL OR p_epoch::text !~ '^(0|[1-9][0-9]{0,77})$'
 OR p_basis IS NULL OR p_basis::text !~ '^(0|[1-9][0-9]{0,77})$'
 OR jsonb_typeof(p_candidate) IS DISTINCT FROM 'object' OR octet_length(p_candidate::text)>1048576 THEN
  RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid candidate identity or envelope'; END IF;
 fingerprint:=encode(sha256(convert_to(jsonb_build_object('owner',p_owner,'epoch',p_epoch::text,'basis',p_basis::text,'candidate',p_candidate)::text,'UTF8')),'hex');
 INSERT INTO app.command_receipt(principal_key,request_ref,scope,target_ref,payload_hash,state,result_ref)
 VALUES('host:'||p_game::text,p_request::text,'host_candidate_v1',p_game::text,fingerprint,'pending','')
 ON CONFLICT DO NOTHING RETURNING * INTO receipt;
 IF receipt.id IS NULL THEN
  SELECT * INTO STRICT receipt FROM app.command_receipt WHERE principal_key='host:'||p_game::text AND scope='host_candidate_v1' AND request_ref=p_request::text FOR UPDATE;
  IF receipt.payload_hash<>fingerprint OR receipt.target_ref<>p_game::text THEN
   RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='candidate request was reused with different terms'; END IF;
  IF receipt.state='pending' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='candidate disposition is not terminal'; END IF;
  RETURN receipt.response_payload;
 END IF;
 SELECT match_id INTO discovered_match FROM app.game WHERE id=p_game;
 IF discovered_match IS NULL THEN RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('game','unavailable')); END IF;
 PERFORM id FROM app.match WHERE id=discovered_match FOR NO KEY UPDATE;
 IF EXISTS(SELECT 1 FROM app.match WHERE id=discovered_match AND availability<>'available') THEN RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('availability','match recovering'));END IF;
 SELECT * INTO source_game FROM app.game WHERE id=p_game FOR UPDATE;
 IF source_game.id IS NULL OR source_game.match_id IS DISTINCT FROM discovered_match THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='host game parent changed before source fence'; END IF;
 SELECT * INTO host FROM app.operational_state WHERE game_id=p_game FOR NO KEY UPDATE;
 IF host.id IS NULL OR host.owner_instance_ref<>p_owner OR host.fence_epoch<>p_epoch OR host.availability<>'available' THEN
  RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('fence','stale or unavailable')); END IF;
 IF source_game.state<>'active' OR source_game.accepted_revision<>p_basis THEN
  RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('basis','stale or terminal')); END IF;
 IF EXISTS(SELECT 1 FROM jsonb_object_keys(p_candidate) k WHERE k NOT IN ('contextId','stateRef','factsRef','events','facts','clocks','terminal','participation'))
 OR jsonb_typeof(p_candidate->'contextId') IS DISTINCT FROM 'string'
 OR (p_candidate->>'contextId') !~ '^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$'
 OR jsonb_typeof(p_candidate->'stateRef') IS DISTINCT FROM 'string' OR length(btrim(p_candidate->>'stateRef')) NOT BETWEEN 1 AND 2048
 OR jsonb_typeof(p_candidate->'factsRef') IS DISTINCT FROM 'string' OR length(btrim(p_candidate->>'factsRef')) NOT BETWEEN 1 AND 2048
 OR jsonb_typeof(p_candidate->'events') IS DISTINCT FROM 'array'
 OR jsonb_typeof(p_candidate->'facts') IS DISTINCT FROM 'array'
 OR jsonb_typeof(p_candidate->'clocks') IS DISTINCT FROM 'array'
 OR jsonb_typeof(p_candidate->'participation') IS DISTINCT FROM 'array' THEN
  RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid candidate artifact fields'; END IF;
 IF jsonb_array_length(p_candidate->'events') NOT BETWEEN 1 AND 1024
 OR jsonb_array_length(p_candidate->'facts')>4096 THEN
  RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='candidate exceeds accepted transaction bounds'; END IF;
 context_id:=(p_candidate->>'contextId')::uuid;
 IF NOT EXISTS(SELECT 1 FROM app.game_context WHERE id=context_id AND game_id=p_game) THEN
  RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('context','unavailable')); END IF;
 terminal:=p_candidate->'terminal';
 IF terminal='null'::jsonb THEN terminal:=NULL; END IF;
 IF terminal IS NOT NULL AND (jsonb_typeof(terminal)<>'object'
  OR (terminal->>'outcome') IS NULL OR terminal->>'outcome' NOT IN ('win','draw','no_contest')
  OR jsonb_typeof(terminal->'winnersRef') IS DISTINCT FROM 'string' OR length(btrim(terminal->>'winnersRef')) NOT BETWEEN 1 AND 2048
  OR jsonb_typeof(terminal->'winnerSeatIds') IS DISTINCT FROM 'array'
  OR jsonb_typeof(terminal->'reason') IS DISTINCT FROM 'string' OR length(btrim(terminal->>'reason')) NOT BETWEEN 1 AND 256
  OR EXISTS(SELECT 1 FROM jsonb_object_keys(terminal) k WHERE k NOT IN ('outcome','winnersRef','reason','winnerSeatIds'))) THEN
  RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid terminal candidate'; END IF;
 IF terminal IS NOT NULL THEN
  IF EXISTS(SELECT 1 FROM jsonb_array_elements(terminal->'winnerSeatIds') x WHERE jsonb_typeof(x) IS DISTINCT FROM 'string' OR x#>>'{}' !~ '^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$') THEN RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid normalized winning seat'; END IF;
  PERFORM app.validate_normalized_winners(source_game.match_id,terminal->>'outcome',ARRAY(SELECT x::uuid FROM jsonb_array_elements_text(terminal->'winnerSeatIds') x));
 END IF;
 SELECT count(*) INTO seat_count FROM app.match_seat WHERE match_id=source_game.match_id;
 SELECT count(*) INTO clock_count FROM app.match_clock c JOIN app.match_seat s ON s.id=c.match_seat_id WHERE s.match_id=source_game.match_id;
 IF seat_count=0 OR clock_count<>seat_count OR jsonb_array_length(p_candidate->'clocks')<>seat_count THEN
  RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('clocks','complete match clock state required')); END IF;
 FOR item IN SELECT v FROM jsonb_array_elements(p_candidate->'clocks') v ORDER BY v->>'seatId' LOOP
  IF jsonb_typeof(item) IS DISTINCT FROM 'object' OR jsonb_typeof(item->'seatId') IS DISTINCT FROM 'string'
   OR (item->>'seatId') !~ '^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$'
   OR jsonb_typeof(item->'clockRevision') IS DISTINCT FROM 'string' OR (item->>'clockRevision') !~ '^(0|[1-9][0-9]{0,77})$'
   OR jsonb_typeof(item->'remainingMs') IS DISTINCT FROM 'string' OR (item->>'remainingMs') !~ '^(0|[1-9][0-9]{0,77})$'
   OR jsonb_typeof(item->'paused') IS DISTINCT FROM 'boolean'
   OR EXISTS(SELECT 1 FROM jsonb_object_keys(item) k WHERE k NOT IN ('seatId','clockRevision','remainingMs','paused')) THEN
   RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid candidate clock'; END IF;
  PERFORM c.id FROM app.match_clock c JOIN app.match_seat s ON s.id=c.match_seat_id
   WHERE s.id=(item->>'seatId')::uuid AND s.match_id=source_game.match_id
    AND c.clock_revision=(item->>'clockRevision')::numeric AND c.remaining_ms>=(item->>'remainingMs')::numeric FOR NO KEY UPDATE OF c;
  IF NOT FOUND THEN RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('clocks','stale or invalid')); END IF;
 END LOOP;
 IF (SELECT count(DISTINCT v->>'seatId') FROM jsonb_array_elements(p_candidate->'clocks') v)<>seat_count THEN
  RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('clocks','duplicate seat')); END IF;
 SELECT coalesce(max(c.clock_revision),0)+1 INTO next_clock FROM app.match_clock c JOIN app.match_seat s ON s.id=c.match_seat_id WHERE s.match_id=source_game.match_id;
 IF terminal IS NULL AND jsonb_array_length(p_candidate->'participation')<>0 THEN
  RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='only terminal candidates carry final participation'; END IF;
 IF terminal IS NOT NULL AND jsonb_array_length(p_candidate->'participation')<>(SELECT count(*) FROM app.match_seat WHERE match_id=source_game.match_id AND controller_kind='human') THEN
  RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='terminal candidate requires every human participation total'; END IF;
 next_revision:=p_basis+1; accepted_at:=clock_timestamp();
 UPDATE app.game SET accepted_revision=next_revision,state=CASE WHEN terminal IS NULL THEN 'active' WHEN terminal->>'outcome'='no_contest' THEN 'no_contest' ELSE 'completed' END WHERE id=p_game;
 FOR item IN SELECT v FROM jsonb_array_elements(p_candidate->'clocks') v ORDER BY v->>'seatId' LOOP
  UPDATE app.match_clock SET remaining_ms=(item->>'remainingMs')::numeric,clock_revision=next_clock,
   paused=CASE WHEN terminal IS NOT NULL THEN true ELSE (item->>'paused')::boolean END,last_accounted_at=accepted_at
   WHERE match_seat_id=(item->>'seatId')::uuid;
 END LOOP;
 INSERT INTO app.game_checkpoint(game_id,context_id,accepted_revision,private_state_ref,facts_ref,clock_revision)
 VALUES(p_game,context_id,next_revision,p_candidate->>'stateRef',p_candidate->>'factsRef',next_clock) RETURNING id INTO checkpoint_id;
 SELECT coalesce(max(sequence),-1)+1 INTO next_sequence FROM app.game_event WHERE game_id=p_game;
 FOR item IN SELECT * FROM jsonb_array_elements(p_candidate->'events') LOOP
  IF jsonb_typeof(item) IS DISTINCT FROM 'string' OR length(btrim(item#>>'{}')) NOT BETWEEN 1 AND 2048 THEN
   RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid private event artifact'; END IF;
  INSERT INTO app.game_event(game_id,context_id,sequence,accepted_revision,private_payload_ref)
  VALUES(p_game,context_id,next_sequence,next_revision,item#>>'{}') RETURNING id INTO event_id;
  event_ids:=array_append(event_ids,event_id); next_sequence:=next_sequence+1;
 END LOOP;
 FOR item IN SELECT * FROM jsonb_array_elements(p_candidate->'facts') LOOP
  IF jsonb_typeof(item) IS DISTINCT FROM 'object' OR jsonb_typeof(item->'eventIndex') IS DISTINCT FROM 'string'
   OR (item->>'eventIndex') !~ '^(0|[1-9][0-9]{0,3})$' OR (item->>'eventIndex')::integer>=cardinality(event_ids)
   OR jsonb_typeof(item->'accountId') IS DISTINCT FROM 'string' OR (item->>'accountId') !~ '^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$'
   OR jsonb_typeof(item->'kind') IS DISTINCT FROM 'string' OR length(btrim(item->>'kind')) NOT BETWEEN 1 AND 128
   OR jsonb_typeof(item->'revision') IS DISTINCT FROM 'string' OR length(btrim(item->>'revision')) NOT BETWEEN 1 AND 128
   OR jsonb_typeof(item->'value') IS DISTINCT FROM 'string' OR (item->>'value') !~ '^(0|[1-9][0-9]{0,77})$'
   OR EXISTS(SELECT 1 FROM jsonb_object_keys(item) k WHERE k NOT IN ('eventIndex','accountId','kind','revision','value')) THEN
   RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid accepted activity fact'; END IF;
  INSERT INTO app.game_activity_fact(game_id,source_event_id,account_id,kind,value,fact_revision)
  VALUES(p_game,event_ids[(item->>'eventIndex')::integer+1],(item->>'accountId')::uuid,item->>'kind',(item->>'value')::numeric,item->>'revision');
 END LOOP;
 FOR item IN SELECT * FROM jsonb_array_elements(p_candidate->'participation') LOOP
  IF jsonb_typeof(item) IS DISTINCT FROM 'object' OR jsonb_typeof(item->'seatId') IS DISTINCT FROM 'string'
   OR (item->>'seatId') !~ '^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$'
   OR jsonb_typeof(item->'ownTurns') IS DISTINCT FROM 'string' OR (item->>'ownTurns') !~ '^(0|[1-9][0-9]{0,77})$'
   OR jsonb_typeof(item->'reason') IS DISTINCT FROM 'string' OR length(btrim(item->>'reason')) NOT BETWEEN 1 AND 256
   OR EXISTS(SELECT 1 FROM jsonb_object_keys(item) k WHERE k NOT IN ('seatId','ownTurns','reason')) THEN
   RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid accepted participation'; END IF;
  INSERT INTO app.game_participation(game_id,match_seat_id,account_id,own_turns,real_game,reason,completed_at,accepted_revision)
  SELECT p_game,s.id,s.account_id,(item->>'ownTurns')::numeric,(item->>'ownTurns')::numeric>=3,item->>'reason',accepted_at,next_revision
  FROM app.match_seat s WHERE s.id=(item->>'seatId')::uuid AND s.match_id=source_game.match_id AND s.controller_kind='human';
  IF NOT FOUND THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='participation seat is not a human in this match'; END IF;
 END LOOP;
 IF terminal IS NOT NULL THEN
  INSERT INTO app.game_result(game_id,source_event_id,outcome,winners_ref,end_reason,winner_seat_ids)
  VALUES(p_game,event_ids[cardinality(event_ids)],terminal->>'outcome',terminal->>'winnersRef',terminal->>'reason',ARRAY(SELECT x::uuid FROM jsonb_array_elements_text(terminal->'winnerSeatIds') x)) RETURNING id INTO result_id;
  INSERT INTO app.durable_handoff(match_id,game_id,source_ref,destination,kind,state)
  VALUES(source_game.match_id,p_game,result_id::text,'match_lifecycle','game_outcome','pending');
 END IF;
 RETURN app.finish_account_command(receipt.id,true,jsonb_build_object('resourceId',checkpoint_id,'revision',next_revision::text));
END $$;

CREATE OR REPLACE FUNCTION app.accept_reserved_game(p_match uuid,p_expected numeric,p_game uuid,p_owner text,p_opening jsonb) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE m app.match; g app.game; receipt app.command_receipt; fingerprint text; context_id uuid; at_time timestamptz; next_clock numeric; ante_source app.ante_commitment; original_room uuid;
BEGIN
 IF current_setting('transaction_isolation')<>'read committed' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='opening acceptance requires read committed retry'; END IF;
 IF p_match IS NULL OR p_game IS NULL OR p_owner IS NULL OR length(btrim(p_owner)) NOT BETWEEN 1 AND 256 OR p_expected IS NULL OR p_expected::text !~ '^(0|[1-9][0-9]{0,77})$'
 OR jsonb_typeof(p_opening) IS DISTINCT FROM 'object' OR octet_length(p_opening::text)>16384
 OR EXISTS(SELECT 1 FROM unnest(ARRAY['snapshotRef','contextRevision','stateRef','factsRef','eventRef','qualificationRef']) k WHERE jsonb_typeof(p_opening->k) IS DISTINCT FROM 'string' OR length(btrim(p_opening->>k)) NOT BETWEEN 1 AND 2048)
 OR EXISTS(SELECT 1 FROM jsonb_object_keys(p_opening) k WHERE k NOT IN ('snapshotRef','contextRevision','stateRef','factsRef','eventRef','qualificationRef')) THEN RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid normalized opening artifacts'; END IF;
 fingerprint:=app.auth_json_hash(jsonb_build_object('match',p_match,'game',p_game,'owner',p_owner,'opening',p_opening));
 INSERT INTO app.command_receipt(principal_key,scope,target_ref,request_ref,payload_hash,state,result_ref)
 VALUES('match-opening:'||p_match::text,'game_opening_v1',p_game::text,p_game::text,fingerprint,'pending','') ON CONFLICT DO NOTHING RETURNING * INTO receipt;
 IF receipt.id IS NULL THEN
 SELECT * INTO STRICT receipt FROM app.command_receipt WHERE principal_key='match-opening:'||p_match::text AND scope='game_opening_v1' AND request_ref=p_game::text FOR UPDATE;
 IF receipt.payload_hash<>fingerprint THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='opening source was reused with different artifacts'; END IF;
 IF receipt.state='pending' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='opening pending'; END IF;
 RETURN receipt.response_payload;
 END IF;
 SELECT room_id INTO original_room FROM app.match WHERE id=p_match;
 PERFORM id FROM app.room WHERE id=original_room FOR NO KEY UPDATE;
 PERFORM b.id FROM app.ai_runner_binding b JOIN app.match_seat s ON s.id=b.match_seat_id WHERE s.match_id=p_match ORDER BY b.id FOR NO KEY UPDATE OF b;
 SELECT * INTO m FROM app.match WHERE id=p_match FOR NO KEY UPDATE;
 IF m.room_id IS DISTINCT FROM original_room THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='opening room source changed before fence';END IF;
 SELECT * INTO g FROM app.game WHERE id=p_game FOR NO KEY UPDATE;
 IF m.id IS NULL OR g.id IS NULL OR m.availability<>'available' OR m.revision<>p_expected OR m.reserved_game_id IS DISTINCT FROM p_game OR m.current_game_id IS NOT NULL OR g.match_id<>m.id OR g.state<>'reserved' OR m.state<>'ready' OR m.accepted_release_id IS NULL THEN
 RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='stale or unavailable game reservation; retry from current basis'; END IF;
 IF EXISTS(SELECT 1 FROM app.match_seat seat WHERE seat.match_id=m.id AND seat.controller_kind='ai' AND NOT EXISTS(
 SELECT 1 FROM app.ai_runner_binding b JOIN app.room_slot s ON s.id=b.room_slot_id JOIN app.controller_grant grant_row ON grant_row.binding_id=b.id AND grant_row.binding_epoch=b.binding_epoch AND grant_row.match_seat_id=seat.id
 WHERE b.match_seat_id=seat.id AND b.bootstrap_receipt_id IS NOT NULL AND b.desired_state='running' AND app.ai_slot_ready(s.id) AND grant_row.state='active' AND grant_row.delivery_state='delivered' AND b.ready_deck_revision_id=seat.source_revision_id)) THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='AI opening requires delivered adopted grant and fresh current-epoch readiness';END IF;
 -- Optional ante must have its accepted commitment before an opening exists.
 IF EXISTS(SELECT 1 FROM app.ante_commitment WHERE match_id=m.id AND state NOT IN ('committed','canceled')) THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='ante source is not committed; opening remains reserved'; END IF;
 FOR ante_source IN SELECT * FROM app.ante_commitment WHERE match_id=m.id LOOP
 IF ante_source.terms IS NULL THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='game opening requires source-qualified ante terms'; END IF;
 PERFORM app.check_ante_source(ante_source.id);
 END LOOP;
 PERFORM c.id FROM app.match_clock c JOIN app.match_seat seat ON seat.id=c.match_seat_id WHERE seat.match_id=m.id ORDER BY c.match_seat_id FOR NO KEY UPDATE OF c;
 SELECT coalesce(max(c.clock_revision),0)+1 INTO next_clock FROM app.match_clock c JOIN app.match_seat seat ON seat.id=c.match_seat_id WHERE seat.match_id=m.id;
 INSERT INTO app.game_context(game_id,release_id,profile_id,snapshot_ref,context_revision)
 VALUES(g.id,m.accepted_release_id,m.profile_id,p_opening->>'snapshotRef',p_opening->>'contextRevision') RETURNING id INTO context_id;
 INSERT INTO app.game_checkpoint(game_id,context_id,accepted_revision,private_state_ref,facts_ref,clock_revision)
 VALUES(g.id,context_id,0,p_opening->>'stateRef',p_opening->>'factsRef',next_clock);
 INSERT INTO app.game_event(game_id,context_id,sequence,accepted_revision,private_payload_ref) VALUES(g.id,context_id,0,0,p_opening->>'eventRef');
 UPDATE app.game SET state='active' WHERE id=g.id;
 UPDATE app.match SET state='active',reserved_game_id=NULL,current_game_id=g.id,revision=revision+1 WHERE id=m.id;
 INSERT INTO app.operational_state(game_id,owner_instance_ref,fence_epoch,availability,revision) VALUES(g.id,p_owner,1,'available',0);
 at_time:=clock_timestamp();
 UPDATE app.match_clock c SET paused=true,last_accounted_at=at_time,clock_revision=next_clock FROM app.match_seat s WHERE s.id=c.match_seat_id AND s.match_id=m.id;
 UPDATE app.command_receipt SET normalized_source=jsonb_build_object('version','game-opening-v1')||p_opening WHERE id=receipt.id;
 RETURN app.finish_account_command(receipt.id,true,jsonb_build_object('resourceId',g.id,'contextId',context_id,'revision',(m.revision+1)::text));
END $$;

CREATE OR REPLACE FUNCTION app.reply_between_games(p_decision uuid,p_expected numeric,p_revision uuid,p_reply text,p_qualification text,p_request uuid) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE d app.between_game_decision; m app.match; seat app.match_seat; chosen app.deck_revision; original app.deck_revision; receipt app.command_receipt;
 fingerprint text; game_id uuid; prior_index numeric; current_clock numeric;
BEGIN
 IF current_setting('transaction_isolation')<>'read committed' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='between-game selection requires read committed retry'; END IF;
 IF p_decision IS NULL OR p_request IS NULL OR p_request='00000000-0000-0000-0000-000000000000'::uuid OR p_revision IS NULL OR p_expected IS NULL OR p_expected::text !~ '^(0|[1-9][0-9]{0,77})$'
 OR p_reply IS NULL OR length(btrim(p_reply)) NOT BETWEEN 1 AND 2048 OR p_qualification IS NULL OR length(btrim(p_qualification)) NOT BETWEEN 1 AND 2048 THEN RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid normalized between-game selection'; END IF;
 SELECT * INTO d FROM app.between_game_decision WHERE id=p_decision;
 IF d.id IS NULL THEN RAISE EXCEPTION USING ERRCODE='23503',MESSAGE='between-game decision unavailable'; END IF;
 fingerprint:=app.auth_json_hash(jsonb_build_object('decision',p_decision,'basis',p_expected::text,'revision',p_revision,'reply',p_reply,'qualification',p_qualification));
 INSERT INTO app.command_receipt(principal_key,scope,target_ref,request_ref,payload_hash,state,result_ref)
 VALUES('match-seat:'||d.seat_id::text,'between_game_reply_v1',d.id::text,p_request::text,fingerprint,'pending','') ON CONFLICT DO NOTHING RETURNING * INTO receipt;
 IF receipt.id IS NULL THEN
 SELECT * INTO STRICT receipt FROM app.command_receipt WHERE principal_key='match-seat:'||d.seat_id::text AND scope='between_game_reply_v1' AND request_ref=p_request::text FOR UPDATE;
 IF receipt.payload_hash<>fingerprint OR receipt.target_ref<>d.id::text THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='between-game reply terms changed'; END IF;
 IF receipt.state='pending' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='between-game reply pending'; END IF;
 RETURN receipt.response_payload;
 END IF;
 SELECT * INTO STRICT m FROM app.match WHERE id=d.match_id FOR NO KEY UPDATE;
 SELECT * INTO STRICT d FROM app.between_game_decision WHERE id=d.id FOR NO KEY UPDATE;
 SELECT * INTO STRICT seat FROM app.match_seat WHERE id=d.seat_id FOR SHARE;
 SELECT clock_revision INTO current_clock FROM app.match_clock WHERE match_seat_id=seat.id FOR SHARE;
 IF m.availability<>'available' OR m.revision<>p_expected OR m.state<>'between_games' OR d.state<>'pending' OR d.after_game_id IS NULL OR current_clock IS DISTINCT FROM d.clock_revision
 OR EXISTS(SELECT 1 FROM app.game g JOIN app.game prior ON prior.id=d.after_game_id WHERE g.match_id=m.id AND g.game_index>prior.game_index)
 OR (d.deadline_at IS NOT NULL AND d.deadline_at<=clock_timestamp()) THEN RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('basis','stale or unavailable between-game decision')); END IF;
 SELECT * INTO chosen FROM app.deck_revision WHERE id=p_revision FOR SHARE;
 SELECT * INTO original FROM app.deck_revision WHERE id=seat.source_revision_id FOR SHARE;
 IF chosen.id IS NULL OR chosen.sealed_at IS NULL OR original.id IS NULL OR NOT EXISTS(SELECT 1 FROM app.deck deck WHERE deck.id=chosen.deck_id AND ((seat.controller_kind='human' AND deck.source_kind='account' AND deck.account_id=seat.account_id) OR (seat.controller_kind='ai' AND (deck.source_kind='system_pool' OR deck.account_id=seat.sponsor_id)))) THEN
 RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('deck','unavailable selected revision')); END IF;
 IF EXISTS((SELECT definition_id,printing_id,sum(quantity) FROM app.deck_entry WHERE revision_id=chosen.id GROUP BY definition_id,printing_id EXCEPT SELECT definition_id,printing_id,sum(quantity) FROM app.deck_entry WHERE revision_id=original.id GROUP BY definition_id,printing_id)
 UNION ALL (SELECT definition_id,printing_id,sum(quantity) FROM app.deck_entry WHERE revision_id=original.id GROUP BY definition_id,printing_id EXCEPT SELECT definition_id,printing_id,sum(quantity) FROM app.deck_entry WHERE revision_id=chosen.id GROUP BY definition_id,printing_id)) THEN
 RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('deck','selection changes the immutable original card pool')); END IF;
 -- Fixed legality and execution-release validation are supplied by the trusted
 -- normalized validator. Current collection ownership is deliberately not read.
 UPDATE app.between_game_decision SET state='answered',selected_deck_revision_id=p_revision,reply_ref=p_reply WHERE id=d.id;
 IF NOT EXISTS(SELECT 1 FROM app.between_game_decision WHERE match_id=m.id AND after_game_id=d.after_game_id AND state='pending') THEN
 SELECT game_index INTO STRICT prior_index FROM app.game WHERE id=d.after_game_id;
 INSERT INTO app.game(match_id,game_index,state,accepted_revision) VALUES(m.id,prior_index+1,'reserved',0) RETURNING id INTO game_id;
 UPDATE app.match SET state='ready',reserved_game_id=game_id,revision=revision+1 WHERE id=m.id;
 INSERT INTO app.durable_handoff(match_id,game_id,source_ref,destination,kind,state) VALUES(m.id,game_id,game_id::text,'game_host','reserved_game','pending');
 ELSE UPDATE app.match SET revision=revision+1 WHERE id=m.id; END IF;
 RETURN app.finish_account_command(receipt.id,true,jsonb_build_object('resourceId',d.id,'revision',(m.revision+1)::text,'reservedGameId',coalesce(game_id::text,''),'qualificationRef',p_qualification));
END $$;
COMMIT;
