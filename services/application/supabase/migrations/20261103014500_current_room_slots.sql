BEGIN;
ALTER TABLE app.room_slot ADD COLUMN retired_at timestamptz CHECK(retired_at IS NULL OR isfinite(retired_at));
ALTER TABLE app.room_slot DROP CONSTRAINT room_slot_room_id_seat_index_key;
CREATE UNIQUE INDEX current_room_slot_index ON app.room_slot(room_id,seat_index) WHERE retired_at IS NULL;
CREATE UNIQUE INDEX current_room_human_identity ON app.room_slot(room_id,account_id) WHERE retired_at IS NULL AND account_id IS NOT NULL;
CREATE UNIQUE INDEX match_human_identity ON app.match_seat(match_id,account_id) WHERE account_id IS NOT NULL;
CREATE FUNCTION app.guard_retired_room_slot() RETURNS trigger LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
BEGIN
 IF TG_OP='INSERT' THEN
 IF NEW.retired_at IS NOT NULL THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='new room slot must start current'; END IF;
 RETURN NEW;
 END IF;
 IF OLD.retired_at IS NOT NULL AND NEW IS DISTINCT FROM OLD THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='retired room slot history is immutable'; END IF;
 IF NEW.retired_at IS NOT NULL AND OLD.retired_at IS NULL THEN
 IF OLD.controller_kind<>'ai' OR NOT EXISTS(SELECT 1 FROM app.room r WHERE r.id=OLD.room_id AND r.state IN ('lobby','countdown','closed')) OR NEW.ready OR (NEW.id,NEW.room_id,NEW.account_id,NEW.sponsor_id,NEW.deck_revision_id,NEW.seat_index,NEW.team_ref,NEW.controller_kind) IS DISTINCT FROM (OLD.id,OLD.room_id,OLD.account_id,OLD.sponsor_id,OLD.deck_revision_id,OLD.seat_index,OLD.team_ref,OLD.controller_kind)
 OR EXISTS(SELECT 1 FROM app.ai_runner_binding b WHERE b.room_slot_id=OLD.id AND (b.desired_state<>'stopped' OR b.match_seat_id IS NOT NULL OR EXISTS(SELECT 1 FROM app.controller_grant g WHERE g.binding_id=b.id AND g.state='active')))
 THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='slot retirement requires revoked stopped pregame bindings and retained identity'; END IF;
 END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER retired_room_slot_history BEFORE INSERT OR UPDATE ON app.room_slot FOR EACH ROW EXECUTE FUNCTION app.guard_retired_room_slot();
REVOKE ALL ON FUNCTION app.guard_retired_room_slot() FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;


CREATE OR REPLACE FUNCTION app.select_room_deck(p_actor uuid,p_room uuid,p_expected numeric,p_revision uuid,p_request uuid)
RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE receipt app.command_receipt; lobby app.room; slot app.room_slot;
BEGIN
 IF p_request IS NULL OR p_request='00000000-0000-0000-0000-000000000000'::uuid THEN
  RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid room request'; END IF;
 receipt:=app.begin_account_command(p_actor,p_request,'room_deck',coalesce(p_room::text,''),
  jsonb_build_object('room',p_room,'expected',p_expected,'revision',p_revision));
 IF receipt.state<>'pending' THEN RETURN receipt.response_payload; END IF;
 PERFORM id FROM app.account WHERE id=p_actor AND state='active' FOR NO KEY UPDATE;
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
 IF p_revision IS NOT NULL AND NOT EXISTS(
  SELECT 1 FROM app.deck_revision r JOIN app.deck d ON d.id=r.deck_id
  WHERE r.id=p_revision AND r.sealed_at IS NOT NULL AND d.source_kind='account'
   AND d.account_id=p_actor AND d.mode=lobby.mode AND d.format_id=lobby.format_id
  FOR SHARE OF r,d) THEN
  RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('deck','unavailable')); END IF;
 IF slot.deck_revision_id IS DISTINCT FROM p_revision THEN
  UPDATE app.room_slot SET deck_revision_id=p_revision,ready=false WHERE id=slot.id;
  UPDATE app.room SET state='lobby',countdown_at=NULL,revision=revision+1 WHERE id=p_room;
 END IF;
 RETURN app.finish_account_command(receipt.id,true,jsonb_build_object('resourceId',p_room));
END $$;

CREATE OR REPLACE FUNCTION app.leave_room(p_actor uuid,p_room uuid,p_expected numeric,p_request uuid)
RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE receipt app.command_receipt; lobby app.room; slot app.room_slot;
BEGIN
 IF p_request IS NULL OR p_request='00000000-0000-0000-0000-000000000000'::uuid THEN
  RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid room request'; END IF;
 receipt:=app.begin_account_command(p_actor,p_request,'room_leave',coalesce(p_room::text,''),
  jsonb_build_object('room',p_room,'expected',p_expected));
 IF receipt.state<>'pending' THEN RETURN receipt.response_payload; END IF;
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

CREATE OR REPLACE FUNCTION app.check_controller_lineage(p_bindings uuid[]) RETURNS void
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE binding app.ai_runner_binding; slot app.room_slot; seat app.match_seat; source_room uuid;
BEGIN
 IF current_setting('transaction_isolation')<>'read committed' THEN
  RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='controller lineage changes require read committed retry'; END IF;
 FOR binding IN SELECT * FROM app.ai_runner_binding WHERE id=ANY(p_bindings) ORDER BY id FOR NO KEY UPDATE LOOP
  SELECT * INTO slot FROM app.room_slot WHERE id=binding.room_slot_id FOR SHARE;
  IF slot.id IS NULL OR slot.controller_kind<>'ai' OR slot.account_id IS NOT NULL OR slot.sponsor_id IS NULL THEN
   RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='runner binding requires a sponsored AI room slot'; END IF;
  IF slot.retired_at IS NOT NULL AND (binding.desired_state<>'stopped' OR EXISTS(SELECT 1 FROM app.controller_grant WHERE binding_id=binding.id AND state='active')) THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='retired slot cannot retain desired running or live authority'; END IF;
  IF binding.match_seat_id IS NOT NULL THEN
   SELECT * INTO seat FROM app.match_seat WHERE id=binding.match_seat_id FOR SHARE;
   SELECT room_id INTO source_room FROM app.match WHERE id=seat.match_id FOR SHARE;
   IF seat.id IS NULL OR seat.controller_kind<>'ai' OR seat.account_id IS NOT NULL
    OR seat.sponsor_id IS DISTINCT FROM slot.sponsor_id OR seat.seat_index<>slot.seat_index
    OR source_room IS DISTINCT FROM slot.room_id THEN
    RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='runner seat must preserve its room slot and sponsor'; END IF;
  END IF;
  IF EXISTS(SELECT 1 FROM app.controller_grant g WHERE g.binding_id=binding.id AND g.state='active'
   AND (g.binding_epoch<>binding.binding_epoch OR g.match_seat_id IS DISTINCT FROM binding.match_seat_id)) THEN
   RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='active controller grants must match the current binding epoch and seat'; END IF;
 END LOOP;
END $$;

CREATE OR REPLACE FUNCTION app.guard_room_capacity() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE targets uuid[];
BEGIN
 IF TG_TABLE_NAME='room' THEN targets:=ARRAY[NEW.id];
 ELSE
  IF TG_OP='INSERT' THEN targets:=ARRAY[NEW.room_id]; ELSE targets:=ARRAY[OLD.room_id,NEW.room_id]; END IF;
 END IF;
 IF current_setting('transaction_isolation')<>'read committed' THEN
  RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='room capacity changes require read committed retry'; END IF;
 PERFORM id FROM app.room WHERE id=ANY(targets) ORDER BY id FOR NO KEY UPDATE;
 IF EXISTS(SELECT 1 FROM app.room r JOIN app.table_profile_revision p ON p.id=r.profile_id
  JOIN app.room_slot s ON s.room_id=r.id WHERE s.retired_at IS NULL AND r.id=ANY(targets) AND
   (s.seat_index<0 OR (p.seat_capacity IS NOT NULL AND s.seat_index>=p.seat_capacity)
    OR (s.controller_kind='human' AND s.sponsor_id IS NOT NULL)
    OR (s.controller_kind='ai' AND (s.account_id IS NOT NULL OR s.sponsor_id IS NULL))
    OR s.controller_kind NOT IN ('human','ai') OR (r.competitive AND s.controller_kind='ai'))) THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='room slot exceeds its profile or controller configuration'; END IF;
 RETURN NULL;
END $$;

CREATE OR REPLACE FUNCTION app.join_room(p_actor uuid,p_room uuid,p_token_hash text,p_request uuid) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE receipt app.command_receipt; lobby app.room; original_host uuid; profile app.table_profile_revision; slot app.room_slot; chosen bigint;
BEGIN
 IF p_request IS NULL OR p_request='00000000-0000-0000-0000-000000000000'::uuid THEN RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid lobby source request'; END IF;
 IF current_setting('transaction_isolation')<>'read committed' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='lobby and queue sources require read committed retry'; END IF;
 receipt:=app.begin_account_command(p_actor,p_request,'room_join',coalesce(p_room::text,''),jsonb_build_object('tokenHash',p_token_hash));
 IF receipt.state<>'pending' THEN RETURN receipt.response_payload; END IF;
 SELECT host_id INTO original_host FROM app.room WHERE id=p_room;
 PERFORM id FROM app.account WHERE id IN (p_actor,original_host) ORDER BY id FOR NO KEY UPDATE;
 IF NOT EXISTS(SELECT 1 FROM app.account WHERE id=p_actor AND state='active') OR NOT EXISTS(SELECT 1 FROM app.account WHERE id=original_host AND state='active') THEN
  RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('room','unavailable')); END IF;
 SELECT * INTO lobby FROM app.room WHERE id=p_room FOR NO KEY UPDATE;
 IF lobby.id IS NULL OR lobby.state NOT IN ('lobby','countdown') THEN RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('room','unavailable')); END IF;
 IF lobby.host_id<>original_host THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='room host changed before account fence'; END IF;
 SELECT * INTO slot FROM app.room_slot WHERE retired_at IS NULL AND room_id=p_room AND account_id=p_actor AND controller_kind='human';
 IF slot.id IS NOT NULL THEN RETURN app.finish_account_command(receipt.id,true,jsonb_build_object('resourceId',p_room,'slotId',slot.id)); END IF;
 IF lobby.visibility<>'public' AND NOT EXISTS(SELECT 1 FROM app.invitation WHERE room_id=p_room AND token_hash=p_token_hash AND state='active') THEN
  RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('invitation','unavailable')); END IF;
 SELECT * INTO STRICT profile FROM app.table_profile_revision WHERE id=lobby.profile_id;
 IF profile.admission_qualification_ref IS NULL THEN RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('profile','unqualified')); END IF;
 SELECT * INTO slot FROM app.room_slot WHERE retired_at IS NULL AND room_id=p_room AND controller_kind='human' AND account_id IS NULL ORDER BY seat_index LIMIT 1 FOR NO KEY UPDATE;
 IF slot.id IS NULL THEN
  SELECT min(candidate) INTO chosen FROM (SELECT 0::bigint candidate UNION SELECT seat_index+1 FROM app.room_slot WHERE retired_at IS NULL AND room_id=p_room AND seat_index<profile.seat_capacity-1) candidates
   WHERE candidate<profile.seat_capacity AND NOT EXISTS(SELECT 1 FROM app.room_slot WHERE retired_at IS NULL AND room_id=p_room AND seat_index=candidate);
  IF chosen IS NULL THEN RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('capacity','full')); END IF;
  INSERT INTO app.room_slot(room_id,account_id,seat_index,controller_kind,ready) VALUES(p_room,p_actor,chosen,'human',false) RETURNING * INTO slot;
 ELSE UPDATE app.room_slot SET account_id=p_actor,sponsor_id=NULL,deck_revision_id=NULL,ready=false WHERE id=slot.id;
 END IF;
 UPDATE app.room SET state='lobby',countdown_at=NULL,revision=revision+1 WHERE id=p_room;
 RETURN app.finish_account_command(receipt.id,true,jsonb_build_object('resourceId',p_room,'slotId',slot.id));
END $$;

CREATE OR REPLACE FUNCTION app.refresh_room_countdown(p_room uuid,p_force boolean) RETURNS boolean
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE lobby app.room; profile app.table_profile_revision; occupied bigint;
BEGIN
 IF p_force IS NULL THEN RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='countdown trigger kind required'; END IF;
 SELECT * INTO STRICT lobby FROM app.room WHERE id=p_room FOR NO KEY UPDATE;
 IF lobby.state='countdown' THEN RETURN false; END IF;
 IF lobby.state<>'lobby' THEN RETURN false; END IF;
 SELECT * INTO STRICT profile FROM app.table_profile_revision WHERE id=lobby.profile_id;
 SELECT count(*) INTO occupied FROM app.room_slot WHERE retired_at IS NULL AND room_id=p_room AND (account_id IS NOT NULL OR controller_kind='ai');
 IF EXISTS(SELECT 1 FROM app.room_slot WHERE retired_at IS NULL AND room_id=p_room AND controller_kind='ai' AND NOT app.ai_slot_ready(id)) THEN RETURN false; END IF;
 IF profile.admission_qualification_ref IS NULL OR occupied NOT BETWEEN profile.minimum_seats AND profile.seat_capacity
 OR EXISTS(SELECT 1 FROM app.room_slot s LEFT JOIN app.deck_revision revision ON revision.id=s.deck_revision_id LEFT JOIN app.deck d ON d.id=revision.deck_id
  WHERE s.retired_at IS NULL AND s.room_id=p_room AND (s.account_id IS NOT NULL OR s.controller_kind='ai') AND
   (revision.id IS NULL OR revision.sealed_at IS NULL OR d.mode<>lobby.mode OR d.format_id<>lobby.format_id
    OR (s.controller_kind='human' AND (d.source_kind<>'account' OR d.account_id IS DISTINCT FROM s.account_id))
    OR (s.controller_kind='ai' AND NOT(d.source_kind='system_pool' OR (d.source_kind='account' AND d.account_id=s.sponsor_id))))) THEN RETURN false; END IF;
 IF occupied<>profile.seat_capacity THEN RETURN false; END IF;
 IF NOT p_force AND (EXISTS(SELECT 1 FROM app.room_slot WHERE retired_at IS NULL AND room_id=p_room AND (account_id IS NOT NULL OR controller_kind='ai') AND NOT ready)) THEN RETURN false; END IF;
 UPDATE app.room SET state='countdown',countdown_at=clock_timestamp()+interval '10 seconds' WHERE id=p_room;
 RETURN true;
END $$;

CREATE OR REPLACE FUNCTION app.set_room_ready(p_actor uuid,p_room uuid,p_expected numeric,p_ready boolean,p_revision uuid,p_qualification text,p_request uuid) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE receipt app.command_receipt; lobby app.room; slot app.room_slot;
BEGIN
 IF p_request IS NULL OR p_request='00000000-0000-0000-0000-000000000000'::uuid THEN RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid room source request'; END IF;
 IF current_setting('transaction_isolation')<>'read committed' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='room ready requires read committed retry'; END IF;
 receipt:=app.begin_account_command(p_actor,p_request,'room_ready',coalesce(p_room::text,''),jsonb_build_object('basis',p_expected,'ready',p_ready,'revision',p_revision,'qualification',p_qualification));
 IF receipt.state<>'pending' THEN RETURN receipt.response_payload; END IF;
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

CREATE OR REPLACE FUNCTION app.configure_room(p_actor uuid,p_room uuid,p_expected numeric,p_profile uuid,p_structure text,p_teams jsonb,p_request uuid) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE receipt app.command_receipt; lobby app.room; profile app.table_profile_revision; item jsonb; changed boolean;
BEGIN
 IF p_request IS NULL OR p_request='00000000-0000-0000-0000-000000000000'::uuid THEN RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid room source request'; END IF;
 IF current_setting('transaction_isolation')<>'read committed' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='room configuration requires read committed retry'; END IF;
 receipt:=app.begin_account_command(p_actor,p_request,'room_config',coalesce(p_room::text,''),jsonb_build_object('basis',p_expected,'profile',p_profile,'structure',p_structure,'teams',p_teams));
 IF receipt.state<>'pending' THEN RETURN receipt.response_payload; END IF;
 PERFORM id FROM app.account WHERE id=p_actor AND state='active' FOR NO KEY UPDATE;
 IF NOT FOUND THEN RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('account','unavailable')); END IF;
 SELECT * INTO lobby FROM app.room WHERE id=p_room FOR NO KEY UPDATE;
 IF lobby.id IS NULL OR lobby.host_id<>p_actor OR lobby.state NOT IN ('lobby','countdown') THEN RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('room','unavailable')); END IF;
 IF p_expected IS NULL OR p_expected::text !~ '^(0|[1-9][0-9]{0,77})$' OR p_expected<>lobby.revision THEN RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('revision','conflict')); END IF;
 SELECT * INTO profile FROM app.table_profile_revision WHERE id=p_profile;
 IF profile.id IS NULL OR profile.admission_qualification_ref IS NULL OR p_structure IS NULL OR p_structure NOT IN ('bo1','bo3') OR (p_structure='bo3' AND profile.seat_capacity<>2)
 OR EXISTS(SELECT 1 FROM app.room_slot WHERE retired_at IS NULL AND room_id=p_room AND seat_index>=profile.seat_capacity)
 OR jsonb_typeof(p_teams) IS DISTINCT FROM 'array' THEN RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('configuration','unavailable')); END IF;
 IF jsonb_array_length(p_teams)<>(SELECT count(*) FROM app.room_slot WHERE retired_at IS NULL AND room_id=p_room) THEN RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('teams','complete slot assignment required')); END IF;
 FOR item IN SELECT * FROM jsonb_array_elements(p_teams) LOOP
  IF jsonb_typeof(item) IS DISTINCT FROM 'object' OR jsonb_typeof(item->'slotId') IS DISTINCT FROM 'string' OR (item->>'slotId') !~ '^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$'
   OR NOT(item ? 'teamRef') OR jsonb_typeof(item->'teamRef') NOT IN ('string','null')
   OR (jsonb_typeof(item->'teamRef')='string' AND length(btrim(item->>'teamRef')) NOT BETWEEN 1 AND 128)
   OR EXISTS(SELECT 1 FROM jsonb_object_keys(item) k WHERE k NOT IN ('slotId','teamRef')) THEN RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid team assignment'; END IF;
  IF NOT EXISTS(SELECT 1 FROM app.room_slot WHERE retired_at IS NULL AND id=(item->>'slotId')::uuid AND room_id=p_room) THEN RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('teams','foreign slot')); END IF;
 END LOOP;
 IF (SELECT count(DISTINCT v->>'slotId') FROM jsonb_array_elements(p_teams) v)<>jsonb_array_length(p_teams) THEN RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('teams','duplicate slot')); END IF;
 changed:=(lobby.profile_id,lobby.structure) IS DISTINCT FROM (p_profile,p_structure)
  OR EXISTS(SELECT 1 FROM jsonb_array_elements(p_teams) v JOIN app.room_slot s ON s.id=(v->>'slotId')::uuid WHERE s.team_ref IS DISTINCT FROM v->>'teamRef');
 IF changed THEN
  FOR item IN SELECT * FROM jsonb_array_elements(p_teams) LOOP UPDATE app.room_slot SET team_ref=item->>'teamRef' WHERE id=(item->>'slotId')::uuid; END LOOP;
  UPDATE app.room_slot SET ready=false WHERE retired_at IS NULL AND room_id=p_room AND controller_kind='human';
  UPDATE app.room SET profile_id=p_profile,structure=p_structure,state='lobby',countdown_at=NULL,revision=revision+1 WHERE id=p_room RETURNING * INTO lobby;
 END IF;
 SELECT * INTO STRICT lobby FROM app.room WHERE id=p_room;
 RETURN app.finish_account_command(receipt.id,true,jsonb_build_object('resourceId',p_room,'revision',lobby.revision::text));
END $$;

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
 UPDATE app.room SET state='started',countdown_at=NULL,revision=revision+1 WHERE id=p_room;
 UPDATE app.invitation SET state='canceled' WHERE room_id=p_room AND state='active';
 RETURN app.finish_account_command(receipt.id,true,jsonb_build_object('resourceId',reserved_match,'gameId',reserved_game));
END $$;

CREATE OR REPLACE FUNCTION app.ai_slot_ready(p_slot uuid) RETURNS boolean
LANGUAGE sql STABLE SECURITY DEFINER SET search_path='' AS $$
 SELECT EXISTS(SELECT 1 FROM app.room_slot slot JOIN app.ai_runner_binding binding ON binding.room_slot_id=slot.id
  JOIN app.controller_grant grant_row ON grant_row.binding_id=binding.id AND grant_row.binding_epoch=binding.binding_epoch AND grant_row.match_seat_id IS NOT DISTINCT FROM binding.match_seat_id AND grant_row.state='active'
  JOIN app.command_receipt receipt ON receipt.controller_grant_id=grant_row.id AND receipt.principal_key='controller:'||grant_row.id::text
   AND receipt.scope='ai_ready_v1' AND receipt.target_ref=binding.id::text AND receipt.state='accepted'
  WHERE slot.retired_at IS NULL AND slot.id=p_slot AND slot.controller_kind='ai' AND slot.ready AND binding.desired_state='running' AND binding.observed_state='ready'
   AND binding.provider_runner_ref IS NOT NULL AND binding.ready_epoch=binding.binding_epoch
   AND binding.ready_deck_revision_id=slot.deck_revision_id
   AND binding.ready_configuration_id=(SELECT configuration_id FROM app.room WHERE id=slot.room_id)
   AND receipt.response_payload->'data'->>'configurationId'=binding.ready_configuration_id::text
   AND receipt.response_payload->'data'->>'epoch'=binding.binding_epoch::text
   AND receipt.response_payload->'data'->>'revisionId'=binding.ready_deck_revision_id::text
   AND receipt.response_payload->'data'->>'qualificationRef'=binding.ready_qualification_ref
   AND receipt.response_payload->'data'->>'authenticationRef'=binding.ready_authentication_ref)
$$;

CREATE OR REPLACE FUNCTION app.stamp_slot_configuration() RETURNS trigger LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE targets uuid[];
BEGIN
 IF TG_OP='UPDATE' AND (NEW.room_id,NEW.account_id,NEW.sponsor_id,NEW.controller_kind,NEW.seat_index,NEW.team_ref,NEW.retired_at) IS NOT DISTINCT FROM
   (OLD.room_id,OLD.account_id,OLD.sponsor_id,OLD.controller_kind,OLD.seat_index,OLD.team_ref,OLD.retired_at) THEN RETURN NULL; END IF;
 IF TG_OP='INSERT' THEN targets:=ARRAY[NEW.room_id]; ELSIF TG_OP='DELETE' THEN targets:=ARRAY[OLD.room_id]; ELSE targets:=ARRAY[OLD.room_id,NEW.room_id]; END IF;
 UPDATE app.room SET configuration_id=gen_random_uuid() WHERE id=ANY(targets) AND state IN ('lobby','countdown');
 RETURN NULL;
END $$;

CREATE OR REPLACE FUNCTION app.commit_ai_ready_confirmation(p_grant uuid,p_epoch numeric,p_revision uuid,p_qualification text,p_authentication text,p_configuration uuid,p_request uuid) RETURNS jsonb
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
 SELECT * INTO slot FROM app.room_slot WHERE retired_at IS NULL AND id=source_slot;
 SELECT * INTO lobby FROM app.room WHERE id=slot.room_id FOR NO KEY UPDATE;
 SELECT * INTO slot FROM app.room_slot WHERE retired_at IS NULL AND id=source_slot FOR NO KEY UPDATE;
 IF slot.id IS NULL THEN RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('slot','retired or unavailable')); END IF;
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
  IF changed THEN UPDATE app.room_slot SET ready=false WHERE retired_at IS NULL AND room_id=lobby.id AND controller_kind='human';UPDATE app.room SET state='lobby',countdown_at=NULL WHERE id=lobby.id; END IF;
  UPDATE app.room SET revision=revision+1 WHERE id=lobby.id;
 END IF;
 accepted:=app.finish_account_command(receipt.id,true,jsonb_build_object('resourceId',binding.id,'epoch',p_epoch::text,'revisionId',p_revision,'qualificationRef',p_qualification,'authenticationRef',p_authentication,'configurationId',p_configuration));
 IF lobby.state IN ('lobby','countdown') THEN PERFORM app.refresh_room_countdown(lobby.id,false); END IF;
 RETURN accepted;
END $$;
COMMIT;
