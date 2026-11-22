BEGIN;

DO $$ BEGIN
  IF pg_catalog.encode(pg_catalog.sha256(pg_catalog.convert_to(
       (SELECT prosrc FROM pg_catalog.pg_proc WHERE oid='app.join_room(uuid,uuid,text,uuid)'::regprocedure),
       'UTF8')),'hex') IS DISTINCT FROM '36488908d7c0e4a4e984f152a3edc68fc4145416b480cbf894e7c0e9d6266ffc' THEN
    RAISE EXCEPTION 'private pilot join predecessor differs';
  END IF;
END $$;

CREATE OR REPLACE FUNCTION app.join_room(p_actor uuid, p_room uuid, p_token_hash text, p_request uuid)
 RETURNS jsonb
 LANGUAGE plpgsql
 SECURITY DEFINER
 SET search_path TO ''
AS $function$
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
 IF EXISTS (
   SELECT 1 FROM app.private_pilot_profile pilot WHERE pilot.profile_id=lobby.profile_id
   AND NOT app.lock_private_pilot_grant(pilot.release_id,p_actor,pilot.commander)
 ) THEN RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('room','unavailable')); END IF;
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
END $function$;

COMMIT;
