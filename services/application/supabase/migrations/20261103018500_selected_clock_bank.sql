BEGIN;
ALTER TABLE app.match_clock ADD COLUMN eligibility_receipt_id uuid REFERENCES app.command_receipt(id) ON DELETE RESTRICT;
-- Absolute UTC millisecond boundaries retain fractional elapsed time across
-- arbitrarily partitioned commits: charge(a,b)+charge(b,c)=charge(a,c).
CREATE FUNCTION app.clock_elapsed_ms(p_from timestamptz,p_to timestamptz) RETURNS numeric LANGUAGE plpgsql IMMUTABLE SET search_path='' AS $$
BEGIN
 IF p_from IS NULL OR p_to IS NULL OR NOT isfinite(p_from) OR NOT isfinite(p_to) OR p_to<p_from THEN RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='finite monotone clock boundary required';END IF;
 RETURN floor(extract(epoch FROM p_to)*1000)-floor(extract(epoch FROM p_from)*1000);
END $$;
CREATE FUNCTION app.guard_selected_clock_bank() RETURNS trigger LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE m app.match; expected numeric;
BEGIN
 SELECT m0.* INTO m FROM app.match m0 JOIN app.match_seat s ON s.match_id=m0.id WHERE s.id=NEW.match_seat_id FOR NO KEY UPDATE OF m0;
 IF m.room_id IS NULL OR m.accepted_release_id IS NULL THEN RETURN NEW;END IF;
 IF TG_OP='UPDATE' AND NEW.last_accounted_at<OLD.last_accounted_at THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='selected bank accounting boundary cannot move backward';END IF;
 IF m.structure='bo1' THEN
 IF NEW.policy_ref<>'bo1-bank-inactive-v1' OR NEW.remaining_ms<>0 OR NOT NEW.paused THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='BO1 overall bank must remain inactive';END IF;
 ELSIF m.structure='bo3' THEN
 IF NEW.policy_ref<>'bo3-overall-bank-v1' THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='BO3 requires its selected overall bank policy';END IF;
 IF TG_OP='INSERT' THEN
 IF NEW.remaining_ms<>1800000 OR NOT NEW.paused OR NEW.clock_revision<>0 THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='BO3 initial bank must be paused thirty minutes';END IF;
 ELSE
 expected:=CASE WHEN OLD.paused THEN OLD.remaining_ms ELSE greatest(0,OLD.remaining_ms-app.clock_elapsed_ms(OLD.last_accounted_at,NEW.last_accounted_at)) END;
 IF NEW.remaining_ms<>expected THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='BO3 bank must debit exact eligible elapsed milliseconds';END IF;
 END IF;
 IF NEW.remaining_ms=0 AND NOT NEW.paused THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='exhausted overall bank cannot remain eligible';END IF;
 ELSE RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='unsupported admitted clock structure';END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER selected_clock_bank BEFORE INSERT OR UPDATE ON app.match_clock FOR EACH ROW EXECUTE FUNCTION app.guard_selected_clock_bank();
CREATE OR REPLACE FUNCTION app.start_room_match(p_room uuid,p_expected numeric,p_release uuid,p_legality uuid,p_qualification text,p_inputs jsonb,p_request uuid) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE receipt app.command_receipt; lobby app.room; actors uuid[]; fingerprint text; reserved_match uuid; reserved_game uuid;
 item jsonb; slot app.room_slot; seat_id uuid; deck_inputs jsonb:='[]'; accepted jsonb; failure jsonb; rejected boolean:=false; accepted_at timestamptz;
BEGIN
 IF current_setting('transaction_isolation')<>'read committed' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='room start requires read committed retry'; END IF;
 IF p_room IS NULL OR p_request IS NULL OR p_request='00000000-0000-0000-0000-000000000000'::uuid OR p_expected IS NULL OR p_expected::text !~ '^(0|[1-9][0-9]{0,77})$'
 OR p_release IS NULL OR p_legality IS NULL OR p_qualification IS NULL OR length(btrim(p_qualification)) NOT BETWEEN 1 AND 2048
 OR jsonb_typeof(p_inputs) IS DISTINCT FROM 'array' OR octet_length(p_inputs::text)>1048576 THEN RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid room start envelope'; END IF;
 fingerprint:=encode(sha256(convert_to(jsonb_build_object('basis',p_expected::text,'release',p_release,'legality',p_legality,'qualification',p_qualification,'inputs',p_inputs)::text,'UTF8')),'hex');
 INSERT INTO app.command_receipt(principal_key,request_ref,scope,target_ref,payload_hash,state,result_ref)
 VALUES('room-start:'||p_room::text,p_request::text,'room_start_v1',p_room::text,fingerprint,'pending','') ON CONFLICT DO NOTHING RETURNING * INTO receipt;
 IF receipt.id IS NULL THEN
  SELECT * INTO STRICT receipt FROM app.command_receipt WHERE principal_key='room-start:'||p_room::text AND scope='room_start_v1' AND request_ref=p_request::text FOR UPDATE;
  IF receipt.payload_hash<>fingerprint OR receipt.target_ref<>p_room::text THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='room start request terms conflict'; END IF;
  IF receipt.state='pending' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='room start disposition pending'; END IF;
  RETURN receipt.response_payload;
 END IF;
 SELECT array_agg(DISTINCT actor ORDER BY actor) INTO actors FROM(
  SELECT account_id actor FROM app.room_slot WHERE retired_at IS NULL AND room_id=p_room AND account_id IS NOT NULL
  UNION SELECT sponsor_id FROM app.room_slot WHERE retired_at IS NULL AND room_id=p_room AND sponsor_id IS NOT NULL
  UNION SELECT host_id FROM app.room WHERE id=p_room) roster;
 PERFORM id FROM app.account WHERE id=ANY(actors) ORDER BY id FOR NO KEY UPDATE;
 PERFORM id FROM app.queue_entry WHERE claimed_room_id=p_room ORDER BY id FOR NO KEY UPDATE;
 SELECT * INTO lobby FROM app.room WHERE id=p_room FOR NO KEY UPDATE;
 IF lobby.id IS NULL OR lobby.state<>'countdown' OR lobby.revision<>p_expected OR lobby.countdown_at IS NULL OR lobby.countdown_at>clock_timestamp()
 OR EXISTS(SELECT 1 FROM app.match WHERE room_id=p_room) THEN RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('room','stale, not due or already reserved')); END IF;
 IF EXISTS(SELECT 1 FROM app.room_slot WHERE retired_at IS NULL AND room_id=p_room AND ((account_id IS NOT NULL AND NOT(account_id=ANY(coalesce(actors,'{}'::uuid[])))) OR (sponsor_id IS NOT NULL AND NOT(sponsor_id=ANY(coalesce(actors,'{}'::uuid[])))))) THEN
  RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='room roster changed before account fence'; END IF;
 IF EXISTS(SELECT 1 FROM unnest(actors) members(actor_id) WHERE NOT EXISTS(SELECT 1 FROM app.account a WHERE a.id=members.actor_id AND a.state='active')) THEN
  failure:=jsonb_build_object('account','unavailable'); rejected:=true;
 END IF;
 IF NOT rejected AND EXISTS(SELECT 1 FROM app.room_slot WHERE retired_at IS NULL AND room_id=p_room AND controller_kind='ai' AND NOT app.ai_slot_ready(id)) THEN failure:=jsonb_build_object('ai','authenticated current-epoch readiness required'); rejected:=true; END IF;
 IF NOT rejected THEN
  IF jsonb_array_length(p_inputs)<>(SELECT count(*) FROM app.room_slot WHERE retired_at IS NULL AND room_id=p_room AND (account_id IS NOT NULL OR controller_kind='ai')) THEN
   failure:=jsonb_build_object('inputs','complete occupied roster required'); rejected:=true;
  END IF;
 END IF;
 IF NOT rejected AND lobby.queue_claim_receipt_id IS NOT NULL THEN PERFORM app.check_queue_claim(p_room);END IF;
 IF NOT rejected THEN
  BEGIN
   INSERT INTO app.match(room_id,format_id,profile_id,legality_revision_id,mode,competitive,structure,state,revision)
   VALUES(p_room,lobby.format_id,lobby.profile_id,p_legality,lobby.mode,lobby.competitive,lobby.structure,'admitting',0) RETURNING id INTO reserved_match;
   FOR item IN SELECT * FROM jsonb_array_elements(p_inputs) LOOP
    IF jsonb_typeof(item) IS DISTINCT FROM 'object' OR jsonb_typeof(item->'slotId') IS DISTINCT FROM 'string' OR (item->>'slotId') !~ '^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$'
     OR jsonb_typeof(item->'revisionId') IS DISTINCT FROM 'string' OR (item->>'revisionId') !~ '^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$'
     OR jsonb_typeof(item->'snapshotRef') IS DISTINCT FROM 'string' OR length(btrim(item->>'snapshotRef')) NOT BETWEEN 1 AND 2048
     OR EXISTS(SELECT 1 FROM jsonb_object_keys(item) k WHERE k NOT IN ('slotId','revisionId','snapshotRef')) THEN RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid room start seat artifact'; END IF;
    SELECT * INTO slot FROM app.room_slot WHERE retired_at IS NULL AND id=(item->>'slotId')::uuid AND room_id=p_room AND (account_id IS NOT NULL OR controller_kind='ai') FOR NO KEY UPDATE;
    IF slot.id IS NULL OR slot.deck_revision_id IS DISTINCT FROM (item->>'revisionId')::uuid THEN failure:=jsonb_build_object('deck','selected revision changed'); RAISE EXCEPTION USING ERRCODE='SW001',MESSAGE='room admission rejected'; END IF;
    INSERT INTO app.match_seat(match_id,account_id,sponsor_id,seat_index,team_ref,controller_kind)
    VALUES(reserved_match,slot.account_id,slot.sponsor_id,slot.seat_index,slot.team_ref,slot.controller_kind) RETURNING id INTO seat_id;
    deck_inputs:=deck_inputs||jsonb_build_array(jsonb_build_object('seatId',seat_id,'revisionId',item->>'revisionId','snapshotRef',item->>'snapshotRef'));
   END LOOP;
   accepted:=app.accept_match_decks(reserved_match,0,p_release,p_qualification,deck_inputs,receipt.id);
   IF accepted->>'status' IS DISTINCT FROM 'success' THEN failure:=accepted->'data'; RAISE EXCEPTION USING ERRCODE='SW001',MESSAGE='room admission rejected'; END IF;
   accepted_at:=clock_timestamp();
   INSERT INTO app.game(match_id,game_index,state,accepted_revision) VALUES(reserved_match,0,'reserved',0) RETURNING id INTO reserved_game;
   INSERT INTO app.match_clock(match_seat_id,remaining_ms,clock_revision,policy_ref,paused,last_accounted_at)
   SELECT seat.id,CASE WHEN lobby.structure='bo3' THEN 1800000 ELSE 0 END,0,CASE WHEN lobby.structure='bo3' THEN 'bo3-overall-bank-v1' ELSE 'bo1-bank-inactive-v1' END,true,accepted_at FROM app.match_seat seat WHERE seat.match_id=reserved_match;
   UPDATE app.match m SET state='ready',started_at=accepted_at,reserved_game_id=reserved_game WHERE m.id=reserved_match;
   PERFORM app.adopt_room_ai(p_room,reserved_match,receipt.id);
   PERFORM app.ensure_game_conversation(reserved_match);
   INSERT INTO app.durable_handoff(match_id,game_id,source_ref,destination,kind,state) VALUES(reserved_match,reserved_game,reserved_match::text,'game_host','ready_match','pending');
  EXCEPTION WHEN SQLSTATE 'SW001' THEN rejected:=true;
  END;
 END IF;
 IF rejected THEN
  UPDATE app.room_slot SET ready=false WHERE retired_at IS NULL AND room_id=p_room;
  UPDATE app.room SET state='lobby',countdown_at=NULL,revision=revision+1 WHERE id=p_room;
  RETURN app.finish_account_command(receipt.id,false,failure);
 END IF;
 UPDATE app.queue_entry SET state='reserved',reserved_match_id=reserved_match WHERE claimed_room_id=p_room AND state='claimed';
 UPDATE app.room SET state='started',countdown_at=NULL,revision=revision+1 WHERE id=p_room;
 UPDATE app.invitation SET state='canceled' WHERE room_id=p_room AND state='active';
 RETURN app.finish_account_command(receipt.id,true,jsonb_build_object('resourceId',reserved_match,'gameId',reserved_game));
END $$;
CREATE OR REPLACE FUNCTION app.accept_host_candidate(p_game uuid,p_owner text,p_epoch numeric,p_basis numeric,p_request uuid,p_candidate jsonb)
RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE source_game app.game; host app.operational_state; receipt app.command_receipt;
 fingerprint text; context_id uuid; checkpoint_id uuid; event_ids uuid[]:='{}'; event_id uuid; result_id uuid;
 item jsonb; next_sequence numeric; next_revision numeric; next_clock numeric; terminal jsonb;
 accepted_at timestamptz; before_clocks jsonb; discovered_match uuid; seat_count bigint; clock_count bigint;
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
 next_revision:=p_basis+1; accepted_at:=clock_timestamp();before_clocks:=app.match_clock_snapshot(source_game.match_id);
 IF EXISTS(SELECT 1 FROM app.match_clock c JOIN app.match_seat s ON s.id=c.match_seat_id WHERE s.match_id=source_game.match_id AND c.policy_ref='bo3-overall-bank-v1' AND (c.remaining_ms=0 OR NOT c.paused AND app.clock_elapsed_ms(c.last_accounted_at,accepted_at)>=c.remaining_ms)) THEN RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('clocks','overall bank requires its timeout transition'));END IF;
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
  INSERT INTO app.game_result(game_id,source_event_id,outcome,winners_ref,end_reason,winner_seat_ids)
  VALUES(p_game,event_ids[cardinality(event_ids)],terminal->>'outcome',terminal->>'winnersRef',terminal->>'reason',ARRAY(SELECT x::uuid FROM jsonb_array_elements_text(terminal->'winnerSeatIds') x)) RETURNING id INTO result_id;
  INSERT INTO app.durable_handoff(match_id,game_id,source_ref,destination,kind,state)
  VALUES(source_game.match_id,p_game,result_id::text,'match_lifecycle','game_outcome','pending');
 END IF;
 UPDATE app.command_receipt SET normalized_source=jsonb_build_object('version','host-clock-transition-v1','gameId',p_game,'matchId',source_game.match_id,'basisRevision',p_basis::text,'acceptedRevision',next_revision::text,'acceptedAt',to_char(accepted_at AT TIME ZONE 'UTC','YYYY-MM-DD"T"HH24:MI:SS.US"Z"'),'before',before_clocks,'after',app.match_clock_snapshot(source_game.match_id),'requestedClocks',p_candidate->'clocks') WHERE id=receipt.id;
 RETURN app.finish_account_command(receipt.id,true,jsonb_build_object('resourceId',checkpoint_id,'revision',next_revision::text));
END $$;
CREATE OR REPLACE FUNCTION app.pause_game_incident(p_game uuid,p_owner text,p_epoch numeric,p_operational_revision numeric,p_incident_at timestamptz,p_kind text,p_proof text,p_request uuid) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE receipt app.command_receipt; game_row app.game; host app.operational_state; parent uuid; checkpoint app.game_checkpoint; next_clock numeric; terms jsonb; fingerprint text;
BEGIN
 IF current_setting('transaction_isolation')<>'read committed' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='recovery requires read committed retry';END IF;
 IF p_game IS NULL OR p_owner IS NULL OR length(btrim(p_owner)) NOT BETWEEN 1 AND 256 OR p_epoch IS NULL OR p_epoch::text !~ '^(0|[1-9][0-9]{0,77})$'
 OR p_operational_revision IS NULL OR p_operational_revision::text !~ '^(0|[1-9][0-9]{0,77})$' OR p_incident_at IS NULL OR NOT isfinite(p_incident_at) OR p_incident_at>clock_timestamp()
 OR p_kind IS NULL OR p_kind NOT IN ('server_process','storage') OR p_proof IS NULL OR length(btrim(p_proof)) NOT BETWEEN 1 AND 2048 OR p_request IS NULL OR p_request='00000000-0000-0000-0000-000000000000'::uuid THEN RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid trusted incident source';END IF;
 terms:=jsonb_build_object('version','game-incident-v1','gameId',p_game,'owner',p_owner,'epoch',p_epoch::text,'basis',p_operational_revision::text,'incidentAt',to_char(p_incident_at AT TIME ZONE 'UTC','YYYY-MM-DD"T"HH24:MI:SS.US"Z"'),'kind',p_kind,'proofRef',p_proof);fingerprint:=app.auth_json_hash(terms);
 INSERT INTO app.command_receipt(principal_key,scope,target_ref,request_ref,payload_hash,state,result_ref,normalized_source)
 VALUES('recovery:'||p_game::text,'game_incident_v1',p_game::text,p_request::text,fingerprint,'pending','',terms) ON CONFLICT DO NOTHING RETURNING * INTO receipt;
 IF receipt.id IS NULL THEN
 SELECT * INTO STRICT receipt FROM app.command_receipt WHERE principal_key='recovery:'||p_game::text AND scope='game_incident_v1' AND request_ref=p_request::text FOR UPDATE;
 IF receipt.payload_hash<>fingerprint THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='incident request terms conflict';END IF;
 IF receipt.state='pending' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='incident source pending';END IF;RETURN receipt.response_payload;
 END IF;
 SELECT match_id INTO parent FROM app.game WHERE id=p_game;
 PERFORM id FROM app.match WHERE id=parent FOR NO KEY UPDATE;
 SELECT * INTO game_row FROM app.game WHERE id=p_game FOR NO KEY UPDATE;
 IF game_row.match_id IS DISTINCT FROM parent THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='incident game parent changed before fence';END IF;
 SELECT * INTO host FROM app.operational_state WHERE game_id=p_game FOR NO KEY UPDATE;
 IF game_row.id IS NULL OR game_row.state<>'active' OR host.id IS NULL OR host.owner_instance_ref<>p_owner OR host.fence_epoch<>p_epoch OR host.revision<>p_operational_revision OR host.availability<>'available' THEN RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('basis','stale or unavailable'));END IF;
 SELECT * INTO checkpoint FROM app.game_checkpoint WHERE game_id=p_game AND accepted_revision=game_row.accepted_revision ORDER BY id LIMIT 1;
 IF checkpoint.id IS NULL THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='recovery requires latest durable accepted checkpoint';END IF;
 PERFORM c.id FROM app.match_clock c JOIN app.match_seat s ON s.id=c.match_seat_id WHERE s.match_id=parent ORDER BY c.match_seat_id FOR NO KEY UPDATE OF c;
 IF EXISTS(SELECT 1 FROM app.match_clock c JOIN app.match_seat s ON s.id=c.match_seat_id WHERE s.match_id=parent AND c.last_accounted_at>p_incident_at) THEN RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('incidentAt','predates durable clock boundary'));END IF;
 SELECT max(c.clock_revision)+1 INTO next_clock FROM app.match_clock c JOIN app.match_seat s ON s.id=c.match_seat_id WHERE s.match_id=parent;
 UPDATE app.match_clock c SET remaining_ms=CASE WHEN c.paused THEN c.remaining_ms ELSE greatest(0,c.remaining_ms-CASE WHEN c.policy_ref='bo3-overall-bank-v1' THEN app.clock_elapsed_ms(c.last_accounted_at,p_incident_at) ELSE floor(extract(epoch FROM (p_incident_at-c.last_accounted_at))*1000) END) END,
 paused=true,last_accounted_at=p_incident_at,clock_revision=next_clock,eligibility_receipt_id=CASE WHEN c.policy_ref='bo3-overall-bank-v1' THEN receipt.id ELSE c.eligibility_receipt_id END FROM app.match_seat s WHERE s.id=c.match_seat_id AND s.match_id=parent;
 UPDATE app.command_receipt SET normalized_source=normalized_source||jsonb_build_object('checkpointId',checkpoint.id,'acceptedRevision',game_row.accepted_revision::text,'clockRevision',next_clock::text,'clockValues',app.match_clock_snapshot(parent)) WHERE id=receipt.id;
 UPDATE app.operational_state SET availability='recovering',fence_epoch=fence_epoch+1,revision=revision+1,incident_ref=p_proof,incident_at=p_incident_at,recovery_checkpoint_id=checkpoint.id,incident_receipt_id=receipt.id,restoration_receipt_id=NULL WHERE id=host.id;
 RETURN app.finish_account_command(receipt.id,true,jsonb_build_object('resourceId',p_game,'checkpointId',checkpoint.id,'epoch',(p_epoch+1)::text,'operationalRevision',(p_operational_revision+1)::text,'clockRevision',next_clock::text));
END $$;
REVOKE ALL ON FUNCTION app.clock_elapsed_ms(timestamptz,timestamptz),app.guard_selected_clock_bank() FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;

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
 UPDATE app.match_clock c SET paused=true,last_accounted_at=at_time,clock_revision=next_clock,eligibility_receipt_id=CASE WHEN c.policy_ref='bo3-overall-bank-v1' THEN receipt.id ELSE c.eligibility_receipt_id END FROM app.match_seat s WHERE s.id=c.match_seat_id AND s.match_id=m.id;
 UPDATE app.command_receipt SET normalized_source=jsonb_build_object('version','game-opening-v1')||p_opening WHERE id=receipt.id;
 UPDATE app.command_receipt SET normalized_source=coalesce(normalized_source,'{}'::jsonb)||jsonb_build_object('matchId',m.id,'gameId',p_game,'clockValues',app.match_clock_snapshot(m.id)) WHERE id=receipt.id;
 RETURN app.finish_account_command(receipt.id,true,jsonb_build_object('resourceId',g.id,'contextId',context_id,'revision',(m.revision+1)::text));
END $$;
CREATE OR REPLACE FUNCTION app.restore_game_checkpoint(p_game uuid,p_epoch numeric,p_operational_revision numeric,p_checkpoint uuid,p_new_owner text,p_restoration_ref text,p_request uuid) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE receipt app.command_receipt; game_row app.game; host app.operational_state; parent uuid; checkpoint app.game_checkpoint; next_clock numeric; terms jsonb; fingerprint text;
BEGIN
 IF current_setting('transaction_isolation')<>'read committed' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='recovery requires read committed retry';END IF;
 IF p_game IS NULL OR p_checkpoint IS NULL OR p_epoch IS NULL OR p_epoch::text !~ '^(0|[1-9][0-9]{0,77})$' OR p_operational_revision IS NULL OR p_operational_revision::text !~ '^(0|[1-9][0-9]{0,77})$'
 OR p_new_owner IS NULL OR length(btrim(p_new_owner)) NOT BETWEEN 1 AND 256 OR p_restoration_ref IS NULL OR length(btrim(p_restoration_ref)) NOT BETWEEN 1 AND 2048 OR p_request IS NULL OR p_request='00000000-0000-0000-0000-000000000000'::uuid THEN RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid checkpoint restoration source';END IF;
 terms:=jsonb_build_object('version','game-restoration-v1','gameId',p_game,'epoch',p_epoch::text,'basis',p_operational_revision::text,'checkpointId',p_checkpoint,'owner',p_new_owner,'restorationRef',p_restoration_ref);fingerprint:=app.auth_json_hash(terms);
 INSERT INTO app.command_receipt(principal_key,scope,target_ref,request_ref,payload_hash,state,result_ref,normalized_source)
 VALUES('recovery:'||p_game::text,'game_restoration_v1',p_game::text,p_request::text,fingerprint,'pending','',terms) ON CONFLICT DO NOTHING RETURNING * INTO receipt;
 IF receipt.id IS NULL THEN
 SELECT * INTO STRICT receipt FROM app.command_receipt WHERE principal_key='recovery:'||p_game::text AND scope='game_restoration_v1' AND request_ref=p_request::text FOR UPDATE;
 IF receipt.payload_hash<>fingerprint THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='restoration request terms conflict';END IF;
 IF receipt.state='pending' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='restoration source pending';END IF;RETURN receipt.response_payload;
 END IF;
 SELECT match_id INTO parent FROM app.game WHERE id=p_game;
 PERFORM id FROM app.match WHERE id=parent FOR NO KEY UPDATE;
 SELECT * INTO game_row FROM app.game WHERE id=p_game FOR NO KEY UPDATE;
 IF game_row.match_id IS DISTINCT FROM parent THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='restoration parent changed before fence';END IF;
 SELECT * INTO host FROM app.operational_state WHERE game_id=p_game FOR NO KEY UPDATE;
 SELECT * INTO checkpoint FROM app.game_checkpoint WHERE id=p_checkpoint AND game_id=p_game;
 IF game_row.id IS NULL OR game_row.state<>'active' OR host.id IS NULL OR host.availability<>'recovering' OR host.fence_epoch<>p_epoch OR host.revision<>p_operational_revision OR host.recovery_checkpoint_id IS DISTINCT FROM p_checkpoint OR checkpoint.accepted_revision IS DISTINCT FROM game_row.accepted_revision THEN RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('basis','stale or unavailable'));END IF;
 PERFORM c.id FROM app.match_clock c JOIN app.match_seat s ON s.id=c.match_seat_id WHERE s.match_id=parent ORDER BY c.match_seat_id FOR NO KEY UPDATE OF c;
 PERFORM app.check_game_recovery_source(p_game);
 IF EXISTS(SELECT 1 FROM app.match_clock c JOIN app.match_seat s ON s.id=c.match_seat_id WHERE s.match_id=parent AND NOT c.paused) THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='incident clocks must remain paused during restoration';END IF;
 SELECT max(c.clock_revision)+1 INTO next_clock FROM app.match_clock c JOIN app.match_seat s ON s.id=c.match_seat_id WHERE s.match_id=parent;
 UPDATE app.match_clock c SET clock_revision=next_clock,last_accounted_at=clock_timestamp(),eligibility_receipt_id=CASE WHEN c.policy_ref='bo3-overall-bank-v1' THEN receipt.id ELSE c.eligibility_receipt_id END FROM app.match_seat s WHERE s.id=c.match_seat_id AND s.match_id=parent;
 UPDATE app.command_receipt SET normalized_source=normalized_source||jsonb_build_object('incidentId',host.incident_receipt_id,'acceptedRevision',game_row.accepted_revision::text,'clockRevision',next_clock::text,'matchId',parent,'clockValues',app.match_clock_snapshot(parent)) WHERE id=receipt.id;
 UPDATE app.operational_state SET owner_instance_ref=p_new_owner,fence_epoch=fence_epoch+1,revision=revision+1,availability='available',restoration_receipt_id=receipt.id WHERE id=host.id;
 RETURN app.finish_account_command(receipt.id,true,jsonb_build_object('resourceId',p_game,'checkpointId',p_checkpoint,'epoch',(p_epoch+1)::text,'operationalRevision',(p_operational_revision+1)::text,'clockRevision',next_clock::text));
END $$;
CREATE OR REPLACE FUNCTION app.finalize_unrecoverable_match(p_match uuid,p_expected numeric,p_availability numeric,p_incident uuid,p_failure_ref text) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE m app.match; receipt app.command_receipt; terms jsonb; fingerprint text; original_room uuid; actors uuid[]; current_actors uuid[]; binding_id uuid; binding_ids uuid[]; current_binding_ids uuid[];
 result_id uuid; sequence_value numeric; source_season app.season; accepted_at timestamptz; final_games uuid[]; canceled_ids uuid[]; next_clock numeric; commitment app.ante_commitment; response jsonb;
BEGIN
 IF current_setting('transaction_isolation')<>'read committed' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='operational terminalization requires read committed retry';END IF;
 IF p_match IS NULL OR p_incident IS NULL OR p_expected IS NULL OR p_expected::text !~ '^(0|[1-9][0-9]{0,77})$' OR p_availability IS NULL OR p_availability::text !~ '^(0|[1-9][0-9]{0,77})$' OR p_failure_ref IS NULL OR length(btrim(p_failure_ref)) NOT BETWEEN 1 AND 2048 THEN RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid failed-restoration source';END IF;
 terms:=jsonb_build_object('version','match-unrecoverable-v1','matchId',p_match,'basis',p_expected::text,'availabilityBasis',p_availability::text,'incidentId',p_incident,'failureRef',p_failure_ref);fingerprint:=app.auth_json_hash(terms);
 INSERT INTO app.command_receipt(principal_key,scope,target_ref,request_ref,payload_hash,state,result_ref,normalized_source)
 VALUES('match-recovery:'||p_match::text,'match_unrecoverable_v1',p_match::text,p_incident::text,fingerprint,'pending','',terms) ON CONFLICT DO NOTHING RETURNING * INTO receipt;
 IF receipt.id IS NULL THEN SELECT * INTO STRICT receipt FROM app.command_receipt WHERE principal_key='match-recovery:'||p_match::text AND scope='match_unrecoverable_v1' AND request_ref=p_incident::text FOR UPDATE;
 IF receipt.payload_hash<>fingerprint THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='failed-restoration source terms conflict';END IF;
 IF receipt.state='pending' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='failed-restoration source pending';END IF;RETURN receipt.response_payload;END IF;
 SELECT room_id INTO original_room FROM app.match WHERE id=p_match;
 SELECT array_agg(b.id ORDER BY b.id) INTO binding_ids FROM app.ai_runner_binding b JOIN app.room_slot slot ON slot.id=b.room_slot_id WHERE slot.room_id=original_room OR EXISTS(SELECT 1 FROM app.match_seat seat WHERE seat.id=b.match_seat_id AND seat.match_id=p_match);
 SELECT array_agg(DISTINCT actor ORDER BY actor) INTO actors FROM(SELECT account_id actor FROM app.match_seat WHERE match_id=p_match AND account_id IS NOT NULL UNION SELECT sponsor_id FROM app.match_seat WHERE match_id=p_match AND sponsor_id IS NOT NULL) roster;
 PERFORM id FROM app.account WHERE id=ANY(actors) ORDER BY id FOR NO KEY UPDATE;
 PERFORM id FROM app.room WHERE id=original_room FOR NO KEY UPDATE;
 PERFORM id FROM app.ai_runner_binding WHERE id=ANY(binding_ids) ORDER BY id FOR NO KEY UPDATE;
 SELECT * INTO m FROM app.match WHERE id=p_match FOR NO KEY UPDATE;
 SELECT array_agg(b.id ORDER BY b.id) INTO current_binding_ids FROM app.ai_runner_binding b JOIN app.room_slot slot ON slot.id=b.room_slot_id WHERE slot.room_id=original_room OR EXISTS(SELECT 1 FROM app.match_seat seat WHERE seat.id=b.match_seat_id AND seat.match_id=p_match);
 IF current_binding_ids IS DISTINCT FROM binding_ids THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='controller set changed before terminal parent fence';END IF;
 SELECT array_agg(DISTINCT actor ORDER BY actor) INTO current_actors FROM(SELECT account_id actor FROM app.match_seat WHERE match_id=p_match AND account_id IS NOT NULL UNION SELECT sponsor_id FROM app.match_seat WHERE match_id=p_match AND sponsor_id IS NOT NULL) roster;
 IF m.room_id IS DISTINCT FROM original_room OR current_actors IS DISTINCT FROM actors THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='terminal source parent or actor discovery changed';END IF;
 IF m.id IS NULL OR m.accepted_release_id IS NULL OR m.revision<>p_expected OR m.availability_revision<>p_availability OR m.availability<>'recovering' OR m.incident_receipt_id IS DISTINCT FROM p_incident OR m.state NOT IN ('ready','active','between_games') OR EXISTS(SELECT 1 FROM app.account WHERE id=ANY(actors) AND state NOT IN ('active','deletion_pending')) OR EXISTS(SELECT 1 FROM app.match_result WHERE match_id=p_match) THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='stale or contradictory unrecoverable match source';END IF;
 PERFORM id FROM app.game WHERE match_id=p_match ORDER BY game_index FOR NO KEY UPDATE;
 PERFORM op.id FROM app.operational_state op JOIN app.game g ON g.id=op.game_id WHERE g.match_id=p_match ORDER BY op.game_id FOR NO KEY UPDATE OF op;
 PERFORM c.id FROM app.match_clock c JOIN app.match_seat seat ON seat.id=c.match_seat_id WHERE seat.match_id=p_match ORDER BY c.match_seat_id FOR NO KEY UPDATE OF c;
 PERFORM app.check_match_availability(p_match);
 IF m.current_game_id IS NOT NULL THEN PERFORM app.check_game_recovery_source(m.current_game_id);END IF;
 IF EXISTS(SELECT 1 FROM app.game g WHERE g.match_id=p_match AND ((g.state='active' AND g.id IS DISTINCT FROM m.current_game_id) OR (g.state='reserved' AND g.id IS DISTINCT FROM m.reserved_game_id)))
 OR EXISTS(SELECT 1 FROM app.game_result WHERE game_id IN (m.current_game_id,m.reserved_game_id))
 OR EXISTS(SELECT 1 FROM app.game_participation WHERE game_id=m.current_game_id)
 OR EXISTS(SELECT 1 FROM app.game g WHERE g.id=m.reserved_game_id AND EXISTS(SELECT 1 FROM app.game_checkpoint WHERE game_id=g.id))
 OR EXISTS(SELECT 1 FROM app.game g WHERE g.match_id=p_match AND g.id IS DISTINCT FROM m.current_game_id AND g.id IS DISTINCT FROM m.reserved_game_id AND EXISTS(SELECT 1 FROM app.game_checkpoint WHERE game_id=g.id) AND NOT EXISTS(SELECT 1 FROM app.game_result r JOIN app.command_receipt used ON used.principal_key='match-lifecycle:'||p_match::text AND used.scope='game_outcome_v1' AND used.target_ref=r.id::text AND used.state='accepted' WHERE r.game_id=g.id AND r.winner_seat_ids IS NOT NULL)) THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='accepted native outcome/frontier must be reconciled instead of overwritten';END IF;
 IF EXISTS(SELECT 1 FROM app.match_clock c JOIN app.match_seat seat ON seat.id=c.match_seat_id WHERE seat.match_id=p_match AND NOT c.paused) THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='terminal incident bank is not paused';END IF;
 SELECT coalesce(array_agg(id ORDER BY game_index),'{}'::uuid[]) INTO final_games FROM app.game g WHERE match_id=p_match AND EXISTS(SELECT 1 FROM app.game_checkpoint WHERE game_id=g.id);
 SELECT coalesce(array_agg(id ORDER BY id),'{}'::uuid[]) INTO canceled_ids FROM app.between_game_decision WHERE match_id=p_match AND state='pending';
 IF m.competitive THEN
 SELECT next_competitive_sequence INTO sequence_value FROM app.format WHERE id=m.format_id FOR NO KEY UPDATE;
 SELECT * INTO source_season FROM app.season WHERE start_at<=clock_timestamp() AND end_at>clock_timestamp() FOR SHARE;
 IF source_season.id IS NULL THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='competitive terminalization requires configured season';END IF;
 accepted_at:=clock_timestamp();IF accepted_at>=source_season.end_at THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='season boundary crossed during source fence';END IF;
 UPDATE app.format SET next_competitive_sequence=next_competitive_sequence+1 WHERE id=m.format_id;
 ELSE accepted_at:=clock_timestamp();END IF;
 INSERT INTO app.match_result(match_id,source_ref,outcome,winners_ref,committed_at,ranking_format_id,rank_sequence,winner_seat_ids,final_game_ids,source_game_result_id,terminal_kind,terminal_receipt_id)
 VALUES(p_match,'incident:'||p_incident::text,'no_contest','operational:none',accepted_at,CASE WHEN m.competitive THEN m.format_id END,sequence_value,'{}'::uuid[],final_games,NULL,'unrecoverable_incident',receipt.id) RETURNING id INTO result_id;
 UPDATE app.game SET state='no_contest' WHERE id=m.current_game_id;
 UPDATE app.game SET state='canceled' WHERE id=m.reserved_game_id;
 UPDATE app.between_game_decision SET state='canceled',terminal_receipt_id=receipt.id WHERE id=ANY(canceled_ids);
 SELECT max(c.clock_revision)+1 INTO next_clock FROM app.match_clock c JOIN app.match_seat seat ON seat.id=c.match_seat_id WHERE seat.match_id=p_match;
 UPDATE app.match_clock c SET paused=true,clock_revision=next_clock,eligibility_receipt_id=CASE WHEN c.policy_ref='bo3-overall-bank-v1' THEN receipt.id ELSE c.eligibility_receipt_id END FROM app.match_seat seat WHERE seat.id=c.match_seat_id AND seat.match_id=p_match;
 UPDATE app.match SET state='no_contest',availability='unrecoverable',availability_revision=availability_revision+1,terminal_receipt_id=receipt.id,current_game_id=NULL,reserved_game_id=NULL,restoration_receipt_id=NULL,revision=revision+1 WHERE id=p_match;
 UPDATE app.operational_state SET availability='unrecoverable',fence_epoch=fence_epoch+1,revision=revision+1,terminal_receipt_id=receipt.id WHERE game_id=m.current_game_id;
 FOR binding_id IN SELECT b.id FROM app.ai_runner_binding b JOIN app.room_slot slot ON slot.id=b.room_slot_id WHERE (slot.room_id=original_room OR EXISTS(SELECT 1 FROM app.match_seat seat WHERE seat.id=b.match_seat_id AND seat.match_id=p_match)) AND b.desired_state<>'stopped' ORDER BY b.id LOOP PERFORM app.request_ai_stop(binding_id,receipt.id);END LOOP;
 UPDATE app.command_receipt SET normalized_source=normalized_source||jsonb_build_object('interruptedGameId',m.current_game_id,'neverOpenedGameId',m.reserved_game_id,'finalGameIds',to_jsonb(final_games),'canceledDecisionIds',to_jsonb(canceled_ids),'frontiers',app.match_retained_frontiers(p_match),'clocks',app.match_clock_snapshot(p_match),'operational',app.match_operational_snapshot(p_match),'terminalResultId',result_id,'matchAvailabilityRevision',(p_availability+1)::text) WHERE id=receipt.id;
 response:=app.finish_account_command(receipt.id,true,jsonb_build_object('resourceId',p_match,'terminalResultId',result_id,'revision',(p_expected+1)::text,'availabilityRevision',(p_availability+1)::text,'phase','terminal'));
 FOR commitment IN SELECT * FROM app.ante_commitment WHERE match_id=p_match AND state='offered' ORDER BY id LOOP PERFORM app.cancel_ante_for_terminal(commitment.id,receipt.id);END LOOP;
 INSERT INTO app.durable_handoff(match_id,source_ref,destination,kind,state) VALUES(p_match,result_id::text,'match_settlement','match_outcome','pending');
 RETURN response;
END $$;
CREATE FUNCTION app.check_selected_clock_eligibility(p_clock uuid) RETURNS void LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path='' AS $$
DECLARE c app.match_clock; seat app.match_seat; source app.command_receipt; requested jsonb; current_clock jsonb; values_key text;
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
 IF NOT EXISTS(SELECT 1 FROM app.game g JOIN app.match m ON m.id=g.match_id JOIN app.operational_state host ON host.game_id=g.id JOIN app.game_checkpoint checkpoint ON checkpoint.game_id=g.id AND checkpoint.id::text=source.response_payload->'data'->>'resourceId' WHERE g.id::text=source.normalized_source->>'gameId' AND g.match_id=seat.match_id AND m.current_game_id=g.id AND m.state='active' AND m.availability='available' AND g.state='active' AND g.accepted_revision::text=source.normalized_source->>'acceptedRevision' AND checkpoint.accepted_revision=g.accepted_revision AND host.availability='available') THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='eligible bank source must retain its actual active game frontier';END IF;
 SELECT value INTO requested FROM jsonb_array_elements(source.normalized_source->'requestedClocks') WHERE value->>'seatId'=seat.id::text;
 IF source.scope<>'host_candidate_v1' OR source.normalized_source->>'version' IS DISTINCT FROM 'host-clock-transition-v1' OR source.target_ref IS DISTINCT FROM source.normalized_source->>'gameId' OR source.principal_key IS DISTINCT FROM 'host:'||(source.normalized_source->>'gameId') OR source.normalized_source->>'acceptedAt' IS DISTINCT FROM current_clock->>'lastAccountedAt'
 OR requested IS NULL OR requested->'paused' IS DISTINCT FROM 'false'::jsonb OR requested->'eligibility'->>'reason' IS DISTINCT FROM 'native_priority' OR requested->'eligibility'->>'acceptedRevision' IS DISTINCT FROM source.normalized_source->>'basisRevision' OR jsonb_typeof(requested->'eligibility'->'proofRef') IS DISTINCT FROM 'string' OR length(btrim(requested->'eligibility'->>'proofRef')) NOT BETWEEN 1 AND 2048 THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='eligible BO3 bank requires its exact accepted native-priority source';END IF;
 END IF;
END $$;
CREATE FUNCTION app.guard_selected_clock_eligibility() RETURNS trigger LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
BEGIN PERFORM app.check_selected_clock_eligibility(NEW.id);RETURN NULL;END $$;
CREATE CONSTRAINT TRIGGER selected_clock_eligibility AFTER INSERT OR UPDATE ON app.match_clock DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION app.guard_selected_clock_eligibility();
REVOKE ALL ON FUNCTION app.check_selected_clock_eligibility(uuid),app.guard_selected_clock_eligibility() FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;

CREATE FUNCTION app.guard_selected_clock_parents() RETURNS trigger LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE parent uuid; item uuid;
BEGIN
 IF TG_TABLE_NAME='match' THEN parent:=NEW.id;ELSIF TG_TABLE_NAME='game' THEN parent:=NEW.match_id;ELSE SELECT match_id INTO parent FROM app.game WHERE id=NEW.game_id;END IF;
 IF current_setting('transaction_isolation')<>'read committed' AND EXISTS(SELECT 1 FROM app.match WHERE id=parent AND room_id IS NOT NULL AND structure='bo3' AND accepted_release_id IS NOT NULL) THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='selected clock parent changes require read committed retry';END IF;
 FOR item IN SELECT c.id FROM app.match_clock c JOIN app.match_seat seat ON seat.id=c.match_seat_id WHERE seat.match_id=parent AND c.policy_ref='bo3-overall-bank-v1' ORDER BY c.match_seat_id LOOP PERFORM app.check_selected_clock_eligibility(item);END LOOP;
 RETURN NULL;
END $$;
CREATE CONSTRAINT TRIGGER selected_clock_match_parent AFTER UPDATE ON app.match DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION app.guard_selected_clock_parents();
CREATE CONSTRAINT TRIGGER selected_clock_game_parent AFTER UPDATE ON app.game DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION app.guard_selected_clock_parents();
CREATE CONSTRAINT TRIGGER selected_clock_host_parent AFTER UPDATE ON app.operational_state DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION app.guard_selected_clock_parents();
REVOKE ALL ON FUNCTION app.guard_selected_clock_parents() FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;
COMMIT;
