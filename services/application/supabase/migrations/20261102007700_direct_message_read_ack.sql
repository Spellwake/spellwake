-- Acknowledge exact viewed message IDs; later arrivals stay unread regardless of clock order.
BEGIN;
CREATE FUNCTION app.valid_direct_read_ids(p_ids uuid[]) RETURNS boolean
LANGUAGE plpgsql IMMUTABLE SET search_path='' AS $$
BEGIN
 IF p_ids IS NULL OR array_ndims(p_ids) IS DISTINCT FROM 1 OR array_lower(p_ids,1) IS DISTINCT FROM 1
 OR cardinality(p_ids) NOT BETWEEN 1 AND 16 THEN RETURN false; END IF;
 RETURN array_position(p_ids,NULL) IS NULL AND NOT '00000000-0000-0000-0000-000000000000'::uuid=ANY(p_ids)
 AND (SELECT count(DISTINCT id) FROM unnest(p_ids) id)=cardinality(p_ids);
END $$;
REVOKE ALL ON FUNCTION app.valid_direct_read_ids(uuid[]) FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;
CREATE FUNCTION app.mark_direct_messages_read(p_subject uuid,p_session uuid,p_request uuid,p_conversation uuid,p_messages uuid[])
RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE player app.account; receipt app.command_receipt; matched bigint;
BEGIN
 IF current_setting('transaction_isolation')<>'read committed' THEN
  RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='message read acknowledgment requires read committed'; END IF;
 player:=app.lock_session_account(p_subject,p_session);
 IF p_request IS NULL OR p_conversation IS NULL OR NOT app.valid_direct_read_ids(p_messages)
 OR '00000000-0000-0000-0000-000000000000'::uuid IN (p_request,p_conversation) THEN
  RETURN jsonb_build_object('status','fail','data',jsonb_build_object('request','invalid')); END IF;
 receipt:=app.begin_account_command(player.id,p_request,'chat_read',p_conversation::text,jsonb_build_object('messageIds',to_jsonb(p_messages)));
 IF receipt.state<>'pending' THEN RETURN receipt.response_payload; END IF;
 SELECT count(*) INTO matched FROM app.message m JOIN app.conversation c ON c.id=m.conversation_id
 WHERE m.id=ANY(p_messages) AND m.conversation_id=p_conversation AND c.kind='dm'
 AND player.id IN (c.dm_low_account_id,c.dm_high_account_id)
 AND EXISTS(SELECT 1 FROM app.conversation_member cm WHERE cm.conversation_id=c.id AND cm.account_id=player.id);
 IF matched<>cardinality(p_messages) THEN
  RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('conversation','conversation unavailable')); END IF;
 UPDATE app.notification n SET read_at=clock_timestamp()
 WHERE n.recipient_id=player.id AND n.source_kind='dm_message' AND n.read_at IS NULL
 AND n.source_ref IN (SELECT id::text FROM unnest(p_messages) id);
 RETURN app.finish_account_command(receipt.id,true,jsonb_build_object('resourceId',p_conversation));
EXCEPTION WHEN invalid_authorization_specification THEN
 RETURN jsonb_build_object('status','fail','data',jsonb_build_object('session','unauthorized'));
WHEN check_violation THEN
 IF SQLERRM='request identity was used for different command terms' THEN
  RETURN jsonb_build_object('status','fail','data',jsonb_build_object('request','conflict')); END IF;
 RAISE;
END $$;
CREATE FUNCTION app.read_direct_read_command(p_subject uuid,p_session uuid,p_input jsonb) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE player app.account; receipt app.command_receipt; request_id uuid; conversation_id uuid; messages uuid[];
BEGIN
 IF current_setting('transaction_isolation')<>'read committed' THEN
  RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='message read acknowledgment requires read committed'; END IF;
 player:=app.lock_session_account(p_subject,p_session);
 IF jsonb_typeof(p_input) IS DISTINCT FROM 'object' THEN
  RETURN jsonb_build_object('status','fail','data',jsonb_build_object('request','invalid')); END IF;
 IF octet_length(p_input::text)>2048 OR (SELECT array_agg(k ORDER BY k) FROM jsonb_object_keys(p_input) k)
 IS DISTINCT FROM ARRAY['conversationId','messageIds','requestId']::text[]
 OR jsonb_typeof(p_input->'conversationId') IS DISTINCT FROM 'string'
 OR jsonb_typeof(p_input->'requestId') IS DISTINCT FROM 'string'
 OR jsonb_typeof(p_input->'messageIds') IS DISTINCT FROM 'array' THEN
  RETURN jsonb_build_object('status','fail','data',jsonb_build_object('request','invalid')); END IF;
 IF jsonb_array_length(p_input->'messageIds') NOT BETWEEN 1 AND 16
 OR EXISTS(SELECT 1 FROM jsonb_array_elements(p_input->'messageIds') id WHERE jsonb_typeof(id)<>'string') THEN
  RETURN jsonb_build_object('status','fail','data',jsonb_build_object('request','invalid')); END IF;
 request_id:=(p_input->>'requestId')::uuid; conversation_id:=(p_input->>'conversationId')::uuid;
 SELECT array_agg(value::uuid ORDER BY ordinality) INTO messages FROM jsonb_array_elements_text(p_input->'messageIds') WITH ORDINALITY;
 IF '00000000-0000-0000-0000-000000000000'::uuid IN (request_id,conversation_id) OR NOT app.valid_direct_read_ids(messages) THEN
  RETURN jsonb_build_object('status','fail','data',jsonb_build_object('request','invalid')); END IF;
 SELECT * INTO receipt FROM app.command_receipt WHERE principal_key='human:'||player.id::text
 AND scope='chat_read' AND request_ref=request_id::text;
 IF FOUND AND (receipt.target_ref IS DISTINCT FROM conversation_id::text
 OR receipt.payload_hash IS DISTINCT FROM encode(sha256(convert_to(jsonb_build_object('messageIds',to_jsonb(messages))::text,'UTF8')),'hex')) THEN
  RETURN jsonb_build_object('status','fail','data',jsonb_build_object('request','conflict')); END IF;
 RETURN jsonb_build_object('status','success','data',app.read_account_command(player.id,'chat_read',request_id));
EXCEPTION WHEN invalid_authorization_specification THEN
 RETURN jsonb_build_object('status','fail','data',jsonb_build_object('session','unauthorized'));
WHEN invalid_text_representation THEN
 RETURN jsonb_build_object('status','fail','data',jsonb_build_object('request','invalid'));
END $$;
REVOKE ALL ON FUNCTION app.mark_direct_messages_read(uuid,uuid,uuid,uuid,uuid[]),app.read_direct_read_command(uuid,uuid,jsonb)
 FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;
GRANT EXECUTE ON FUNCTION app.mark_direct_messages_read(uuid,uuid,uuid,uuid,uuid[]),app.read_direct_read_command(uuid,uuid,jsonb) TO service_role;
COMMIT;
