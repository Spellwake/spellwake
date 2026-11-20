BEGIN;
-- Bootstrap source receipts retain private start identity; human responses never do.
CREATE FUNCTION app.public_ai_bootstrap_response(p_response jsonb) RETURNS jsonb
LANGUAGE sql IMMUTABLE SET search_path='' AS $$
 SELECT CASE WHEN p_response->>'status'='success' THEN jsonb_build_object('status','success','data',jsonb_build_object('resourceId',p_response->'data'->'resourceId','epoch',p_response->'data'->'epoch')) ELSE p_response END
$$;
CREATE FUNCTION app.manage_ai_slot_command(p_subject uuid,p_session uuid,p_request uuid,p_kind text,p_slot uuid,p_expected numeric,p_revision uuid) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE player app.account;actor_id uuid;original_room uuid;original_host uuid;original_sponsor uuid;slot app.room_slot;lobby app.room;response jsonb;
BEGIN
 IF current_setting('transaction_isolation')<>'read committed' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='AI intent requires read committed retry';END IF;
 PERFORM id FROM auth.users WHERE id=p_subject FOR KEY SHARE;
 SELECT id INTO actor_id FROM app.account WHERE auth_subject=p_subject;
 IF actor_id IS NULL THEN RAISE EXCEPTION USING ERRCODE='28000',MESSAGE='verified live account required';END IF;
 SELECT s.room_id,r.host_id,s.sponsor_id INTO original_room,original_host,original_sponsor FROM app.room_slot s JOIN app.room r ON r.id=s.room_id WHERE s.id=p_slot;
 PERFORM id FROM app.account WHERE id IN(actor_id,original_host,original_sponsor) ORDER BY id FOR NO KEY UPDATE;
 player:=app.lock_session_account(p_subject,p_session);
 IF player.id IS DISTINCT FROM actor_id THEN RAISE EXCEPTION USING ERRCODE='28000',MESSAGE='verified account changed';END IF;
 PERFORM id FROM app.queue_entry WHERE claimed_room_id=original_room ORDER BY id FOR NO KEY UPDATE;
 SELECT * INTO slot FROM app.room_slot WHERE id=p_slot;
 SELECT * INTO lobby FROM app.room WHERE id=slot.room_id;
 IF (slot.room_id,slot.sponsor_id,lobby.host_id) IS DISTINCT FROM(original_room,original_sponsor,original_host) THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='AI intent account scope changed before fence';END IF;
 IF p_request IS NULL OR p_slot IS NULL OR '00000000-0000-0000-0000-000000000000'::uuid IN(p_request,p_slot,p_revision) OR p_expected IS NULL OR p_expected::text !~ '^(0|[1-9][0-9]{0,77})$' OR p_kind IS NULL OR p_kind NOT IN('bootstrap','remove') OR(p_kind='remove' AND p_revision IS NOT NULL) THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('request','invalid'));END IF;
 IF p_kind='bootstrap' THEN
  response:=app.bootstrap_ai_slot(player.id,p_slot,p_expected,p_revision,p_request);
  RETURN app.public_ai_bootstrap_response(response);
 END IF;
 RETURN app.remove_ai_slot(player.id,p_slot,p_expected,p_request);
EXCEPTION WHEN invalid_authorization_specification THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('session','unauthorized'));
WHEN check_violation THEN IF SQLERRM='request identity was used for different command terms' THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('request','conflict'));END IF;RAISE;
END $$;
CREATE FUNCTION app.read_ai_slot_command(p_subject uuid,p_session uuid,p_request uuid,p_kind text,p_slot uuid,p_expected numeric,p_revision uuid) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE player app.account;scope_name text;terms jsonb;receipt app.command_receipt;disposition jsonb;
BEGIN
 IF current_setting('transaction_isolation')<>'read committed' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='AI intent requires read committed retry';END IF;
 player:=app.lock_session_account(p_subject,p_session);
 IF p_request IS NULL OR p_slot IS NULL OR '00000000-0000-0000-0000-000000000000'::uuid IN(p_request,p_slot,p_revision) OR p_expected IS NULL OR p_expected::text !~ '^(0|[1-9][0-9]{0,77})$' OR p_kind IS NULL OR p_kind NOT IN('bootstrap','remove') OR(p_kind='remove' AND p_revision IS NOT NULL) THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('request','invalid'));END IF;
 IF p_kind='bootstrap' THEN scope_name:='ai_bootstrap_v1';terms:=jsonb_build_object('basis',p_expected::text,'revision',p_revision);
 ELSE scope_name:='ai_remove_v1';terms:=jsonb_build_object('basis',p_expected::text);END IF;
 SELECT * INTO receipt FROM app.command_receipt WHERE principal_key='human:'||player.id::text AND scope=scope_name AND request_ref=p_request::text;
 IF FOUND AND(receipt.target_ref IS DISTINCT FROM p_slot::text OR receipt.payload_hash IS DISTINCT FROM app.auth_json_hash(terms)) THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('request','conflict'));END IF;
 disposition:=app.read_account_command(player.id,scope_name,p_request);
 IF p_kind='bootstrap' AND disposition->>'state'='accepted' THEN disposition:=jsonb_set(disposition,'{response}',app.public_ai_bootstrap_response(disposition->'response'));END IF;
 RETURN jsonb_build_object('status','success','data',disposition);
EXCEPTION WHEN invalid_authorization_specification THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('session','unauthorized'));
END $$;
CREATE FUNCTION app.read_room_ai_status(p_room uuid,p_after uuid,p_limit integer) RETURNS jsonb
LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path='' AS $$
DECLARE actor uuid:=app.current_account_id();authority jsonb;host uuid;items jsonb;cursor_id uuid;
BEGIN
 IF actor IS NULL THEN authority:=app.read_current_session();IF authority->>'status'='fail' THEN RETURN authority;END IF;RETURN jsonb_build_object('status','fail','data',jsonb_build_object('account','unavailable'));END IF;
 IF p_room IS NULL OR '00000000-0000-0000-0000-000000000000'::uuid IN(p_room,p_after) OR p_limit IS NULL OR p_limit NOT BETWEEN 1 AND 100 THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('query','invalid'));END IF;
 SELECT host_id INTO host FROM app.room WHERE id=p_room;
 IF host IS NULL OR(actor<>host AND NOT EXISTS(SELECT 1 FROM app.room_slot WHERE room_id=p_room AND sponsor_id=actor AND controller_kind='ai')) THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('room','unavailable'));END IF;
 SELECT coalesce(jsonb_agg(body ORDER BY id),'[]'::jsonb) INTO items FROM(
 SELECT s.id,jsonb_build_object('slotId',s.id,'seatIndex',s.seat_index::text,'isSponsor',s.sponsor_id=actor,'isRetired',s.retired_at IS NOT NULL,'stopPending',EXISTS(SELECT 1 FROM app.ai_runner_binding historical CROSS JOIN LATERAL app.read_ai_stop_work(historical.id) pending WHERE historical.room_slot_id=s.id),'deckRevisionId',CASE WHEN s.sponsor_id=actor THEN s.deck_revision_id END,'isReady',s.retired_at IS NULL AND app.ai_slot_ready(s.id),'binding',CASE WHEN b.id IS NOT NULL THEN jsonb_build_object('id',b.id,'epoch',b.binding_epoch::text,'desiredState',b.desired_state,'observedState',b.observed_state) END) body FROM app.room_slot s LEFT JOIN LATERAL(SELECT id,binding_epoch,desired_state,observed_state FROM app.ai_runner_binding WHERE room_slot_id=s.id AND desired_state='running') b ON true
 WHERE s.room_id=p_room AND s.controller_kind='ai' AND(actor=host OR s.sponsor_id=actor) AND(p_after IS NULL OR s.id>p_after) ORDER BY s.id LIMIT p_limit) page;
 IF jsonb_array_length(items)=p_limit THEN cursor_id:=(items->-1->>'slotId')::uuid;END IF;
 RETURN jsonb_build_object('status','success','data',jsonb_build_object('accountId',actor,'roomId',p_room,'items',items,'nextCursor',cursor_id));
END $$;
CREATE FUNCTION app.unready_room_command(p_subject uuid,p_session uuid,p_request uuid,p_room uuid,p_expected numeric) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE player app.account;
BEGIN
 IF current_setting('transaction_isolation')<>'read committed' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='room unready requires read committed retry';END IF;
 player:=app.lock_session_account(p_subject,p_session);
 IF p_room IS NULL OR p_request IS NULL OR '00000000-0000-0000-0000-000000000000'::uuid IN(p_room,p_request) OR p_expected IS NULL OR p_expected::text !~ '^(0|[1-9][0-9]{0,77})$' THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('request','invalid'));END IF;
 RETURN app.set_room_ready(player.id,p_room,p_expected,false,NULL,NULL,p_request);
EXCEPTION WHEN invalid_authorization_specification THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('session','unauthorized'));
WHEN check_violation THEN IF SQLERRM='request identity was used for different command terms' THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('request','conflict'));END IF;RAISE;
END $$;
CREATE FUNCTION app.read_unready_room_command(p_subject uuid,p_session uuid,p_request uuid,p_room uuid,p_expected numeric) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE player app.account;receipt app.command_receipt;
BEGIN
 IF current_setting('transaction_isolation')<>'read committed' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='room unready requires read committed retry';END IF;
 player:=app.lock_session_account(p_subject,p_session);
 IF p_room IS NULL OR p_request IS NULL OR '00000000-0000-0000-0000-000000000000'::uuid IN(p_room,p_request) OR p_expected IS NULL OR p_expected::text !~ '^(0|[1-9][0-9]{0,77})$' THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('request','invalid'));END IF;
 SELECT * INTO receipt FROM app.command_receipt WHERE principal_key='human:'||player.id::text AND scope='room_ready' AND request_ref=p_request::text;
 IF FOUND AND(receipt.target_ref IS DISTINCT FROM p_room::text OR receipt.payload_hash IS DISTINCT FROM app.auth_json_hash(jsonb_build_object('basis',p_expected,'ready',false,'revision',NULL,'qualification',NULL))) THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('request','conflict'));END IF;
 RETURN jsonb_build_object('status','success','data',app.read_account_command(player.id,'room_ready',p_request));
EXCEPTION WHEN invalid_authorization_specification THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('session','unauthorized'));
END $$;
REVOKE ALL ON FUNCTION app.public_ai_bootstrap_response(jsonb),app.manage_ai_slot_command(uuid,uuid,uuid,text,uuid,numeric,uuid),app.read_ai_slot_command(uuid,uuid,uuid,text,uuid,numeric,uuid),app.read_room_ai_status(uuid,uuid,integer),app.unready_room_command(uuid,uuid,uuid,uuid,numeric),app.read_unready_room_command(uuid,uuid,uuid,uuid,numeric) FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;
GRANT EXECUTE ON FUNCTION app.manage_ai_slot_command(uuid,uuid,uuid,text,uuid,numeric,uuid),app.read_ai_slot_command(uuid,uuid,uuid,text,uuid,numeric,uuid),app.unready_room_command(uuid,uuid,uuid,uuid,numeric),app.read_unready_room_command(uuid,uuid,uuid,uuid,numeric) TO service_role;
GRANT EXECUTE ON FUNCTION app.read_room_ai_status(uuid,uuid,integer) TO authenticated;
COMMIT;
