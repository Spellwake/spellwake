BEGIN;
-- Game text uses existing human membership; it never fabricates an AI account.
CREATE FUNCTION app.lock_game_message_admission(p_actor uuid,p_conversation uuid) RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE thread app.conversation; m app.match; actors uuid[]; current_actors uuid[]; source uuid;
BEGIN
 IF current_setting('transaction_isolation')<>'read committed' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='game message requires read committed retry';END IF;
 SELECT * INTO thread FROM app.conversation WHERE id=p_conversation AND kind='game';
 IF thread.id IS NULL THEN RETURN NULL;END IF;
 SELECT array_agg(account_id ORDER BY account_id) INTO actors FROM app.match_seat WHERE match_id=thread.match_id AND controller_kind='human';
 IF actors IS NULL OR NOT p_actor=ANY(actors) THEN RETURN NULL;END IF;
 -- Compatible with receipt FK KEY SHARE; social block writers take the same
 -- accounts before editing their pair. No account lock is acquired after match.
 PERFORM id FROM app.account WHERE id=ANY(actors) ORDER BY id FOR NO KEY UPDATE;
 SELECT * INTO m FROM app.match WHERE id=thread.match_id FOR NO KEY UPDATE;
 SELECT array_agg(account_id ORDER BY account_id) INTO current_actors FROM app.match_seat WHERE match_id=thread.match_id AND controller_kind='human';
 IF current_actors IS DISTINCT FROM actors THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='game message roster changed during account fence';END IF;
 IF NOT EXISTS(SELECT 1 FROM app.account WHERE id=p_actor AND state='active') OR m.state NOT IN('ready','active','between_games') OR m.accepted_release_id IS NULL
 OR NOT EXISTS(SELECT 1 FROM app.conversation_member WHERE conversation_id=thread.id AND account_id=p_actor)
 OR EXISTS(SELECT 1 FROM app.match_seat s WHERE s.match_id=m.id AND s.controller_kind='human' AND NOT EXISTS(SELECT 1 FROM app.conversation_member cm WHERE cm.conversation_id=thread.id AND cm.account_id=s.account_id))
 OR EXISTS(SELECT 1 FROM app.conversation_member cm WHERE cm.conversation_id=thread.id AND NOT cm.account_id=ANY(actors))
 OR EXISTS(SELECT 1 FROM unnest(actors) recipient WHERE recipient<>p_actor AND app.contact_blocked(p_actor,recipient)) THEN
 RETURN NULL;END IF;
 SELECT id INTO source FROM app.command_receipt WHERE scope='room_start_v1' AND principal_key='room-start:'||m.room_id::text AND target_ref=m.room_id::text AND state='accepted' AND response_payload->'data'->>'resourceId'=m.id::text;
 IF source IS NULL THEN RETURN NULL;END IF;
 RETURN jsonb_build_object('matchId',m.id,'admissionReceiptId',source,'matchRevision',m.revision::text,'matchState',m.state,'humanAccountIds',to_jsonb(actors));
END $$;

CREATE FUNCTION app.send_game_message(p_actor uuid,p_conversation uuid,p_body text,p_request uuid) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE receipt app.command_receipt; context jsonb; message_id uuid; sent timestamptz;
BEGIN
 IF current_setting('transaction_isolation')<>'read committed' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='game message requires read committed retry';END IF;
 receipt:=app.begin_account_command(p_actor,p_request,'chat',p_conversation::text,jsonb_build_object('action','send_game','body',p_body));
 IF receipt.state<>'pending' THEN RETURN receipt.response_payload;END IF;
 IF p_body IS NULL OR length(p_body) NOT BETWEEN 1 AND 4000 OR p_body !~ '[^[:space:]]' THEN RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('body','message must contain 1 to 4000 characters'));END IF;
 context:=app.lock_game_message_admission(p_actor,p_conversation);
 IF context IS NULL THEN RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('conversation','conversation unavailable'));END IF;
 sent:=clock_timestamp();
 INSERT INTO app.message(conversation_id,sender_id,send_request_ref,body,sent_at) VALUES(p_conversation,p_actor,p_request::text,p_body,sent) RETURNING id INTO message_id;
 UPDATE app.command_receipt SET normalized_source=context||jsonb_build_object('version','game-message-v1','messageId',message_id,'conversationId',p_conversation) WHERE id=receipt.id;
 RETURN app.finish_account_command(receipt.id,true,jsonb_build_object('resourceId',message_id::text));
END $$;

CREATE FUNCTION app.check_game_message_source(p_message uuid) RETURNS void LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path='' AS $$
DECLARE item app.message; thread app.conversation; m app.match; receipt app.command_receipt; source app.command_receipt; actors uuid[];
BEGIN
 SELECT * INTO item FROM app.message WHERE id=p_message;SELECT * INTO thread FROM app.conversation WHERE id=item.conversation_id;
 IF item.id IS NULL OR thread.kind<>'game' THEN RETURN;END IF;
 SELECT * INTO m FROM app.match WHERE id=thread.match_id;
 SELECT * INTO receipt FROM app.command_receipt WHERE principal_key='human:'||item.sender_id::text AND scope='chat' AND request_ref=item.send_request_ref;
 SELECT * INTO source FROM app.command_receipt WHERE id::text=receipt.normalized_source->>'admissionReceiptId';
 SELECT array_agg(account_id ORDER BY account_id) INTO actors FROM app.match_seat WHERE match_id=m.id AND controller_kind='human';
 IF receipt.id IS NULL OR receipt.account_id IS DISTINCT FROM item.sender_id OR receipt.state<>'accepted' OR receipt.target_ref IS DISTINCT FROM thread.id::text
 OR receipt.payload_hash IS DISTINCT FROM app.auth_json_hash(jsonb_build_object('action','send_game','body',item.body))
 OR receipt.response_payload IS DISTINCT FROM jsonb_build_object('status','success','data',jsonb_build_object('resourceId',item.id::text))
 OR receipt.normalized_source->>'matchRevision' IS NULL OR receipt.normalized_source->>'matchRevision' !~ '^(0|[1-9][0-9]{0,77})$' OR (receipt.normalized_source->>'matchRevision')::numeric>m.revision
 OR receipt.normalized_source->>'matchState' IS NULL OR actors IS NULL OR receipt.normalized_source->>'matchState' NOT IN('ready','active','between_games') OR NOT item.sender_id=ANY(actors)
 OR NOT EXISTS(SELECT 1 FROM app.conversation_member WHERE conversation_id=thread.id AND account_id=item.sender_id)
 OR source.id IS NULL OR source.scope<>'room_start_v1' OR source.principal_key IS DISTINCT FROM 'room-start:'||m.room_id::text OR source.target_ref IS DISTINCT FROM m.room_id::text OR source.state<>'accepted' OR source.response_payload->'data'->>'resourceId' IS DISTINCT FROM m.id::text
 OR receipt.normalized_source IS DISTINCT FROM jsonb_build_object('version','game-message-v1','messageId',item.id,'conversationId',thread.id,'matchId',m.id,'admissionReceiptId',source.id,'matchRevision',receipt.normalized_source->>'matchRevision','matchState',receipt.normalized_source->>'matchState','humanAccountIds',to_jsonb(actors)) THEN
 RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='game message requires exact accepted admission and send receipt';END IF;
END $$;
CREATE FUNCTION app.guard_game_message_source() RETURNS trigger LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE current_receipt app.command_receipt;
BEGIN
 IF TG_TABLE_NAME='message' THEN PERFORM app.check_game_message_source(NEW.id);
 ELSIF NEW.scope='chat' AND NEW.normalized_source->>'version'='game-message-v1' THEN
 SELECT * INTO current_receipt FROM app.command_receipt WHERE id=NEW.id;
 IF current_receipt.state<>'accepted' OR NOT EXISTS(SELECT 1 FROM app.message item JOIN app.conversation thread ON thread.id=item.conversation_id WHERE item.id::text=NEW.normalized_source->>'messageId' AND thread.kind='game' AND item.sender_id=NEW.account_id AND item.send_request_ref=NEW.request_ref AND item.conversation_id::text=NEW.target_ref) THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='game send receipt requires its exact persisted message';END IF;
 PERFORM app.check_game_message_source((current_receipt.normalized_source->>'messageId')::uuid);END IF;
 RETURN NULL;
END $$;
CREATE FUNCTION app.guard_game_message_insertion() RETURNS trigger LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
BEGIN
 IF EXISTS(SELECT 1 FROM app.conversation WHERE id=NEW.conversation_id AND kind='game') AND app.lock_game_message_admission(NEW.sender_id,NEW.conversation_id) IS NULL THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='game message sender or current recipient context unavailable';END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER game_message_insertion BEFORE INSERT ON app.message FOR EACH ROW EXECUTE FUNCTION app.guard_game_message_insertion();
CREATE CONSTRAINT TRIGGER game_message_source AFTER INSERT OR UPDATE ON app.message DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION app.guard_game_message_source();
CREATE CONSTRAINT TRIGGER game_message_receipt AFTER INSERT OR UPDATE ON app.command_receipt DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION app.guard_game_message_source();
REVOKE ALL ON FUNCTION app.send_game_message(uuid,uuid,text,uuid),app.check_game_message_source(uuid),app.guard_game_message_source(),app.lock_game_message_admission(uuid,uuid),app.guard_game_message_insertion() FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;
COMMIT;
