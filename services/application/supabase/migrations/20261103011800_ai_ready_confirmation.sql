BEGIN;
ALTER TABLE app.room ADD COLUMN configuration_id uuid NOT NULL DEFAULT gen_random_uuid();
ALTER TABLE app.ai_runner_binding ADD COLUMN ready_configuration_id uuid;
ALTER TABLE app.ai_runner_binding ADD COLUMN ready_epoch numeric(78,0);
ALTER TABLE app.ai_runner_binding ADD COLUMN ready_deck_revision_id uuid REFERENCES app.deck_revision(id) ON DELETE RESTRICT;
ALTER TABLE app.ai_runner_binding ADD COLUMN ready_qualification_ref text;
ALTER TABLE app.ai_runner_binding ADD COLUMN ready_authentication_ref text;
ALTER TABLE app.ai_runner_binding ADD CONSTRAINT ai_ready_source_tuple CHECK(
 (ready_configuration_id IS NULL AND ready_epoch IS NULL AND ready_deck_revision_id IS NULL AND ready_qualification_ref IS NULL AND ready_authentication_ref IS NULL)
 OR (ready_configuration_id IS NOT NULL AND ready_epoch IS NOT NULL AND ready_epoch=binding_epoch AND ready_deck_revision_id IS NOT NULL
  AND ready_qualification_ref IS NOT NULL AND length(btrim(ready_qualification_ref)) BETWEEN 1 AND 2048
  AND ready_authentication_ref IS NOT NULL AND length(btrim(ready_authentication_ref)) BETWEEN 1 AND 2048
  AND desired_state='running' AND observed_state='ready'));
-- Stopped historical bindings remain retained; only one runner may currently
-- desire control of a slot. Its epoch identifies the browser confirmation.
CREATE UNIQUE INDEX one_desired_room_runner ON app.ai_runner_binding(room_slot_id) WHERE desired_state='running';

CREATE FUNCTION app.guard_ai_ready_lifecycle() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE parent_room uuid;
BEGIN
 IF TG_OP='INSERT' THEN
  IF NEW.ready_configuration_id IS NOT NULL OR NEW.ready_epoch IS NOT NULL OR NEW.ready_deck_revision_id IS NOT NULL OR NEW.ready_qualification_ref IS NOT NULL OR NEW.ready_authentication_ref IS NOT NULL THEN
   RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='runner readiness requires its original authenticated confirmation'; END IF;
  RETURN NEW;
 END IF;
 IF NEW.binding_epoch<>OLD.binding_epoch OR NEW.desired_state<>'running' OR NEW.observed_state<>'ready' THEN
  NEW.ready_configuration_id:=NULL;NEW.ready_epoch:=NULL;NEW.ready_deck_revision_id:=NULL;NEW.ready_qualification_ref:=NULL;NEW.ready_authentication_ref:=NULL;
  IF OLD.ready_epoch IS NOT NULL THEN
   UPDATE app.room_slot SET ready=false WHERE id=NEW.room_slot_id RETURNING room_id INTO parent_room;
   UPDATE app.room SET state='lobby',countdown_at=NULL,revision=revision+1 WHERE id=parent_room AND state IN ('lobby','countdown');
  END IF;
 END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER ai_ready_lifecycle BEFORE INSERT OR UPDATE ON app.ai_runner_binding FOR EACH ROW EXECUTE FUNCTION app.guard_ai_ready_lifecycle();

CREATE FUNCTION app.ai_slot_ready(p_slot uuid) RETURNS boolean
LANGUAGE sql STABLE SECURITY DEFINER SET search_path='' AS $$
 SELECT EXISTS(SELECT 1 FROM app.room_slot slot JOIN app.ai_runner_binding binding ON binding.room_slot_id=slot.id
  JOIN app.controller_grant grant_row ON grant_row.binding_id=binding.id AND grant_row.binding_epoch=binding.binding_epoch AND grant_row.match_seat_id IS NOT DISTINCT FROM binding.match_seat_id AND grant_row.state='active'
  JOIN app.command_receipt receipt ON receipt.controller_grant_id=grant_row.id AND receipt.principal_key='controller:'||grant_row.id::text
   AND receipt.scope='ai_ready_v1' AND receipt.target_ref=binding.id::text AND receipt.state='accepted'
  WHERE slot.id=p_slot AND slot.controller_kind='ai' AND slot.ready AND binding.desired_state='running' AND binding.observed_state='ready'
   AND binding.provider_runner_ref IS NOT NULL AND binding.ready_epoch=binding.binding_epoch
   AND binding.ready_deck_revision_id=slot.deck_revision_id
   AND binding.ready_configuration_id=(SELECT configuration_id FROM app.room WHERE id=slot.room_id)
   AND receipt.response_payload->'data'->>'configurationId'=binding.ready_configuration_id::text
   AND receipt.response_payload->'data'->>'epoch'=binding.binding_epoch::text
   AND receipt.response_payload->'data'->>'revisionId'=binding.ready_deck_revision_id::text
   AND receipt.response_payload->'data'->>'qualificationRef'=binding.ready_qualification_ref
   AND receipt.response_payload->'data'->>'authenticationRef'=binding.ready_authentication_ref)
$$;
CREATE OR REPLACE FUNCTION app.refresh_room_countdown(p_room uuid,p_force boolean) RETURNS boolean
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE lobby app.room; profile app.table_profile_revision; occupied bigint;
BEGIN
 IF p_force IS NULL THEN RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='countdown trigger kind required'; END IF;
 SELECT * INTO STRICT lobby FROM app.room WHERE id=p_room FOR NO KEY UPDATE;
 IF lobby.state='countdown' THEN RETURN false; END IF;
 IF lobby.state<>'lobby' THEN RETURN false; END IF;
 SELECT * INTO STRICT profile FROM app.table_profile_revision WHERE id=lobby.profile_id;
 SELECT count(*) INTO occupied FROM app.room_slot WHERE room_id=p_room AND (account_id IS NOT NULL OR controller_kind='ai');
 IF EXISTS(SELECT 1 FROM app.room_slot WHERE room_id=p_room AND controller_kind='ai' AND NOT app.ai_slot_ready(id)) THEN RETURN false; END IF;
 IF profile.admission_qualification_ref IS NULL OR occupied NOT BETWEEN profile.minimum_seats AND profile.seat_capacity
 OR EXISTS(SELECT 1 FROM app.room_slot s LEFT JOIN app.deck_revision revision ON revision.id=s.deck_revision_id LEFT JOIN app.deck d ON d.id=revision.deck_id
  WHERE s.room_id=p_room AND (s.account_id IS NOT NULL OR s.controller_kind='ai') AND
   (revision.id IS NULL OR revision.sealed_at IS NULL OR d.mode<>lobby.mode OR d.format_id<>lobby.format_id
    OR (s.controller_kind='human' AND (d.source_kind<>'account' OR d.account_id IS DISTINCT FROM s.account_id))
    OR (s.controller_kind='ai' AND NOT(d.source_kind='system_pool' OR (d.source_kind='account' AND d.account_id=s.sponsor_id))))) THEN RETURN false; END IF;
 IF occupied<>profile.seat_capacity THEN RETURN false; END IF;
 IF NOT p_force AND (EXISTS(SELECT 1 FROM app.room_slot WHERE room_id=p_room AND (account_id IS NOT NULL OR controller_kind='ai') AND NOT ready)) THEN RETURN false; END IF;
 UPDATE app.room SET state='countdown',countdown_at=clock_timestamp()+interval '10 seconds' WHERE id=p_room;
 RETURN true;
END $$;

CREATE FUNCTION app.commit_ai_ready_confirmation(p_grant uuid,p_epoch numeric,p_revision uuid,p_qualification text,p_authentication text,p_configuration uuid,p_request uuid) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE receipt app.command_receipt; binding app.ai_runner_binding; slot app.room_slot; lobby app.room;
 original_sponsor uuid; source_binding uuid; source_slot uuid; fingerprint text; changed boolean; accepted jsonb;
BEGIN
 IF current_setting('transaction_isolation')<>'read committed' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='AI ready confirmation requires read committed retry'; END IF;
 IF p_configuration IS NULL OR p_grant IS NULL OR p_epoch IS NULL OR p_epoch::text !~ '^(0|[1-9][0-9]{0,77})$' OR p_revision IS NULL
 OR p_qualification IS NULL OR length(btrim(p_qualification)) NOT BETWEEN 1 AND 2048 OR p_authentication IS NULL OR length(btrim(p_authentication)) NOT BETWEEN 1 AND 2048
 OR p_request IS NULL OR p_request='00000000-0000-0000-0000-000000000000'::uuid THEN RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid authenticated AI ready envelope'; END IF;
 SELECT b.id,b.room_slot_id,s.sponsor_id INTO source_binding,source_slot,original_sponsor FROM app.controller_grant g JOIN app.ai_runner_binding b ON b.id=g.binding_id JOIN app.room_slot s ON s.id=b.room_slot_id WHERE g.id=p_grant;
 IF source_binding IS NULL THEN RAISE EXCEPTION USING ERRCODE='23503',MESSAGE='AI controller grant unavailable'; END IF;
 fingerprint:=encode(sha256(convert_to(jsonb_build_object('epoch',p_epoch::text,'revision',p_revision,'qualification',p_qualification,'authentication',p_authentication,'configuration',p_configuration)::text,'UTF8')),'hex');
 INSERT INTO app.command_receipt(controller_grant_id,principal_key,scope,target_ref,request_ref,payload_hash,state,result_ref)
 VALUES(p_grant,'controller:'||p_grant::text,'ai_ready_v1',source_binding::text,p_request::text,fingerprint,'pending','') ON CONFLICT DO NOTHING RETURNING * INTO receipt;
 IF receipt.id IS NULL THEN
  SELECT * INTO STRICT receipt FROM app.command_receipt WHERE principal_key='controller:'||p_grant::text AND scope='ai_ready_v1' AND request_ref=p_request::text FOR UPDATE;
  IF receipt.target_ref<>source_binding::text OR receipt.payload_hash<>fingerprint THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='AI ready source terms conflict'; END IF;
  IF receipt.state='pending' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='AI ready disposition pending'; END IF;
  RETURN receipt.response_payload;
 END IF;
 PERFORM id FROM app.account WHERE id=original_sponsor AND state='active' FOR NO KEY UPDATE;
 IF NOT FOUND THEN RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('sponsor','unavailable')); END IF;
 SELECT * INTO slot FROM app.room_slot WHERE id=source_slot;
 SELECT * INTO lobby FROM app.room WHERE id=slot.room_id FOR NO KEY UPDATE;
 SELECT * INTO slot FROM app.room_slot WHERE id=source_slot FOR NO KEY UPDATE;
 SELECT * INTO binding FROM app.ai_runner_binding WHERE id=source_binding FOR NO KEY UPDATE;
 IF slot.sponsor_id IS DISTINCT FROM original_sponsor OR binding.room_slot_id<>slot.id THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='AI sponsor or slot changed before fence'; END IF;
 IF lobby.configuration_id IS DISTINCT FROM p_configuration THEN RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('configuration','stale')); END IF;
 IF binding.binding_epoch<>p_epoch OR binding.desired_state<>'running' OR binding.observed_state NOT IN ('running','ready') OR binding.provider_runner_ref IS NULL
  OR (lobby.state NOT IN ('lobby','countdown') AND NOT(lobby.state='started' AND EXISTS(SELECT 1 FROM app.match m JOIN app.match_seat seat ON seat.match_id=m.id WHERE seat.id=binding.match_seat_id AND m.room_id=lobby.id AND m.state='ready')))
  OR NOT EXISTS(SELECT 1 FROM app.controller_grant WHERE id=p_grant AND binding_id=binding.id AND binding_epoch=p_epoch AND match_seat_id IS NOT DISTINCT FROM binding.match_seat_id AND state='active' FOR SHARE) THEN
  RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('controller','stale or unavailable')); END IF;
 IF NOT EXISTS(SELECT 1 FROM app.deck_revision r JOIN app.deck d ON d.id=r.deck_id WHERE r.id=p_revision AND r.sealed_at IS NOT NULL AND d.mode=lobby.mode AND d.format_id=lobby.format_id
   AND (d.source_kind='system_pool' OR d.account_id=original_sponsor) FOR SHARE OF r,d) THEN
  RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('deck','unavailable')); END IF;
 IF lobby.state='started' AND slot.deck_revision_id IS DISTINCT FROM p_revision THEN RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('deck','accepted selection is immutable')); END IF;
 changed:=slot.deck_revision_id IS DISTINCT FROM p_revision;
 UPDATE app.ai_runner_binding SET observed_state='ready',ready_configuration_id=p_configuration,ready_epoch=p_epoch,ready_deck_revision_id=p_revision,ready_qualification_ref=p_qualification,ready_authentication_ref=p_authentication WHERE id=binding.id;
 UPDATE app.room_slot SET deck_revision_id=p_revision,ready=true WHERE id=slot.id;
 IF lobby.state IN ('lobby','countdown') THEN
  IF changed THEN UPDATE app.room_slot SET ready=false WHERE room_id=lobby.id AND controller_kind='human';UPDATE app.room SET state='lobby',countdown_at=NULL WHERE id=lobby.id; END IF;
  UPDATE app.room SET revision=revision+1 WHERE id=lobby.id;
 END IF;
 accepted:=app.finish_account_command(receipt.id,true,jsonb_build_object('resourceId',binding.id,'epoch',p_epoch::text,'revisionId',p_revision,'qualificationRef',p_qualification,'authenticationRef',p_authentication,'configurationId',p_configuration));
 IF lobby.state IN ('lobby','countdown') THEN PERFORM app.refresh_room_countdown(lobby.id,false); END IF;
 RETURN accepted;
END $$;

CREATE FUNCTION app.stamp_room_configuration() RETURNS trigger LANGUAGE plpgsql SET search_path='' AS $$
BEGIN
 IF (NEW.profile_id,NEW.structure,NEW.format_id,NEW.mode) IS DISTINCT FROM (OLD.profile_id,OLD.structure,OLD.format_id,OLD.mode) THEN NEW.configuration_id:=gen_random_uuid(); END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER room_configuration_identity BEFORE UPDATE ON app.room FOR EACH ROW EXECUTE FUNCTION app.stamp_room_configuration();
CREATE FUNCTION app.stamp_slot_configuration() RETURNS trigger LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE targets uuid[];
BEGIN
 IF TG_OP='UPDATE' AND (NEW.room_id,NEW.account_id,NEW.sponsor_id,NEW.controller_kind,NEW.seat_index,NEW.team_ref) IS NOT DISTINCT FROM
   (OLD.room_id,OLD.account_id,OLD.sponsor_id,OLD.controller_kind,OLD.seat_index,OLD.team_ref) THEN RETURN NULL; END IF;
 IF TG_OP='INSERT' THEN targets:=ARRAY[NEW.room_id]; ELSIF TG_OP='DELETE' THEN targets:=ARRAY[OLD.room_id]; ELSE targets:=ARRAY[OLD.room_id,NEW.room_id]; END IF;
 UPDATE app.room SET configuration_id=gen_random_uuid() WHERE id=ANY(targets) AND state IN ('lobby','countdown');
 RETURN NULL;
END $$;
CREATE TRIGGER slot_configuration_identity AFTER INSERT OR UPDATE OR DELETE ON app.room_slot FOR EACH ROW EXECUTE FUNCTION app.stamp_slot_configuration();
CREATE FUNCTION app.invalidate_ai_ready_source() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
BEGIN
 IF TG_TABLE_NAME='controller_grant' THEN
  IF OLD.state='active' AND NEW.state='revoked' THEN UPDATE app.ai_runner_binding SET observed_state='running' WHERE id=NEW.binding_id AND ready_epoch IS NOT NULL; END IF;
 ELSIF TG_TABLE_NAME='room_slot' THEN
  IF NEW.controller_kind='ai' AND (NEW.team_ref IS DISTINCT FROM OLD.team_ref OR NEW.deck_revision_id IS DISTINCT FROM OLD.deck_revision_id) THEN
   UPDATE app.ai_runner_binding SET observed_state='running' WHERE room_slot_id=NEW.id AND ready_epoch IS NOT NULL
    AND (NEW.team_ref IS DISTINCT FROM OLD.team_ref OR ready_deck_revision_id IS DISTINCT FROM NEW.deck_revision_id);
  END IF;
 ELSE
  IF NEW.configuration_id IS DISTINCT FROM OLD.configuration_id THEN
   UPDATE app.ai_runner_binding b SET observed_state='running' FROM app.room_slot slot WHERE b.room_slot_id=slot.id AND slot.room_id=NEW.id AND b.ready_epoch IS NOT NULL;
  END IF;
 END IF;
 RETURN NULL;
END $$;
CREATE TRIGGER ai_grant_ready_invalidation AFTER UPDATE ON app.controller_grant FOR EACH ROW EXECUTE FUNCTION app.invalidate_ai_ready_source();
CREATE TRIGGER ai_deck_ready_invalidation AFTER UPDATE OF deck_revision_id,team_ref ON app.room_slot FOR EACH ROW EXECUTE FUNCTION app.invalidate_ai_ready_source();
CREATE TRIGGER ai_profile_ready_invalidation AFTER UPDATE ON app.room FOR EACH ROW EXECUTE FUNCTION app.invalidate_ai_ready_source();
CREATE FUNCTION app.check_ai_grant_lifecycle() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE target uuid; desired text;
BEGIN
 IF TG_TABLE_NAME='controller_grant' THEN target:=NEW.binding_id; ELSE target:=NEW.id; END IF;
 IF current_setting('transaction_isolation')<>'read committed' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='controller lifecycle requires read committed retry'; END IF;
 SELECT desired_state INTO desired FROM app.ai_runner_binding WHERE id=target FOR NO KEY UPDATE;
 IF desired<>'running' AND EXISTS(SELECT 1 FROM app.controller_grant WHERE binding_id=target AND state='active') THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='stopped controller cannot retain active grants'; END IF;
 RETURN NULL;
END $$;
CREATE CONSTRAINT TRIGGER ai_binding_grant_lifecycle AFTER INSERT OR UPDATE ON app.ai_runner_binding DEFERRABLE INITIALLY IMMEDIATE FOR EACH ROW EXECUTE FUNCTION app.check_ai_grant_lifecycle();
CREATE CONSTRAINT TRIGGER ai_grant_binding_lifecycle AFTER INSERT OR UPDATE ON app.controller_grant DEFERRABLE INITIALLY IMMEDIATE FOR EACH ROW EXECUTE FUNCTION app.check_ai_grant_lifecycle();

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
  SELECT account_id actor FROM app.room_slot WHERE room_id=p_room AND account_id IS NOT NULL
  UNION SELECT sponsor_id FROM app.room_slot WHERE room_id=p_room AND sponsor_id IS NOT NULL
  UNION SELECT host_id FROM app.room WHERE id=p_room) roster;
 PERFORM id FROM app.account WHERE id=ANY(actors) ORDER BY id FOR NO KEY UPDATE;
 SELECT * INTO lobby FROM app.room WHERE id=p_room FOR NO KEY UPDATE;
 IF lobby.id IS NULL OR lobby.state<>'countdown' OR lobby.revision<>p_expected OR lobby.countdown_at IS NULL OR lobby.countdown_at>clock_timestamp()
 OR EXISTS(SELECT 1 FROM app.match WHERE room_id=p_room) THEN RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('room','stale, not due or already reserved')); END IF;
 IF EXISTS(SELECT 1 FROM app.room_slot WHERE room_id=p_room AND ((account_id IS NOT NULL AND NOT(account_id=ANY(coalesce(actors,'{}'::uuid[])))) OR (sponsor_id IS NOT NULL AND NOT(sponsor_id=ANY(coalesce(actors,'{}'::uuid[])))))) THEN
  RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='room roster changed before account fence'; END IF;
 IF EXISTS(SELECT 1 FROM unnest(actors) members(actor_id) WHERE NOT EXISTS(SELECT 1 FROM app.account a WHERE a.id=members.actor_id AND a.state='active')) THEN
  failure:=jsonb_build_object('account','unavailable'); rejected:=true;
 END IF;
 IF NOT rejected AND EXISTS(SELECT 1 FROM app.room_slot WHERE room_id=p_room AND controller_kind='ai' AND NOT app.ai_slot_ready(id)) THEN failure:=jsonb_build_object('ai','authenticated current-epoch readiness required'); rejected:=true; END IF;
 IF NOT rejected THEN
  IF jsonb_array_length(p_inputs)<>(SELECT count(*) FROM app.room_slot WHERE room_id=p_room AND (account_id IS NOT NULL OR controller_kind='ai')) THEN
   failure:=jsonb_build_object('inputs','complete occupied roster required'); rejected:=true;
  END IF;
 END IF;
 IF NOT rejected THEN
  BEGIN
   INSERT INTO app.match(room_id,format_id,profile_id,legality_revision_id,mode,competitive,structure,state,revision)
   VALUES(p_room,lobby.format_id,lobby.profile_id,p_legality,lobby.mode,lobby.competitive,lobby.structure,'admitting',0) RETURNING id INTO reserved_match;
   FOR item IN SELECT * FROM jsonb_array_elements(p_inputs) LOOP
    IF jsonb_typeof(item) IS DISTINCT FROM 'object' OR jsonb_typeof(item->'slotId') IS DISTINCT FROM 'string' OR (item->>'slotId') !~ '^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$'
     OR jsonb_typeof(item->'revisionId') IS DISTINCT FROM 'string' OR (item->>'revisionId') !~ '^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$'
     OR jsonb_typeof(item->'snapshotRef') IS DISTINCT FROM 'string' OR length(btrim(item->>'snapshotRef')) NOT BETWEEN 1 AND 2048
     OR EXISTS(SELECT 1 FROM jsonb_object_keys(item) k WHERE k NOT IN ('slotId','revisionId','snapshotRef')) THEN RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid room start seat artifact'; END IF;
    SELECT * INTO slot FROM app.room_slot WHERE id=(item->>'slotId')::uuid AND room_id=p_room AND (account_id IS NOT NULL OR controller_kind='ai') FOR NO KEY UPDATE;
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
  UPDATE app.room_slot SET ready=false WHERE room_id=p_room;
  UPDATE app.room SET state='lobby',countdown_at=NULL,revision=revision+1 WHERE id=p_room;
  RETURN app.finish_account_command(receipt.id,false,failure);
 END IF;
 UPDATE app.room SET state='started',countdown_at=NULL,revision=revision+1 WHERE id=p_room;
 UPDATE app.invitation SET state='canceled' WHERE room_id=p_room AND state='active';
 RETURN app.finish_account_command(receipt.id,true,jsonb_build_object('resourceId',reserved_match,'gameId',reserved_game));
END $$;

REVOKE ALL ON FUNCTION app.stamp_room_configuration(),app.stamp_slot_configuration(),app.invalidate_ai_ready_source(),app.check_ai_grant_lifecycle(),app.guard_ai_ready_lifecycle(),app.ai_slot_ready(uuid),app.commit_ai_ready_confirmation(uuid,numeric,uuid,text,text,uuid,uuid)
 FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;
COMMIT;
