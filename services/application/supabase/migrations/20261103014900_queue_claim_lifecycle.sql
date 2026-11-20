BEGIN;
CREATE FUNCTION app.check_queue_claim(p_room uuid) RETURNS void LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE lobby app.room; receipt app.command_receipt; configuration jsonb; selection jsonb; item jsonb; entry app.queue_entry; slot app.room_slot; accepted_match uuid;
BEGIN
 SELECT * INTO lobby FROM app.room WHERE id=p_room;
 IF lobby.id IS NULL OR lobby.queue_claim_receipt_id IS NULL THEN RETURN; END IF;
 SELECT * INTO receipt FROM app.command_receipt WHERE id=lobby.queue_claim_receipt_id;
 selection:=receipt.normalized_source->'selection';configuration:=receipt.normalized_source->'configuration';
 IF receipt.state IS DISTINCT FROM 'accepted' OR receipt.scope IS DISTINCT FROM 'queue_claim_v1' OR receipt.normalized_source->>'version' IS DISTINCT FROM 'queue-claim-v1'
 OR lobby.host_id IS DISTINCT FROM (SELECT account_id FROM app.queue_entry WHERE id=(selection->>'hostEntryId')::uuid)
 OR receipt.response_payload->'data'->>'resourceId' IS DISTINCT FROM lobby.id::text OR selection->>'profileId' IS DISTINCT FROM lobby.profile_id::text
 OR configuration->>'formatId' IS DISTINCT FROM lobby.format_id::text OR configuration->>'seatCount' IS DISTINCT FROM lobby.target_seat_count::text
 OR configuration->>'structure' IS DISTINCT FROM lobby.structure OR receipt.normalized_source->>'mode' IS DISTINCT FROM lobby.mode
 OR receipt.normalized_source->'competitive' IS DISTINCT FROM to_jsonb(lobby.competitive) OR lobby.visibility<>'private' THEN
 RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='claimed room differs from accepted concrete selection'; END IF;
 IF (SELECT count(*) FROM app.queue_entry WHERE claimed_room_id=p_room)<>jsonb_array_length(selection->'humanSeats') THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='claim needs exact human source roster'; END IF;
 SELECT id INTO accepted_match FROM app.match WHERE room_id=p_room;
 IF (lobby.state='started') IS DISTINCT FROM (accepted_match IS NOT NULL) THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='claim match and room state disagree'; END IF;
 IF lobby.state NOT IN ('lobby','countdown','started','closed') THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='unknown claim state'; END IF;
 FOR item IN SELECT * FROM jsonb_array_elements(selection->'humanSeats') LOOP
 SELECT * INTO entry FROM app.queue_entry WHERE id=(item->>'queueEntryId')::uuid;
 IF entry.claimed_room_id IS DISTINCT FROM p_room OR entry.claim_receipt_id IS DISTINCT FROM receipt.id OR entry.state IS DISTINCT FROM (CASE WHEN lobby.state='closed' THEN 'canceled' WHEN lobby.state='started' THEN 'reserved' ELSE 'claimed' END)
 OR entry.reserved_match_id IS DISTINCT FROM accepted_match THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='queue claim disposition differs from room source'; END IF;
 IF EXISTS(SELECT 1 FROM app.queue_entry q WHERE q.intent_receipt_id=entry.intent_receipt_id AND q.id<>entry.id AND q.state<>'canceled') THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='whole intent already consumed by claim'; END IF;
 SELECT * INTO slot FROM app.room_slot WHERE room_id=p_room AND retired_at IS NULL AND seat_index=(item->>'seatIndex')::bigint;
 IF slot.id IS NULL OR slot.controller_kind<>'human' OR slot.account_id IS DISTINCT FROM entry.account_id OR slot.deck_revision_id IS DISTINCT FROM entry.deck_revision_id
 OR slot.team_ref IS DISTINCT FROM configuration->'teams'->>((item->>'seatIndex')::integer) THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='claimed human slot differs from selected alternative'; END IF;
 IF accepted_match IS NOT NULL AND NOT EXISTS(SELECT 1 FROM app.match_seat s WHERE s.match_id=accepted_match AND s.account_id=entry.account_id AND s.seat_index=slot.seat_index AND s.source_revision_id=entry.deck_revision_id) THEN
 RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='reserved queue alternative lacks accepted matched seat'; END IF;
 END LOOP;
 FOR item IN SELECT * FROM jsonb_array_elements(selection->'aiSeats') LOOP
 SELECT * INTO entry FROM app.queue_entry WHERE id=(item->>'sponsorEntryId')::uuid;
 SELECT * INTO slot FROM app.room_slot WHERE room_id=p_room AND retired_at IS NULL AND seat_index=(item->>'seatIndex')::bigint;
 IF slot.id IS NULL OR slot.controller_kind<>'ai' OR slot.sponsor_id IS DISTINCT FROM entry.account_id OR slot.team_ref IS DISTINCT FROM configuration->'teams'->>((item->>'seatIndex')::integer) THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='claimed AI slot differs from selected allowance'; END IF;
 END LOOP;
 IF (SELECT count(*) FROM app.room_slot WHERE room_id=p_room AND retired_at IS NULL)<>lobby.target_seat_count THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='claimed room has extra slots'; END IF;
END $$;
CREATE FUNCTION app.guard_queue_claim() RETURNS trigger LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
BEGIN
 IF TG_TABLE_NAME='room' THEN PERFORM app.check_queue_claim(NEW.id);
 ELSIF TG_TABLE_NAME='room_slot' THEN IF TG_OP<>'DELETE' THEN PERFORM app.check_queue_claim(NEW.room_id);END IF;IF TG_OP<>'INSERT' THEN PERFORM app.check_queue_claim(OLD.room_id);END IF;
 ELSE IF NEW.claimed_room_id IS NOT NULL THEN PERFORM app.check_queue_claim(NEW.claimed_room_id);END IF;END IF;
 RETURN NULL;
END $$;
CREATE CONSTRAINT TRIGGER room_queue_claim_source AFTER INSERT OR UPDATE ON app.room DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION app.guard_queue_claim();
CREATE CONSTRAINT TRIGGER slot_queue_claim_source AFTER INSERT OR UPDATE OR DELETE ON app.room_slot DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION app.guard_queue_claim();
CREATE CONSTRAINT TRIGGER entry_queue_claim_source AFTER INSERT OR UPDATE ON app.queue_entry DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION app.guard_queue_claim();
CREATE FUNCTION app.guard_room_claim_identity() RETURNS trigger LANGUAGE plpgsql SET search_path='' AS $$
BEGIN
 IF (NEW.queue_claim_receipt_id,NEW.target_seat_count) IS DISTINCT FROM (OLD.queue_claim_receipt_id,OLD.target_seat_count) THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='room claim and selected seat count are immutable';END IF;RETURN NEW;
END $$;
CREATE TRIGGER room_claim_identity BEFORE UPDATE ON app.room FOR EACH ROW EXECUTE FUNCTION app.guard_room_claim_identity();

CREATE FUNCTION app.cancel_matchmaking_claim(p_actor uuid,p_entry uuid,p_request uuid,p_expected numeric,p_room uuid) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE receipt app.command_receipt; entry app.queue_entry; lobby app.room; claim_room uuid; actors uuid[]; intents uuid[];
BEGIN
 IF p_request IS NULL OR p_request='00000000-0000-0000-0000-000000000000'::uuid THEN RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid queue cancellation request'; END IF;
 IF current_setting('transaction_isolation')<>'read committed' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='queue cancellation requires read committed retry'; END IF;
 receipt:=app.begin_account_command(p_actor,p_request,'queue_cancel',coalesce(p_entry::text,''),jsonb_build_object('expected',p_expected,'room',p_room));
 IF receipt.state<>'pending' THEN RETURN receipt.response_payload; END IF;
 SELECT * INTO entry FROM app.queue_entry WHERE id=p_entry AND account_id=p_actor;
 IF entry.id IS NULL THEN RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('queue','unavailable')); END IF;
 -- A canceled sibling still identifies the whole consumed intent.
 SELECT claimed_room_id INTO claim_room FROM app.queue_entry WHERE intent_receipt_id=entry.intent_receipt_id AND claimed_room_id IS NOT NULL ORDER BY id LIMIT 1;
 SELECT array_agg(DISTINCT a ORDER BY a) INTO actors FROM(SELECT p_actor a UNION SELECT account_id FROM app.queue_entry WHERE claimed_room_id=claim_room UNION SELECT sponsor_id FROM app.room_slot WHERE room_id=claim_room AND retired_at IS NULL AND sponsor_id IS NOT NULL) ids;
 PERFORM id FROM app.account WHERE id=ANY(actors) ORDER BY id FOR NO KEY UPDATE;
 IF NOT EXISTS(SELECT 1 FROM app.account WHERE id=p_actor AND state IN ('active','deletion_pending')) THEN RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('account','unavailable'));END IF;
 SELECT array_agg(DISTINCT intent_receipt_id) INTO intents FROM app.queue_entry WHERE claimed_room_id=claim_room;
 PERFORM id FROM app.queue_entry WHERE id=p_entry OR intent_receipt_id=entry.intent_receipt_id OR intent_receipt_id=ANY(intents) ORDER BY id FOR NO KEY UPDATE;
 IF claim_room IS DISTINCT FROM (SELECT claimed_room_id FROM app.queue_entry WHERE intent_receipt_id=entry.intent_receipt_id AND claimed_room_id IS NOT NULL ORDER BY id LIMIT 1) THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='queue claim changed before account fence';END IF;
 IF claim_room IS NULL THEN
 IF EXISTS(SELECT 1 FROM app.queue_entry WHERE id=p_entry AND reserved_match_id IS NOT NULL) THEN RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('queue','already reserved'));END IF;
 UPDATE app.queue_entry SET state='canceled' WHERE (id=p_entry OR intent_receipt_id=entry.intent_receipt_id) AND state='queued';
 ELSE
 SELECT * INTO STRICT lobby FROM app.room WHERE id=claim_room FOR NO KEY UPDATE;
 IF p_room IS NOT NULL AND (p_room<>claim_room OR p_expected IS NULL OR p_expected<>lobby.revision) THEN RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('revision','conflict'));END IF;
 IF lobby.state='started' OR EXISTS(SELECT 1 FROM app.queue_entry WHERE claimed_room_id=claim_room AND reserved_match_id IS NOT NULL) THEN RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('queue','already reserved'));END IF;
 UPDATE app.controller_grant g SET state='revoked' FROM app.ai_runner_binding b JOIN app.room_slot s ON s.id=b.room_slot_id WHERE g.binding_id=b.id AND s.room_id=claim_room AND g.state='active';
 UPDATE app.ai_runner_binding b SET desired_state='stopped' FROM app.room_slot s WHERE b.room_slot_id=s.id AND s.room_id=claim_room AND b.desired_state<>'stopped';
 UPDATE app.room_slot SET ready=false WHERE room_id=claim_room AND retired_at IS NULL;
 UPDATE app.room SET state='closed',countdown_at=NULL,revision=revision+1 WHERE id=claim_room AND state<>'closed';
 UPDATE app.queue_entry SET state='canceled' WHERE claimed_room_id=claim_room AND state='claimed';
 END IF;
 RETURN app.finish_account_command(receipt.id,true,jsonb_build_object('resourceId',p_entry));
END $$;
CREATE OR REPLACE FUNCTION app.cancel_matchmaking(p_actor uuid,p_entry uuid,p_request uuid) RETURNS jsonb LANGUAGE sql SECURITY DEFINER SET search_path='' AS $$ SELECT app.cancel_matchmaking_claim(p_actor,p_entry,p_request,NULL,NULL) $$;
REVOKE ALL ON FUNCTION app.cancel_matchmaking_claim(uuid,uuid,uuid,numeric,uuid),app.check_queue_claim(uuid),app.guard_queue_claim(),app.guard_room_claim_identity() FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;

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
   SELECT id,1800000,0,'whole-match-30m-v1',true,accepted_at FROM app.match_seat WHERE app.match_seat.match_id=reserved_match;
   UPDATE app.match m SET state='ready',started_at=accepted_at,reserved_game_id=reserved_game WHERE m.id=reserved_match;
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
CREATE OR REPLACE FUNCTION app.leave_room(p_actor uuid,p_room uuid,p_expected numeric,p_request uuid)
RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE receipt app.command_receipt; lobby app.room; slot app.room_slot; queued_entry uuid; canceled jsonb;
BEGIN
 IF p_request IS NULL OR p_request='00000000-0000-0000-0000-000000000000'::uuid THEN
  RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid room request'; END IF;
 receipt:=app.begin_account_command(p_actor,p_request,'room_leave',coalesce(p_room::text,''),
  jsonb_build_object('room',p_room,'expected',p_expected));
 IF receipt.state<>'pending' THEN RETURN receipt.response_payload; END IF;
 SELECT id INTO queued_entry FROM app.queue_entry WHERE account_id=p_actor AND claimed_room_id=p_room ORDER BY id LIMIT 1;
 IF queued_entry IS NOT NULL THEN
 canceled:=app.cancel_matchmaking_claim(p_actor,queued_entry,p_request,p_expected,p_room);
 RETURN app.finish_account_command(receipt.id,canceled->>'status'='success',CASE WHEN canceled->>'status'='success' THEN jsonb_build_object('resourceId',p_room) ELSE canceled->'data' END);
 END IF;
 -- Leaving releases lobby participation and remains permitted while deletion is pending.
 PERFORM id FROM app.account WHERE id=p_actor AND state IN ('active','deletion_pending') FOR NO KEY UPDATE;
 IF NOT FOUND THEN RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('account','unavailable')); END IF;
 SELECT * INTO lobby FROM app.room WHERE id=p_room FOR NO KEY UPDATE;
 IF lobby.id IS NULL OR lobby.state NOT IN ('lobby','countdown') OR NOT EXISTS(
  SELECT 1 FROM app.room_slot WHERE retired_at IS NULL AND room_id=p_room AND account_id=p_actor AND controller_kind='human') THEN
  RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('room','unavailable')); END IF;
 IF p_expected IS NULL OR NOT (p_expected>=0 AND p_expected<power(10::numeric,78) AND scale(p_expected)=0)
 OR p_expected<>lobby.revision THEN
  RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('revision','conflict')); END IF;
 SELECT * INTO STRICT slot FROM app.room_slot
 WHERE retired_at IS NULL AND room_id=p_room AND account_id=p_actor AND controller_kind='human' FOR NO KEY UPDATE;
 IF lobby.host_id=p_actor THEN
  UPDATE app.room_slot SET ready=false WHERE retired_at IS NULL AND room_id=p_room;
  UPDATE app.room SET state='closed',countdown_at=NULL,revision=revision+1 WHERE id=p_room;
 ELSE
  UPDATE app.room_slot SET account_id=NULL,sponsor_id=NULL,deck_revision_id=NULL,ready=false WHERE id=slot.id;
  UPDATE app.room SET state='lobby',countdown_at=NULL,revision=revision+1 WHERE id=p_room;
 END IF;
 RETURN app.finish_account_command(receipt.id,true,jsonb_build_object('resourceId',p_room));
END $$;
COMMIT;
