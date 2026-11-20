BEGIN;
-- A manual participant sponsors its own new AI seat. This creates no runner,
-- provider authority, selected deck, or synthetic readiness confirmation.
CREATE FUNCTION app.add_room_ai_slot(p_actor uuid,p_room uuid,p_expected numeric,p_request uuid) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE receipt app.command_receipt;lobby app.room;profile app.table_profile_revision;original_host uuid;actors uuid[];slot_id uuid;chosen bigint;
BEGIN
 IF current_setting('transaction_isolation')<>'read committed' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='AI slot admission requires read committed retry';END IF;
 IF p_actor IS NULL OR p_room IS NULL OR p_request IS NULL OR '00000000-0000-0000-0000-000000000000'::uuid IN(p_actor,p_room,p_request)
 OR p_expected IS NULL OR p_expected::text !~ '^(0|[1-9][0-9]{0,77})$' THEN RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid AI slot admission envelope';END IF;
 receipt:=app.begin_account_command(p_actor,p_request,'ai_slot_add_v1',p_room::text,jsonb_build_object('basis',p_expected::text));
 IF receipt.state<>'pending' THEN RETURN receipt.response_payload;END IF;
 SELECT host_id INTO original_host FROM app.room WHERE id=p_room;
 SELECT array_agg(DISTINCT a ORDER BY a) INTO actors FROM unnest(ARRAY[p_actor,original_host]) a WHERE a IS NOT NULL;
 PERFORM id FROM app.account WHERE id=ANY(actors) ORDER BY id FOR NO KEY UPDATE;
 PERFORM id FROM app.queue_entry WHERE claimed_room_id=p_room ORDER BY id FOR NO KEY UPDATE;
 SELECT * INTO lobby FROM app.room WHERE id=p_room FOR NO KEY UPDATE;
 IF lobby.host_id IS DISTINCT FROM original_host THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='AI slot host changed before account fence';END IF;
 IF EXISTS(SELECT 1 FROM app.account WHERE id=ANY(actors) AND state<>'active') THEN RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('account','unavailable'));END IF;
 IF lobby.id IS NULL OR lobby.state NOT IN('lobby','countdown') OR lobby.competitive OR NOT EXISTS(SELECT 1 FROM app.room_slot WHERE room_id=p_room AND retired_at IS NULL AND account_id=p_actor AND controller_kind='human') THEN RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('room','unavailable'));END IF;
 IF lobby.revision<>p_expected THEN RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('revision','conflict'));END IF;
 IF lobby.queue_claim_receipt_id IS NOT NULL THEN RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('queue','cancel the claimed room before changing its roster'));END IF;
 SELECT * INTO profile FROM app.table_profile_revision WHERE id=lobby.profile_id AND admission_qualification_ref IS NOT NULL AND seat_capacity IS NOT NULL;
 IF profile.id IS NULL THEN RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('room','unavailable'));END IF;
 SELECT id INTO slot_id FROM app.room_slot WHERE room_id=p_room AND retired_at IS NULL AND controller_kind='human' AND account_id IS NULL ORDER BY seat_index LIMIT 1 FOR NO KEY UPDATE;
 IF slot_id IS NULL THEN
  SELECT candidate INTO chosen FROM generate_series(0,profile.seat_capacity-1) candidate WHERE NOT EXISTS(SELECT 1 FROM app.room_slot WHERE room_id=p_room AND retired_at IS NULL AND seat_index=candidate) ORDER BY candidate LIMIT 1;
  IF chosen IS NULL THEN RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('capacity','full'));END IF;
  INSERT INTO app.room_slot(room_id,sponsor_id,seat_index,controller_kind,ready) VALUES(p_room,p_actor,chosen,'ai',false) RETURNING id INTO slot_id;
 ELSE
  UPDATE app.room_slot SET controller_kind='ai',sponsor_id=p_actor,deck_revision_id=NULL,ready=false WHERE id=slot_id;
 END IF;
 -- Existing configuration triggers rotate identity and invalidate any AI Ready.
 UPDATE app.room_slot SET ready=false WHERE room_id=p_room AND retired_at IS NULL AND controller_kind='human';
 UPDATE app.room SET state='lobby',countdown_at=NULL,revision=revision+1 WHERE id=p_room RETURNING * INTO lobby;
 RETURN app.finish_account_command(receipt.id,true,jsonb_build_object('resourceId',slot_id,'revision',lobby.revision::text));
END $$;
CREATE FUNCTION app.add_room_ai_slot_command(p_subject uuid,p_session uuid,p_request uuid,p_room uuid,p_expected numeric) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE player app.account;actor_id uuid;original_host uuid;current_host uuid;
BEGIN
 IF current_setting('transaction_isolation')<>'read committed' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='AI slot admission requires read committed retry';END IF;
 PERFORM id FROM auth.users WHERE id=p_subject FOR KEY SHARE;
 SELECT id INTO actor_id FROM app.account WHERE auth_subject=p_subject;
 IF actor_id IS NULL THEN RAISE EXCEPTION USING ERRCODE='28000',MESSAGE='verified live account required';END IF;
 SELECT host_id INTO original_host FROM app.room WHERE id=p_room;
 PERFORM id FROM app.account WHERE id IN(actor_id,original_host) ORDER BY id FOR NO KEY UPDATE;
 player:=app.lock_session_account(p_subject,p_session);
 SELECT host_id INTO current_host FROM app.room WHERE id=p_room;
 IF player.id IS DISTINCT FROM actor_id THEN RAISE EXCEPTION USING ERRCODE='28000',MESSAGE='verified account changed';END IF;
 IF current_host IS DISTINCT FROM original_host THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='AI slot host changed before session fence';END IF;
 IF p_room IS NULL OR p_request IS NULL OR '00000000-0000-0000-0000-000000000000'::uuid IN(p_room,p_request) OR p_expected IS NULL OR p_expected::text !~ '^(0|[1-9][0-9]{0,77})$' THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('request','invalid'));END IF;
 RETURN app.add_room_ai_slot(player.id,p_room,p_expected,p_request);
EXCEPTION WHEN invalid_authorization_specification THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('session','unauthorized'));
WHEN check_violation THEN IF SQLERRM='request identity was used for different command terms' THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('request','conflict'));END IF;RAISE;
END $$;
CREATE FUNCTION app.read_add_room_ai_slot_command(p_subject uuid,p_session uuid,p_request uuid,p_room uuid,p_expected numeric) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE player app.account;receipt app.command_receipt;
BEGIN
 IF current_setting('transaction_isolation')<>'read committed' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='AI slot admission requires read committed retry';END IF;
 player:=app.lock_session_account(p_subject,p_session);
 IF p_room IS NULL OR p_request IS NULL OR '00000000-0000-0000-0000-000000000000'::uuid IN(p_room,p_request) OR p_expected IS NULL OR p_expected::text !~ '^(0|[1-9][0-9]{0,77})$' THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('request','invalid'));END IF;
 SELECT * INTO receipt FROM app.command_receipt WHERE principal_key='human:'||player.id::text AND scope='ai_slot_add_v1' AND request_ref=p_request::text;
 IF FOUND AND(receipt.target_ref IS DISTINCT FROM p_room::text OR receipt.payload_hash IS DISTINCT FROM app.auth_json_hash(jsonb_build_object('basis',p_expected::text))) THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('request','conflict'));END IF;
 RETURN jsonb_build_object('status','success','data',app.read_account_command(player.id,'ai_slot_add_v1',p_request));
EXCEPTION WHEN invalid_authorization_specification THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('session','unauthorized'));
END $$;
REVOKE ALL ON FUNCTION app.add_room_ai_slot(uuid,uuid,numeric,uuid),app.add_room_ai_slot_command(uuid,uuid,uuid,uuid,numeric),app.read_add_room_ai_slot_command(uuid,uuid,uuid,uuid,numeric) FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;
GRANT EXECUTE ON FUNCTION app.add_room_ai_slot_command(uuid,uuid,uuid,uuid,numeric),app.read_add_room_ai_slot_command(uuid,uuid,uuid,uuid,numeric) TO service_role;
COMMIT;
