BEGIN;
ALTER TABLE app.game_result ADD COLUMN bank_forfeit_receipt_id uuid REFERENCES app.command_receipt(id) ON DELETE RESTRICT;
ALTER TABLE app.game_result ADD COLUMN bank_forfeit_seat_id uuid REFERENCES app.match_seat(id) ON DELETE RESTRICT;
ALTER TABLE app.game_result ADD CONSTRAINT bank_forfeit_source_pair CHECK((bank_forfeit_receipt_id IS NULL)=(bank_forfeit_seat_id IS NULL));
ALTER TABLE app.game_result ADD CONSTRAINT bank_forfeit_winner_shape CHECK(bank_forfeit_receipt_id IS NULL OR (outcome='win' AND winner_seat_ids IS NOT NULL AND cardinality(winner_seat_ids)=1 AND NOT bank_forfeit_seat_id=ANY(winner_seat_ids)));
ALTER TABLE app.match_result DROP CONSTRAINT match_result_terminal_kind_check;
ALTER TABLE app.match_result ADD CONSTRAINT match_result_terminal_kind_check CHECK(terminal_kind IN('game_outcome','unrecoverable_incident','bank_forfeit'));
ALTER TABLE app.match_result DROP CONSTRAINT match_result_terminal_tuple;
ALTER TABLE app.match_result ADD CONSTRAINT match_result_terminal_tuple CHECK((terminal_kind IN('unrecoverable_incident','bank_forfeit'))=(terminal_receipt_id IS NOT NULL));
-- An outage can itself commit the exhausted balance. Restoration must not
-- erase that zero or require the now-paused seat to become eligible again.
CREATE FUNCTION app.bank_exhaustion_source(p_source uuid,p_seat uuid) RETURNS boolean LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path='' AS $$
DECLARE source app.command_receipt; before_clock jsonb; after_clock jsonb; prior_id uuid; target text; last_revision numeric; revision numeric;
BEGIN
 SELECT * INTO source FROM app.command_receipt WHERE id=p_source;target:=source.target_ref;
 LOOP
 IF source.id IS NULL OR source.state<>'accepted' OR source.scope NOT IN('game_incident_v1','game_restoration_v1') OR source.target_ref IS DISTINCT FROM target THEN RETURN false;END IF;
 SELECT value INTO after_clock FROM jsonb_array_elements(source.normalized_source->'clockValues') value WHERE value->>'seatId'=p_seat::text;
 IF after_clock IS NULL OR after_clock->>'remainingMs' IS DISTINCT FROM '0' OR after_clock->'paused' IS DISTINCT FROM 'true'::jsonb OR after_clock->>'clockRevision' IS NULL OR after_clock->>'clockRevision' !~ '^(0|[1-9][0-9]{0,77})$' THEN RETURN false;END IF;
 revision:=(after_clock->>'clockRevision')::numeric;
 IF last_revision IS NOT NULL AND revision>=last_revision THEN RETURN false;END IF;last_revision:=revision;
 IF source.scope='game_restoration_v1' THEN prior_id:=(source.normalized_source->>'incidentId')::uuid;
 ELSE
 SELECT value INTO before_clock FROM jsonb_array_elements(source.normalized_source->'beforeClocks') value WHERE value->>'seatId'=p_seat::text;
 IF before_clock IS NULL THEN RETURN false;END IF;
 IF before_clock->'paused'='false'::jsonb AND (before_clock->>'remainingMs')::numeric>0 THEN RETURN (before_clock->>'remainingMs')::numeric<=app.clock_elapsed_ms((before_clock->>'lastAccountedAt')::timestamptz,(after_clock->>'lastAccountedAt')::timestamptz);END IF;
 IF before_clock->'paused' IS DISTINCT FROM 'true'::jsonb OR before_clock->>'remainingMs' IS DISTINCT FROM '0' THEN RETURN false;END IF;
 SELECT (value->>'receiptId')::uuid INTO prior_id FROM jsonb_array_elements(source.normalized_source->'clockSources') value WHERE value->>'seatId'=p_seat::text;
 END IF;
 SELECT * INTO source FROM app.command_receipt WHERE id=prior_id;
 END LOOP;
END $$;
CREATE OR REPLACE FUNCTION app.accept_host_candidate(p_game uuid,p_owner text,p_epoch numeric,p_basis numeric,p_request uuid,p_candidate jsonb)
RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE source_game app.game; host app.operational_state; receipt app.command_receipt;
 fingerprint text; context_id uuid; checkpoint_id uuid; event_ids uuid[]:='{}'; event_id uuid; result_id uuid;
 item jsonb; next_sequence numeric; next_revision numeric; next_clock numeric; terminal jsonb;
 accepted_at timestamptz; before_clocks jsonb; timeout jsonb; exhausted uuid[]; losing uuid; inherited uuid; discovered_match uuid; seat_count bigint; clock_count bigint;
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
 IF EXISTS(SELECT 1 FROM jsonb_object_keys(p_candidate) k WHERE k NOT IN ('contextId','stateRef','factsRef','events','facts','clocks','terminal','participation','bankTimeout'))
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
   OR EXISTS(SELECT 1 FROM jsonb_object_keys(item) k WHERE k NOT IN ('seatId','clockRevision','remainingMs','paused','eligibility')) THEN
   RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid candidate clock'; END IF;
  PERFORM c.id FROM app.match_clock c JOIN app.match_seat s ON s.id=c.match_seat_id
   WHERE s.id=(item->>'seatId')::uuid AND s.match_id=source_game.match_id
    AND c.clock_revision=(item->>'clockRevision')::numeric AND c.remaining_ms>=(item->>'remainingMs')::numeric FOR NO KEY UPDATE OF c;
  IF NOT FOUND THEN RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('clocks','stale or invalid')); END IF;
  IF EXISTS(SELECT 1 FROM app.match_clock WHERE match_seat_id=(item->>'seatId')::uuid AND policy_ref IN('bo3-overall-bank-v1','bo1-bank-inactive-v1') AND remaining_ms<>(item->>'remainingMs')::numeric) THEN RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('clocks','send the current bank; elapsed debit is authoritative'));END IF;
  IF NOT (item->>'paused')::boolean AND EXISTS(SELECT 1 FROM app.match_clock WHERE match_seat_id=(item->>'seatId')::uuid AND policy_ref='bo1-bank-inactive-v1') THEN RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('clocks','BO1 overall bank is inactive'));END IF;
  IF EXISTS(SELECT 1 FROM app.match_clock WHERE match_seat_id=(item->>'seatId')::uuid AND policy_ref='bo3-overall-bank-v1' AND (NOT (item->>'paused')::boolean OR NOT paused AND terminal IS NULL)) THEN
   IF item->'eligibility' IS NULL OR jsonb_typeof(item->'eligibility') IS DISTINCT FROM 'object' OR item->'eligibility'->>'reason' IS DISTINCT FROM (CASE WHEN (item->>'paused')::boolean THEN 'native_wait' ELSE 'native_priority' END) OR item->'eligibility'->>'acceptedRevision' IS DISTINCT FROM p_basis::text OR jsonb_typeof(item->'eligibility'->'proofRef') IS DISTINCT FROM 'string' OR length(btrim(item->'eligibility'->>'proofRef')) NOT BETWEEN 1 AND 2048 OR EXISTS(SELECT 1 FROM jsonb_object_keys(item->'eligibility') k WHERE k NOT IN('reason','acceptedRevision','proofRef')) THEN RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='bank eligibility change requires native priority or wait source at current basis';END IF;
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
  INSERT INTO app.game_result(game_id,source_event_id,outcome,winners_ref,end_reason,winner_seat_ids,bank_forfeit_receipt_id,bank_forfeit_seat_id)
  VALUES(p_game,event_ids[cardinality(event_ids)],terminal->>'outcome',terminal->>'winnersRef',terminal->>'reason',ARRAY(SELECT x::uuid FROM jsonb_array_elements_text(terminal->'winnerSeatIds') x),CASE WHEN timeout IS NOT NULL THEN receipt.id END,losing) RETURNING id INTO result_id;
  INSERT INTO app.durable_handoff(match_id,game_id,source_ref,destination,kind,state)
  VALUES(source_game.match_id,p_game,result_id::text,'match_lifecycle','game_outcome','pending');
 END IF;
 UPDATE app.command_receipt SET normalized_source=jsonb_build_object('version','host-clock-transition-v1','gameId',p_game,'matchId',source_game.match_id,'basisRevision',p_basis::text,'acceptedRevision',next_revision::text,'acceptedAt',to_char(accepted_at AT TIME ZONE 'UTC','YYYY-MM-DD"T"HH24:MI:SS.US"Z"'),'before',before_clocks,'after',app.match_clock_snapshot(source_game.match_id),'requestedClocks',p_candidate->'clocks','bankTimeout',timeout,'gameResultId',result_id) WHERE id=receipt.id;
 RETURN app.finish_account_command(receipt.id,true,jsonb_build_object('resourceId',checkpoint_id,'revision',next_revision::text));
END $$;

CREATE FUNCTION app.check_bank_forfeit_game(p_game uuid) RETURNS void LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path='' AS $$
DECLARE result app.game_result; g app.game; m app.match; source app.command_receipt; timeout jsonb; before_clock jsonb; after_clock jsonb; terminal app.match_result;
BEGIN
 SELECT * INTO result FROM app.game_result WHERE game_id=p_game;
 IF result.id IS NULL OR result.bank_forfeit_receipt_id IS NULL THEN RETURN;END IF;
 SELECT * INTO g FROM app.game WHERE id=p_game;SELECT * INTO m FROM app.match WHERE id=g.match_id;
 SELECT * INTO source FROM app.command_receipt WHERE id=result.bank_forfeit_receipt_id;timeout:=source.normalized_source->'bankTimeout';
 SELECT value INTO before_clock FROM jsonb_array_elements(source.normalized_source->'before') value WHERE value->>'seatId'=result.bank_forfeit_seat_id::text;
 SELECT value INTO after_clock FROM jsonb_array_elements(source.normalized_source->'after') value WHERE value->>'seatId'=result.bank_forfeit_seat_id::text;
 IF source.id IS NULL OR source.state<>'accepted' OR source.scope<>'host_candidate_v1' OR source.principal_key IS DISTINCT FROM 'host:'||g.id::text OR source.target_ref IS DISTINCT FROM g.id::text OR source.normalized_source->>'version' IS DISTINCT FROM 'host-clock-transition-v1' OR source.normalized_source->>'gameId' IS DISTINCT FROM g.id::text OR source.normalized_source->>'matchId' IS DISTINCT FROM m.id::text OR source.normalized_source->>'gameResultId' IS DISTINCT FROM result.id::text
 OR timeout IS NULL OR timeout->>'kind' IS DISTINCT FROM 'overall_bank' OR timeout->>'seatId' IS DISTINCT FROM result.bank_forfeit_seat_id::text OR timeout->>'clockRevision' IS DISTINCT FROM before_clock->>'clockRevision' OR jsonb_typeof(timeout->'proofRef') IS DISTINCT FROM 'string' OR length(btrim(timeout->>'proofRef')) NOT BETWEEN 1 AND 2048
 OR m.structure<>'bo3' OR (SELECT count(*) FROM app.match_seat WHERE match_id=m.id)<>2 OR result.outcome<>'win' OR result.winner_seat_ids IS NULL OR cardinality(result.winner_seat_ids)<>1 OR result.bank_forfeit_seat_id=ANY(result.winner_seat_ids) OR NOT EXISTS(SELECT 1 FROM app.match_seat WHERE id=result.bank_forfeit_seat_id AND match_id=m.id)
 OR EXISTS(SELECT 1 FROM jsonb_array_elements(source.normalized_source->'after') value WHERE value->>'seatId'<>result.bank_forfeit_seat_id::text AND value->>'remainingMs'='0') OR before_clock IS NULL OR after_clock IS NULL OR after_clock->>'remainingMs' IS DISTINCT FROM '0' OR after_clock->'paused' IS DISTINCT FROM 'true'::jsonb OR after_clock->>'lastAccountedAt' IS DISTINCT FROM source.normalized_source->>'acceptedAt'
 OR NOT ((before_clock->'paused'='false'::jsonb AND (before_clock->>'remainingMs')::numeric<=app.clock_elapsed_ms((before_clock->>'lastAccountedAt')::timestamptz,(after_clock->>'lastAccountedAt')::timestamptz)) OR (before_clock->'paused'='true'::jsonb AND before_clock->>'remainingMs'='0' AND app.bank_exhaustion_source((timeout->>'priorClockSourceId')::uuid,result.bank_forfeit_seat_id)))
 OR NOT EXISTS(SELECT 1 FROM app.game_event e WHERE e.id=result.source_event_id AND e.game_id=g.id AND e.accepted_revision=g.accepted_revision)
 THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='bank-forfeit result requires exact exhausted clock and native terminal source';END IF;
 IF EXISTS(SELECT 1 FROM app.game other WHERE other.match_id=m.id AND other.id<>g.id AND (other.state NOT IN('completed','no_contest') OR NOT EXISTS(SELECT 1 FROM app.game_result WHERE game_id=other.id))) OR EXISTS(SELECT 1 FROM app.between_game_decision WHERE match_id=m.id AND state='pending') THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='bank timeout cannot retain unplayed games or pending sideboard work';END IF;
 SELECT * INTO terminal FROM app.match_result WHERE match_id=m.id;
 IF g.state<>'completed' OR (terminal.id IS NULL AND (m.state<>'active' OR m.current_game_id IS DISTINCT FROM g.id OR m.reserved_game_id IS NOT NULL)) OR (terminal.id IS NOT NULL AND (m.state<>'completed' OR m.current_game_id IS NOT NULL OR m.reserved_game_id IS NOT NULL OR terminal.final_game_ids IS DISTINCT FROM (SELECT array_agg(id ORDER BY game_index) FROM app.game WHERE match_id=m.id))) THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='bank-forfeit native and match terminal frontiers cannot be reopened';END IF;
 IF terminal.id IS NOT NULL AND (terminal.terminal_kind<>'bank_forfeit' OR terminal.terminal_receipt_id IS DISTINCT FROM source.id OR terminal.source_game_result_id IS DISTINCT FROM result.id OR terminal.winner_seat_ids IS DISTINCT FROM result.winner_seat_ids OR terminal.outcome<>'win' OR NOT g.id=ANY(terminal.final_game_ids)) THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='bank forfeit must finalize its actual match as a win';END IF;
END $$;
CREATE FUNCTION app.guard_bank_forfeit_source() RETURNS trigger LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE target uuid; source app.command_receipt;
BEGIN
 IF TG_TABLE_NAME='game_result' THEN target:=NEW.game_id;
 ELSIF TG_TABLE_NAME='game' THEN target:=NEW.id;
 ELSIF TG_TABLE_NAME='match_result' THEN
 IF NEW.terminal_kind='bank_forfeit' AND NOT EXISTS(SELECT 1 FROM app.game_result r JOIN app.game g ON g.id=r.game_id WHERE r.id=NEW.source_game_result_id AND g.match_id=NEW.match_id AND r.bank_forfeit_receipt_id=NEW.terminal_receipt_id) THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='bank match result requires its actual native game timeout';END IF;
 SELECT game_id INTO target FROM app.game_result WHERE id=NEW.source_game_result_id;
 ELSE
 SELECT * INTO source FROM app.command_receipt WHERE id=NEW.id;
 IF source.normalized_source->'bankTimeout' IS NULL OR source.normalized_source->'bankTimeout'='null'::jsonb THEN RETURN NULL;END IF;
 IF source.state<>'accepted' OR NOT EXISTS(SELECT 1 FROM app.game_result r WHERE r.id::text=source.normalized_source->>'gameResultId' AND r.bank_forfeit_receipt_id=source.id) THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='bank timeout receipt requires its exact persisted result';END IF;
 target:=(source.normalized_source->>'gameId')::uuid;
 END IF;
 PERFORM app.check_bank_forfeit_game(target);RETURN NULL;
END $$;
CREATE CONSTRAINT TRIGGER bank_forfeit_game_result AFTER INSERT ON app.game_result DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION app.guard_bank_forfeit_source();
CREATE CONSTRAINT TRIGGER bank_forfeit_game_parent AFTER UPDATE ON app.game DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION app.guard_bank_forfeit_source();
CREATE CONSTRAINT TRIGGER bank_forfeit_match_result AFTER INSERT ON app.match_result DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION app.guard_bank_forfeit_source();
CREATE CONSTRAINT TRIGGER bank_forfeit_receipt AFTER INSERT OR UPDATE ON app.command_receipt DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION app.guard_bank_forfeit_source();
REVOKE ALL ON FUNCTION app.bank_exhaustion_source(uuid,uuid),app.check_bank_forfeit_game(uuid),app.guard_bank_forfeit_source() FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;

CREATE OR REPLACE FUNCTION app.consume_game_outcome(p_result uuid,p_expected numeric) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE r app.game_result; g app.game; m app.match; receipt app.command_receipt; terminal boolean; winners uuid[];
 final_games uuid[]; result_id uuid; sequence_value numeric; accepted_at timestamptz; source_season app.season; pending_ids uuid[];
BEGIN
 IF current_setting('transaction_isolation')<>'read committed' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='outcome consumption requires read committed retry'; END IF;
 IF p_result IS NULL OR p_expected IS NULL OR p_expected::text !~ '^(0|[1-9][0-9]{0,77})$' THEN RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid outcome source basis'; END IF;
 SELECT * INTO r FROM app.game_result WHERE id=p_result;
 SELECT * INTO g FROM app.game WHERE id=r.game_id;
 IF r.id IS NULL OR g.id IS NULL THEN RAISE EXCEPTION USING ERRCODE='23503',MESSAGE='game outcome unavailable'; END IF;
 INSERT INTO app.command_receipt(principal_key,scope,target_ref,request_ref,payload_hash,state,result_ref)
 VALUES('match-lifecycle:'||g.match_id::text,'game_outcome_v1',r.id::text,r.id::text,app.auth_json_hash(jsonb_build_object('resultId',r.id)),'pending','') ON CONFLICT DO NOTHING RETURNING * INTO receipt;
 IF receipt.id IS NULL THEN
 SELECT * INTO STRICT receipt FROM app.command_receipt WHERE principal_key='match-lifecycle:'||g.match_id::text AND scope='game_outcome_v1' AND request_ref=r.id::text FOR UPDATE;
 IF receipt.state='pending' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='outcome consumption pending'; END IF;
 RETURN receipt.response_payload;
 END IF;
 SELECT * INTO STRICT m FROM app.match WHERE id=g.match_id FOR NO KEY UPDATE;
 SELECT * INTO STRICT g FROM app.game WHERE id=g.id FOR SHARE;
 SELECT * INTO STRICT r FROM app.game_result WHERE id=r.id FOR SHARE;
 IF m.revision<>p_expected OR m.state<>'active' OR m.current_game_id IS DISTINCT FROM g.id OR g.state NOT IN ('completed','no_contest') OR r.winner_seat_ids IS NULL THEN
 -- A stale delivery has no permanent consumption disposition; the same immutable
 -- source can be retried after obtaining the actual lifecycle basis.
 RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='stale or unqualified game outcome; retry from current basis'; END IF;
 PERFORM app.validate_normalized_winners(m.id,r.outcome,r.winner_seat_ids);
 IF m.structure NOT IN ('bo1','bo3') OR (m.structure='bo3' AND (SELECT count(*) FROM app.match_seat WHERE match_id=m.id)<>2) THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='unsupported match scoring structure'; END IF;
 PERFORM app.check_bank_forfeit_game(r.game_id);
 terminal:=m.structure='bo1' OR r.outcome='no_contest' OR r.bank_forfeit_receipt_id IS NOT NULL;winners:=r.winner_seat_ids;
 IF NOT terminal AND r.outcome='win' THEN
  SELECT coalesce(array_agg(s.id ORDER BY s.id),'{}'::uuid[]) INTO winners FROM app.match_seat s WHERE s.match_id=m.id AND
   (SELECT count(*) FROM app.game_result past JOIN app.game pg ON pg.id=past.game_id WHERE pg.match_id=m.id AND past.outcome='win' AND s.id=ANY(past.winner_seat_ids)
    AND (past.id=r.id OR EXISTS(SELECT 1 FROM app.command_receipt used WHERE used.principal_key='match-lifecycle:'||m.id::text AND used.scope='game_outcome_v1' AND used.target_ref=past.id::text AND used.state='accepted')))>=2;
  terminal:=cardinality(winners)>0;
 END IF;
 IF terminal THEN
  SELECT array_agg(pg.id ORDER BY pg.game_index) INTO final_games FROM app.game pg JOIN app.game_result past ON past.game_id=pg.id WHERE pg.match_id=m.id AND
   (past.id=r.id OR EXISTS(SELECT 1 FROM app.command_receipt used WHERE used.principal_key='match-lifecycle:'||m.id::text AND used.scope='game_outcome_v1' AND used.target_ref=past.id::text AND used.state='accepted'));
  IF m.competitive THEN
   SELECT next_competitive_sequence INTO sequence_value FROM app.format WHERE id=m.format_id FOR NO KEY UPDATE;
   SELECT * INTO source_season FROM app.season WHERE start_at<=clock_timestamp() AND end_at>clock_timestamp() FOR SHARE;
   IF source_season.id IS NULL THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='competitive completion requires the configured season'; END IF;
   accepted_at:=clock_timestamp();
   IF accepted_at>=source_season.end_at THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='season boundary crossed during source fence; retry'; END IF;
   UPDATE app.format SET next_competitive_sequence=next_competitive_sequence+1 WHERE id=m.format_id;
  ELSE accepted_at:=clock_timestamp(); END IF;
  INSERT INTO app.match_result(match_id,source_ref,outcome,winners_ref,committed_at,ranking_format_id,rank_sequence,winner_seat_ids,final_game_ids,source_game_result_id,terminal_kind,terminal_receipt_id)
  VALUES(m.id,'game-result:'||r.id::text,r.outcome,'normalized:'||r.id::text,accepted_at,CASE WHEN m.competitive THEN m.format_id END,sequence_value,winners,final_games,r.id,CASE WHEN r.bank_forfeit_receipt_id IS NOT NULL THEN 'bank_forfeit' ELSE 'game_outcome' END,r.bank_forfeit_receipt_id) RETURNING id INTO result_id;
  UPDATE app.match SET state=CASE WHEN r.outcome='no_contest' THEN 'no_contest' ELSE 'completed' END,current_game_id=NULL,revision=revision+1 WHERE id=m.id;
  INSERT INTO app.durable_handoff(match_id,game_id,source_ref,destination,kind,state) VALUES(m.id,g.id,result_id::text,'match_settlement','match_outcome','pending');
 ELSE
  UPDATE app.match SET state='between_games',current_game_id=NULL,revision=revision+1 WHERE id=m.id;
  INSERT INTO app.between_game_decision(match_id,seat_id,decision_kind,basis_revision,clock_revision,state,after_game_id)
   SELECT m.id,s.id,'sideboard',m.revision+1,c.clock_revision,'pending',g.id FROM app.match_seat s JOIN app.match_clock c ON c.match_seat_id=s.id WHERE s.match_id=m.id;
  SELECT array_agg(id ORDER BY id) INTO pending_ids FROM app.between_game_decision WHERE match_id=m.id AND after_game_id=g.id;
 END IF;
 UPDATE app.command_receipt SET normalized_source=jsonb_build_object('version','game-outcome-consumption-v1','gameResultId',r.id,'gameId',g.id,'matchId',m.id,'winnerSeatIds',r.winner_seat_ids) WHERE id=receipt.id;
 RETURN app.finish_account_command(receipt.id,true,jsonb_build_object('resourceId',m.id,'revision',(m.revision+1)::text,'phase',CASE WHEN terminal THEN 'terminal' ELSE 'between_games' END,'terminalResultId',coalesce(result_id::text,''),'pendingDecisionIds',coalesce(to_jsonb(pending_ids),'[]'::jsonb)::text));
END $$;

CREATE FUNCTION app.guard_bank_forfeit_parent() RETURNS trigger LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE parent uuid; game_id uuid;
BEGIN
 IF TG_TABLE_NAME='match' THEN parent:=NEW.id;ELSE parent:=NEW.match_id;END IF;
 IF current_setting('transaction_isolation')<>'read committed' AND EXISTS(SELECT 1 FROM app.match WHERE id=parent AND structure='bo3') THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='bank terminal parent changes require read committed retry';END IF;
 FOR game_id IN SELECT g.id FROM app.game g JOIN app.game_result r ON r.game_id=g.id WHERE g.match_id=parent AND r.bank_forfeit_receipt_id IS NOT NULL LOOP PERFORM app.check_bank_forfeit_game(game_id);END LOOP;
 RETURN NULL;
END $$;
CREATE CONSTRAINT TRIGGER bank_forfeit_match_parent AFTER UPDATE ON app.match DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION app.guard_bank_forfeit_parent();
CREATE CONSTRAINT TRIGGER bank_forfeit_decision AFTER INSERT OR UPDATE ON app.between_game_decision DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION app.guard_bank_forfeit_parent();
CREATE CONSTRAINT TRIGGER bank_forfeit_new_game AFTER INSERT OR UPDATE ON app.game DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION app.guard_bank_forfeit_parent();
REVOKE ALL ON FUNCTION app.guard_bank_forfeit_parent() FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;
COMMIT;
