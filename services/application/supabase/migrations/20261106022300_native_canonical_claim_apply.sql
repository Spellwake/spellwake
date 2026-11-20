BEGIN;
-- Extract claim/apply seams from existing writers. Mutation tails below are
-- byte-identical to the current owners; runtime wrappers keep their signatures.

CREATE FUNCTION app.claim_set_room_ready(p_actor uuid,p_room uuid,p_expected numeric,p_ready boolean,p_revision uuid,p_qualification text,p_request uuid) RETURNS app.command_receipt
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE receipt app.command_receipt; lobby app.room; slot app.room_slot;
BEGIN
 IF p_request IS NULL OR p_request='00000000-0000-0000-0000-000000000000'::uuid THEN RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid room source request'; END IF;
 IF current_setting('transaction_isolation')<>'read committed' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='room ready requires read committed retry'; END IF;
 receipt:=app.begin_account_command(p_actor,p_request,'room_ready',coalesce(p_room::text,''),jsonb_build_object('basis',p_expected,'ready',p_ready,'revision',p_revision,'qualification',p_qualification));
 IF receipt.state<>'pending' THEN RETURN receipt; END IF;
 RETURN receipt;
END $$;

CREATE FUNCTION app.apply_set_room_ready(p_receipt uuid,p_actor uuid,p_room uuid,p_expected numeric,p_ready boolean,p_revision uuid,p_qualification text,p_request uuid) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE receipt app.command_receipt; lobby app.room; slot app.room_slot;
BEGIN
 SELECT * INTO receipt FROM app.command_receipt WHERE id=p_receipt FOR UPDATE;
 IF receipt.id IS NULL OR receipt.state<>'pending' OR receipt.scope IS DISTINCT FROM 'room_ready'
  OR receipt.principal_key IS DISTINCT FROM 'human:'||p_actor::text OR receipt.target_ref IS DISTINCT FROM coalesce(p_room::text,'')
  OR receipt.request_ref IS DISTINCT FROM p_request::text
  OR receipt.payload_hash IS DISTINCT FROM app.auth_json_hash(jsonb_build_object('basis',p_expected,'ready',p_ready,'revision',p_revision,'qualification',p_qualification)) THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='locked set_room_ready core requires its preclaimed exact identity';END IF;
 PERFORM id FROM app.account WHERE id=p_actor AND (state='active' OR (state='deletion_pending' AND p_ready=false)) FOR NO KEY UPDATE;
 IF NOT FOUND THEN RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('account','unavailable')); END IF;
 SELECT * INTO lobby FROM app.room WHERE id=p_room FOR NO KEY UPDATE;
 IF lobby.id IS NULL OR lobby.state NOT IN ('lobby','countdown') THEN RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('room','unavailable')); END IF;
 IF p_expected IS NULL OR p_expected::text !~ '^(0|[1-9][0-9]{0,77})$' OR p_expected<>lobby.revision THEN RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('revision','conflict')); END IF;
 SELECT * INTO slot FROM app.room_slot WHERE retired_at IS NULL AND room_id=p_room AND account_id=p_actor AND controller_kind='human' FOR NO KEY UPDATE;
 IF slot.id IS NULL OR p_ready IS NULL THEN RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('ready','unavailable')); END IF;
 IF p_ready AND (p_revision IS NULL OR p_revision IS DISTINCT FROM slot.deck_revision_id OR p_qualification IS NULL OR length(btrim(p_qualification)) NOT BETWEEN 1 AND 2048
  OR NOT EXISTS(SELECT 1 FROM app.deck_revision r JOIN app.deck d ON d.id=r.deck_id WHERE r.id=p_revision AND r.sealed_at IS NOT NULL AND d.source_kind='account' AND d.account_id=p_actor AND d.mode=lobby.mode AND d.format_id=lobby.format_id FOR SHARE OF r,d)) THEN
  RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('deck','qualified selected revision required')); END IF;
 IF slot.ready IS DISTINCT FROM p_ready THEN
  UPDATE app.room_slot SET ready=p_ready WHERE id=slot.id;
  IF NOT p_ready THEN UPDATE app.room SET state='lobby',countdown_at=NULL WHERE id=p_room;
  ELSE PERFORM app.refresh_room_countdown(p_room,false); END IF;
  UPDATE app.room SET revision=revision+1 WHERE id=p_room RETURNING * INTO lobby;
 END IF;
 RETURN app.finish_account_command(receipt.id,true,jsonb_build_object('resourceId',p_room,'revision',lobby.revision::text));
END $$;

CREATE OR REPLACE FUNCTION app.set_room_ready(p_actor uuid,p_room uuid,p_expected numeric,p_ready boolean,p_revision uuid,p_qualification text,p_request uuid) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE receipt app.command_receipt;
BEGIN
 receipt:=app.claim_set_room_ready(p_actor,p_room,p_expected,p_ready,p_revision,p_qualification,p_request);
 IF receipt.state<>'pending' THEN RETURN receipt.response_payload;END IF;
 RETURN app.apply_set_room_ready(receipt.id,p_actor,p_room,p_expected,p_ready,p_revision,p_qualification,p_request);
END $$;

REVOKE ALL ON FUNCTION app.claim_set_room_ready(uuid,uuid,numeric,boolean,uuid,text,uuid),app.apply_set_room_ready(uuid,uuid,uuid,numeric,boolean,uuid,text,uuid) FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;

CREATE FUNCTION app.claim_start_room_countdown(p_actor uuid,p_room uuid,p_expected numeric,p_qualification text,p_request uuid) RETURNS app.command_receipt
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE receipt app.command_receipt; lobby app.room;
BEGIN
 IF p_request IS NULL OR p_request='00000000-0000-0000-0000-000000000000'::uuid THEN RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid room source request'; END IF;
 IF current_setting('transaction_isolation')<>'read committed' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='room countdown requires read committed retry'; END IF;
 receipt:=app.begin_account_command(p_actor,p_request,'room_countdown',coalesce(p_room::text,''),jsonb_build_object('basis',p_expected,'qualification',p_qualification));
 IF receipt.state<>'pending' THEN RETURN receipt; END IF;
 RETURN receipt;
END $$;

CREATE FUNCTION app.apply_start_room_countdown(p_receipt uuid,p_actor uuid,p_room uuid,p_expected numeric,p_qualification text,p_request uuid) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE receipt app.command_receipt; lobby app.room;
BEGIN
 SELECT * INTO receipt FROM app.command_receipt WHERE id=p_receipt FOR UPDATE;
 IF receipt.id IS NULL OR receipt.state<>'pending' OR receipt.scope IS DISTINCT FROM 'room_countdown'
  OR receipt.principal_key IS DISTINCT FROM 'human:'||p_actor::text OR receipt.target_ref IS DISTINCT FROM coalesce(p_room::text,'')
  OR receipt.request_ref IS DISTINCT FROM p_request::text
  OR receipt.payload_hash IS DISTINCT FROM app.auth_json_hash(jsonb_build_object('basis',p_expected,'qualification',p_qualification)) THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='locked start_room_countdown core requires its preclaimed exact identity';END IF;
 PERFORM id FROM app.account WHERE id=p_actor AND state='active' FOR NO KEY UPDATE;
 IF NOT FOUND THEN RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('account','unavailable')); END IF;
 SELECT * INTO lobby FROM app.room WHERE id=p_room FOR NO KEY UPDATE;
 IF lobby.id IS NULL OR lobby.host_id<>p_actor OR lobby.state NOT IN ('lobby','countdown') THEN RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('room','unavailable')); END IF;
 IF p_expected IS NULL OR p_expected::text !~ '^(0|[1-9][0-9]{0,77})$' OR p_expected<>lobby.revision THEN RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('revision','conflict')); END IF;
 IF p_qualification IS NULL OR length(btrim(p_qualification)) NOT BETWEEN 1 AND 2048 THEN RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('decks','qualification required')); END IF;
 IF app.refresh_room_countdown(p_room,true) THEN UPDATE app.room SET revision=revision+1 WHERE id=p_room RETURNING * INTO lobby;
 ELSIF lobby.state<>'countdown' THEN RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('room','invalid slots or selected decks')); END IF;
 RETURN app.finish_account_command(receipt.id,true,jsonb_build_object('resourceId',p_room,'revision',lobby.revision::text));
END $$;

CREATE OR REPLACE FUNCTION app.start_room_countdown(p_actor uuid,p_room uuid,p_expected numeric,p_qualification text,p_request uuid) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE receipt app.command_receipt;
BEGIN
 receipt:=app.claim_start_room_countdown(p_actor,p_room,p_expected,p_qualification,p_request);
 IF receipt.state<>'pending' THEN RETURN receipt.response_payload;END IF;
 RETURN app.apply_start_room_countdown(receipt.id,p_actor,p_room,p_expected,p_qualification,p_request);
END $$;

REVOKE ALL ON FUNCTION app.claim_start_room_countdown(uuid,uuid,numeric,text,uuid),app.apply_start_room_countdown(uuid,uuid,uuid,numeric,text,uuid) FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;

CREATE FUNCTION app.claim_accept_host_candidate(p_game uuid,p_owner text,p_epoch numeric,p_basis numeric,p_request uuid,p_candidate jsonb) RETURNS app.command_receipt
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
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
  RETURN receipt;
 END IF;
 RETURN receipt;
END $$;

CREATE FUNCTION app.apply_accept_host_candidate(p_receipt uuid,p_game uuid,p_owner text,p_epoch numeric,p_basis numeric,p_request uuid,p_candidate jsonb) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
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
 IF EXISTS(SELECT 1 FROM jsonb_object_keys(p_candidate) k WHERE k NOT IN ('contextId','stateRef','factsRef','events','facts','clocks','terminal','participation','bankTimeout','nativeEliminations'))
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
 PERFORM app.record_native_eliminations(receipt.id);
 RETURN app.finish_account_command(receipt.id,true,jsonb_build_object('resourceId',checkpoint_id,'revision',next_revision::text));
END $$;

CREATE OR REPLACE FUNCTION app.accept_host_candidate(p_game uuid,p_owner text,p_epoch numeric,p_basis numeric,p_request uuid,p_candidate jsonb) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE receipt app.command_receipt;
BEGIN
 receipt:=app.claim_accept_host_candidate(p_game,p_owner,p_epoch,p_basis,p_request,p_candidate);
 IF receipt.state<>'pending' THEN RETURN receipt.response_payload;END IF;
 RETURN app.apply_accept_host_candidate(receipt.id,p_game,p_owner,p_epoch,p_basis,p_request,p_candidate);
END $$;

REVOKE ALL ON FUNCTION app.claim_accept_host_candidate(uuid,text,numeric,numeric,uuid,jsonb),app.apply_accept_host_candidate(uuid,uuid,text,numeric,numeric,uuid,jsonb) FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;

COMMIT;
