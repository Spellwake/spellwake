BEGIN;

-- A vacant human slot has no account; one account cannot occupy two human slots in a room.
CREATE UNIQUE INDEX room_human_member ON app.room_slot(room_id,account_id)
 WHERE controller_kind='human' AND account_id IS NOT NULL;
ALTER TABLE app.room ADD CONSTRAINT room_mode_competition
 CHECK (mode IN ('open','collection') AND (NOT competitive OR mode='collection'));

-- Private canonical writers only. Session facades and their permissions follow separately.
-- A draft lobby is not deck legality, executable-release or match-admission authority.
CREATE FUNCTION app.create_room(p_actor uuid,p_format uuid,p_profile uuid,p_mode text,
 p_competitive boolean,p_visibility text,p_request uuid) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE receipt app.command_receipt; room_id uuid;
BEGIN
 IF p_request IS NULL OR p_request='00000000-0000-0000-0000-000000000000'::uuid THEN
  RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid room request'; END IF;
 receipt:=app.begin_account_command(p_actor,p_request,'room_create',p_actor::text,
  jsonb_build_object('format',p_format,'profile',p_profile,'mode',p_mode,'competitive',p_competitive,'visibility',p_visibility));
 IF receipt.state<>'pending' THEN RETURN receipt.response_payload; END IF;
 PERFORM id FROM app.account WHERE id=p_actor AND state='active' FOR NO KEY UPDATE;
 IF NOT FOUND THEN RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('account','unavailable')); END IF;
 IF p_mode IS NULL OR p_mode NOT IN ('open','collection') OR p_competitive IS NULL
 OR (p_competitive AND p_mode<>'collection') OR p_visibility IS NULL OR p_visibility NOT IN ('public','private') THEN
  RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('configuration','invalid')); END IF;
 PERFORM id FROM app.format WHERE id=p_format FOR KEY SHARE;
 IF NOT FOUND THEN RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('format','unavailable')); END IF;
 PERFORM id FROM app.table_profile_revision WHERE id=p_profile FOR KEY SHARE;
 IF NOT FOUND THEN RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('profile','unavailable')); END IF;
 INSERT INTO app.room(host_id,format_id,profile_id,mode,competitive,visibility,state,revision)
 VALUES(p_actor,p_format,p_profile,p_mode,p_competitive,p_visibility,'lobby',0) RETURNING id INTO room_id;
 INSERT INTO app.room_slot(room_id,account_id,seat_index,controller_kind,ready)
 VALUES(room_id,p_actor,0,'human',false);
 RETURN app.finish_account_command(receipt.id,true,jsonb_build_object('resourceId',room_id));
END $$;

CREATE FUNCTION app.select_room_deck(p_actor uuid,p_room uuid,p_expected numeric,p_revision uuid,p_request uuid)
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
  SELECT 1 FROM app.room_slot WHERE room_id=p_room AND account_id=p_actor AND controller_kind='human') THEN
  RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('room','unavailable')); END IF;
 IF p_expected IS NULL OR NOT (p_expected>=0 AND p_expected<power(10::numeric,78) AND scale(p_expected)=0)
 OR p_expected<>lobby.revision THEN
  RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('revision','conflict')); END IF;
 SELECT * INTO STRICT slot FROM app.room_slot
 WHERE room_id=p_room AND account_id=p_actor AND controller_kind='human' FOR NO KEY UPDATE;
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

CREATE FUNCTION app.leave_room(p_actor uuid,p_room uuid,p_expected numeric,p_request uuid)
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
  SELECT 1 FROM app.room_slot WHERE room_id=p_room AND account_id=p_actor AND controller_kind='human') THEN
  RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('room','unavailable')); END IF;
 IF p_expected IS NULL OR NOT (p_expected>=0 AND p_expected<power(10::numeric,78) AND scale(p_expected)=0)
 OR p_expected<>lobby.revision THEN
  RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('revision','conflict')); END IF;
 SELECT * INTO STRICT slot FROM app.room_slot
 WHERE room_id=p_room AND account_id=p_actor AND controller_kind='human' FOR NO KEY UPDATE;
 IF lobby.host_id=p_actor THEN
  UPDATE app.room_slot SET ready=false WHERE room_id=p_room;
  UPDATE app.room SET state='closed',countdown_at=NULL,revision=revision+1 WHERE id=p_room;
 ELSE
  UPDATE app.room_slot SET account_id=NULL,sponsor_id=NULL,deck_revision_id=NULL,ready=false WHERE id=slot.id;
  UPDATE app.room SET state='lobby',countdown_at=NULL,revision=revision+1 WHERE id=p_room;
 END IF;
 RETURN app.finish_account_command(receipt.id,true,jsonb_build_object('resourceId',p_room));
END $$;

REVOKE ALL ON FUNCTION app.create_room(uuid,uuid,uuid,text,boolean,text,uuid),
 app.select_room_deck(uuid,uuid,numeric,uuid,uuid),app.leave_room(uuid,uuid,numeric,uuid)
 FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;

COMMIT;
