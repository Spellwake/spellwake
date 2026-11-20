BEGIN;
ALTER TABLE app.game_checkpoint ADD COLUMN source_receipt_id uuid REFERENCES app.command_receipt(id) ON DELETE RESTRICT;
ALTER TABLE app.game_event ADD COLUMN source_receipt_id uuid REFERENCES app.command_receipt(id) ON DELETE RESTRICT;
CREATE INDEX checkpoint_creator ON app.game_checkpoint(source_receipt_id) WHERE source_receipt_id IS NOT NULL;
CREATE INDEX event_creator ON app.game_event(source_receipt_id) WHERE source_receipt_id IS NOT NULL;
CREATE UNIQUE INDEX accepted_host_game_revision ON app.command_receipt(target_ref,(normalized_source->>'acceptedRevision')) WHERE scope='host_candidate_v1' AND state='accepted' AND normalized_source->>'version'='host-clock-transition-v1';
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
 INSERT INTO app.game_checkpoint(game_id,context_id,accepted_revision,private_state_ref,facts_ref,clock_revision,source_receipt_id)
 VALUES(g.id,context_id,0,p_opening->>'stateRef',p_opening->>'factsRef',next_clock,receipt.id);
 INSERT INTO app.game_event(game_id,context_id,sequence,accepted_revision,private_payload_ref,source_receipt_id) VALUES(g.id,context_id,0,0,p_opening->>'eventRef',receipt.id);
 UPDATE app.game SET state='active' WHERE id=g.id;
 UPDATE app.match SET state='active',reserved_game_id=NULL,current_game_id=g.id,revision=revision+1 WHERE id=m.id;
 INSERT INTO app.operational_state(game_id,owner_instance_ref,fence_epoch,availability,revision) VALUES(g.id,p_owner,1,'available',0);
 at_time:=clock_timestamp();
 UPDATE app.match_clock c SET paused=true,last_accounted_at=at_time,clock_revision=next_clock,eligibility_receipt_id=CASE WHEN c.policy_ref='bo3-overall-bank-v1' THEN receipt.id ELSE c.eligibility_receipt_id END FROM app.match_seat s WHERE s.id=c.match_seat_id AND s.match_id=m.id;
 UPDATE app.command_receipt SET normalized_source=jsonb_build_object('version','game-opening-v1')||p_opening WHERE id=receipt.id;
 UPDATE app.command_receipt SET normalized_source=coalesce(normalized_source,'{}'::jsonb)||jsonb_build_object('matchId',m.id,'gameId',p_game,'owner',p_owner,'clockValues',app.match_clock_snapshot(m.id)) WHERE id=receipt.id;
 RETURN app.finish_account_command(receipt.id,true,jsonb_build_object('resourceId',g.id,'contextId',context_id,'revision',(m.revision+1)::text));
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
 RETURN app.finish_account_command(receipt.id,true,jsonb_build_object('resourceId',checkpoint_id,'revision',next_revision::text));
END $$;
CREATE OR REPLACE FUNCTION app.check_selected_clock_eligibility(p_clock uuid) RETURNS void LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path='' AS $$
DECLARE c app.match_clock; seat app.match_seat; source app.command_receipt; requested jsonb; current_clock jsonb; values_key text; incident app.command_receipt; prior app.command_receipt; prior_clock jsonb; prior_key text;
BEGIN
 SELECT * INTO c FROM app.match_clock WHERE id=p_clock;
 IF c.id IS NULL OR c.policy_ref<>'bo3-overall-bank-v1' THEN RETURN;END IF;
 IF current_setting('transaction_isolation')<>'read committed' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='selected clock source checks require read committed retry';END IF;
 SELECT * INTO seat FROM app.match_seat WHERE id=c.match_seat_id;
 IF c.clock_revision=0 AND c.paused AND c.remaining_ms=1800000 AND c.eligibility_receipt_id IS NULL THEN RETURN;END IF;
 SELECT * INTO source FROM app.command_receipt WHERE id=c.eligibility_receipt_id;
 current_clock:=jsonb_build_object('seatId',seat.id,'remainingMs',c.remaining_ms::text,'clockRevision',c.clock_revision::text,'paused',c.paused,'lastAccountedAt',to_char(c.last_accounted_at AT TIME ZONE 'UTC','YYYY-MM-DD"T"HH24:MI:SS.US"Z"'));
 values_key:=CASE WHEN source.scope='host_candidate_v1' THEN 'after' WHEN source.scope='match_unrecoverable_v1' THEN 'clocks' ELSE 'clockValues' END;
 IF source.id IS NULL OR source.state<>'accepted' OR source.scope NOT IN('host_candidate_v1','game_opening_v1','game_incident_v1','game_restoration_v1','match_unrecoverable_v1') OR source.normalized_source->>'matchId' IS DISTINCT FROM seat.match_id::text OR NOT coalesce(source.normalized_source->values_key @> jsonb_build_array(current_clock),false) THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='BO3 clock requires its exact accepted transition source';END IF;
 IF NOT c.paused THEN
 IF NOT EXISTS(SELECT 1 FROM app.game g JOIN app.match m ON m.id=g.match_id JOIN app.operational_state host ON host.game_id=g.id JOIN app.game_checkpoint checkpoint ON checkpoint.game_id=g.id AND checkpoint.id::text=CASE WHEN source.scope='game_restoration_v1' THEN source.normalized_source->>'checkpointId' ELSE source.response_payload->'data'->>'resourceId' END WHERE g.id::text=source.normalized_source->>'gameId' AND g.match_id=seat.match_id AND m.current_game_id=g.id AND m.state='active' AND m.availability='available' AND g.state='active' AND g.accepted_revision::text=source.normalized_source->>'acceptedRevision' AND checkpoint.accepted_revision=g.accepted_revision AND host.availability='available') THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='eligible bank source must retain its actual active game frontier';END IF;
 IF source.scope='game_restoration_v1' THEN
 SELECT * INTO incident FROM app.command_receipt WHERE id::text=source.normalized_source->>'incidentId';
 SELECT value INTO prior_clock FROM jsonb_array_elements(incident.normalized_source->'beforeClocks') value WHERE value->>'seatId'=seat.id::text;
 SELECT * INTO prior FROM app.command_receipt WHERE id::text=(SELECT value->>'receiptId' FROM jsonb_array_elements(incident.normalized_source->'eligibilitySources') value WHERE value->>'seatId'=seat.id::text);
 prior_key:=CASE WHEN prior.scope='host_candidate_v1' THEN 'after' ELSE 'clockValues' END;
 IF incident.id IS NULL OR incident.state<>'accepted' OR incident.scope<>'game_incident_v1' OR incident.target_ref IS DISTINCT FROM source.target_ref OR prior.id IS NULL OR prior.state<>'accepted' OR prior.scope NOT IN('host_candidate_v1','game_restoration_v1') OR prior.target_ref IS DISTINCT FROM source.target_ref OR prior_clock IS NULL OR prior_clock->'paused' IS DISTINCT FROM 'false'::jsonb OR NOT coalesce(prior.normalized_source->prior_key @> jsonb_build_array(prior_clock),false) OR (prior_clock->>'clockRevision')::numeric>=c.clock_revision OR (prior_clock->>'remainingMs')::numeric<c.remaining_ms OR source.normalized_source->>'resumedAt' IS DISTINCT FROM current_clock->>'lastAccountedAt' OR (source.normalized_source->>'resumedAt')::timestamptz<(incident.normalized_source->>'incidentAt')::timestamptz OR (source.normalized_source->>'resumedAt')::timestamptz<(prior_clock->>'lastAccountedAt')::timestamptz THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='restored eligibility must continue exact preincident qualified clock';END IF;
 ELSE
 SELECT value INTO requested FROM jsonb_array_elements(source.normalized_source->'candidate'->'clocks') WHERE value->>'seatId'=seat.id::text;
 IF source.scope<>'host_candidate_v1' OR source.normalized_source->>'version' IS DISTINCT FROM 'host-clock-transition-v1' OR source.target_ref IS DISTINCT FROM source.normalized_source->>'gameId' OR source.principal_key IS DISTINCT FROM 'host:'||(source.normalized_source->>'gameId') OR source.normalized_source->>'acceptedAt' IS DISTINCT FROM current_clock->>'lastAccountedAt'
 OR requested IS NULL OR requested->'paused' IS DISTINCT FROM 'false'::jsonb OR requested->'eligibility'->>'reason' IS DISTINCT FROM 'native_priority' OR requested->'eligibility'->>'acceptedRevision' IS DISTINCT FROM source.normalized_source->>'basisRevision' OR jsonb_typeof(requested->'eligibility'->'proofRef') IS DISTINCT FROM 'string' OR length(btrim(requested->'eligibility'->>'proofRef')) NOT BETWEEN 1 AND 2048 THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='eligible BO3 bank requires its exact accepted native-priority source';END IF;
 END IF;
 END IF;
END $$;

-- Reproduce a complete immutable frame from its exact original normalized input.
-- Private state remains in artifacts; the input contains only references/facts.
CREATE FUNCTION app.check_native_creator_frame(p_source uuid) RETURNS void LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path='' AS $$
DECLARE source app.command_receipt; g app.game; m app.match; cp app.game_checkpoint; ctx app.game_context; event app.game_event; outcome app.game_result; body jsonb; expected jsonb; original jsonb; item jsonb; terminal jsonb; fact app.game_activity_fact; participant app.game_participation; seat app.match_seat; ordinal bigint; first_sequence numeric; revision numeric; is_opening boolean; clock_values jsonb; requested_timeout jsonb; projected_timeout jsonb;
BEGIN
 SELECT * INTO source FROM app.command_receipt WHERE id=p_source;
 IF source.id IS NULL THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='native creator is unavailable';END IF;
 is_opening:=source.scope='game_opening_v1';
 IF NOT ((is_opening AND source.principal_key LIKE 'match-opening:%') OR (source.scope='host_candidate_v1' AND source.principal_key LIKE 'host:%')) THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='native creator family is unavailable';END IF;
 IF source.state<>'accepted' THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='native frame creator must be accepted';END IF;
 SELECT * INTO g FROM app.game WHERE id::text=source.target_ref;SELECT * INTO m FROM app.match WHERE id=g.match_id;
 SELECT * INTO cp FROM app.game_checkpoint WHERE source_receipt_id=source.id;
 SELECT * INTO ctx FROM app.game_context WHERE id=cp.context_id;
 IF g.id IS NULL OR m.id IS NULL OR (SELECT count(*) FROM app.game_checkpoint WHERE source_receipt_id=source.id)<>1 OR cp.game_id IS DISTINCT FROM g.id OR ctx.game_id IS DISTINCT FROM g.id OR source.normalized_source->>'gameId' IS DISTINCT FROM g.id::text OR source.normalized_source->>'matchId' IS DISTINCT FROM m.id::text OR cp.accepted_revision>g.accepted_revision THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='native frame requires one actual same-game checkpoint';END IF;
 clock_values:=CASE WHEN is_opening THEN source.normalized_source->'clockValues' ELSE source.normalized_source->'after' END;
 IF jsonb_typeof(clock_values) IS DISTINCT FROM 'array' THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='native checkpoint requires its creator clock snapshot';END IF;
 IF jsonb_array_length(clock_values)=0 OR jsonb_array_length(clock_values)<>(SELECT count(*) FROM app.match_seat WHERE match_id=m.id) OR (SELECT count(DISTINCT value->>'seatId') FROM jsonb_array_elements(clock_values))<>jsonb_array_length(clock_values) OR EXISTS(SELECT 1 FROM jsonb_array_elements(clock_values) value WHERE value->>'clockRevision' IS DISTINCT FROM cp.clock_revision::text OR NOT EXISTS(SELECT 1 FROM app.match_seat WHERE id::text=value->>'seatId' AND match_id=m.id)) THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='checkpoint clock revision must match its exact complete creator snapshot';END IF;
 IF is_opening THEN
 original:=jsonb_build_object('snapshotRef',source.normalized_source->'snapshotRef','contextRevision',source.normalized_source->'contextRevision','stateRef',source.normalized_source->'stateRef','factsRef',source.normalized_source->'factsRef','eventRef',source.normalized_source->'eventRef','qualificationRef',source.normalized_source->'qualificationRef');
 expected:=jsonb_build_object('match',m.id,'game',g.id,'owner',source.normalized_source->>'owner','opening',original);
 SELECT * INTO event FROM app.game_event WHERE source_receipt_id=source.id;
 IF source.principal_key IS DISTINCT FROM 'match-opening:'||m.id::text OR source.request_ref IS DISTINCT FROM g.id::text OR source.payload_hash IS DISTINCT FROM app.auth_json_hash(expected) OR source.normalized_source->>'version' IS DISTINCT FROM 'game-opening-v1' OR source.normalized_source->>'owner' IS NULL OR cp.accepted_revision<>0 OR cp.private_state_ref IS DISTINCT FROM original->>'stateRef' OR cp.facts_ref IS DISTINCT FROM original->>'factsRef' OR ctx.snapshot_ref IS DISTINCT FROM original->>'snapshotRef' OR ctx.context_revision IS DISTINCT FROM original->>'contextRevision' OR source.response_payload->'data'->>'resourceId' IS DISTINCT FROM g.id::text OR source.response_payload->'data'->>'contextId' IS DISTINCT FROM ctx.id::text
 OR (SELECT count(*) FROM app.game_event WHERE source_receipt_id=source.id)<>1 OR event.game_id IS DISTINCT FROM g.id OR event.context_id IS DISTINCT FROM ctx.id OR event.sequence<>0 OR event.accepted_revision<>0 OR event.private_payload_ref IS DISTINCT FROM original->>'eventRef' OR EXISTS(SELECT 1 FROM app.game_activity_fact WHERE source_event_id=event.id) OR EXISTS(SELECT 1 FROM app.game_result WHERE source_event_id=event.id) OR EXISTS(SELECT 1 FROM app.game_participation WHERE game_id=g.id AND accepted_revision=0) THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='opening frame differs from its exact accepted source';END IF;
 RETURN;
 END IF;
 body:=source.normalized_source->'candidate';revision:=cp.accepted_revision;
 IF jsonb_typeof(body) IS DISTINCT FROM 'object' OR jsonb_typeof(body->'events') IS DISTINCT FROM 'array' OR jsonb_typeof(body->'facts') IS DISTINCT FROM 'array' OR jsonb_typeof(body->'participation') IS DISTINCT FROM 'array' OR jsonb_typeof(body->'clocks') IS DISTINCT FROM 'array' OR jsonb_typeof(source.normalized_source->'eventIds') IS DISTINCT FROM 'array' THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='host frame requires its exact original normalized input';END IF;
 expected:=jsonb_build_object('owner',source.normalized_source->>'owner','epoch',source.normalized_source->>'epoch','basis',source.normalized_source->>'basisRevision','candidate',body);
 IF source.principal_key IS DISTINCT FROM 'host:'||g.id::text OR source.normalized_source->>'version' IS DISTINCT FROM 'host-clock-transition-v1' OR source.normalized_source->>'owner' IS NULL OR source.normalized_source->>'epoch' IS NULL OR source.normalized_source->>'epoch' !~ '^(0|[1-9][0-9]{0,77})$' OR source.normalized_source->>'basisRevision' IS NULL OR source.normalized_source->>'basisRevision' !~ '^(0|[1-9][0-9]{0,77})$' OR revision<>(source.normalized_source->>'basisRevision')::numeric+1 OR source.normalized_source->>'acceptedRevision' IS DISTINCT FROM revision::text OR source.payload_hash IS DISTINCT FROM app.auth_json_hash(expected) OR source.response_payload->'data'->>'resourceId' IS DISTINCT FROM cp.id::text OR source.response_payload->'data'->>'revision' IS DISTINCT FROM revision::text
 OR body->>'contextId' IS DISTINCT FROM ctx.id::text OR cp.private_state_ref IS DISTINCT FROM body->>'stateRef' OR cp.facts_ref IS DISTINCT FROM body->>'factsRef' OR jsonb_array_length(body->'events') NOT BETWEEN 1 AND 1024 OR jsonb_array_length(body->'events')<>jsonb_array_length(source.normalized_source->'eventIds') OR (SELECT count(*) FROM app.game_event WHERE source_receipt_id=source.id)<>jsonb_array_length(body->'events') THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='host checkpoint or exact frame membership differs from its creator';END IF;
 IF jsonb_typeof(source.normalized_source->'before') IS DISTINCT FROM 'array' THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='host frame requires its prior clock basis';END IF;
 FOR item IN SELECT value FROM jsonb_array_elements(source.normalized_source->'before') LOOP
 IF item->>'clockRevision' IS NULL OR item->>'clockRevision' !~ '^(0|[1-9][0-9]{0,77})$' THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='host prior clock revision must be canonical';END IF;
 END LOOP;
 IF cp.clock_revision IS DISTINCT FROM (SELECT max((value->>'clockRevision')::numeric)+1 FROM jsonb_array_elements(source.normalized_source->'before')) THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='host checkpoint must advance the shared clock revision once';END IF;
 SELECT min(sequence) INTO first_sequence FROM app.game_event WHERE source_receipt_id=source.id;
 FOR item,ordinal IN SELECT value,ordinality FROM jsonb_array_elements(source.normalized_source->'eventIds') WITH ORDINALITY LOOP
 SELECT * INTO event FROM app.game_event WHERE id::text=item#>>'{}';
 IF event.id IS NULL OR event.source_receipt_id IS DISTINCT FROM source.id OR event.game_id IS DISTINCT FROM g.id OR event.context_id IS DISTINCT FROM ctx.id OR event.accepted_revision<>revision OR event.sequence<>first_sequence+ordinal-1 OR event.private_payload_ref IS DISTINCT FROM body->'events'->>(ordinal-1)::int THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='host events must preserve exact creator order and artifacts';END IF;
 END LOOP;
 IF first_sequence IS DISTINCT FROM (SELECT coalesce(max(sequence),-1)+1 FROM app.game_event WHERE game_id=g.id AND accepted_revision<revision) THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='host frame must extend the actual prior native event frontier';END IF;
 IF (SELECT count(*) FROM app.game_activity_fact f JOIN app.game_event e ON e.id=f.source_event_id WHERE e.source_receipt_id=source.id)<>jsonb_array_length(body->'facts') THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='host fact set differs from accepted normalized input';END IF;
 FOR fact IN SELECT f.* FROM app.game_activity_fact f JOIN app.game_event e ON e.id=f.source_event_id WHERE e.source_receipt_id=source.id LOOP
 SELECT ordinality-1 INTO ordinal FROM jsonb_array_elements(source.normalized_source->'eventIds') WITH ORDINALITY WHERE value#>>'{}'=fact.source_event_id::text;
 item:=jsonb_build_object('eventIndex',ordinal::text,'accountId',fact.account_id,'kind',fact.kind,'value',fact.value::text,'revision',fact.fact_revision);
 IF fact.game_id IS DISTINCT FROM g.id OR NOT coalesce(body->'facts' @> jsonb_build_array(item),false) THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='normalized fact lacks exact accepted host provenance';END IF;
 END LOOP;
 IF (SELECT count(*) FROM app.game_participation WHERE game_id=g.id AND accepted_revision=revision)<>jsonb_array_length(body->'participation') THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='participation set differs from accepted native frame';END IF;
 FOR participant IN SELECT * FROM app.game_participation WHERE game_id=g.id AND accepted_revision=revision LOOP
 SELECT * INTO seat FROM app.match_seat WHERE id=participant.match_seat_id;
 item:=jsonb_build_object('seatId',participant.match_seat_id,'ownTurns',participant.own_turns::text,'reason',participant.reason);
 IF seat.match_id IS DISTINCT FROM m.id OR seat.controller_kind IS DISTINCT FROM 'human' OR participant.account_id IS DISTINCT FROM seat.account_id OR participant.real_game IS DISTINCT FROM (participant.own_turns>=3) OR participant.completed_at IS DISTINCT FROM app.financial_instant(source.normalized_source->>'acceptedAt') OR NOT coalesce(body->'participation' @> jsonb_build_array(item),false) THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='human participation lacks exact accepted host provenance';END IF;
 END LOOP;
 terminal:=body->'terminal';IF terminal='null'::jsonb THEN terminal:=NULL;END IF;
 requested_timeout:=body->'bankTimeout';IF requested_timeout='null'::jsonb THEN requested_timeout:=NULL;END IF;
 projected_timeout:=source.normalized_source->'bankTimeout';IF projected_timeout='null'::jsonb THEN projected_timeout:=NULL;END IF;
 IF requested_timeout IS DISTINCT FROM (projected_timeout-'priorClockSourceId') THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='derived bank timeout must retain the exact original accepted input';END IF;

 SELECT r.* INTO outcome FROM app.game_result r JOIN app.game_event e ON e.id=r.source_event_id WHERE e.source_receipt_id=source.id;
 IF terminal IS NULL THEN
 IF outcome.id IS NOT NULL OR source.normalized_source->>'gameResultId' IS NOT NULL OR jsonb_array_length(body->'participation')<>0 THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='nonterminal host frame cannot acquire a later result or participation';END IF;
 ELSE
 IF outcome.id IS NULL OR (SELECT count(*) FROM app.game_result r JOIN app.game_event e ON e.id=r.source_event_id WHERE e.source_receipt_id=source.id)<>1 OR outcome.id::text IS DISTINCT FROM source.normalized_source->>'gameResultId' OR outcome.game_id IS DISTINCT FROM g.id OR outcome.source_event_id::text IS DISTINCT FROM source.normalized_source->'eventIds'->>-1 OR outcome.outcome IS DISTINCT FROM terminal->>'outcome' OR outcome.winners_ref IS DISTINCT FROM terminal->>'winnersRef' OR outcome.end_reason IS DISTINCT FROM terminal->>'reason' OR to_jsonb(outcome.winner_seat_ids) IS DISTINCT FROM terminal->'winnerSeatIds' THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='native terminal output differs from its exact accepted frame';END IF;
 END IF;
 IF (requested_timeout IS NULL AND outcome.bank_forfeit_receipt_id IS NOT NULL) OR (requested_timeout IS NOT NULL AND (terminal IS NULL OR outcome.id IS NULL OR outcome.bank_forfeit_receipt_id IS DISTINCT FROM source.id OR outcome.bank_forfeit_seat_id::text IS DISTINCT FROM requested_timeout->>'seatId')) THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='native bank result must project its actual accepted timeout';END IF;
END $$;

CREATE FUNCTION app.check_native_game_storage(p_game uuid) RETURNS void LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path='' AS $$
DECLARE g app.game; m app.match; source uuid;
BEGIN
 SELECT * INTO g FROM app.game WHERE id=p_game;SELECT * INTO m FROM app.match WHERE id=g.match_id;
 IF g.id IS NULL THEN RETURN;END IF;
 IF m.room_id IS NOT NULL AND m.started_at IS NOT NULL AND m.accepted_release_id IS NOT NULL THEN
 IF NOT EXISTS(SELECT 1 FROM app.game_context WHERE game_id=g.id) AND g.state IN('reserved','canceled') THEN RETURN;END IF;
 IF EXISTS(SELECT 1 FROM app.game_checkpoint checkpoint JOIN app.match_clock clock ON true JOIN app.match_seat seat ON seat.id=clock.match_seat_id WHERE checkpoint.game_id=g.id AND seat.match_id=m.id AND checkpoint.clock_revision>clock.clock_revision) THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='persisted native checkpoint cannot outrun the actual match clock basis';END IF;
 IF (SELECT count(*) FROM app.game_checkpoint WHERE game_id=g.id)<>g.accepted_revision+1 OR EXISTS(SELECT 1 FROM app.game_checkpoint WHERE game_id=g.id AND (source_receipt_id IS NULL OR accepted_revision>g.accepted_revision)) OR EXISTS(SELECT 1 FROM app.game_event WHERE game_id=g.id AND (source_receipt_id IS NULL OR accepted_revision>g.accepted_revision)) THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='qualified native history requires all creator receipts and no future checkpoint/event poison';END IF;
 END IF;
 FOR source IN SELECT source_receipt_id FROM app.game_checkpoint WHERE game_id=g.id AND source_receipt_id IS NOT NULL UNION SELECT source_receipt_id FROM app.game_event WHERE game_id=g.id AND source_receipt_id IS NOT NULL LOOP PERFORM app.check_native_creator_frame(source);END LOOP;
END $$;

CREATE FUNCTION app.guard_native_creator_rows() RETURNS trigger LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE target_game uuid; source uuid; parent uuid; expected_parent uuid;
BEGIN
 IF current_setting('transaction_isolation')<>'read committed' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='native creator mutations require read committed retry';END IF;
 IF TG_TABLE_NAME='command_receipt' THEN
 -- Reload after pending→accepted stages; NEW may be the earlier row version.
 IF EXISTS(SELECT 1 FROM app.command_receipt r WHERE r.id=NEW.id AND r.state='accepted' AND ((r.scope='host_candidate_v1' AND r.principal_key='host:'||r.target_ref) OR (r.scope='game_opening_v1' AND r.principal_key LIKE 'match-opening:%'))) THEN PERFORM app.check_native_creator_frame(NEW.id);END IF;RETURN NULL;
 ELSIF TG_TABLE_NAME='match' THEN
 FOR target_game IN SELECT id FROM app.game WHERE match_id=NEW.id ORDER BY game_index LOOP PERFORM app.check_native_game_storage(target_game);END LOOP;RETURN NULL;
 ELSIF TG_TABLE_NAME='game' THEN target_game:=NEW.id;
 ELSE target_game:=NEW.game_id;
 END IF;
 SELECT match_id INTO expected_parent FROM app.game WHERE id=target_game;
 PERFORM id FROM app.match WHERE id=expected_parent FOR NO KEY UPDATE;
 SELECT match_id INTO parent FROM app.game WHERE id=target_game FOR SHARE;
 IF parent IS DISTINCT FROM expected_parent THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='native source parent changed before creator fence';END IF;
 IF TG_TABLE_NAME IN('game_checkpoint','game_event') THEN source:=NEW.source_receipt_id;
 ELSIF TG_TABLE_NAME='game_activity_fact' THEN SELECT source_receipt_id INTO source FROM app.game_event WHERE id=NEW.source_event_id;
 ELSIF TG_TABLE_NAME='game_result' THEN SELECT source_receipt_id INTO source FROM app.game_event WHERE id=NEW.source_event_id;
 ELSIF TG_TABLE_NAME='game_participation' THEN SELECT event.source_receipt_id INTO source FROM app.game_result result JOIN app.game_event event ON event.id=result.source_event_id WHERE result.game_id=target_game;
 END IF;
 IF TG_TABLE_NAME NOT IN('game','game_context') THEN
 IF source IS NULL AND EXISTS(SELECT 1 FROM app.match WHERE id=parent AND room_id IS NOT NULL AND started_at IS NOT NULL AND accepted_release_id IS NOT NULL) THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='qualified native row requires its actual creator receipt';END IF;
 IF source IS NOT NULL THEN PERFORM app.check_native_creator_frame(source);END IF;
 END IF;
 PERFORM app.check_native_game_storage(target_game);RETURN NULL;
END $$;
CREATE CONSTRAINT TRIGGER native_checkpoint_creator AFTER INSERT ON app.game_checkpoint DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION app.guard_native_creator_rows();
CREATE CONSTRAINT TRIGGER native_event_creator AFTER INSERT ON app.game_event DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION app.guard_native_creator_rows();
CREATE CONSTRAINT TRIGGER native_fact_creator AFTER INSERT ON app.game_activity_fact DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION app.guard_native_creator_rows();
CREATE CONSTRAINT TRIGGER native_participation_creator AFTER INSERT ON app.game_participation DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION app.guard_native_creator_rows();
CREATE CONSTRAINT TRIGGER native_result_creator AFTER INSERT ON app.game_result DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION app.guard_native_creator_rows();
CREATE CONSTRAINT TRIGGER native_receipt_frame AFTER INSERT OR UPDATE ON app.command_receipt DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION app.guard_native_creator_rows();
CREATE CONSTRAINT TRIGGER native_match_storage AFTER UPDATE ON app.match DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION app.guard_native_creator_rows();
CREATE CONSTRAINT TRIGGER native_game_storage AFTER INSERT OR UPDATE ON app.game DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION app.guard_native_creator_rows();
CREATE CONSTRAINT TRIGGER native_context_storage AFTER INSERT OR UPDATE ON app.game_context DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION app.guard_native_creator_rows();
REVOKE ALL ON FUNCTION app.check_native_creator_frame(uuid),app.check_native_game_storage(uuid),app.guard_native_creator_rows() FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;
COMMIT;
