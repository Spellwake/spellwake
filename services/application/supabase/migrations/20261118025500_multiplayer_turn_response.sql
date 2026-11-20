BEGIN;

CREATE OR REPLACE FUNCTION app.apply_accept_host_candidate(p_receipt uuid, p_game uuid, p_owner text, p_epoch numeric, p_basis numeric, p_request uuid, p_candidate jsonb)
 RETURNS jsonb
 LANGUAGE plpgsql
 SECURITY DEFINER
 SET search_path TO ''
AS $function$
DECLARE source_game app.game; host app.operational_state; receipt app.command_receipt;
 fingerprint text; context_id uuid; checkpoint_id uuid; event_ids uuid[]:='{}'; event_id uuid; result_id uuid;
 item jsonb; next_sequence numeric; next_revision numeric; next_clock numeric; terminal jsonb;
 accepted_at timestamptz; before_clocks jsonb; timeout jsonb; exhausted uuid[]; losing uuid; inherited uuid; discovered_match uuid; seat_count bigint; clock_count bigint;
BEGIN
 PERFORM pg_advisory_xact_lock_shared(hashtextextended('supply-population-cutoff-v1',0));
 SELECT * INTO receipt FROM app.command_receipt WHERE id=p_receipt FOR UPDATE;
 IF receipt.id IS NULL OR receipt.state<>'pending' OR receipt.scope IS DISTINCT FROM 'host_candidate_v1'
  OR receipt.principal_key IS DISTINCT FROM 'host:'||p_game::text OR receipt.target_ref IS DISTINCT FROM p_game::text
  OR receipt.request_ref IS DISTINCT FROM p_request::text
  OR receipt.payload_hash IS DISTINCT FROM app.auth_json_hash(jsonb_build_object('owner',p_owner,'epoch',p_epoch::text,'basis',p_basis::text,'candidate',p_candidate)) THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='locked accept_host_candidate core requires its preclaimed exact identity';END IF;
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
 IF EXISTS(SELECT 1 FROM jsonb_object_keys(p_candidate) k WHERE k NOT IN ('contextId','stateRef','factsRef','events','facts','clocks','terminal','participation','bankTimeout','nativeEliminations','eventSchemaRevision','openingResponseSeatId','turnResponse','turnResponseProtocol'))
 OR jsonb_typeof(p_candidate->'contextId') IS DISTINCT FROM 'string'
 OR (p_candidate->>'contextId') !~ '^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$'
 OR jsonb_typeof(p_candidate->'stateRef') IS DISTINCT FROM 'string' OR length(btrim(p_candidate->>'stateRef')) NOT BETWEEN 1 AND 2048
 OR jsonb_typeof(p_candidate->'factsRef') IS DISTINCT FROM 'string' OR length(btrim(p_candidate->>'factsRef')) NOT BETWEEN 1 AND 2048
 OR jsonb_typeof(p_candidate->'events') IS DISTINCT FROM 'array'
 OR jsonb_typeof(p_candidate->'facts') IS DISTINCT FROM 'array'
 OR jsonb_typeof(p_candidate->'clocks') IS DISTINCT FROM 'array'
 OR jsonb_typeof(p_candidate->'participation') IS DISTINCT FROM 'array' THEN
  RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid candidate artifact fields'; END IF;
 IF p_candidate ? 'eventSchemaRevision' AND (p_candidate->'eventSchemaRevision' IS DISTINCT FROM '"native-transition-v1"'::jsonb AND p_candidate->'eventSchemaRevision' IS DISTINCT FROM '"native-transition-activity-v1"'::jsonb AND p_candidate->'eventSchemaRevision' IS DISTINCT FROM '"native-transition-land-play-v1"'::jsonb) THEN
  RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid candidate event schema';END IF;
 -- New finite native activity codec; legacy generic V1 source behavior is unchanged.
 -- This proves only closed shape and selected human membership, never actual casting.
 IF p_candidate->>'eventSchemaRevision' = 'native-transition-land-play-v1' THEN
  -- The pending original already holds the exact authenticated terms. On replay
  -- the accepted same original supplies them after the native child was linked.
  IF NOT EXISTS (
   SELECT 1 FROM app.command_receipt AS player
   JOIN app.match_seat AS actor ON actor.match_id=source_game.match_id
    AND player.principal_key='native-seat:'||actor.id::text
   WHERE player.id::text=p_request::text AND player.scope='native_player_v1'
    AND player.target_ref=p_game::text AND player.state IN ('pending','accepted')
    AND player.normalized_source->>'actorSeatId'=actor.id::text
    AND player.normalized_source->'original'->'action'->>'kind'='play-land'
    AND player.payload_hash=app.auth_json_hash(jsonb_build_object('actorSeatId',actor.id,'original',player.normalized_source->'original'))
    AND player.normalized_source->'prepared'->'candidate'=p_candidate
    AND ((actor.controller_kind='ai' AND player.controller_grant_id IS NOT NULL AND p_candidate->'facts'='[]'::jsonb)
     OR (actor.controller_kind='human' AND player.account_id=actor.account_id AND p_candidate->'facts'=jsonb_build_array(
      jsonb_build_object('eventIndex','0','accountId',actor.account_id,'kind','native:land-play','revision','native-land-play-v1','value','1'))))
  ) OR jsonb_array_length(p_candidate->'events')<>1 THEN
   RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='land play requires the exact player original and attributed fact';END IF;
 END IF;
 IF p_candidate->'eventSchemaRevision' = '"native-transition-activity-v1"'::jsonb THEN
  IF jsonb_typeof(p_candidate->'events') IS DISTINCT FROM 'array' OR jsonb_typeof(p_candidate->'facts') IS DISTINCT FROM 'array' THEN
   RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid native activity arrays';END IF;
  IF jsonb_array_length(p_candidate->'events')<>1 OR jsonb_array_length(p_candidate->'facts')>4096 THEN
   RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid native activity event count';END IF;
  FOR item IN SELECT value FROM jsonb_array_elements(p_candidate->'facts') LOOP
   IF jsonb_typeof(item) IS DISTINCT FROM 'object'
    OR item->'eventIndex' IS DISTINCT FROM '"0"'::jsonb
    OR (item->'kind' IS DISTINCT FROM '"native:cast-red-or-green"'::jsonb AND item->'kind' IS DISTINCT FROM '"native:cast-black-or-green"'::jsonb)
    OR item->'revision' IS DISTINCT FROM '"native-cast-colors-v1"'::jsonb
    OR jsonb_typeof(item->'value') IS DISTINCT FROM 'string' OR item->>'value' !~ '^[1-9][0-9]{0,19}$'
    OR NOT EXISTS(SELECT 1 FROM app.match_seat s WHERE s.match_id=source_game.match_id AND s.controller_kind='human' AND s.account_id::text=item->>'accountId') THEN
    RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid native cast activity fact';END IF;
   IF (item->>'value')::numeric>18446744073709551615 THEN RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='native cast activity count overflow';END IF;
  END LOOP;
 END IF;

 IF p_candidate ? 'turnResponseProtocol' AND (p_candidate->'turnResponseProtocol' IS DISTINCT FROM '"native-turn-response-v1"'::jsonb
    OR (SELECT count(*) FROM app.match_seat WHERE match_id=source_game.match_id)<3) THEN
  RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid multiplayer response protocol';
 END IF;
 IF p_candidate ? 'turnResponse' THEN
  item:=p_candidate->'turnResponse';
  IF p_candidate->'turnResponseProtocol' IS DISTINCT FROM '"native-turn-response-v1"'::jsonb OR jsonb_typeof(item) IS DISTINCT FROM 'object'
   OR NOT item ?& ARRAY['kind','seatId','remainingSeatIds','sameNativeWait'] OR (item - ARRAY['kind','seatId','remainingSeatIds','sameNativeWait']) <> '{}'::jsonb
   OR (item->>'kind' IN ('priority','decision')) IS DISTINCT FROM true
   OR jsonb_typeof(item->'sameNativeWait') IS DISTINCT FROM 'boolean'
   OR jsonb_typeof(item->'seatId') IS DISTINCT FROM 'string' OR NOT app.native_uuid_is_canonical(item->>'seatId')
   OR jsonb_typeof(item->'remainingSeatIds') IS DISTINCT FROM 'array'
   OR jsonb_array_length(item->'remainingSeatIds') NOT BETWEEN 2 AND 6
   OR NOT (item->'remainingSeatIds' @> jsonb_build_array(item->'seatId'))
   OR (SELECT count(DISTINCT s.id) FROM jsonb_array_elements(item->'remainingSeatIds') v
     JOIN app.match_seat s ON s.match_id=source_game.match_id AND to_jsonb(s.id)=v) <> jsonb_array_length(item->'remainingSeatIds')
   OR (SELECT count(*) FROM app.match_seat WHERE match_id=source_game.match_id)<3
   OR p_candidate ? 'openingResponseSeatId' OR p_candidate->>'terminal' IS NOT NULL THEN
   RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid native turn responder or remaining seats';
  END IF;
 END IF;
 IF p_candidate ? 'openingResponseSeatId' AND (
  NOT app.native_uuid_is_canonical(p_candidate->>'openingResponseSeatId')
  OR NOT EXISTS (SELECT 1 FROM app.match_seat s WHERE s.match_id=source_game.match_id AND s.id::text=p_candidate->>'openingResponseSeatId')
  OR (SELECT count(*) FROM app.match_seat WHERE match_id=source_game.match_id)<3
  OR jsonb_typeof(p_candidate->'openingResponseSeatId') IS DISTINCT FROM 'string'
  OR p_candidate->>'terminal' IS NOT NULL) THEN
  RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid multiplayer opening responder';END IF;
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
   OR EXISTS(SELECT 1 FROM jsonb_object_keys(item) k WHERE k NOT IN ('seatId','clockRevision','remainingMs','paused','eligibility')) THEN
   RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid candidate clock'; END IF;
  PERFORM c.id FROM app.match_clock c JOIN app.match_seat s ON s.id=c.match_seat_id
   WHERE s.id=(item->>'seatId')::uuid AND s.match_id=source_game.match_id
    AND c.clock_revision=(item->>'clockRevision')::numeric AND c.remaining_ms>=(item->>'remainingMs')::numeric FOR NO KEY UPDATE OF c;
  IF NOT FOUND THEN RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('clocks','stale or invalid')); END IF;
  IF EXISTS(SELECT 1 FROM app.match_clock WHERE match_seat_id=(item->>'seatId')::uuid AND policy_ref IN('bo3-overall-bank-v1','bo1-bank-inactive-v1') AND remaining_ms<>(item->>'remainingMs')::numeric) THEN RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('clocks','send the current bank; elapsed debit is authoritative'));END IF;
  IF NOT (item->>'paused')::boolean AND EXISTS(SELECT 1 FROM app.match_clock WHERE match_seat_id=(item->>'seatId')::uuid AND policy_ref='bo1-bank-inactive-v1') THEN RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('clocks','BO1 overall bank is inactive'));END IF;
  IF EXISTS(SELECT 1 FROM app.match_clock WHERE match_seat_id=(item->>'seatId')::uuid AND policy_ref='bo3-overall-bank-v1' AND (NOT (item->>'paused')::boolean OR NOT paused AND terminal IS NULL)) THEN
   IF item->'eligibility' IS NULL OR jsonb_typeof(item->'eligibility') IS DISTINCT FROM 'object' OR (CASE WHEN (item->>'paused')::boolean THEN item->'eligibility'->>'reason' IS DISTINCT FROM 'native_wait' ELSE item->'eligibility'->>'reason' IS DISTINCT FROM 'native_priority' AND item->'eligibility'->>'reason' IS DISTINCT FROM 'native_decision' END) OR(item->'eligibility'->>'reason'='native_decision' AND item->'eligibility'->>'proofRef' IS DISTINCT FROM p_candidate->>'stateRef') OR item->'eligibility'->>'acceptedRevision' IS DISTINCT FROM p_basis::text OR jsonb_typeof(item->'eligibility'->'proofRef') IS DISTINCT FROM 'string' OR length(btrim(item->'eligibility'->>'proofRef')) NOT BETWEEN 1 AND 2048 OR EXISTS(SELECT 1 FROM jsonb_object_keys(item->'eligibility') k WHERE k NOT IN('reason','acceptedRevision','proofRef')) THEN RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='bank eligibility change requires native priority or wait source at current basis';END IF;
  ELSIF item ? 'eligibility' THEN RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='inactive clock cannot claim native priority eligibility';END IF;

 END LOOP;
 IF (SELECT count(DISTINCT v->>'seatId') FROM jsonb_array_elements(p_candidate->'clocks') v)<>seat_count THEN
  RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('clocks','duplicate seat')); END IF;
 SELECT coalesce(max(c.clock_revision),0)+1 INTO next_clock FROM app.match_clock c JOIN app.match_seat s ON s.id=c.match_seat_id WHERE s.match_id=source_game.match_id;
 IF terminal IS NULL AND jsonb_array_length(p_candidate->'participation')<>0 THEN
  RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='only terminal candidates carry final participation'; END IF;
 IF terminal IS NOT NULL AND jsonb_array_length(p_candidate->'participation')<>(SELECT count(*) FROM app.match_seat WHERE match_id=source_game.match_id AND controller_kind='human') THEN
  RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='terminal candidate requires every human participation total'; END IF;
 PERFORM app.check_selected_clock_eligibility(c.id) FROM app.match_clock c JOIN app.match_seat seat ON seat.id=c.match_seat_id WHERE seat.match_id=source_game.match_id AND c.policy_ref='bo3-overall-bank-v1';
 next_revision:=p_basis+1; accepted_at:=clock_timestamp();before_clocks:=app.match_clock_snapshot(source_game.match_id);
 SELECT coalesce(array_agg(c.match_seat_id ORDER BY c.match_seat_id),'{}'::uuid[]) INTO exhausted FROM app.match_clock c JOIN app.match_seat s ON s.id=c.match_seat_id WHERE s.match_id=source_game.match_id AND c.policy_ref='bo3-overall-bank-v1' AND (c.remaining_ms=0 OR NOT c.paused AND app.clock_elapsed_ms(c.last_accounted_at,accepted_at)>=c.remaining_ms);
 timeout:=p_candidate->'bankTimeout';IF timeout='null'::jsonb THEN timeout:=NULL;END IF;
 IF timeout IS NULL THEN
 IF cardinality(exhausted)>0 THEN RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('clocks','overall bank requires its timeout transition'));END IF;
 ELSE
 IF jsonb_typeof(timeout) IS DISTINCT FROM 'object' OR timeout->>'kind' IS DISTINCT FROM 'overall_bank' OR jsonb_typeof(timeout->'seatId') IS DISTINCT FROM 'string' OR timeout->>'seatId' !~ '^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$' OR jsonb_typeof(timeout->'clockRevision') IS DISTINCT FROM 'string' OR timeout->>'clockRevision' !~ '^(0|[1-9][0-9]{0,77})$' OR jsonb_typeof(timeout->'proofRef') IS DISTINCT FROM 'string' OR length(btrim(timeout->>'proofRef')) NOT BETWEEN 1 AND 2048 OR EXISTS(SELECT 1 FROM jsonb_object_keys(timeout) k WHERE k NOT IN('kind','seatId','clockRevision','proofRef')) THEN RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid normalized bank-timeout source';END IF;
 losing:=(timeout->>'seatId')::uuid;
 IF cardinality(exhausted)=0 THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='bank timeout is not due at this acceptance boundary';END IF;
 IF exhausted IS DISTINCT FROM ARRAY[losing] OR seat_count<>2 OR NOT EXISTS(SELECT 1 FROM app.match WHERE id=source_game.match_id AND structure='bo3') OR terminal IS NULL OR terminal->>'outcome'<>'win' OR jsonb_array_length(terminal->'winnerSeatIds')<>1 OR terminal->'winnerSeatIds' @> jsonb_build_array(losing::text) THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='bank timeout requires one exhausted loser and actual sole opposing native winner';END IF;
 PERFORM app.check_selected_clock_eligibility(c.id) FROM app.match_clock c JOIN app.match_seat s ON s.id=c.match_seat_id WHERE s.match_id=source_game.match_id AND c.policy_ref='bo3-overall-bank-v1';
 SELECT c.eligibility_receipt_id INTO inherited FROM app.match_clock c WHERE c.match_seat_id=losing AND c.clock_revision=(timeout->>'clockRevision')::numeric AND c.policy_ref='bo3-overall-bank-v1' AND (NOT c.paused OR c.remaining_ms=0 AND app.bank_exhaustion_source(c.eligibility_receipt_id,losing));
 IF NOT FOUND THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='bank timeout clock basis or inherited exhaustion source differs';END IF;
 timeout:=timeout||jsonb_build_object('priorClockSourceId',inherited);
 END IF;
 UPDATE app.game SET accepted_revision=next_revision,state=CASE WHEN terminal IS NULL THEN 'active' WHEN terminal->>'outcome'='no_contest' THEN 'no_contest' ELSE 'completed' END WHERE id=p_game;
 FOR item IN SELECT v FROM jsonb_array_elements(p_candidate->'clocks') v ORDER BY v->>'seatId' LOOP
  UPDATE app.match_clock SET remaining_ms=CASE WHEN policy_ref='bo3-overall-bank-v1' AND NOT paused THEN greatest(0,remaining_ms-app.clock_elapsed_ms(last_accounted_at,accepted_at)) ELSE (item->>'remainingMs')::numeric END,clock_revision=next_clock,
   paused=CASE WHEN terminal IS NOT NULL THEN true ELSE (item->>'paused')::boolean END,last_accounted_at=accepted_at,eligibility_receipt_id=CASE WHEN policy_ref='bo3-overall-bank-v1' THEN receipt.id ELSE eligibility_receipt_id END
   WHERE match_seat_id=(item->>'seatId')::uuid;
 END LOOP;
 INSERT INTO app.game_checkpoint(game_id,context_id,accepted_revision,private_state_ref,facts_ref,clock_revision,source_receipt_id)
 VALUES(p_game,context_id,next_revision,p_candidate->>'stateRef',p_candidate->>'factsRef',next_clock,receipt.id) RETURNING id INTO checkpoint_id;
 SELECT coalesce(max(sequence),-1)+1 INTO next_sequence FROM app.game_event WHERE game_id=p_game;
 FOR item IN SELECT * FROM jsonb_array_elements(p_candidate->'events') LOOP
  IF jsonb_typeof(item) IS DISTINCT FROM 'string' OR length(btrim(item#>>'{}')) NOT BETWEEN 1 AND 2048 THEN
   RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid private event artifact'; END IF;
  INSERT INTO app.game_event(game_id,context_id,sequence,accepted_revision,private_payload_ref,source_receipt_id)
  VALUES(p_game,context_id,next_sequence,next_revision,item#>>'{}',receipt.id) RETURNING id INTO event_id;
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
  INSERT INTO app.game_result(game_id,source_event_id,outcome,winners_ref,end_reason,winner_seat_ids,bank_forfeit_receipt_id,bank_forfeit_seat_id)
  VALUES(p_game,event_ids[cardinality(event_ids)],terminal->>'outcome',terminal->>'winnersRef',terminal->>'reason',ARRAY(SELECT x::uuid FROM jsonb_array_elements_text(terminal->'winnerSeatIds') x),CASE WHEN timeout IS NOT NULL THEN receipt.id END,losing) RETURNING id INTO result_id;
  INSERT INTO app.durable_handoff(match_id,game_id,source_ref,destination,kind,state)
  VALUES(source_game.match_id,p_game,result_id::text,'match_lifecycle','game_outcome','pending');
 END IF;
 UPDATE app.command_receipt SET normalized_source=jsonb_build_object('version','host-clock-transition-v1','gameId',p_game,'matchId',source_game.match_id,'basisRevision',p_basis::text,'acceptedRevision',next_revision::text,'acceptedAt',to_char(accepted_at AT TIME ZONE 'UTC','YYYY-MM-DD"T"HH24:MI:SS.US"Z"'),'before',before_clocks,'after',app.match_clock_snapshot(source_game.match_id),'owner',p_owner,'epoch',p_epoch::text,'candidate',p_candidate,'eventIds',to_jsonb(event_ids),'bankTimeout',timeout,'gameResultId',result_id) WHERE id=receipt.id;
 PERFORM app.record_native_eliminations(receipt.id);
 RETURN app.finish_account_command(receipt.id,true,jsonb_build_object('resourceId',checkpoint_id,'revision',next_revision::text));
END $function$;

CREATE FUNCTION app.native_turn_wait_window(p_game uuid,p_revision numeric)
RETURNS TABLE(seat_id uuid,decision_ref text,expires_at timestamptz,start_revision numeric)
LANGUAGE sql STABLE SECURITY DEFINER SET search_path='' AS $$
 WITH RECURSIVE steps AS NOT MATERIALIZED (
  SELECT cp.accepted_revision AS rev,cp.private_state_ref,
    child.normalized_source->'candidate'->'turnResponse' AS wait,
    app.financial_instant(child.normalized_source->>'acceptedAt') AS started,
    parent.scope,parent.normalized_source AS original
  FROM app.game_checkpoint cp JOIN app.command_receipt child ON child.id=cp.source_receipt_id
   AND child.scope='host_candidate_v1' AND child.state='accepted'
  JOIN app.command_receipt parent ON parent.id::text=child.request_ref AND parent.state='accepted'
   AND parent.scope IN ('native_player_v1','native_host_input_v1') AND parent.target_ref=p_game::text
   AND parent.normalized_source->>'childReceiptId'=child.id::text
  WHERE cp.game_id=p_game AND child.normalized_source->'candidate' ? 'turnResponse'
 ), span AS (
  SELECT s.*,s.private_state_ref AS tip FROM steps s WHERE s.rev=p_revision
  UNION ALL
  SELECT prev.*,cur.tip FROM span cur JOIN steps prev ON prev.rev=cur.rev-1
   AND prev.wait->>'seatId'=cur.wait->>'seatId' AND prev.wait->>'kind'=cur.wait->>'kind'
  WHERE cur.wait->'sameNativeWait'='true'::jsonb AND ((cur.scope='native_host_input_v1' AND cur.original->'original'->>'kind'='entropy')
     OR (cur.scope='native_player_v1' AND cur.original->'original'->'action'->>'kind'='concede'
         AND cur.original->>'actorSeatId'<>cur.wait->>'seatId'))
 )
 SELECT (s.wait->>'seatId')::uuid,s.tip,s.started+interval '120 seconds',s.rev FROM span s ORDER BY s.rev LIMIT 1
$$;
REVOKE ALL ON FUNCTION app.native_turn_wait_window(uuid,numeric) FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;

CREATE FUNCTION app.native_match_wait_window(p_game uuid,p_revision numeric)
RETURNS TABLE(seat_id uuid,decision_ref text,expires_at timestamptz,start_revision numeric)
LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path='' AS $$
BEGIN
 RETURN QUERY SELECT * FROM app.native_turn_wait_window(p_game,p_revision);
 IF FOUND THEN RETURN; END IF;
 RETURN QUERY SELECT * FROM app.native_opening_wait_window(p_game,p_revision);
END $$;
REVOKE ALL ON FUNCTION app.native_match_wait_window(uuid,numeric) FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;

CREATE OR REPLACE FUNCTION app.native_response_wait_for_fence(p_game uuid,p_revision numeric,p_fence numeric)
RETURNS TABLE(seat_id uuid,decision_ref text,expires_at timestamptz)
LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path='' AS $$
DECLARE w record; span record; reached boolean:=false;
BEGIN
 IF p_fence IS NULL OR p_fence<1 THEN RETURN; END IF;
 SELECT * INTO w FROM app.native_match_wait_window(p_game,p_revision);
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

CREATE OR REPLACE FUNCTION app.native_response_wait_at(p_game uuid,p_revision numeric)
RETURNS TABLE(seat_id uuid,decision_ref text,expires_at timestamptz)
LANGUAGE sql STABLE SECURITY DEFINER SET search_path='' AS $$
 SELECT w.seat_id,w.decision_ref,w.expires_at FROM app.native_match_wait_window(p_game,p_revision) w
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
 SELECT ARRAY(SELECT v::uuid FROM jsonb_array_elements_text(c.normalized_source->'candidate'->'turnResponse'->'remainingSeatIds') v
       WHERE v<>timing.seat_id::text ORDER BY v)
 INTO survivors FROM app.game_checkpoint cp JOIN app.command_receipt c ON c.id=cp.source_receipt_id
 WHERE cp.game_id=g AND cp.accepted_revision=(src->'original'->'basis'->>'revision')::numeric
  AND c.normalized_source->'candidate' ? 'turnResponse';
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
    'eventSchemaRevision',marker,'events',events)||CASE WHEN body ? 'turnResponseProtocol' THEN jsonb_build_object('turnResponseProtocol',body->'turnResponseProtocol') ELSE '{}'::jsonb END||CASE WHEN body ? 'turnResponse' THEN jsonb_build_object('turnResponse',body->'turnResponse') ELSE '{}'::jsonb END||CASE WHEN body ? 'openingResponseSeatId' THEN jsonb_build_object('openingResponseSeatId',body->'openingResponseSeatId') ELSE '{}'::jsonb END||CASE WHEN bank_evidence IS NULL THEN '{}'::jsonb ELSE jsonb_build_object('bankExpiry',bank_evidence) END||CASE WHEN response_evidence IS NULL THEN '{}'::jsonb ELSE jsonb_build_object('openingResponseExpiry',response_evidence) END);
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
   IF p_original->'action'->>'kind'<>'concede' THEN
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
   IF original->'action'->>'kind'='concede'
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
