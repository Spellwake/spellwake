BEGIN;
-- One shared nonlocking policy supplies a read hint and the locked canonical send context.
-- No caller receives the private admission/source fields in this context.
CREATE FUNCTION app.game_message_context(p_actor uuid,p_conversation uuid) RETURNS jsonb
LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path='' AS $$
DECLARE thread app.conversation;m app.match;actors uuid[];source uuid;
BEGIN
 SELECT * INTO thread FROM app.conversation WHERE id=p_conversation AND kind='game';
 IF thread.id IS NULL THEN RETURN NULL;END IF;
 SELECT * INTO m FROM app.match WHERE id=thread.match_id;
 SELECT array_agg(account_id ORDER BY account_id) INTO actors FROM app.match_seat WHERE match_id=thread.match_id AND controller_kind='human';
 IF actors IS NULL OR NOT p_actor=ANY(actors) THEN RETURN NULL;END IF;
 IF NOT EXISTS(SELECT 1 FROM app.account WHERE id=p_actor AND state='active') OR m.state NOT IN('ready','active','between_games') OR m.accepted_release_id IS NULL
 OR NOT EXISTS(SELECT 1 FROM app.conversation_member WHERE conversation_id=thread.id AND account_id=p_actor)
 OR EXISTS(SELECT 1 FROM app.match_seat s WHERE s.match_id=m.id AND s.controller_kind='human' AND NOT EXISTS(SELECT 1 FROM app.conversation_member cm WHERE cm.conversation_id=thread.id AND cm.account_id=s.account_id))
 OR EXISTS(SELECT 1 FROM app.conversation_member cm WHERE cm.conversation_id=thread.id AND NOT cm.account_id=ANY(actors))
 OR EXISTS(SELECT 1 FROM unnest(actors) recipient WHERE recipient<>p_actor AND app.contact_blocked(p_actor,recipient)) THEN RETURN NULL;END IF;
 SELECT id INTO source FROM app.command_receipt WHERE scope='room_start_v1' AND principal_key='room-start:'||m.room_id::text AND target_ref=m.room_id::text AND state='accepted' AND response_payload->'data'->>'resourceId'=m.id::text;
 IF source IS NULL THEN RETURN NULL;END IF;
 RETURN jsonb_build_object('matchId',m.id,'admissionReceiptId',source,'matchRevision',m.revision::text,'matchState',m.state,'humanAccountIds',to_jsonb(actors));
END $$;
CREATE OR REPLACE FUNCTION app.lock_game_message_admission(p_actor uuid,p_conversation uuid) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE thread app.conversation;actors uuid[];current_actors uuid[];
BEGIN
 IF current_setting('transaction_isolation')<>'read committed' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='game message requires read committed retry';END IF;
 SELECT * INTO thread FROM app.conversation WHERE id=p_conversation AND kind='game';
 IF thread.id IS NULL THEN RETURN NULL;END IF;
 SELECT array_agg(account_id ORDER BY account_id) INTO actors FROM app.match_seat WHERE match_id=thread.match_id AND controller_kind='human';
 IF actors IS NULL OR NOT p_actor=ANY(actors) THEN RETURN NULL;END IF;
 PERFORM id FROM app.account WHERE id=ANY(actors) ORDER BY id FOR NO KEY UPDATE;
 PERFORM id FROM app.match WHERE id=thread.match_id FOR NO KEY UPDATE;
 SELECT array_agg(account_id ORDER BY account_id) INTO current_actors FROM app.match_seat WHERE match_id=thread.match_id AND controller_kind='human';
 IF current_actors IS DISTINCT FROM actors THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='game message roster changed during account fence';END IF;
 RETURN app.game_message_context(p_actor,p_conversation);
END $$;
CREATE FUNCTION app.send_game_message_command(p_subject uuid,p_session uuid,p_request uuid,p_match uuid,p_body text) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE player app.account;actor_id uuid;conversation_id uuid;actors uuid[];current_actors uuid[];observed_conversation uuid;
BEGIN
 IF current_setting('transaction_isolation')<>'read committed' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='game message requires read committed retry';END IF;
 PERFORM id FROM auth.users WHERE id=p_subject FOR KEY SHARE;
 SELECT id INTO actor_id FROM app.account WHERE auth_subject=p_subject;
 IF actor_id IS NULL THEN RAISE EXCEPTION USING ERRCODE='28000',MESSAGE='verified live account required';END IF;
 SELECT c.id INTO conversation_id FROM app.conversation c JOIN app.match m ON m.id=c.match_id
 WHERE c.kind='game' AND c.match_id=p_match AND m.started_at IS NOT NULL AND m.accepted_release_id IS NOT NULL
 AND EXISTS(SELECT 1 FROM app.match_seat s WHERE s.match_id=m.id AND s.controller_kind='human' AND s.account_id=actor_id)
 AND EXISTS(SELECT 1 FROM app.conversation_member cm WHERE cm.conversation_id=c.id AND cm.account_id=actor_id);
 IF conversation_id IS NULL THEN
  player:=app.lock_session_account(p_subject,p_session);
  RETURN jsonb_build_object('status','fail','data',jsonb_build_object('conversation','conversation unavailable'));
 END IF;
 SELECT array_agg(account_id ORDER BY account_id) INTO actors FROM app.match_seat WHERE match_id=p_match AND controller_kind='human';
 PERFORM id FROM app.account WHERE id=ANY(actors) ORDER BY id FOR NO KEY UPDATE;
 player:=app.lock_session_account(p_subject,p_session);
 IF player.id IS DISTINCT FROM actor_id THEN RAISE EXCEPTION USING ERRCODE='28000',MESSAGE='verified account changed';END IF;
 SELECT c.id INTO observed_conversation FROM app.conversation c WHERE c.kind='game' AND c.match_id=p_match;
 SELECT array_agg(account_id ORDER BY account_id) INTO current_actors FROM app.match_seat WHERE match_id=p_match AND controller_kind='human';
 IF (actors,conversation_id) IS DISTINCT FROM(current_actors,observed_conversation) THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='game chat scope changed before account fence';END IF;
 IF p_request IS NULL OR p_request='00000000-0000-0000-0000-000000000000'::uuid THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('request','invalid'));END IF;
 RETURN app.send_game_message(player.id,conversation_id,p_body,p_request);
EXCEPTION WHEN invalid_authorization_specification THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('session','unauthorized'));
WHEN check_violation THEN IF SQLERRM='request identity was used for different command terms' THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('request','conflict'));END IF;RAISE;
END $$;
CREATE FUNCTION app.read_game_message_command(p_subject uuid,p_session uuid,p_request uuid,p_match uuid,p_body text) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE player app.account;conversation_id uuid;receipt app.command_receipt;
BEGIN
 IF current_setting('transaction_isolation')<>'read committed' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='chat notification command requires read committed retry';END IF;
 player:=app.lock_session_account(p_subject,p_session);
 IF p_request IS NULL OR p_request='00000000-0000-0000-0000-000000000000'::uuid OR p_body IS NULL OR length(p_body) NOT BETWEEN 1 AND 4000 OR p_body !~ '[^[:space:]]' THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('request','invalid'));END IF;
 SELECT c.id INTO conversation_id FROM app.conversation c JOIN app.match m ON m.id=c.match_id
 WHERE c.kind='game' AND c.match_id=p_match AND m.started_at IS NOT NULL AND m.accepted_release_id IS NOT NULL
 AND EXISTS(SELECT 1 FROM app.match_seat s WHERE s.match_id=m.id AND s.controller_kind='human' AND s.account_id=player.id)
 AND EXISTS(SELECT 1 FROM app.conversation_member cm WHERE cm.conversation_id=c.id AND cm.account_id=player.id);
 IF conversation_id IS NULL THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('conversation','conversation unavailable'));END IF;
 SELECT * INTO receipt FROM app.command_receipt WHERE principal_key='human:'||player.id::text AND scope='chat' AND request_ref=p_request::text;
 IF FOUND AND(receipt.target_ref IS DISTINCT FROM conversation_id::text OR receipt.payload_hash IS DISTINCT FROM app.auth_json_hash(jsonb_build_object('action','send_game','body',p_body))) THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('request','conflict'));END IF;
 RETURN jsonb_build_object('status','success','data',app.read_account_command(player.id,'chat',p_request));
EXCEPTION WHEN invalid_authorization_specification THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('session','unauthorized'));
END $$;
CREATE FUNCTION app.read_my_game_chat(p_match uuid,p_before_at timestamptz,p_before_id uuid,p_limit integer) RETURNS jsonb
LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path='' AS $$
DECLARE actor uuid:=app.current_account_id();authority jsonb;conversation_id uuid;participants jsonb;items jsonb;cursor_value jsonb;
BEGIN
 IF actor IS NULL THEN authority:=app.read_current_session();IF authority->>'status'='fail' THEN RETURN authority;END IF;RETURN jsonb_build_object('status','fail','data',jsonb_build_object('account','unavailable'));END IF;
 IF p_match IS NULL OR p_match='00000000-0000-0000-0000-000000000000'::uuid OR p_limit IS NULL OR p_limit NOT BETWEEN 1 AND 16 OR ((p_before_at IS NULL)<>(p_before_id IS NULL)) OR p_before_id='00000000-0000-0000-0000-000000000000'::uuid OR(p_before_at IS NOT NULL AND NOT isfinite(p_before_at)) THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('page','invalid'));END IF;
 IF NOT app.can_read_match(p_match) THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('conversation','unavailable'));END IF;
 SELECT id INTO conversation_id FROM app.conversation c WHERE c.kind='game' AND c.match_id=p_match AND EXISTS(SELECT 1 FROM app.conversation_member cm WHERE cm.conversation_id=c.id AND cm.account_id=actor);
 IF conversation_id IS NULL THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('conversation','unavailable'));END IF;
 SELECT jsonb_agg(jsonb_build_object('id',a.id,'username',CASE WHEN a.state='deleted' THEN 'Deleted player' ELSE a.username END,'isDeleted',a.state='deleted') ORDER BY a.id) INTO participants FROM app.match_seat s JOIN app.account a ON a.id=s.account_id WHERE s.match_id=p_match AND s.controller_kind='human';
 SELECT coalesce(jsonb_agg(jsonb_build_object('id',m.message_id,'senderId',m.sender_id,'author',m.author,'body',m.body,'sentAt',m.sent_at) ORDER BY m.sent_at DESC,m.message_id DESC),'[]'::jsonb) INTO items FROM app.read_conversation_messages(actor,conversation_id,p_before_at,p_before_id,p_limit) m;
 IF jsonb_array_length(items)=p_limit THEN cursor_value:=jsonb_build_object('sentAt',items->-1->'sentAt','id',items->-1->'id');END IF;
 RETURN jsonb_build_object('status','success','data',jsonb_build_object('accountId',actor,'matchId',p_match,'conversationId',conversation_id,'participants',participants,'canSend',app.game_message_context(actor,conversation_id) IS NOT NULL,'items',items,'nextCursor',cursor_value));
END $$;
-- Share the actual exact-ID rule with the existing DM-specific read acknowledgment.
CREATE FUNCTION app.valid_read_ids(p_ids uuid[],p_limit integer) RETURNS boolean LANGUAGE plpgsql IMMUTABLE SET search_path='' AS $$
BEGIN
 IF p_limit IS NULL OR p_limit NOT BETWEEN 1 AND 100 OR p_ids IS NULL OR array_ndims(p_ids) IS DISTINCT FROM 1 OR array_lower(p_ids,1) IS DISTINCT FROM 1 OR cardinality(p_ids) NOT BETWEEN 1 AND p_limit THEN RETURN false;END IF;
 RETURN array_position(p_ids,NULL) IS NULL AND NOT '00000000-0000-0000-0000-000000000000'::uuid=ANY(p_ids) AND(SELECT count(DISTINCT id) FROM unnest(p_ids) id)=cardinality(p_ids);
END $$;
CREATE OR REPLACE FUNCTION app.valid_direct_read_ids(p_ids uuid[]) RETURNS boolean LANGUAGE sql IMMUTABLE SET search_path='' AS $$ SELECT app.valid_read_ids(p_ids,16) $$;
CREATE FUNCTION app.read_my_notifications(p_after uuid,p_limit integer) RETURNS jsonb LANGUAGE plpgsql STABLE SECURITY INVOKER SET search_path='' AS $$
DECLARE actor uuid:=app.current_account_id();authority jsonb;items jsonb;cursor_id uuid;unread numeric;
BEGIN
 IF actor IS NULL THEN authority:=app.read_current_session();IF authority->>'status'='fail' THEN RETURN authority;END IF;RETURN jsonb_build_object('status','fail','data',jsonb_build_object('account','unavailable'));END IF;
 IF p_after='00000000-0000-0000-0000-000000000000'::uuid OR p_limit IS NULL OR p_limit NOT BETWEEN 1 AND 100 THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('query','invalid'));END IF;
 SELECT count(*) INTO unread FROM app.notification WHERE recipient_id=actor AND read_at IS NULL;
 SELECT coalesce(jsonb_agg(body ORDER BY id),'[]'::jsonb) INTO items FROM(
 SELECT id,jsonb_build_object('id',id,'sourceKind',source_kind,'sourceRef',source_ref,'body',body,'readAt',read_at) body FROM app.notification WHERE recipient_id=actor AND(p_after IS NULL OR id>p_after) ORDER BY id LIMIT p_limit) page;
 IF jsonb_array_length(items)=p_limit THEN cursor_id:=(items->-1->>'id')::uuid;END IF;
 RETURN jsonb_build_object('status','success','data',jsonb_build_object('accountId',actor,'unreadCount',unread::text,'items',items,'nextCursor',cursor_id));
END $$;
CREATE FUNCTION app.mark_notifications_read(p_subject uuid,p_session uuid,p_request uuid,p_notifications uuid[]) RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE player app.account;receipt app.command_receipt;matched bigint;
BEGIN
 IF current_setting('transaction_isolation')<>'read committed' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='chat notification command requires read committed retry';END IF;
 player:=app.lock_session_account(p_subject,p_session);
 IF p_request IS NULL OR p_request='00000000-0000-0000-0000-000000000000'::uuid OR NOT app.valid_read_ids(p_notifications,100) THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('request','invalid'));END IF;
 receipt:=app.begin_account_command(player.id,p_request,'notifications_read',player.id::text,jsonb_build_object('notificationIds',to_jsonb(p_notifications)));
 IF receipt.state<>'pending' THEN RETURN receipt.response_payload;END IF;
 PERFORM id FROM app.notification WHERE recipient_id=player.id AND id=ANY(p_notifications) ORDER BY id FOR UPDATE;
 GET DIAGNOSTICS matched=ROW_COUNT;
 IF matched<>cardinality(p_notifications) THEN RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('notifications','unavailable'));END IF;
 UPDATE app.notification SET read_at=clock_timestamp() WHERE recipient_id=player.id AND id=ANY(p_notifications) AND read_at IS NULL;
 RETURN app.finish_account_command(receipt.id,true,jsonb_build_object('resourceId',player.id));
EXCEPTION WHEN invalid_authorization_specification THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('session','unauthorized'));
WHEN check_violation THEN IF SQLERRM='request identity was used for different command terms' THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('request','conflict'));END IF;RAISE;
END $$;
CREATE FUNCTION app.read_notifications_command(p_subject uuid,p_session uuid,p_request uuid,p_notifications uuid[]) RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE player app.account;receipt app.command_receipt;
BEGIN
 IF current_setting('transaction_isolation')<>'read committed' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='chat notification command requires read committed retry';END IF;
 player:=app.lock_session_account(p_subject,p_session);
 IF p_request IS NULL OR p_request='00000000-0000-0000-0000-000000000000'::uuid OR NOT app.valid_read_ids(p_notifications,100) THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('request','invalid'));END IF;
 SELECT * INTO receipt FROM app.command_receipt WHERE principal_key='human:'||player.id::text AND scope='notifications_read' AND request_ref=p_request::text;
 IF FOUND AND(receipt.target_ref IS DISTINCT FROM player.id::text OR receipt.payload_hash IS DISTINCT FROM app.auth_json_hash(jsonb_build_object('notificationIds',to_jsonb(p_notifications)))) THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('request','conflict'));END IF;
 RETURN jsonb_build_object('status','success','data',app.read_account_command(player.id,'notifications_read',p_request));
EXCEPTION WHEN invalid_authorization_specification THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('session','unauthorized'));
END $$;
REVOKE ALL ON FUNCTION app.game_message_context(uuid,uuid),app.lock_game_message_admission(uuid,uuid),app.send_game_message_command(uuid,uuid,uuid,uuid,text),app.read_game_message_command(uuid,uuid,uuid,uuid,text),app.read_my_game_chat(uuid,timestamp with time zone,uuid,integer),app.valid_read_ids(uuid[],integer),app.valid_direct_read_ids(uuid[]),app.read_my_notifications(uuid,integer),app.mark_notifications_read(uuid,uuid,uuid,uuid[]),app.read_notifications_command(uuid,uuid,uuid,uuid[]) FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;
GRANT EXECUTE ON FUNCTION app.send_game_message_command(uuid,uuid,uuid,uuid,text),app.read_game_message_command(uuid,uuid,uuid,uuid,text),app.mark_notifications_read(uuid,uuid,uuid,uuid[]),app.read_notifications_command(uuid,uuid,uuid,uuid[]) TO service_role;
GRANT EXECUTE ON FUNCTION app.read_my_game_chat(uuid,timestamp with time zone,uuid,integer),app.read_my_notifications(uuid,integer) TO authenticated;
COMMIT;
