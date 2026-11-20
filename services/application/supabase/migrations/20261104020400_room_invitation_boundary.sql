BEGIN;
CREATE FUNCTION app.issue_room_invitation_command(p_subject uuid,p_session uuid,p_request uuid,p_room uuid,p_expected numeric,p_token_hash text) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE player app.account;
BEGIN
 player:=app.lock_session_account(p_subject,p_session);
 IF p_request IS NULL OR p_request='00000000-0000-0000-0000-000000000000'::uuid THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('request','invalid'));END IF;
 RETURN app.issue_room_invitation(player.id,p_room,p_expected,p_token_hash,p_request);
EXCEPTION WHEN invalid_authorization_specification THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('session','unauthorized'));
WHEN check_violation THEN IF SQLERRM='request identity was used for different command terms' THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('request','conflict'));END IF;RAISE;
END $$;
CREATE FUNCTION app.revoke_room_invitation_command(p_subject uuid,p_session uuid,p_request uuid,p_invitation uuid) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE player app.account;
BEGIN
 player:=app.lock_session_account(p_subject,p_session);
 IF p_request IS NULL OR p_request='00000000-0000-0000-0000-000000000000'::uuid THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('request','invalid'));END IF;
 RETURN app.revoke_room_invitation(player.id,p_invitation,p_request);
EXCEPTION WHEN invalid_authorization_specification THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('session','unauthorized'));
WHEN check_violation THEN IF SQLERRM='request identity was used for different command terms' THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('request','conflict'));END IF;RAISE;
END $$;
CREATE FUNCTION app.join_room_command(p_subject uuid,p_session uuid,p_request uuid,p_room uuid,p_token_hash text) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE player app.account;actor_id uuid;host_id uuid;
BEGIN
 -- Preserve canonical145 sorted pair order before lock_session_account takes the actor lock.
 PERFORM id FROM auth.users WHERE id=p_subject FOR KEY SHARE;
 SELECT id INTO actor_id FROM app.account WHERE auth_subject=p_subject;
 IF actor_id IS NULL THEN RAISE EXCEPTION USING ERRCODE='28000',MESSAGE='verified live account required';END IF;
 SELECT r.host_id INTO host_id FROM app.room r WHERE r.id=p_room;
 PERFORM id FROM app.account WHERE id IN(actor_id,host_id) ORDER BY id FOR NO KEY UPDATE;
 player:=app.lock_session_account(p_subject,p_session);
 IF player.id IS DISTINCT FROM actor_id THEN RAISE EXCEPTION USING ERRCODE='28000',MESSAGE='verified account changed';END IF;
 IF p_request IS NULL OR p_request='00000000-0000-0000-0000-000000000000'::uuid THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('request','invalid'));END IF;
 RETURN app.join_room(player.id,p_room,p_token_hash,p_request);
EXCEPTION WHEN invalid_authorization_specification THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('session','unauthorized'));
WHEN check_violation THEN IF SQLERRM='request identity was used for different command terms' THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('request','conflict'));END IF;RAISE;
END $$;
CREATE FUNCTION app.read_room_invitation_command(p_subject uuid,p_session uuid,p_kind text,p_request uuid,p_target uuid,p_expected numeric,p_token_hash text) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE player app.account;scope_name text;terms jsonb;receipt app.command_receipt;
BEGIN
 player:=app.lock_session_account(p_subject,p_session);
 IF p_request IS NULL OR p_target IS NULL OR '00000000-0000-0000-0000-000000000000'::uuid IN(p_request,p_target) THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('request','invalid'));END IF;
 IF p_kind='issue' AND p_expected IS NOT NULL AND p_expected::text ~ '^(0|[1-9][0-9]{0,77})$' AND p_token_hash ~ '^[0-9a-f]{64}$' THEN
  scope_name:='room_invite';terms:=jsonb_build_object('basis',p_expected,'tokenHash',p_token_hash);
 ELSIF p_kind='join' AND p_expected IS NULL AND(p_token_hash IS NULL OR p_token_hash ~ '^[0-9a-f]{64}$') THEN
  scope_name:='room_join';terms:=jsonb_build_object('tokenHash',p_token_hash);
 ELSIF p_kind='revoke' AND p_expected IS NULL AND p_token_hash IS NULL THEN
  scope_name:='room_invite_revoke';terms:='{}'::jsonb;
 ELSE RETURN jsonb_build_object('status','fail','data',jsonb_build_object('request','invalid'));
 END IF;
 SELECT * INTO receipt FROM app.command_receipt WHERE principal_key='human:'||player.id::text AND scope=scope_name AND request_ref=p_request::text;
 IF FOUND AND(receipt.target_ref IS DISTINCT FROM p_target::text OR receipt.payload_hash IS DISTINCT FROM app.auth_json_hash(terms)) THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('request','conflict'));END IF;
 RETURN jsonb_build_object('status','success','data',app.read_account_command(player.id,scope_name,p_request));
EXCEPTION WHEN invalid_authorization_specification THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('session','unauthorized'));
END $$;
CREATE FUNCTION app.read_my_room_invitations(p_room uuid,p_after uuid,p_limit integer) RETURNS jsonb
LANGUAGE plpgsql STABLE SECURITY INVOKER SET search_path='' AS $$
DECLARE actor uuid:=app.current_account_id();authority jsonb;items jsonb;cursor_id uuid;
BEGIN
 IF actor IS NULL THEN authority:=app.read_current_session();IF authority->>'status'='fail' THEN RETURN authority;END IF;RETURN jsonb_build_object('status','fail','data',jsonb_build_object('account','unavailable'));END IF;
 IF '00000000-0000-0000-0000-000000000000'::uuid IN(p_room,p_after) OR p_limit IS NULL OR p_limit NOT BETWEEN 1 AND 100 THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('query','invalid'));END IF;
 SELECT coalesce(jsonb_agg(body ORDER BY id),'[]'::jsonb) INTO items FROM(
 SELECT id,jsonb_build_object('id',id,'roomId',room_id,'state',state) body FROM app.invitation WHERE issuer_id=actor AND(p_room IS NULL OR room_id=p_room) AND(p_after IS NULL OR id>p_after) ORDER BY id LIMIT p_limit) page;
 IF jsonb_array_length(items)=p_limit THEN cursor_id:=(items->-1->>'id')::uuid;END IF;
 RETURN jsonb_build_object('status','success','data',jsonb_build_object('accountId',actor,'items',items,'nextCursor',cursor_id));
END $$;
REVOKE ALL ON FUNCTION app.issue_room_invitation_command(uuid,uuid,uuid,uuid,numeric,text),app.revoke_room_invitation_command(uuid,uuid,uuid,uuid),app.join_room_command(uuid,uuid,uuid,uuid,text),app.read_room_invitation_command(uuid,uuid,text,uuid,uuid,numeric,text),app.read_my_room_invitations(uuid,uuid,integer) FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;
GRANT EXECUTE ON FUNCTION app.issue_room_invitation_command(uuid,uuid,uuid,uuid,numeric,text),app.revoke_room_invitation_command(uuid,uuid,uuid,uuid),app.join_room_command(uuid,uuid,uuid,uuid,text),app.read_room_invitation_command(uuid,uuid,text,uuid,uuid,numeric,text) TO service_role;
GRANT EXECUTE ON FUNCTION app.read_my_room_invitations(uuid,uuid,integer) TO authenticated;
COMMIT;
