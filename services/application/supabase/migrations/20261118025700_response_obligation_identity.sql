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

 -- Once this game opts in, an older host must not silently discard its clock.
 IF EXISTS(SELECT 1 FROM app.game_checkpoint cp JOIN app.command_receipt prior ON prior.id=cp.source_receipt_id
  WHERE cp.game_id=p_game AND cp.accepted_revision=p_basis AND prior.normalized_source->'candidate' ? 'turnResponseProtocol'
   AND (NOT p_candidate ? 'turnResponseProtocol' OR (prior.normalized_source->'candidate'->>'turnResponseProtocol'='native-turn-response-v2'
     AND p_candidate->>'turnResponseProtocol' IS DISTINCT FROM 'native-turn-response-v2'))) THEN
  RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='multiplayer response protocol regression';
 END IF;
 IF p_candidate->>'turnResponseProtocol' IS DISTINCT FROM 'native-turn-response-v2' AND EXISTS(
  SELECT 1 FROM app.game_checkpoint cp JOIN app.native_response_adoption adopted ON adopted.checkpoint_id=cp.id
  WHERE cp.game_id=p_game AND cp.accepted_revision=p_basis) THEN
  RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='adopted response requires the current protocol';
 END IF;
 IF p_candidate ? 'turnResponseProtocol' AND ((p_candidate->>'turnResponseProtocol' IN ('native-turn-response-v1','native-turn-response-v2')) IS DISTINCT FROM true
    OR (SELECT count(*) FROM app.match_seat WHERE match_id=source_game.match_id)<3) THEN
  RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid multiplayer response protocol';
 END IF;
 IF p_candidate ? 'turnResponse' THEN
  item:=p_candidate->'turnResponse';
  IF (p_candidate->>'turnResponseProtocol' IN ('native-turn-response-v1','native-turn-response-v2')) IS DISTINCT FROM true OR jsonb_typeof(item) IS DISTINCT FROM 'object'
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

COMMIT;
