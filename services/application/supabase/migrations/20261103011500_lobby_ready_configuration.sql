BEGIN;
ALTER TABLE app.room ADD COLUMN structure text NOT NULL DEFAULT 'bo1' CHECK(structure IN ('bo1','bo3'));

CREATE FUNCTION app.refresh_room_countdown(p_room uuid,p_force boolean) RETURNS boolean
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE lobby app.room; profile app.table_profile_revision; occupied bigint;
BEGIN
 IF p_force IS NULL THEN RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='countdown trigger kind required'; END IF;
 SELECT * INTO STRICT lobby FROM app.room WHERE id=p_room FOR NO KEY UPDATE;
 IF lobby.state='countdown' THEN RETURN false; END IF;
 IF lobby.state<>'lobby' THEN RETURN false; END IF;
 SELECT * INTO STRICT profile FROM app.table_profile_revision WHERE id=lobby.profile_id;
 SELECT count(*) INTO occupied FROM app.room_slot WHERE room_id=p_room AND (account_id IS NOT NULL OR controller_kind='ai');
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
CREATE FUNCTION app.set_room_ready(p_actor uuid,p_room uuid,p_expected numeric,p_ready boolean,p_revision uuid,p_qualification text,p_request uuid) RETURNS jsonb
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
 SELECT * INTO slot FROM app.room_slot WHERE room_id=p_room AND account_id=p_actor AND controller_kind='human' FOR NO KEY UPDATE;
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
CREATE FUNCTION app.start_room_countdown(p_actor uuid,p_room uuid,p_expected numeric,p_qualification text,p_request uuid) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE receipt app.command_receipt; lobby app.room;
BEGIN
 IF p_request IS NULL OR p_request='00000000-0000-0000-0000-000000000000'::uuid THEN RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid room source request'; END IF;
 IF current_setting('transaction_isolation')<>'read committed' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='room countdown requires read committed retry'; END IF;
 receipt:=app.begin_account_command(p_actor,p_request,'room_countdown',coalesce(p_room::text,''),jsonb_build_object('basis',p_expected,'qualification',p_qualification));
 IF receipt.state<>'pending' THEN RETURN receipt.response_payload; END IF;
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
CREATE FUNCTION app.configure_room(p_actor uuid,p_room uuid,p_expected numeric,p_profile uuid,p_structure text,p_teams jsonb,p_request uuid) RETURNS jsonb
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
 OR EXISTS(SELECT 1 FROM app.room_slot WHERE room_id=p_room AND seat_index>=profile.seat_capacity)
 OR jsonb_typeof(p_teams) IS DISTINCT FROM 'array' THEN RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('configuration','unavailable')); END IF;
 IF jsonb_array_length(p_teams)<>(SELECT count(*) FROM app.room_slot WHERE room_id=p_room) THEN RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('teams','complete slot assignment required')); END IF;
 FOR item IN SELECT * FROM jsonb_array_elements(p_teams) LOOP
  IF jsonb_typeof(item) IS DISTINCT FROM 'object' OR jsonb_typeof(item->'slotId') IS DISTINCT FROM 'string' OR (item->>'slotId') !~ '^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$'
   OR NOT(item ? 'teamRef') OR jsonb_typeof(item->'teamRef') NOT IN ('string','null')
   OR (jsonb_typeof(item->'teamRef')='string' AND length(btrim(item->>'teamRef')) NOT BETWEEN 1 AND 128)
   OR EXISTS(SELECT 1 FROM jsonb_object_keys(item) k WHERE k NOT IN ('slotId','teamRef')) THEN RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid team assignment'; END IF;
  IF NOT EXISTS(SELECT 1 FROM app.room_slot WHERE id=(item->>'slotId')::uuid AND room_id=p_room) THEN RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('teams','foreign slot')); END IF;
 END LOOP;
 IF (SELECT count(DISTINCT v->>'slotId') FROM jsonb_array_elements(p_teams) v)<>jsonb_array_length(p_teams) THEN RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('teams','duplicate slot')); END IF;
 changed:=(lobby.profile_id,lobby.structure) IS DISTINCT FROM (p_profile,p_structure)
  OR EXISTS(SELECT 1 FROM jsonb_array_elements(p_teams) v JOIN app.room_slot s ON s.id=(v->>'slotId')::uuid WHERE s.team_ref IS DISTINCT FROM v->>'teamRef');
 IF changed THEN
  FOR item IN SELECT * FROM jsonb_array_elements(p_teams) LOOP UPDATE app.room_slot SET team_ref=item->>'teamRef' WHERE id=(item->>'slotId')::uuid; END LOOP;
  UPDATE app.room_slot SET ready=false WHERE room_id=p_room AND controller_kind='human';
  UPDATE app.room SET profile_id=p_profile,structure=p_structure,state='lobby',countdown_at=NULL,revision=revision+1 WHERE id=p_room RETURNING * INTO lobby;
 END IF;
 SELECT * INTO STRICT lobby FROM app.room WHERE id=p_room;
 RETURN app.finish_account_command(receipt.id,true,jsonb_build_object('resourceId',p_room,'revision',lobby.revision::text));
END $$;
REVOKE ALL ON FUNCTION app.refresh_room_countdown(uuid,boolean),app.set_room_ready(uuid,uuid,numeric,boolean,uuid,text,uuid),app.start_room_countdown(uuid,uuid,numeric,text,uuid),app.configure_room(uuid,uuid,numeric,uuid,text,jsonb,uuid)
 FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;
COMMIT;
